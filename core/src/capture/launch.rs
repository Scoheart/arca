//! Prepare a fresh, narrowly scoped Agent runtime for traffic capture.
use super::{acquire_lease, certificate_trusted, expand_pids, private_dir, private_write, processes, read_bytes, root, targets, tool, CaptureCaScope, CaptureTarget};
use super::engine::Worker;
use std::{collections::BTreeSet, fs, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};
use uuid::Uuid;

/// Launch a fresh Agent with additional public CA roots, without saving proxy,
/// credential or launcher preferences and without disabling TLS validation.
pub fn launch_agent(agent_id: &str, directory: Option<&str>) -> Result<CaptureTarget, String> {
    launch_with_proxy(agent_id, directory, None)
}

pub(super) fn launch_with_proxy(agent_id: &str, directory: Option<&str>, proxy: Option<&str>) -> Result<CaptureTarget, String> {
    let profile = targets::PROFILES.get(agent_id).ok_or("该 Agent 尚未核验抓包启动方式。")?;
    let info = crate::application::agent_launch::info(agent_id)?;
    let mut target = info.resolved_target.ok_or("请先安装该 Agent 或配置启动方式。")?;
    let ca = dirs::home_dir().ok_or("无法找到用户目录。")?.join(".mitmproxy/mitmproxy-ca-cert.pem");
    if !ca.exists() { initialize_ca(&ca)?; }
    if profile.ca_scope == CaptureCaScope::System && !certificate_trusted(&ca) {
        return Err("capture_system_ca_required: 此桌面客户端需要系统信任抓包 CA，追加进程环境变量不能保证 HTTPS 解密。".into());
    }
    let read_certificates = |path: &Path| -> Result<String, String> {
        let input = String::from_utf8(read_bytes(path, 2 * 1024 * 1024).map_err(|_| "无法安全读取 CA 证书文件。")?)
            .map_err(|_| "CA 证书必须是 PEM 文本。")?;
        let certificates = regex::Regex::new(r"(?s)-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----").unwrap()
            .find_iter(&input).map(|value| value.as_str()).collect::<Vec<_>>().join("\n");
        if certificates.is_empty() { return Err("文件中没有可用的公开 CA 证书。".into()); }
        Ok(certificates)
    };
    let extra = read_certificates(&ca)?;
    private_dir(&root())?;
    let trust = root().join("trust"); private_dir(&trust)?;
    let bundle = trust.join(format!("{}.pem", Uuid::new_v4()));
    let mut pem = String::new();
    if Path::new("/etc/ssl/cert.pem").is_file() { pem.push_str(&read_certificates(Path::new("/etc/ssl/cert.pem"))?); }
    let environment = match &mut target {
        crate::domain::agents::LaunchTarget::App { env, .. } | crate::domain::agents::LaunchTarget::Cli { env, .. } => env,
        _ => return Err("抓包启动只支持已核验的本机应用和 CLI。".into()),
    };
    for name in &profile.ca_environment {
        if let Some(previous) = environment.get(name).cloned().or_else(|| std::env::var(name).ok()) {
            if !previous.is_empty() && Path::new(&previous) != ca {
                pem.push('\n'); pem.push_str(&read_certificates(Path::new(&previous))?);
            }
        }
    }
    pem.push('\n'); pem.push_str(&extra); pem.push('\n');
    private_write(&bundle, pem.as_bytes())?;
    for name in &profile.ca_environment { environment.insert(name.clone(), bundle.to_string_lossy().into_owned()); }
    // The capture session owns its selected egress. An inherited shell proxy
    // can otherwise hand requests to a different process before interception.
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        environment.insert(name.into(), proxy.unwrap_or_default().to_owned());
    }
    for name in ["NO_PROXY", "no_proxy"] { environment.insert(name.into(), if proxy.is_some() { "" } else { "*" }.into()); }
    let chromium = profile.chromium && targets::chromium(&target);
    if let crate::domain::agents::LaunchTarget::App { new_instance, args, .. } = &mut target {
        *new_instance = true;
        if chromium && !args.iter().any(|arg| arg == "--disable-quic") { args.push("--disable-quic".into()); }
    }
    let directory = if matches!(target, crate::domain::agents::LaunchTarget::Cli { .. }) {
        let value = directory.map(str::to_owned).or(info.directory)
            .map(|value| crate::resources::mcp::scanner::expand_tilde(&value))
            .or_else(dirs::home_dir).ok_or("请填写工作目录。")?;
        if !value.is_absolute() || !value.is_dir() { return Err("工作目录不存在。".into()); }
        Some(value)
    } else { None };
    // CLI titles and installation wrappers are not stable process identities.
    // Have our own terminal child report its PID, then exec the exact launcher
    // with the already prepared environment. exec preserves PID and birth time;
    // a shell wrapper that waits for Node remains the root of its descendants.
    let handoff = if let crate::domain::agents::LaunchTarget::Cli { command, args, env } = &target {
        let parent = root().join("launch"); private_dir(&parent)?;
        let directory = parent.join(Uuid::new_v4().to_string()); private_dir(&directory)?;
        let pid_file = directory.join("pid");
        let bin = Path::new(command).parent().ok_or("无效的可执行程序路径。")?;
        let mut wrapped_args = vec!["-c".into(),
            r#"umask 077; set -C; { printf '%s\n' "$$"; /bin/ps -p "$$" -o lstart=; } > "$1" || exit 1; set +C; shift; PATH="$1:$PATH"; export PATH; shift; exec "$@""#.into(),
            "arca-capture-launch".into(), pid_file.to_string_lossy().into_owned(),
            bin.to_string_lossy().into_owned(), command.clone()];
        wrapped_args.extend(args.clone());
        Some((LaunchHandoff(directory), pid_file, crate::domain::agents::LaunchTarget::Cli {
            command: "/bin/sh".into(), args: wrapped_args, env: env.clone(),
        }))
    } else { None };
    let discover = || targets::bind_agent(&processes(), agent_id);
    if let Some((_cleanup, pid_file, wrapped)) = handoff {
        crate::application::agent_launch::dispatch(&wrapped, directory.as_deref())?;
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let identity = read_bytes(&pid_file, 64).ok().and_then(|bytes| {
                if !bytes.ends_with(b"\n") { return None; }
                let text = std::str::from_utf8(&bytes).ok()?;
                let (pid, birth) = text.trim_end().split_once('\n')?;
                let birth = birth.split_whitespace().collect::<Vec<_>>().join(" ");
                if birth.is_empty() { return None; }
                Some((pid.parse::<u32>().ok()?, birth))
            });
            if let Some((pid, birth)) = identity {
                let rows = processes();
                if let Some(row) = rows.iter().find(|row| row.pid == pid && row.started_at == birth) {
                    return Ok(CaptureTarget {
                        id: format!("pid:{}:{}", pid, row.started_at),
                        name: format!("{} · PID {}", info.name, pid),
                        pids: expand_pids(&rows, &[pid]), agent_ids: vec![agent_id.into()],
                    });
                }
                return Err("capture_launch_exited: Agent 在绑定抓包前已退出，请查看终端中的启动错误。".into());
            }
            if Instant::now() >= until {
                return Err("capture_launch_unverified: 终端尚未执行本次启动命令，请检查终端窗口和自动化权限。".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let before = discover().into_iter().map(|target| target.id).collect::<BTreeSet<_>>();
    crate::application::agent_launch::dispatch(&target, directory.as_deref())?;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let mut fresh = discover().into_iter().filter(|target| target.id.starts_with("pid:")
            && target.agent_ids.iter().any(|id| id == agent_id) && !before.contains(&target.id)).collect::<Vec<_>>();
        fresh.sort_by_key(|target| std::cmp::Reverse(target.pids.len()));
        if let Some(root) = fresh.first() {
            // Prefer the new root covering its helpers, never an application
            // group that could include existing user instances.
            if fresh.iter().all(|target| target.pids.iter().all(|pid| root.pids.contains(pid))) { return Ok(root.clone()); }
            return Err("capture_launch_ambiguous: 检测到多个新实例，请刷新后选择要记录的进程。".into());
        }
        if Instant::now() >= until { return Err("capture_launch_unverified: 已发出启动请求，但未检测到独立的新进程；请检查终端或应用窗口后刷新。".into()); }
        std::thread::sleep(Duration::from_millis(200));
    }
}

struct LaunchHandoff(PathBuf);
impl Drop for LaunchHandoff {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
}


pub(super) fn initialize_ca(ca: &Path) -> Result<(), String> {
    let engine = tool().ok_or("未找到 mitmdump，请先安装官方 mitmproxy。")?;
    let lease = acquire_lease(".ca.lock")?;
    if ca.exists() { return Ok(()); }
    let directory = ca.parent().ok_or("CA 目录无效。")?;
    private_dir(directory)?;
    // Certificate generation needs no interception, listening port or system
    // trust changes. Preserve an existing native CA; never replace it.
    let mut command = Command::new(engine);
    command.args(["-q", "--no-server", "--set"]).arg(format!("confdir={}", directory.display()));
    let mut worker = Worker::spawn(command, "certificate".into(), lease)?;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        // Creation is not completion: don't kill the generator between its
        // open() and write(), leaving a partial public certificate behind.
        if read_bytes(ca, 2 * 1024 * 1024).ok().and_then(|bytes| String::from_utf8(bytes).ok())
            .is_some_and(|pem| pem.contains("-----BEGIN CERTIFICATE-----") && pem.contains("-----END CERTIFICATE-----")) { return Ok(()); }
        if !worker.alive()? { return Err(worker.error("capture_ca_init: 抓包证书生成失败，请检查 mitmproxy 安装。")); }
        if Instant::now() >= until { return Err("capture_ca_timeout: 抓包证书未在 10 秒内生成，请检查 mitmproxy 安装。".into()); }
        std::thread::sleep(Duration::from_millis(100));
    }
}
