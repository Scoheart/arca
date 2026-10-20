//! Resolve ordinary capture defaults in Core, shared by Desktop and CLI.
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Serialize)]
pub struct CaptureProxy { pub proxy_url: Option<String>, pub source: String }

pub(super) fn normalize_proxy(value: &str) -> Result<String, String> {
    let url = url::Url::parse(value.trim()).map_err(|_| "请输入完整 HTTP 代理地址。")?;
    if value.len() > 2048 || url.scheme() != "http" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err("抓包上游需要无认证 HTTP 代理，例如 http://127.0.0.1:6789；未回退直连。".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn environment_proxy(values: &BTreeMap<String, String>) -> Option<String> {
    // Match Pi/Undici: lowercase variables override uppercase variables.
    ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY"].into_iter()
        .find_map(|key| values.get(key).filter(|value| !value.trim().is_empty()).cloned())
}

/// Resolve the upstream BEFORE replacing the Agent's proxy with capture's
/// private listener. Only validated, credential-free proxy URLs leave Core.
pub fn capture_proxy(agent_id: &str) -> Result<CaptureProxy, String> {
    let selected = |value: String, source: &str| -> Result<CaptureProxy, String> {
        Ok(CaptureProxy { proxy_url: Some(normalize_proxy(&value)?), source: source.into() })
    };
    if let Some(value) = crate::network::configured_proxy_url()? { return selected(value, "Arca 代理"); }
    let info = crate::application::agent_launch::info(agent_id)?;
    let variables = match info.resolved_target {
        Some(crate::domain::agents::LaunchTarget::Cli { env, .. } | crate::domain::agents::LaunchTarget::App { env, .. }) => env,
        _ => BTreeMap::new(),
    };
    if let Some(value) = environment_proxy(&variables) { return selected(value, "Agent 启动环境"); }
    let inherited = ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY", "http_proxy", "HTTP_PROXY"]
        .into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value))).collect();
    if let Some(value) = environment_proxy(&inherited) { return selected(value, "Arca 进程环境"); }
    if let Some(value) = shell_proxy()? { return selected(value, "终端环境"); }
    if agent_id == "pi" {
        let directory = variables.get("PI_CODING_AGENT_DIR").cloned().or_else(|| std::env::var("PI_CODING_AGENT_DIR").ok())
            .map(|value| crate::resources::mcp::scanner::expand_tilde(&value))
            .or_else(|| dirs::home_dir().map(|home| home.join(".pi/agent"))).ok_or("无法找到 Pi 设置目录。")?;
        if !directory.is_absolute() { return Err("Pi 设置目录必须是绝对路径。".into()); }
        let path = directory.join("settings.json");
        match std::fs::metadata(&path) {
            Ok(_) => {
                let settings: serde_json::Value = serde_json::from_slice(&super::read_bytes(&path, 1024 * 1024)?)
                    .map_err(|_| "Pi 设置无法解析，未切换直连；请检查设置或选择自定义 HTTP 出口。")?;
                match settings.get("httpProxy") {
                    Some(serde_json::Value::String(value)) if !value.trim().is_empty() => return selected(value.clone(), "Pi httpProxy"),
                    Some(serde_json::Value::Null | serde_json::Value::String(_)) | None => {},
                    _ => return Err("Pi httpProxy 必须是代理地址文本。".into()),
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("无法读取 Pi 代理设置，未切换直连。".into()),
        }
    }
    if let Some(value) = system_proxy()? { return selected(value, "macOS 系统代理"); }
    Ok(CaptureProxy { proxy_url: None, source: "未配置上游代理，将直连".into() })
}

#[cfg(target_os = "macos")]
fn shell_proxy() -> Result<Option<String>, String> {
    use std::{process::{Command, Stdio}, time::{Duration, Instant}};
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let shell_path = std::path::Path::new(&shell);
    if !shell_path.is_absolute() || !shell_path.file_name().is_some_and(|name| matches!(name.to_str(), Some("zsh" | "bash" | "sh"))) {
        return Err("自动读取终端代理暂支持 zsh/bash/sh；请在记录选项中指定 HTTP 上游。".into());
    }
    super::private_dir(&super::root())?;
    let directory = super::root().join(format!(".proxy-{}", uuid::Uuid::new_v4()));
    super::private_dir(&directory)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    let _cleanup = Cleanup(directory.clone());
    let path = directory.join("route");
    // Startup output is discarded. Only a credential-free HTTP authority (or a
    // fixed marker) is written to this private, create-only handoff. Never dump
    // the shell environment or retain proxy credentials from a startup file.
    let script = r#"umask 077; set -C
p=${https_proxy:-${HTTPS_PROXY:-${all_proxy:-${ALL_PROXY:-${http_proxy:-${HTTP_PROXY:-}}}}}}
if [ -z "$p" ]; then printf 'none\n' > "$1"
else
  case "$p" in
    http://*)
      a=${p#http://}; a=${a%/}
      if printf '%s' "$a" | /usr/bin/grep -Eq '^([A-Za-z0-9._-]+|\[[A-Fa-f0-9:]+\])(:[0-9]+)?$'; then
        printf 'http://%s\n' "$a" > "$1"
      else printf 'unsupported\n' > "$1"; fi ;;
    *) printf 'unsupported\n' > "$1" ;;
  esac
fi"#;
    let mut child = Command::new(shell).args(["-ilc", script, "arca-capture-proxy"]).arg(&path)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        .map_err(|_| "无法读取终端代理；请在记录选项中指定 HTTP 上游。")?;
    let until = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("终端代理读取失败；请在记录选项中指定 HTTP 上游。".into()),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(30)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("终端初始化超时；请在记录选项中指定 HTTP 上游。".into()); },
        }
    }
    let bytes = super::read_bytes(&path, 2048)?;
    let value = std::str::from_utf8(&bytes).map_err(|_| "终端代理地址无法解析。")?.trim();
    match value {
        "none" => Ok(None),
        "unsupported" => Err("终端代理不是无认证 HTTP 地址；请指定可用的 HTTP 上游，未回退直连。".into()),
        value => normalize_proxy(value).map(Some),
    }
}

#[cfg(not(target_os = "macos"))]
fn shell_proxy() -> Result<Option<String>, String> { Ok(None) }

fn system_proxy() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let settings = super::output("/usr/sbin/scutil", &["--proxy"])
            .ok_or("无法读取系统网络出口，请在抓包设置中选择出口后重试。")?;
        let fields: BTreeMap<_, _> = settings.lines().filter_map(|line| line.trim().split_once(" : ")).collect();
        for scheme in ["HTTPS", "HTTP"] {
            if fields.get(format!("{scheme}Enable").as_str()) == Some(&"1") {
                let host = fields.get(format!("{scheme}Proxy").as_str()).ok_or("系统代理缺少主机地址。")?;
                let port = fields.get(format!("{scheme}Port").as_str()).ok_or("系统代理缺少端口。")?;
                let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
                return Ok(Some(format!("http://{host}:{port}")));
            }
        }
        if ["ProxyAutoConfigEnable", "ProxyAutoDiscoveryEnable", "SOCKSEnable"].iter().any(|key| fields.get(key) == Some(&"1")) {
            return Err("当前网络使用自动代理或 SOCKS，请在抓包设置中选择可用的 HTTP 代理；未切换直连。".into());
        }
    }
    Ok(None)
}

/// Generate only the public certificate entry point. Trust is a separate,
/// explicit action performed by the user in macOS Keychain Access.
pub fn prepare_certificate() -> Result<PathBuf, String> {
    if super::tool().is_none() { super::install_engine()?; }
    let path = dirs::home_dir().ok_or("无法找到用户目录。")?.join(".mitmproxy/mitmproxy-ca-cert.pem");
    super::launch::initialize_ca(&path)?;
    let content = super::read_bytes(&path, 2 * 1024 * 1024)?;
    if !String::from_utf8_lossy(&content).contains("-----BEGIN CERTIFICATE-----") {
        return Err("抓包证书无效，请检查 mitmproxy 的证书文件。".into());
    }
    Ok(path)
}
