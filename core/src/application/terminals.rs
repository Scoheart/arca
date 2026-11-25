//! Global CLI terminal preference and macOS launch adapters.
use crate::settings::{load_settings_strict, mutate_settings_checked};
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, process::{Command, Stdio}};
use super::launch_command::{cli_command_exact, quoted_word};

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

pub(super) fn dispatch(
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    directory: &Path,
    path_prefix: Option<&Path>,
) -> Result<(), String> {
    let selected = settings()?.selected;
    let app = app_path(&selected).ok_or("默认终端不可用，请在设置中重新选择")?;
    if selected == "warp" {
        return dispatch_warp(&app, program, args, env, directory, path_prefix);
    }
    let line = cli_command_exact(program, args, env, directory, path_prefix);
    // Dynamic command text is passed only as argv, never AppleScript source.
    // Each adapter creates a fresh window/session instead of typing into a busy shell.
    let script = match selected.as_str() {
        "terminal" => "on run argv\ntell application id \"com.apple.Terminal\"\nactivate\ndo script (item 1 of argv)\nend tell\nend run",
        "iterm" => "on run argv\ntell application id \"com.googlecode.iterm2\"\nactivate\nset w to (create window with default profile)\ntell current session of w to write text (item 1 of argv)\nend tell\nend run",
        "ghostty" => "on run argv\ntell application id \"com.mitchellh.ghostty\"\nactivate\nset cfg to new surface configuration\nset initial input of cfg to (item 1 of argv) & linefeed\nnew window with configuration cfg\nend tell\nend run",
        _ => return Err("不支持的默认终端，请重新选择".into()),
    };
    let mut command = Command::new("/usr/bin/osascript");
    command.args(["-e", script, "--", &line]);
    super::agent_launch::dispatch_command(command)
}

#[derive(Serialize)]
struct WarpTab<'a> {
    name: &'a str,
    panes: [WarpPane<'a>; 1],
}

#[derive(Serialize)]
struct WarpPane<'a> {
    id: &'a str,
    r#type: &'a str,
    shell: &'a str,
    directory: &'a str,
    commands: [&'a str; 1],
}

fn write_private_launch_file(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)] use std::os::unix::fs::OpenOptionsExt;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(body.as_bytes())
}

fn warp_can_show_env(name: &str, value: &str) -> bool {
    // Warp keeps startup commands in its history. Only display recognized
    // launch settings whose values have a non-credential form.
    if value.chars().any(char::is_control) { return false; }
    match name {
        "HTTP_PROXY" | "HTTPS_PROXY" | "ALL_PROXY" | "http_proxy" | "https_proxy" | "all_proxy" => {
            if value.is_empty() { return true; }
            let Some((scheme, authority)) = value.split_once("://") else { return false; };
            let authority = authority.strip_suffix('/').unwrap_or(authority);
            matches!(scheme, "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h")
                && !authority.contains(['/', '\\', '?', '#', '@', '%'])
                && !authority.chars().any(char::is_whitespace)
                && url::Url::parse(value).is_ok_and(|url| url.has_host())
        }
        "NO_PROXY" | "no_proxy" => value.bytes().all(|byte|
            byte.is_ascii_alphanumeric() || b" .,:-*/[]_".contains(&byte)),
        "PATH" => value.is_empty() || std::env::split_paths(value).all(|path| path.is_absolute()),
        "NODE_EXTRA_CA_CERTS" => value.is_empty() || Path::new(value).is_absolute(),
        "QODER_ENABLE_CODE_MODE" => matches!(value, "0" | "1" | "false" | "true"),
        _ => false,
    }
}

fn dispatch_warp(
    app: &Path,
    program: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    directory: &Path,
    path_prefix: Option<&Path>,
) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("找不到用户目录")?;
    let configs = home.join(".warp/tab_configs");
    fs::create_dir_all(&configs).map_err(|_| "无法创建 Warp Tab Config 目录")?;
    let name = format!("arca_launch_{}", uuid::Uuid::new_v4().simple());
    let config = configs.join(format!("{name}.toml"));
    let staging = configs.join(format!(".{name}.tmp"));
    let environment = configs.join(format!(".{name}.env"));
    let directory_text = directory.to_str().ok_or("Warp 工作目录需要使用 UTF-8 路径")?;

    let clear = || {
        for path in [&staging, &config, &environment] { let _ = fs::remove_file(path); }
    };
    let result = (|| {
        let mut visible_env = BTreeMap::new();
        let mut private_env = Vec::new();
        for (name, value) in env {
            if warp_can_show_env(name, value) {
                visible_env.insert(name.clone(), value.clone());
            } else {
                private_env.push((name, value));
            }
        }
        let path_prefix = path_prefix.filter(|_| !env.contains_key("PATH"));
        let line = cli_command_exact(program, args, &visible_env, directory, path_prefix);
        let line = if private_env.is_empty() {
            line
        } else {
            // Source the settings in a subshell so Warp records the invocation
            // without the values or changes to the interactive shell's environment.
            let path = environment.to_str().ok_or("Warp 启动配置需要使用 UTF-8 路径")?;
            let source = quoted_word(path);
            let mut body = format!("/bin/rm -f -- {source}\n");
            for (name, value) in private_env {
                body.push_str(&format!("export {name}={}\n", quoted_word(value)));
            }
            write_private_launch_file(&environment, &body).map_err(|_| "无法写入 Warp 启动环境")?;
            format!("(\n  . {source} &&\n{line}\n)")
        };
        // Warp expands {{…}} in a tab config. Opening braces in our command
        // occur only in already-quoted words. Quote each separately to keep
        // Warp from expanding them while zsh still receives the original text.
        let line = if line.contains("{{") { line.replace('{', "'{'") } else { line };
        let initial_directory = if directory_text.contains("{{") { "/" } else { directory_text };
        let body = toml::to_string(&WarpTab {
            name: "Arca Agent",
            panes: [WarpPane { id: "agent", r#type: "terminal", shell: "zsh", directory: initial_directory, commands: [&line] }],
        }).map_err(|_| "无法生成 Warp Tab Config")?;
        write_private_launch_file(&staging, &body).map_err(|_| "无法写入 Warp Tab Config")?;
        fs::rename(&staging, &config).map_err(|_| "无法保存 Warp Tab Config")?;

        // Launch Services returns before Warp reads the file. Let a short-lived
        // child clean up after handoff even when the Arca CLI has already exited.
        let mut cleanup = Command::new("/bin/sh");
        #[cfg(unix)] {
            use std::os::unix::process::CommandExt;
            cleanup.process_group(0);
        }
        let mut cleanup = cleanup.args(["-c", "/bin/sleep 60; /bin/rm -f -- \"$1\" \"$2\"", "arca-warp-cleanup"])
            .arg(&config).arg(&environment)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|_| "无法安排清理 Warp 启动配置")?;
        std::thread::spawn(move || { let _ = cleanup.wait(); });

        let mut command = Command::new("/usr/bin/open");
        command.arg("-a").arg(app).arg(format!("warp://tab_config/{name}?new_window=true"));
        super::agent_launch::dispatch_command(command)
    })();
    if result.is_err() { clear(); }
    result
}
