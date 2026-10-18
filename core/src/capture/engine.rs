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
    reaped: bool,
}
impl Worker {
    pub fn spawn(mut command: Command, session_id: String, lease: fs::File) -> Result<Self, String> {
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)] {
            use std::os::{fd::{AsRawFd, BorrowedFd}, unix::process::CommandExt};
            command.process_group(0);
            let descriptor = lease.as_raw_fd();
            command.env("ARCA_CAPTURE_LEASE_FD", descriptor.to_string());
            // Keep the lease across an Arca crash until its engine exits.
            unsafe { command.pre_exec(move || rustix::io::fcntl_setfd(BorrowedFd::borrow_raw(descriptor), rustix::io::FdFlags::empty()).map_err(Into::into)); }
        }
        let child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => "capture_engine_permission: 抓包引擎无法执行，请检查文件权限。".to_owned(),
            _ => "capture_engine_spawn: 无法启动抓包引擎，请检查 mitmproxy 安装。".to_owned(),
        })?;
        let mut worker = Self { session_id, child, issue: Arc::new(Mutex::new(None)), _lease: lease, reaped: false };
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
        if self.reaped { return Ok(false); }
        #[cfg(any(target_os = "macos", target_os = "linux"))] {
            use rustix::process::{waitid, Pid, WaitId, WaitIdOptions};
            let pid = Pid::from_raw(self.child.id() as i32).ok_or("抓包引擎进程标识无效。")?;
            // Keep an exited leader waitable until its owned group is cleaned.
            // Reaping it here frees its PID and loses safe group ownership.
            return waitid(WaitId::Pid(pid), WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT)
                .map(|status| status.is_none()).map_err(|_| "无法查询抓包引擎进程。".to_owned());
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
        self.terminate()
    }
    pub fn pause_recording(&mut self, directory: &Path) -> Result<(), String> {
        super::request_stop(directory)?;
        let until = Instant::now() + Duration::from_secs(6);
        while self.alive()? {
            if read_json::<CaptureState>(&directory.join("status.json")).is_ok_and(|state| state.state == "stopped") { return Ok(()); }
            if Instant::now() >= until { return Err("capture_stop_pending: 正在停止记录，请稍后刷新。".into()); }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }
    /// A spawned CLI still uses this proxy after recording stops. Keep routing
    /// without retaining traffic until the Agent (or owning Arca) exits.
    pub fn forward_until_exit(mut self) -> Result<(), String> {
        fs2::FileExt::unlock(&self._lease).map_err(|_| "无法释放已停止会话的记录锁。")?;
        std::thread::spawn(move || {
            while self.alive().unwrap_or(false) { std::thread::sleep(Duration::from_secs(2)); }
        });
        Ok(())
    }
    fn terminate(&mut self) -> Result<(), String> {
        if self.reaped { return Ok(()); }
        #[cfg(unix)] {
            // Packaged engines may have a bootloader plus Python child. Both
            // belong to the dedicated group created above, never an Agent's.
            if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                // The unreaped child reserves this PID even after it exits.
                // Its group can still contain a bootloader/helper holding flock.
                match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
                    Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                    Err(_) => return Err("无法清理抓包引擎的辅助进程。".into()),
                }
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                if rustix::process::getpgid(Some(pid)).is_ok_and(|group| group == pid) {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
            }
        }
        if self.alive()? { self.child.kill().map_err(|_| "无法停止抓包引擎。".to_owned())?; }
        self.child.wait().map_err(|_| "无法回收抓包引擎。".to_owned())?;
        self.reaped = true;
        Ok(())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if self.terminate().is_ok() {
            // Inherited descriptors refer to the same flock description.
            // A helper must not keep a completed/failed worker's lease alive.
            let _ = fs2::FileExt::unlock(&self._lease);
        }
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;
    use fs2::FileExt;
    use std::os::unix::process::CommandExt;

    #[test]
    fn exited_leader_does_not_leave_its_helper_or_lease_behind() {
        let home = crate::testenv::TestHome::new("capture-worker-lease");
        let path = home.home.join("engine.lock");
        let lease = fs::OpenOptions::new().create(true).read(true).write(true).open(&path).unwrap();
        lease.try_lock_exclusive().unwrap();
        // TestHome deliberately replaces PATH with an empty fixture directory.
        let mut unrelated = Command::new("/bin/sleep").arg("30").process_group(0).spawn().unwrap();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "/bin/sleep 30 & exit 0"]);
        let mut worker = Worker::spawn(command, "fixture".into(), lease).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.alive().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        let contender = fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
        assert_eq!(contender.try_lock_exclusive().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        drop(worker);
        let acquired = contender.try_lock_exclusive();
        let unrelated_alive = unrelated.try_wait().unwrap().is_none();
        if unrelated_alive { unrelated.kill().unwrap(); }
        unrelated.wait().unwrap();
        assert!(acquired.is_ok(), "helper retained a finished worker's lease");
        assert!(unrelated_alive, "cleanup touched an unrelated process group");
    }
}
