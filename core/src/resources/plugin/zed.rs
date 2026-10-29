//! Zed's physical global extension manifests. No index rebuild or WASM loading.
use super::*;
use super::vscode_extensions as metadata;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract { bundle_id:String, bundle_name:String, audited_version:String, installed_root:String }
fn contract()->&'static Contract {
    static VALUE:OnceLock<Contract>=OnceLock::new();
    VALUE.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-zed.json")).expect("audited Zed descriptor must be valid"))
}
fn text(value:&Value,key:&str)->Result<String,()> {value.get(key).and_then(Value::as_str).and_then(metadata_name).ok_or(())}
fn id(value:&str)->Result<String,()> {
    let id=metadata::identifier(value)?;if matches!(id.as_str(),"."|".."){return Err(());}Ok(id)
}
fn add(items:&mut Vec<PluginComponent>,kind:PluginComponentKind,name:String,path:&Path,native_id:Option<String>)->Result<(),()> {
    if items.len()>=MAX_COMPONENTS{return Err(());}
    items.push(PluginComponent{kind,name,source_path:collapse_home(path),native_id});Ok(())
}
fn path_component(root:&Path,input:&str,kind:PluginComponentKind,items:&mut Vec<PluginComponent>,budget:&mut ScanBudget)->Result<(),()> {
    budget.visit()?;if input.chars().any(char::is_control){return Err(());}
    let path=relative(root,input)?;checked_root(&path)?;
    let value=fs::symlink_metadata(&path).map_err(|_|())?;
    if !value.is_dir()&&!value.file_type().is_file(){return Err(());}
    add(items,kind,metadata_name(input).ok_or(())?,&path,None)
}
fn map_components(value:&Value,field:&str,kind:PluginComponentKind,path:&Path,items:&mut Vec<PluginComponent>,budget:&mut ScanBudget)->Result<(),()> {
    let Some(input)=value.get(field)else{return Ok(());};
    for (name,entry) in input.as_object().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())? {
        budget.visit()?;if !entry.is_object(){return Err(());}
        let name=metadata_name(name).ok_or(())?;
        if field=="slash_commands"&&(entry.get("description").is_none_or(|v|!v.is_string())||entry.get("requires_argument").is_none_or(|v|!v.is_boolean())){return Err(());}
        add(items,kind.clone(),name.clone(),path,Some(name))?;
    }
    Ok(())
}
fn declared_paths(root:&Path,value:&Value,field:&str,kind:PluginComponentKind,legacy:bool,items:&mut Vec<PluginComponent>,budget:&mut ScanBudget)->Result<(),()> {
    let Some(input)=value.get(field)else{return Ok(());};
    let paths=if legacy {
        input.as_object().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())?.values().collect::<Vec<_>>()
    }else{input.as_array().filter(|v|v.len()<=MAX_COMPONENTS).ok_or(())?.iter().collect()};
    for input in paths {path_component(root,input.as_str().ok_or(())?,kind.clone(),items,budget)?;}
    Ok(())
}
fn components(root:&Path,path:&Path,value:&Value,legacy:bool,result:&mut PluginScan)->Vec<PluginComponent> {
    let mut items=Vec::new();
    for (field,kind) in [("themes",PluginComponentKind::Theme),("languages",PluginComponentKind::TypeDeclaration)] {
        if declared_paths(root,value,field,kind,legacy,&mut items,&mut result.budget).is_err(){result.unavailable=true;}
    }
    if let Some(grammars)=value.get("grammars") {
        match grammars.as_object().filter(|v|v.len()<=MAX_COMPONENTS) {
            Some(grammars)=>for name in grammars.keys(){if result.budget.visit().is_err()||metadata_name(name).ok_or(()).and_then(|name|add(&mut items,PluginComponentKind::TypeDeclaration,name.clone(),path,Some(name))).is_err(){result.unavailable=true;break;}},
            None=>result.unavailable=true,
        }
    }
    if !legacy {
        if let Some(library)=value.get("lib").filter(|v|!v.is_null()) {
            if !library.is_object(){result.unavailable=true;}
            if library.get("kind").and_then(Value::as_str).is_some_and(|v|v!="Rust"){result.unavailable=true;}
            if let Some(version)=library.get("version").filter(|v|!v.is_null()) {
                match version.as_str().and_then(metadata_name){Some(version)=>if add(&mut items,PluginComponentKind::TypeDeclaration,format!("WASM API {version} (declaration)"),path,None).is_err(){result.unavailable=true;},None=>result.unavailable=true}
            }
        }
        for (field,kind) in [("language_servers",PluginComponentKind::Lsp),("context_servers",PluginComponentKind::McpReference),("slash_commands",PluginComponentKind::Command),("debug_adapters",PluginComponentKind::TypeDeclaration),("debug_locators",PluginComponentKind::TypeDeclaration),("language_model_providers",PluginComponentKind::TypeDeclaration)] {
            if map_components(value,field,kind,path,&mut items,&mut result.budget).is_err(){result.unavailable=true;}
        }
        if declared_paths(root,value,"icon_themes",PluginComponentKind::Theme,false,&mut items,&mut result.budget).is_err(){result.unavailable=true;}
        if let Some(snippets)=value.get("snippets").filter(|v|!v.is_null()) {
            let paths=snippets.as_array().map(|v|v.iter().collect::<Vec<_>>()).unwrap_or_else(||vec![snippets]);
            if paths.len()>MAX_COMPONENTS {result.unavailable=true;}else{for path in paths {if path.as_str().ok_or(()).and_then(|p|path_component(root,p,PluginComponentKind::TypeDeclaration,&mut items,&mut result.budget)).is_err(){result.unavailable=true;}}}
        }
    }
    // Zed also discovers immediate package resource directories when rebuilding
    // its cache. Retain path metadata; names inside resource bodies stay unknown.
    for (folder,kind,directory) in [("languages",PluginComponentKind::TypeDeclaration,true),("themes",PluginComponentKind::Theme,false),("icon_themes",PluginComponentKind::Theme,false)] {
        let base=root.join(folder);
        let entries=match directory_entries(&base,&mut result.budget){Ok(value)=>value,Err(())=>{result.unavailable=true;continue;}};
        for entry in entries {
            let outcome=(||{
                if !entry.directory&&!entry.file{return Err(());}
                if (directory&&!entry.directory)||(!directory&&(!entry.file||entry.path.extension().and_then(|v|v.to_str())!=Some("json"))){return Ok(());}
                let relative=entry.path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?;
                path_component(root,relative,kind.clone(),&mut items,&mut result.budget)
            })();if outcome.is_err(){result.unavailable=true;}
        }
    }
    let wasm=root.join("extension.wasm");
    match regular_metadata_file(&wasm) {
        Ok(true)=>{if add(&mut items,PluginComponentKind::Code,"extension.wasm (availability)".into(),&wasm,None).is_err(){result.unavailable=true;}},
        Ok(false) if value.pointer("/lib/kind").is_some_and(|v|!v.is_null())=>result.unavailable=true,
        Ok(false)=>{},Err(())=>result.unavailable=true,
    }
    items.sort();items.dedup();items
}
fn package(host:&Host,root:&Path,app:&Path,version:&str,native:bool,result:&mut PluginScan)->Result<Option<NativePluginObservation>,()> {
    let folder=id(root.file_name().and_then(|v|v.to_str()).ok_or(())?)?;
    let (value,path,legacy)=if let Some(value)=optional_document(root,"extension.toml",DocumentFormat::Toml,&mut result.budget)? {(value,root.join("extension.toml"),false)}
        else if let Some(value)=optional_json(root,"extension.json",&mut result.budget)?{(value,root.join("extension.json"),true)}else{return Ok(None);};
    let name=text(&value,"name")?;let package_version=text(&value,"version")?;
    let identity=if legacy {folder.clone()}else{id(value.get("id").and_then(Value::as_str).ok_or(())?)?};
    if identity!=folder{return Err(());}
    let known_schema=legacy||value.get("schema_version").and_then(Value::as_i64)==Some(1);
    let mut items=vec![PluginComponent{kind:PluginComponentKind::Extension,name:identity.clone(),source_path:collapse_home(&path),native_id:Some(identity.clone())}];
    if native&&known_schema {items.extend(components(root,&path,&value,legacy,result));}else{result.unavailable=true;}
    if items.len()>MAX_COMPONENTS{return Err(());}
    items.sort();items.dedup();let revision=hex::encode(Sha256::digest(serde_json::to_vec(&(root,version,&name,&package_version,&items)).map_err(|_|())?));
    Ok(Some(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:identity,name,version:package_version,revision:format!("host:{version}; metadata-sha256:{revision}"),presence:PluginPresence::Installed,enabled:None,
        source_paths:vec![collapse_home(&app.join("Contents/Info.plist")),collapse_home(&path)],components:items,docs_url:host.docs_url.clone(),source_fingerprint:String::new()}))
}
fn scan(host:&Host,c:&Contract,result:&mut PluginScan)->Result<(),()> {
    let Some(app)=metadata::select_app_for_layout(host,"zed_app")?else{return Ok(());};
    let version=electron_package::app_version(&app,&c.bundle_id,&c.bundle_name,&mut result.budget)?;
    let installed=expand_tilde(&c.installed_root);checked_root(&installed)?;
    for root in dirs(&installed,&mut result.budget)? {
        match package(host,&root,&app,&version,version==c.audited_version,result){Ok(Some(item))=>result.plugins.push(item),Ok(None)=>{},Err(())=>result.unavailable=true}
    }
    Ok(())
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    let mut local=PluginScan::default();if scan(host,contract(),&mut local).is_err(){local.unavailable=true;}
    result.plugins.extend(local.plugins);result.unavailable|=local.unavailable;
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf> {
    let mut paths=BTreeSet::new();let mut budget=ScanBudget::default();
    for source in &host.roots {let app=metadata::root_path(source);paths.insert(app.join("Contents/Info.plist"));paths.insert(app);}
    let installed=expand_tilde(&contract().installed_root);paths.insert(installed.clone());
    if let Ok(roots)=dirs(&installed,&mut budget){for root in roots {
        paths.insert(root.clone());for file in ["extension.toml","extension.json","extension.wasm","languages","themes","icon_themes"]{paths.insert(root.join(file));}
        for (name,format) in [("extension.toml",DocumentFormat::Toml),("extension.json",DocumentFormat::Json)] {
            match optional_document(&root,name,format,&mut budget){Ok(Some(value))=>{
                for field in ["languages","themes","icon_themes","snippets"] {
                    if let Some(value)=value.get(field) {
                        let values=if let Some(values)=value.as_array(){values.iter().collect::<Vec<_>>()}else if value.is_string(){vec![value]}else{value.as_object().map(|v|v.values().collect()).unwrap_or_default()};
                        for value in values {if budget.visit().is_err(){break;}if let Some(input)=value.as_str(){if let Ok(path)=relative(&root,input){paths.insert(path);}}}
                    }
                }break;
            },Ok(None)=>{},Err(())=>break}
        }
    }}paths.into_iter().collect()
}

#[cfg(test)]
mod tests;
