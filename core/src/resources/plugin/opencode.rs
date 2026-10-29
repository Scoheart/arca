//! Inert OpenCode declarations and audited embedded provider integrations.
//! This reader never imports JS/TS, resolves npm over the network or reads auth.
use super::*;
use std::io::{Seek, SeekFrom};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact { agent_id: String, version: String, magic_hex: String, module_offset: u64, module_length: usize,
    module_sha256: String, version_offset: u64, version_marker: String, providers: Vec<Provider> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provider { id: String, name: String }
fn artifacts() -> &'static [Artifact] {
    static VALUE: OnceLock<Vec<Artifact>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-artifacts.json")).expect("audited artifact descriptors must be valid"))
}

fn xdg(variable: &str, fallback: &str, leaf: &str) -> PathBuf {
    std::env::var_os(variable).filter(|value| !value.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| expand_tilde(fallback)).join(leaf)
}
fn config_root() -> PathBuf { xdg("XDG_CONFIG_HOME", "~/.config", "opencode") }
fn meta_path() -> PathBuf {
    std::env::var_os("OPENCODE_PLUGIN_META_FILE").filter(|value| !value.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| xdg("XDG_STATE_HOME", "~/.local/state", "opencode/plugin-meta.json"))
}
fn extra_directory() -> Option<PathBuf> { std::env::var_os("OPENCODE_CONFIG_DIR").filter(|value| !value.is_empty()).map(PathBuf::from) }
fn extra_config() -> Option<PathBuf> { std::env::var_os("OPENCODE_CONFIG").filter(|value| !value.is_empty()).map(PathBuf::from) }
fn document(path: &Path, jsonc: bool, budget: &mut ScanBudget) -> Result<Option<Arc<Value>>, ()> {
    checked_root(path)?;
    optional_document(path.parent().ok_or(())?, path.file_name().and_then(|name| name.to_str()).ok_or(())?,
        if jsonc { DocumentFormat::Jsonc } else { DocumentFormat::Json }, budget)
}

fn slice(file: &mut fs::File, offset: u64, length: usize, budget: &mut ScanBudget) -> Result<Vec<u8>, ()> {
    budget.visit()?;
    if length > 131_072 || length > MAX_READ_BYTES.saturating_sub(budget.bytes) { return Err(()); }
    let metadata = file.metadata().map_err(|_| ())?;
    if !metadata.is_file() || offset.checked_add(length as u64).is_none_or(|end| end > metadata.len()) { return Err(()); }
    file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
    let mut bytes = Vec::new(); let read = (&mut *file).take(length as u64).read_to_end(&mut bytes);
    budget.bytes += bytes.len(); read.map_err(|_| ())?;
    if bytes.len() != length { return Err(()); }
    Ok(bytes)
}

fn bundled(host: &Host, source: &Root, artifact: &Artifact, result: &mut PluginScan) -> Result<(), ()> {
    let path = expand_tilde(&source.path); checked_root(&path)?;
    if !path.try_exists().map_err(|_| ())? { return Ok(()); }
    let mut file = open_confined_file(&path)?;
    let before = file.metadata().map_err(|_| ())?;
    if !before.is_file() { return Err(()); }
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; if before.permissions().mode() & 0o111 == 0 { return Err(()); } }
    if hex::encode(slice(&mut file, 0, 4, &mut result.budget)?) != artifact.magic_hex { return Err(()); }
    let module = slice(&mut file, artifact.module_offset, artifact.module_length, &mut result.budget)?;
    if hex::encode(Sha256::digest(&module)) != artifact.module_sha256 { return Err(()); }
    if slice(&mut file, artifact.version_offset, artifact.version_marker.len(), &mut result.budget)? != artifact.version_marker.as_bytes() { return Err(()); }
    let after = file.metadata().map_err(|_| ())?; let bound = fs::symlink_metadata(&path).map_err(|_| ())?;
    if !bound.file_type().is_file() || before.len() != after.len() || before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != bound.dev() || before.ino() != bound.ino() || before.ctime() != after.ctime() || before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
    }
    for provider in &artifact.providers {
        result.plugins.push(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: format!("builtin:{}", provider.id), name: provider.name.clone(),
            version: artifact.version.clone(), revision: artifact.module_sha256.clone(), presence: PluginPresence::Bundled, enabled: None,
            source_paths: vec![collapse_home(&path)], components: vec![PluginComponent { kind: PluginComponentKind::AuthProvider,
                name: provider.id.clone(), source_path: collapse_home(&path), native_id: None }], docs_url: host.docs_url.clone(), source_fingerprint: String::new() });
    }
    Ok(())
}

enum Spec { Npm { name: String }, File(PathBuf), Unresolved }
struct Candidate { identity: String, native_spec: String, name: String, spec: Spec, source: PathBuf }
const INDEX_FILES: [&str; 5] = ["index.ts", "index.tsx", "index.js", "index.mjs", "index.cjs"];

fn local_path(input: &str, directory: &Path) -> Result<PathBuf, ()> {
    let path = if input.starts_with("file://") {
        let url = url::Url::parse(input).map_err(|_| ())?;
        if !url.username().is_empty() || url.password().is_some() { return Err(()); }
        url.to_file_path().map_err(|_| ())?
    } else if Path::new(input).is_absolute() { PathBuf::from(input) }
    else { directory.join(input) };
    // Native paths may contain './'; no traversal or symlink resolution is needed.
    let path: PathBuf = path.components().collect(); checked_root(&path)?;
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) && !path.join("package.json").try_exists().map_err(|_| ())? {
        for name in INDEX_FILES { let file = path.join(name); if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file()) { return Ok(file); } }
    }
    Ok(path)
}

fn candidate(input: &str, source: &Path, ordinal: usize) -> Result<Candidate, ()> {
    if input.len() > 4096 || input.is_empty() || input.chars().any(char::is_control) { return Err(()); }
    if input.starts_with("file://") || input.starts_with('.') || Path::new(input).is_absolute() {
        let path = local_path(input, source.parent().ok_or(())?)?;
        let native_spec = url::Url::from_file_path(&path).map_err(|_| ())?.to_string();
        let name = path.file_name().and_then(|name| name.to_str()).and_then(metadata_name).ok_or(())?;
        return Ok(Candidate { identity: native_spec.clone(), native_spec, name, spec: Spec::File(path), source: source.to_path_buf() });
    }
    static NPM: OnceLock<regex::Regex> = OnceLock::new();
    let npm = NPM.get_or_init(|| regex::Regex::new(r"^(?:npm:)?(?P<name>(?:@[A-Za-z0-9_.-]+/)?[A-Za-z0-9_.-]+)(?:@[A-Za-z0-9_.~^*+<>=| -]+)?$").unwrap());
    if let Some(capture) = npm.captures(input) {
        let name = metadata_name(&capture["name"]).ok_or(())?;
        return Ok(Candidate { identity: format!("npm:{name}"), native_spec: input.into(), name: name.clone(), spec: Spec::Npm { name }, source: source.to_path_buf() });
    }
    // URL/userinfo, dynamic substitutions and unhandled npm specifier dialects
    // never reach display fields. Keep their declaration/provenance observable.
    let identity = format!("unresolved:{}", hex::encode(Sha256::digest(format!("{}:{ordinal}", source.to_string_lossy()).as_bytes())));
    Ok(Candidate { identity, native_spec: String::new(), name: "Unresolved plugin declaration".into(), spec: Spec::Unresolved, source: source.to_path_buf() })
}

fn declarations(path: &Path, budget: &mut ScanBudget) -> Result<Option<Vec<Candidate>>, ()> {
    checked_root(path)?;
    let value = if path.file_name().is_some_and(|name| name == "config") {
        optional_document(path.parent().ok_or(())?, "config", DocumentFormat::Toml, budget)?
    } else { document(path, true, budget)? };
    let Some(value) = value else { return Ok(None); };
    if !value.is_object() { return Err(()); }
    let Some(plugins) = value.get("plugin") else { return Ok(None); };
    let plugins = plugins.as_array().filter(|values| values.len() <= MAX_ENTRIES).ok_or(())?;
    plugins.iter().enumerate().map(|(ordinal, value)| {
        let input = match value {
            Value::String(value) => value.as_str(),
            Value::Array(values) if values.len() == 2 && values[1].is_object() => values[0].as_str().ok_or(())?,
            _ => return Err(()),
        };
        candidate(input, path, ordinal)
    }).collect::<Result<Vec<_>, _>>().map(Some)
}

fn selected(global: &Path, extra: Option<&Path>, custom: Option<&Path>, result: &mut PluginScan) -> Vec<Candidate> {
    let mut current = Vec::new();
    // Native deep-merge replaces arrays across global files. Its legacy TOML
    // import is read inertly here; no conversion, rename or deletion occurs.
    for name in ["config.json", "opencode.json", "opencode.jsonc", "config"] {
        match declarations(&global.join(name), &mut result.budget) {
            Ok(Some(values)) => current = values, Ok(None) => {},
            Err(()) => { result.unavailable = true; if name != "config" { current.clear(); break; } },
        }
    }
    if let Some(path) = custom {
        match declarations(path, &mut result.budget) { Ok(Some(values)) => current.extend(values), Ok(None) => {}, Err(()) => result.unavailable = true }
    }
    let directories: Vec<_> = std::iter::once(global).chain(extra.filter(|path| *path != global)).collect();
    for directory in directories {
        if extra == Some(directory) {
            for name in ["opencode.json", "opencode.jsonc"] {
                match declarations(&directory.join(name), &mut result.budget) { Ok(Some(values)) => current.extend(values), Ok(None) => {}, Err(()) => result.unavailable = true }
            }
        }
        for folder in ["plugin", "plugins"] {
            match directory_entries(&directory.join(folder), &mut result.budget) {
                Ok(entries) => for entry in entries {
                    let extension = entry.path.extension().and_then(|value| value.to_str());
                    if !matches!(extension, Some("ts" | "js")) { continue; }
                    if !entry.file { result.unavailable = true; continue; }
                    let input = entry.path.to_string_lossy();
                    match candidate(&input, &entry.path, 0) { Ok(value) => current.push(value), Err(()) => result.unavailable = true }
                }, Err(()) => result.unavailable = true,
            }
        }
    }
    let mut seen = BTreeSet::new();
    current.reverse(); current.retain(|candidate| seen.insert(candidate.identity.clone())); current.reverse(); current
}

fn add(kind: PluginComponentKind, name: &str, path: &Path, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS { return Err(()); }
    components.push(PluginComponent { kind, name: metadata_name(name).ok_or(())?, source_path: collapse_home(path), native_id: None }); Ok(())
}
fn exported(value: &Value) -> Option<&str> {
    value.as_str().or_else(|| value.get("import").and_then(Value::as_str)).or_else(|| value.get("default").and_then(Value::as_str))
}

fn metadata_index(value: &Value) -> Result<BTreeMap<&str, Option<&Value>>, ()> {
    let values = value.as_object().filter(|values| values.len() <= MAX_ENTRIES).ok_or(())?;
    let mut index = BTreeMap::new();
    for row in values.values().filter(|row| row.get("source").and_then(Value::as_str) == Some("npm")) {
        let spec = row.get("spec").and_then(Value::as_str).filter(|spec| spec.len() <= 4096).ok_or(())?;
        match index.entry(spec) {
            std::collections::btree_map::Entry::Vacant(entry) => { entry.insert(Some(row)); },
            std::collections::btree_map::Entry::Occupied(mut entry) => { entry.insert(None); },
        }
    }
    Ok(index)
}

fn cached_target(candidate: &Candidate, metadata: &BTreeMap<&str, Option<&Value>>) -> Result<Option<PathBuf>, ()> {
    let Some(record) = metadata.get(candidate.native_spec.as_str()) else { return Ok(None); };
    let record = record.ok_or(())?;
    let target = record.get("target").and_then(Value::as_str).ok_or(())?;
    if !target.starts_with("file://") && !Path::new(target).is_absolute() { return Err(()); }
    local_path(target, Path::new("/")).map(Some)
}
fn package_metadata(root: &Path, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<Option<(String, String, PathBuf)>, ()> {
    let Some(manifest) = optional_json(root, "package.json", budget)? else { return Ok(None); };
    let name = manifest.get("name").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "Plugin package".into());
    let version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "unknown".into());
    for kind in ["server", "tui"] {
        let input = manifest.get("exports").and_then(|value| value.get(format!("./{kind}"))).and_then(exported)
            .or_else(|| if kind == "server" { manifest.get("main").and_then(Value::as_str) } else { None });
        if let Some(input) = input {
            let path = relative(root, input)?; checked_root(&path)?;
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) { return Err(()); }
            add(PluginComponentKind::Code, &format!("{kind} entry"), &path, components)?;
        }
    }
    if let Some(themes) = manifest.get("oc-themes") {
        let themes = themes.as_array().filter(|values| values.len() <= MAX_COMPONENTS).ok_or(())?;
        for value in themes {
            let input = value.as_str().ok_or(())?; let path = relative(root, input)?; checked_root(&path)?;
            if !fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file()) { return Err(()); }
            add(PluginComponentKind::Theme, input, &path, components)?;
        }
    }
    Ok(Some((name, version, root.join("package.json"))))
}

fn observe_candidate(host: &Host, candidate: Candidate, metadata: &BTreeMap<&str, Option<&Value>>, metadata_path: &Path, result: &mut PluginScan) -> Result<NativePluginObservation, ()> {
    let mut components = Vec::new(); let mut paths = vec![collapse_home(&candidate.source)];
    let mut presence = PluginPresence::Declared; let mut name = candidate.name.clone(); let mut version = "unknown".to_string();
    match &candidate.spec {
        Spec::Unresolved => result.unavailable = true,
        Spec::File(path) => {
            checked_root(path)?;
            match fs::symlink_metadata(path) {
                Ok(data) if data.file_type().is_file() => {
                    presence = PluginPresence::Local; add(PluginComponentKind::Code, &candidate.name, path, &mut components)?;
                    match package_metadata(path.parent().ok_or(())?, &mut components, &mut result.budget) {
                        Ok(Some((_, value, source))) => { version = value; paths.push(collapse_home(&source)); },
                        Ok(None) => {}, Err(()) => result.unavailable = true,
                    }
                },
                Ok(data) if data.file_type().is_dir() => {
                    presence = PluginPresence::Local;
                    match package_metadata(path, &mut components, &mut result.budget) {
                        Ok(Some((label, value, source))) => { name = label; version = value; paths.push(collapse_home(&source)); },
                        _ => result.unavailable = true,
                    }
                    paths.push(collapse_home(path));
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => result.unavailable = true,
                _ => return Err(()),
            }
        },
        Spec::Npm { name: expected } => {
            let target = cached_target(&candidate, metadata);
            if target.is_err() { result.unavailable = true; }
            if let Ok(Some(path)) = target {
                if path.is_absolute() && path.try_exists().map_err(|_| ())? {
                    let root = if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) { path.clone() } else { path.parent().ok_or(())?.to_path_buf() };
                    match package_metadata(&root, &mut components, &mut result.budget) {
                        Ok(Some(info)) if &info.0 == expected => {
                            name = info.0; version = info.1; presence = PluginPresence::CacheOnly;
                            paths.push(collapse_home(&info.2)); paths.push(collapse_home(metadata_path));
                        },
                        _ => { components.clear(); result.unavailable = true; },
                    }
                }
            }
        },
    }
    paths.sort(); paths.dedup(); components.sort(); components.dedup();
    Ok(NativePluginObservation { agent_id: host.agent_id.clone(), plugin_id: candidate.identity, name, version,
        revision: match candidate.spec { Spec::Npm { .. } => candidate.native_spec, _ => "file declaration".into() },
        presence, enabled: None, source_paths: paths, components, docs_url: host.docs_url.clone(), source_fingerprint: String::new() })
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let global = config_root(); let extra = extra_directory(); let custom = extra_config(); let metadata_path = meta_path();
    let metadata = match document(&metadata_path, false, &mut result.budget) {
        Ok(value) => { if value.as_ref().is_some_and(|value| !value.is_object()) { result.unavailable = true; } value },
        Err(()) => { result.unavailable = true; None },
    };
    let metadata = match metadata.as_deref().map(metadata_index).transpose() {
        Ok(index) => index.unwrap_or_default(), Err(()) => { result.unavailable = true; BTreeMap::new() },
    };
    for candidate in selected(&global, extra.as_deref(), custom.as_deref(), result) {
        match observe_candidate(host, candidate, &metadata, &metadata_path, result) { Ok(plugin) => result.plugins.push(plugin), Err(()) => result.unavailable = true }
    }
    if std::env::var_os("OPENCODE_CONFIG_CONTENT").is_some_and(|value| !value.is_empty()) { result.unavailable = true; }
    for source in &host.roots {
        if source.layout != "opencode_bundled" { continue; }
        for artifact in artifacts().iter().filter(|artifact| artifact.agent_id == host.agent_id) {
            if bundled(host, source, artifact, result).is_err() { result.unavailable = true; }
        }
    }
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths = BTreeSet::from([config_root(), meta_path()]);
    if let Some(path) = extra_config() { paths.insert(path); } if let Some(path) = extra_directory() { paths.insert(path); }
    for source in &host.roots { if source.layout == "opencode_bundled" { paths.insert(expand_tilde(&source.path)); } }
    let mut scan = PluginScan::default(); let extra = extra_directory(); let custom = extra_config();
    let metadata = document(&meta_path(), false, &mut scan.budget).ok().flatten();
    let metadata = metadata.as_deref().and_then(|value| metadata_index(value).ok()).unwrap_or_default();
    for candidate in selected(&config_root(), extra.as_deref(), custom.as_deref(), &mut scan) {
        match &candidate.spec {
            Spec::File(path) => {
                paths.insert(path.clone());
                if fs::symlink_metadata(path).is_ok_and(|value| value.file_type().is_file()) {
                    if let Some(parent) = path.parent() {
                        let manifest = parent.join("package.json");
                        if fs::symlink_metadata(&manifest).is_ok_and(|value| value.file_type().is_file()) { paths.insert(manifest); }
                    }
                }
            },
            Spec::Npm { name } => if let Ok(Some(path)) = cached_target(&candidate, &metadata) {
                let root = if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_dir()) { path } else { path.parent().unwrap_or(&path).to_path_buf() };
                if document(&root.join("package.json"), false, &mut scan.budget).ok().flatten().is_some_and(|value| value.get("name").and_then(Value::as_str) == Some(name.as_str())) { paths.insert(root); }
            },
            _ => {},
        }
    }
    let home = expand_tilde("~/");
    paths.into_iter().filter(|path| path.is_absolute() && !home.starts_with(path) && checked_root(path).is_ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "opencode").unwrap() }
    fn write(path: &Path, content: &str) { fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, content).unwrap(); }
    #[test]
    fn native_global_arrays_replace_then_later_sources_and_local_files_deduplicate() {
        let home = TestHome::new("opencode-declarations"); let global = home.home.join(".config/opencode"); let extra = home.home.join("extra");
        write(&global.join("config.json"), r#"{"plugin":["ignored-global"]}"#);
        write(&global.join("opencode.json"), r#"{"plugin":["replaced-global"]}"#);
        write(&global.join("opencode.jsonc"), "// JSONC\n{\"plugin\":[[\"chosen@1\",{\"token\":\"SECRET_SENTINEL\"}]],}");
        write(&extra.join("opencode.json"), r#"{"plugin":["chosen@2","extra"]}"#);
        write(&global.join("plugins/local.js"), "PRIVATE_CONFIG_SENTINEL");
        write(&global.join("plugins/nested/ignored.js"), "PRIVATE_CONFIG_SENTINEL");
        let mut scan = PluginScan::default(); let values = selected(&global, Some(&extra), None, &mut scan);
        assert!(!scan.unavailable); assert_eq!(values.len(), 3);
        assert!(values.iter().any(|value| value.native_spec == "chosen@2"));
        assert!(!values.iter().any(|value| value.native_spec.contains("global") || value.native_spec.contains("ignored")));
        let plugins: Vec<_> = values.into_iter().map(|value| observe_candidate(host(), value, &BTreeMap::new(), &home.home.join("metadata.json"), &mut scan).unwrap()).collect();
        assert!(plugins.iter().any(|value| value.presence == PluginPresence::Local));
        let output = serde_json::to_string(&plugins).unwrap(); assert!(!output.contains("SECRET_SENTINEL") && !output.contains("PRIVATE_CONFIG_SENTINEL"));
        assert!(plugins.iter().all(|value| value.enabled.is_none() && value.components.iter().all(|component| component.kind != PluginComponentKind::Mcp)));
    }
    #[test]
    fn native_toml_override_is_observed_without_triggering_its_migration() {
        let home = TestHome::new("opencode-native-toml"); let global = home.home.join(".config/opencode");
        write(&global.join("opencode.json"), r#"{"plugin":["overridden"]}"#);
        write(&global.join("config"), "plugin = [\"native-toml\"]\n");
        let before = fs::read(global.join("config")).unwrap(); let mut scan = PluginScan::default();
        let values = selected(&global, None, None, &mut scan);
        assert!(!scan.unavailable); assert_eq!(values.len(), 1); assert_eq!(values[0].native_spec, "native-toml");
        assert_eq!(fs::read(global.join("config")).unwrap(), before); assert!(!global.join("config.json").exists());
    }
    #[test]
    fn historical_metadata_only_joins_current_specifiers_and_does_not_claim_installation_or_activation() {
        let home = TestHome::new("opencode-history"); let root = home.home.join("package");
        write(&root.join("package.json"), r#"{"name":"chosen","version":"1.2.0","main":"index.js","oc-themes":["theme.json"]}"#);
        write(&root.join("index.js"), "SECRET_SENTINEL"); write(&root.join("theme.json"), "SECRET_SENTINEL");
        let values = serde_json::json!({"old":{"source":"npm","spec":"old@1","target":root},"current":{"source":"npm","spec":"chosen@1","target":root,"version":"WRONG_HISTORICAL_VERSION"}});
        let index = metadata_index(&values).unwrap(); let mut scan = PluginScan::default();
        let value = candidate("chosen@1", &home.home.join("opencode.json"), 0).unwrap();
        let observed = observe_candidate(host(), value, &index, &home.home.join("metadata.json"), &mut scan).unwrap();
        assert_eq!(observed.version, "1.2.0"); assert_eq!(observed.presence, PluginPresence::CacheOnly); assert_eq!(observed.enabled, None);
        assert!(observed.components.iter().any(|component| component.kind == PluginComponentKind::Theme));
        assert!(!serde_json::to_string(&observed).unwrap().contains("SECRET_SENTINEL"));
        let value = candidate("chosen@2", &home.home.join("opencode.json"), 0).unwrap();
        let observed = observe_candidate(host(), value, &index, &home.home.join("metadata.json"), &mut scan).unwrap();
        assert_eq!(observed.presence, PluginPresence::Declared); assert!(observed.components.is_empty());
        let value = candidate("new", &home.home.join("opencode.json"), 0).unwrap();
        let observed = observe_candidate(host(), value, &index, &home.home.join("metadata.json"), &mut scan).unwrap();
        assert_eq!(observed.presence, PluginPresence::Declared);
    }
    #[test]
    fn unsupported_urls_and_tuple_options_never_appear_in_metadata_and_json_modes_are_separate() {
        let home = TestHome::new("opencode-private-specifier"); let source = home.home.join("opencode.jsonc");
        write(&source, "// comment\n{\"plugin\":[[\"https://SECRET_SENTINEL:SECRET_SENTINEL@invalid/plugin\",{\"token\":\"SECRET_SENTINEL\"}]],}");
        let mut scan = PluginScan::default(); let mut values = declarations(&source, &mut scan.budget).unwrap().unwrap();
        assert!(document(&source, false, &mut scan.budget).is_err());
        let observed = observe_candidate(host(), values.pop().unwrap(), &BTreeMap::new(), &home.home.join("metadata.json"), &mut scan).unwrap();
        assert!(scan.unavailable); assert_eq!(observed.presence, PluginPresence::Declared);
        assert!(!serde_json::to_string(&observed).unwrap().contains("SECRET_SENTINEL"));
        write(&home.home.join("invalid.jsonc"), "{plugin:['not JSONC']}");
        assert!(document(&home.home.join("invalid.jsonc"), true, &mut scan.budget).is_err());
        assert!(check_jsonc_depth("/* { */ {\"value\":\"[\"} // ]").is_ok());
        assert!(check_jsonc_depth(&format!("{}0{}", "[".repeat(128), "]".repeat(128))).is_ok());
        assert!(check_jsonc_depth(&format!("{}0{}", "[".repeat(129), "]".repeat(129))).is_err());
    }
    #[test]
    fn builtin_integration_requires_both_exact_embedded_source_and_version_marker() {
        let home = TestHome::new("opencode-builtin-proof"); let path = home.home.join("native");
        let bytes = b"MAGIVERfixture-module"; write(&path, std::str::from_utf8(bytes).unwrap());
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap(); }
        let source = Root { path:path.to_string_lossy().into(),layout:"opencode_bundled".into(),presence:PluginPresence::Bundled,complete_marker:None };
        let artifact = Artifact { agent_id:"opencode".into(),version:"fixture".into(),magic_hex:hex::encode(b"MAGI"),module_offset:7,module_length:14,module_sha256:hex::encode(Sha256::digest(b"fixture-module")),version_offset:4,version_marker:"VER".into(),providers:vec![Provider {id:"FixtureAuth".into(),name:"Fixture".into()}] };
        let mut scan = PluginScan::default(); bundled(host(), &source, &artifact, &mut scan).unwrap();
        assert_eq!(scan.plugins[0].presence, PluginPresence::Bundled); assert_eq!(scan.plugins[0].components[0].kind, PluginComponentKind::AuthProvider); normalize(&mut scan); assert!(scan.mcps.is_empty());
        write(&path, "MAGIBADfixture-module"); let mut scan = PluginScan::default();
        assert!(bundled(host(), &source, &artifact, &mut scan).is_err() && scan.plugins.is_empty());
        write(&path, "MAGIVERchanged-module"); let mut scan = PluginScan::default();
        assert!(bundled(host(), &source, &artifact, &mut scan).is_err() && scan.plugins.is_empty());
    }
}
