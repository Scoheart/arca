use super::*;
use crate::testenv::TestHome;

fn write(root:&Path,path:&str,body:&str){let p=root.join(path);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,body).unwrap();}
fn host()->&'static Host{hosts().iter().find(|h|h.agent_id=="qwenwork").unwrap()}
fn proof(home:&Path)->Proof{let path=home.join("bound-app-fixture");fs::create_dir_all(&path).unwrap();Proof{app:Bound::capture(path,true,&mut Budget::default()).unwrap(),files:BTreeMap::new(),identity:"isolated-fixture".into()}}

#[test]
fn authored_contract_separates_native_candidates_and_bundled_install_sources(){
    let c=contract().unwrap();c.validate().unwrap();
    assert_eq!(c.groups.iter().flat_map(|g|&g.components).filter(|i|i.kind==PluginComponentKind::Skill).count(),206);
    assert_eq!(c.groups.iter().filter(|g|g.id.starts_with("@example/")).count(),15);
    assert_eq!(c.groups.iter().filter(|g|g.id.starts_with("@example/")).flat_map(|g|&g.components).filter(|i|i.kind==PluginComponentKind::Skill).count(),140);
    assert!(c.groups.iter().flat_map(|g|&g.components).all(|i|i.kind!=PluginComponentKind::Mcp));
    assert!(c.groups.iter().flat_map(|g|&g.components).filter(|i|i.kind==PluginComponentKind::Skill).all(|i|i.native_id.is_none()));
}
#[test]
fn malformed_preferred_manifest_never_reads_secondary_metadata(){
    let home=TestHome::new("qwenwork-preferred");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json","{broken");write(&root,".claude-plugin/plugin.json",r#"{"name":"must-not-win","version":"99"}"#);
    write(&root,"skills/one/SKILL.md","opaque Skill body");let mut s=Snapshot::default();
    let row=physical(host(),&proof(&home.home),&root,"package",false,&mut s).unwrap().unwrap();
    assert_eq!(row.name,"package");assert_eq!(row.version,"unknown");assert_eq!(row.enabled,None);assert!(s.incomplete);
    assert!(!s.documents.contains_key(&root.join(".claude-plugin/plugin.json")));
}
#[test]
fn native_skill_scan_is_one_level_and_keeps_body_and_runtime_name_opaque(){
    let home=TestHome::new("qwenwork-skill-level");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json",r#"{"name":"native-owner","version":"3.2.1"}"#);
    write(&root,"skills/one/SKILL.md","name: secret-runtime-name\nprivate-skill-body");
    write(&root,"skills/nested/deeper/SKILL.md","must-not-recurse");write(&root,"commands/run.md","private-command-body");
    let mut s=Snapshot::default();let row=physical(host(),&proof(&home.home),&root,"package",false,&mut s).unwrap().unwrap();
    assert_eq!(row.components.iter().filter(|v|v.kind==PluginComponentKind::Skill).count(),1);
    assert!(row.components.iter().all(|v|v.native_id.is_none()));
    let serialized=serde_json::to_string(&row).unwrap();for secret in["private-skill-body","secret-runtime-name","private-command-body","must-not-recurse"]{assert!(!serialized.contains(secret));}
    assert_eq!(s.documents.len(),1);
}
#[test]
fn custom_native_folder_shadows_installed_folder_after_manifest_discovery(){
    let home=TestHome::new("qwenwork-custom-priority");let root=home.home.join(".qwenwork");
    write(&root,"plugins/same/.qoder-plugin/plugin.json",r#"{"name":"installed","version":"1"}"#);
    write(&root,"plugins-custom/same/.qoder-plugin/plugin.json",r#"{"name":"custom","version":"2"}"#);
    write(&root,"plugins-custom/no-manifest/skills/one/SKILL.md","opaque");
    let mut s=Snapshot::default();let rows=global(host(),contract().unwrap(),&proof(&home.home),&mut s).unwrap();
    assert_eq!(rows.len(),1);assert_eq!(rows[0].name,"custom");assert_eq!(rows[0].presence,PluginPresence::Local);assert_eq!(rows[0].enabled,None);
}
#[test]
fn mcp_and_hook_alias_remains_opaque_without_losing_manifest_connector_reference(){
    let home=TestHome::new("qwenwork-hook-alias");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json",r#"{"name":"fixture","hooks":"./.mcp.json","connectors":[{"id":"service","name":"Service"}]}"#);
    write(&root,".mcp.json","private-hook-body-invalid-json");let mut s=Snapshot::default();
    let row=physical(host(),&proof(&home.home),&root,"package",false,&mut s).unwrap().unwrap();
    assert_eq!(row.components.len(),1);assert_eq!(row.components[0].kind,PluginComponentKind::McpReference);
    assert_eq!(row.components[0].name,"Service");assert!(!s.documents.contains_key(&root.join(".mcp.json")));assert!(!s.incomplete);
}
#[cfg(unix)]
#[test]
fn symlinked_root_and_hardlinked_metadata_cannot_be_read(){
    use std::os::unix::fs::symlink;
    let home=TestHome::new("qwenwork-links");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json",r#"{"name":"fixture"}"#);let link=home.home.join("alias");symlink(&root,&link).unwrap();
    assert!(Snapshot::default().json(&link.join(".qoder-plugin/plugin.json")).is_err());
    fs::hard_link(root.join(".qoder-plugin/plugin.json"),home.home.join("copy.json")).unwrap();
    assert!(Snapshot::default().json(&root.join(".qoder-plugin/plugin.json")).is_err());
}
#[test]
fn snapshot_rejects_metadata_replacement_and_preserves_central_inventory(){
    let home=TestHome::new("qwenwork-replacement");let root=home.home.join("package");write(&root,"manifest.json",r#"{"name":"one"}"#);
    let mut s=Snapshot::default();s.json(&root.join("manifest.json")).unwrap();write(&root,"replacement.json",r#"{"name":"two"}"#);fs::rename(root.join("replacement.json"),root.join("manifest.json")).unwrap();
    assert!(s.revalidate().is_err());assert!(PluginScan::default().mcps.is_empty());
}
#[test]
fn unsafe_names_and_unsupported_artifacts_never_create_execution_fallback(){
    let home=TestHome::new("qwenwork-unknown");for n in["../escape","a/b","a\\b","two..parts","bad\nname"]{assert!(!segment(n));}
    assert_eq!(runtime_version(&home.home.join("unknown.app")),None);
    assert_eq!(fresh_passive_runtime_version("qwenwork",&home.home.join("unknown.app")),Some(None));
}
#[test]
fn cn_identity_scope_cannot_reactivate_stale_fixed_directory_writers(){
    let _home=TestHome::new("qwenwork-cn-stale-scope");let builtins=crate::agents::builtin_agents();
    let mut saved=builtins["qwenwork-cn"].clone();saved.global=Some("~/.qwenworkcn/mcp.json".into());saved.key="mcpServers".into();saved.skills=builtins["qwenwork"].skills.clone();
    let mut settings=crate::settings::Settings::default();settings.agents=Some(BTreeMap::from([("qwenwork-cn".into(),saved)]));let before=serde_json::to_value(&settings).unwrap();
    let loaded=crate::agents::load_agents_from_settings(&settings);assert!(loaded["qwenwork-cn"].global.is_none());assert!(loaded["qwenwork-cn"].skills.is_none());
    assert_eq!(serde_json::to_value(&settings).unwrap(),before);
}
#[test]
fn invalid_skill_directory_does_not_hide_independent_native_commands(){
    let home=TestHome::new("qwenwork-independent-command-root");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json",r#"{"name":"fixture"}"#);write(&root,"skills","ordinary file instead of a directory");write(&root,"commands/run.md","opaque command body");
    let mut s=Snapshot::default();let row=physical(host(),&proof(&home.home),&root,"package",false,&mut s).unwrap().unwrap();
    assert!(s.incomplete);assert_eq!(row.components.iter().filter(|v|v.kind==PluginComponentKind::Command).count(),1);assert_eq!(row.components.iter().filter(|v|v.kind==PluginComponentKind::Skill).count(),0);
}
#[cfg(unix)]
#[test]
fn unsafe_skill_leaf_does_not_hide_independent_native_commands(){
    use std::os::unix::fs::symlink;
    let home=TestHome::new("qwenwork-independent-command-leaf");let root=home.home.join("package");
    write(&root,".qoder-plugin/plugin.json",r#"{"name":"fixture"}"#);write(&root,"private.txt","opaque Skill body");write(&root,"commands/run.md","opaque command body");fs::create_dir_all(root.join("skills/one")).unwrap();symlink(root.join("private.txt"),root.join("skills/one/SKILL.md")).unwrap();
    let mut s=Snapshot::default();let row=physical(host(),&proof(&home.home),&root,"package",false,&mut s).unwrap().unwrap();
    assert!(s.incomplete);assert_eq!(row.components.iter().filter(|v|v.kind==PluginComponentKind::Command).count(),1);assert_eq!(row.components.iter().filter(|v|v.kind==PluginComponentKind::Skill).count(),0);
    assert!(!serde_json::to_string(&row).unwrap().contains("opaque Skill body"));
}
#[cfg(unix)]
#[test]
fn many_metadata_candidates_fit_a_desktop_process_descriptor_limit(){
    use std::os::unix::process::CommandExt;
    if std::env::var_os("ARCA_TEST_QWENWORK_FD_CASE").as_deref()!=Some(std::ffi::OsStr::new("1")){
        let _home=TestHome::new("qwenwork-fd-parent");
        let mut command=std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","resources::plugin::qwenwork::tests::many_metadata_candidates_fit_a_desktop_process_descriptor_limit","--test-threads=1","--nocapture"])
            .env("ARCA_TEST_QWENWORK_FD_CASE","1");
        // Change only the isolated child, never the test runner or system limit.
        unsafe{command.pre_exec(||rustix::process::setrlimit(rustix::process::Resource::Nofile,rustix::process::Rlimit{current:Some(128),maximum:Some(128)}).map_err(|e|std::io::Error::from_raw_os_error(e.raw_os_error())));}
        let output=command.output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));assert!(String::from_utf8_lossy(&output.stdout).contains("qwenwork-low-fd-case:400"));return;
    }
    let home=TestHome::new("qwenwork-fd-child");let root=home.home.join("candidates");
    write(&root,"guard.bin","opaque guard fixture");
    for i in 0..400{write(&root,&format!("skill-{i}/SKILL.md"),"opaque Skill fixture");write(&root,&format!("metadata/{i}.json"),r#"{"name":"candidate"}"#);}
    let guards=(0..27).map(|_|fs::File::open(root.join("guard.bin")).unwrap()).collect::<Vec<_>>();
    let mut s=Snapshot::default();for i in 0..400{assert!(s.stat(&root.join(format!("skill-{i}/SKILL.md"))).unwrap().unwrap().file);assert!(s.json(&root.join(format!("metadata/{i}.json"))).unwrap().is_some());}
    assert_eq!(s.entries(&root).unwrap().len(),402);s.revalidate().unwrap();assert!(!s.fingerprint().unwrap().is_empty());assert_eq!(guards.len(),27);
    println!("qwenwork-low-fd-case:400");
}
