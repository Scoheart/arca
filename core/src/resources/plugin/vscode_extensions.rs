//! Bundled VS Code extension metadata plus independently audited native facts.
//! No extension entrypoint, host command, native RPC or permission API runs.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    agent_id: String, bundle_id: String, product_name: String, data_folder: String,
    declared_lists: Vec<String>, native_components: Vec<NativeComponents>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeComponents {
    plugin_id: String, entrypoint: String, guards: Vec<Guard>, components: Vec<Declaration>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard { path: String, bytes: u64, sha256: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration { kind: PluginComponentKind, name: String, native_id: Option<String>, source: String }

fn profiles() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-vscode-hosts.json"))
        .expect("audited extension profiles must be valid"))
}

fn root_path(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) } else { expand_tilde(&source.path) }
}

fn select_app(host: &Host) -> Result<Option<PathBuf>, ()> {
    for source in &host.roots {
        if source.layout != "vscode_app" { return Err(()); }
        let path=root_path(source); checked_root(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => return Ok(Some(path)),
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => {},
            _ => return Err(()),
        }
    }
    Ok(None)
}

fn app_version(app: &Path, profile: &Profile, budget: &mut ScanBudget) -> Result<String, ()> {
    let path=relative(app,"Contents/Info.plist")?; budget.visit()?;
    let file=open_confined_file(&path)?;let metadata=file.metadata().map_err(|_| ())?;
    let limit=MAX_READ_BYTES.saturating_sub(budget.bytes).min(1_048_576);
    if !metadata.is_file() || metadata.len()>limit as u64 { return Err(()); }
    let mut bytes=Vec::new();let read=file.take(limit as u64+1).read_to_end(&mut bytes);budget.bytes+=bytes.len();read.map_err(|_| ())?;
    if bytes.len()>limit { return Err(()); }
    let value=plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|_| ())?;
    let value=value.as_dictionary().ok_or(())?;
    if value.get("CFBundleIdentifier").and_then(plist::Value::as_string)!=Some(profile.bundle_id.as_str())
        || value.get("CFBundleName").and_then(plist::Value::as_string)!=Some(profile.product_name.as_str()) { return Err(()); }
    value.get("CFBundleShortVersionString").and_then(plist::Value::as_string).and_then(metadata_name).ok_or(())
}

fn identifier(input: &str) -> Result<String, ()> {
    if input.is_empty() || input.len()>256 || !input.bytes().all(|ch|ch.is_ascii_alphanumeric()||b"._-".contains(&ch)) { return Err(()); }
    Ok(input.into())
}

fn package_id(value: &Value) -> Result<String, ()> {
    let publisher=identifier(value.get("publisher").and_then(Value::as_str).ok_or(())?)?;
    let name=identifier(value.get("name").and_then(Value::as_str).ok_or(())?)?;
    identifier(&format!("{publisher}.{name}"))
}

fn entrypoint_path(root: &Path, input: &str) -> Result<Option<PathBuf>, ()> {
    if input.chars().any(char::is_control) {return Err(());}
    let mut path=relative(root,input)?;
    if !regular_metadata_file(&path)? && path.extension().is_none() { path.set_extension("js"); }
    if regular_metadata_file(&path)? {Ok(Some(path))} else {Ok(None)}
}

fn entrypoint(root: &Path, input: &str) -> Result<PathBuf, ()> {
    entrypoint_path(root,input)?.ok_or(())
}

fn title(root: &Path, manifest: &Value, sources: &mut Vec<String>, result: &mut PluginScan) -> String {
    let fallback=manifest.get("name").and_then(Value::as_str).unwrap_or("unknown");
    let input=manifest.get("displayName").and_then(Value::as_str).unwrap_or(fallback);
    if let Some(key)=input.strip_prefix('%').and_then(|value|value.strip_suffix('%')) {
        match optional_json(root,"package.nls.json",&mut result.budget) {
            Ok(Some(value)) => if let Some(name)=value.get(key).and_then(|value|value.as_str().or_else(||value.get("message").and_then(Value::as_str))).and_then(metadata_name) {
                sources.push(collapse_home(&root.join("package.nls.json"))); return name;
            },
            Ok(None) => {},Err(()) => result.unavailable=true,
        }
        return metadata_name(fallback).unwrap_or_else(||"unknown".into());
    }
    metadata_name(input).unwrap_or_else(||fallback.into())
}

fn add(kind: PluginComponentKind, name: &str, path: &Path, items: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if items.len()>=MAX_COMPONENTS { return Err(()); }
    items.push(PluginComponent {kind,name:metadata_name(name).ok_or(())?,source_path:collapse_home(path),native_id:None});Ok(())
}

fn declarations(root: &Path, manifest: &Value, components: &mut Vec<PluginComponent>, result: &mut PluginScan) -> Result<(), ()> {
    let path=root.join("package.json");
    if let Some(value)=manifest.pointer("/contributes/commands") {
        let commands=value.as_array().filter(|items|items.len()<=MAX_COMPONENTS).ok_or(())?;
        for command in commands { add(PluginComponentKind::Command,command.get("command").and_then(Value::as_str).ok_or(())?,&path,components)?; }
    }
    if let Some(value)=manifest.pointer("/contributes/themes") {
        let themes=value.as_array().filter(|items|items.len()<=MAX_COMPONENTS).ok_or(())?;
        for theme in themes {
            let name=theme.get("id").and_then(Value::as_str).or_else(||theme.get("label").and_then(Value::as_str)).ok_or(())?;
            add(PluginComponentKind::Theme,name,&path,components)?;
        }
    }
    for (field,key,kind) in [("languageModelTools","name",PluginComponentKind::Tool),("chatOutputRenderers","viewType",PluginComponentKind::Renderer)] {
        if let Some(value)=manifest.get("contributes").and_then(|value|value.get(field)) {
            let items=value.as_array().filter(|items|items.len()<=MAX_COMPONENTS).ok_or(())?;
            for item in items {
                let name=metadata_name(item.get(key).and_then(Value::as_str).ok_or(())?).ok_or(())?;
                if components.len()>=MAX_COMPONENTS {return Err(());}
                components.push(PluginComponent {kind:kind.clone(),name:name.clone(),source_path:collapse_home(&path),native_id:Some(name)});
            }
        }
    }
    for field in ["main","browser"] {
        if let Some(input)=manifest.get(field) {
            match input.as_str().ok_or(()).and_then(|input|entrypoint_path(root,input)) {
                Ok(Some(path)) => add(PluginComponentKind::Code,field,&path,components)?,
                Ok(None) => add(PluginComponentKind::Code,&format!("{field} (declaration only): {}",input.as_str().ok_or(())?),&root.join("package.json"),components)?,
                Err(()) => result.unavailable=true,
            }
        }
    }
    Ok(())
}

fn native(root: &Path, manifest: &Value, definition: &NativeComponents, components: &mut Vec<PluginComponent>, sources: &mut Vec<String>, result: &mut PluginScan) -> Result<(), ()> {
    let main=entrypoint(root,manifest.get("main").and_then(Value::as_str).ok_or(())?)?;
    if main!=relative(root,&definition.entrypoint)? || definition.guards.is_empty() || definition.guards.len()>64 { return Err(()); }
    for guard in &definition.guards { verify_artifact(&relative(root,&guard.path)?,guard.bytes,&guard.sha256,&mut result.budget)?; }
    if definition.components.len()>MAX_COMPONENTS.saturating_sub(components.len()) { return Err(()); }
    let mut native_components=Vec::new();let mut native_sources=Vec::new();
    for declaration in &definition.components {
        if !definition.guards.iter().any(|guard|guard.path==declaration.source) { return Err(()); }
        native_components.push(PluginComponent {kind:declaration.kind.clone(),name:metadata_name(&declaration.name).ok_or(())?,
            source_path:collapse_home(&relative(root,&declaration.source)?),native_id:declaration.native_id.clone()});
    }
    for guard in &definition.guards { native_sources.push(collapse_home(&relative(root,&guard.path)?)); }
    components.extend(native_components);sources.extend(native_sources);
    Ok(())
}

fn package(host: &Host, profile: &Profile, root: &Path, version: &str, product_path: &Path, result: &mut PluginScan) -> Result<Option<String>, ()> {
    let Some(manifest)=optional_json(root,"package.json",&mut result.budget)? else {return Ok(None);};
    let id=package_id(&manifest)?;let package_version=manifest.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    let path=root.join("package.json");let mut sources=vec![collapse_home(product_path),collapse_home(&path)];let mut components=Vec::new();
    let name=title(root,&manifest,&mut sources,result);add(PluginComponentKind::Extension,&id,&path,&mut components)?;
    if declarations(root,&manifest,&mut components,result).is_err() {result.unavailable=true;}
    for definition in profile.native_components.iter().filter(|definition|definition.plugin_id==id) {
        if native(root,&manifest,definition,&mut components,&mut sources,result).is_err() {result.unavailable=true;}
    }
    components.sort();components.dedup();sources.sort();sources.dedup();
    let digest=hex::encode(Sha256::digest(serde_json::to_vec(&(root,version,&name,&package_version,&components)).map_err(|_| ())?));
    result.plugins.push(NativePluginObservation {agent_id:host.agent_id.clone(),plugin_id:id.clone(),name,version:package_version,
        revision:format!("host:{version}; metadata-sha256:{digest}"),presence:PluginPresence::Bundled,enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()});Ok(Some(id))
}

fn declared(product: &Value, profile: &Profile, result: &mut PluginScan) -> BTreeSet<String> {
    let mut names=BTreeSet::new();
    for key in &profile.declared_lists {
        let Some(items)=product.get(key).and_then(Value::as_array).filter(|items|items.len()<=MAX_ENTRIES) else {result.unavailable=true;continue;};
        for item in items {
            match item.get("id").and_then(Value::as_str).ok_or(()).and_then(identifier) {Ok(id)=>{names.insert(id);},Err(())=>result.unavailable=true}
        }
    }
    names
}

fn observe_profile(host: &Host, profile: &Profile, result: &mut PluginScan) -> Result<(), ()> {
    let Some(app)=select_app(host)? else {return Ok(());};let version=app_version(&app,profile,&mut result.budget)?;
    let resources=relative(&app,"Contents/Resources/app")?;let product_path=resources.join("product.json");
    let product=optional_json(&resources,"product.json",&mut result.budget)?.ok_or(())?;
    if product.get("nameShort").and_then(Value::as_str)!=Some(profile.product_name.as_str())
        || product.get("dataFolderName").and_then(Value::as_str)!=Some(profile.data_folder.as_str()) {return Err(());}
    let mut remaining=declared(&product,profile,result);
    for root in dirs(&resources.join("extensions"),&mut result.budget)? {
        match package(host,profile,&root,&version,&product_path,result) {
            Ok(Some(id))=>{remaining.remove(&id);},Ok(None)=>{},Err(())=>result.unavailable=true,
        }
    }
    for id in remaining {
        result.plugins.push(NativePluginObservation {agent_id:host.agent_id.clone(),plugin_id:id.clone(),name:id,version:"unknown".into(),
            revision:format!("host:{version}"),presence:PluginPresence::Declared,enabled:None,source_paths:vec![collapse_home(&product_path)],components:Vec::new(),docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
    }
    Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    if profiles().iter().find(|profile|profile.agent_id==host.agent_id).ok_or(()).and_then(|profile|observe_profile(host,profile,result)).is_err() {result.unavailable=true;}
}

pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=Vec::new();let mut budget=ScanBudget::default();let profile=profiles().iter().find(|profile|profile.agent_id==host.agent_id);
    for source in &host.roots {
        let app=root_path(source);paths.push(app.clone());paths.push(app.join("Contents/Info.plist"));
        let resources=app.join("Contents/Resources/app");paths.push(resources.join("product.json"));paths.push(resources.join("extensions"));
        let Ok(packages)=dirs(&resources.join("extensions"),&mut budget) else {continue;};
        for root in packages {
            paths.push(root.clone());paths.push(root.join("package.json"));paths.push(root.join("package.nls.json"));
            if let Ok(Some(manifest))=optional_json(&root,"package.json",&mut budget) {
                for key in ["main","browser"] {if let Some(input)=manifest.get(key).and_then(Value::as_str) {if let Ok(path)=relative(&root,input) {
                    if path.extension().is_none() {paths.push(path.with_extension("js"));}paths.push(path);
                }}}
                if let (Some(profile),Ok(id))=(profile,package_id(&manifest)) {
                    for definition in profile.native_components.iter().filter(|definition|definition.plugin_id==id) {
                        for guard in &definition.guards {if let Ok(path)=relative(&root,&guard.path) {paths.push(path);}}
                    }
                }
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use serde_json::json;

    fn host() -> &'static Host {hosts().iter().find(|host|host.agent_id=="trae-cn").unwrap()}
    fn write(root: &Path, input: &str, content: &[u8]) {let path=root.join(input);fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,content).unwrap();}
    fn fixture() -> (TestHome,PathBuf,Profile) {
        let home=TestHome::new("bundled-vscode-extensions");let app=root_path(&host().roots[0]);
        let mut plist=plist::Dictionary::new();for (key,value) in [("CFBundleIdentifier","cn.trae.app"),("CFBundleName","Trae CN"),("CFBundleShortVersionString","fixture")] {plist.insert(key.into(),plist::Value::String(value.into()));}
        fs::create_dir_all(app.join("Contents")).unwrap();plist::Value::Dictionary(plist).to_file_xml(app.join("Contents/Info.plist")).unwrap();
        let resources=app.join("Contents/Resources/app");write(&resources,"product.json",br#"{"nameShort":"Trae CN","dataFolderName":".trae-cn","traeBuiltinExtensions":[{"id":"example.native"},{"id":"missing.declared"}],"soloBuiltinExtensions":[{"id":"example.native"}]}"#);
        let profile=Profile {agent_id:"trae-cn".into(),bundle_id:"cn.trae.app".into(),product_name:"Trae CN".into(),data_folder:".trae-cn".into(),declared_lists:vec!["traeBuiltinExtensions".into(),"soloBuiltinExtensions".into()],native_components:Vec::new()};
        (home,resources,profile)
    }
    fn install(resources: &Path, folder: &str, manifest: Value) -> PathBuf {let path=resources.join("extensions").join(folder);write(&path,"package.json",manifest.to_string().as_bytes());path}
    fn scan(profile: &Profile) -> PluginScan {let mut result=PluginScan::default();if observe_profile(host(),profile,&mut result).is_err() {result.unavailable=true;}normalize(&mut result);result}

    #[test]
    fn all_bundled_packages_and_missing_product_declarations_stay_distinct_and_readonly() {
        let (_home,resources,profile)=fixture();
        let root=install(&resources,"native",json!({"publisher":"example","name":"native","version":"1","displayName":"%title%","main":"./extension","contributes":{"commands":[{"command":"native.action","title":"private title"}]}}));
        write(&root,"package.nls.json",br#"{"title":"Native display","unused":"private nls value"}"#);write(&root,"extension.js",b"\xffprivate executable body");
        install(&resources,"ordinary-language",json!({"publisher":"vscode","name":"language","version":"1"}));
        let result=scan(&profile);assert!(!result.unavailable);assert_eq!(result.plugins.len(),3);assert!(result.mcps.is_empty());
        assert_eq!(result.plugins.iter().filter(|plugin|plugin.presence==PluginPresence::Declared).count(),1);
        let native=result.plugins.iter().find(|plugin|plugin.plugin_id=="example.native").unwrap();assert_eq!(native.name,"Native display");assert_eq!(native.components.len(),3);
        assert!(result.plugins.iter().all(|plugin|plugin.enabled.is_none()&&plugin.agent_id=="trae-cn"));
        let output=serde_json::to_string(&result.plugins).unwrap();assert!(!output.contains("private"));assert!(!native.revision.contains(resources.to_str().unwrap()));
    }

    #[test]
    fn native_components_require_entrypoint_and_all_exact_artifact_guards() {
        let (_home,resources,mut profile)=fixture();let root=install(&resources,"native",json!({"publisher":"example","name":"native","version":"1","main":"extension.js"}));
        let body=b"inert audited source";write(&root,"extension.js",body);
        profile.native_components.push(NativeComponents {plugin_id:"example.native".into(),entrypoint:"extension.js".into(),guards:vec![Guard {path:"extension.js".into(),bytes:body.len() as u64,sha256:hex::encode(Sha256::digest(body))}],components:vec![
            Declaration {kind:PluginComponentKind::Mcp,name:"Native MCP".into(),native_id:Some("native-provider".into()),source:"extension.js".into()},
            Declaration {kind:PluginComponentKind::PluginReference,name:"Marketplace reference".into(),native_id:Some("public-package-id".into()),source:"extension.js".into()}]});
        let result=scan(&profile);assert!(!result.unavailable);assert_eq!(result.mcps.len(),1);assert_eq!(result.mcps[0].server_id,"native-provider");
        write(&root,"extension.js",b"registerMcpProvider('native-provider')");let result=scan(&profile);assert!(result.unavailable);assert!(result.mcps.is_empty());assert_eq!(result.plugins.len(),2);
        write(&root,"extension.js",body);write(&root,"other.js",body);write(&root,"package.json",br#"{"publisher":"example","name":"native","version":"1","main":"other.js"}"#);
        assert!(scan(&profile).mcps.is_empty());
    }

    #[test]
    fn wrong_product_identity_does_not_fall_through_to_another_installation() {
        let (_home,resources,profile)=fixture();write(&resources,"product.json",br#"{"nameShort":"TRAE","dataFolderName":".trae"}"#);
        let result=scan(&profile);assert!(result.unavailable);assert!(result.plugins.is_empty());
    }

    #[test]
    fn unmaterialized_browser_entries_remain_manifest_declarations() {
        let (_home,resources,profile)=fixture();install(&resources,"native",json!({"publisher":"example","name":"native","version":"1","browser":"dist/browser/extension"}));
        let result=scan(&profile);assert!(!result.unavailable);let package=result.plugins.iter().find(|plugin|plugin.plugin_id=="example.native").unwrap();
        let entry=package.components.iter().find(|component|component.kind==PluginComponentKind::Code).unwrap();assert!(entry.name.contains("declaration only"));assert!(entry.source_path.ends_with("package.json"));
        assert!(result.mcps.is_empty());assert!(watch_paths(host()).iter().any(|path|path.ends_with("dist/browser/extension.js")));
    }

    #[test]
    fn model_tools_and_chat_renderers_are_names_only_and_never_become_mcps() {
        let (_home,resources,profile)=fixture();install(&resources,"native",json!({"publisher":"example","name":"native","version":"1","contributes":{
            "languageModelTools":[{"name":"renderDiagram","modelDescription":"private prompt","inputSchema":{"TOKEN":"private schema"},"when":"private config"}],
            "chatOutputRenderers":[{"viewType":"chat.diagram","mimeTypes":["private mime"]}]}}));
        let result=scan(&profile);assert!(!result.unavailable);assert!(result.mcps.is_empty());let components=&result.plugins.iter().find(|plugin|plugin.plugin_id=="example.native").unwrap().components;
        assert!(components.iter().any(|component|component.kind==PluginComponentKind::Tool&&component.native_id.as_deref()==Some("renderDiagram")));
        assert!(components.iter().any(|component|component.kind==PluginComponentKind::Renderer&&component.name=="chat.diagram"));assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private"));
    }

    #[test]
    fn malformed_sibling_and_escaped_entry_preserve_independent_metadata() {
        let (_home,resources,profile)=fixture();write(&resources,"extensions/broken/package.json",b"malformed");
        install(&resources,"safe",json!({"publisher":"example","name":"native","version":"1","main":"../../outside.js"}));
        let result=scan(&profile);assert!(result.unavailable);assert_eq!(result.plugins.len(),2);assert!(result.plugins.iter().flat_map(|plugin|&plugin.components).all(|component|component.kind!=PluginComponentKind::Code));
    }

    #[cfg(unix)]
    #[test]
    fn native_artifact_symlinks_never_cross_the_package_boundary() {
        let (_home,resources,mut profile)=fixture();let root=install(&resources,"native",json!({"publisher":"example","name":"native","version":"1","main":"extension.js"}));
        write(&resources,"outside.js",b"inert");std::os::unix::fs::symlink(resources.join("outside.js"),root.join("extension.js")).unwrap();
        profile.native_components.push(NativeComponents {plugin_id:"example.native".into(),entrypoint:"extension.js".into(),guards:vec![Guard {path:"extension.js".into(),bytes:5,sha256:hex::encode(Sha256::digest(b"inert"))}],components:vec![]});
        let result=scan(&profile);assert!(result.unavailable);assert!(result.mcps.is_empty());assert_eq!(result.plugins.len(),2);
    }
}
