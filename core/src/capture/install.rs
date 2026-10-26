//! Pinned official capture runtime, installed only on explicit preparation/start.
//! No Homebrew, Python installation, shell profile edits or administrator rights.
use super::{acquire_lease, private_dir, reject_symlink, root};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::PathBuf, process::{Command, Stdio}, time::Duration};

const VERSION: &str = "12.2.1";
const EXECUTABLE: &str = "mitmproxy.app/Contents/MacOS/mitmdump";
const MAX_DOWNLOAD: usize = 256 * 1024 * 1024;

fn runtime_dir() -> PathBuf { root().join("runtime").join(format!("mitmproxy-{VERSION}")) }

pub(super) fn installed() -> Option<PathBuf> {
    let directory = runtime_dir();
    if [root(), root().join("runtime"), directory.clone()].iter().any(|path| reject_symlink(path).is_err()) { return None; }
    let executable = directory.join(EXECUTABLE);
    (directory.join("verified").is_file() && executable.is_file()).then_some(executable)
}

/// Hashes are pinned to Homebrew's review of the vendor's official tarballs:
/// https://github.com/Homebrew/homebrew-cask/blob/e0021c91dcc56d71c47967ca9429ac9e0b864ab3/Casks/m/mitmproxy.rb
pub fn install_engine() -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") { return Err("当前抓包组件仅支持 macOS。".into()); }
    if let Some(path) = installed() { return Ok(path); }
    let _lease = acquire_lease(".install.lock")?;
    if let Some(path) = installed() { return Ok(path); }
    let (arch, expected) = match std::env::consts::ARCH {
        "aarch64" => ("arm64", "34d2719d91ff1b85ffa64acf20d3a56d6e3a98d308eb45a827ab4266bf68e179"),
        "x86_64" => ("x86_64", "6cb7228747df7209bcd87b791233a7d61ea63de54b257fdab6e3e1f1795c9816"),
        _ => return Err("当前处理器尚未支持自动安装抓包组件。".into()),
    };
    let parent = root().join("runtime");
    private_dir(&parent)?;
    let staging = parent.join(format!(".install-{}", uuid::Uuid::new_v4()));
    private_dir(&staging)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    let _cleanup = Cleanup(staging.clone());
    let url = format!("https://downloads.mitmproxy.org/{VERSION}/mitmproxy-{VERSION}-macos-{arch}.tar.gz");
    let client = crate::network::build_ureq_agent(ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(180)))
        .timeout_connect(Some(Duration::from_secs(10))))?;
    let mut response = client.get(&url).call().map_err(|_| "抓包组件下载失败，请检查 Arca 网络设置后重试。")?;
    let archive_path = staging.join("download.tar.gz");
    let mut archive = fs::OpenOptions::new().write(true).create_new(true).open(&archive_path)
        .map_err(|_| "无法保存抓包组件。")?;
    let mut reader = response.body_mut().as_reader();
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0usize;
    loop {
        let count = reader.read(&mut buffer).map_err(|_| "抓包组件下载中断，请重试。")?;
        if count == 0 { break; }
        total += count;
        if total > MAX_DOWNLOAD { return Err("抓包组件超过预期大小，已取消安装。".into()); }
        digest.update(&buffer[..count]);
        archive.write_all(&buffer[..count]).map_err(|_| "抓包组件写入失败，请检查磁盘空间。")?;
    }
    archive.sync_all().map_err(|_| "无法保存抓包组件。")?;
    drop(archive);
    if hex::encode(digest.finalize()) != expected { return Err("抓包组件完整性校验失败，已取消安装。".into()); }
    let file = fs::File::open(&archive_path).map_err(|_| "无法读取抓包组件。")?;
    tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(&staging)
        .map_err(|_| "抓包组件解压失败，已取消安装。")?;
    let app = staging.join("mitmproxy.app");
    if !staging.join(EXECUTABLE).is_file() || !Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"]).arg(&app)
        .stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success()) {
        return Err("抓包组件签名验证失败，已取消安装。".into());
    }
    fs::remove_file(archive_path).map_err(|_| "无法清理抓包组件下载文件。")?;
    super::private_write(&staging.join("verified"), expected.as_bytes())?;
    let destination = runtime_dir();
    reject_symlink(&destination)?;
    if destination.exists() { return Err("抓包组件目录不完整，请检查本机 runtime 目录后重试。".into()); }
    fs::rename(staging, &destination).map_err(|_| "无法完成抓包组件安装。")?;
    Ok(destination.join(EXECUTABLE))
}
