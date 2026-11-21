use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
pub(super) enum Failure {
    NotStarted(&'static str),
    Started(&'static str),
}

impl Failure {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::NotStarted(message) | Self::Started(message) => message,
        }
    }
}

pub(super) struct Diagnostics {
    pub start: &'static str,
    pub rejected: &'static str,
    pub io: &'static str,
    pub timeout: &'static str,
    pub output_limit: &'static str,
}

pub(super) fn executable(command: &str, default: &str) -> Option<PathBuf> {
    if command != default {
        let path = PathBuf::from(command);
        return executable_file(&path).then_some(path);
    }
    let path_env = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_env)
        .chain(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(crate::paths::system_probe_path))
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(default))
        .find(|path| executable_file(path))
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

pub(super) fn scoped_path(path: &Path) -> Option<PathBuf> {
    let path = std::fs::canonicalize(path).ok()?;
    #[cfg(any(test, debug_assertions))]
    {
        if let Some(root) = crate::brand::env_os("ARCA_TEST_PROBE_ROOT") {
            let root = std::fs::canonicalize(root).ok()?;
            return path.starts_with(root).then_some(path);
        }
        #[cfg(test)]
        return None;
    }
    #[cfg(not(test))]
    Some(path)
}

pub(super) fn command(executable: &Path, credential_env: &str) -> Command {
    let mut command = Command::new(executable);
    command.env_clear();
    // Keep runtime, certificate trust, and proxy settings while isolating the
    // selected vault authentication from every other connector.
    for name in [
        "PATH", "HOME", "TMPDIR", "SSL_CERT_FILE", "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS",
        "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy",
    ] {
        if name != credential_env {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
    }
    command
}

#[cfg(unix)]
pub(super) fn run(
    mut command: Command, input: Option<&[u8]>, limit: usize, timeout: Duration, diagnostics: &Diagnostics,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;

    command.process_group(0)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| Failure::NotStarted(diagnostics.start))?;
    drop(command);
    let (status, output) = match capture(&mut child, input.unwrap_or_default(), limit, timeout, diagnostics) {
        Ok(result) => result,
        Err(failure) => {
            // Capture has not reaped the child on this path, so its PID still
            // identifies the process group we created, even if it has exited.
            if let Some(pid) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            let _ = child.kill();
            // Reap asynchronously so an uninterruptible process cannot hold
            // the caller beyond the deadline. Both capture pipes are closed.
            let _ = std::thread::Builder::new().spawn(move || { let _ = child.wait(); });
            return Err(failure);
        }
    };
    if status.success() {
        Ok(output)
    } else {
        Err(Failure::Started(diagnostics.rejected))
    }
}

#[cfg(unix)]
fn capture(
    child: &mut std::process::Child, mut input: &[u8], limit: usize, timeout: Duration, diagnostics: &Diagnostics,
) -> Result<(std::process::ExitStatus, Zeroizing<Vec<u8>>), Failure> {
    use rustix::fs::{fcntl_getfl, fcntl_setfl, OFlags};
    use std::io::{ErrorKind, Read, Write};
    use std::time::Instant;

    let io_error = Failure::Started(diagnostics.io);
    let mut stdout = child.stdout.take().ok_or(io_error)?;
    let mut stdin = child.stdin.take();
    let flags = fcntl_getfl(&stdout).map_err(|_| io_error)?;
    fcntl_setfl(&stdout, flags | OFlags::NONBLOCK).map_err(|_| io_error)?;
    if let Some(writer) = &stdin {
        let flags = fcntl_getfl(writer).map_err(|_| io_error)?;
        fcntl_setfl(writer, flags | OFlags::NONBLOCK).map_err(|_| io_error)?;
    }

    let deadline = Instant::now() + timeout;
    let mut chunk = Zeroizing::new([0u8; 8192]);
    let mut output = Zeroizing::new(Vec::with_capacity(limit));
    let mut eof = false;
    loop {
        if Instant::now() >= deadline {
            return Err(Failure::Started(diagnostics.timeout));
        }
        let mut progress = false;
        if let Some(writer) = &mut stdin {
            if !input.is_empty() {
                match writer.write(input) {
                    Ok(count) if count > 0 => {
                        input = &input[count..];
                        progress = true;
                    }
                    Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                    _ => return Err(io_error),
                }
            }
            if input.is_empty() {
                stdin = None;
            }
        }
        if !eof {
            match stdout.read(&mut *chunk) {
                Ok(0) => eof = true,
                Ok(count) => {
                    if output.len() + count > limit {
                        return Err(Failure::Started(diagnostics.output_limit));
                    }
                    output.extend_from_slice(&chunk[..count]);
                    progress = true;
                }
                Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
                Err(_) => return Err(io_error),
            }
        }
        if eof && stdin.is_none() {
            if let Some(status) = child.try_wait().map_err(|_| io_error)? {
                return Ok((status, output));
            }
        }
        if !progress {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(not(unix))]
pub(super) fn run(
    _command: Command, _input: Option<&[u8]>, _limit: usize, _timeout: Duration,
    _diagnostics: &Diagnostics,
) -> Result<Zeroizing<Vec<u8>>, Failure> {
    Err(Failure::NotStarted("credential_store_unavailable: This CLI credential store requires Unix."))
}
