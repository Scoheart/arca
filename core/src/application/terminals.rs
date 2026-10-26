//! Global CLI terminal preference and macOS launch adapters.
use crate::settings::{load_settings_strict, mutate_settings_checked};
use serde::Serialize;
use std::{fs, path::PathBuf, process::Command};

const TERMINALS: &[(&str, &str, &str)] = &[
    ("terminal", "Terminal", "Terminal"),
    ("iterm", "iTerm2", "iTerm"),
    ("ghostty", "Ghostty", "Ghostty"),
    ("warp", "Warp", "Warp"),
];

#[derive(Serialize)]
pub struct TerminalOption { pub id: String, pub name: String, pub installed: bool }
#[derive(Serialize)]
pub struct TerminalSettings { pub selected: String, pub options: Vec<TerminalOption> }

fn app_path(id: &str) -> Option<PathBuf> {
    let (_, _, name) = TERMINALS.iter().find(|(key, _, _)| *key == id)?;
    let mut roots = vec![crate::paths::system_probe_path("/Applications"), crate::paths::system_probe_path("/System/Applications/Utilities")];
    if let Some(home) = dirs::home_dir() { roots.push(home.join("Applications")); }
    roots.into_iter().map(|root| root.join(format!("{name}.app")))
        .find(|path| path.join("Contents/Info.plist").is_file())
}

pub fn settings() -> Result<TerminalSettings, String> {
    super::gate::read(|| {
        let selected = load_settings_strict().map_err(|e| e.to_string())?.cli_terminal.unwrap_or_else(|| "terminal".into());
        Ok(TerminalSettings { selected, options: TERMINALS.iter().map(|(id, name, _)| TerminalOption {
            id: (*id).into(), name: (*name).into(), installed: cfg!(target_os = "macos") && app_path(id).is_some(),
        }).collect() })
    })
}

pub fn configure(id: &str) -> Result<TerminalSettings, String> {
    if !cfg!(target_os = "macos") { return Err("当前平台暂不支持终端启动".into()); }
    if app_path(id).is_none() { return Err("请选择已安装的终端".into()); }
    super::gate::write_independent(|| mutate_settings_checked(|settings| {
        settings.cli_terminal = Some(id.into()); Ok(())
    }).map_err(|e| e.to_string()))?;
    settings()
}

pub(super) fn dispatch(line: &str) -> Result<(), String> {
    let selected = settings()?.selected;
    let app = app_path(&selected).ok_or("默认终端不可用，请在设置中重新选择")?;
    if selected == "warp" { return dispatch_command_file(&app, line); }
    // Dynamic command text is passed only as argv, never AppleScript source.
    // Each adapter creates a fresh window/session instead of typing into a busy shell.
    let script = match selected.as_str() {
        "terminal" => "on run argv\ntell application id \"com.apple.Terminal\"\nactivate\ndo script (item 1 of argv)\nend tell\nend run",
        "iterm" => "on run argv\ntell application id \"com.googlecode.iterm2\"\nactivate\nset w to (create window with default profile)\ntell current session of w to write text (item 1 of argv)\nend tell\nend run",
        "ghostty" => "on run argv\ntell application id \"com.mitchellh.ghostty\"\nactivate\nset cfg to new surface configuration\nset initial input of cfg to (item 1 of argv) & linefeed\nnew window with configuration cfg\nend tell\nend run",
        _ => return Err("不支持的默认终端，请重新选择".into()),
    };
    let mut command = Command::new("/usr/bin/osascript");
    command.args(["-e", script, "--", line]);
    super::agent_launch::dispatch_command(command)
}

fn dispatch_command_file(app: &std::path::Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    #[cfg(unix)] use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let cache = dirs::cache_dir().ok_or("找不到用户缓存目录")?;
    let directory = cache.join(format!("arca-launch-{}", uuid::Uuid::new_v4()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)] builder.mode(0o700);
    builder.create(&directory).map_err(|_| "无法创建终端启动文件夹")?;
    let file = directory.join("Agent.command");
    let result = (|| {
        let mut options = fs::OpenOptions::new(); options.write(true).create_new(true);
        #[cfg(unix)] options.mode(0o700);
        let mut output = options.open(&file).map_err(|_| "无法创建终端启动文件")?;
        // Warp supports executable .command files. Remove this private handoff
        // before starting the Agent; no credentials are read from the Keychain.
        write!(output, "#!/bin/zsh -l\n/bin/rm -- \"$0\"\n/bin/rmdir -- \"${{0:h}}\"\n{line}\n")
            .map_err(|_| "无法写入终端启动文件")?;
        drop(output);
        let mut command = Command::new("/usr/bin/open");
        command.arg("-a").arg(app).arg(&file);
        super::agent_launch::dispatch_command(command)
    })();
    if result.is_err() { let _ = fs::remove_file(&file); let _ = fs::remove_dir(&directory); }
    result
}
