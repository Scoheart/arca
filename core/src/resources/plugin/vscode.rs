//! VS Code package/registration metadata, independent of runtime extension state.
//! Public host digests bind native field meanings. No extension or loader runs.
use super::*;
use super::electron_package::{app_version, Guard};
use super::vscode_extensions as metadata;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    bundle_id: String, product_name: String, data_folder: String, version: String,
    environment: Guard, workbench: Guard,
}
fn contract() -> &'static Contract {
    static CONTRACT:OnceLock<Contract>=OnceLock::new();
    CONTRACT.get_or_init(||serde_json::from_str(include_str!("../../../../data/native-plugin-vscode.json"))
        .expect("audited VS Code metadata contract must be valid"))
}

fn extensions_root(c:&Contract)->Result<PathBuf,()> {
    if std::env::var_os("VSCODE_DEV").is_some_and(|value|!value.is_empty()){return Err(());}
    for (key,portable) in [("VSCODE_EXTENSIONS",false),("VSCODE_PORTABLE",true)] {
        if let Some(value)=std::env::var_os(key).filter(|value|!value.is_empty()) {
            let path=PathBuf::from(value);
            if !path.is_absolute()||path.to_str().is_none_or(|value|value.chars().any(char::is_control)){return Err(());}
            let path=if portable {path.join("extensions")}else{path};checked_root(&path)?;return Ok(path);
        }
    }
    let path=expand_tilde(&format!("~/{}/extensions",c.data_folder));checked_root(&path)?;Ok(path)
}

struct Registration { id:String, version:String, root:Option<PathBuf> }
fn registered_path(root:&Path,row:&Value)->Result<PathBuf,()> {
    if let Some(value)=row.get("relativeLocation").and_then(Value::as_str).filter(|value|!value.is_empty()) {
        if value.chars().any(char::is_control){return Err(());}
        return relative(root,value);
    }
    let location=row.get("location").and_then(Value::as_object).ok_or(())?;
    if location.get("scheme").and_then(Value::as_str)!=Some("file")
        || ["authority","query","fragment"].iter().any(|key|location.get(*key).and_then(Value::as_str).is_some_and(|value|!value.is_empty())) {return Err(());}
    let value=location.get("path").and_then(Value::as_str).ok_or(())?;
    if value.chars().any(char::is_control){return Err(());}
    let path=Path::new(value);if !path.is_absolute(){return Err(());}
    // External and linked installations require their own reviewed scope.
    relative(root,path.strip_prefix(root).map_err(|_|())?.to_str().ok_or(())?)
}
fn registrations(root:&Path,value:&Value)->Result<Vec<Registration>,()> {
    let items=value.as_array().filter(|rows|rows.len()<=MAX_ENTRIES).ok_or(())?;
    let mut rows=Vec::new();
    for row in items {
        let id=metadata::identifier(row.pointer("/identifier/id").and_then(Value::as_str).ok_or(())?)?;
        let version=row.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
        let location=row.get("location").ok_or(())?;
        if !(location.as_str().is_some_and(|value|!value.is_empty())
            ||location.as_object().is_some_and(|value|value.get("path").is_some_and(Value::is_string)&&value.get("scheme").is_some_and(Value::is_string)))
            ||row.get("relativeLocation").is_some_and(|value|!value.is_string()) {return Err(());}
        rows.push(Registration{id,version,root:registered_path(root,row).ok()});
    }
    Ok(rows)
}

fn component_id(value:&Value,key:&str)->Result<String,()> {
    let value=value.get(key).and_then(Value::as_str).ok_or(())?;
    metadata_name(value).ok_or(())?;Ok(value.into())
}
fn field_items<'a>(manifest:&'a Value,field:&str)->Result<Option<&'a Vec<Value>>,()> {
    manifest.get("contributes").and_then(|value|value.get(field))
        .map(|value|value.as_array().filter(|items|items.len()<=MAX_COMPONENTS).ok_or(())).transpose()
}
fn file_components(root:&Path,manifest:&Value,field:&str,kind:PluginComponentKind,result:&mut PluginScan)->Result<Vec<PluginComponent>,()> {
    let Some(items)=field_items(manifest,field)? else{return Ok(Vec::new());};
    let mut components=Vec::new();
    for item in items {
        result.budget.visit()?;
        let component=(||{
            let name=component_id(item,"name")?;
            if !name.bytes().all(|ch|ch.is_ascii_alphanumeric()||b"._-".contains(&ch))
                ||item.get("description").and_then(Value::as_str).is_none_or(|value|value.is_empty()){return Err(());}
            let input=item.get("path").and_then(Value::as_str).ok_or(())?;
            if input.chars().any(char::is_control){return Err(());}
            let path=relative(root,input)?;
            if !regular_metadata_file(&path)? {return Err(());}
            Ok(PluginComponent{kind:kind.clone(),name:name.clone(),source_path:collapse_home(&path),native_id:Some(name)})
        })();
        match component {Ok(component)=>components.push(component),Err(())=>result.unavailable=true}
    }
    Ok(components)
}
fn provider_components(root:&Path,manifest:&Value,budget:&mut ScanBudget)->Result<Vec<PluginComponent>,()> {
    let Some(items)=field_items(manifest,"mcpServerDefinitionProviders")? else{return Ok(Vec::new());};
    let mut components=Vec::new();
    for item in items {
        budget.visit()?;
        let id=component_id(item,"id")?;let name=component_id(item,"label")?;
        if item.get("when").is_some_and(|value|value.as_str().and_then(metadata_name).is_none()){return Err(());}
        components.push(PluginComponent{kind:PluginComponentKind::McpReference,name,native_id:Some(id),
            source_path:format!("{}#/contributes/mcpServerDefinitionProviders",collapse_home(&root.join("package.json")))});
    }
    Ok(components)
}
fn tool_components(root:&Path,manifest:&Value,field:&str,toolset:bool,result:&mut PluginScan)->Result<Vec<PluginComponent>,()> {
    let Some(items)=field_items(manifest,field)? else{return Ok(Vec::new());};
    let mut components=Vec::new();
    for item in items {
        result.budget.visit()?;
        let component=(||{
            let name=component_id(item,"name")?;
            if !name.bytes().all(|ch|ch.is_ascii_alphanumeric()||b"_-".contains(&ch)){return Err(());}
            if toolset {
                if item.get("description").and_then(Value::as_str).is_none_or(|value|value.is_empty())
                    ||item.get("tools").and_then(Value::as_array).is_none_or(|items|items.is_empty()||items.len()>MAX_COMPONENTS||items.iter().any(|value|!value.is_string())){return Err(());}
            }else if ["displayName","modelDescription"].iter().any(|key|item.get(*key).and_then(Value::as_str).is_none_or(|value|value.is_empty())){return Err(());}
            Ok(PluginComponent{kind:if toolset{PluginComponentKind::TypeDeclaration}else{PluginComponentKind::Tool},
                name:name.clone(),native_id:Some(name),
                source_path:format!("{}#/contributes/{field}",collapse_home(&root.join("package.json")))})
        })();
        match component {Ok(component)=>components.push(component),Err(())=>result.unavailable=true}
    }
    Ok(components)
}
fn native_components(root:&Path,manifest:&Value,capacity:usize,result:&mut PluginScan)->Vec<PluginComponent> {
    let mut components=Vec::new();
    for items in [
        file_components(root,manifest,"chatPromptFiles",PluginComponentKind::Command,result),
        file_components(root,manifest,"chatInstructions",PluginComponentKind::Rule,result),
        file_components(root,manifest,"chatAgents",PluginComponentKind::Agent,result),
        provider_components(root,manifest,&mut result.budget),tool_components(root,manifest,"languageModelTools",false,result),
        tool_components(root,manifest,"languageModelToolSets",true,result),
    ] {
        match items {Ok(items) if items.len()<=capacity.saturating_sub(components.len())=>components.extend(items),_=>result.unavailable=true}
    }
    components
}

fn package(host:&Host,root:&Path,version:&str,mut sources:Vec<String>,presence:PluginPresence,native:bool,result:&mut PluginScan)->Result<Option<NativePluginObservation>,()> {
    let Some(manifest)=optional_json(root,"package.json",&mut result.budget)? else{return Ok(None);};
    let id=metadata::package_id(&manifest)?;let package_version=manifest.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let path=root.join("package.json");sources.push(collapse_home(&path));
    let name=metadata::title(root,&manifest,&mut sources,result);
    let mut components=vec![PluginComponent{kind:PluginComponentKind::Extension,name:id.clone(),source_path:collapse_home(&path),native_id:Some(id.clone())}];
    for field in ["main","browser"] {
        if let Some(input)=manifest.get(field) {
            match input.as_str().ok_or(()).and_then(|value|metadata::entrypoint_path(root,value)) {
                Ok(file)=>components.push(PluginComponent{kind:PluginComponentKind::Code,name:if file.is_some(){field.into()}else{format!("{field} (declaration only)")},
                    source_path:collapse_home(file.as_deref().unwrap_or(&path)),native_id:Some(field.into())}),
                Err(())=>result.unavailable=true,
            }
        }
    }
    if native {let capacity=MAX_COMPONENTS.saturating_sub(components.len());components.extend(native_components(root,&manifest,capacity,result));}
    components.sort();components.dedup();sources.sort();sources.dedup();
    let digest=hex::encode(Sha256::digest(serde_json::to_vec(&(root,version,&package_version,&components)).map_err(|_|())?));
    Ok(Some(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:id,name,version:package_version,
        revision:format!("host:{version}; metadata-sha256:{digest}"),presence,enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()}))
}
fn unresolved(host:&Host,row:Registration,ledger:&Path,version:&str)->NativePluginObservation {
    NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:row.id.clone(),name:row.id,version:row.version,
        revision:format!("host:{version}; unresolved-registration"),presence:PluginPresence::Declared,enabled:None,
        source_paths:vec![collapse_home(ledger)],components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new()}
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan)->Result<(),()> {
    let Some(app)=metadata::select_app(host)? else{return Ok(());};
    let version=app_version(&app,&c.bundle_id,&c.product_name,&mut result.budget)?;
    let resources=relative(&app,"Contents/Resources/app")?;let product=optional_json(&resources,"product.json",&mut result.budget)?.ok_or(())?;
    let package_json=optional_json(&resources,"package.json",&mut result.budget)?.ok_or(())?;
    if product.get("nameShort").and_then(Value::as_str)!=Some(c.product_name.as_str())
        ||product.get("dataFolderName").and_then(Value::as_str)!=Some(c.data_folder.as_str())
        ||package_json.get("version").and_then(Value::as_str)!=Some(version.as_str()){return Err(());}
    let guard=|g:&Guard,budget:&mut ScanBudget|version==c.version&&relative(&resources,&g.path)
        .and_then(|path|verify_artifact(&path,g.bytes,&g.sha256,budget)).is_ok();
    let environment=guard(&c.environment,&mut result.budget);let native=guard(&c.workbench,&mut result.budget);
    if !environment||!native {result.unavailable=true;}
    let mut sources=vec![collapse_home(&app.join("Contents/Info.plist")),collapse_home(&resources.join("product.json")),collapse_home(&resources.join("package.json"))];
    if native {sources.push(collapse_home(&resources.join(&c.workbench.path)));}
    if environment {sources.push(collapse_home(&resources.join(&c.environment.path)));}
    for root in dirs(&resources.join("extensions"),&mut result.budget)? {
        match package(host,&root,&version,sources.clone(),PluginPresence::Bundled,native,result) {
            Ok(Some(item))=>result.plugins.push(item),Ok(None)=>{},Err(())=>result.unavailable=true,
        }
    }
    if !environment{return Ok(());}
    let root=extensions_root(c)?;let ledger=root.join("extensions.json");
    let Some(value)=optional_json(&root,"extensions.json",&mut result.budget)? else{return Ok(());};
    let rows=registrations(&root,&value)?;
    for row in rows {
        let mut registered_sources=sources.clone();registered_sources.push(collapse_home(&ledger));
        let observed=row.root.as_ref().and_then(|path|package(host,path,&version,registered_sources,PluginPresence::Installed,native,result).ok().flatten());
        if let Some(item)=observed.filter(|item|item.plugin_id.eq_ignore_ascii_case(&row.id)&&item.version==row.version) {result.plugins.push(item);}
        else {result.unavailable=true;result.plugins.push(unresolved(host,row,&ledger,&version));}
    }
    Ok(())
}
fn observe_isolated(host:&Host,c:&Contract,result:&mut PluginScan) {
    // This physical host has its own finite metadata/hash/entry allowances.
    // Large preceding clients must not decide which VS Code packages exist.
    let mut local=PluginScan::default();
    if observe_contract(host,c,&mut local).is_err(){local.unavailable=true;}
    result.plugins.extend(local.plugins);result.unavailable|=local.unavailable;
}
pub(super) fn observe(host:&Host,result:&mut PluginScan) {
    observe_isolated(host,contract(),result);
}

fn package_watches(root:&Path,paths:&mut BTreeSet<PathBuf>,budget:&mut ScanBudget) {
    if budget.visit().is_err(){return;}
    paths.insert(root.into());paths.insert(root.join("package.json"));paths.insert(root.join("package.nls.json"));
    if let Ok(Some(manifest))=optional_json(root,"package.json",budget) {
        for field in ["chatPromptFiles","chatInstructions","chatAgents"] {
            if let Ok(Some(items))=field_items(&manifest,field) {for item in items {if budget.visit().is_err(){return;}if let Some(input)=item.get("path").and_then(Value::as_str).filter(|input|!input.chars().any(char::is_control)) {if let Ok(path)=relative(root,input){paths.insert(path);}}}}
        }
        for field in ["main","browser"] {if let Some(input)=manifest.get(field).and_then(Value::as_str) {if let Ok(path)=relative(root,input){if path.extension().is_none(){paths.insert(path.with_extension("js"));}paths.insert(path);}}}
    }
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf> {
    let c=contract();let mut paths=BTreeSet::new();let mut budget=ScanBudget::default();
    for source in &host.roots {
        let app=metadata::root_path(source);paths.insert(app.join("Contents/Info.plist"));paths.insert(app.clone());
        let resources=app.join("Contents/Resources/app");
        for input in ["product.json","package.json",&c.environment.path,&c.workbench.path,"extensions"]{paths.insert(resources.join(input));}
        if let Ok(roots)=dirs(&resources.join("extensions"),&mut budget){for root in roots {package_watches(&root,&mut paths,&mut budget);}}
    }
    if let Ok(root)=extensions_root(c) {
        paths.insert(root.clone());paths.insert(root.join("extensions.json"));
        if let Ok(Some(value))=optional_json(&root,"extensions.json",&mut budget){if let Ok(rows)=registrations(&root,&value){for path in rows.into_iter().filter_map(|row|row.root){package_watches(&path,&mut paths,&mut budget);}}}
    }
    paths.into_iter().collect()
}

#[cfg(test)]
mod tests;
