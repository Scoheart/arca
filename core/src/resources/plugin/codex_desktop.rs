//! Digest-bound desktop session MCP declarations, independent of plugin files.
//! Static constructor availability never establishes effective session state.
use super::*;
use super::electron_package::{app_version, guard_asar, Guard};
use crate::domain::mcp::BuiltinMcpObservation;
use crate::resources::mcp::builtin::BuiltinMcpScan;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contract {
    agent_id: String, version: String, bundle_id: String, product_name: String,
    app_paths: Vec<String>, docs_url: String, guards: Vec<Guard>, declarations: Vec<Declaration>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Declaration { id: String, server_id: String, name: String, member: String, symbol: String }

fn contract() -> &'static Contract {
    static VALUE: OnceLock<Contract> = OnceLock::new();
    VALUE.get_or_init(|| serde_json::from_str(include_str!("../../../../data/native-plugin-codex-desktop.json"))
        .expect("audited ChatGPT desktop session MCP contract"))
}

fn app_path(input: &str) -> PathBuf {
    if input.starts_with("~/") { expand_tilde(input) } else { crate::paths::system_probe_path(input) }
}

fn observe_app(c: &Contract, app: &Path, budget: &mut ScanBudget) -> Result<Vec<BuiltinMcpObservation>, ()> {
    if app_version(app, &c.bundle_id, &c.product_name, budget)? != c.version { return Err(()); }
    let asar = relative(app, "Contents/Resources/app.asar")?;
    // Verify complete source members, including the imported constructor and
    // literal server-key bindings. Strings or filenames alone prove nothing.
    guard_asar(&asar, &c.guards, budget)?;
    let manifest = format!("{}!package.json", collapse_home(&asar));
    c.declarations.iter().map(|declaration| {
        if !c.guards.iter().any(|g| g.path == declaration.member) { return Err(()); }
        let source = format!("{}!{}#{}", collapse_home(&asar), declaration.member, declaration.symbol);
        let fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(
            &c.agent_id, &c.version, &c.bundle_id, &declaration.server_id, &source,
            c.guards.iter().map(|g| (&g.path, g.bytes, &g.sha256)).collect::<Vec<_>>(),
        )).map_err(|_| ())?));
        Ok(BuiltinMcpObservation {
            agent_id: c.agent_id.clone(), plugin_id: format!("desktop-session-{}", declaration.id),
            plugin_version: c.version.clone(), manifest_path: manifest.clone(),
            server_id: declaration.server_id.clone(), name: declaration.name.clone(),
            provider: c.product_name.clone(), source_path: source, app_version: c.version.clone(),
            docs_url: c.docs_url.clone(), source_fingerprint: fingerprint,
        })
    }).collect()
}

pub(crate) fn observe(agents: &BTreeMap<String, AgentDefinition>) -> BuiltinMcpScan {
    let c = contract(); let mut scan = BuiltinMcpScan::default();
    if !agents.get(&c.agent_id).is_some_and(|a| a.enabled) { return scan; }
    // Match the host's system-before-user installation probes. An invalid
    // selected bundle never falls through to another app's declarations.
    for app in c.app_paths.iter().map(|p| app_path(p)) {
        match fs::symlink_metadata(&app) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Ok(metadata) if metadata.file_type().is_dir() => match observe_app(c, &app, &mut ScanBudget::default()) {
                Ok(items) => scan.items = items,
                Err(()) => scan.unavailable = true,
            },
            _ => scan.unavailable = true,
        }
        break;
    }
    scan
}

pub(crate) fn watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    let c = contract();
    if !agents.get(&c.agent_id).is_some_and(|a| a.enabled) { return Vec::new(); }
    c.app_paths.iter().map(|p| app_path(p)).flat_map(|app| [
        app.clone(), app.join("Contents/Info.plist"), app.join("Contents/Resources/app.asar"),
    ]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    #[test]
    fn native_constructors_require_exact_members_and_identity_without_exporting_code() {
        let home = TestHome::new("chatgpt-session-mcps");
        let app = home.home.join("ChatGPT.app"); let contents = app.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        let write_info = |bundle: &str| {
            fs::write(contents.join("Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{bundle}</string><key>CFBundleName</key><string>ChatGPT</string><key>CFBundleShortVersionString</key><string>fixture</string></dict></plist>"#)).unwrap();
        };
        write_info("com.openai.codex");
        let code = b"PRIVATE_SENTINEL"; let package = b"{}";
        let header = serde_json::to_vec(&serde_json::json!({"files":{
            "main.js":{"offset":"0","size":code.len()},
            "package.json":{"offset":code.len().to_string(),"size":package.len()}
        }})).unwrap();
        let pickle = (header.len()+11)&!3; let mut raw = Vec::new();
        for word in [4, pickle as u32, (pickle-4) as u32, header.len() as u32] { raw.extend(word.to_le_bytes()); }
        raw.extend(header); raw.resize(pickle+8,0); raw.extend(code); raw.extend(package);
        let asar = contents.join("Resources/app.asar"); fs::write(&asar,&raw).unwrap();
        let mut c = Contract {
            agent_id:"codex-desktop".into(),version:"fixture".into(),bundle_id:"com.openai.codex".into(),product_name:"ChatGPT".into(),
            app_paths:Vec::new(),docs_url:"https://developers.openai.com/codex".into(),
            guards:vec![Guard{path:"main.js".into(),bytes:code.len() as u64,sha256:hex::encode(Sha256::digest(code))}],
            declarations:vec![Declaration{id:"fixture".into(),server_id:"fixture_server".into(),name:"Fixture".into(),member:"main.js".into(),symbol:"constructor".into()}],
        };
        let rows = observe_app(&c,&app,&mut ScanBudget::default()).unwrap();
        assert_eq!(rows.len(),1); assert_eq!(rows[0].server_id,"fixture_server");
        let json = serde_json::to_string(&rows).unwrap(); assert!(!json.contains("PRIVATE_SENTINEL"));
        let value = serde_json::to_value(&rows[0]).unwrap();
        for absent in ["asset","command","env","enabled","connected","available_actions"] { assert!(value.get(absent).is_none()); }
        write_info("foreign.bundle"); assert!(observe_app(&c,&app,&mut ScanBudget::default()).is_err()); write_info("com.openai.codex");
        *raw.last_mut().unwrap() = b'x'; fs::write(&asar,&raw).unwrap();
        c.guards.push(Guard{path:"package.json".into(),bytes:2,sha256:hex::encode(Sha256::digest(package))});
        assert!(observe_app(&c,&app,&mut ScanBudget::default()).is_err());
        assert!(observe(&BTreeMap::new()).items.is_empty());
        assert!(watch_paths(&BTreeMap::new()).is_empty());
    }
}
