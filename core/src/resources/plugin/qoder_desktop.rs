//! Qoder Desktop's current, independent native layers. This observer never
//! initializes its SDK/App Plugins, evaluates enablement, or materializes ZIPs.
use super::*;
use super::electron_package::{app_version, guard_asar, guarded_json, Guard};
use super::qoder::Package;
use std::cell::RefCell;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    agent_id: String, product_id: String, version: String, bundle_id: String, product_name: String,
    config_directory: String, environment_prefix: String, guards: Vec<Guard>, sdk_version: String, sdk_worker: Guard,
    bundled_manifest: Guard, extension_catalog: Guard, builtin_mcps: Vec<Builtin>, extension_mcps: Vec<ExtensionMcp>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Builtin { id: String, name: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtensionMcp { plugin_id: String, source: Guard, servers: Vec<String>, dynamic_reference: Option<String> }
fn profiles() -> &'static [Profile] {
    static VALUE: OnceLock<Vec<Profile>> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-qoder-desktop.json")).expect("audited Qoder Desktop profiles"))
}
fn root(source: &Root) -> PathBuf {
    if source.path.starts_with("/Applications/") { crate::paths::system_probe_path(&source.path) } else { expand_tilde(&source.path) }
}
fn confined_absolute(path: PathBuf) -> Result<PathBuf, ()> {
    let path=PathBuf::from(icu_normalizer::ComposingNormalizer::new_nfc().normalize(path.to_str().ok_or(())?).into_owned());
    if !path.is_absolute() { return Err(()); }
    let mut clean = PathBuf::new();
    for part in path.components() { match part { Component::ParentDir => { clean.pop(); }, Component::CurDir => {}, other => clean.push(other.as_os_str()) } }
    if expand_tilde("~/").starts_with(&clean) { return Err(()); }
    checked_root(&clean)?; Ok(clean)
}
fn native_folder(name: &str) -> Result<String, ()> {
    let name=icu_normalizer::ComposingNormalizer::new_nfc().normalize(name).into_owned();
    if name.is_empty() || name.chars().count()>64 || name.trim()!=name || name.ends_with('.')
        || name.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) { return Err(()); }
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if ["CON","PRN","AUX","NUL","COM1","COM2","COM3","COM4","COM5","COM6","COM7","COM8","COM9","LPT1","LPT2","LPT3","LPT4","LPT5","LPT6","LPT7","LPT8","LPT9"].contains(&stem.as_str()) { return Err(()); }
    Ok(name)
}
fn config_root(profile: &Profile) -> Result<PathBuf, ()> {
    let env = |suffix: &str| std::env::var_os(format!("{}{suffix}",profile.environment_prefix)).filter(|v|!v.is_empty());
    // Relative overrides depend on the native process CWD, which Arca does not
    // observe. Do not expand '~' as if Node's path.resolve did shell expansion.
    if let Some(path)=env("_CONFIG_DIR") { return confined_absolute(PathBuf::from(path)); }
    let home=env("_CLI_HOME").map(PathBuf::from).unwrap_or_else(||expand_tilde("~/"));
    if !home.is_absolute() { return Err(()); }
    let name=env("_CONFIG_DIR_NAME"); let name=name.as_ref().map(|v|v.to_str().ok_or(())).transpose()?.unwrap_or(&profile.config_directory);
    confined_absolute(home.join(native_folder(name)?))
}
fn snapshot(host: &Host, id: &str, name: &str, version: &str, presence: PluginPresence, source: &str, revision: &str) -> Result<NativePluginObservation, ()> {
    Ok(NativePluginObservation { agent_id:host.agent_id.clone(),plugin_id:metadata_name(id).ok_or(())?,name:metadata_name(name).ok_or(())?,
        version:metadata_name(version).ok_or(())?,revision:revision.into(),presence,enabled:None,source_paths:vec![source.into()],components:Vec::new(),
        docs_url:host.docs_url.clone(),source_fingerprint:String::new() })
}
fn add(out: &mut Vec<PluginComponent>, kind: PluginComponentKind, name: &str, source: String, id: Option<String>) -> Result<(), ()> {
    if out.len()>=MAX_COMPONENTS || id.as_ref().is_some_and(|id|id.len()>1024 || id.chars().any(char::is_control)) { return Err(()); }
    out.push(PluginComponent {kind,name:metadata_name(name).ok_or(())?,source_path:source,native_id:id}); Ok(())
}
fn authored(p: &Package<'_>, budget: &mut ScanBudget) -> Result<Option<(String,Arc<Value>)>, ()> {
    // The Desktop inventory's k6 reader skips invalid first manifests. Root
    // plugin.json is an IDE projection and cannot establish SDK ownership.
    let mut invalid=false;
    for folder in [".qoder-plugin",".claude-plugin"] {
        let input=format!("{folder}/plugin.json");
        match p.json(&input,budget) {
            Ok(Some(value)) if value.is_object() && value.get("name").and_then(Value::as_str).and_then(metadata_name).is_some() => return Ok(Some((input,value))),
            Ok(None) => {}, _ => invalid=true,
        }
    }
    if invalid { Err(()) } else { Ok(None) }
}
fn sdk_package(host: &Host, p: &Package<'_>, id: &str, presence: PluginPresence, sources: &[String], result: &mut PluginScan) -> Result<Option<NativePluginObservation>, ()> {
    let Some(manifest)=authored(p,&mut result.budget)? else { return Ok(None); };
    super::qoder_cli::sdk_package(host,p,id,presence,sources,Some(manifest),result).map(Some)
}
fn cli_cache(path: &Path) -> bool {
    path.components().filter_map(|p|p.as_os_str().to_str()).collect::<Vec<_>>().windows(3).any(|p|p==["plugins","cache","qodercli-bundler"])
}
fn bundle_namespace(id: &str) -> bool { id.rsplit_once('@').is_some_and(|(_,market)|["qoder-bundler","qoderapp-bundler"].contains(&market)) }
fn bundle_path(path: &Path) -> bool {
    path.components().filter_map(|p|p.as_os_str().to_str()).collect::<Vec<_>>().windows(3).any(|p|p[0]=="plugins" && p[1]=="cache" && ["qoder-bundler","qoderapp-bundler"].contains(&p[2]))
}
fn bundled_match(id: &str, path: &Path, config: &Path, manifest: Option<&Value>, record_version: Option<&str>, result: &mut PluginScan) -> Result<bool, ()> {
    let Some(resources)=manifest.and_then(|v|v.get("resources")).and_then(Value::as_array) else { return Ok(false); };
    for item in resources {
        if item.get("kind").and_then(Value::as_str)!=Some("qoder-plugin") { continue; }
        let install=item.get("install").ok_or(())?;
        let plugin_id=install.get("pluginId").and_then(Value::as_str).ok_or(())?;
        if !plugin_id.eq_ignore_ascii_case(id) { continue; }
        let (name,market)=plugin_id.rsplit_once('@').ok_or(())?;
        let folder=install.get("folderName").and_then(Value::as_str).ok_or(())?;
        if name!=folder || !bundle_namespace(plugin_id) { return Err(()); }
        let base=config.join("plugins/cache").join(market).join(native_folder(folder)?);
        if market=="qoder-bundler" { return Ok(path==base); }
        let version=item.get("version").and_then(Value::as_str).ok_or(())?;
        let version_path=|version: &str| base.join(version.chars().map(|c|if c.is_ascii_alphanumeric() || "-_.".contains(c) {c} else {'-'}).collect::<String>());
        if path==version_path(version) { return Ok(true); }
        // The native explicit overwrite opt-out preserves only a registered
        // user installation whose authored manifest version matches the record.
        if std::env::var("QODER_DISABLE_BUNDLED_PLUGIN_OVERWRITE").ok().as_deref()==Some("true") {
            if let Some(version)=record_version.filter(|v|!v.trim().is_empty()) {
                if path==version_path(version.trim()) {
                    for input in [".qoder-plugin/plugin.json",".claude-plugin/plugin.json","plugin.json"] {
                        if optional_json(path,input,&mut result.budget)?.is_some_and(|v|v.get("version").and_then(Value::as_str)==Some(version.trim())) { return Ok(true); }
                    }
                }
            }
        }
        return Ok(false);
    }
    Ok(false)
}
fn installed(host: &Host, config: &Path, manifest: Option<&Value>, result: &mut PluginScan) -> Result<(), ()> {
    let root=config.join("plugins");
    let Some(state)=optional_json(&root,"installed_plugins_v2.json",&mut result.budget)? else {return Ok(());};
    if state.get("version").and_then(Value::as_u64)!=Some(2) { return Err(()); }
    let plugins=state.get("plugins").and_then(Value::as_object).filter(|v|v.len()<=MAX_ENTRIES).ok_or(())?;
    for (id,entries) in plugins {
        let Some(entries)=entries.as_array().filter(|v|v.len()<=MAX_ENTRIES) else {result.unavailable=true;continue;};
        for (ordinal,entry) in entries.iter().enumerate() {
            result.budget.visit()?;
            if !matches!(entry.get("scope").and_then(Value::as_str),Some("user"|"managed")) {continue;}
            let observed=(||{
                metadata_name(id).ok_or(())?;
                let path=confined_absolute(PathBuf::from(entry.get("installPath").and_then(Value::as_str).ok_or(())?))?;
                if cli_cache(&path) {return Ok(());}
                if bundle_namespace(id) {
                    if !bundled_match(id,&path,config,manifest,entry.get("version").and_then(Value::as_str),result)? {return Ok(());}
                } else if bundle_path(&path) {return Ok(());}
                let source=collapse_home(&root.join("installed_plugins_v2.json"));
                let mut plugin=if path.try_exists().map_err(|_|())? {
                    sdk_package(host,&Package::Disk(&path),id,PluginPresence::Installed,&[source.clone()],result)?
                } else {None};
                if plugin.is_none() {plugin=Some(snapshot(host,id,id,"unknown",PluginPresence::Declared,&source,"registered-declaration")?);}
                let mut plugin=plugin.ok_or(())?;plugin.revision=format!("registered:{ordinal}:{}",collapse_home(&path));result.plugins.push(plugin);Ok::<(),()>(())
            })();if observed.is_err(){result.unavailable=true;}
        }
    }
    Ok(())
}
fn archive<'a>(path: &'a Path, resource: &Value, budget: &mut ScanBudget) -> Result<Package<'a>, ()> {
    let meta=resource.get("archive").ok_or(())?;
    let count=meta.get("size").and_then(Value::as_u64).filter(|n|*n<=16*1_048_576).ok_or(())?;
    let digest=meta.get("sha256").and_then(Value::as_str).ok_or(())?;
    let file=verified_artifact(path,count,digest,budget)?;let mut zip=zip::ZipArchive::new(file).map_err(|_|())?;
    let max_entries=meta.get("maxEntries").and_then(Value::as_u64).filter(|n|*n<=4096).ok_or(())?;
    let max_bytes=meta.get("maxExpandedBytes").and_then(Value::as_u64).filter(|n|*n<=96*1_048_576).ok_or(())?;
    if zip.len() as u64>max_entries {return Err(());}
    let prefix=match meta.get("root").and_then(Value::as_str) {Some("qoder-context-plugin")=>"qoder-context-plugin/",Some("qoder-computer-use")=>"qoder-computer-use/",_=>return Err(())};
    let mut members=BTreeMap::new();let mut total=0u64;
    for i in 0..zip.len() {
        budget.visit()?;let entry=zip.by_index(i).map_err(|_|())?;
        let name=entry.name();let plain=name.trim_end_matches('/');
        if plain.is_empty() || name.contains('\\') || qoder::local_path(plain)?!=plain
            || entry.unix_mode().is_some_and(|m|![0,0o100000,0o040000].contains(&(m&0o170000))) {return Err(());}
        total=total.checked_add(entry.size()).filter(|n|*n<=max_bytes).ok_or(())?;
        if name==prefix && entry.is_dir(){continue;}
        let member=name.strip_prefix(prefix).ok_or(())?.trim_end_matches('/');
        if member.is_empty() || members.insert(member.into(),(i,entry.is_dir())).is_some(){return Err(());}
    }
    for name in members.keys(){for parent in Path::new(name).ancestors().skip(1){if members.get(parent.to_string_lossy().as_ref()).is_some_and(|(_,dir)|!dir){return Err(());}}}
    Ok(Package::Archive {path,prefix,zip:RefCell::new(zip),members})
}
fn bundled_archives(host: &Host, resources: &Path, manifest: &Value, result: &mut PluginScan) -> Result<(), ()> {
    if manifest.get("schemaVersion").and_then(Value::as_u64)!=Some(1) {return Err(());}
    for resource in manifest.get("resources").and_then(Value::as_array).filter(|v|v.len()<=64).ok_or(())? {
        let observed=(||{
            let id=resource.get("id").and_then(Value::as_str).ok_or(())?;
            let version=resource.get("version").and_then(Value::as_str).ok_or(())?;
            let path=relative(&resources.join("bundled-resources"),resource.pointer("/archive/path").and_then(Value::as_str).ok_or(())?)?;
            let p=archive(&path,resource,&mut result.budget)?;
            let mut plugin=match resource.get("kind").and_then(Value::as_str){
                Some("qoder-plugin")=>sdk_package(host,&p,resource.pointer("/install/pluginId").and_then(Value::as_str).ok_or(())?,PluginPresence::Bundled,&[collapse_home(&path)],result)?.ok_or(())?,
                Some("shared-binary")=>{
                    let mut plugin=snapshot(host,&format!("@builtin/{id}"),id,version,PluginPresence::Bundled,&collapse_home(&path),"bundled-binary-archive")?;
                    for entry in resource.get("entrypoints").and_then(Value::as_array).filter(|v|v.len()<=64).ok_or(())? {
                        let input=entry.get("path").and_then(Value::as_str).ok_or(())?;
                        if !p.exists(input)? {return Err(());}
                        let executable=entry.get("executable").and_then(Value::as_bool).ok_or(())?;
                        add(&mut plugin.components,if executable{PluginComponentKind::Executable}else{PluginComponentKind::App},entry.get("name").and_then(Value::as_str).ok_or(())?,p.path(input)?,None)?;
                    }plugin
                },_=>return Err(())
            };plugin.version=metadata_name(version).ok_or(())?;plugin.revision=format!("bundled-resource:{}",resource.get("revision").and_then(Value::as_u64).ok_or(())?);result.plugins.push(plugin);Ok::<(),()>(())
        })();if observed.is_err(){result.unavailable=true;}
    }Ok(())
}
fn extension(host: &Host, path: &Path, manifest: &Value, presence: PluginPresence, profile: Option<&Profile>, result: &mut PluginScan) -> Result<(), ()> {
    let source=collapse_home(&path.join(".qoder-app-plugin/plugin.json"));
    let id=manifest.get("id").and_then(Value::as_str).ok_or(())?;
    let name=manifest.get("name").and_then(Value::as_str).ok_or(())?;
    let mut plugin=snapshot(host,id,name,manifest.get("version").and_then(Value::as_str).ok_or(())?,presence.clone(),&source,"native-app-plugin-declaration")?;
    let main=manifest.get("main").and_then(Value::as_str).ok_or(())?;
    if !regular_metadata_file(&relative(path,main)?)? {return Err(());}
    add(&mut plugin.components,PluginComponentKind::Code,main,collapse_home(&relative(path,main)?),None)?;
    for permission in manifest.get("permissions").and_then(Value::as_array).filter(|v|v.len()<=128).ok_or(())? {
        add(&mut plugin.components,PluginComponentKind::Policy,permission.as_str().ok_or(())?,source.clone(),None)?;
    }
    for field in ["views","nativeModules","sidebarNavItems","filePreviewers","sessionResources","chatMentionProviders"] {
        if let Some(values)=manifest.get("contributes").and_then(|v|v.get(field)) {
            for value in values.as_array().filter(|v|v.len()<=128).ok_or(())? {
                let key=value.get("id").and_then(Value::as_str).ok_or(())?;
                let (kind,component_source)=if field=="views" {
                    let file=relative(path,value.get("entry").and_then(Value::as_str).ok_or(())?)?;
                    if !regular_metadata_file(&file)? {return Err(());}
                    (PluginComponentKind::Renderer,collapse_home(&file))
                } else {(PluginComponentKind::Extension,source.clone())};
                add(&mut plugin.components,kind,&format!("{field}:{key}"),component_source,None)?;
            }
        }
    }
    if let Some(values)=manifest.pointer("/contributes/qoderAgentSdk/plugins") {
        for value in values.as_array().filter(|v|v.len()<=64).ok_or(())? {
            if value.get("type").and_then(Value::as_str)!=Some("local"){return Err(());}
            let input=value.get("path").and_then(Value::as_str).ok_or(())?;let dir=relative(path,input)?;
            add(&mut plugin.components,PluginComponentKind::PluginReference,input,collapse_home(&dir),None)?;
            if let Some(item)=sdk_package(host,&Package::Disk(&dir),&format!("{id}/{input}"),presence.clone(),&[source.clone()],result)? {result.plugins.push(item);} else {result.unavailable=true;}
        }
    }
    if let Some(fact)=profile.and_then(|p|p.extension_mcps.iter().find(|v|v.plugin_id==id)) {
        let observed=(||{
            if fact.source.path!=main {return Err(());}
            let file=relative(path,&fact.source.path)?;verify_artifact(&file,fact.source.bytes,&fact.source.sha256,&mut result.budget)?;
            let mut components=Vec::new();
            for server in &fact.servers {add(&mut components,PluginComponentKind::Mcp,server,collapse_home(&file),Some(format!("plugin:{name}:{server}")))?;}
            if let Some(label)=&fact.dynamic_reference {add(&mut components,PluginComponentKind::McpReference,label,collapse_home(&file),None)?;}
            if components.len()>MAX_COMPONENTS.saturating_sub(plugin.components.len()){return Err(());}
            plugin.components.extend(components);Ok::<(),()>(())
        })();if observed.is_err(){result.unavailable=true;}
    }
    result.plugins.push(plugin);Ok(())
}
fn extensions(host: &Host, resources: &Path, profile: &Profile, result: &mut PluginScan) -> Result<(), ()> {
    let catalog=guarded_json(&relative(resources,&profile.extension_catalog.path)?,&profile.extension_catalog,&mut result.budget)?;
    if catalog.get("schemaVersion").and_then(Value::as_u64)!=Some(1) || catalog.get("productId").and_then(Value::as_str)!=Some(&profile.product_id) {return Err(());}
    let mut ids=BTreeSet::new();
    let values=catalog.get("plugins").and_then(Value::as_array).filter(|v|v.len()<=128).ok_or(())?;
    for value in values {if !ids.insert(value.pointer("/manifest/id").and_then(Value::as_str).ok_or(())?){return Err(());}}
    for value in values {
        let observed=(||{
            let path=relative(&resources.join("extensions"),value.get("root").and_then(Value::as_str).ok_or(())?)?;
            let manifest=value.get("manifest").ok_or(())?;
            let expected=value.get("files").and_then(Value::as_array).ok_or(())?.iter().find(|f|f.get("path").and_then(Value::as_str)==Some(".qoder-app-plugin/plugin.json")).ok_or(())?;
            let file=path.join(".qoder-app-plugin/plugin.json");let bytes=fs::symlink_metadata(&file).map_err(|_|())?.len();
            let guard=Guard{path:".qoder-app-plugin/plugin.json".into(),bytes,sha256:expected.get("sha256").and_then(Value::as_str).ok_or(())?.into()};
            if guarded_json(&file,&guard,&mut result.budget)?!=*manifest {return Err(());}
            // These are physical declarations. The native host additionally
            // verifies its complete catalog and applies OS/config/account gates.
            extension(host,&path,manifest,PluginPresence::Bundled,Some(profile),result)
        })();if observed.is_err(){result.unavailable=true;}
    }Ok(())
}
fn packaged_skills(host: &Host, resources: &Path, profile: &Profile, result: &mut PluginScan) -> Result<(), ()> {
    let worker_file=relative(resources,&profile.sdk_worker.path)?;
    verify_artifact(&worker_file,profile.sdk_worker.bytes,&profile.sdk_worker.sha256,&mut result.budget)?;
    let worker=worker_file.parent().ok_or(())?;
    let builtin=relative(worker,"builtin")?;let mut item=snapshot(host,"@builtin/sdk-skills","Qoder SDK Skills",&profile.sdk_version,PluginPresence::Bundled,&collapse_home(&builtin),"shipped-sdk-resources")?;
    super::qoder_cli::files(&Package::Disk(&builtin),"",PluginComponentKind::Skill,0,&mut item.components,&mut result.budget)?;result.plugins.push(item);
    let security=relative(worker,"vendor/qoder-security")?;
    if let Some(item)=sdk_package(host,&Package::Disk(&security),"security-scan@sdk",PluginPresence::Bundled,&[collapse_home(&worker_file)],result)?{result.plugins.push(item);}else{result.unavailable=true;}
    let sites=relative(worker,"vendor/qoder-sites")?;
    let artifact=optional_json(&sites,"artifact.json",&mut result.budget)?.ok_or(())?;
    let mut item=super::qoder_cli::sdk_package(host,&Package::Disk(&sites),"qoder-sites@sdk",PluginPresence::Bundled,&[collapse_home(&sites.join("artifact.json"))],None,result)?;
    item.name="Qoder SDK Sites".into();item.version=artifact.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
    for event in ["SessionStart","SessionEnd"] {add(&mut item.components,PluginComponentKind::Hook,event,collapse_home(&worker_file),None)?;}result.plugins.push(item);
    for (id,name) in [("collaboration","Qoder collaboration"),("qoder-helper","Qoder Helper")] {
        let dir=relative(resources,id)?;
        let mut item=super::qoder_cli::sdk_package(host,&Package::Disk(&dir),&format!("@builtin/{id}"),PluginPresence::Bundled,&[collapse_home(&dir)],None,result)?;
        item.name=name.into();item.version=profile.version.clone();result.plugins.push(item);
    }Ok(())
}
fn development(host: &Host, config: &Path, result: &mut PluginScan) -> Result<(), ()> {
    if std::env::var("QODER_APP_PLUGIN_ALLOW_UNVERIFIED").ok().as_deref()!=Some("true"){return Ok(());}
    let mut paths=Vec::new();
    if let Some(value)=std::env::var_os("QODER_APP_PLUGIN_DEV_DIRS") {for path in std::env::split_paths(&value){if !path.as_os_str().is_empty(){paths.push(confined_absolute(path)?);}}}
    paths.extend(dirs(&config.join("app/dev-extensions"),&mut result.budget)?);paths.sort();paths.dedup();
    if paths.len()>128{return Err(());}
    for path in paths {
        let observed=(||{let manifest=optional_json(&path,".qoder-app-plugin/plugin.json",&mut result.budget)?.ok_or(())?;extension(host,&path,&manifest,PluginPresence::Local,None,result)})();if observed.is_err(){result.unavailable=true;}
    }Ok(())
}
pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    let Some(profile)=profiles().iter().find(|p|p.agent_id==host.agent_id)else{result.unavailable=true;return;};
    let config=config_root(profile);let mut manifest=None;
    for source in &host.roots {
        if source.layout!="qoder_desktop_app" {continue;}
        let app=root(source);
        match fs::symlink_metadata(&app){Err(e)if e.kind()==std::io::ErrorKind::NotFound=>continue,Ok(m)if m.is_dir()=>{},_=>{result.unavailable=true;break;}}
        let observed=(||{
            if app_version(&app,&profile.bundle_id,&profile.product_name,&mut result.budget)?!=profile.version{return Err(());}
            let resources=relative(&app,"Contents/Resources")?;
            guard_asar(&resources.join("app.asar"),&profile.guards,&mut result.budget)?;
            let value=guarded_json(&relative(&resources,&profile.bundled_manifest.path)?,&profile.bundled_manifest,&mut result.budget)?;
            manifest=Some(value);
            let code=format!("{}!/out/main/electronMain.js",collapse_home(&resources.join("app.asar")));
            let mut plugin=snapshot(host,"@builtin/desktop-mcp","Qoder native MCP factories",&profile.version,PluginPresence::Bundled,&code,"audited-native-factories")?;
            for fact in &profile.builtin_mcps{add(&mut plugin.components,PluginComponentKind::Mcp,&fact.name,code.clone(),Some(fact.id.clone()))?;}result.plugins.push(plugin);
            if bundled_archives(host,&resources,manifest.as_ref().ok_or(())?,result).is_err(){result.unavailable=true;}
            if packaged_skills(host,&resources,profile,result).is_err(){result.unavailable=true;}
            if extensions(host,&resources,profile,result).is_err(){result.unavailable=true;}
            Ok::<(),()>(())
        })();if observed.is_err(){result.unavailable=true;}break;
    }
    match config {Ok(config)=>{if installed(host,&config,manifest.as_ref(),result).is_err(){result.unavailable=true;}if development(host,&config,result).is_err(){result.unavailable=true;}},Err(())=>result.unavailable=true}
}
pub(super) fn watch_paths(host: &Host) -> Vec<PathBuf> {
    let mut paths=host.roots.iter().filter(|r|r.layout=="qoder_desktop_app").map(root).collect::<Vec<_>>();
    if let Some(profile)=profiles().iter().find(|p|p.agent_id==host.agent_id){if let Ok(config)=config_root(profile){paths.push(config.join("plugins"));if std::env::var("QODER_APP_PLUGIN_ALLOW_UNVERIFIED").ok().as_deref()==Some("true"){paths.push(config.join("app/dev-extensions"));if let Some(value)=std::env::var_os("QODER_APP_PLUGIN_DEV_DIRS"){paths.extend(std::env::split_paths(&value).filter_map(|path|confined_absolute(path).ok()));}}}}
    paths.sort();paths.dedup();paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    #[test]
    fn native_paths_normalize_unicode_and_reject_unknown_cwd_or_broad_roots() {
        let home=TestHome::new("qoder-desktop-paths");
        assert_eq!(native_folder("e\u{301}").unwrap(),"é");
        assert!(native_folder("../outside").is_err());assert!(native_folder("CON.txt").is_err());
        assert!(confined_absolute(PathBuf::from("relative/path")).is_err());
        assert!(confined_absolute(home.home.clone()).is_err());assert!(confined_absolute(PathBuf::from("/")).is_err());
    }
    #[test]
    fn desktop_ownership_does_not_mix_ide_or_cli_bundles_and_requires_current_app_path() {
        let home=TestHome::new("qoder-desktop-ownership");let config=home.home.join(".qoder");
        let value=serde_json::json!({"resources":[{"kind":"qoder-plugin","version":"1.0.84","install":{"folderName":"qoder-context","pluginId":"qoder-context@qoderapp-bundler"}}]});
        let mut scan=PluginScan::default();
        assert!(cli_cache(&config.join("plugins/cache/qodercli-bundler/sites/1")));
        assert!(!bundled_match("security-scan@qoder-bundler",&config.join("plugins/cache/qoder-bundler/security-scan"),&config,Some(&value),None,&mut scan).unwrap());
        assert!(bundled_match("qoder-context@qoderapp-bundler",&config.join("plugins/cache/qoderapp-bundler/qoder-context/1.0.84"),&config,Some(&value),None,&mut scan).unwrap());
        assert!(!bundled_match("qoder-context@qoderapp-bundler",&config.join("plugins/cache/qoderapp-bundler/qoder-context/other"),&config,Some(&value),None,&mut scan).unwrap());
        // Identical plugin IDs and versions cannot establish ownership across
        // the two products' independently resolved native roots.
        let cn=home.home.join(".qoder-cn");
        let global_path=config.join("plugins/cache/qoderapp-bundler/qoder-context/1.0.84");
        let cn_path=cn.join("plugins/cache/qoderapp-bundler/qoder-context/1.0.84");
        assert!(!bundled_match("qoder-context@qoderapp-bundler",&global_path,&cn,Some(&value),None,&mut scan).unwrap());
        assert!(!bundled_match("qoder-context@qoderapp-bundler",&cn_path,&config,Some(&value),None,&mut scan).unwrap());
        assert!(bundled_match("qoder-context@qoderapp-bundler",&cn_path,&cn,Some(&value),None,&mut scan).unwrap());
    }
    #[test]
    fn authored_sdk_fallback_and_mcp_overlay_are_metadata_only() {
        let home=TestHome::new("qoder-desktop-sdk");let path=home.home.join("plugin");fs::create_dir_all(path.join(".qoder-plugin")).unwrap();fs::create_dir_all(path.join(".claude-plugin")).unwrap();
        fs::write(path.join(".qoder-plugin/plugin.json"),"invalid").unwrap();
        fs::write(path.join(".claude-plugin/plugin.json"),r#"{"name":"sample","version":"1","mcpServers":{"one":{"type":"sdk"}}}"#).unwrap();
        fs::write(path.join(".mcp.json"),r#"{"mcpServers":{"one":{"url":"https://private.example","headers":{"Authorization":"private-token"}},"two":{"command":"private-command"}}}"#).unwrap();
        let host=hosts().iter().find(|h|h.agent_id=="qoder-desktop").unwrap();let mut scan=PluginScan::default();
        let item=sdk_package(host,&Package::Disk(&path),"sample@local",PluginPresence::Installed,&[],&mut scan).unwrap().unwrap();
        assert_eq!(item.enabled,None);assert_eq!(item.components.iter().filter(|c|c.kind==PluginComponentKind::Mcp).count(),2);
        let data=serde_json::to_string(&item).unwrap();for value in ["private.example","private-token","private-command"]{assert!(!data.contains(value));}
    }
    #[test]
    fn a_changed_extension_entrypoint_keeps_manifest_metadata_without_stale_mcp_facts() {
        let home=TestHome::new("qoder-desktop-unknown-entry");let path=home.home.join("extension");fs::create_dir_all(&path).unwrap();fs::write(path.join("main.cjs"),"code").unwrap();
        let mut profiles:Vec<Profile>=serde_json::from_str(include_str!("../../../../data/native-plugin-qoder-desktop.json")).unwrap();
        profiles[0].extension_mcps=vec![ExtensionMcp{plugin_id:"sample".into(),source:Guard{path:"main.cjs".into(),bytes:4,sha256:"not-the-audited-digest".into()},servers:vec!["stale".into()],dynamic_reference:None}];
        let manifest=serde_json::json!({"id":"sample","name":"sample","version":"1","main":"main.cjs","permissions":[]});
        let host=hosts().iter().find(|h|h.agent_id=="qoder-desktop").unwrap();let mut scan=PluginScan::default();
        extension(host,&path,&manifest,PluginPresence::Bundled,Some(&profiles[0]),&mut scan).unwrap();
        assert!(scan.unavailable);assert_eq!(scan.plugins.len(),1);assert_eq!(scan.plugins[0].components.len(),1);assert_eq!(scan.plugins[0].components[0].kind,PluginComponentKind::Code);
    }
}
