//! Current native registration sources. Cache files establish availability,
//! never installation, ownership, accepted exports or a running SDK instance.
use super::*;
use super::metadata::Snapshot;

const SERVER_FILES:[&str;5]=["config.json","kilo.json","kilo.jsonc","opencode.json","opencode.jsonc"];
const CATALOG_FILES:[&str;7]=["kilo.jsonc","kilo.json","opencode.jsonc","opencode.json","tui.jsonc","tui.json","config.json"];
const MAX_DECLARATIONS:usize=512;

fn absolute(path:&Path)->Result<PathBuf,()> {
    if !path.is_absolute()||path.to_str().is_none_or(|v|v.len()>4096||v.chars().any(char::is_control)) {return Err(());}
    let mut result=PathBuf::new();
    for component in path.components(){match component{
        Component::ParentDir=>{if !result.pop(){return Err(());}},Component::CurDir=>{},
        Component::Prefix(_)=>return Err(()),_=>result.push(component.as_os_str()),
    }}
    if !result.is_absolute()||result.components().count()>64{return Err(());}Ok(result)
}
fn env_path(key:&str,fallback:PathBuf)->Result<PathBuf,()> {
    let raw=std::env::var_os(key).filter(|v|!v.is_empty()).map(PathBuf::from).unwrap_or(fallback);
    absolute(&raw)
}
fn opaque(path:&Path)->bool {
    path.components().any(|v|v.as_os_str().to_str().is_none_or(|v|{
        let v=v.to_ascii_lowercase();
        matches!(v.as_str(),".ssh"|".git"|".gnupg"|".auth"|"auth.json"|"auth-store.json"|"sessions"|"hooks"|"hooks.json")
            ||["credential","secret","token","account","cookie","oauth"].iter().any(|word|v.contains(word))
    }))
}

pub(super) struct Scope {
    pub config:PathBuf, pub cache:PathBuf, pub home:PathBuf,
    pub extra:Option<PathBuf>, pub custom:Option<PathBuf>,
    env:Vec<(&'static str,Option<std::ffi::OsString>)>,
}
impl Scope {
    pub fn capture()->Result<Self,()> {
        let keys=["XDG_CONFIG_HOME","XDG_CACHE_HOME","KILO_TEST_HOME","KILO_CONFIG_DIR","KILO_CONFIG"];
        let env=keys.into_iter().map(|key|(key,std::env::var_os(key))).collect();
        let os_home=absolute(&expand_tilde("~/"))?;
        let home=match std::env::var_os("KILO_TEST_HOME"){
            Some(value)=>absolute(Path::new(value.to_str().ok_or(())?.trim()))?,None=>os_home.clone(),
        };
        // Immutable Global.Path config/cache use XDG defaults, independent of
        // KILO_TEST_HOME and Global.make's service-only config-dir override.
        let config=env_path("XDG_CONFIG_HOME",os_home.join(".config"))?.join("kilo");
        let cache=env_path("XDG_CACHE_HOME",os_home.join(".cache"))?.join("kilo");
        let extra=std::env::var_os("KILO_CONFIG_DIR").filter(|v|!v.is_empty()).map(|v|absolute(Path::new(&v))).transpose()?;
        let custom=std::env::var_os("KILO_CONFIG").filter(|v|!v.is_empty()).map(|v|absolute(Path::new(&v))).transpose()?;
        Ok(Self{config,cache,home,extra,custom,env})
    }
    pub fn revalidate(&self)->Result<(),()> {
        if self.env.iter().any(|(key,value)|std::env::var_os(key)!=*value){Err(())}else{Ok(())}
    }
    pub fn directories(&self)->Vec<PathBuf> {
        let mut dirs=vec![self.config.clone(),self.home.join(".kilocode"),self.home.join(".kilo")];
        if let Some(extra)=&self.extra{dirs.push(extra.clone());}
        let mut seen=BTreeSet::new();dirs.retain(|v|seen.insert(v.clone()));dirs
    }
    pub fn watch_paths(&self)->Vec<PathBuf> {
        let mut paths=self.directories();
        for file in CATALOG_FILES{paths.push(self.config.join(file));}
        paths.push(self.config.join("config"));
        if let Some(path)=&self.custom{paths.push(path.clone());}
        paths
    }
}

#[derive(Clone)]
enum Target { Npm{name:String,cache_spec:Option<String>}, Local(PathBuf), Opaque }
#[derive(Clone)]
struct Spec { identity:String, label:String, target_key:String, target:Target }
fn unresolved(source:&Path,ordinal:usize)->Spec {
    let key=hex::encode(Sha256::digest(format!("{}:{ordinal}",source.to_string_lossy()).as_bytes()));
    Spec{identity:format!("opaque:{key}"),label:"Unresolved Kilo SDK plugin".into(),target_key:key,target:Target::Opaque}
}
fn spec(input:&str,source:&Path,ordinal:usize)->Spec {
    let unknown=||unresolved(source,ordinal);
    if input.is_empty()||input.len()>4096||input.chars().any(char::is_control)
        ||["${","{env:","{file:"].iter().any(|v|input.contains(v)){return unknown();}
    if input.starts_with("file://")||input.starts_with('.')||Path::new(input).is_absolute(){
        let path=if input.starts_with("file://"){
            let Ok(url)=url::Url::parse(input)else{return unknown();};
            if !url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return unknown();}
            let Ok(path)=url.to_file_path()else{return unknown();};path
        }else if Path::new(input).is_absolute(){PathBuf::from(input)}else{
            let Some(parent)=source.parent()else{return unknown();};parent.join(input)
        };
        let Ok(path)=absolute(&path)else{return unknown();};if opaque(&path){return unknown();}
        let Ok(url)=url::Url::from_file_path(&path)else{return unknown();};
        let Some(label)=path.file_name().and_then(|v|v.to_str()).and_then(metadata_name)else{return unknown();};
        let key=url.to_string();return Spec{identity:format!("file:{key}"),label,target_key:key,target:Target::Local(path)};
    }
    static NPM:OnceLock<regex::Regex>=OnceLock::new();
    let regex=NPM.get_or_init(||regex::Regex::new(r"^(?P<prefix>npm:)?(?P<name>(?:@[A-Za-z0-9_.-]+/)?[A-Za-z0-9_.-]+)(?:@(?P<version>[A-Za-z0-9_.~^*+<>=| -]+))?$").unwrap());
    let Some(capture)=regex.captures(input.trim())else{return unknown();};
    let package=&capture["name"];
    if package.len()>214||package.split('/').any(|v|matches!(v,"."|".."|"@")){return unknown();}
    let cache_spec=if capture.name("prefix").is_some(){None}
        else if input==package{Some(format!("{package}@latest"))}else{Some(input.to_owned())};
    Spec{identity:format!("npm:{package}"),label:package.into(),target_key:input.into(),target:Target::Npm{name:package.into(),cache_spec}}
}
struct Registration {
    label:String, sources:BTreeSet<PathBuf>, roles:BTreeSet<&'static str>, targets:BTreeMap<String,Target>, selected_global:bool,
}
#[derive(Default)]
struct Registry { entries:BTreeMap<String,Registration>, count:usize }
impl Registry {
    fn add(&mut self,value:Spec,source:&Path,role:&'static str,selected:bool,snapshot:&mut Snapshot){
        if self.count>=MAX_DECLARATIONS{snapshot.incomplete=true;return;}self.count+=1;
        let entry=self.entries.entry(value.identity).or_insert_with(||Registration{label:value.label,sources:BTreeSet::new(),roles:BTreeSet::new(),targets:BTreeMap::new(),selected_global:false});
        entry.sources.insert(source.into());entry.roles.insert(role);entry.selected_global|=selected;
        if entry.targets.len()<32{entry.targets.insert(value.target_key,value.target);}else{snapshot.incomplete=true;}
    }
}
fn read_list(path:&Path,format:DocumentFormat,snapshot:&mut Snapshot)->Result<Option<Vec<String>>,()> {
    if opaque(path){return Err(());}
    let Some(value)=snapshot.json(path,format)?else{return Ok(None);};
    let object=value.as_object().ok_or(())?;
    let Some(values)=object.get("plugin")else{return Ok(None);};
    let values=values.as_array().filter(|v|v.len()<=MAX_DECLARATIONS).ok_or(())?;
    let mut result=Vec::new();
    for value in values{
        let input=match value{Value::String(v)=>Some(v.as_str()),Value::Array(v)=>v.first().and_then(Value::as_str),_=>None};
        if let Some(input)=input{result.push(input.into());}else{snapshot.incomplete=true;}
    }
    Ok(Some(result))
}
fn append_file(path:&Path,role:&'static str,registry:&mut Registry,snapshot:&mut Snapshot){
    match read_list(path,DocumentFormat::Jsonc,snapshot){
        Ok(Some(values))=>for(i,value)in values.iter().enumerate(){registry.add(spec(value,path,i),path,role,false,snapshot);},
        Ok(None)=>{},Err(())=>snapshot.incomplete=true,
    }
}
fn version(value:&Value)->Option<String>{
    static V:OnceLock<regex::Regex>=OnceLock::new();
    let value=value.as_str().filter(|v|v.len()<=96)?;
    V.get_or_init(||regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?$").unwrap()).is_match(value).then(||value.into())
}
fn available(target:&Target,scope:&Scope,snapshot:&mut Snapshot)->Result<Option<(PathBuf,Option<String>)>,()> {
    let (path,package)=match target{
        Target::Opaque=>return Ok(None),
        Target::Local(path)=>(path.clone(),None),
        Target::Npm{name,cache_spec:Some(spec)}=>{
            let tail=Path::new(spec);
            if tail.is_absolute()||tail.components().any(|v|!matches!(v,Component::Normal(_))){return Err(());}
            (scope.cache.join("packages").join(tail).join("node_modules").join(name),Some(name))
        },
        Target::Npm{cache_spec:None,..}=>return Ok(None),
    };
    if opaque(&path){return Err(());}
    let Some(leaf)=snapshot.stat(&path)?else{return Ok(None);};
    if leaf.file{return Ok(Some((path,None)));}
    if !leaf.directory{return Err(());}
    let manifest=path.join("package.json");
    if let Some(value)=snapshot.json(&manifest,DocumentFormat::Json)?{
        if !value.is_object(){return Err(());}
        if package.is_some_and(|p|value.get("name").and_then(Value::as_str)!=Some(p.as_str())){return Err(());}
        return Ok(Some((manifest,value.get("version").and_then(version))));
    }
    // Native directories without a package manifest try these index names.
    // Presence is observed without opening any private SDK module body.
    for name in ["index.ts","index.tsx","index.js","index.mjs","index.cjs"]{
        let file=path.join(name);if snapshot.stat(&file)?.is_some_and(|v|v.file){return Ok(Some((file,None)));}
    }
    Ok(None)
}

pub(super) fn collect(host:&Host,proof:&runtime::Proof,snapshot:&mut Snapshot)->Result<Vec<NativePluginObservation>,()> {
    let scope=Scope::capture()?;let mut registry=Registry::default();
    // The catalog detector unions seven sources. This is registration metadata,
    // not the different server merge order or runtime enablement.
    for file in CATALOG_FILES{append_file(&scope.config.join(file),"native-catalog",&mut registry,snapshot);}
    let mut global=Vec::new();
    for file in SERVER_FILES{
        let path=scope.config.join(file);
        match read_list(&path,DocumentFormat::Jsonc,snapshot){Ok(Some(values))=>global=values.into_iter().map(|v|(v,path.clone())).collect(),Ok(None)=>{},Err(())=>snapshot.incomplete=true}
    }
    let legacy=scope.config.join("config");
    match read_list(&legacy,DocumentFormat::Toml,snapshot){Ok(Some(values))=>global=values.into_iter().map(|v|(v,legacy.clone())).collect(),Ok(None)=>{},Err(())=>snapshot.incomplete=true}
    for(i,(value,path))in global.iter().enumerate(){registry.add(spec(value,path,i),path,"global-server-merge",true,snapshot);}
    if let Some(path)=&scope.custom{append_file(path,"explicit-config",&mut registry,snapshot);}
    for directory in scope.directories(){
        if directory!=scope.config{
            for file in SERVER_FILES{append_file(&directory.join(file),"config-directory",&mut registry,snapshot);}
            for file in ["tui.json","tui.jsonc"]{append_file(&directory.join(file),"tui-config-directory",&mut registry,snapshot);}
        }
        for folder in ["plugin","plugins"]{
            let path=directory.join(folder);
            match snapshot.entries(&path){
                Ok(entries)=>for(path,_,file)in entries{
                    if !matches!(path.extension().and_then(|v|v.to_str()),Some("ts"|"js")){continue;}
                    if !file{snapshot.incomplete=true;continue;}
                    let Ok(url)=url::Url::from_file_path(&path)else{snapshot.incomplete=true;continue;};
                    registry.add(spec(url.as_str(),&path,0),&path,"auto-discovery",true,snapshot);
                },Err(())=>snapshot.incomplete=true,
            }
        }
    }
    let mut result=Vec::new();
    for(identity,entry)in registry.entries{
        let mut sources=entry.sources.iter().map(|v|collapse_home(v)).collect::<Vec<_>>();
        let mut components=Vec::new();let mut versions=BTreeSet::new();let mut local=false;
        for target in entry.targets.values(){match available(target,&scope,snapshot){
            Ok(Some((path,version)))=>{local=true;if let Some(v)=version{versions.insert(v);}
                sources.push(collapse_home(&path));components.push(PluginComponent{kind:PluginComponentKind::Code,name:entry.label.clone(),source_path:collapse_home(&path),native_id:None});},
            Ok(None)=>{},Err(())=>snapshot.incomplete=true,
        }}
        sources.sort();sources.dedup();components.sort();components.dedup();
        if components.is_empty(){components.push(PluginComponent{kind:PluginComponentKind::PluginReference,name:entry.label.clone(),source_path:sources.first().cloned().unwrap_or_default(),native_id:None});}
        let package_version=if entry.targets.len()==1&&versions.len()==1{versions.into_iter().next().unwrap_or_else(||"unknown".into())}else{"unknown".into()};
        result.push(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:format!("sdk:{}",hex::encode(Sha256::digest(identity.as_bytes()))),
            name:entry.label,version:package_version,revision:format!("selected-runtime:{}; registration roles:{}; observed-global-server-array:{}; catalog membership is declaration-based; cache=file availability; private exports/ownership/loading/enablement=unknown",proof.identity(),entry.roles.into_iter().collect::<Vec<_>>().join(","),entry.selected_global),
            presence:if local{PluginPresence::Local}else{PluginPresence::Declared},enabled:None,source_paths:sources,components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()});
    }
    scope.revalidate()?;snapshot.revalidate()?;let fingerprint=snapshot.fingerprint()?;
    for item in &mut result{item.revision.push_str(&format!("; metadata:{fingerprint}"));}
    Ok(result)
}

pub(super) fn watch_paths()->Vec<PathBuf>{Scope::capture().map(|s|s.watch_paths()).unwrap_or_default()}
