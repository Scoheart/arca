//! Platform configuration readers. No Agent identities or capture state.
#[cfg(target_os = "macos")]
use std::{collections::BTreeMap, path::PathBuf};

#[cfg(target_os = "macos")]
fn output(program: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    Command::new(program).args(args).stderr(Stdio::null()).output().ok()
        .filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(target_os = "macos")]
pub fn shell_proxy() -> Result<Option<String>, String> {
    use std::{process::{Command, Stdio}, time::{Duration, Instant}};
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let shell_path = std::path::Path::new(&shell);
    if !shell_path.is_absolute() || !shell_path.file_name().is_some_and(|name| matches!(name.to_str(), Some("zsh" | "bash" | "sh"))) {
        return Err("自动读取终端代理暂支持 zsh/bash/sh；请显式指定网络代理。".into());
    }
    let directory = std::env::temp_dir().join(format!("arca-network-{}", uuid::Uuid::new_v4()));
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(&directory).map_err(|_| "无法创建私有网络查询目录。")?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("route");
    // Startup output is discarded. Only a credential-free proxy authority (or a
    // fixed marker) is written to this private, create-only handoff. Never dump
    // the shell environment or retain proxy credentials from a startup file.
    let script = r#"umask 077; set -C
p=${https_proxy:-${HTTPS_PROXY:-${all_proxy:-${ALL_PROXY:-${http_proxy:-${HTTP_PROXY:-}}}}}}
if [ -z "$p" ]; then printf 'none\n' > "$1"
else
  case "$p" in
    http://*|socks4://*|socks4a://*|socks5://*)
      scheme=${p%%://*}; a=${p#*://}; a=${a%/}
      if printf '%s' "$a" | /usr/bin/grep -Eq '^([A-Za-z0-9._-]+|\[[A-Fa-f0-9:]+\])(:[0-9]+)?$'; then
        printf '%s://%s\n' "$scheme" "$a" > "$1"
      else printf 'unsupported\n' > "$1"; fi ;;
    *) printf 'unsupported\n' > "$1" ;;
  esac
fi"#;
    let mut child = Command::new(shell).args(["-ilc", script, "arca-capture-proxy"]).arg(&path)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .map_err(|_| "无法读取终端代理；请显式指定网络代理。")?;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("终端代理读取失败；请显式指定网络代理。".into()),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(30)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("终端初始化超时；请显式指定网络代理。".into()); },
        }
    }
    let bytes = super::context::read_bounded(&path, 2048)?.ok_or("终端未返回网络配置。")?;
    let value = std::str::from_utf8(&bytes).map_err(|_| "终端代理地址无法解析。")?.trim();
    match value {
        "none" => Ok(None),
        "unsupported" => Err("终端代理地址不支持或包含认证信息；未切换直连。".into()),
        value => super::canonical_proxy_url(value).map(Some),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn shell_proxy() -> Result<Option<String>, String> { Ok(None) }

pub fn system_proxy() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let settings = output("/usr/sbin/scutil", &["--proxy"])
            .ok_or("无法读取系统网络出口，请在网络设置中指定出口后重试。")?;
        let fields: BTreeMap<_, _> = settings.lines().filter_map(|line| line.trim().split_once(" : ")).collect();
        for (scheme, protocol) in [("HTTPS", "http"), ("HTTP", "http"), ("SOCKS", "socks5")] {
            if fields.get(format!("{scheme}Enable").as_str()) == Some(&"1") {
                let host = fields.get(format!("{scheme}Proxy").as_str()).ok_or("系统代理缺少主机地址。")?;
                let port = fields.get(format!("{scheme}Port").as_str()).ok_or("系统代理缺少端口。")?;
                let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
                return Ok(Some(format!("{protocol}://{host}:{port}")));
            }
        }
        if ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable"].iter().any(|key| fields.get(key) == Some(&"1")) {
            return Err("当前网络使用自动代理脚本；请显式指定代理，未切换直连。".into());
        }
    }
    Ok(None)
}

