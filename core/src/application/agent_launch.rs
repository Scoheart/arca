//! Agent runtime discovery and explicit user-initiated launch requests.
use crate::settings::{load_settings_strict, mutate_settings_checked};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::{Path, PathBuf}, process::{Command, Stdio}, sync::{Arc, LazyLock, Mutex}, time::{Duration, Instant, SystemTime}};

pub use crate::domain::agents::{LaunchPreferences, LaunchTarget};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum CatalogTarget {
    App { candidates: Vec<String>, host_name: Option<String>, #[serde(default)] bundle_ids: Vec<String> },
    Cli { candidates: Vec<String>, #[serde(default)] args: Vec<String> },
}

#[derive(Serialize)]
pub struct LaunchInfo {
    pub agent_id: String,
    pub name: String,
    pub category: String,
    pub kind: Option<String>,
    pub available: bool,
    /// An official product runtime was found. A plugin host is not evidence
    /// that its extension is installed; that remains unknown (`None`).
    pub installed: Option<bool>,
    pub supported: bool,
    pub host_name: Option<String>,
    pub install_url: Option<String>,
    pub directory: Option<String>,
    pub default_directory: Option<String>,
    pub directory_exists: bool,
    pub configured_target: Option<LaunchTarget>,
    pub resolved_target: Option<LaunchTarget>,
}

static LAUNCH_CATALOG: LazyLock<BTreeMap<String, CatalogTarget>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/agent-launchers.json"))
        .expect("embedded Agent launcher catalog must be valid")
});

/// Probe an already loaded definition without rereading or cloning the Agent
/// catalog. Batch callers supply their shared query snapshot.
pub(crate) fn official_runtime_detected(
    agent_id: &str,
    category: &str,
    skill_probes: &[crate::domain::types::AgentInstallProbe],
) -> Option<bool> {
    // Opening VS Code/JetBrains does not prove that an extension is installed.
    if matches!(category, "plugin" | "ide-extension") { return None; }
    match LAUNCH_CATALOG.get(agent_id) {
        Some(CatalogTarget::App { candidates, host_name: None, bundle_ids }) => Some(application_matching(candidates, bundle_ids).is_some()),
        Some(CatalogTarget::Cli { candidates, .. }) => Some(candidates.iter().any(|name| find_command(name).is_some())),
        Some(CatalogTarget::App { host_name: Some(_), .. }) => None,
        None if skill_probes.is_empty() => None,
        None => crate::resources::skill::detect_agent_runtime(skill_probes),
    }
}

fn require_agent(id: &str) -> Result<String, String> {
    crate::agents::list_infos().into_iter().find(|agent| agent.id == id)
        .map(|agent| agent.name).ok_or_else(|| "未找到该 Agent".into())
}

fn home() -> Result<PathBuf, String> { dirs::home_dir().ok_or_else(|| "无法找到用户目录".into()) }
fn expand(value: &str) -> PathBuf {
    if value == "~" { return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value)); }
    value.strip_prefix("~/").and_then(|suffix| dirs::home_dir().map(|root| root.join(suffix)))
        .unwrap_or_else(|| PathBuf::from(value))
}
fn app_exists(path: &Path) -> bool {
    path.is_absolute() && path.extension().is_some_and(|ext| ext == "app")
        && path.join("Contents/Info.plist").is_file()
}
fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else { return false; };
    if !metadata.is_file() { return false; }
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; metadata.permissions().mode() & 0o111 != 0 }
    #[cfg(not(unix))] { true }
}
fn application_matching(names: &[String], bundle_ids: &[String]) -> Option<PathBuf> {
    let mut roots = vec![crate::paths::system_probe_path("/Applications")];
    if let Ok(home) = home() { roots.push(home.join("Applications")); }
    roots.push(crate::paths::system_probe_path("/System/Applications"));
    names.iter().flat_map(|name| roots.iter().map(move |root| root.join(format!("{name}.app"))))
        .find(|path| app_exists(path) && (bundle_ids.is_empty() || {
            let plist = path.join("Contents/Info.plist");
            fs::metadata(&plist).is_ok_and(|metadata| metadata.len() <= 1024 * 1024)
                && plist::Value::from_file(plist).ok().and_then(|value| value.as_dictionary().and_then(|fields|
                    fields.get("CFBundleIdentifier").and_then(plist::Value::as_string)).map(str::to_owned))
                    .is_some_and(|id| bundle_ids.contains(&id))
        }))
}

fn find_command(name: &str) -> Option<PathBuf> {
    let direct = expand(name);
    if direct.is_absolute() { return executable(&direct).then_some(direct); }
    if name.is_empty() || name.starts_with('-') || name.contains(['/', '\\']) || name.chars().any(char::is_whitespace) {
        return None;
    }
    let mut paths = std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect::<Vec<_>>()).unwrap_or_default();
    if let Ok(home) = home() {
        paths.extend([".local/bin", ".cargo/bin", ".bun/bin", ".volta/bin", ".npm-global/bin"].map(|p| home.join(p)));
        // GUI launches do not inherit a login shell's Node manager PATH.
        for (root, suffix) in [(".local/share/fnm/node-versions", "installation/bin"),
            ("Library/Application Support/fnm/node-versions", "installation/bin"), (".nvm/versions/node", "bin")] {
            if let Ok(entries) = fs::read_dir(home.join(root)) {
                let mut versions = entries.filter_map(Result::ok).take(128).map(|e| e.path()).collect::<Vec<_>>();
                versions.sort_by_cached_key(|p| p.file_name().unwrap_or_default().to_string_lossy().trim_start_matches('v')
                    .split('.').map(|n| n.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>());
                paths.extend(versions.into_iter().rev().map(|path| path.join(suffix)));
            }
        }
    }
    paths.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(crate::paths::system_probe_path));
    paths.into_iter().filter(|p| p.is_absolute()).map(|p| p.join(name)).find(|p| executable(p))
}

fn validate_environment(env: &BTreeMap<String, String>) -> Result<(), String> {
    if env.len() > 64 { return Err("环境变量最多 64 项".into()); }
    for (name, value) in env {
        let mut bytes = name.bytes();
        let valid_first = bytes.next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == b'_');
        if !valid_first || !bytes.all(|ch| ch.is_ascii_alphanumeric() || ch == b'_') || name.len() > 256 {
            return Err("环境变量名需以字母或下划线开头，且仅包含字母、数字和下划线".into());
        }
        if value.contains('\0') || value.len() > 8192 { return Err("环境变量值无效或过长".into()); }
    }
    Ok(())
}

fn validate_target(target: &LaunchTarget) -> Result<(), String> {
    match target {
        LaunchTarget::App { env, .. } | LaunchTarget::Cli { env, .. } => validate_environment(env)?,
        LaunchTarget::Web { .. } => {},
    }
    let no_nul = |v: &str| !v.is_empty() && !v.contains('\0') && v.len() <= 8192;
    match target {
        LaunchTarget::App { path, args, .. } => {
            if args.len() > 64 || args.iter().any(|arg| arg.contains('\0') || arg.len() > 8192) { return Err("启动参数无效".into()); }
            if !no_nul(path) || !app_exists(&expand(path)) { return Err("请选择已安装的 .app 应用".into()); }
        }
        LaunchTarget::Cli { command, args, .. } => {
            if !no_nul(command) || find_command(command).is_none() { return Err("找不到可执行程序，请选择文件或输入已安装的命令名称".into()); }
            if args.len() > 64 || args.iter().any(|arg| arg.contains('\0') || arg.len() > 8192) { return Err("启动参数无效".into()); }
        }
        LaunchTarget::Web { url } => {
            let parsed = url::Url::parse(url).map_err(|_| "请输入有效的网址")?;
            if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() ||
                !parsed.username().is_empty() || parsed.password().is_some() {
                return Err("仅支持不含登录凭据的 HTTP / HTTPS 网址".into());
            }
        }
    }
    Ok(())
}

fn target_kind(target: &LaunchTarget) -> &'static str {
    match target {
        LaunchTarget::App { .. } => "app",
        LaunchTarget::Cli { .. } => "cli",
        LaunchTarget::Web { .. } => "web",
    }
}

/// Launch mechanism is fixed by the Agent type chosen when it is added.
/// Web is not a product type; legacy web targets are not launched or saved.
fn required_launch_kind(category: &str) -> Option<&'static str> {
    match category {
        "cli" | "coding-agent" | "terminal" => Some("cli"),
        "desktop" | "ide" | "plugin" | "ide-extension" => Some("app"),
        _ => None,
    }
}

fn compatible_target<'a>(category: &str, target: &'a LaunchTarget) -> Option<&'a LaunchTarget> {
    match required_launch_kind(category) {
        Some(kind) if target_kind(target) == kind => Some(target),
        Some(_) | None => None,
    }
}

fn resolve(target: &LaunchTarget) -> Option<LaunchTarget> {
    match target {
        LaunchTarget::App { path, args, new_instance, env } => validate_target(target).is_ok().then(|| LaunchTarget::App { path: expand(path).to_string_lossy().into_owned(), args: args.clone(), new_instance: *new_instance, env: env.clone() }),
        LaunchTarget::Cli { command, args, env } => find_command(command).map(|path| LaunchTarget::Cli { command: path.to_string_lossy().into_owned(), args: args.clone(), env: env.clone() }),
        LaunchTarget::Web { .. } => validate_target(target).is_ok().then(|| target.clone()),
    }
}

pub fn info(agent_id: &str) -> Result<LaunchInfo, String> {
    super::gate::read(|| info_unlocked(agent_id))
}

/// Capture discovery reuses this resolver without nesting workspace gates.
/// This never writes preferences or launches a process.
pub(crate) fn info_unlocked(agent_id: &str) -> Result<LaunchInfo, String> {
        let settings = load_settings_strict().map_err(|e| e.to_string())?;
        let definitions = crate::agents::load_agents_from_settings(&settings);
        let definition = definitions.get(agent_id).ok_or("未找到该 Agent")?;
        let name = definition.name.clone().unwrap_or_else(|| agent_id.into());
        let category = definition.category.clone().unwrap_or_else(|| "cli".into());
        let prefs = settings.agent_launch
            .and_then(|mut values| values.remove(agent_id)).unwrap_or_default();
        let links: BTreeMap<String, serde_json::Value> = serde_json::from_str(include_str!("../../../data/agent-install-links.json")).map_err(|e| e.to_string())?;
        let mut host_name = None;
        let required = required_launch_kind(&category);
        let configured_target = prefs.target.as_ref().and_then(|target| compatible_target(&category, target)).cloned();
        let resolved_target = if let Some(target) = &configured_target {
            resolve(target)
        } else {
            match LAUNCH_CATALOG.get(agent_id) {
                Some(CatalogTarget::App { candidates, host_name: host, bundle_ids }) if required != Some("cli") => {
                    host_name = host.clone();
                    application_matching(candidates, bundle_ids).map(|path| LaunchTarget::App { path: path.to_string_lossy().into_owned(), args: Vec::new(), new_instance: false, env: BTreeMap::new() })
                }
                Some(CatalogTarget::Cli { candidates, args }) if required != Some("app") => {
                    candidates.iter().find_map(|name| find_command(name)).map(|command| LaunchTarget::Cli { command: command.to_string_lossy().into_owned(), args: args.clone(), env: BTreeMap::new() })
                }
                _ => None,
            }
        };
        let kind = required.map(str::to_string).or_else(|| resolved_target.as_ref().map(|target| target_kind(target).into()));
        let supported = cfg!(target_os = "macos");
        let directory = prefs.default_directory.clone().or(prefs.directory);
        let installed = official_runtime_detected(agent_id, &category,
            definition.skills.as_ref().map(|capability| capability.probes.as_slice()).unwrap_or_default());
        Ok(LaunchInfo { agent_id: agent_id.into(), name, category, kind, supported, host_name, installed,
            available: supported && resolved_target.is_some(),
            install_url: links.get(agent_id).and_then(|v| v.get("url")).and_then(|v| v.as_str()).map(str::to_owned),
            directory_exists: directory.as_ref().is_some_and(|p| expand(p).is_dir()),
            directory, default_directory: prefs.default_directory, configured_target, resolved_target,
        })
}

fn version_token(text: &str) -> Option<String> {
    let pattern = regex::Regex::new(r"(?i)(?:^|[^a-z0-9])v?(\d+\.\d+(?:\.\d+){0,3}(?:[-+][a-z0-9.-]+)?)").ok()?;
    pattern.captures(text)?.get(1).map(|value| value.as_str().to_string())
}

fn app_version(path: &str) -> Option<String> {
    let app = Path::new(path);
    if !app_exists(app) { return None; }
    let plist = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let fields = plist.as_dictionary()?;
    ["CFBundleShortVersionString", "CFBundleVersion"].into_iter()
        .filter_map(|key| fields.get(key)?.as_string())
        .find_map(version_token)
}

fn cli_version(path: &str) -> Option<String> {
    let program = Path::new(path);
    if !program.is_absolute() || !executable(program) { return None; }
    let output_path = std::env::temp_dir().join(format!("arca-agent-version-{}", uuid::Uuid::new_v4()));
    let mut output_file = fs::OpenOptions::new();
    output_file.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        output_file.mode(0o600);
    }
    let output_file = output_file.open(&output_path).ok()?;
    let result = (|| {
        let bin = program.parent()?;
        let mut child = Command::new(program).arg("--version")
            .env_clear()
            .env("PATH", format!("{}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin", bin.display()))
            .stdin(Stdio::null()).stdout(Stdio::from(output_file)).stderr(Stdio::null())
            .spawn().ok()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let success = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                _ => { let _ = child.kill(); let _ = child.wait(); break false; }
            }
        };
        if !success { return None; }
        let mut text = String::new();
        fs::File::open(&output_path).ok()?.take(2048).read_to_string(&mut text).ok()?;
        version_token(&text)
    })();
    let _ = fs::remove_file(output_path);
    result
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct VersionIdentity {
    path: PathBuf,
    modified: Option<SystemTime>,
    length: u64,
}

type VersionEntry = Arc<Mutex<Option<(Instant, Option<String>)>>>;
static VERSION_CACHE: LazyLock<Mutex<BTreeMap<VersionIdentity, VersionEntry>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
const VERSION_CACHE_TTL: Duration = Duration::from_secs(30);

fn cached_version(path: &Path, read: impl FnOnce() -> Option<String>) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    let identity = VersionIdentity {
        path: fs::canonicalize(path).ok()?,
        modified: metadata.modified().ok(),
        length: metadata.len(),
    };
    let entry = {
        let mut cache = VERSION_CACHE.lock().unwrap_or_else(|error| error.into_inner());
        // Keep in-flight entries so concurrent requests always share one probe.
        if cache.len() >= 128 {
            cache.retain(|_, entry| Arc::strong_count(entry) > 1);
        }
        cache.entry(identity).or_insert_with(|| Arc::new(Mutex::new(None))).clone()
    };
    let mut cached = entry.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((checked_at, version)) = cached.as_ref() {
        if checked_at.elapsed() < VERSION_CACHE_TTL { return version.clone(); }
    }
    let version = read();
    *cached = Some((Instant::now(), version.clone()));
    version
}

/// Inspect only the resolved official launcher. Custom CLI commands are never
/// executed automatically; a missing version is distinct from not installed.
pub fn runtime_version(agent_id: &str) -> Result<Option<String>, String> {
    let launch = info(agent_id)?;
    if !launch.available { return Ok(None); }
    Ok(match launch.resolved_target.as_ref() {
        Some(LaunchTarget::App { path, .. }) => cached_version(&Path::new(path).join("Contents/Info.plist"), || app_version(path)),
        Some(LaunchTarget::Cli { command, .. }) if launch.configured_target.is_none() => cached_version(Path::new(command), || cli_version(command)),
        _ => None,
    })
}

pub fn configure(agent_id: &str, target: Option<LaunchTarget>) -> Result<LaunchInfo, String> {
    configure_with_directory(agent_id, target, None)
}

pub fn configure_with_directory(agent_id: &str, target: Option<LaunchTarget>, default_directory: Option<String>) -> Result<LaunchInfo, String> {
    let directory = default_directory.map(|value| {
        let value = value.trim();
        if value.is_empty() { return Ok(None); }
        let path = expand(value);
        if !path.is_absolute() || !path.is_dir() { return Err("默认工作目录不存在，请选择有效文件夹".to_string()); }
        Ok(Some(path.to_string_lossy().into_owned()))
    }).transpose()?;
    super::gate::write_independent(|| {
        require_agent(agent_id)?;
        if let Some(target) = &target {
            validate_target(target)?;
            let category = crate::agents::load_agents().get(agent_id).and_then(|agent| agent.category.clone()).unwrap_or_else(|| "cli".into());
            if compatible_target(&category, target).is_none() {
                return Err("启动方式由添加 Agent 时的类型决定".into());
            }
        }
        mutate_settings_checked(|settings| {
            let preferences = settings.agent_launch.get_or_insert_default().entry(agent_id.into()).or_default();
            preferences.target = target;
            if let Some(directory) = directory { preferences.default_directory = directory; }
            Ok(())
        }).map_err(|e| e.to_string())
    })?;
    info(agent_id)
}

fn quote_shell(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

pub(crate) fn dispatch(target: &LaunchTarget, directory: Option<&Path>) -> Result<(), String> {
    #[cfg(any(test, debug_assertions))]
    if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
        return Err("test_launch_blocked: isolated fixtures cannot start host applications".into());
    }
    if !cfg!(target_os = "macos") { return Err("当前平台暂不支持启动 Agent".into()); }
    validate_target(target)?;
    let mut command;
    match target {
        LaunchTarget::App { path, args, new_instance, env } => {
            command = Command::new("/usr/bin/open");
            if *new_instance { command.arg("-n"); }
            command.arg("-a").arg(path);
            for (name, value) in env { command.arg("--env").arg(format!("{name}={value}")); }
            if !args.is_empty() { command.arg("--args").args(args); }
        }
        LaunchTarget::Web { url } => { command = Command::new("/usr/bin/open"); command.arg(url); }
        LaunchTarget::Cli { command: program, args, env } => {
            let directory = directory.ok_or("请先选择工作目录")?;
            let bin = Path::new(program).parent().ok_or("无效的可执行程序路径")?;
            let environment = env.iter().map(|(name, value)| format!("{name}={} ", quote_shell(value))).collect::<String>();
            let line = format!("cd -- {} && PATH={}:\"$PATH\" {}{}{}", quote_shell(&directory.to_string_lossy()),
                quote_shell(&bin.to_string_lossy()), environment, quote_shell(program), args.iter().map(|arg| format!(" {}", quote_shell(arg))).collect::<String>());
            return super::terminals::dispatch(&line);
        }
    }
    let result = command.output().map_err(|_| "无法调用系统启动器")?;
    if result.status.success() { Ok(()) }
    else { Err("系统未能打开 Agent；请检查应用状态或终端自动化权限".into()) }
}

#[derive(Serialize)]
pub struct LaunchReceipt { pub directory_saved: bool }

pub fn launch(agent_id: &str, directory: Option<String>) -> Result<LaunchReceipt, String> {
    let info = info(agent_id)?;
    let target = info.resolved_target.ok_or("未找到可启动的 Agent，请安装或设置启动方式")?;
    let chosen = if matches!(target, LaunchTarget::Cli { .. }) {
        let path = expand(&directory.or(info.directory).ok_or("请先选择工作目录")?);
        if !path.is_absolute() || !path.is_dir() { return Err("工作目录不存在，请重新选择".into()); }
        Some(path)
    } else { None };
    dispatch(&target, chosen.as_deref())?;
    // Failure to remember a preference must not imply that launch failed or
    // invite a second click that would start a duplicate terminal session.
    let directory_saved = chosen.map(|path| super::gate::write_independent(|| {
        mutate_settings_checked(|settings| {
            settings.agent_launch.get_or_insert_default().entry(agent_id.into()).or_default().directory = Some(path.to_string_lossy().into_owned());
            Ok(())
        }).map_err(|e| e.to_string())
    }).is_ok()).unwrap_or(true);
    Ok(LaunchReceipt { directory_saved })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_cache_shares_probes_and_invalidates_changed_files() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let home = crate::testenv::TestHome::new("version-cache");
        let path = home.home.join("version-fixture");
        fs::write(&path, "first").unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let count = count.clone();
                let path = &path;
                scope.spawn(move || {
                    assert_eq!(cached_version(path, || {
                        count.fetch_add(1, Ordering::SeqCst);
                        Some("1.0.0".into())
                    }).as_deref(), Some("1.0.0"));
                });
            }
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
        fs::write(&path, "second version").unwrap();
        assert_eq!(cached_version(&path, || Some("2.0.0".into())).as_deref(), Some("2.0.0"));
    }

    #[test]
    fn plugin_host_does_not_prove_extension_installation() {
        let home = crate::testenv::TestHome::new("plugin-host-runtime");
        let app = home.home.join("Applications/Visual Studio Code.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "fixture").unwrap();
        let launch = info("codex-ide").unwrap();
        assert!(launch.resolved_target.is_some());
        assert_eq!(launch.installed, None);
        assert_eq!(super::super::agents::runtime_detected("codex-ide"), None);
    }

    #[test]
    fn version_discovery_reads_bundle_metadata_and_bounded_cli_output() {
        let home = crate::testenv::TestHome::new("agent-version-discovery");
        let app = home.home.join("Applications/Versioned.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        let mut fields = plist::Dictionary::new();
        fields.insert("CFBundleShortVersionString".into(), plist::Value::String("v2.4.1".into()));
        plist::to_file_xml(app.join("Contents/Info.plist"), &plist::Value::Dictionary(fields)).unwrap();
        assert_eq!(app_version(&app.to_string_lossy()).as_deref(), Some("2.4.1"));
        assert_eq!(version_token("Pi Coding Agent v0.99.0\n").as_deref(), Some("0.99.0"));
        assert!(version_token("version unavailable").is_none());

        let cli = home.home.join("versioned-agent");
        fs::write(&cli, "#!/bin/sh\nprintf 'versioned-agent 3.2.1\\n'\n").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(cli_version(&cli.to_string_lossy()).as_deref(), Some("3.2.1"));
    }

    #[test]
    fn opencode_desktop_discovers_user_app_without_a_cli_install() {
        let home = crate::testenv::TestHome::new("opencode-desktop-launch");
        let app = home.home.join("Applications/OpenCode.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "fixture").unwrap();
        let info = info("opencode-desktop").unwrap();
        assert_eq!(info.category, "desktop");
        assert_eq!(info.kind.as_deref(), Some("app"));
        assert!(matches!(info.resolved_target, Some(LaunchTarget::App { path, .. }) if path == app.to_string_lossy()));
        assert_eq!(info.install_url.as_deref(), Some("https://opencode.ai/download"));
    }

    #[test]
    fn factory_desktop_discovers_user_app_without_droid_cli() {
        let home = crate::testenv::TestHome::new("factory-desktop-launch");
        let app = home.home.join("Applications/Factory.app");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "fixture").unwrap();
        let info = info("factory-desktop").unwrap();
        assert_eq!(info.category, "desktop");
        assert_eq!(info.kind.as_deref(), Some("app"));
        assert!(matches!(info.resolved_target, Some(LaunchTarget::App { path, .. }) if path == app.to_string_lossy()));
        assert_eq!(info.install_url.as_deref(), Some("https://factory.com/product/desktop"));
    }
}
