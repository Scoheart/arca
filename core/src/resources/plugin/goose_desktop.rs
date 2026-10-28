//! Goose's default desktop catalog and confined global Open Plugins metadata.
//! Native discovery/auto-update mutate private state and are never invoked.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};
use crate::domain::mcp::BuiltinMcpObservation;
use crate::resources::mcp::builtin::BuiltinMcpScan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String,
    app_paths: Vec<String>, docs_url: String, guards: Vec<Guard>, backend: Guard,
    catalog_member: String, platform_extensions: Vec<Extension>, mcp_extensions: Vec<Extension>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Extension { id: String, name: String }
fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-goose-desktop.json"))
        .expect("audited Goose Desktop contract"))
}
fn app_path(input: &str) -> PathBuf {
    if input.starts_with("~/") { expand_tilde(input) } else { crate::paths::system_probe_path(input) }
}
fn verified_app(c: &Contract, backend: bool, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    for app in c.app_paths.iter().map(|p| app_path(p)) {
        match fs::symlink_metadata(&app) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound => continue,
            Ok(m) if m.file_type().is_dir() => {}, _ => return Err(()),
        }
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version { return Err(()); }
        guard_asar(&relative(&app,"Contents/Resources/app.asar")?,&c.guards,budget)?;
        if backend { verify_artifact(&relative(&app,&c.backend.path)?,c.backend.bytes,&c.backend.sha256,budget)?; }
        return Ok(Some(app));
    }
    Ok(None)
}
pub(crate) fn builtin_observe(agents: &BTreeMap<String, AgentDefinition>) -> BuiltinMcpScan {
    let c=contract(); let mut scan=BuiltinMcpScan::default();
    if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return scan; }
    // Full native hashing has a separate finite budget, so this native package
    // cannot consume another host's private metadata or artifact allowance.
    let app=match verified_app(c,true,&mut ScanBudget::default()) {
        Ok(Some(app))=>app, Ok(None)=>return scan, Err(())=>{scan.unavailable=true;return scan;},
    };
    let asar=app.join("Contents/Resources/app.asar"); let binary=app.join(&c.backend.path);
    for extension in &c.mcp_extensions {
        let source=format!("{}#goose_mcp::BUILTIN_EXTENSIONS::{}",collapse_home(&binary),extension.id);
        scan.items.push(BuiltinMcpObservation {
            agent_id:c.agent_id.clone(),plugin_id:format!("goose-{}",extension.id),plugin_version:c.version.clone(),
            manifest_path:format!("{}!{}#PU",collapse_home(&asar),c.catalog_member),
            server_id:extension.id.clone(),name:extension.name.clone(),provider:c.product_name.clone(),
            source_path:source.clone(),app_version:c.version.clone(),docs_url:c.docs_url.clone(),
            source_fingerprint:hex::encode(Sha256::digest(format!("{}:{}:{source}",c.version,c.backend.sha256).as_bytes())),
        });
    }
    scan
}
pub(crate) fn builtin_watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    let c=contract(); if !agents.get(&c.agent_id).is_some_and(|a|a.enabled) { return Vec::new(); }
    c.app_paths.iter().map(|p|app_path(p)).flat_map(|app|[
        app.clone(),app.join("Contents/Info.plist"),app.join("Contents/Resources/app.asar"),app.join(&c.backend.path),
    ]).collect()
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let c=contract(); if host.agent_id!=c.agent_id { result.unavailable=true;return; }
    match verified_app(c,false,&mut result.budget) {
        Ok(Some(app))=>for extension in &c.platform_extensions {
            let source=format!("{}!{}#PU::{}",collapse_home(&app.join("Contents/Resources/app.asar")),c.catalog_member,extension.id);
            result.plugins.push(NativePluginObservation {
                agent_id:host.agent_id.clone(),plugin_id:format!("@builtin/{}",extension.id),name:extension.name.clone(),
                version:c.version.clone(),revision:c.guards[1].sha256.clone(),presence:PluginPresence::Bundled,enabled:None,
                source_paths:vec![source.clone()],components:vec![PluginComponent{kind:PluginComponentKind::Code,name:"Platform tools".into(),source_path:source,native_id:None}],
                docs_url:c.docs_url.clone(),source_fingerprint:String::new(),
            });
        },Ok(None)=>{},Err(())=>result.unavailable=true,
    }
    super::goose_plugins::observe(host,result);
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    super::goose_plugins::watch_paths(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_host_does_not_read_builtin_artifacts() {
        let _home=crate::testenv::TestHome::new("goose-desktop-disabled");
        assert!(builtin_observe(&BTreeMap::new()).items.is_empty());
        assert!(builtin_watch_paths(&BTreeMap::new()).is_empty());
    }
}
