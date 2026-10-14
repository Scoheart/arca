//! Local, opt-in Agent traffic capture. The engine is a separately installed mitmproxy.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

mod index;
mod targets;
mod engine;
mod launch;
pub use launch::launch_agent;
use engine::Worker;
pub use targets::{AgentCaptureSupport, CaptureCaScope};

const ADDON: &str = include_str!("addon.py");
const MAX_RECORD_BYTES: u64 = 20 * 1024 * 1024;
static ACTIVE: LazyLock<Mutex<Option<Worker>>> = LazyLock::new(|| Mutex::new(None));
#[derive(Serialize, Deserialize)]
struct RuntimeIdentity { owner_pid: u32, owner_start: String, engine_pid: u32, engine_start: String }

fn process_start(pid: u32) -> Option<String> {
    output("/bin/ps", &["-p", &pid.to_string(), "-o", "lstart="])
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| !value.is_empty())
}
fn runtime_alive(directory: &Path) -> bool {
    read_json::<RuntimeIdentity>(&directory.join("runtime.json")).is_ok_and(|identity|
        process_start(identity.owner_pid).as_deref() == Some(identity.owner_start.as_str())
            && process_start(identity.engine_pid).as_deref() == Some(identity.engine_start.as_str()))
}
fn close_abandoned(flows: &mut [FlowSummary], status: &CaptureState) {
    if matches!(status.state.as_str(), "running" | "starting") { return; }
    for flow in flows {
        if matches!(flow.phase.as_deref(), Some("pending" | "receiving" | "streaming")) {
            flow.phase = Some("cancelled".into());
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureTarget { pub id: String, pub name: String, pub pids: Vec<u32>, pub agent_ids: Vec<String> }
#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureEnvironment {
    pub supported: bool, pub engine: Option<String>, pub extension_enabled: bool,
    pub certificate_present: bool, pub certificate_trusted: bool, pub other_extensions: Vec<String>, pub targets: Vec<CaptureTarget>,
    pub agents: Vec<AgentCaptureSupport>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StartCapture { pub agent_id: String, pub target_id: String, pub egress: String, pub proxy_url: Option<String> }
#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureSession {
    pub id: String, pub agent_id: String, pub agent_name: String, pub target_id: String,
    pub target_name: String, pub pids: Vec<u32>, pub proxy_url: Option<String>, pub started_at: String,
    #[serde(default)] pub loopback_tls: Vec<(String, u16)>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureState { pub state: String, pub message: String, pub pids: Vec<u32>, pub flow_count: usize }
#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureSnapshot { pub session: CaptureSession, pub status: CaptureState, pub flows: Vec<FlowSummary> }
#[derive(Clone, Serialize, Deserialize)]
pub struct CaptureDelta {
    pub session: CaptureSession, pub status: CaptureState, pub revision: String,
    pub reset: bool, pub upserts: Vec<FlowSummary>, pub removed: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowSummary {
    pub id: String, pub method: String, pub url: String, pub status: Option<u16>,
    pub started_at: f64, pub duration_ms: Option<u64>, pub error: Option<String>,
    #[serde(default)] pub phase: Option<String>,
    #[serde(default)] pub updated_at: Option<f64>,
    #[serde(default)] pub request_bytes: Option<u64>,
    #[serde(default)] pub response_bytes: Option<u64>,
    #[serde(default)] pub time_to_first_byte_ms: Option<u64>,
    #[serde(default)] pub content_type: Option<String>,
    #[serde(default)] pub stream_type: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CapturedMessage { pub headers: Vec<(String, String)>, pub body: String, pub bytes: usize, pub truncated: bool, pub encoding: String }
#[derive(Clone, Serialize, Deserialize)]
pub struct CapturedFlow {
    #[serde(flatten)] pub summary: FlowSummary,
    pub request: Option<CapturedMessage>, pub response: Option<CapturedMessage>,
    pub request_http_version: String, pub response_http_version: Option<String>, pub proxy_url: Option<String>,
}

fn root() -> PathBuf { crate::paths::arca_dir().join("captures") }
fn session_dir(id: &str) -> Result<PathBuf, String> {
    Uuid::parse_str(id).map_err(|_| "抓包会话 ID 无效。".to_owned())?;
    let path = root().join(id);
    reject_symlink(&root())?;
    reject_symlink(&path)?;
    Ok(path)
}
fn reject_symlink(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err("抓包文件不能是符号链接。".into());
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<(), String> {
    reject_symlink(path)?;
    fs::create_dir_all(path).map_err(|_| "无法创建抓包目录。".to_owned())?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|_| "无法收紧抓包目录权限。".to_owned())?; }
    Ok(())
}
fn private_write(path: &Path, content: &[u8]) -> Result<(), String> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    options.open(path).and_then(|mut f| f.write_all(content)).map_err(|_| "无法保存抓包文件；目标可能已经存在。".into())
}
fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_bytes(path, MAX_RECORD_BYTES)?).map_err(|_| "抓包记录尚未写完或已经损坏。".into())
}
fn read_bytes(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    reject_symlink(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32);
    }
    let file = options.open(path).map_err(|_| "抓包记录不存在或无法安全读取。".to_owned())?;
    let metadata = file.metadata().map_err(|_| "无法读取抓包记录。".to_owned())?;
    if !metadata.is_file() { return Err("抓包记录必须是普通文件。".into()); }
    if metadata.len() > maximum { return Err("抓包文件超过读取上限。".into()); }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).map_err(|_| "无法读取抓包文件。".to_owned())?;
    if bytes.len() as u64 > maximum { return Err("抓包文件超过读取上限。".into()); }
    Ok(bytes)
}
fn tool() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]);
    if let Some(home) = dirs::home_dir() { dirs.push(home.join(".local/bin")); }
    dirs.into_iter().map(|d| d.join("mitmdump")).find(|p| p.is_file())
}
fn output(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program).args(args).stderr(Stdio::null()).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}
struct Process { pid: u32, parent: u32, command: String, arguments: String, started_at: String }
#[cfg(target_os = "macos")]
fn executable(pid: u32, fallback: &str) -> String {
    #[link(name = "proc")]
    extern "C" { fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32; }
    let mut bytes = [0u8; 4096];
    let size = unsafe { proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if size <= 0 { return fallback.to_owned(); }
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
#[cfg(not(target_os = "macos"))]
fn executable(_pid: u32, fallback: &str) -> String { fallback.to_owned() }
fn processes() -> Vec<Process> {
    // Arguments are used only to bind script runtimes to their exact launcher.
    // They may contain credentials and must never enter capture records/UI.
    let arguments = output("/bin/ps", &["-axo", "pid=,args="]).unwrap_or_default().lines().filter_map(|line| {
        let line = line.trim(); let split = line.find(char::is_whitespace)?;
        Some((line[..split].parse::<u32>().ok()?, line[split..].trim().to_owned()))
    }).collect::<BTreeMap<_, _>>();
    output("/bin/ps", &["-axo", "pid=,ppid=,lstart=,comm="]).unwrap_or_default().lines().filter_map(|line| {
        let mut rest = line.trim();
        let mut fields = Vec::new();
        for _ in 0..7 {
            let split = rest.find(char::is_whitespace)?;
            fields.push(&rest[..split]); rest = rest[split..].trim_start();
        }
        let pid = fields[0].parse().ok()?;
        let command = executable(pid, rest);
        let command = fs::canonicalize(&command).unwrap_or_else(|_| PathBuf::from(command)).to_string_lossy().into_owned();
        Some(Process { pid, parent: fields[1].parse().ok()?, started_at: fields[2..7].join(" "),
            command, arguments: arguments.get(&pid).cloned().unwrap_or_default() })
    }).collect()
}
fn expand_pids(rows: &[Process], initial: &[u32]) -> Vec<u32> {
    let mut pids: BTreeSet<u32> = initial.iter().copied().collect();
    loop { let before = pids.len(); for row in rows { if pids.contains(&row.parent) { pids.insert(row.pid); } } if before == pids.len() { break; } }
    pids.into_iter().collect()
}
fn targets() -> Vec<CaptureTarget> {
    let rows = processes();
    let agents = crate::agents::load_agents();
    let mut groups: BTreeMap<String, CaptureTarget> = BTreeMap::new();
    for row in &rows {
        let (id, name) = if let Some(end) = row.command.find(".app/Contents/") {
            let app = &row.command[..end + 4];
            (format!("app:{app}"), Path::new(app).file_stem().unwrap_or_default().to_string_lossy().into_owned())
        } else {
            let name = Path::new(&row.command).file_name().unwrap_or_default().to_string_lossy().into_owned();
            let known = agents.iter().any(|(_, def)| def.skills.as_ref().is_some_and(|skills| skills.probes.iter().any(|probe| matches!(probe, crate::domain::types::AgentInstallProbe::Command { name: command } if command == &name))));
            if !known || matches!(name.as_str(), "node" | "python" | "python3" | "bash" | "zsh") { continue; }
            (format!("exe:{}", row.command), name)
        };
        if name == "Arca" || name == "MUX" || name == "Mitmproxy Redirector" { continue; }
        let entry = groups.entry(id.clone()).or_insert_with(|| CaptureTarget { id, name, pids: vec![], agent_ids: vec![] });
        entry.pids.push(row.pid);
    }
    for target in groups.values_mut() {
        target.pids = expand_pids(&rows, &target.pids);
        for (id, def) in &agents {
            let name_match = def.name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(&target.name));
            let probe_match = def.skills.as_ref().is_some_and(|skills| skills.probes.iter().any(|probe| match probe {
                crate::domain::types::AgentInstallProbe::Path { path } => target.id == format!("app:{path}"),
                crate::domain::types::AgentInstallProbe::Command { name } => target.name == *name,
                _ => false,
            }));
            if (name_match || probe_match) && !targets::PROFILES.contains_key(id) { target.agent_ids.push(id.clone()); }
        }
    }
    let mut discovered = targets::bind_launchers(&rows);
    for target in groups.into_values() {
        if !target.agent_ids.is_empty() && !discovered.iter().any(|bound| bound.id == target.id) {
            discovered.push(target);
        }
    }
    discovered
}

fn certificate_trusted(ca: &Path) -> bool {
    ca.is_file() && Command::new("/usr/bin/security").args(["verify-cert", "-c"]).arg(ca)
        .stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success())
}

pub fn environment() -> CaptureEnvironment {
    let supported = cfg!(target_os = "macos");
    let extensions = output("/usr/bin/systemextensionsctl", &["list"]).unwrap_or_default();
    let extension_enabled = extensions.lines().any(|l| l.contains("org.mitmproxy.macos-redirector.network-extension") && l.contains("activated enabled"));
    let other_extensions = [("com.interceptsuite.ProxyBridge.extension", "ProxyBridge"), ("com.progress-telerik.fiddler.fiddler-extension", "Fiddler Everywhere")].into_iter().filter(|(id, _)| extensions.lines().any(|line| line.contains(id) && line.contains("activated enabled"))).map(|(_, name)| name.to_owned()).collect();
    let ca = dirs::home_dir().unwrap_or_default().join(".mitmproxy/mitmproxy-ca-cert.pem");
    let certificate_trusted = certificate_trusted(&ca);
    let targets = if supported { targets() } else { vec![] };
    let agents = targets::support(&targets);
    CaptureEnvironment { supported, engine: tool().map(|p| p.to_string_lossy().into_owned()), extension_enabled, certificate_present: ca.is_file(), certificate_trusted, other_extensions, targets, agents }
}


fn proxy(request: &StartCapture) -> Result<Option<String>, String> {
    let value = match request.egress.as_str() { "direct" => return Ok(None), "arca" | "mux" => crate::network::configured_proxy_url()?.ok_or("Arca 尚未配置代理出口。")?, "custom" => request.proxy_url.clone().ok_or("请填写代理地址。")?, _ => return Err("出口类型无效。".into()) };
    let url = url::Url::parse(value.trim()).map_err(|_| "请输入完整 HTTP 代理地址。".to_owned())?;
    if url.scheme() != "http" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() || !matches!(url.path(), "" | "/") {
        return Err("第一版支持无认证 HTTP 上游代理，例如 http://127.0.0.1:6789。不会回退直连。".into());
    }
    // Validate reachability without issuing a target request or exposing a credential.
    use std::net::ToSocketAddrs;
    let host = url.host_str().unwrap().trim_matches(|c| c == '[' || c == ']');
    let addresses = (host, url.port_or_known_default().unwrap_or(80)).to_socket_addrs().map_err(|_| "无法解析代理出口。".to_owned())?;
    if !addresses.take(4).any(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_secs(2)).is_ok()) { return Err("代理出口无法连接；未回退直连。".into()); }
    Ok(Some(value.trim().trim_end_matches('/').to_owned()))
}

pub fn start(request: StartCapture) -> Result<CaptureSnapshot, String> {
    start_session(request, TargetSelection::Existing)
}
/// One core-owned workflow shared by CLI and Desktop. The fresh runtime gets
/// additional CA roots and capture binds to that instance, not a stale PID.
pub fn start_fresh(request: StartCapture, directory: Option<&str>) -> Result<CaptureSnapshot, String> {
    start_session(request, TargetSelection::Fresh { directory })
}
enum TargetSelection<'a> { Existing, Fresh { directory: Option<&'a str> } }
fn start_session(request: StartCapture, selection: TargetSelection<'_>) -> Result<CaptureSnapshot, String> {
    {
        let mut active = ACTIVE.lock().map_err(|_| "抓包会话锁失败。".to_owned())?;
        if let Some(worker) = active.as_mut() { if worker.alive()? { return Err("capture_busy: 请先停止当前抓包会话。".into()); } }
        active.take();
    }
    private_dir(&root())?;
    let lease = acquire_lease(".engine.lock")?;
    let env = environment();
    if !env.supported { return Err("按应用抓包仅支持 macOS。".into()); }
    if !env.extension_enabled { return Err("请在系统设置的登录项与扩展中启用 Mitmproxy Redirector 网络扩展。".into()); }
    let engine = env.engine.ok_or("未找到 mitmdump，请先安装官方 mitmproxy。")?;
    if !env.certificate_trusted && env.agents.iter().any(|agent| agent.agent_id == request.agent_id && agent.ca_scope == CaptureCaScope::System) {
        return Err("capture_system_ca_required: 请先让系统信任抓包 CA，再记录此桌面客户端；否则会中断其 HTTPS 请求。".into());
    }
    let agents = crate::agents::load_agents();
    let agent = agents.get(&request.agent_id).ok_or("请选择有效的 Agent。")?;
    let proxy_url = proxy(&request)?;
    // Only user-configured model endpoints may opt loopback TLS into MITM.
    // Agent-local HTTPS IPC retains its original certificate and routing.
    let settings = crate::settings::load_settings_strict().map_err(|_| "无法读取模型端点，未开始抓包。")?;
    let loopback_tls = settings.model_providers.iter().flat_map(|providers| providers.values()).filter_map(|provider| {
        let address = url::Url::parse(&provider.base_url).ok()?;
        let host = address.host_str()?.trim_matches(['[', ']']);
        let local = host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback());
        (address.scheme() == "https" && local).then(|| (host.to_owned(), address.port_or_known_default().unwrap_or(443)))
    }).collect();
    let target = match selection {
        TargetSelection::Fresh { directory } => launch_agent(&request.agent_id, directory)?,
        TargetSelection::Existing => env.targets.into_iter().find(|t| t.id == request.target_id).ok_or("capture_target_exited: 目标进程已退出，请刷新进程列表。")?,
    };
    if !target.agent_ids.contains(&request.agent_id) { return Err("目标进程不属于所选 Agent，请刷新并重新选择。".into()); }
    let session = CaptureSession { id: Uuid::new_v4().to_string(), agent_id: request.agent_id.clone(), agent_name: agent.name.clone().unwrap_or(request.agent_id.clone()), target_id: target.id, target_name: target.name, pids: target.pids, proxy_url, started_at: chrono::Utc::now().to_rfc3339(), loopback_tls };
    let directory = session_dir(&session.id)?;
    private_dir(&directory)?; private_dir(&directory.join("flows"))?; private_dir(&directory.join("summaries"))?;
    private_write(&directory.join("session.json"), &serde_json::to_vec(&session).map_err(|_| "无法保存会话。".to_owned())?)?;
    private_write(&directory.join("addon.py"), ADDON.as_bytes())?;
    let owner = std::process::id().to_string();
    let owner_start = process_start(std::process::id()).ok_or("无法核验抓包所有者进程。")?;
    let mut command = Command::new(engine);
    command.args(["-q", "--server", "--no-ssl-insecure", "--no-rawtcp", "--no-store-streamed-bodies", "--mode", &format!("local:{}", session.pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")), "--set", "connection_strategy=lazy", "--set", "upstream_cert=false", "-s"]).arg(directory.join("addon.py")).env("ARCA_CAPTURE_DIR", &directory).env("ARCA_CAPTURE_OWNER", owner).env("ARCA_CAPTURE_OWNER_START", &owner_start);
    let worker: Result<Worker, String> = (|| {
        let mut worker = Worker::spawn(command, session.id.clone(), lease)?;
        worker.ready(&directory)?;
        Ok(worker)
    })();
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => { record_failure(&directory, &session, &error)?; return Err(error); }
    };
    *ACTIVE.lock().map_err(|_| "抓包会话锁失败。".to_owned())? = Some(worker);
    snapshot(&session.id)
}

fn acquire_lease(name: &str) -> Result<fs::File, String> {
    private_dir(&root())?;
    let mut options = OpenOptions::new(); options.read(true).write(true).create(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600).custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32); }
    let lease = options.open(root().join(name)).map_err(|_| "无法安全打开抓包锁。")?;
    if !lease.metadata().is_ok_and(|metadata| metadata.is_file()) { return Err("抓包锁必须是普通文件。".into()); }
    fs2::FileExt::try_lock_exclusive(&lease).map_err(|error| if error.kind() == std::io::ErrorKind::WouldBlock {
        "capture_busy: 另一个 Arca 实例正在抓包，请先停止该会话。".to_owned()
    } else { "capture_lock_failed: 无法锁定抓包引擎。".to_owned() })?;
    Ok(lease)
}


fn record_failure(directory: &Path, session: &CaptureSession, error: &str) -> Result<(), String> {
    let state = CaptureState { state: "failed".into(), message: error.into(), pids: session.pids.clone(), flow_count: 0 };
    private_write(&directory.join("failure.json"), &serde_json::to_vec(&state).map_err(|_| "无法保存抓包失败原因。")?)
}
fn request_stop(directory: &Path) -> Result<(), String> {
    let path = directory.join("stop"); reject_symlink(&path)?;
    if path.exists() { if !path.is_file() { return Err("抓包停止标记必须是普通文件。".into()); } return Ok(()); }
    private_write(&path, b"stop")
}

pub fn sessions() -> Result<Vec<CaptureSession>, String> {
    if !root().exists() { return Ok(vec![]); }
    reject_symlink(&root())?;
    let mut result = vec![];
    for entry in fs::read_dir(root()).map_err(|_| "无法读取抓包会话。".to_owned())?.take(1000).flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        if let Ok(directory) = session_dir(&id) { if let Ok(session) = read_json(&directory.join("session.json")) { result.push(session); } }
    }
    result.sort_by(|a: &CaptureSession, b| b.started_at.cmp(&a.started_at));
    Ok(result)
}
fn session_status(id: &str) -> Result<(PathBuf, CaptureSession, CaptureState), String> {
    let directory = session_dir(id)?;
    let session: CaptureSession = read_json(&directory.join("session.json"))?;
    if session.id != id { return Err("抓包会话记录与目录不一致。".into()); }
    let mut status: CaptureState = read_json(&directory.join("failure.json")).or_else(|_| read_json(&directory.join("status.json"))).unwrap_or(CaptureState {
        state: "starting".into(), message: "正在启动抓包引擎…".into(), pids: session.pids.clone(), flow_count: 0,
    });
    let mut active = ACTIVE.lock().map_err(|_| "抓包会话锁失败。".to_owned())?;
    if let Some(worker) = active.as_mut() {
        if !worker.alive()? {
            let worker = active.take().unwrap();
            let ended = session_dir(&worker.session_id)?;
            if !ended.join("failure.json").exists() && read_json::<CaptureState>(&ended.join("status.json")).is_ok_and(|state| state.state == "running") {
                let ended_session = read_json(&ended.join("session.json"))?;
                record_failure(&ended, &ended_session, &worker.error("capture_engine_exit: 抓包引擎意外结束，已保留现有记录。"))?;
                if worker.session_id == id { status = read_json(&ended.join("failure.json"))?; }
            }
        }
    }
    // A CLI-owned live session is also visible to Desktop. Verify both PID
    // and birth time; a persisted running flag alone is never authoritative.
    let live = if let Some(worker) = active.as_mut().filter(|worker| worker.session_id == id) {
        worker.alive()?
    } else { matches!(status.state.as_str(), "running" | "starting") && runtime_alive(&directory) };
    if !live && matches!(status.state.as_str(), "running" | "starting") {
        if status.state == "starting" {
            status.state = "failed".into(); status.message = "抓包引擎启动失败，请检查 mitmproxy 版本与网络扩展。".into();
        } else {
            status.state = "stopped".into(); status.message = "抓包进程已结束；记录仍可查看。".into();
        }
    }
    Ok((directory, session, status))
}

/// Read historical summaries without inspecting or changing capture workers.
pub fn summaries(id: &str) -> Result<Vec<FlowSummary>, String> {
    let directory = session_dir(id)?;
    let session: CaptureSession = read_json(&directory.join("session.json"))?;
    if session.id != id { return Err("抓包会话记录与目录不一致。".into()); }
    Ok(index::query(&directory, None)?.upserts)
}

/// Live view shared by Desktop and CLI, verified against process identities.
pub fn snapshot(id: &str) -> Result<CaptureSnapshot, String> {
    let (directory, session, mut status) = session_status(id)?;
    let mut flows = index::query(&directory, None)?.upserts;
    close_abandoned(&mut flows, &status);
    status.flow_count = flows.len();
    Ok(CaptureSnapshot { session, status, flows })
}

pub fn delta(id: &str, revision: Option<&str>) -> Result<CaptureDelta, String> {
    let (directory, session, mut status) = session_status(id)?;
    // A crash or quota can prevent a final per-flow write. Return all rows
    // when the session ends so every client observes their interrupted state.
    let revision = if matches!(status.state.as_str(), "running" | "starting") { revision } else { None };
    let mut changes = index::query(&directory, revision)?;
    close_abandoned(&mut changes.upserts, &status);
    status.flow_count = changes.count;
    Ok(CaptureDelta { session, status, revision: changes.revision, reset: changes.reset,
        upserts: changes.upserts, removed: changes.removed })
}
pub fn detail(session_id: &str, flow_id: &str) -> Result<CapturedFlow, String> {
    Uuid::parse_str(flow_id).map_err(|_| "请求 ID 无效。".to_owned())?;
    let path = session_dir(session_id)?.join("flows"); reject_symlink(&path)?;
    let mut flow: CapturedFlow = read_json(&path.join(format!("{flow_id}.json")))?;
    let (_, _, status) = session_status(session_id)?;
    if !matches!(status.state.as_str(), "running" | "starting") && matches!(flow.summary.phase.as_deref(), Some("pending" | "receiving" | "streaming")) {
        flow.summary.phase = Some("cancelled".into());
        if let Some(response) = &mut flow.response { response.truncated = true; }
    }
    Ok(flow)
}
pub fn stop(id: &str) -> Result<CaptureSnapshot, String> {
    let directory = session_dir(id)?;
    let worker = {
        let mut active = ACTIVE.lock().map_err(|_| "抓包会话锁失败。".to_owned())?;
        if active.as_ref().is_some_and(|worker| worker.session_id == id) { active.take() } else { None }
    };
    if let Some(mut worker) = worker {
        worker.stop(&directory)?;
    } else {
        if runtime_alive(&directory) {
            request_stop(&directory)?;
            let until = Instant::now() + Duration::from_secs(6);
            while runtime_alive(&directory) {
                if Instant::now() >= until { return Err("capture_stop_pending: 已请求停止，请稍后刷新；没有强制终止其他 Arca 进程。".into()); }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    snapshot(id)
}
pub fn shutdown() { let id = ACTIVE.lock().ok().and_then(|a| a.as_ref().map(|w| w.session_id.clone())); if let Some(id) = id { let _ = stop(&id); } }
pub fn export(session_id: &str, flow_id: Option<&str>, destination: &Path) -> Result<(), String> {
    // Stage beside the destination; publish only after every detail was safely
    // read and serialized. Reuse core's create-only publication primitive so
    // existing files (including a symlink appearing during export) stay intact.
    reject_symlink(destination)?;
    let parent = destination.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temporary = parent.join(format!(".arca-capture-{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new(); options.write(true).create_new(true);
    #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let file = options.open(&temporary).map_err(|_| "无法创建导出文件。".to_owned())?;
    let result = (|| {
        let mut writer = BufWriter::new(&file);
        if let Some(id) = flow_id {
            serde_json::to_writer_pretty(&mut writer, &detail(session_id, id)?)
                .map_err(|_| "无法导出请求。".to_owned())?;
        } else {
            let snapshot = snapshot(session_id)?;
            serde_json::to_writer_pretty(&mut writer, &SessionExport { snapshot: &snapshot })
                .map_err(|_| "无法完整导出会话；请检查记录和所选目录。".to_owned())?;
        }
        writer.flush().and_then(|_| file.sync_all()).map_err(|_| "导出未完成，请检查所选目录。".to_owned())?;
        crate::safe_write::publish_path_noreplace(&temporary, destination)
            .map_err(|_| "无法保存抓包文件；目标可能已经存在。".to_owned())
    })();
    drop(file);
    let _ = fs::remove_file(&temporary);
    result
}

struct SessionExport<'a> { snapshot: &'a CaptureSnapshot }
impl Serialize for SessionExport<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut document = serializer.serialize_struct("CaptureExport", 4)?;
        document.serialize_field("session", &self.snapshot.session)?;
        document.serialize_field("status", &self.snapshot.status)?;
        document.serialize_field("redacted", &true)?;
        document.serialize_field("flows", &ExportFlows { snapshot: self.snapshot })?;
        document.end()
    }
}

struct ExportFlows<'a> { snapshot: &'a CaptureSnapshot }
impl Serialize for ExportFlows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeSeq};
        let mut flows = serializer.serialize_seq(Some(self.snapshot.flows.len()))?;
        for summary in &self.snapshot.flows {
            let flow = detail(&self.snapshot.session.id, &summary.id).map_err(S::Error::custom)?;
            flows.serialize_element(&flow)?;
        }
        flows.end()
    }
}
