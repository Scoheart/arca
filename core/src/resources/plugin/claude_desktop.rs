//! Claude Desktop's current MCPB installation metadata and digest-bound shipped
//! declarations. Native policy, credentials, Code/Cowork session state and
//! lifecycle remain owned by Claude; no application or extension code is run.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String,
    app_path: String, guards: Vec<Guard>, mcp_factories: Vec<Factory>,
    skills_manifest: String, skills_ref: String, skills: Vec<String>,
    managed_connector_kinds: Vec<String>, managed_factory_source: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Factory { name: String, source: String, factory: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-claude-desktop.json"))
        .expect("audited Claude Desktop contract"))
}
fn id(value: &str) -> Result<&str, ()> {
    if value.is_empty() || value.len()>128 || value.starts_with('.') || value.contains("..")
        || !value.bytes().all(|c|c.is_ascii_alphanumeric()||b"._-".contains(&c)) { return Err(()); }
    Ok(value)
}
fn native_roots(host: &Host) -> Result<Vec<(&'static str,PathBuf)>, ()> {
    if let Some(path)=std::env::var_os("CLAUDE_USER_DATA_DIR").filter(|v|!v.is_empty()) {
        let path=PathBuf::from(path);
        if !path.is_absolute() || expand_tilde("~/").starts_with(&path) { return Err(()); }
        checked_root(&path)?; return Ok(vec![("override",path)]);
    }
    host.roots.iter().map(|source| {
        let scope=match source.layout.as_str() { "claude_desktop_standard"=>"standard","claude_desktop_third_party"=>"third-party",_=>return Err(()) };
        Ok((scope,expand_tilde(&source.path)))
    }).collect()
}
fn snapshot(host: &Host, identity: &str, name: &str, version: &str, presence: PluginPresence, source: String) -> NativePluginObservation {
    NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:identity.into(), name:name.into(),
        version:version.into(), revision:source.clone(), presence, enabled:None,
        source_paths:vec![source], components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() }
}
fn component(plugin: &mut NativePluginObservation, kind: PluginComponentKind, name: &str, source: String, native_id: Option<String>) -> Result<(), ()> {
    if plugin.components.len()>=MAX_COMPONENTS { return Err(()); }
    plugin.components.push(PluginComponent { kind, name:metadata_name(name).ok_or(())?, source_path:source, native_id }); Ok(())
}
fn named_features(value: &Value, key: &str, kind: PluginComponentKind, source: &str, plugin: &mut NativePluginObservation) -> Result<(), ()> {
    let Some(values)=value.get(key) else { return Ok(()); };
    let values=values.as_array().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())?;
    for value in values {
        component(plugin,kind.clone(),value.get("name").and_then(Value::as_str).ok_or(())?,source.into(),None)?;
    }
    Ok(())
}
fn extension(host: &Host, scope: &str, directory: &Path, identity: &str, recorded: bool, metadata: &Path, result: &mut PluginScan) -> Result<Option<NativePluginObservation>, ()> {
    id(identity)?; checked_root(directory)?;
    let Some(value)=optional_json(directory,"manifest.json",&mut result.budget)? else { return Ok(None); };
    let version=value.get("manifest_version").or_else(||value.get("dxt_version")).and_then(Value::as_str).ok_or(())?;
    if !["0.1","0.2","0.3","0.4"].contains(&version) { return Err(()); }
    for key in ["manifest_version","dxt_version"] {
        if value.get(key).is_some_and(|v|v.as_str()!=Some(version)) { return Err(()); }
    }
    let name=value.get("name").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let display=match value.get("display_name") {
        Some(Value::String(s)) if s.is_empty()=>name.clone(),
        Some(Value::String(s))=>metadata_name(s).ok_or(())?, None=>name.clone(), _=>return Err(()),
    };
    let package_version=value.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    if !value.get("description").is_some_and(Value::is_string)
        || !value.get("author").and_then(|v|v.get("name")).is_some_and(Value::is_string) { return Err(()); }
    let server=value.get("server").and_then(Value::as_object).ok_or(())?;
    let kind=server.get("type").and_then(Value::as_str).ok_or(())?;
    if !["node","python","binary"].contains(&kind) && !(kind=="uv"&&version=="0.4") { return Err(()); }
    let entry=server.get("entry_point").and_then(Value::as_str).ok_or(())?;
    if !server.get("mcp_config").and_then(|v|v.get("command")).is_some_and(Value::is_string) { return Err(()); }
    let source=collapse_home(&directory.join("manifest.json"));
    let mut plugin=snapshot(host,&format!("extension:{scope}:{identity}"),&display,&package_version,PluginPresence::Installed,source.clone());
    if recorded { plugin.source_paths.push(collapse_home(metadata)); }
    // This stable inventory ID identifies the extension declaration. The native
    // display-name collision resolver and effective callable namespace stay native.
    component(&mut plugin,PluginComponentKind::Mcp,&display,source.clone(),Some(format!("extension:{scope}:{identity}")))?;
    named_features(&value,"tools",PluginComponentKind::Tool,&source,&mut plugin)?;
    named_features(&value,"prompts",PluginComponentKind::Context,&source,&mut plugin)?;
    // Entry-point bodies, mcp_config arguments/env and all user configuration are
    // intentionally not projected. Only a confined physical entry-point is named.
    let entry=relative(directory,entry)?;
    if regular_metadata_file(&entry)? {
        component(&mut plugin,PluginComponentKind::Code,"server entry point",collapse_home(&entry),None)?;
    }
    Ok(Some(plugin))
}
fn extensions(host: &Host, scope: &str, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    checked_root(root)?;
    let metadata=root.join("extensions-installations.json");
    let mut recorded=BTreeMap::new();
    if let Some(value)=optional_json(root,"extensions-installations.json",&mut result.budget)? {
        let rows=value.get("extensions").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
        for (key,value) in rows {
            id(key)?;
            if !value.is_object() || value.get("id").is_some_and(|v|v.as_str()!=Some(key)) { return Err(()); }
            // Retain only non-secret identifiers from the native installation
            // record. Never retain its manifest, signature, source or other fields.
            let version=value.get("manifest").and_then(|v|v.get("version")).and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(||"unknown".into());
            recorded.insert(key.clone(),version);
        }
    }
    let directory=root.join("Claude Extensions");
    for entry in directory_entries(&directory,&mut result.budget)? {
        if !entry.directory { if !entry.file { result.unavailable=true; } continue; }
        let key=entry.path.file_name().and_then(|v|v.to_str()).ok_or(())?;
        match extension(host,scope,&entry.path,key,recorded.contains_key(key),&metadata,result) {
            Ok(Some(plugin))=>{recorded.remove(key);result.plugins.push(plugin);},
            Ok(None)=>{},Err(())=>result.unavailable=true,
        }
    }
    // A ledger entry without a readable physical manifest is a declaration; its
    // stale embedded manifest cannot manufacture installed capabilities.
    for (key,version) in recorded {
        result.plugins.push(snapshot(host,&format!("extension:{scope}:{key}"),&key,&version,PluginPresence::Declared,collapse_home(&metadata)));
    }
    Ok(())
}
fn bundled(host: &Host, result: &mut PluginScan) -> Result<(), ()> {
    let c=contract(); if c.agent_id!=host.agent_id { return Err(()); }
    let app=crate::paths::system_probe_path(&c.app_path);
    if !app.try_exists().map_err(|_| ())? { return Ok(()); }
    if app_version(&app,&c.bundle_id,&c.product_name,&mut result.budget)?!=c.version { return Err(()); }
    let asar=relative(&app,"Contents/Resources/app.asar")?;
    // Only the complete packed members whose current bytes were analyzed are
    // accepted. Unpacked native extensions and their conflicting ASAR digests are
    // outside this observation contract; no execution or trust claim is made.
    guard_asar(&asar,&c.guards,&mut result.budget)?;
    let source=|member:&str|format!("{}!{member}",collapse_home(&asar));
    let mut factories=snapshot(host,"@builtin/mcp-factories","Claude 内置 MCP 工厂",&c.version,PluginPresence::Bundled,source(".vite/build/index.chunk-BaALgXPu.js#lC"));
    for factory in &c.mcp_factories {
        let path=source(&format!("{}#{}",factory.source,factory.factory));
        component(&mut factories,PluginComponentKind::Mcp,&factory.name,path.clone(),Some(factory.name.clone()))?;
        component(&mut factories,PluginComponentKind::Code,&format!("{} · session gate",factory.name),path,None)?;
    }
    let mut skills=snapshot(host,"@builtin/skills","Claude 随包 Skills",&c.skills_ref,PluginPresence::Bundled,source(&c.skills_manifest));
    for name in &c.skills {
        component(&mut skills,PluginComponentKind::Skill,name,source(&format!("resources/bundled-skills/{name}.skill")),None)?;
    }
    let mut managed=snapshot(host,"@builtin/managed-connectors","Claude 管理型连接器适配器",&c.version,PluginPresence::Bundled,source(&c.managed_factory_source));
    for name in &c.managed_connector_kinds {
        // These spawners receive an administrator's server name at runtime.
        // Their adapter kind is not fabricated into a configured MCP identity.
        component(&mut managed,PluginComponentKind::Code,name,source(&c.managed_factory_source),None)?;
    }
    result.plugins.extend([factories,skills,managed]); Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    if bundled(host,result).is_err() { result.unavailable=true; }
    match native_roots(host) {
        Ok(roots)=>for (scope,root) in roots { if extensions(host,scope,&root,result).is_err() { result.unavailable=true; } },
        Err(())=>result.unavailable=true,
    }
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=vec![crate::paths::system_probe_path(&contract().app_path)];
    for (_,root) in native_roots(host).unwrap_or_default() {
        paths.extend([root.clone(),root.join("Claude Extensions"),root.join("extensions-installations.json")]);
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    fn host() -> &'static Host { hosts().iter().find(|h|h.agent_id=="claude-desktop").unwrap() }
    fn fixture(root: &Path, key: &str) {
        let path=root.join("Claude Extensions").join(key);fs::create_dir_all(&path).unwrap();
        fs::write(path.join("server.js"),"never executed").unwrap();
        fs::write(path.join("manifest.json"),serde_json::json!({"manifest_version":"0.4","name":"fixture","display_name":"Fixture","version":"1.2.3","description":"metadata","author":{"name":"fixture"},"server":{"type":"node","entry_point":"server.js","mcp_config":{"command":"secret-command","args":["secret-argument"],"env":{"TOKEN":"secret-env"}}},"user_config":{"TOKEN":{"sensitive":true}},"tools":[{"name":"tool"}],"prompts":[{"name":"prompt"}]}).to_string()).unwrap();
    }
    #[test]
    fn physical_extensions_remain_readonly_without_reading_settings_or_projecting_secrets() {
        let home=TestHome::new("claude-desktop-extension-metadata");fixture(&home.home,"local.fixture");
        fs::create_dir_all(home.home.join("Claude Extensions Settings")).unwrap();
        fs::write(home.home.join("Claude Extensions Settings/local.fixture.json"),"deliberately unreadable settings JSON secret-user-config").unwrap();
        let mut scan=PluginScan::default();extensions(host(),"standard",&home.home,&mut scan).unwrap();
        assert_eq!(scan.plugins.len(),1);let p=&scan.plugins[0];assert_eq!(p.presence,PluginPresence::Installed);assert_eq!(p.enabled,None);
        assert!(p.components.iter().any(|c|c.kind==PluginComponentKind::Mcp&&c.native_id.as_deref()==Some("extension:standard:local.fixture")));
        let encoded=serde_json::to_string(p).unwrap();for s in ["secret-command","secret-argument","secret-env","secret-user-config"] { assert!(!encoded.contains(s)); }
        assert!(!scan.unavailable);
    }
    #[test]
    fn stale_install_records_cannot_invent_capabilities_and_invalid_shapes_fail_closed() {
        let home=TestHome::new("claude-desktop-install-ledger");
        let p=home.home.join("extensions-installations.json");
        fs::write(&p,r#"{"extensions":{"local.missing":{"id":"local.missing","manifest":{"version":"9","server":{"type":"node"}},"userConfig":{"secret":"ignored"}}}}"#).unwrap();
        let mut scan=PluginScan::default();extensions(host(),"standard",&home.home,&mut scan).unwrap();
        assert_eq!(scan.plugins.len(),1);assert_eq!(scan.plugins[0].presence,PluginPresence::Declared);assert!(scan.plugins[0].components.is_empty());
        fs::write(&p,r#"{"extensions":{"../escape":{}}}"#).unwrap();assert!(extensions(host(),"standard",&home.home,&mut PluginScan::default()).is_err());
        fs::write(&p,r#"{"extensions":[]}"#).unwrap();assert!(extensions(host(),"standard",&home.home,&mut PluginScan::default()).is_err());
    }
    #[test]
    fn profile_identities_remain_independent_and_native_overrides_never_guess_a_relative_root() {
        let home=TestHome::new("claude-desktop-native-roots");
        std::env::remove_var("CLAUDE_USER_DATA_DIR");assert_eq!(native_roots(host()).unwrap().len(),2);
        let mut scan=PluginScan::default();
        for (scope,root) in [("standard",home.home.join("one")),("third-party",home.home.join("two"))] {
            fixture(&root,"local.same");extensions(host(),scope,&root,&mut scan).unwrap();
        }
        assert_eq!(scan.plugins.len(),2);assert_ne!(scan.plugins[0].plugin_id,scan.plugins[1].plugin_id);
        std::env::set_var("CLAUDE_USER_DATA_DIR","relative");assert!(native_roots(host()).is_err());
        let custom=home.home.join("custom");std::env::set_var("CLAUDE_USER_DATA_DIR",&custom);
        assert_eq!(native_roots(host()).unwrap(),vec![("override",custom)]);
    }
}
