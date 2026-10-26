//! 命令行工具（arca CLI）随桌面 App 分发：CLI 以 Tauri sidecar
//! （`bundle.externalBin`）打进 `Arca.app/Contents/MacOS/arca`，这里负责把它
//! 软链到 `~/.local/bin/arca`（免管理员授权）。软链指向包内 → App 自动更新后
//! CLI 天然同版本（VS Code / Docker Desktop 同款思路的用户目录变体）。

use std::fs;
use std::path::{Path, PathBuf};

use arca_core::application::ArcaCore;
use arca_core::domain::error::{CoreError, CoreResult};
use serde::Serialize;

/// `~/.local/bin/arca` — 免授权的用户级安装位置。
fn link_path() -> Option<PathBuf> {
    dirs_home().map(|h| h.join(".local/bin/arca"))
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// 包内 sidecar 的路径：externalBin 会把 `arca` 放在主二进制旁边
/// （macOS: `Arca.app/Contents/MacOS/arca`）。dev 模式下不存在 → None。
fn bundled_cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let cli = exe.parent()?.join("arca");
    (cli.is_file() && cli != exe).then_some(cli)
}

/// 用户的登录 shell PATH 是否包含 `~/.local/bin`。GUI 进程从 launchd 继承的
/// PATH 不代表终端环境，所以走一次 `$SHELL -lc` 取真实值；失败则回退环境变量。
fn local_bin_in_path() -> bool {
    let Some(home) = dirs_home() else {
        return false;
    };
    let needle = home.join(".local/bin");
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let path = std::process::Command::new(&shell)
        .args(["-lc", "printf %s \"$PATH\""])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    path.split(':').any(|p| Path::new(p) == needle)
}

#[derive(Serialize)]
pub struct CliStatus {
    /// 包里带了 sidecar（dev 模式下 false）。
    pub bundled: bool,
    /// `~/.local/bin/arca` 软链存在且指向本 App 包内的 CLI。
    pub installed: bool,
    /// 软链位置（展示用，`~/.local/bin/arca`）。
    pub link_path: String,
    /// `~/.local/bin` 在用户登录 shell 的 PATH 里。
    pub in_path: bool,
}

/// 当前 CLI 安装状态（供前端决定是否静默安装/提示 PATH）。
#[tauri::command]
pub fn cli_status() -> CliStatus {
    let bundled = bundled_cli();
    let link = link_path();
    let installed = match (&bundled, &link) {
        (Some(cli), Some(l)) => fs::read_link(l).map(|t| &t == cli).unwrap_or(false),
        _ => false,
    };
    CliStatus {
        bundled: bundled.is_some(),
        installed,
        link_path: "~/.local/bin/arca".into(),
        in_path: local_bin_in_path(),
    }
}

/// 把包内 CLI 软链到 `~/.local/bin/arca`。已有软链（包括指向旧位置/别的副本的）
/// 会被替换；是真实文件时拒绝覆盖（那是用户自己装的，比如 cargo install）。
/// 返回安装后的状态。
#[tauri::command]
pub fn install_cli() -> CoreResult<CliStatus> {
    ArcaCore::independent_host_mutation("desktop_cli_install", install_cli_unlocked)
}

fn install_cli_unlocked() -> CoreResult<CliStatus> {
    let cli = bundled_cli()
        .ok_or_else(|| CoreError::new("cli_not_bundled", "此构建未打包命令行工具（dev 模式？）"))?;
    let link =
        link_path().ok_or_else(|| CoreError::new("home_unavailable", "无法定位 HOME 目录"))?;
    if let Some(dir) = link.parent() {
        fs::create_dir_all(dir).map_err(|error| {
            CoreError::new(
                "cli_install_failed",
                format!("创建 {} 失败: {}", dir.display(), error),
            )
        })?;
    }
    match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::remove_file(&link).map_err(|error| {
                CoreError::new("cli_install_failed", format!("移除旧软链失败: {error}"))
            })?;
        }
        Ok(_) => {
            return Err(CoreError::new(
                "cli_install_conflict",
                format!(
                    "{} 已存在且不是软链（可能是手动安装的 arca），不覆盖。可删除后重试。",
                    link.display()
                ),
            ));
        }
        Err(_) => {}
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&cli, &link)
        .map_err(|error| CoreError::new("cli_install_failed", format!("创建软链失败: {error}")))?;
    #[cfg(not(unix))]
    return Err(CoreError::new("unsupported_platform", "仅支持 macOS/Linux"));
    // Preserve an explicitly installed former command only when its symlink
    // points to an app carrying our old/new bundle identity. Other mux tools,
    // real files and unknown broken links remain untouched.
    let legacy = link.with_file_name("mux");
    if let Ok(previous) = fs::read_link(&legacy) {
        if arca_core::brand::is_former_bundled_cli(&previous)
            && fs::read_link(&legacy).ok().as_ref() == Some(&previous) {
            fs::remove_file(&legacy).map_err(|error| CoreError::new("cli_install_failed", error.to_string()))?;
            #[cfg(unix)] std::os::unix::fs::symlink(&cli, &legacy)
                .map_err(|error| CoreError::new("cli_install_failed", error.to_string()))?;
        }
    }
    Ok(cli_status())
}
