use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn host() -> Host {
    serde_json::from_value(json!({
        "agent_id":"kimi-code-desktop", "adapter":"kimi_desktop",
        "docs_url":"https://www.kimi.com/code/docs/en/kimi-code-desktop/settings-and-extensions.html",
        "manifest_paths":["kimi.plugin.json",".kimi-plugin/plugin.json"],
        "default_mcp_files":[], "mcp_id_prefix":"plugin-",
        "roots":[{"path":"~/.kimi-code/plugins","layout":"kimi_registry","presence":"installed"}]
    })).unwrap()
}

fn fixture_contract() -> Contract {
    Contract {
        agent_id:"kimi-code-desktop".into(), version:"0.1.0".into(),
        bundle_id:"com.kimi.code.desktop".into(), product_name:"Kimi Code".into(),
        app_paths:vec!["/Applications/Kimi Code.app".into(),"~/secondary/Kimi Code.app".into()],
        catalog_member:"out/core.cjs".into(), builtin_skills:vec!["mcp-config".into(),"custom-theme".into()],
        guards:vec![
            Guard {path:"out/core.cjs".into(),bytes:14,sha256:hex::encode(Sha256::digest(b"fixture loader"))},
            Guard {path:"package.json".into(),bytes:2,sha256:hex::encode(Sha256::digest(b"{}"))},
        ],
    }
}

fn fixture(app: &Path, version: &str, payload: &[u8], unpacked: bool) {
    fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    let mut info=plist::Dictionary::new();
    for (key,value) in [("CFBundleIdentifier","com.kimi.code.desktop"),("CFBundleName","Kimi Code"),
        ("CFBundleShortVersionString",version),("CFBundleExecutable","Kimi Code")] {
        info.insert(key.into(),plist::Value::String(value.into()));
    }
    plist::Value::Dictionary(info).to_file_xml(app.join("Contents/Info.plist")).unwrap();
    let header=serde_json::to_vec(&json!({"files":{
        "out":{"files":{"core.cjs":{"offset":"0","size":payload.len(),"unpacked":unpacked}}},
        "package.json":{"offset":payload.len().to_string(),"size":2}
    }})).unwrap();
    let pickle=(header.len()+11)&!3;let mut raw=Vec::new();
    for word in [4,pickle as u32,(pickle-4) as u32,header.len() as u32] {raw.extend(word.to_le_bytes());}
    raw.extend(header);raw.resize(pickle+8,0);raw.extend(payload);raw.extend(b"{}");
    fs::write(app.join("Contents/Resources/app.asar"),raw).unwrap();
}

// Shipped source markers do not establish effective visibility or native MCPs.
#[test]
fn independent_desktop_catalog_is_bound_to_actual_asar_members() {
    let home=TestHome::new("kimi-desktop-catalog");let c=fixture_contract();
    fixture(&home.home.join("Applications/Kimi Code.app"),"0.1.0",b"fixture loader",false);
    let mut result=PluginScan::default();observe_contract(&host(),&c,&mut result);normalize(&mut result);
    assert_eq!(result.plugins.len(),1);assert!(result.mcps.is_empty());
    let plugin=&result.plugins[0];assert_eq!(plugin.agent_id,"kimi-code-desktop");
    assert_eq!(plugin.version,"0.1.0");assert_eq!(plugin.presence,PluginPresence::Bundled);assert_eq!(plugin.enabled,None);
    assert!(plugin.source_paths.iter().any(|p|p.ends_with("app.asar!package.json")));
    assert_eq!(plugin.components.iter().map(|c|c.name.as_str()).collect::<Vec<_>>(),vec!["mcp-config","custom-theme"]);
    assert!(plugin.components.iter().all(|c|c.kind==PluginComponentKind::Skill&&c.native_id.is_none()
        &&c.source_path.ends_with("app.asar!out/core.cjs")));
}

// A broken selected app cannot borrow a later installation or the CLI identity.
#[test]
fn changed_version_digest_and_unpacked_member_block_catalog_association() {
    let home=TestHome::new("kimi-desktop-guards");let c=fixture_contract();
    let first=home.home.join("Applications/Kimi Code.app");let second=home.home.join("secondary/Kimi Code.app");
    fixture(&second,"0.1.0",b"fixture loader",false);
    assert_eq!(verified_app(&c,&mut ScanBudget::default()).unwrap(),Some(second));
    for (version,payload,unpacked) in [("2.4.0",b"fixture loader".as_slice(),false),
        ("0.1.0",b"changed loader".as_slice(),false),("0.1.0",b"fixture loader".as_slice(),true)] {
        fixture(&first,version,payload,unpacked);
        assert!(verified_app(&c,&mut ScanBudget::default()).is_err());
        let mut result=PluginScan::default();observe_contract(&host(),&c,&mut result);
        assert!(result.unavailable);assert!(result.plugins.is_empty());assert!(result.mcps.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn symlinked_app_does_not_select_a_different_installation() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("kimi-desktop-link");let c=fixture_contract();
    let second=home.home.join("secondary/Kimi Code.app");fixture(&second,"0.1.0",b"fixture loader",false);
    fs::create_dir_all(home.home.join("Applications")).unwrap();
    symlink(&second,home.home.join("Applications/Kimi Code.app")).unwrap();
    assert!(verified_app(&c,&mut ScanBudget::default()).is_err());
}

// Shared physical declarations retain separate host identities and native IDs.
#[test]
fn shared_registry_does_not_require_or_borrow_the_sibling_runtime() {
    let home=TestHome::new("kimi-desktop-registry");let c=fixture_contract();
    let root=home.home.join("package");fs::create_dir_all(&root).unwrap();
    fs::write(root.join("kimi.plugin.json"),r#"{"name":"other-name","mcpServers":{" tool ":{"command":"node","env":{"TOKEN":"PRIVATE_VALUE"}}}}"#).unwrap();
    fs::create_dir_all(home.home.join(".kimi-code/plugins")).unwrap();
    fs::write(home.home.join(".kimi-code/plugins/installed.json"),serde_json::to_vec(&json!({"plugins":[{"id":"CasePlug","root":root,"enabled":false}]})).unwrap()).unwrap();
    let mut unknown=PluginScan::default();observe_contract(&host(),&c,&mut unknown);normalize(&mut unknown);
    assert!(unknown.unavailable);assert!(unknown.mcps.is_empty());
    assert_eq!(unknown.plugins.len(),1);assert_eq!(unknown.plugins[0].presence,PluginPresence::Declared);
    fixture(&home.home.join("Applications/Kimi Code.app"),"0.1.0",b"fixture loader",false);
    let mut result=PluginScan::default();observe_contract(&host(),&c,&mut result);
    let mut cli=host();cli.agent_id="kimi-code".into();cli.adapter="kimi".into();
    super::super::kimi::observe_registered(&cli,true,&mut result);normalize(&mut result);
    assert_eq!(result.mcps.len(),2);
    assert!(result.mcps.iter().all(|m|m.server_id=="plugin-CasePlug:tool"));
    assert_eq!(result.mcps.iter().map(|m|m.agent_id.as_str()).collect::<Vec<_>>(),vec!["kimi-code","kimi-code-desktop"]);
    assert!(result.plugins.iter().all(|p|p.enabled.is_none()));
    assert!(!serde_json::to_string(&result.plugins).unwrap().contains("PRIVATE_VALUE"));
}

// Version labels use Info.plist; a target executable is never a fallback probe.
#[cfg(unix)]
#[test]
fn desktop_version_label_does_not_execute_the_target() {
    use std::os::unix::fs::PermissionsExt;
    let home=TestHome::new("kimi-desktop-passive");let app=home.home.join("Applications/Kimi Code.app");
    fixture(&app,"0.1.0",b"fixture loader",false);fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    let target=app.join("Contents/MacOS/Kimi Code");
    fs::write(&target,"#!/bin/sh\nprintf executed > \"$HOME/executed-marker\"\n").unwrap();
    fs::set_permissions(&target,fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(crate::application::agent_launch::runtime_version("kimi-code-desktop").unwrap().as_deref(),Some("0.1.0"));
    assert!(!home.home.join("executed-marker").exists());
}
