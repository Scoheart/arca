//! CoCo CLI Node SEA resources. Desktop uses an independent native contract.
use super::*;
mod runtime;
mod metadata;
mod declarations;

#[derive(Deserialize,serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Contract{schema_version:u32,agent_id:String,runtime:runtime::Contract,research_sha256:String,authority_sha256:String,app_owned_skills:Vec<String>,audited_manifests:Vec<runtime::Guard>}
fn digest(v:&str)->bool{v.len()==64&&v.bytes().all(|b|b.is_ascii_digit()||matches!(b,b'a'..=b'f'))}
impl Contract{
 fn validate(&self)->Result<(),()>{
  self.runtime.validate()?;
  if self.schema_version!=1||self.agent_id!="cortex-code"||!digest(&self.research_sha256)||!digest(&self.authority_sha256)||self.app_owned_skills.len()>512||self.audited_manifests.len()!=4{return Err(());}
  for p in &self.app_owned_skills{let tail=Path::new(p);if tail.is_absolute()||tail.components().any(|c|!matches!(c,Component::Normal(_)))||tail.file_name().is_none_or(|n|n!="SKILL.md"){return Err(());}}
  for g in &self.audited_manifests{if g.bytes==0||!digest(&g.sha256)||!g.path.ends_with("/plugin.json")||Path::new(&g.path).is_absolute()||Path::new(&g.path).components().any(|c|!matches!(c,Component::Normal(_))){return Err(());}}
  Ok(())
 }
}
fn contract()->Option<&'static Contract>{
 static C:OnceLock<Option<Contract>>=OnceLock::new();
 C.get_or_init(||serde_json::from_str::<Contract>(include_str!("../../../../data/native-plugin-coco-cli.json")).ok().filter(|v|v.validate().is_ok())).as_ref()
}
fn selected_default()->Result<Option<PathBuf>,()>{
 use crate::application::agent_launch::{info_unlocked,LaunchTarget};
 let info=info_unlocked("cortex-code").map_err(|_|())?;if info.configured_target.is_some(){return Err(());}
 match info.resolved_target{Some(LaunchTarget::Cli{command,args,env})if args.is_empty()&&env.is_empty()=>Ok(Some(PathBuf::from(command))),None=>Ok(None),_=>Err(())}
}
pub(super) fn runtime_version(selected:&Path)->Option<String>{
 let c=contract()?;let before=selected_default().ok()??;if before!=selected{return None;}
 let proof=runtime::Proof::capture(selected,&c.runtime).ok()?;proof.revalidate().ok()?;if selected_default().ok()??!=before{return None;}Some(c.runtime.version.clone())
}
pub(super) fn observe(host:&Host,result:&mut PluginScan){
 let Some(c)=contract()else{result.unavailable=true;return;};observe_contract(host,c,result);
}
fn observe_contract(host:&Host,c:&Contract,result:&mut PluginScan){
 if c.validate().is_err()||host.roots.iter().any(|r|r.layout!="coco_cli_global"){result.unavailable=true;return;}
 let selected=match selected_default(){Ok(Some(p))=>p,Ok(None)=>return,Err(())=>{result.unavailable=true;return;}};
 let Ok(proof)=runtime::Proof::capture(&selected,&c.runtime)else{result.unavailable=true;return;};
 let mut snapshot=metadata::Snapshot::default();let mut rows=declarations::collect(host,&proof,&mut snapshot);
 if snapshot.revalidate().is_err()||proof.revalidate().is_err()||selected_default()!=Ok(Some(selected)){result.unavailable=true;return;}
 let fingerprint=match snapshot.fingerprint(){Ok(v)=>v,Err(())=>{result.unavailable=true;return;}};
 let descriptor=hex::encode(Sha256::digest(serde_json::to_vec(c).unwrap_or_default()));
 for row in &mut rows{row.revision.push_str(&format!("; selected-carrier:{}; metadata:{fingerprint}; descriptor:{descriptor}; loading/policy/frontmatter=unknown",proof.identity()));}
 result.plugins.extend(rows);result.unavailable|=snapshot.incomplete;
}
pub(super) fn watch_paths(host:&Host)->Vec<PathBuf>{
 if host.roots.iter().any(|r|r.layout!="coco_cli_global"){return Vec::new();}
 let mut paths=Vec::new();if let Ok(root)=declarations::global_root(){paths.extend([root.join("settings.json"),root.join("plugins")]);}
 if let (Some(c),Ok(Some(selected)))=(contract(),selected_default()){
  paths.push(selected.clone());if let Ok(proof)=runtime::Proof::capture(&selected,&c.runtime){paths.extend(proof.watch_paths());let mut snapshot=metadata::Snapshot::default();let _=declarations::collect(host,&proof,&mut snapshot);paths.extend(snapshot.watches);}
 }
 paths.into_iter().filter(|p|checked_root(p).is_ok()).collect::<BTreeSet<_>>().into_iter().collect()
}

#[cfg(test)]
mod tests;
