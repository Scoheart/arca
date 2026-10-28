//! Read-only observations of audited, host-owned MCP extension providers.
//! This module never produces McpConfig/AssetRef values or mutates host files.

use crate::domain::mcp::BuiltinMcpObservation;
use crate::domain::types::AgentDefinition;
use super::scanner::{collapse_home, expand_tilde};
use crate::resources::plugin::text;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Host {
    agent_id: String,
    app_roots: Vec<String>,
    product_name: String,
    providers: Vec<Provider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Provider {
    server_id: String,
    name: String,
    extension: String,
    publisher: String,
    docs_url: String,
}

fn hosts() -> &'static [Host] {
    static HOSTS: OnceLock<Vec<Host>> = OnceLock::new();
    HOSTS.get_or_init(|| serde_json::from_str(include_str!("../../../../data/builtin-mcps.json"))
        .expect("audited built-in MCP descriptors must be valid"))
}

#[derive(Default)]
pub(crate) struct BuiltinMcpScan {
    pub items: Vec<BuiltinMcpObservation>,
    pub unavailable: bool,
}

fn app_root(path: &str) -> PathBuf {
    if path.starts_with("~/") { expand_tilde(path) }
    else { crate::paths::system_probe_path(path) }
}


fn json(path: &Path) -> Result<Value, ()> {
    serde_json::from_str(&text(path, 1_048_576)?).map_err(|_| ())
}

fn entrypoint(extension: &Path, manifest: &Value) -> Result<PathBuf, ()> {
    let main = manifest.get("main").and_then(Value::as_str).ok_or(())?;
    let relative = Path::new(main);
    if main.is_empty() || relative.is_absolute()
        || relative.components().any(|part| matches!(part, Component::ParentDir | Component::Prefix(_))) {
        return Err(());
    }
    let mut entry = extension.join(relative);
    if entry.extension().is_none() { entry.set_extension("js"); }
    crate::resources::plugin::checked_root(&entry)?;
    if !entry.starts_with(extension) { return Err(()); }
    Ok(entry)
}

fn observe_provider(host: &Host, provider: &Provider, app: &Path, product: &Value)
    -> Result<Option<BuiltinMcpObservation>, ()> {
    let extension = app.join("Contents/Resources/app/extensions").join(&provider.extension);
    let manifest_path = extension.join("package.json");
    match fs::metadata(&manifest_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
        Ok(_) => {}
    }
    let manifest = json(&manifest_path)?;
    if manifest.get("name").and_then(Value::as_str) != Some(provider.extension.as_str())
        || manifest.get("publisher").and_then(Value::as_str) != Some(provider.publisher.as_str()) {
        return Err(());
    }
    let entry = entrypoint(&extension, &manifest)?;
    let source = text(&entry, 16 * 1_048_576)?;
    // The manifest alone is insufficient: a formerly native extension might
    // remain installed after its provider implementation changes.
    if !source.contains("registerMcpProvider") || !source.contains(&provider.server_id) {
        return Err(());
    }
    let app_version = product.get("version").and_then(Value::as_str).ok_or(())?;
    let material = serde_json::to_vec(&(&host.agent_id, &provider.server_id, app_version,
        &manifest, hex::encode(Sha256::digest(source.as_bytes())))).map_err(|_| ())?;
    Ok(Some(BuiltinMcpObservation {
        plugin_id: provider.extension.clone(),
        plugin_version: manifest.get("version").and_then(Value::as_str).unwrap_or("unknown").into(),
        manifest_path: collapse_home(&manifest_path.to_string_lossy()),
        agent_id: host.agent_id.clone(), server_id: provider.server_id.clone(),
        name: provider.name.clone(), provider: host.product_name.clone(),
        source_path: collapse_home(&entry.to_string_lossy()), app_version: app_version.into(),
        docs_url: provider.docs_url.clone(), source_fingerprint: hex::encode(Sha256::digest(material)),
    }))
}

pub(crate) fn observe(agents: &BTreeMap<String, AgentDefinition>) -> BuiltinMcpScan {
    let mut scan = crate::resources::plugin::codex_desktop::observe(agents);
    let goose = crate::resources::plugin::goose_desktop::builtin_observe(agents);
    scan.items.extend(goose.items); scan.unavailable |= goose.unavailable;
    for host in hosts() {
        if !agents.get(&host.agent_id).is_some_and(|agent| agent.enabled) { continue; }
        // Respect the same system-before-user app search order as the host's
        // audited installation probes. Never merge two installed versions.
        let mut app = None;
        for path in host.app_roots.iter().map(|path| app_root(path)) {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => { app = Some(path); break; }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => { scan.unavailable = true; break; }
            }
        }
        let Some(app) = app else { continue; };
        let product = match json(&app.join("Contents/Resources/app/product.json")) {
            Ok(product) if product.get("nameShort").and_then(Value::as_str) == Some(host.product_name.as_str()) => product,
            _ => { scan.unavailable = true; continue; }
        };
        for provider in &host.providers {
            match observe_provider(host, provider, &app, &product) {
                Ok(Some(item)) => scan.items.push(item),
                Ok(None) => {}
                Err(()) => scan.unavailable = true,
            }
        }
    }
    scan.items.sort_by(|a, b| (&a.agent_id, &a.server_id).cmp(&(&b.agent_id, &b.server_id)));
    scan
}

pub(crate) fn watch_paths(agents: &BTreeMap<String, AgentDefinition>) -> Vec<PathBuf> {
    let mut paths = crate::resources::plugin::codex_desktop::watch_paths(agents);
    paths.extend(crate::resources::plugin::goose_desktop::builtin_watch_paths(agents));
    for host in hosts() {
        if !agents.get(&host.agent_id).is_some_and(|agent| agent.enabled) { continue; }
        for root in &host.app_roots {
            let root = app_root(root);
            // Installation/update commonly renames the complete .app. A
            // nested file-only watch can miss that ancestor event once the
            // new bundle exists, so retain the bundle path itself as input.
            paths.push(root.clone());
            let app = root.join("Contents/Resources/app");
            paths.push(app.join("product.json"));
            for provider in &host.providers {
                let extension = app.join("extensions").join(&provider.extension);
                paths.push(extension.join("package.json"));
                if let Ok(manifest) = json(&extension.join("package.json")) {
                    if let Ok(entry) = entrypoint(&extension, &manifest) { paths.push(entry); }
                }
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn agents() -> BTreeMap<String, AgentDefinition> {
        BTreeMap::from([("cursor".into(), AgentDefinition { enabled: true, ..Default::default() })])
    }

    fn install(home: &TestHome, provider: &Provider, source: &str) -> PathBuf {
        let app = home.home.join("Applications/Cursor.app/Contents/Resources/app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("product.json"), r#"{"nameShort":"Cursor","version":"fixture"}"#).unwrap();
        let extension = app.join("extensions").join(&provider.extension);
        fs::create_dir_all(extension.join("dist")).unwrap();
        fs::write(extension.join("package.json"), serde_json::to_vec(&serde_json::json!({
            "name":provider.extension,"publisher":provider.publisher,"main":"./dist/extension.js"
        })).unwrap()).unwrap();
        fs::write(extension.join("dist/extension.js"), source).unwrap();
        extension
    }

    #[test]
    fn native_providers_require_installed_identity_and_registration() {
        let home = TestHome::new("builtin-mcp");
        assert!(observe(&agents()).items.is_empty());
        for provider in &hosts()[0].providers {
            install(&home, provider, &format!("const id={:?}; cursor.registerMcpProvider({{id}})", provider.server_id));
        }
        let scan = observe(&agents());
        assert!(!scan.unavailable);
        assert_eq!(scan.items.len(), 2);
        assert!(scan.items.iter().all(|item| item.agent_id == "cursor" && item.app_version == "fixture"));
        assert_eq!(scan.items, observe(&agents()).items);
        let serialized = serde_json::to_value(&scan.items[0]).unwrap();
        for absent in ["asset", "command", "url", "enabled", "connected", "available_actions"] {
            assert!(serialized.get(absent).is_none());
        }
        assert!(observe(&BTreeMap::new()).items.is_empty());
    }

    #[test]
    fn changed_provider_does_not_hide_other_native_providers() {
        let home = TestHome::new("builtin-mcp-errors");
        let providers = &hosts()[0].providers;
        install(&home, &providers[0], "const id='cursor-ide-browser';cursor.registerMcpProvider({id})");
        let extension = install(&home, &providers[1], "not a native provider anymore");
        let scan = observe(&agents());
        assert!(scan.unavailable);
        assert_eq!(scan.items.len(), 1);
        fs::write(extension.join("package.json"), r#"{"name":"cursor-computer-use","publisher":"cursor","main":"../../outside.js"}"#).unwrap();
        assert!(entrypoint(&extension, &json(&extension.join("package.json")).unwrap()).is_err());
    }

    #[test]
    fn installation_watch_includes_the_bundle_and_special_files_are_rejected() {
        let home = TestHome::new("builtin-mcp-install");
        let app = home.home.join("Applications/Cursor.app");
        assert!(watch_paths(&agents()).contains(&app));
        install(&home, &hosts()[0].providers[0], "cursor.registerMcpProvider('cursor-ide-browser')");
        assert!(watch_paths(&agents()).contains(&app));
        assert!(text(&app, 100).is_err());
        #[cfg(unix)]
        {
            let link = home.home.join("manifest-link");
            std::os::unix::fs::symlink(app.join("Contents/Resources/app/product.json"), &link).unwrap();
            assert!(text(&link, 1000).is_err());
        }
    }
}

