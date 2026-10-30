//! Isolation regressions retained unrun under the authorized fast mode.
#![cfg(all(target_os="macos",target_arch="aarch64"))]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;
use std::os::unix::fs::{symlink,PermissionsExt};
struct Env(Vec<(&'static str,Option<std::ffi::OsString>)>);
impl Env{fn clear()->Self{Self(["CORTEX_HOME","SNOWFLAKE_HOME","COCO_DIST_DIR","NODE_OPTIONS","NODE_PATH"].into_iter().map(|k|{let v=std::env::var_os(k);std::env::remove_var(k);(k,v)}).collect())}}
impl Drop for Env{fn drop(&mut self){for(k,v)in &self.0{match v{Some(v)=>std::env::set_var(k,v),None=>std::env::remove_var(k)}}}}
fn put(p:&Path,b:&[u8]){fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,b).unwrap();}
struct Fixture{_env:Env,home:TestHome,c:Contract,host:Host,alias:PathBuf,base:PathBuf}
fn fixture()->Fixture{
 let home=TestHome::new("coco-cli-native");let env=Env::clear();
 let mut c:Contract=serde_json::from_str(include_str!("../../../../../data/native-plugin-coco-cli.json")).unwrap();
 c.runtime.version="1.2.3".into();c.runtime.version_file.bytes=6;c.runtime.version_file.sha256=hex::encode(Sha256::digest(b"1.2.3\n"));
 let mut bytes=vec![0u8;512];bytes[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());c.runtime.sea_offset=64;c.runtime.sea_bytes=256;c.runtime.sea_header_bytes=32;
 bytes[64..68].copy_from_slice(&0x143da20u32.to_le_bytes());bytes[68..72].copy_from_slice(&1u32.to_le_bytes());bytes[72]=1;
 c.runtime.sea_header_sha256=hex::encode(Sha256::digest(&bytes[64..96]));c.runtime.carrier.bytes=bytes.len()as u64;c.runtime.carrier.sha256=hex::encode(Sha256::digest(&bytes));
 let base=home.home.join("distribution/coco/1.2.3");let binary=base.join("cortex");put(&binary,&bytes);fs::set_permissions(&binary,fs::Permissions::from_mode(0o755)).unwrap();put(&base.join("cortex-version.txt"),b"1.2.3\n");
 let alias=home.home.join("bin/cortex");fs::create_dir_all(alias.parent().unwrap()).unwrap();symlink(&binary,&alias).unwrap();
 for folder in ["bundled_plugins","bundled_external_plugins","bundled_skills"]{fs::create_dir_all(base.join(folder)).unwrap();}
 let host=Host{agent_id:"cortex-code".into(),adapter:"coco_cli".into(),docs_url:"https://docs.snowflake.com/en/user-guide/cortex-code/cortex-code-plugins".into(),manifest_paths:vec![],default_mcp_files:vec![],mcp_id_prefix:String::new(),resource_groups:vec![],roots:vec![Root{path:"~/.snowflake/cortex".into(),layout:"coco_cli_global".into(),presence:PluginPresence::Local,complete_marker:None}]};
 Fixture{_env:env,home,c,host,alias,base}
}
fn read(f:&Fixture)->(Vec<NativePluginObservation>,metadata::Snapshot){let proof=runtime::Proof::capture(&f.alias,&f.c.runtime).unwrap();let mut s=metadata::Snapshot::default();let rows=declarations::collect(&f.host,&proof,&mut s);s.revalidate().unwrap();(rows,s)}
fn manifest(root:&Path,value:Value){put(&root.join(".cortex-plugin/plugin.json"),&serde_json::to_vec(&value).unwrap());}
fn global(f:&Fixture)->PathBuf{f.home.home.join(".snowflake/cortex")}

#[test]
fn node_sea_version_is_bound_to_selected_carrier_and_companion(){let f=fixture();f.c.validate().unwrap();let p=runtime::Proof::capture(&f.alias,&f.c.runtime).unwrap();assert_eq!(p.version(),"1.2.3");put(&f.base.join("cortex-version.txt"),b"9.9.9\n");assert!(p.revalidate().is_err());assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());}
#[test]
fn unknown_carrier_never_invokes_a_version_flag(){let f=fixture();let marker=f.home.home.join("executed");put(&f.base.join("cortex"),format!("#!/bin/sh\ntouch '{}'\n",marker.display()).as_bytes());assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());assert!(!marker.exists());}
#[test]
fn preload_or_dist_override_cannot_inherit_default_passive_identity(){let f=fixture();std::env::set_var("COCO_DIST_DIR",&f.base);assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());std::env::remove_var("COCO_DIST_DIR");std::env::set_var("NODE_OPTIONS","--require example");assert!(runtime::Proof::capture(&f.alias,&f.c.runtime).is_err());}
#[test]
fn bundled_application_skills_stop_before_fixture_descendants(){let f=fixture();put(&f.base.join("bundled_skills/root/SKILL.md"),b"PRIVATE_SKILL_BODY");put(&f.base.join("bundled_skills/root/fixtures/nested/SKILL.md"),b"PRIVATE_NESTED_BODY");let(rows,s)=read(&f);assert!(!s.incomplete);assert_eq!(s.metadata_bytes_read(),0);let r=&rows[0];assert_eq!(r.plugin_id,"@builtin/skills");assert_eq!(r.components.len(),1);assert_eq!(r.components[0].name,"root");assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_"));}
#[test]
fn skill_container_scans_only_immediate_child_directories(){let f=fixture();put(&f.base.join("bundled_skills/container/first/SKILL.md"),b"PRIVATE");put(&f.base.join("bundled_skills/container/extra/deep/SKILL.md"),b"PRIVATE");let(rows,s)=read(&f);assert!(!s.incomplete);assert_eq!(rows[0].components.len(),1);assert_eq!(rows[0].components[0].name,"first");assert_eq!(s.metadata_bytes_read(),0);}
#[test]
fn preferred_manifest_parse_error_does_not_fall_back(){let f=fixture();let p=f.base.join("bundled_plugins/example");put(&p.join(".cortex-plugin/plugin.json"),b"invalid JSON");put(&p.join(".claude-plugin/plugin.json"),br#"{"name":"fallback"}"#);let(rows,s)=read(&f);assert!(s.incomplete);assert!(rows.is_empty());}
#[test]
fn manifest_and_skill_owner_versions_are_independent(){let f=fixture();let p=f.base.join("bundled_plugins/example");manifest(&p,json!({"name":"example","version":"8.9.0"}));put(&p.join("skills/demo/SKILL.md"),b"PRIVATE_FRONTMATTER");let(rows,s)=read(&f);assert!(!s.incomplete);assert_eq!(rows[0].version,"8.9.0");assert!(rows[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill&&c.name=="demo"));assert!(rows[0].enabled.is_none());}
#[test]
fn registry_membership_and_physical_availability_are_separate(){let f=fixture();let root=global(&f);manifest(&root.join("plugins/local"),json!({"name":"local","version":"2.0.0"}));put(&root.join("plugins/registry.json"),br#"{"missing":{"source":"PRIVATE_SOURCE","installedAt":"today","active":false}}"#);let(rows,s)=read(&f);assert!(!s.incomplete);assert_eq!(rows.iter().find(|r|r.plugin_id=="local").unwrap().presence,PluginPresence::Local);assert_eq!(rows.iter().find(|r|r.plugin_id=="missing").unwrap().presence,PluginPresence::Declared);assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_SOURCE"));}
#[test]
fn one_bad_registry_entry_is_not_silently_accepted_as_valid_state(){let f=fixture();let root=global(&f);manifest(&root.join("plugins/physical"),json!({"name":"physical"}));put(&root.join("plugins/registry.json"),br#"{"bad":{"source":"x","installedAt":"today"}}"#);let(rows,s)=read(&f);assert!(s.incomplete);assert!(rows.iter().any(|r|r.plugin_id=="physical"&&r.presence==PluginPresence::Local));}
#[test]
fn empty_native_registry_is_valid_and_never_initialized(){let f=fixture();let p=global(&f).join("plugins/registry.json");put(&p,b" \n");let before=fs::read(&p).unwrap();let(rows,s)=read(&f);assert!(!s.incomplete);assert!(rows.is_empty());assert_eq!(fs::read(&p).unwrap(),before);assert!(!global(&f).join("plugins/.activation-stubs").exists());}
#[test]
fn hooks_and_cross_declared_agent_json_are_never_read_as_mcp_metadata(){let f=fixture();let root=f.base.join("bundled_plugins/example");let value=json!({"name":"example","agents":"private.json","mcpServers":["private.json","hooks/hooks.json","skills/demo/SKILL.md"]});manifest(&root,value.clone());put(&root.join("private.json"),br#"{"PRIVATE_AGENT_BODY":{}}"#);put(&root.join("hooks/hooks.json"),br#"{"PRIVATE_HOOK_BODY":{}}"#);let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(s.metadata_bytes_read(),serde_json::to_vec(&value).unwrap().len());assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_"));}
#[test]
fn inline_mcp_names_are_references_and_values_stay_private(){let f=fixture();let root=f.base.join("bundled_plugins/example");manifest(&root,json!({"name":"example","mcpServers":{"server":{"command":"PRIVATE_COMMAND","env":{"TOKEN":"PRIVATE_TOKEN"}}}}));let(rows,s)=read(&f);assert!(!s.incomplete);let c=&rows[0].components[0];assert_eq!(c.kind,PluginComponentKind::McpReference);assert_eq!(c.native_id.as_deref(),Some("example-server"));assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_"));}
#[test]
fn absolute_and_parent_component_paths_stay_opaque(){let f=fixture();let root=f.base.join("bundled_plugins/example");manifest(&root,json!({"name":"example","skills":"../../escape","mcpServers":"/private/config.json"}));let(rows,s)=read(&f);assert!(s.incomplete);assert!(rows[0].components.is_empty());}
#[test]
fn leaf_or_parent_replacement_invalidates_the_snapshot(){let f=fixture();let p=global(&f).join("settings.json");put(&p,b"{\"plugins\":[]}");let mut s=metadata::Snapshot::default();s.json(&p,DocumentFormat::Json).unwrap();fs::rename(&p,p.with_extension("old")).unwrap();put(&p,b"{\"plugins\":[]}");assert!(s.revalidate().is_err());assert!(!s.watches.contains(Path::new("/")));assert!(!s.watches.contains(&f.home.home));}
#[test]
fn unsupported_extension_is_rejected_before_any_body_read(){let f=fixture();let p=f.home.home.join("SKILL.md");put(&p,br#"{"plugins":["PRIVATE_BODY"]}"#);let mut s=metadata::Snapshot::default();assert!(s.json(&p,DocumentFormat::Json).is_err());assert_eq!(s.metadata_bytes_read(),0);}
#[test]
fn relative_runtime_config_override_is_not_resolved_into_project_scope(){let _f=fixture();std::env::set_var("CORTEX_HOME","relative-project");assert!(declarations::global_root().is_err());}
#[test]
fn shared_metadata_opener_rejects_a_symlink_at_the_home_base(){
 let f=fixture();let original=f.home.home.with_extension("held-original");let external=f.home.home.with_extension("unrelated-tree");
 assert!(!original.exists()&&!external.exists());fs::create_dir(&external).unwrap();put(&external.join("metadata.json"),b"PRIVATE_UNRELATED_BODY");
 struct Restore{home:PathBuf,original:PathBuf,external:PathBuf}
 impl Drop for Restore{fn drop(&mut self){let _=fs::remove_file(&self.home);let _=fs::rename(&self.original,&self.home);let _=fs::remove_dir_all(&self.external);}}
 fs::rename(&f.home.home,&original).unwrap();let restore=Restore{home:f.home.home.clone(),original,external:external.clone()};symlink(&external,&f.home.home).unwrap();
 let opened=open_confined_file(&f.home.home.join("metadata.json"));drop(restore);assert!(opened.is_err());
}
#[test]
fn custom_hook_json_is_opaque_while_independent_mcp_metadata_survives(){
 let f=fixture();let root=f.base.join("bundled_plugins/example");let value=json!({"name":"example","hooks":"payload.json","mcpServers":["payload.json","safe.json"]});manifest(&root,value.clone());
 put(&root.join("payload.json"),br#"{"PRIVATE_HOOK_BODY":{"command":"PRIVATE_SECRET"}}"#);let safe=br#"{"mcpServers":{"safe":{"command":"PRIVATE_MCP_VALUE"}}}"#;put(&root.join("safe.json"),safe);
 let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(s.metadata_bytes_read(),serde_json::to_vec(&value).unwrap().len()+safe.len());assert!(rows[0].components.iter().any(|c|c.kind==PluginComponentKind::McpReference&&c.name=="safe"));assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_"));
}
#[test]
fn nested_hook_file_and_unclosed_command_paths_cannot_be_mcp_documents(){
 let f=fixture();let root=f.base.join("bundled_plugins/example");let value=json!({"name":"example","hooks":{"event":{"file":"payload.json"}},"mcpServers":"payload.json"});manifest(&root,value.clone());put(&root.join("payload.json"),br#"{"PRIVATE_HOOK_BODY":{}}"#);
 let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(s.metadata_bytes_read(),serde_json::to_vec(&value).unwrap().len());assert!(rows[0].components.iter().all(|c|c.kind!=PluginComponentKind::McpReference));
 let value=json!({"name":"example","hooks":{"event":{"command":"dynamic shell body"}},"mcpServers":"payload.json"});manifest(&root,value.clone());let(_,s)=read(&f);assert!(s.incomplete);assert_eq!(s.metadata_bytes_read(),serde_json::to_vec(&value).unwrap().len());
}
