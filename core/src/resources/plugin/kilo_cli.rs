//! Kilo's CLI SDK plugin system is independent of Desktop plugin.json.
//! Observations contain metadata only and never import a module or Skill body.
use super::*;
mod runtime;
mod metadata;
mod declarations;

#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Member { index:usize, name:String, offset:u64, bytes:u64, encoding:u8 }
#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Skill { name:String, member:Member }
#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Bun {
    section_offset:u64, section_bytes:u64, graph_bytes:u64,
    table_offset:u64, table_bytes:usize, table_sha256:String,
    entry:Member, builtin_skill:Skill,
}
#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Sdk { offset:u64, bytes:usize, sha256:String, callbacks:Vec<String> }
#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract { schema_version:u32, agent_id:String, runtime:runtime::Contract, bun:Bun, sdk:Sdk, research_sha256:String, authority_sha256:String }
fn digest(value:&str)->bool {value.len()==64&&value.bytes().all(|v|v.is_ascii_digit()||matches!(v,b'a'..=b'f'))}
fn name(value:&str)->Result<String,()> {
    if value.is_empty()||value.len()>240||value.chars().all(char::is_whitespace)||value.chars().any(char::is_control){return Err(());}
    Ok(value.into())
}
fn relative_member(value:&str)->Result<(),()> {
    if value.len()>4096||value.contains('\\')||value.contains("://")||value.chars().any(char::is_control){return Err(());}
    let tail=value.strip_prefix("/$bunfs/root/").ok_or(())?;
    if tail.is_empty()||Path::new(tail).components().any(|c|!matches!(c,Component::Normal(_))){return Err(());}
    Ok(())
}
impl Contract {
    fn validate(&self)->Result<(),()> {
        self.runtime.validate()?;let b=&self.bun;
        if !digest(&self.authority_sha256)||!digest(&self.sdk.sha256)||self.sdk.bytes==0||self.sdk.bytes>65536
            ||self.sdk.offset.checked_add(self.sdk.bytes as u64).is_none_or(|v|v>self.runtime.carrier.bytes)
            ||self.sdk.callbacks.len()!=15||self.sdk.callbacks.iter().collect::<BTreeSet<_>>().len()!=15{return Err(());}
        for item in &self.sdk.callbacks{name(item)?;}
        if self.schema_version!=1||self.agent_id!="kilo-code"||!digest(&self.research_sha256)||!digest(&b.table_sha256)
            ||b.section_bytes.checked_sub(8)!=Some(b.graph_bytes)||b.graph_bytes<48||b.table_bytes==0
            ||b.table_bytes%52!=0||b.table_bytes/52>4096||b.table_bytes>65536
            ||b.section_offset.checked_add(b.section_bytes).is_none_or(|v|v>self.runtime.carrier.bytes)
            ||b.table_offset<b.section_offset+8||b.table_offset.checked_add(b.table_bytes as u64).is_none_or(|v|v>b.section_offset+8+b.graph_bytes-48)
            ||b.entry.name!="/$bunfs/root/kilo"||b.entry.encoding!=2||b.builtin_skill.name!="kilo-config" {return Err(());}
        for m in [&b.entry,&b.builtin_skill.member] {
            relative_member(&m.name)?;
            if m.index>=b.table_bytes/52||m.bytes==0||m.offset<b.section_offset+8
                ||m.offset.checked_add(m.bytes).is_none_or(|v|v>b.section_offset+8+b.graph_bytes-48){return Err(());}
        }
        name(&b.builtin_skill.name)?;Ok(())
    }
}
fn contract()->Option<&'static Contract> {
    static C:OnceLock<Option<Contract>>=OnceLock::new();
    C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-kilo-cli.json")).ok().filter(|v|v.validate().is_ok())).as_ref()
}
fn selected_default()->Result<Option<PathBuf>,()> {
    use crate::application::agent_launch::{info_unlocked,LaunchTarget};
    let info=info_unlocked("kilo-code").map_err(|_|())?;
    if info.configured_target.is_some(){return Err(());}
    match info.resolved_target {
        Some(LaunchTarget::Cli{command,args,env})if args.is_empty()&&env.is_empty()=>Ok(Some(PathBuf::from(command))),
        None=>Ok(None),_=>Err(()),
    }
}
pub(super) fn runtime_version(selected:&Path)->Option<String> {
    let c=contract()?;let before=selected_default().ok()??;if before!=selected{return None;}
    let proof=runtime::Proof::capture(selected,&c.runtime).ok()?;
    proof.revalidate().ok()?;if selected_default().ok()??!=before{return None;}
    Some(c.runtime.version.clone())
}
fn word(bytes:&[u8],at:usize)->Result<u32,()> {Ok(u32::from_le_bytes(bytes.get(at..at+4).ok_or(())?.try_into().map_err(|_|())?))}
fn wide(bytes:&[u8],at:usize)->Result<u64,()> {Ok(u64::from_le_bytes(bytes.get(at..at+8).ok_or(())?.try_into().map_err(|_|())?))}
fn check_member(proof:&mut runtime::Proof,b:&Bun,table:&[u8],m:&Member)->Result<(),()> {
    let row=table.get(m.index*52..m.index*52+52).ok_or(())?;let base=b.section_offset+8;
    let name_offset=base.checked_add(word(row,0)? as u64).ok_or(())?;let name_bytes=word(row,4)? as usize;
    if name_bytes!=m.name.len()||name_bytes>4096||name_offset.checked_add(name_bytes as u64+1).is_none_or(|v|v>base+b.graph_bytes-48)
        ||base.checked_add(word(row,8)? as u64)!=Some(m.offset)||word(row,12)? as u64!=m.bytes||row[48]!=m.encoding{return Err(());}
    let raw=proof.read_metadata(name_offset,name_bytes+1)?;
    if raw.last()!=Some(&0)||raw.get(..raw.len()-1)!=Some(m.name.as_bytes()){return Err(());}
    Ok(())
}
fn check_bun(proof:&mut runtime::Proof,b:&Bun)->Result<(),()> {
    let header=proof.read_metadata(0,32)?;
    if word(&header,0)?!=0xfeedfacf||word(&header,4)?!=0x0100000c{return Err(());}
    if wide(&proof.read_metadata(b.section_offset,8)?,0)?!=b.graph_bytes{return Err(());}
    let footer=proof.read_metadata(b.section_offset+8+b.graph_bytes-48,48)?;
    if footer.get(32..)!=Some(b"\n---- Bun! ----\n".as_slice())||wide(&footer,0)?!=b.graph_bytes-48
        ||b.section_offset+8+word(&footer,8)? as u64!=b.table_offset||word(&footer,12)? as usize!=b.table_bytes||word(&footer,16)? as usize!=b.entry.index{return Err(());}
    let table=proof.read_metadata(b.table_offset,b.table_bytes)?;
    if hex::encode(Sha256::digest(&table))!=b.table_sha256{return Err(());}
    check_member(proof,b,&table,&b.entry)?;check_member(proof,b,&table,&b.builtin_skill.member)?;proof.revalidate()
}
fn bundled_skills(host:&Host,c:&Contract,proof:&mut runtime::Proof)->Result<NativePluginObservation,()> {
    check_bun(proof,&c.bun)?;let skill=&c.bun.builtin_skill;
    let source=format!("{}!{}",collapse_home(proof.carrier_path()),skill.member.name);
    Ok(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:"@builtin/skills".into(),name:"Kilo bundled Skills".into(),
        version:c.runtime.version.clone(),revision:format!("selected-runtime-sha256:{}; application-owned; builtin member availability; body/frontmatter/overrides/loading=unknown",proof.identity()),
        presence:PluginPresence::Bundled,enabled:None,source_paths:vec![source.clone()],components:vec![PluginComponent{kind:PluginComponentKind::Skill,
            name:skill.name.clone(),source_path:source,native_id:None}],docs_url:host.docs_url.clone(),source_fingerprint:String::new()})
}

fn bundled_sdk(host:&Host,c:&Contract,proof:&mut runtime::Proof)->Result<NativePluginObservation,()> {
    let raw=proof.read_metadata(c.sdk.offset,c.sdk.bytes)?;
    if hex::encode(Sha256::digest(&raw))!=c.sdk.sha256{return Err(());}
    let source=format!("{}!{}",collapse_home(proof.carrier_path()),c.bun.entry.name);
    let components=c.sdk.callbacks.iter().map(|name|PluginComponent{kind:PluginComponentKind::Code,name:name.clone(),source_path:source.clone(),native_id:None}).collect();
    Ok(NativePluginObservation{agent_id:host.agent_id.clone(),plugin_id:"@builtin/sdk".into(),name:"Kilo built-in SDK integrations".into(),version:c.runtime.version.clone(),
        revision:format!("selected-runtime:{}; compiler-bound callbacks:{}; application-owned code; factory/policy/enablement/loading=unknown",proof.identity(),c.sdk.sha256),
        presence:PluginPresence::Bundled,enabled:None,source_paths:vec![source],components,docs_url:host.docs_url.clone(),source_fingerprint:String::new()})
}
pub(super) fn observe(host:&Host,result:&mut PluginScan){
    let Some(c)=contract()else{result.unavailable=true;return;};observe_contract(host,c,result);
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan){
    if host.roots.iter().any(|v|v.layout!="kilo_cli_global"){result.unavailable=true;return;}
    if c.validate().is_err(){result.unavailable=true;return;}
    let selected=match selected_default(){Ok(Some(v))=>v,Ok(None)=>return,Err(())=>{result.unavailable=true;return;}};
    let Ok(mut proof)=runtime::Proof::capture(&selected,&c.runtime)else{result.unavailable=true;return;};
    let mut bundled=Vec::new();
    for item in [bundled_skills(host,c,&mut proof),bundled_sdk(host,c,&mut proof)]{
        match item{Ok(v)=>bundled.push(v),Err(())=>{result.unavailable=true;return;}}
    }
    let mut snapshot=metadata::Snapshot::default();
    let declarations=match declarations::collect(host,&proof,&mut snapshot){Ok(v)=>v,Err(())=>{snapshot.incomplete=true;Vec::new()}};
    if proof.revalidate().is_err()||selected_default()!=Ok(Some(selected)){result.unavailable=true;return;}
    let descriptor=hex::encode(Sha256::digest(serde_json::to_vec(c).unwrap_or_default()));
    for item in &mut bundled{item.revision.push_str(&format!("; descriptor:{descriptor}"));}
    result.plugins.extend(bundled);
    if snapshot.revalidate().is_ok(){result.plugins.extend(declarations);}else{snapshot.incomplete=true;}
    result.unavailable|=snapshot.incomplete;
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf>{
    if host.roots.iter().any(|v|v.layout!="kilo_cli_global"){return Vec::new();}
    let mut paths=declarations::watch_paths();
    if let Ok(Some(selected))=selected_default(){
        paths.push(selected.clone());
        if let Some(c)=contract(){if let Ok(proof)=runtime::Proof::capture(&selected,&c.runtime){
            paths.extend(proof.watch_paths());let mut snapshot=metadata::Snapshot::default();
            let _=declarations::collect(host,&proof,&mut snapshot);paths.extend(snapshot.watches);
        }}
    }
    paths.into_iter().filter(|v|checked_root(v).is_ok()).collect::<BTreeSet<_>>().into_iter().collect()
}

#[cfg(test)]
mod tests;
