//! Passive Cordis/NPM declarations, never Claude-format plugin ownership.
use super::*;
use crate::resources::deepseek_harness as cordis;
use std::str::FromStr;
use yaml_edit::{Document, YamlNode};

mod carrier;

const PRESET: &str = "@deepseek-ai/dsh-agent-preset";
const PRESET_VERSIONS: &[&str] = &["0.2.1-rc.3", "0.2.2-rc.1"];
fn profile_patch(agent: &AgentDefinition) -> Result<PathBuf, ()> {
    agent.global.as_deref().map(expand_tilde).ok_or(())
}
fn package_name(name: &str) -> bool {
    let parts = name.strip_prefix('@').map(|s| s.split('/').collect::<Vec<_>>()).unwrap_or_else(|| vec![name]);
    !name.is_empty() && name.len() <= 214 && parts.len() == (if name.starts_with('@') { 2 } else { 1 })
        && parts.iter().all(|p| !p.is_empty() && !matches!(*p, "." | "..")
            && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')))
}
fn signature(metadata: &fs::Metadata) -> Result<String, ()> {
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        Ok(format!("{}:{}:{}:{}:{}:{}", metadata.dev(), metadata.ino(), metadata.len(), metadata.ctime(), metadata.ctime_nsec(), metadata.mtime_nsec()))
    }
    #[cfg(not(unix))] { Ok(format!("{}:{:?}",metadata.len(),metadata.modified().map_err(|_| ())?)) }
}
struct BoundText { path: PathBuf, text: String, signature: String, digest: String }
impl BoundText {
    fn read(path: &Path, budget: &mut ScanBudget) -> Result<Option<Self>, ()> {
        budget.visit()?; checked_root(path)?;
        if !regular_metadata_file(path)? { return Ok(None); }
        let mut file = open_confined_file(path)?;
        let before = file.metadata().map_err(|_| ())?;
        if !before.is_file() || before.len() > 2_097_152 || before.len() as usize > MAX_READ_BYTES.saturating_sub(budget.bytes) { return Err(()); }
        let mut bytes = Vec::new(); let read = (&mut file).take(2_097_153).read_to_end(&mut bytes);
        budget.bytes += bytes.len();
        read.map_err(|_| ())?;
        if bytes.len() > 2_097_152 || bytes.len() != before.len() as usize || budget.bytes > MAX_READ_BYTES { return Err(()); }
        let sig = signature(&before)?;
        if sig != signature(&file.metadata().map_err(|_| ())?)? || sig != signature(&fs::symlink_metadata(path).map_err(|_| ())?)? { return Err(()); }
        let digest = hex::encode(Sha256::digest(&bytes));
        Ok(Some(Self { path:path.into(), text:String::from_utf8(bytes).map_err(|_| ())?, signature:sig, digest }))
    }
    fn json(&self) -> Result<Value, ()> { serde_json::from_str(&self.text).map_err(|_| ()) }
    fn recheck(&self, budget: &mut ScanBudget) -> Result<(), ()> {
        let now = Self::read(&self.path, budget)?.ok_or(())?;
        if now.signature == self.signature && now.digest == self.digest { Ok(()) } else { Err(()) }
    }
}
fn reference(source: &Path, selected: bool) -> PluginComponent {
    PluginComponent { kind:PluginComponentKind::PluginReference,
        name:if selected { "Selected Cordis bundle; runtime activation unobserved" } else { "Declared dependency; selection and activation unobserved" }.into(),
        source_path:collapse_home(source),native_id:None }
}
// Only the named slot is resolved. Profile-local pnpm aliases must remain in
// that node_modules tree; external/global installations stay unresolved.
fn installed(modules: &Path, name: &str, budget: &mut ScanBudget) -> Result<Option<(PathBuf, PathBuf, BoundText, Value)>, ()> {
    installed_within(modules, modules, name, budget)
}

fn installed_within(modules: &Path, pool: &Path, name: &str, budget: &mut ScanBudget) -> Result<Option<(PathBuf, PathBuf, BoundText, Value)>, ()> {
    checked_root(modules)?;
    let slot = modules.join(name);
    match fs::symlink_metadata(&slot) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(_) => return Err(()), Ok(_) => {},
    }
    let physical = fs::canonicalize(&slot).map_err(|_| ())?;
    if !physical.starts_with(fs::canonicalize(pool).map_err(|_| ())?) || !physical.is_dir() { return Err(()); }
    checked_root(&physical)?;
    let file = BoundText::read(&physical.join("package.json"), budget)?.ok_or(())?;
    let value = file.json()?;
    if value.get("name").and_then(Value::as_str) != Some(name) { return Err(()); }
    Ok(Some((slot,physical,file,value)))
}
fn mcp_references(node: &YamlNode, path: &Path, output: &mut Vec<PluginComponent>, depth: usize) -> Result<(), ()> {
    if depth > 48 || output.len() > MAX_COMPONENTS { return Err(()); }
    if let Some(sequence) = node.as_sequence() {
        for child in sequence { mcp_references(&child,path,output,depth+1)?; }
    } else if let Some(mapping) = node.as_mapping() {
        let field = |key| mapping.get(key).and_then(|n| cordis::node_json(&n).ok())
            .and_then(|v| v.as_str().map(str::to_owned));
        if let Some(name) = field("name") {
            if matches!(name.as_str(), "@deepseek-ai/dsh-mcp-manager" | cordis::DSH_MCP_CLIENT_NAME) {
                let before = output.len();
                if let Some(config) = mapping.get_mapping("config") {
                    if name == cordis::DSH_MCP_CLIENT_NAME {
                        if let Some(server) = config.get("serverName").and_then(|node| cordis::node_json(&node).ok()) {
                            if let Some(server) = server.as_str() { mcp_declaration(server, &config, path, output)?; }
                        }
                    } else if let Some(servers) = config.get_mapping("servers") {
                        for (key, entry) in servers.iter() {
                            if let (Some(server), Some(config)) = (key.as_scalar(), entry.as_mapping()) {
                                mcp_declaration(&server.as_string(), config, path, output)?;
                            }
                        }
                    }
                }
                if output.len() == before {
                    let id = field("id").as_deref().and_then(metadata_name);
                    output.push(PluginComponent {
                        kind: PluginComponentKind::McpReference,
                        name: "Cordis MCP component reference; runtime registrations unobserved".into(),
                        source_path: format!("{}#{}", collapse_home(path), id.unwrap_or(name)),
                        native_id: None,
                    });
                }
            }
        } else if mapping.contains_key("name") {
            return Err(());
        }
        if let Some(n) = mapping.get("insert") { mcp_references(&n,path,output,depth+1)?; }
        if mapping.get("group").and_then(|n| cordis::node_json(&n).ok()) == Some(Value::Bool(true)) {
            if let Some(n) = mapping.get("config") { mcp_references(&n,path,output,depth+1)?; }
        }
    }
    Ok(())
}

fn mcp_declaration(name: &str, config: &yaml_edit::Mapping, path: &Path, output: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if cordis::valid_mcp_server_name(name)
        && config.get("transport").and_then(|node| cordis::node_json(&node).ok())
            .is_some_and(|transport| matches!(transport.as_str(), Some("stdio" | "streamable-http")))
    {
        if output.len() >= MAX_COMPONENTS { return Err(()); }
        output.push(PluginComponent {
            kind: PluginComponentKind::Mcp, name: name.into(),
            source_path: collapse_home(path), native_id: Some(name.into()),
        });
    }
    Ok(())
}
// Only this fixed upstream contract explicitly publishes Skills. Do not infer
// a `skills/` convention for arbitrary Cordis or NPM packages.
fn check_preset(metadata: &Value) -> Result<(), ()> {
    if metadata.get("version").and_then(Value::as_str).is_none_or(|version| !PRESET_VERSIONS.contains(&version))
        || metadata.pointer("/repository/url").and_then(Value::as_str) != Some("git+https://github.com/deepseek-ai/deepseek-harness.git")
        || metadata.pointer("/repository/directory").and_then(Value::as_str) != Some("packages/preset/agent-preset")
        || !metadata.get("files").and_then(Value::as_array).is_some_and(|a| a.iter().any(|v| v.as_str()==Some("skills"))) { return Err(()); }
    Ok(())
}

fn skill_files(source: &Path, budget: &mut ScanBudget) -> Result<Vec<PluginComponent>, ()> {
    if !source.is_dir() { return Err(()); }
    let mut output = Vec::new();
    for directory in dirs(source,budget)? {
        let path = relative(&directory,"SKILL.md")?;
        if regular_metadata_file(&path)? {
            let name = directory.file_name().and_then(|n| n.to_str()).and_then(metadata_name).ok_or(())?;
            output.push(PluginComponent { kind:PluginComponentKind::Skill,name,source_path:collapse_home(&path),native_id:None });
            if output.len()>MAX_COMPONENTS { return Err(()); }
        }
    }
    Ok(output)
}
fn scan(host: &Host, path: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let profile = cordis::agent_profile_at(&host.agent_id, path).map_err(|_| ())?;
    let directory = path.parent().ok_or(())?;
    let manifest = match BoundText::read(&directory.join("package.json"),&mut result.budget)? { Some(m)=>m,None=>return Ok(()) };
    let value = manifest.json()?;
    let mut pending = BTreeMap::<String,(PathBuf,bool)>::new();
    if let Some(bundles) = value.pointer("/dsh/profile/bundles") {
        for name in bundles.as_array().ok_or(())? {
            let name = name.as_str().filter(|s| package_name(s)).ok_or(())?;
            pending.insert(name.into(),(manifest.path.clone(),true));
        }
    }
    if let Some(dependencies) = value.get("dependencies") {
        for (name, specification) in dependencies.as_object().ok_or(())? {
            if !package_name(name) || !specification.is_string() { return Err(()); }
            pending.entry(name.clone()).or_insert((manifest.path.clone(),false));
        }
    }
    if pending.len()>256 { return Err(()); }
    let modules = directory.join("node_modules");
    let installation_bundles = pending.iter().filter(|(name, (_, selected))| *selected
        && fs::symlink_metadata(modules.join(name)).is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound))
        .map(|(name, _)| name.as_str()).collect::<Vec<_>>();
    let mut bundled = match carrier::observe(profile, &installation_bundles, &mut result.budget) {
        Ok(packages) => packages,
        Err(()) => { result.unavailable = true; BTreeMap::new() }
    };
    for name in [PRESET, carrier::OFFICE] {
        if bundled.contains_key(name) {
            pending.entry(name.into()).or_insert((manifest.path.clone(), false));
        }
    }
    let mut staged=Vec::new(); let mut seen=BTreeSet::new();
    while let Some((name,(source,selected))) = pending.pop_first() {
        if !seen.insert(name.clone()) { continue; }
        let mut observation = NativePluginObservation {
            agent_id:host.agent_id.clone(),plugin_id:format!("cordis:{profile}:{name}"),name:name.clone(),
            version:"unknown".into(),revision:format!("profile:{profile}"),presence:PluginPresence::Declared,enabled:None,
            source_paths:vec![collapse_home(&source)],components:vec![reference(&source,selected)],docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
        };
        match installed(&modules,&name,&mut result.budget) {
            Ok(Some((slot,root,file,metadata))) => {
                let mut components=Vec::new(); let mut documents=Vec::new();
                let parsed=(|| {
                    if let Some(patch) = metadata.pointer("/dsh/bundle/patch") {
                        let paths=match patch { Value::String(s)=>vec![s.as_str()],Value::Array(a)=>a.iter().map(|v|v.as_str().ok_or(())).collect::<Result<Vec<_>,_>>()?,_=>return Err(()) };
                        if paths.len()>64 { return Err(()); }
                        for input in paths {
                            if input.len() > 4096 || input.chars().any(char::is_control) { return Err(()); }
                            let path=relative(&root,input)?;
                            if !matches!(path.extension().and_then(|s|s.to_str()), Some("yml" | "yaml")) { return Err(()); }
                            let text=BoundText::read(&path,&mut result.budget)?.ok_or(())?;
                            let document=Document::from_str(&text.text).map_err(|_| ())?;
                            let sequence=document.as_sequence().ok_or(())?;
                            mcp_references(&YamlNode::Sequence(sequence),&path,&mut components,0)?;
                            components.push(PluginComponent {kind:PluginComponentKind::Code,name:"Cordis bundle patch reference; composition not evaluated".into(),source_path:collapse_home(&path),native_id:None});
                            documents.push(text);
                        }
                    }
                    if name == PRESET {
                        check_preset(&metadata)?;
                        components.extend(skill_files(&relative(&root, "skills")?, &mut result.budget)?);
                    }
                    file.recheck(&mut result.budget)?;
                    for document in &documents { document.recheck(&mut result.budget)?; }
                    if fs::canonicalize(&slot).map_err(|_| ())? != root { return Err(()); }
                    Ok::<(),()>(())
                })();
                if parsed.is_err() { result.unavailable=true; }
                if file.recheck(&mut result.budget).is_ok() && fs::canonicalize(&slot).ok().as_ref()==Some(&root) {
                    observation.presence=PluginPresence::Installed;
                    let version=metadata.get("version").and_then(Value::as_str).filter(|v| v.len()<=128 && semver::Version::parse(v).is_ok());
                    observation.version=version.unwrap_or("unknown").into();
                    if version.is_none() { result.unavailable=true; }
                    observation.revision=format!("manifest-sha256:{}",file.digest);
                    observation.source_paths.push(collapse_home(&file.path));
                    if parsed.is_ok() && version.is_some() { observation.components.extend(components); }
                    // Follow one authored dependency declaration from a selected
                    // bundle; never search directories for the preset's name.
                    if selected && metadata.get("dependencies").and_then(|v|v.get(PRESET)).is_some() && !seen.contains(PRESET) {
                        pending.entry(PRESET.into()).or_insert((file.path.clone(),false));
                    }
                } else { result.unavailable=true; }
            }
            Ok(None) => match bundled.remove(&name) {
                Some(package) if !value.get("dependencies").is_some_and(|deps| deps.get(&name).is_some()) => {
                    observation.presence = PluginPresence::Bundled;
                    observation.version = package.version;
                    observation.revision = package.revision;
                    observation.components.clear();
                    observation.source_paths.push(package.source);
                    observation.components.extend(package.skills);
                }
                _ => result.unavailable = true,
            },
            Err(()) => result.unavailable = true,
        }
        staged.push(observation); if staged.len()>256 { return Err(()); }
    }
    manifest.recheck(&mut result.budget)?;
    result.plugins.extend(staged); Ok(())
}
pub(super) fn observe(host: &Host, agent: &AgentDefinition, result: &mut PluginScan) {
    if profile_patch(agent).and_then(|path| scan(host, &path, result)).is_err() { result.unavailable = true; }
}
pub(super) fn watch_paths(host: &Host, agent: &AgentDefinition) -> Vec<PathBuf> {
    let Ok(path) = profile_patch(agent) else { return vec![]; };
    let Ok(profile) = cordis::agent_profile_at(&host.agent_id, &path) else { return vec![]; };
    let Some(root) = path.parent() else { return vec![]; };
    let mut paths = vec![root.join("package.json"),root.join("node_modules")];
    paths.extend(carrier::watch_paths(profile));
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn fixture() -> (TestHome,PathBuf) {
        let home=TestHome::new("dsh-plugin-metadata");let root=home.home.join(".dsh");std::env::set_var("DSH_HOME",&root);
        let directory=root.join("profiles/web");fs::create_dir_all(&directory).unwrap();(home,directory)
    }
    fn host()->&'static Host {hosts().iter().find(|h|h.agent_id=="deepseek-harness").unwrap()}
    #[test]
    fn selected_bundle_is_a_declaration_until_physical_metadata_is_bound() {
        let (_home,root)=fixture();fs::write(root.join("package.json"),serde_json::json!({"dsh":{"profile":{"bundles":["@fixture/bundle"]}}}).to_string()).unwrap();
        let mut result=PluginScan::default();scan(host(),&root.join("cordis.patch.yml"),&mut result).unwrap();assert!(result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Declared);assert_eq!(result.plugins[0].enabled,None);assert!(result.mcps.is_empty());
    }
    #[test]
    fn official_manifest_skills_are_metadata_only_and_do_not_invent_connections() {
        let (_home,root)=fixture();fs::write(root.join("package.json"),serde_json::json!({"dependencies":{"@deepseek-ai/dsh-agent-preset":"*"}}).to_string()).unwrap();
        let package=root.join("node_modules").join(PRESET);fs::create_dir_all(package.join("skills/fixture-skill")).unwrap();
        fs::write(package.join("skills/fixture-skill/SKILL.md"),"private body sentinel").unwrap();
        for version in PRESET_VERSIONS {
            fs::write(package.join("package.json"),serde_json::json!({"name":PRESET,"version":version,"repository":{"url":"git+https://github.com/deepseek-ai/deepseek-harness.git","directory":"packages/preset/agent-preset"},"files":["skills"]}).to_string()).unwrap();
            let mut result=PluginScan::default();scan(host(),&root.join("cordis.patch.yml"),&mut result).unwrap();assert!(!result.unavailable);assert_eq!(result.plugins[0].presence,PluginPresence::Installed);assert_eq!(result.plugins[0].enabled,None);assert!(result.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.name=="fixture-skill"));assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private body sentinel"));assert!(result.mcps.is_empty());
        }
    }
    #[cfg(unix)]
    #[test]
    fn external_package_alias_stays_unavailable_and_never_reads_private_metadata() {
        use std::os::unix::fs::symlink;
        let (home,root)=fixture();fs::write(root.join("package.json"),serde_json::json!({"dependencies":{"@fixture/escape":"*"}}).to_string()).unwrap();
        fs::create_dir_all(root.join("node_modules/@fixture")).unwrap();let outside=home.home.join("outside");fs::create_dir(&outside).unwrap();fs::write(outside.join("package.json"),"private sentinel").unwrap();
        symlink(&outside,root.join("node_modules/@fixture/escape")).unwrap();let mut result=PluginScan::default();scan(host(),&root.join("cordis.patch.yml"),&mut result).unwrap();assert!(result.unavailable);assert_eq!(result.plugins[0].presence,PluginPresence::Declared);assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private sentinel"));
    }
}
