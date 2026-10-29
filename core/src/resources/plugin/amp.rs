//! Amp's guarded shipped code and inert global plugin entry/cache metadata.
//! Never runs Bun, the CLI, plugin code or a version probe; no private bodies.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    command:String, version:String, binary_bytes:u64, binary_sha256:String,
    entry_name:String, builtin_plugins:Vec<String>,
}

fn contract() -> &'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-amp.json"))
        .expect("audited Amp native distribution contract"))
}

fn verified_runtime(c:&Contract,selected:&Path,budget:&mut ScanBudget) -> Result<PathBuf,()> {
    if !selected.is_absolute() || selected.to_str().is_none_or(|v|v.chars().any(char::is_control)) {return Err(());}
    let physical=fs::canonicalize(selected).map_err(|_|())?;
    // The complete native artifact binds source, bytecode and native companion
    // bytes. The packaged source path comes from its audited Bun member table.
    verify_artifact(&physical,c.binary_bytes,&c.binary_sha256,budget)?;
    if fs::canonicalize(selected).map_err(|_|())?!=physical {return Err(());}
    Ok(physical)
}

pub(super) fn runtime_version(selected:&Path) -> Option<String> {
    verified_runtime(contract(),selected,&mut ScanBudget::default()).ok().map(|_|contract().version.clone())
}

fn absolute(value:&std::ffi::OsStr) -> Result<PathBuf,()> {
    let text=value.to_str().ok_or(())?;let path=PathBuf::from(value);
    if !path.is_absolute() || text.len()>4096 || text.chars().any(char::is_control) {return Err(());}
    checked_root(&path)?;Ok(path)
}

fn root(host:&Host,layout:&str,presence:PluginPresence,variable:&str,truthy:bool,suffix:&str) -> Result<PathBuf,()> {
    let sources=host.roots.iter().filter(|r|r.layout==layout&&r.presence==presence).collect::<Vec<_>>();
    let [source]=sources.as_slice()else{return Err(());};
    if let Some(value)=std::env::var_os(variable).filter(|v|!truthy||!v.is_empty()) {
        return Ok(absolute(&value)?.join(suffix));
    }
    absolute(expand_tilde(&source.path).as_os_str())
}
fn system_root(host:&Host) -> Result<PathBuf,()> {
    root(host,"amp_system_plugins",PluginPresence::Local,"XDG_CONFIG_HOME",true,"amp/plugins")
}
fn cache_root(host:&Host) -> Result<PathBuf,()> {
    root(host,"amp_global_cache",PluginPresence::CacheOnly,"XDG_CACHE_HOME",false,"amp/global-plugins")
}

fn bundled(host:&Host,c:&Contract,binary:&Path,result:&mut PluginScan) {
    let source=format!("{}!{}",collapse_home(binary),c.entry_name);
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:"@builtin/plugins".into(),name:"Amp bundled plugins".into(),
        version:c.version.clone(),revision:c.binary_sha256.clone(),presence:PluginPresence::Bundled,enabled:None,
        source_paths:vec![source.clone()],components:c.builtin_plugins.iter().map(|name|PluginComponent {
            kind:PluginComponentKind::Code,name:name.clone(),source_path:source.clone(),native_id:None,
        }).collect(),docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });
}

fn script(name:&str) -> bool {
    (name.ends_with(".ts")||name.ends_with(".js"))&&!name.ends_with(".test.ts")&&!name.ends_with(".test.js")
}
fn file_name(path:&Path) -> Result<&str,()> {path.file_name().and_then(|n|n.to_str()).ok_or(())}

fn marker(host:&Host,id:String,name:String,version:String,path:&Path,presence:PluginPresence,code:bool,label:&str,result:&mut PluginScan) {
    let source=collapse_home(path);
    result.plugins.push(NativePluginObservation {
        agent_id:host.agent_id.clone(),plugin_id:id,name,version,revision:source.clone(),presence,enabled:None,
        source_paths:vec![source.clone()],components:vec![PluginComponent {
            kind:if code{PluginComponentKind::Code}else{PluginComponentKind::PluginReference},
            name:label.into(),source_path:source,native_id:None,
        }],docs_url:host.docs_url.clone(),source_fingerprint:String::new(),
    });
}

fn directory_entry(path:&Path,budget:&mut ScanBudget) -> Result<Option<PathBuf>,()> {
    checked_root(path)?;
    for name in ["index.ts","index.js"] {
        budget.visit()?;let entry=path.join(name);
        match fs::symlink_metadata(&entry) {
            Ok(m) if m.file_type().is_file()=>{if regular_metadata_file(&entry)?{return Ok(Some(entry));}return Err(());},
            Ok(_)=>{},Err(e) if e.kind()==std::io::ErrorKind::NotFound=>{},Err(_)=>return Err(()),
        }
    }
    Ok(None)
}

fn local_plugins(host:&Host,root:&Path,verified:bool,result:&mut PluginScan) -> Result<(),()> {
    let entries=directory_entries(root,&mut result.budget)?;
    // Native root-file masking includes unsupported symlinks/special entries;
    // they cannot cause us to borrow a same-stem directory instead.
    let masked=entries.iter().filter(|e|!e.directory).filter_map(|e|file_name(&e.path).ok().filter(|s|script(s)))
        .filter_map(|s|Path::new(s).file_stem().and_then(|v|v.to_str()).map(str::to_owned)).collect::<BTreeSet<_>>();
    for entry in entries {
        let raw=file_name(&entry.path)?;
        let name=if entry.directory {raw.to_owned()}else {
            if !script(raw){continue;}
            entry.path.file_stem().and_then(|v|v.to_str()).ok_or(())?.to_owned()
        };
        if metadata_name(&name).as_deref()!=Some(name.as_str()) {result.unavailable=true;continue;}
        let (path,regular)=if entry.directory {
            if masked.contains(raw){continue;}
            match directory_entry(&entry.path,&mut result.budget) {
                Ok(Some(path))=>(path,true),Ok(None)=>continue,Err(())=>{result.unavailable=true;(entry.path.clone(),false)},
            }
        }else {(entry.path.clone(),entry.file&&regular_metadata_file(&entry.path).unwrap_or(false))};
        let code=verified&&regular;
        if !code{result.unavailable=true;}
        let tail=path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;
        marker(host,format!("@local/{tail}"),name,"unknown".into(),&path,
            if code{PluginPresence::Local}else{PluginPresence::Declared},code,
            if code{"Plugin entrypoint; runtime registrations unobserved"}else{"Amp runtime identity or entry type unverified"},result);
    }
    Ok(())
}

fn cache_name(value:&str) -> bool {
    !value.is_empty()&&value.encode_utf16().count()<=128&&!value.starts_with('.')
        &&!value.chars().any(|c|c<='\u{1f}'||matches!(c,'@'|'/'|'\\'|'<'|'>'|':'|'"'|'|'|'?'|'*'))
}
fn cache_version(value:&str) -> bool {
    !value.is_empty()&&value.len()<=64&&value.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'_'|b'-'))
}
fn cached_plugins(host:&Host,root:&Path,verified:bool,result:&mut PluginScan) -> Result<(),()> {
    for server in dirs(root,&mut result.budget)? {
        let server_name=file_name(&server)?;
        if !server_name.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'.'|b'-')) {continue;}
        for scope in ["user","workspace"] {
            for package in dirs(&server.join(scope),&mut result.budget)? {
                let Some((name,version))=file_name(&package)?.rsplit_once('@')else{continue;};
                if !cache_name(name)||!cache_version(version){continue;}
                if !verified{result.unavailable=true;}
                marker(host,format!("@cache/{server_name}/{scope}/{name}"),name.into(),version.into(),&package,
                    if verified{PluginPresence::CacheOnly}else{PluginPresence::Declared},false,
                    "Cached package; server selection and entrypoint unobserved",result);
            }
        }
    }
    Ok(())
}

fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan) {
    let mut verified=false;
    if let Some(selected)=crate::application::agent_launch::find_command(&c.command) {
        match verified_runtime(c,&selected,&mut ScanBudget::default()) {
            Ok(binary)=>{bundled(host,c,&binary,result);verified=true;},Err(())=>result.unavailable=true,
        }
    }
    match system_root(host) {
        Ok(root)=>if local_plugins(host,&root,verified,result).is_err(){result.unavailable=true;},Err(())=>result.unavailable=true,
    }
    match cache_root(host) {
        Ok(root)=>if cached_plugins(host,&root,verified,result).is_err(){result.unavailable=true;},Err(())=>result.unavailable=true,
    }
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {observe_contract(host,contract(),result);}

pub(super) fn watch_paths(host:&Host) -> Vec<PathBuf> {
    let mut paths=crate::application::agent_launch::command_candidates(&contract().command);
    if let Some(selected)=crate::application::agent_launch::find_command(&contract().command) {
        if let Ok(physical)=fs::canonicalize(selected){paths.push(physical);}
    }
    let mut budget=ScanBudget::default();
    if let Ok(root)=system_root(host) {
        paths.push(root.clone());
        if let Ok(entries)=directory_entries(&root,&mut budget) {
            for entry in entries {
                paths.push(entry.path.clone());
                if entry.directory {paths.extend([entry.path.join("index.ts"),entry.path.join("index.js")]);}
            }
        }
    }
    if let Ok(root)=cache_root(host) {
        paths.push(root.clone());
        if let Ok(servers)=dirs(&root,&mut budget) {
            for server in servers {paths.extend([server.clone(),server.join("user"),server.join("workspace")]);}
        }
    }
    paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests;
