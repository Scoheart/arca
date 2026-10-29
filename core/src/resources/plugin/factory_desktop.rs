//! Factory Desktop's own packed loader and bundled Droid, observed passively.
//! Shared user registrations do not imply shared runtime identity or activity.
use super::*;
use super::electron_package::{app_version,Guard};
use std::ffi::OsStr;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id:String,version:String,bundle_id:String,product_name:String,
    app_paths:Vec<String>,guards:Vec<Guard>,
}
fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-factory-desktop.json"))
        .expect("audited Factory Desktop distribution contract"))
}
fn app_path(input:&str) -> PathBuf {
    if input.starts_with("~/"){expand_tilde(input)}else{crate::paths::system_probe_path(input)}
}
fn unchanged(before:&fs::Metadata,after:&fs::Metadata) -> bool {
    if !after.file_type().is_file()||before.len()!=after.len()
        ||before.modified().ok()!=after.modified().ok(){return false;}
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if before.dev()!=after.dev()||before.ino()!=after.ino()
            ||before.ctime()!=after.ctime()||before.ctime_nsec()!=after.ctime_nsec(){return false;}
    }
    true
}
fn verified_app(c:&Contract,budget:&mut ScanBudget) -> Result<Option<PathBuf>,()> {
    let expected=BTreeSet::from(["Contents/Resources/app.asar","Contents/Resources/bin/droid"]);
    if c.guards.len()!=2||c.guards.iter().map(|g|g.path.as_str()).collect::<BTreeSet<_>>()!=expected{return Err(());}
    for app in c.app_paths.iter().map(|p|app_path(p)) {
        match fs::symlink_metadata(&app) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>continue,
            Ok(m) if m.file_type().is_dir()=>{},_=>return Err(()),
        }
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version{return Err(());}
        let files=c.guards.iter().map(|g|relative(&app,&g.path)).collect::<Result<Vec<_>,_>>()?;
        let before=files.iter().map(|p|fs::symlink_metadata(p).map_err(|_|())).collect::<Result<Vec<_>,_>>()?;
        for (guard,path) in c.guards.iter().zip(&files){verify_artifact(path,guard.bytes,&guard.sha256,budget)?;}
        if app_version(&app,&c.bundle_id,&c.product_name,budget)?!=c.version{return Err(());}
        for (path,before) in files.iter().zip(&before) {
            if !unchanged(before,&fs::symlink_metadata(path).map_err(|_|())?){return Err(());}
        }
        return Ok(Some(app));
    }
    Ok(None)
}
fn base_from_values(home:Option<&OsStr>,fallback:Option<&Path>,home_override:Option<&OsStr>) -> Result<PathBuf,()> {
    // The packaged daemon explicitly overwrites FACTORY_ENV with production;
    // Desktop must not borrow the CLI's inherited development selection.
    super::factory_droid::base_from_env(home,fallback,home_override,Some(OsStr::new("production")))
}
fn native_base() -> Result<PathBuf,()> {
    let home=std::env::var_os("HOME");let fallback=dirs::home_dir();let home_override=std::env::var_os("FACTORY_HOME_OVERRIDE");
    base_from_values(home.as_deref(),fallback.as_deref(),home_override.as_deref())
}
fn observe_contract_at(host:&Host,c:&Contract,base:&Path,result:&mut PluginScan) {
    if host.agent_id!=c.agent_id{result.unavailable=true;return;}
    // Shipped byte verification has its own finite allowance, independent of
    // private metadata and other installed hosts. No native addon is loaded.
    match verified_app(c,&mut ScanBudget::default()) {
        Ok(Some(_))=>super::factory_droid::observe_registered(host,base,result),
        Ok(None)=>{},Err(())=>result.unavailable=true,
    }
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    match native_base(){Ok(base)=>observe_contract_at(host,contract(),&base,result),Err(())=>result.unavailable=true,}
}
fn app_watch_paths(c:&Contract) -> Vec<PathBuf> {
    c.app_paths.iter().map(|p|app_path(p)).flat_map(|app|[
        app.clone(),app.join("Contents/Info.plist"),app.join("Contents/Resources/app.asar"),app.join("Contents/Resources/bin/droid"),
    ]).collect()
}
fn watch_contract_at(host:&Host,c:&Contract,base:&Path) -> Vec<PathBuf> {
    let mut paths=app_watch_paths(c);
    let bound=host.agent_id==c.agent_id&&verified_app(c,&mut ScanBudget::default()).is_ok_and(|p|p.is_some());
    paths.extend(super::factory_droid::registry_watch_paths(host,base,bound));paths.sort();paths.dedup();paths
}
pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    match native_base(){Ok(base)=>watch_contract_at(host,contract(),&base),Err(())=>app_watch_paths(contract()),}
}

#[cfg(test)]
mod tests;
