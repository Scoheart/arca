use super::*;
use crate::testenv::TestHome;
use serde_json::json;
use std::ffi::OsStr;

const ASAR:&[u8]=b"fixture packed desktop loader";
const DAEMON:&[u8]=b"#!/bin/sh\nprintf executed > \"$HOME/factory-executed-marker\"\n";
fn host() -> Host {serde_json::from_value(json!({
    "agent_id":"factory-desktop","adapter":"factory_desktop","docs_url":"https://docs.factory.com/harness/plugins",
    "manifest_paths":[],"default_mcp_files":["mcp.json"],"mcp_id_prefix":"",
    "roots":[{"path":"~/.factory/plugins","layout":"factory_installed_registry","presence":"installed"}]
})).unwrap()}
fn write(path:&Path,body:&[u8]) {fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,body).unwrap();}
fn contract() -> Contract {serde_json::from_value(json!({
    "agent_id":"factory-desktop","version":"0.1.0","bundle_id":"com.electron.factory","product_name":"Factory",
    "app_paths":["~/Applications/Factory.app","~/secondary/Factory.app"],
    "guards":[
        {"path":"Contents/Resources/app.asar","bytes":ASAR.len(),"sha256":hex::encode(Sha256::digest(ASAR))},
        {"path":"Contents/Resources/bin/droid","bytes":DAEMON.len(),"sha256":hex::encode(Sha256::digest(DAEMON))}
    ]
})).unwrap()}
fn app_fixture(app:&Path,version:&str,bundle:&str) {
    let mut info=plist::Dictionary::new();
    for (key,value) in [("CFBundleIdentifier",bundle),("CFBundleName","Factory"),
        ("CFBundleShortVersionString",version),("CFBundleExecutable","Factory")] {
        info.insert(key.into(),plist::Value::String(value.into()));
    }
    fs::create_dir_all(app.join("Contents")).unwrap();
    plist::Value::Dictionary(info).to_file_xml(app.join("Contents/Info.plist")).unwrap();
    write(&app.join("Contents/Resources/app.asar"),ASAR);
    write(&app.join("Contents/Resources/bin/droid"),DAEMON);
    write(&app.join("Contents/MacOS/Factory"),DAEMON);
    #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;
        for file in ["Contents/Resources/bin/droid","Contents/MacOS/Factory"] {
            fs::set_permissions(app.join(file),fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}
fn registration(base:&Path,package:&Path) {
    write(&base.join("plugins/installed_plugins.json"),serde_json::to_vec(&json!({"plugins":{"registered@market":[{
        "scope":"user","installPath":package,"version":"registered-revision","installedAt":"metadata",
        "lastUpdated":"metadata","source":"PRIVATE_SOURCE"
    }]}})).unwrap().as_slice());
    write(&package.join("mcp.json"),br#"{"mcpServers":{"raw-key":{"command":"PRIVATE_COMMAND","env":{"TOKEN":"PRIVATE_ENV"}}}}"#);
    write(&package.join("skills/example/SKILL.md"),b"PRIVATE_COMPONENT_BODY");
}
fn scan(c:&Contract,base:&Path) -> PluginScan {
    let mut result=PluginScan::default();observe_contract_at(&host(),c,base,&mut result);normalize(&mut result);result
}

#[test]
fn desktop_uses_its_bundled_backend_without_a_droid_on_path() {
    let home=TestHome::new("factory-desktop-independent");let app=home.home.join("Applications/Factory.app");
    app_fixture(&app,"0.1.0","com.electron.factory");let base=home.home.join(".factory");registration(&base,&home.home.join("registered"));
    assert!(crate::application::agent_launch::find_command("droid").is_none());
    let result=scan(&contract(),&base);assert!(!result.unavailable);assert_eq!(result.plugins.len(),1);assert_eq!(result.mcps.len(),1);
    assert_eq!(result.plugins[0].agent_id,"factory-desktop");assert_eq!(result.plugins[0].version,"registered-revision");
    assert_eq!(result.mcps[0].server_id,"raw-key");assert!(result.plugins[0].enabled.is_none());
    assert!(!serde_json::to_string(&(&result.plugins,&result.mcps)).unwrap().contains("PRIVATE_"));
    assert!(!home.home.join("factory-executed-marker").exists());
}

#[test]
fn absent_app_never_borrows_cli_or_reads_private_registrations() {
    let home=TestHome::new("factory-desktop-absent");let base=home.home.join(".factory");
    write(&home.home.join("bin/droid"),DAEMON);write(&base.join("plugins/installed_plugins.json"),b"INVALID_PRIVATE_REGISTRY");
    let result=scan(&contract(),&base);assert!(!result.unavailable);assert!(result.plugins.is_empty()&&result.mcps.is_empty());
    assert_eq!(result.budget.bytes,0);assert!(!home.home.join("factory-executed-marker").exists());
}

#[test]
fn bundle_version_identity_and_both_images_independently_gate_association() {
    let home=TestHome::new("factory-desktop-identity");let app=home.home.join("Applications/Factory.app");let c=contract();
    let base=home.home.join(".factory");registration(&base,&home.home.join("registered"));
    for (version,bundle,changed) in [("9.0.0","com.electron.factory",None),
        ("0.1.0","other.app",None),("0.1.0","com.electron.factory",Some("Contents/Resources/app.asar")),
        ("0.1.0","com.electron.factory",Some("Contents/Resources/bin/droid"))] {
        app_fixture(&app,version,bundle);if let Some(file)=changed {write(&app.join(file),b"changed image");}
        let result=scan(&c,&base);assert!(result.unavailable);assert!(result.plugins.is_empty()&&result.mcps.is_empty());
        assert_eq!(result.budget.bytes,0);
    }
}

#[test]
fn broken_selected_app_does_not_fall_through_to_a_later_copy() {
    let home=TestHome::new("factory-desktop-priority");let c=contract();
    let second=home.home.join("secondary/Factory.app");app_fixture(&second,"0.1.0","com.electron.factory");
    assert_eq!(verified_app(&c,&mut ScanBudget::default()).unwrap(),Some(second));
    app_fixture(&home.home.join("Applications/Factory.app"),"9.0.0","com.electron.factory");
    assert!(verified_app(&c,&mut ScanBudget::default()).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_bundle_or_guarded_member_never_establishes_runtime_identity() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("factory-desktop-links");let c=contract();let app=home.home.join("Applications/Factory.app");
    let second=home.home.join("secondary/Factory.app");app_fixture(&second,"0.1.0","com.electron.factory");
    fs::create_dir_all(app.parent().unwrap()).unwrap();symlink(&second,&app).unwrap();
    assert!(verified_app(&c,&mut ScanBudget::default()).is_err());fs::remove_file(&app).unwrap();
    for member in ["Contents/Info.plist","Contents/Resources/app.asar","Contents/Resources/bin/droid"] {
        app_fixture(&app,"0.1.0","com.electron.factory");let file=app.join(member);fs::remove_file(&file).unwrap();symlink(second.join(member),&file).unwrap();
        assert!(verified_app(&c,&mut ScanBudget::default()).is_err());fs::remove_file(&file).unwrap();
    }
}

#[test]
fn production_home_selection_stays_distinct_from_the_cli_development_profile() {
    let home=TestHome::new("factory-desktop-home");let alternate=home.home.join("alternate");
    assert_eq!(base_from_values(Some(home.home.as_os_str()),None,None).unwrap(),home.home.join(".factory"));
    assert_eq!(super::super::factory_droid::base_from_env(Some(home.home.as_os_str()),None,None,Some(OsStr::new("development"))).unwrap(),home.home.join(".factory-dev"));
    assert_eq!(base_from_values(Some(home.home.as_os_str()),None,Some(alternate.as_os_str())).unwrap(),alternate.join(".factory"));
    assert!(base_from_values(None,None,None).is_err());
    for input in ["relative","/tmp/../escape","/tmp/line\nbreak"] {assert!(base_from_values(Some(home.home.as_os_str()),None,Some(OsStr::new(input))).is_err());}
}

#[test]
fn shared_physical_registry_keeps_separate_desktop_and_cli_host_identities() {
    let home=TestHome::new("factory-desktop-shared");let base=home.home.join(".factory");registration(&base,&home.home.join("registered"));
    app_fixture(&home.home.join("Applications/Factory.app"),"0.1.0","com.electron.factory");let mut result=scan(&contract(),&base);
    let mut cli=host();cli.agent_id="factory-droid".into();cli.adapter="factory_droid".into();
    super::super::factory_droid::observe_registered(&cli,&base,&mut result);normalize(&mut result);
    assert_eq!(result.plugins.len(),2);assert_eq!(result.mcps.len(),2);
    assert_eq!(result.mcps.iter().map(|m|m.agent_id.as_str()).collect::<BTreeSet<_>>(),BTreeSet::from(["factory-desktop","factory-droid"]));
    assert!(result.mcps.iter().all(|m|m.server_id=="raw-key"));
}

#[test]
fn watchers_enumerate_registered_roots_only_after_desktop_identity_is_bound() {
    let home=TestHome::new("factory-desktop-watch");let base=home.home.join(".factory");let package=home.home.join("registered");registration(&base,&package);
    let c=contract();let before=watch_contract_at(&host(),&c,&base);assert!(!before.contains(&package));
    assert!(before.contains(&base.join("plugins/installed_plugins.json")));
    app_fixture(&home.home.join("Applications/Factory.app"),"0.1.0","com.electron.factory");
    let after=watch_contract_at(&host(),&c,&base);assert!(after.contains(&package));
    assert!(after.contains(&home.home.join("Applications/Factory.app/Contents/Resources/bin/droid")));
    assert!(!after.iter().any(|p|p.ends_with("auth.v2.keyring")||p.ends_with("settings.json")));
}

#[test]
fn unrelated_native_addon_bytes_do_not_become_plugin_or_mcp_catalogs() {
    let home=TestHome::new("factory-desktop-addon-boundary");let app=home.home.join("Applications/Factory.app");app_fixture(&app,"0.1.0","com.electron.factory");
    write(&app.join("Contents/Resources/app.asar.unpacked/node_modules/keytar/build/Release/keytar.node"),b"UNTRUSTED_NATIVE_ADDON");
    let result=scan(&contract(),&home.home.join(".factory"));assert!(!result.unavailable);assert!(result.plugins.is_empty()&&result.mcps.is_empty());
    assert!(!home.home.join("factory-executed-marker").exists());
}
