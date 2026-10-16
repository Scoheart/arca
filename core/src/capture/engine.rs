//! Own only the engine we spawned. Never retain raw engine output: even an
//! exception or connection log may contain a URL, prompt or credential.
use super::{private_write, process_start, read_json, CaptureState, RuntimeIdentity};
use std::{fs, io::Read, path::Path, process::{Child, Command, Stdio}, sync::{Arc, Mutex}, time::{Duration, Instant}};

#[derive(Clone, Copy)]
struct Issue { code: &'static str, message: &'static str }
impl Issue {
    fn classify(bytes: &[u8]) -> Option<Self> {
        let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
        let (code, message) = if text.contains("permission denied") || text.contains("operation not permitted") {
            ("capture_engine_permission", "抓包引擎缺少权限，请检查 mitmproxy 安装及网络扩展权限。")
        } else if text.contains("address already in use") {
            ("capture_engine_conflict", "抓包引擎地址已被占用，请停止另一个 mitmproxy 实例。")
        } else if text.contains("error loading script") || text.contains("traceback (most recent call last)") {
            ("capture_addon_failed", "抓包适配器加载失败，请检查 mitmproxy 版本是否兼容。")
        } else if text.contains("unrecognized arguments") || text.contains("unknown option") || text.contains("invalid proxy mode") {
            ("capture_engine_incompatible", "mitmproxy 不支持所需启动参数，请更新官方 mitmproxy。")
        } else if text.contains("redirector") && (text.contains("failed") || text.contains("unable") || text.contains("not available")) {
            ("capture_redirector_failed", "网络拦截扩展未能连接，请检查 Mitmproxy Redirector 是否正常运行。")
        } else if text.contains("error") && (text.contains("config") || text.contains("yaml")) {
            ("capture_engine_config", "mitmproxy 配置无法加载，请检查其配置文件。")
        } else { return None; };
        Some(Self { code, message })
    }
}

pub(super) struct Worker {
    pub session_id: String,
    child: Child,
    issue: Arc<Mutex<Option<Issue>>>,
    _lease: fs::File,
}
impl Worker {
    pub fn spawn(mut command: Command, session_id: String, lease: fs::File) -> Result<Self, String> {
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)] {
            use std::os::{fd::{AsRawFd, BorrowedFd}, unix::process::CommandExt};
            command.process_group(0);
            let descriptor = lease.as_raw_fd();
            // Keep the lease across an Arca crash until its engine exits.
            unsafe { command.pre_exec(move || rustix::io::fcntl_setfd(BorrowedFd::borrow_raw(descriptor), rustix::io::FdFlags::empty()).map_err(Into::into)); }
        }
        let child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => "capture_engine_permission: 抓包引擎无法执行，请检查文件权限。".to_owned(),
            _ => "capture_engine_spawn: 无法启动抓包引擎，请检查 mitmproxy 安装。".to_owned(),
        })?;
        let mut worker = Self { session_id, child, issue: Arc::new(Mutex::new(None)), _lease: lease };
        if let Some(reader) = worker.child.stdout.take() { worker.observe(reader); }
        if let Some(reader) = worker.child.stderr.take() { worker.observe(reader); }
        Ok(worker)
    }
    fn observe(&self, mut reader: impl Read + Send + 'static) {
        let issue = self.issue.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            let mut window = Vec::with_capacity(8192);
            while let Ok(count) = reader.read(&mut chunk) {
                if count == 0 { break; }
                window.extend_from_slice(&chunk[..count]);
                if let Some(found) = Issue::classify(&window) {
                    if let Ok(mut value) = issue.lock() { *value = Some(found); }
                }
                // Drain continuously with a fixed budget. Nothing raw escapes
                // this observer, including text without a newline.
                if window.len() > 4096 { window.drain(..window.len() - 4096); }
            }
        });
    }
    pub fn alive(&mut self) -> Result<bool, String> {
        self.child.try_wait().map(|status| status.is_none()).map_err(|_| "无法查询抓包引擎进程。".to_owned())
    }
    pub fn error(&self, fallback: &str) -> String {
        self.issue.lock().ok().and_then(|value| *value)
            .map(|issue| format!("{}: {}", issue.code, issue.message)).unwrap_or_else(|| fallback.to_owned())
    }
    pub fn ready(&mut self, directory: &Path, awaiting_extension_approval: bool) -> Result<(), String> {
        let identity = RuntimeIdentity {
            owner_pid: std::process::id(), owner_start: process_start(std::process::id()).ok_or("无法核验抓包所有者进程。")?,
            engine_pid: self.child.id(), engine_start: process_start(self.child.id()).ok_or("capture_engine_exit: 引擎在初始化时退出。")?,
        };
        private_write(&directory.join("runtime.json"), &serde_json::to_vec(&identity).map_err(|_| "无法保存抓包进程标识。")?)?;
        // First launch may need a human to approve the official system
        // extension. Keep ordinary engine startup bounded to twelve seconds.
        let until = Instant::now() + Duration::from_secs(if awaiting_extension_approval { 180 } else { 12 });
        loop {
            if !self.alive()? {
                // Let the pipe observers consume the last diagnostic chunk.
                std::thread::sleep(Duration::from_millis(30));
                return Err(self.error(if awaiting_extension_approval {
                    "capture_extension_setup_failed: 官方网络扩展初始化未完成。请检查 macOS 的安装或授权提示，完成后重试。"
                } else { "capture_engine_exit: 抓包引擎在就绪前退出，请检查 mitmproxy 安装与网络扩展。" }));
            }
            if let Ok(status) = read_json::<CaptureState>(&directory.join("status.json")) {
                if status.state == "running" { return Ok(()); }
                if status.state == "failed" { return Err(status.message); }
            }
            if Instant::now() >= until {
                return Err(self.error(if awaiting_extension_approval {
                    "capture_extension_setup_pending: 未在 3 分钟内完成网络扩展初始化，本次记录已停止。请完成 macOS 的安装与授权后重试。"
                } else { "capture_engine_timeout: 抓包引擎未在 12 秒内就绪，本次记录已停止。" }));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    pub fn stop(&mut self, directory: &Path) -> Result<(), String> {
        super::request_stop(directory)?;
        let until = Instant::now() + Duration::from_secs(4);
        while self.alive()? {
            if Instant::now() >= until { return self.terminate(); }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
    fn terminate(&mut self) -> Result<(), String> {
        if !self.alive()? { return Ok(()); }
        #[cfg(unix)] {
            // Packaged engines may have a bootloader plus Python child. Both
            // belong to the dedicated group created above, never an Agent's.
            if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
                if rustix::process::getpgid(Some(pid)).is_ok_and(|group| group == pid) {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
            }
        }
        if self.alive()? { self.child.kill().map_err(|_| "无法停止抓包引擎。".to_owned())?; }
        self.child.wait().map(|_| ()).map_err(|_| "无法回收抓包引擎。".to_owned())
    }
}
impl Drop for Worker {
    fn drop(&mut self) { let _ = self.terminate(); }
}
