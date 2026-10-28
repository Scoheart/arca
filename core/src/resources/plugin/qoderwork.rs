//! QoderWork's physical packages and digest-bound builtins. Host initialization,
//! account gates, defaults, SDK acceptance and live connections stay unobserved.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    version: String, bundle_id: String, product_name: String, guards: Vec<Guard>,
    connectors: Vec<Connector>, legokits: Vec<Legokit>, blocked_plugins: Vec<String>,
    sdk_package: Guard, sdk_version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Connector { id: String, name: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Legokit { id: String, server_id: String, bundle: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-qoderwork.json")).expect("audited QoderWork contract"))
}
fn root(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) } else { expand_tilde(&source.path) }
}
fn folder(value: &str) -> Result<String, ()> {
    if value.starts_with('.') || value.contains("..") || value.contains(['/', '\\']) { return Err(()); }
    metadata_name(value).ok_or(())
}
fn snapshot(host: &Host, id: &str, name: &str, version: &str, presence: PluginPresence, path: &Path, revision: &str) -> NativePluginObservation {
    NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:id.into(), name:name.into(), version:version.into(),
        revision:revision.into(), presence, enabled:None, source_paths:vec![collapse_home(path)], components:Vec::new(),
        docs_url:host.docs_url.clone(), source_fingerprint:String::new() }
}
fn add(path: &Path, kind: PluginComponentKind, name: &str, id: Option<String>, out: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if out.len() >= MAX_COMPONENTS { return Err(()); }
    out.push(PluginComponent { kind, name:metadata_name(name).ok_or(())?, native_id:id, source_path:collapse_home(path) }); Ok(())
}
fn features(root: &Path, input: &str, kind: PluginComponentKind, out: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let path = relative(root, input)?;
    // Native Work UI reads direct children. Bodies/frontmatter remain private.
    for entry in directory_entries(&path, budget)? {
        let name = entry.path.file_name().and_then(|v| v.to_str()).ok_or(())?;
        if name.starts_with('.') || name.contains("..") { continue; }
        let file = if kind == PluginComponentKind::Skill && entry.directory { entry.path.join("SKILL.md") }
            else if kind != PluginComponentKind::Skill && entry.file && name.ends_with(".md") { entry.path }
            else { continue; };
        if regular_metadata_file(&file)? {
            let label = file.strip_prefix(root).map_err(|_| ())?.to_string_lossy();
            add(&file,kind.clone(),&label,None,out)?;
        }
    }
    Ok(())
}
fn mcps(root: &Path, directory: &str, out: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let Some(value) = optional_json(root,".mcp.json",budget)? else { return Ok(()); };
    let Some(servers) = value.get("mcpServers") else { return if value.is_object() { Ok(()) } else { Err(()) }; };
    let servers = servers.as_object().filter(|v| v.len() <= MAX_COMPONENTS).ok_or(())?;
    for (name,value) in servers {
        metadata_name(name).ok_or(())?;
        if !value.is_object() { return Err(()); }
        // This is a declaration, even when Work has its connector disabled.
        add(&root.join(".mcp.json"),PluginComponentKind::Mcp,name,Some(format!("plugin:{directory}:{name}")),out)?;
    }
    Ok(())
}
fn package(host: &Host, path: &Path, presence: PluginPresence, revision: &str, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(path)?;
    let directory = folder(path.file_name().and_then(|v| v.to_str()).ok_or(())?)?;
    if contract().blocked_plugins.contains(&directory) { return Ok(()); }
    let mut selected = None;
    for input in &host.manifest_paths {
        if let Some(value) = optional_json(path,input,&mut result.budget)? { selected=Some((input,value)); break; }
    }
    let Some((input,value)) = selected else { return Ok(()); };
    if !value.is_object() { return Err(()); }
    let name = value.get("displayName").or_else(||value.get("name")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||directory.clone());
    let version = value.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
    let mut plugin = snapshot(host,&directory,&name,&version,presence,&path.join(input),revision);
    for (input,kind) in [("skills",PluginComponentKind::Skill),("commands",PluginComponentKind::Command)] {
        if features(path,input,kind,&mut plugin.components,&mut result.budget).is_err() { result.unavailable=true; }
    }
    if mcps(path,&directory,&mut plugin.components,&mut result.budget).is_err() { result.unavailable=true; }
    result.plugins.push(plugin); Ok(())
}
fn packages(host: &Host, path: &Path, presence: PluginPresence, revision: &str, result: &mut PluginScan) -> Result<(), ()> {
    for entry in directory_entries(path,&mut result.budget)? {
        if entry.directory {
            if entry.path.file_name().and_then(|v|v.to_str()).is_some_and(|v|v.starts_with('.')) { continue; }
            if package(host,&entry.path,presence.clone(),revision,result).is_err() { result.unavailable=true; }
        } else if !entry.file { result.unavailable=true; }
    }
    Ok(())
}
fn sdk_resources(host: &Host, resources: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let c=contract(); let package=relative(resources,&c.sdk_package.path)?;
    verify_artifact(&package,c.sdk_package.bytes,&c.sdk_package.sha256,&mut result.budget)?;
    let worker=relative(package.parent().ok_or(())?,"dist/_worker")?;
    let mut skills=snapshot(host,"@builtin/sdk-skills","QoderWork SDK builtin Skills",&c.sdk_version,PluginPresence::Bundled,&worker.join("builtin"),"shipped-sdk-resources");
    features(&worker,"builtin",PluginComponentKind::Skill,&mut skills.components,&mut result.budget)?;
    if !skills.components.is_empty() { result.plugins.push(skills); }
    let root=relative(&worker,"vendor/qoder-security")?;
    let input=".qoder-plugin/plugin.json";
    let value=optional_json(&root,input,&mut result.budget)?.ok_or(())?;
    if value.get("name").and_then(Value::as_str)!=Some("security-scan") { return Err(()); }
    let version=value.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let mut plugin=snapshot(host,"security-scan@qoder-bundler","Qoder Security",&version,PluginPresence::Bundled,&root.join(input),"shipped-sdk-vendor");
    features(&root,"skills",PluginComponentKind::Skill,&mut plugin.components,&mut result.budget)?;
    if let Some(input)=value.get("hooks").and_then(Value::as_str) {
        if let Some(value)=optional_json(&root,input,&mut result.budget)? {
            let events=value.get("hooks").unwrap_or(&value).as_object().filter(|v|v.len()<=64).ok_or(())?;
            for (name,records) in events {
                if !records.is_array() { return Err(()); }
                add(&relative(&root,input)?,PluginComponentKind::Hook,name,None,&mut plugin.components)?;
            }
        }
    }
    for entry in directory_entries(&root.join("bin"),&mut result.budget)? {
        if entry.file { add(&entry.path,PluginComponentKind::Executable,entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?,None,&mut plugin.components)?; }
        else if !entry.directory { result.unavailable=true; }
    }
    result.plugins.push(plugin); Ok(())
}
fn bundled(host: &Host, app: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let c=contract();
    let version=app_version(app,&c.bundle_id,&c.product_name,&mut result.budget)?;
    if version != c.version { return Err(()); }
    let resources=relative(app,"Contents/Resources")?;
    let asar=resources.join("app.asar"); guard_asar(&asar,&c.guards,&mut result.budget)?;
    let source=PathBuf::from(format!("{}!/out/main/main.js",asar.to_string_lossy()));
    let mut connectors=snapshot(host,"@builtin/connectors","QoderWork builtin connectors",&version,PluginPresence::Bundled,&source,"audited-native-factories");
    for connector in &c.connectors { add(&source,PluginComponentKind::Mcp,&connector.name,Some(connector.id.clone()),&mut connectors.components)?; }
    result.plugins.push(connectors);
    for (id,name,folder,kind) in [
        ("@builtin/skills","QoderWork builtin Skills","skills",PluginComponentKind::Skill),
        ("@builtin/commands","QoderWork builtin commands","commands",PluginComponentKind::Command),
    ] {
        let mut plugin=snapshot(host,id,name,&version,PluginPresence::Bundled,&resources.join(folder),"shipped-resources");
        features(&resources,folder,kind,&mut plugin.components,&mut result.budget)?;
        if !plugin.components.is_empty() { result.plugins.push(plugin); }
    }
    for kit in &c.legokits {
        let path=relative(&resources,&format!("legokits/{}",folder(&kit.id)?))?;
        let bundle=relative(&path,&kit.bundle)?;
        if !regular_metadata_file(&bundle)? { return Err(()); }
        let mut plugin=snapshot(host,&format!("@builtin/legokit:{}",kit.id),&format!("QoderWork {}",kit.id),&version,PluginPresence::Bundled,&bundle,"host-managed-runtime");
        add(&bundle,PluginComponentKind::Mcp,&kit.server_id,Some(kit.server_id.clone()),&mut plugin.components)?;
        for (folder,kind) in [("skills",PluginComponentKind::Skill),("agents",PluginComponentKind::Agent)] {
            if features(&path,folder,kind,&mut plugin.components,&mut result.budget).is_err() { result.unavailable=true; }
        }
        let context=path.join("SYSTEM.md");
        if regular_metadata_file(&context)? { add(&context,PluginComponentKind::Context,"SYSTEM.md",None,&mut plugin.components)?; }
        result.plugins.push(plugin);
    }
    if sdk_resources(host,&resources,result).is_err() { result.unavailable=true; }
    // These physical examples are native install sources, never installed state.
    packages(host,&resources.join("plugins-example"),PluginPresence::Bundled,"bundled-install-source",result)
}
fn external_legokits(host: &Host, path: &Path, result: &mut PluginScan) -> Result<(), ()> {
    for entry in directory_entries(path,&mut result.budget)? {
        if !entry.directory { if !entry.file { result.unavailable=true; } continue; }
        let observed=(|| {
            let name=folder(entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?)?;
            let Some(manifest)=optional_json(&entry.path,"manifest.json",&mut result.budget)? else { return Ok(()); };
            let id=manifest.get("id").or_else(||manifest.get("legokitId")).and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
            let mut plugin=snapshot(host,&format!("legokit:{name}"),&id,"unknown",PluginPresence::Local,&entry.path.join("manifest.json"),"external-declaration");
            let config=manifest.get("mcp").unwrap_or(&manifest);
            if let Some(server)=config.get("serverName") {
                let server=server.as_str().and_then(metadata_name).ok_or(())?;
                let bundle=config.get("bundleFile").and_then(Value::as_str).ok_or(())?;
                let bundle=relative(&entry.path,bundle)?;
                if !regular_metadata_file(&bundle)? { return Err(()); }
                add(&bundle,PluginComponentKind::Mcp,&server,Some(server.clone()),&mut plugin.components)?;
            }
            for (folder,kind) in [("skills",PluginComponentKind::Skill),("agents",PluginComponentKind::Agent)] { features(&entry.path,folder,kind,&mut plugin.components,&mut result.budget)?; }
            result.plugins.push(plugin); Ok::<(),()>(())
        })(); if observed.is_err() { result.unavailable=true; }
    }
    Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let mut selected=false;
    for source in &host.roots {
        let path=root(source);
        let observed=match source.layout.as_str() {
            "qoderwork_app" => {
                if selected { continue; }
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_dir() => { selected=true; bundled(host,&path,result) },
                    Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(()),
                    _ => { selected=true; Err(()) },
                }
            },
            "qoderwork_plugins" => packages(host,&path,PluginPresence::Installed,"user-package",result),
            "qoderwork_custom_plugins" => packages(host,&path,PluginPresence::Local,"custom-package",result),
            "qoderwork_legokits" => external_legokits(host,&path,result),
            _ => Err(()),
        }; if observed.is_err() { result.unavailable=true; }
    }
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> { host.roots.iter().map(root).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|h|h.agent_id=="qoderwork").unwrap() }
    fn write(root: &Path, input: &str, body: &str) { let path=root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path,body).unwrap(); }
    #[test]
    fn physical_plugins_require_authored_manifest_and_export_no_config_values_or_bodies() {
        let home=TestHome::new("work-plugin"); let root=home.home.join(".qoderwork/plugins/example");
        write(&root,"plugin.json",r#"{"name":"wrong-root"}"#);
        let mut scan=PluginScan::default(); package(host(),&root,PluginPresence::Installed,"user-package",&mut scan).unwrap(); assert!(scan.plugins.is_empty());
        write(&root,".qoder-plugin/plugin.json",r#"{"name":"example","version":"1.0"}"#);
        write(&root,"skills/one/SKILL.md","private skill body");
        write(&root,".mcp.json",r#"{"mcpServers":{"service":{"url":"https://private.example","headers":{"Authorization":"private-token"}}}}"#);
        package(host(),&root,PluginPresence::Installed,"user-package",&mut scan).unwrap(); normalize(&mut scan);
        let serialized=serde_json::to_string(&scan.plugins).unwrap();
        for secret in ["private skill body","private.example","private-token"] { assert!(!serialized.contains(secret)); }
        assert_eq!(scan.mcps[0].server_id,"plugin:example:service"); assert_eq!(scan.plugins[0].enabled,None);
    }
    #[test]
    fn external_legokit_cannot_escape_its_own_package() {
        let home=TestHome::new("work-legokit"); let root=home.home.join(".qoderwork/legokits");
        write(&root,"example/manifest.json",r#"{"id":"example","mcp":{"serverName":"example","bundleFile":"../outside.cjs"}}"#);
        write(&root,"outside.cjs","must remain inert");
        let mut scan=PluginScan::default(); external_legokits(host(),&root,&mut scan).unwrap(); assert!(scan.unavailable); assert!(scan.plugins.is_empty());
    }
}
