//! Isolated regression source. Fast mode retains these cases without running them.
#![cfg(unix)]
use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn approved_index(c:&mut Contract)->Vec<u8>{
    let records=c.index.records.iter().map(|r|{let mut metadata=json!({"name":r.name,"description":"isolated public metadata fixture"});
        if let Some(version)=&r.version{metadata["version"]=json!(version);}json!({"source":r.source,"folderName":r.folder,
        "stamp":[r.bytes.to_string(),r.mode.to_string()],"contentDigest":"a".repeat(64),"metadata":metadata})}).collect::<Vec<_>>();
    let payload=json!({"format":3,"appVersion":c.version,"sources":["skills","profile-skills"],"records":records});
    c.index.payload_digest=hex::encode(Sha256::digest(serde_json::to_vec(&payload).unwrap()));
    let bytes=serde_json::to_vec(&json!({"payload":payload,"digest":c.index.payload_digest})).unwrap();
    seal(&mut c.index.guard,&bytes);bytes
}
fn seal(g:&mut Guard,bytes:&[u8]){g.bytes=bytes.len()as u64;g.sha256=hex::encode(Sha256::digest(bytes));}
fn rewrite_index(c:&mut Contract,bytes:&[u8],edit:impl FnOnce(&mut Value))->Vec<u8>{
    let mut value:Value=serde_json::from_slice(bytes).unwrap();edit(&mut value);let bytes=serde_json::to_vec(&value).unwrap();seal(&mut c.index.guard,&bytes);bytes
}

#[test]
fn approved_public_metadata_has_fifteen_app_and_one_profile_skill(){
    let _home=TestHome::new("qwenwork-cn-index");let mut c=contract().unwrap().clone();c.validate().unwrap();
    let bytes=approved_index(&mut c);let rows=parse_index(&c,&bytes).unwrap();
    assert_eq!(rows.len(),16);assert_eq!(rows.iter().filter(|r|r.source=="skills").count(),15);
    assert_eq!(rows.iter().filter(|r|r.source=="profile-skills").count(),1);
    assert!(rows.iter().all(|r|!r.path.contains("agent-sdk")&&!r.path.contains("legokits")));
}
#[test]
fn published_skill_version_can_be_unknown_without_borrowing_app_version(){
    let _home=TestHome::new("qwenwork-cn-version");let mut c=contract().unwrap().clone();let bytes=approved_index(&mut c);
    let rows=parse_index(&c,&bytes).unwrap();assert_eq!(rows.iter().find(|r|r.folder=="media-read").unwrap().version,None);
    assert_eq!(rows.iter().find(|r|r.folder=="create-skill").unwrap().version.as_deref(),Some("1.2.1"));
}
#[test]
fn index_digest_or_app_version_mismatch_never_uses_authored_records_as_fallback(){
    let _home=TestHome::new("qwenwork-cn-bad-index");let mut c=contract().unwrap().clone();let bytes=approved_index(&mut c);
    let bad=rewrite_index(&mut c,&bytes,|v|v["digest"]=json!("0".repeat(64)));assert!(parse_index(&c,&bad).is_err());
    let bad=rewrite_index(&mut c,&bytes,|v|v["payload"]["appVersion"]=json!("99.0"));assert!(parse_index(&c,&bad).is_err());
    assert!(parse_index(&c,b"{broken").is_err());
}
#[test]
fn duplicate_folder_or_foreign_source_is_not_an_app_profile_skill(){
    let _home=TestHome::new("qwenwork-cn-foreign-source");let mut c=contract().unwrap().clone();let bytes=approved_index(&mut c);
    let bad=rewrite_index(&mut c,&bytes,|v|v["payload"]["records"][1]["folderName"]=json!("create-skill"));assert!(parse_index(&c,&bad).is_err());
    let bad=rewrite_index(&mut c,&bytes,|v|v["payload"]["records"][0]["source"]=json!("agent-sdk"));assert!(parse_index(&c,&bad).is_err());
}
#[test]
fn metadata_index_whole_byte_guard_precedes_native_shape_parsing(){
    let _home=TestHome::new("qwenwork-cn-index-guard");let mut c=contract().unwrap().clone();let mut bytes=approved_index(&mut c);bytes.push(b' ');
    assert!(parse_index(&c,&bytes).is_err());
}
#[test]
fn cn_market_manifest_priority_is_separate_from_runtime_catalog_conventions(){
    let _home=TestHome::new("qwenwork-cn-market-priority");let c=contract().unwrap();let entry=c.catalog.entries.iter().find(|e|e.folder=="产品设计").unwrap();
    assert_eq!(entry.manifests.iter().map(|m|m.path.rsplit('/').nth(1).unwrap()).collect::<Vec<_>>(),vec![".qwenwork-plugin",".qoder-plugin",".claude-plugin"]);
    assert_eq!(select_manifest(entry,&[false,true,true]),Ok(Some(1)));
    assert!(select_manifest(entry,&[true,true,true]).is_err());
    assert_eq!(select_manifest(entry,&[false,false,true]),Ok(Some(2)));
    assert_eq!(select_manifest(entry,&[false,false,false]),Ok(None));
}
#[test]
fn malformed_selected_manifest_does_not_select_a_secondary_variant(){
    let _home=TestHome::new("qwenwork-cn-selected-manifest");let c=contract().unwrap();let entry=c.catalog.entries.iter().find(|e|e.folder=="产品设计").unwrap();
    let chosen=select_manifest(entry,&[false,true,true]).unwrap().unwrap();assert_eq!(chosen,1);
    assert!(manifest_metadata(b"{broken",&entry.folder).is_err());
    assert_eq!(entry.manifests[1].guard.as_ref().unwrap().sha256,entry.manifests[2].guard.as_ref().unwrap().sha256);
}
#[test]
fn typed_empty_manifest_display_name_uses_declared_name_without_version_fabrication(){
    let _home=TestHome::new("qwenwork-cn-manifest-fields");
    let m=manifest_metadata(br#"{"displayName":"","name":"declared-name"}"#,"folder").unwrap();
    assert_eq!(m.name,"declared-name");assert!(m.version.is_empty());
    assert!(manifest_metadata(br#"[]"#,"folder").is_err());
}
#[test]
fn authored_contract_rejects_unknown_fields_and_nonlocal_resource_paths(){
    let _home=TestHome::new("qwenwork-cn-contract");let mut v:Value=serde_json::from_str(include_str!("../../../../../data/native-plugin-qwenwork-cn.json")).unwrap();
    v["foreign_scope"]=json!(true);assert!(serde_json::from_value::<Contract>(v).is_err());
    let mut c=contract().unwrap().clone();c.index.records[0].folder="../private".into();assert!(c.validate().is_err());
    for path in ["../escape","https://user:secret@example.test/file","a\\b","/absolute"]{assert!(local(path).is_err());}
}

struct Fixture{home:TestHome,c:Contract,host:Host,app:PathBuf}
fn put(path:&Path,bytes:&[u8]){fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,bytes).unwrap();}
fn fixture()->Fixture{
    let home=TestHome::new("qwenwork-cn-public");let mut c=contract().unwrap().clone();let app=home.home.join("Applications/QwenWorkCN.app");
    fs::create_dir_all(&app).unwrap();let body=b"PRIVATE_COMPONENT_BODY\xff";
    for record in &mut c.index.records{use std::os::unix::fs::MetadataExt;let p=app.join(format!("Contents/Resources/{}/{}/SKILL.md",record.source,record.folder));put(&p,body);record.bytes=body.len()as u64;record.mode=fs::metadata(&p).unwrap().mode();}
    let index=approved_index(&mut c);put(&app.join(&c.index.guard.path),&index);
    for source in &mut c.catalog.entries{
        for leaf in &mut source.skills{put(&app.join(&leaf.path),body);leaf.bytes=body.len()as u64;}
        let metadata=serde_json::to_vec(&json!({"name":source.folder,"displayName":source.name,"version":source.version})).unwrap();
        for candidate in &mut source.manifests{if let Some(g)=&mut candidate.guard{put(&app.join(&candidate.path),&metadata);seal(g,&metadata);}}
    }
    let catalog=serde_json::to_vec(&json!({"plugins":c.catalog.entries.iter().map(|e|json!({"plugin_id":e.catalog_id,"plugin_name":e.folder,"display_name":e.name,"version":e.version})).collect::<Vec<_>>()})).unwrap();
    seal(&mut c.catalog.guard,&catalog);put(&app.join(&c.catalog.guard.path),&catalog);
    c.asar.header_json_bytes=16;c.asar.payload_offset=32;c.asar.header_sha256="f".repeat(64);let mut archive=vec![0u8;32];
    for m in &mut c.asar.members{let bytes=if m.path==PACKAGE{serde_json::to_vec(&json!({"name":c.package_name,"version":c.version,"main":c.package_main})).unwrap()}else{format!("INERT_PUBLIC_ENTRY_{}",m.path).into_bytes()};m.offset=archive.len()as u64;m.bytes=bytes.len()as u64;m.sha256=hex::encode(Sha256::digest(&bytes));archive.extend(bytes);}
    let mut info=plist::Dictionary::new();for(k,v)in[("CFBundleIdentifier",c.bundle_id.clone()),("CFBundleName","QwenWorkCN".into()),("CFBundleDisplayName","QwenWorkCN".into()),("CFBundleExecutable","QwenWorkCN".into()),("CFBundleShortVersionString",c.version.clone()),("CFBundleVersion",c.build.clone())]{info.insert(k.into(),plist::Value::String(v));}
    let mut hash=plist::Dictionary::new();hash.insert("algorithm".into(),plist::Value::String("SHA256".into()));hash.insert("hash".into(),plist::Value::String(c.asar.header_sha256.clone()));let mut entries=plist::Dictionary::new();entries.insert("Resources/app.asar".into(),plist::Value::Dictionary(hash));info.insert("ElectronAsarIntegrity".into(),plist::Value::Dictionary(entries));let mut plist=Vec::new();plist::Value::Dictionary(info).to_writer_xml(&mut plist).unwrap();
    for g in &mut c.guards{let bytes=match g.path.as_str(){"Contents/Info.plist"=>plist.clone(),ASAR=>archive.clone(),_=>b"INERT_MACHO_FIXTURE".to_vec()};put(&app.join(&g.path),&bytes);seal(g,&bytes);}
    let host=Host{agent_id:c.agent_id.clone(),adapter:"qwenwork_cn".into(),docs_url:"https://qwenwork.cn/docs/features/skills".into(),manifest_paths:Vec::new(),default_mcp_files:Vec::new(),mcp_id_prefix:String::new(),resource_groups:Vec::new(),roots:["/Applications/QwenWorkCN.app","~/Applications/QwenWorkCN.app"].into_iter().map(|path|Root{path:path.into(),layout:"qwenwork_cn_app".into(),presence:PluginPresence::Bundled,complete_marker:None}).collect()};
    c.validate().unwrap();Fixture{home,c,host,app}
}
fn proof(f:&Fixture)->Proof{let mut budget=Budget::default();let app=Bound::capture(f.app.clone(),true,&mut budget).unwrap();let mut files=BTreeMap::new();for g in &f.c.guards{files.insert(g.path.clone(),Bound::capture(f.app.join(&g.path),false,&mut budget).unwrap());}Proof{app,files,identity:"isolated-approved-carrier".into()}}
fn read(f:&Fixture)->(Vec<NativePluginObservation>,Snapshot){let p=proof(f);let mut s=Snapshot::default();let rows=source_inventory(&f.host,&f.c,&p,&mut s);p.revalidate().unwrap();s.revalidate().unwrap();(rows,s)}

#[test]
fn source_inventory_has_only_seventeen_bundled_sources_and_156_opaque_skills(){
    let f=fixture();let(rows,s)=read(&f);assert!(!s.incomplete);assert_eq!(rows.len(),17);assert_eq!(rows.iter().flat_map(|r|&r.components).count(),156);
    assert!(rows.iter().all(|r|r.enabled.is_none()&&r.presence==PluginPresence::Bundled));assert!(rows.iter().flat_map(|r|&r.components).all(|c|c.kind==PluginComponentKind::Skill&&c.native_id.is_none()));
    assert!(s.documents.keys().all(|p|p.extension().and_then(|x|x.to_str())==Some("json")));assert!(!serde_json::to_string(&rows).unwrap().contains("PRIVATE_COMPONENT_BODY"));
}
#[test]
fn missing_or_invalid_index_drops_only_app_metadata_without_reading_skill_bodies(){
    let mut f=fixture();let p=f.app.join(&f.c.index.guard.path);fs::remove_file(&p).unwrap();let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),15);assert!(rows.iter().all(|r|r.plugin_id.starts_with("@market-source/")));
    let bad=b"{broken";put(&p,bad);seal(&mut f.c.index.guard,bad);let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),15);assert!(s.documents.keys().all(|p|!p.ends_with("SKILL.md")));
}
#[test]
fn profile_duplicate_uses_main_metadata_without_reading_shadowed_profile_body(){
    let f=fixture();put(&f.app.join("Contents/Resources/profile-skills/create-skill/SKILL.md"),b"SHADOWED_PRIVATE_BODY\xff");let(rows,s)=read(&f);assert!(!s.incomplete);
    assert_eq!(rows.iter().filter(|r|r.plugin_id.starts_with("@builtin/")).flat_map(|r|&r.components).filter(|c|c.name=="create-skill").count(),1);assert_eq!(rows.len(),17);
}
#[test]
fn unknown_preferred_cn_manifest_is_not_read_or_replaced_by_a_legacy_variant(){
    let f=fixture();let e=f.c.catalog.entries.iter().find(|e|e.folder=="产品设计").unwrap();let preferred=f.app.join(&e.manifests[0].path);put(&preferred,b"UNREVIEWED_PREFERRED_SOURCE\xff");
    let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),16);assert!(!s.documents.contains_key(&preferred));
}
#[test]
fn malformed_primary_manifest_keeps_other_sources_and_never_reads_the_secondary(){
    let mut f=fixture();let e=f.c.catalog.entries.iter_mut().find(|e|e.folder=="产品设计").unwrap();let p=f.app.join(&e.manifests[1].path);let secondary=f.app.join(&e.manifests[2].path);put(&p,b"{broken");seal(e.manifests[1].guard.as_mut().unwrap(),b"{broken");
    let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),16);assert!(!s.documents.contains_key(&secondary));
}
#[test]
fn an_unsafe_market_skill_leaf_does_not_hide_independent_candidates(){
    use std::os::unix::fs::symlink;let f=fixture();let leaf=f.app.join(&f.c.catalog.entries[0].skills[0].path);fs::remove_file(&leaf).unwrap();let target=f.home.home.join("opaque-private.txt");put(&target,b"PRIVATE_FIXTURE\xff");symlink(&target,&leaf).unwrap();
    let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),17);assert_eq!(rows.iter().flat_map(|r|&r.components).count(),155);assert!(!s.documents.contains_key(&target));
}
#[test]
fn metadata_hardlinks_and_replaced_paths_fail_closed(){
    let f=fixture();let path=f.app.join(&f.c.index.guard.path);fs::hard_link(&path,f.home.home.join("metadata-copy.json")).unwrap();let(rows,s)=read(&f);assert!(s.incomplete);assert_eq!(rows.len(),15);assert!(!s.documents.contains_key(&path));
    fs::remove_file(f.home.home.join("metadata-copy.json")).unwrap();let mut s=Snapshot::default();s.approved(&f.app,&f.c.index.guard).unwrap().unwrap();let old=f.home.home.join("old-index.json");fs::rename(&path,&old).unwrap();fs::copy(&old,&path).unwrap();assert!(s.approved(&f.app,&f.c.index.guard).is_err());assert!(s.revalidate().is_err());
}
#[test]
fn passive_version_selection_is_bound_to_the_same_physical_app_and_guard_snapshot(){
    let f=fixture();let p=proof(&f);assert!(p.selected(&f.app).is_ok());assert!(p.selected(&f.app.join("Contents/MacOS/QwenWorkCN")).is_ok());
    assert!(p.selected(Path::new("/Applications/QwenWorkCN.app")).is_err());assert!(p.selected(&f.app.join("Contents/Resources/app.asar")).is_err());
    let executable=f.app.join("Contents/MacOS/QwenWorkCN");let prior=f.home.home.join("prior-executable");fs::rename(&executable,&prior).unwrap();fs::copy(&prior,&executable).unwrap();assert!(p.selected(&f.app).is_err());
}
#[test]
fn streaming_guard_cache_reuses_only_the_unchanged_physical_identity(){
    let f=fixture();let mut budget=Budget::default();let app=Bound::capture(f.app.clone(),true,&mut budget).unwrap();let g=f.c.guards.iter().find(|g|g.path=="Contents/MacOS/QwenWorkCN").unwrap();let cache=Mutex::new(Cache::default());
    let mut file=Bound::capture(f.app.join(&g.path),false,&mut budget).unwrap();hash(&mut file,&app,&g.sha256,&cache,&mut budget).unwrap();assert_eq!(budget.hashed,g.bytes);
    let mut cached_budget=Budget::default();let mut same=Bound::capture(f.app.join(&g.path),false,&mut cached_budget).unwrap();hash(&mut same,&app,&g.sha256,&cache,&mut cached_budget).unwrap();assert_eq!(cached_budget.hashed,0);
    put(&f.app.join(&g.path),&vec![b'X';g.bytes as usize]);let mut changed=Bound::capture(f.app.join(&g.path),false,&mut cached_budget).unwrap();assert!(hash(&mut changed,&app,&g.sha256,&cache,&mut cached_budget).is_err());
}
#[test]
fn cn_watch_scope_never_includes_private_identity_or_old_writer_roots(){
    let f=fixture();let paths=watch_contract(&f.host,&f.c);assert!(paths.contains(&f.app.join(&f.c.index.guard.path)));assert!(paths.contains(&f.app.join(&f.c.catalog.entries[0].manifests[0].path)));
    assert!(paths.iter().all(|p|!p.starts_with(f.home.home.join(".qwenworkcn"))&&!p.ends_with("agents.db")&&!p.ends_with("mcp.json")));assert_eq!(runtime_version(&f.home.home.join("unknown.app")),None);
}
#[test]
fn source_snapshots_keep_finite_descriptors_in_an_isolated_child(){
    use std::os::unix::process::CommandExt;
    if std::env::var_os("ARCA_TEST_QWENWORK_CN_FD_CASE").as_deref()!=Some(std::ffi::OsStr::new("1")){
        let _home=TestHome::new("qwenwork-cn-fd-parent");let mut command=std::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","resources::plugin::qwenwork_cn::tests::source_snapshots_keep_finite_descriptors_in_an_isolated_child","--test-threads=1","--nocapture"]).env("ARCA_TEST_QWENWORK_CN_FD_CASE","1");
        unsafe{command.pre_exec(||rustix::process::setrlimit(rustix::process::Resource::Nofile,rustix::process::Rlimit{current:Some(128),maximum:Some(128)}).map_err(|e|std::io::Error::from_raw_os_error(e.raw_os_error())));}
        let output=command.output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));assert!(String::from_utf8_lossy(&output.stdout).contains("qwenwork-cn-low-fd:156"));return;
    }
    let f=fixture();let guards=(0..27).map(|_|fs::File::open(f.app.join("Contents/MacOS/QwenWorkCN")).unwrap()).collect::<Vec<_>>();let(rows,s)=read(&f);
    assert!(!s.incomplete);assert_eq!(rows.iter().flat_map(|r|&r.components).count(),156);assert_eq!(guards.len(),27);println!("qwenwork-cn-low-fd:156");
}
