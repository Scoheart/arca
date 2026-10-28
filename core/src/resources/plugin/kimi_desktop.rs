//! Kimi Code Desktop's independently guarded compiled catalog and registry.
//! Reads inert packed metadata only; no native helper or target code is run.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String,
    app_paths: Vec<String>, catalog_member: String, builtin_skills: Vec<String>, guards: Vec<Guard>,
}

fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-kimi-desktop.json"))
        .expect("audited Kimi Code Desktop contract"))
}

fn app_path(input: &str) -> PathBuf {
    if input.starts_with("~/") { expand_tilde(input) } else { crate::paths::system_probe_path(input) }
}

fn verified_app(c: &Contract, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    if !c.guards.iter().any(|g|g.path==c.catalog_member) || !c.guards.iter().any(|g|g.path=="package.json") { return Err(()); }
    for app in c.app_paths.iter().map(|p|app_path(p)) {
        match fs::symlink_metadata(&app) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue,
            Ok(m) if m.file_type().is_dir()=>{}, _=>return Err(()),
        }
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version { return Err(()); }
        // Bind the packed entry/loader members only. Unrelated unpacked native
        // companions and runtime/session behavior are outside this contract.
        guard_asar(&relative(&app,"Contents/Resources/app.asar")?,&c.guards,budget)?;
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version { return Err(()); }
        return Ok(Some(app));
    }
    Ok(None)
}

fn observe_contract(host: &Host, c: &Contract, result: &mut PluginScan) {
    if host.agent_id!=c.agent_id { result.unavailable=true;return; }
    let mut verified=false;
    match verified_app(c,&mut ScanBudget::default()) {
        Ok(Some(app))=>{
            let asar=collapse_home(&app.join("Contents/Resources/app.asar"));
            super::kimi::observe_compiled_skills(host,&c.version,format!("{asar}!{}",c.catalog_member),
                format!("{asar}!package.json"),&c.builtin_skills,result);
            verified=true;
        },
        Ok(None)=>{},Err(())=>result.unavailable=true,
    }
    // Shared storage/grammar does not let either host borrow the other's
    // runtime identity. An unverified desktop keeps registered roots declared.
    super::kimi::observe_registered(host,verified,result);
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    observe_contract(host,contract(),result);
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=super::kimi::registry_watch_paths(host);
    for app in contract().app_paths.iter().map(|p|app_path(p)) {
        paths.extend([app.clone(),app.join("Contents/Info.plist"),app.join("Contents/Resources/app.asar")]);
    }
    paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests;
