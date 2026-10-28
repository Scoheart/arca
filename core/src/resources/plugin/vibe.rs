//! Vibe's selected wheel-backed CLI and user-global plugin metadata.
//! No Python import, native client, version probe or component is executed.
use super::*;
mod packages;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    command:String, version:String, launcher_body_sha256:String, server_launcher_body_sha256:String,
    builtin:Bundled, members:Vec<Member>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundled {path:String,name:String,version:String,skills:Vec<String>}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member { path:String, bytes:u64, sha256:String }
struct Runtime { launcher:PathBuf, site:PathBuf }

fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-vibe.json")).expect("audited Vibe wheel contract"))
}

fn layout(c:&Contract,selected:&Path) -> Result<Runtime,()> {
    if !selected.is_absolute() || selected.to_str().is_none_or(|s|s.chars().any(char::is_control)) {return Err(());}
    let launcher=fs::canonicalize(selected).map_err(|_|())?;checked_root(&launcher)?;
    let content=text(&launcher,8192)?;let (shebang,body)=content.split_once('\n').ok_or(())?;
    let interpreter=Path::new(shebang.strip_prefix("#!").ok_or(())?);
    let bin=launcher.parent().ok_or(())?;let venv=bin.parent().ok_or(())?;
    if interpreter.parent()!=Some(bin) || !interpreter.file_name().and_then(|s|s.to_str()).is_some_and(|s|matches!(s,"python"|"python3") || s.strip_prefix("python3.").is_some_and(|v|!v.is_empty()&&v.bytes().all(|b|b.is_ascii_digit())))
        || hex::encode(Sha256::digest(body.as_bytes()))!=c.launcher_body_sha256 {return Err(());}
    let server=text(&bin.join("vibe-app-server"),8192)?;let (server_interpreter,server_body)=server.split_once('\n').ok_or(())?;
    if server_interpreter!=shebang || hex::encode(Sha256::digest(server_body.as_bytes()))!=c.server_launcher_body_sha256 {return Err(());}
    // A script-local module or explicit Python search override prevents a
    // passive association with the venv's site-packages. Never import to decide.
    if bin.join("vibe.py").symlink_metadata().is_ok()
        || bin.join("vibe").symlink_metadata().is_ok_and(|m|m.is_dir()||m.file_type().is_symlink())
        || ["PYTHONPATH","PYTHONHOME"].iter().any(|key|std::env::var_os(key).is_some_and(|v|!v.is_empty())) {return Err(());}
    let config=text(&venv.join("pyvenv.cfg"),8192)?;
    let field=|name:&str| -> Result<&str,()> {
        let values=config.lines().filter_map(|line|line.split_once('=')).filter(|(k,_)|k.trim()==name).map(|(_,v)|v.trim()).collect::<Vec<_>>();
        if values.len()==1 {Ok(values[0])}else{Err(())}
    };
    if field("implementation")?!="CPython" || field("include-system-site-packages")?!="false" {return Err(());}
    let version=field("version_info")?.split('.').collect::<Vec<_>>();
    if version.len()<2 || version[0]!="3" || version[1].is_empty() || !version[1].bytes().all(|b|b.is_ascii_digit()) || version[1].parse::<u32>().map_err(|_|())?<12 {return Err(());}
    let site=venv.join(format!("lib/python3.{}/site-packages",version[1]));checked_root(&site)?;
    Ok(Runtime {launcher,site})
}

fn verified_runtime(c:&Contract,selected:&Path,budget:&mut ScanBudget) -> Result<Runtime,()> {
    let runtime=layout(c,selected)?;
    for member in &c.members {verify_artifact(&relative(&runtime.site,&member.path)?,member.bytes,&member.sha256,budget)?;}
    let after=layout(c,selected)?;
    if after.launcher!=runtime.launcher || after.site!=runtime.site {return Err(());}
    Ok(runtime)
}

pub(super) fn runtime_version(selected:&Path) -> Option<String> {
    verified_runtime(contract(),selected,&mut ScanBudget::default()).ok().map(|_|contract().version.clone())
}

fn global_base(value:Option<&str>) -> Option<PathBuf> {
    let value=value.filter(|s|!s.is_empty())?;let p=if value=="~"{expand_tilde("~/")}else{expand_tilde(value)};
    (p.is_absolute()&&p.to_str().is_some_and(|s|!s.chars().any(char::is_control))).then_some(p)
}
fn global_plugins(host:&Host) -> Option<PathBuf> {
    if let Some(value)=std::env::var_os("VIBE_HOME").filter(|v|!v.is_empty()) {return global_base(value.to_str()).map(|base|base.join("plugins"));}
    let [source]=host.roots.as_slice()else{return None;};
    if source.layout!="vibe_global_plugins"||source.presence!=PluginPresence::Local{return None;}
    let root=expand_tilde(&source.path);(root.is_absolute()&&root.to_str().is_some_and(|s|!s.chars().any(char::is_control))).then_some(root)
}

pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    let c=contract();let mut candidates=Vec::new();
    let mut roots=Vec::new();
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        match verified_runtime(c,&selected,&mut ScanBudget::default()) {
            Ok(runtime)=>if bundled(host,c,&runtime,result).is_err(){result.unavailable=true;},
            Err(())=>result.unavailable=true,
        }
    }
    match global_plugins(host) {Some(root)=>roots.push((root,PluginPresence::Local)),None=>result.unavailable=true}
    for (root,presence) in roots {
        let entries=match directory_entries(&root,&mut result.budget) {Ok(entries)=>entries,Err(())=>{result.unavailable=true;continue;}};
        for entry in entries {
            if !entry.directory {if !entry.file {result.unavailable=true;}continue;}
            match packages::read_package(host,&entry.path,presence.clone(),result) {
                Ok(Some(candidate))=>candidates.push(candidate),Ok(None)=>{},Err(())=>result.unavailable=true,
            }
            if result.budget.visited>MAX_SCAN_ENTRIES || result.budget.references>MAX_REFERENCES || result.budget.bytes>=MAX_READ_BYTES {result.unavailable=true;break;}
        }
    }
    packages::resolve_candidates(candidates,result);
}

pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    let c=contract();let mut paths=crate::application::agent_launch::command_candidates(&c.command);
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        if let Ok(physical)=fs::canonicalize(&selected) {
            if let Some(bin)=physical.parent() {paths.push(bin.join("vibe-app-server"));if let Some(venv)=bin.parent(){paths.push(venv.join("pyvenv.cfg"));}}
            paths.push(physical);
        }
        if let Ok(runtime)=layout(c,&selected) {
            if let Some(venv)=runtime.launcher.parent().and_then(Path::parent) {paths.push(venv.join("pyvenv.cfg"));paths.push(venv.join("lib"));}
            paths.extend(c.members.iter().filter_map(|member|relative(&runtime.site,&member.path).ok()));
            if let Ok(root)=relative(&runtime.site,&c.builtin.path) {paths.push(root);}
            paths.push(runtime.launcher.parent().unwrap().join("vibe-app-server"));
        }
    }
    if let Some(root)=global_plugins(host) {paths.push(root);}
    paths.into_iter().filter(|p|p.to_str().is_some_and(|s|!s.chars().any(char::is_control))).collect::<BTreeSet<_>>().into_iter().collect()
}

fn bundled(host:&Host,c:&Contract,runtime:&Runtime,result:&mut PluginScan) -> Result<(),()> {
    let root=relative(&runtime.site,&c.builtin.path)?;let mut components=Vec::new();
    for skill in &c.builtin.skills {
        let p=relative(&root,&format!("skills/{skill}/SKILL.md"))?;
        if regular_metadata_file(&p)? {components.push(PluginComponent {kind:PluginComponentKind::Skill,name:skill.clone(),source_path:collapse_home(&p),native_id:None});}
        else {result.unavailable=true;}
    }
    // This is the audited shipped catalog. Added local files cannot impersonate
    // another bundled MCP or Skill; runtime selection is still unknown.
    for entry in directory_entries(&root.join("skills"),&mut result.budget)? {
        if !entry.directory || !entry.path.file_name().and_then(|s|s.to_str()).is_some_and(|s|c.builtin.skills.iter().any(|name|name==s)) {result.unavailable=true;}
    }
    if root.join("mcp.json").symlink_metadata().is_ok(){result.unavailable=true;}
    result.plugins.push(NativePluginObservation {agent_id:host.agent_id.clone(),plugin_id:format!("@builtin/{}",c.builtin.name),name:c.builtin.name.clone(),version:c.builtin.version.clone(),revision:format!("{}:{}",c.version,collapse_home(&root)),presence:PluginPresence::Bundled,enabled:None,source_paths:vec![collapse_home(&runtime.launcher),collapse_home(&root.join("plugin.json"))],components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()});Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[cfg(unix)]
    fn fixture(home: &TestHome) -> (Contract, PathBuf) {
        use std::os::unix::fs::{symlink,PermissionsExt};
        let venv=home.home.join("runtime/vibe");let bin=venv.join("bin");fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("python"),"#!/bin/sh\nprintf executed > \"$HOME/executed-marker\"\n").unwrap();
        fs::set_permissions(bin.join("python"),fs::Permissions::from_mode(0o755)).unwrap();
        let physical=bin.join("vibe");fs::write(&physical,format!("#!{}\nentry\n",bin.join("python").display())).unwrap();
        fs::write(bin.join("vibe-app-server"),format!("#!{}\nentry\n",bin.join("python").display())).unwrap();
        fs::set_permissions(&physical,fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(venv.join("pyvenv.cfg"),"implementation = CPython\nversion_info = 3.13.7\ninclude-system-site-packages = false\n").unwrap();
        let site=venv.join("lib/python3.13/site-packages");fs::create_dir_all(&site).unwrap();fs::write(site.join("identity"),"audited").unwrap();
        let launcher=home.home.join("bin/vibe");symlink(&physical,&launcher).unwrap();
        let c=Contract { command:"vibe".into(),version:"3.0.3".into(),launcher_body_sha256:"6e1c1326f48d0c74e7f089ddbb0fad5f3a3848f234f7ba0235d1ca29edbbb2cb".into(),server_launcher_body_sha256:"6e1c1326f48d0c74e7f089ddbb0fad5f3a3848f234f7ba0235d1ca29edbbb2cb".into(),
            builtin:Bundled {path:"bundle".into(),name:"vibe".into(),version:"1.0.0".into(),skills:Vec::new()},members:vec![Member {path:"identity".into(),bytes:7,sha256:"b19b2022fbca5eebe2f23af94ff755f30bbd176d59e5e1c746e11b0adde61a7a".into()}] };
        (c,launcher)
    }

    // Catches active --version fallback, package replacement, and borrowing a
    // second installation after the selected launcher fails its identity gate.
    #[cfg(unix)]
    #[test]
    fn selected_package_identity_and_version_lookup_are_inert() {
        let home=TestHome::new("vibe-passive");let (c,launcher)=fixture(&home);
        let runtime=verified_runtime(&c,&launcher,&mut ScanBudget::default()).unwrap();
        assert_eq!(runtime.site,home.home.join("runtime/vibe/lib/python3.13/site-packages"));
        assert_eq!(crate::application::agent_launch::runtime_version("mistral-vibe").unwrap(),None);
        assert!(!home.home.join("executed-marker").exists());
        fs::write(runtime.site.join("identity"),"changed").unwrap();
        assert!(verified_runtime(&c,&launcher,&mut ScanBudget::default()).is_err());
        assert_eq!(passive_runtime_version("mistral-vibe",&launcher),Some(None));
        assert!(!home.home.join("executed-marker").exists());
    }

    #[cfg(unix)]
    #[test]
    fn unsupported_launcher_and_venv_search_path_cannot_supply_a_version() {
        let home=TestHome::new("vibe-layout");let (c,launcher)=fixture(&home);
        let runtime=verified_runtime(&c,&launcher,&mut ScanBudget::default()).unwrap();
        let venv=runtime.launcher.parent().unwrap().parent().unwrap();
        fs::write(venv.join("pyvenv.cfg"),"implementation = CPython\nversion_info = 3.13.7\ninclude-system-site-packages = true\n").unwrap();
        assert!(verified_runtime(&c,&launcher,&mut ScanBudget::default()).is_err());
        fs::write(&runtime.launcher,"#!/bin/sh\nexit 0\n").unwrap();
        assert!(verified_runtime(&c,&launcher,&mut ScanBudget::default()).is_err());
        assert_eq!(global_base(Some("relative/path")),None);
        assert_eq!(global_base(Some("/absolute/vibe")),Some(PathBuf::from("/absolute/vibe")));
        assert!(!home.home.join("executed-marker").exists());
    }
}
