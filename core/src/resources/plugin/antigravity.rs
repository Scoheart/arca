//! Antigravity App's default global plugins and materialized builtins. The
//! native language server, user config, bodies and lifecycle RPCs stay untouched.
use super::*;

fn marker(root: &Path, expected: &str, budget: &mut ScanBudget) -> Result<Option<PathBuf>, ()> {
    let candidates = directory_entries(root, budget)?.into_iter().filter(|entry|
        entry.path.file_name().and_then(|value| value.to_str()).is_some_and(|name| name.eq_ignore_ascii_case(expected)))
        .collect::<Vec<_>>();
    // The native reader is case insensitive. Ambiguous markers are not guessed.
    match candidates.as_slice() {
        [] => Ok(None),
        [entry] if entry.file => Ok(Some(entry.path.clone())),
        _ => Err(()),
    }
}

fn document(root: &Path, path: &Path, budget: &mut ScanBudget) -> Result<Arc<Value>, ()> {
    optional_document(root, path.strip_prefix(root).map_err(|_| ())?.to_str().ok_or(())?, DocumentFormat::Jsonc, budget)?.ok_or(())
}

fn add(root: &Path, path: &Path, kind: PluginComponentKind, components: &mut Vec<PluginComponent>) -> Result<(), ()> {
    if components.len() >= MAX_COMPONENTS || !regular_metadata_file(path)? { return Err(()); }
    let label = if kind == PluginComponentKind::Skill { path.parent().ok_or(())? } else { path };
    let label = metadata_name(&label.strip_prefix(root).map_err(|_| ())?.to_string_lossy()).ok_or(())?;
    components.push(PluginComponent { kind, name: label, source_path: collapse_home(path), native_id: None }); Ok(())
}

fn skills(root: &Path, folder: &Path, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    if let Some(path) = marker(folder, "SKILL.md", budget)? {
        add(root, &path, PluginComponentKind::Skill, components)?; return Ok(());
    }
    for directory in dirs(folder, budget)? {
        if let Some(path) = marker(&directory, "SKILL.md", budget)? { add(root, &path, PluginComponentKind::Skill, components)?; }
    }
    Ok(())
}

fn markdown(root: &Path, folder: &str, kind: PluginComponentKind, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    for entry in directory_entries(&root.join(folder), budget)? {
        if entry.file && entry.path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("md")) {
            add(root, &entry.path, kind.clone(), components)?;
        }
    }
    Ok(())
}

fn mcp(root: &Path, directory: &str, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let Some(value) = optional_document(root, "mcp_config.json", DocumentFormat::Jsonc, budget)? else { return Ok(()); };
    let Some(servers) = value.get("mcpServers") else { return if value.is_object() { Ok(()) } else { Err(()) }; };
    let servers = servers.as_object().filter(|value| value.len() <= MAX_COMPONENTS).ok_or(())?;
    let prefix = directory.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' { ch } else { '_' }).collect::<String>();
    for (name, declaration) in servers {
        // Only object declarations contribute metadata. Native semantic
        // validation, credential expansion and connection remain host-owned.
        if !declaration.is_object() { return Err(()); }
        if components.len() >= MAX_COMPONENTS { return Err(()); }
        metadata_name(name).ok_or(())?;
        let name = name.clone();
        let native_id = if prefix.is_empty() { name.clone() } else { format!("{prefix}_{name}") };
        components.push(PluginComponent { kind: PluginComponentKind::Mcp, name, native_id: Some(native_id), source_path: collapse_home(&root.join("mcp_config.json")) });
    }
    Ok(())
}

fn hooks(root: &Path, components: &mut Vec<PluginComponent>, budget: &mut ScanBudget) -> Result<(), ()> {
    let Some(value) = optional_document(root, "hooks.json", DocumentFormat::Jsonc, budget)? else { return Ok(()); };
    let hooks = value.as_object().filter(|value| value.len() <= MAX_COMPONENTS).ok_or(())?;
    for (name, hook) in hooks {
        let hook = hook.as_object().ok_or(())?;
        if !["PreToolUse", "PostToolUse", "PreInvocation", "PostInvocation", "Stop"].iter()
            .any(|event| hook.get(*event).is_some_and(|value| value.as_array().is_some_and(|items| !items.is_empty()))) { continue; }
        if components.len() >= MAX_COMPONENTS { return Err(()); }
        components.push(PluginComponent { kind: PluginComponentKind::Hook, name: metadata_name(name).ok_or(())?, source_path: collapse_home(&root.join("hooks.json")), native_id: None });
    }
    Ok(())
}

fn package(host: &Host, root: &Path, presence: PluginPresence, result: &mut PluginScan) -> Result<(), ()> {
    let Some(path) = marker(root, "plugin.json", &mut result.budget)? else { return Ok(()); };
    let directory = root.file_name().and_then(|value| value.to_str()).ok_or(())?;
    metadata_name(directory).ok_or(())?;
    let directory = directory.to_string();
    let mut plugin = NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:directory.clone(), name:directory.clone(), version:"unknown".into(), revision:"package-files".into(), presence, enabled:None,
        source_paths:vec![collapse_home(&path)], components:Vec::new(), docs_url:host.docs_url.clone(), source_fingerprint:String::new() };
    let observed = (|| {
        let value = document(root, &path, &mut result.budget)?;
        let manifest = value.as_object().ok_or(())?;
        let name = manifest.get("name").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or(&directory);
        plugin.name = manifest.get("displayName").and_then(Value::as_str).and_then(metadata_name)
            .or_else(|| metadata_name(name)).ok_or(())?;
        plugin.version = manifest.get("version").and_then(Value::as_str).and_then(metadata_name).unwrap_or_else(|| "unknown".into());
        // The stamp describes the installed package independently of the authored
        // manifest version; disagreement must not invent a second physical copy.
        if let Some(stamp) = optional_document(root, "installed_version.json", DocumentFormat::Jsonc, &mut result.budget)? {
            let installed = stamp.get("version").and_then(Value::as_str).and_then(metadata_name).ok_or(())?;
            plugin.revision = format!("installed-version:{installed}");
            plugin.source_paths.push(collapse_home(&root.join("installed_version.json")));
        }
        // This initial observer is scoped to the shipped legacy default-path
        // contract. A gated schema or explicit selectors cannot silently fall
        // through to that contract; retain package metadata and signal partial.
        if manifest.contains_key("$schema") || ["skills", "agents", "rules", "workflows", "mcpServers", "hooks"].iter().any(|field| manifest.contains_key(*field)) { return Err(()); }
        if skills(root, &root.join("skills"), &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
        for (folder, kind) in [("agents", PluginComponentKind::Agent), ("rules", PluginComponentKind::Rule), ("workflows", PluginComponentKind::Workflow)] {
            if markdown(root, folder, kind, &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
        }
        if mcp(root, &directory, &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
        if hooks(root, &mut plugin.components, &mut result.budget).is_err() { result.unavailable = true; }
        Ok::<(), ()>(())
    })();
    if observed.is_err() { result.unavailable = true; }
    plugin.source_paths.sort(); plugin.source_paths.dedup(); plugin.components.sort(); plugin.components.dedup();
    result.plugins.push(plugin); Ok(())
}

fn builtin(host: &Host, root: &Path, result: &mut PluginScan) -> Result<(), ()> {
    let mut components = Vec::new();
    skills(root, &root.join("skills"), &mut components, &mut result.budget)?;
    if !components.is_empty() {
        // A native customization group, not a fabricated plugin manifest or
        // package version. The source is the materialized builtin directory.
        result.plugins.push(NativePluginObservation { agent_id:host.agent_id.clone(), plugin_id:"@builtin/skills".into(), name:"Antigravity built-in Skills".into(), version:"unknown".into(), revision:"materialized-builtin-files".into(), presence:PluginPresence::Bundled, enabled:None,
            source_paths:vec![collapse_home(&root.join("skills"))], components, docs_url:host.docs_url.clone(), source_fingerprint:String::new() });
    }
    Ok(())
}

pub(super) fn observe(host: &Host, result: &mut PluginScan) {
    for source in &host.roots {
        let root = expand_tilde(&source.path);
        let observed = (|| {
            checked_root(&root)?;
            if source.layout == "antigravity_builtin" {
                if builtin(host, &root, result).is_err() { result.unavailable = true; }
            }
            else if source.layout == "antigravity_global" {
                // Workspace-relative paths, inherited filter semantics and
                // arbitrary registries are not guessed by a global observer.
                match regular_metadata_file(&root.join("plugins.json")) {
                    Ok(false) => {}, _ => result.unavailable = true,
                }
            } else { return Err(()); }
            for entry in directory_entries(&root.join("plugins"), &mut result.budget)? {
                if entry.directory {
                    if package(host, &entry.path, source.presence.clone(), result).is_err() { result.unavailable = true; }
                } else if !entry.file { result.unavailable = true; }
                if result.budget.references > MAX_REFERENCES || result.budget.bytes >= MAX_READ_BYTES { break; }
            }
            Ok::<(), ()>(())
        })();
        if observed.is_err() { result.unavailable = true; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn host() -> &'static Host { hosts().iter().find(|host| host.agent_id == "antigravity").unwrap() }
    fn write(root: &Path, input: &str, content: &str) {
        let path = root.join(input); fs::create_dir_all(path.parent().unwrap()).unwrap(); fs::write(path, content).unwrap();
    }
    fn scan() -> PluginScan { let mut result = PluginScan::default(); observe(host(), &mut result); normalize(&mut result); result }

    #[test]
    fn directory_identity_and_install_stamp_survive_manifest_disagreement_without_secrets() {
        let home = TestHome::new("antigravity-identities"); let root = home.home.join(".gemini/config/plugins/-123");
        write(&root, "plugin.json", r#"{/* native JSONC */"name":"authored-name","displayName":"Visible Name","version":"0.2",}"#);
        write(&root, "installed_version.json", r#"{"version":"1.0"}"#);
        write(&root, "mcp_config.json", r#"{"mcpServers":{"api":{"command":"private-command","env":{"KEY":"private-token"}}}}"#);
        let result = scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(), 1);
        let plugin = &result.plugins[0]; assert_eq!(plugin.plugin_id,"-123"); assert_eq!(plugin.name,"Visible Name");
        assert_eq!(plugin.version,"0.2"); assert_eq!(plugin.revision,"installed-version:1.0"); assert_eq!(plugin.source_paths.len(),2);
        assert_eq!(result.mcps[0].server_id,"-123_api"); assert!(plugin.enabled.is_none());
        let output = serde_json::to_string(&plugin).unwrap(); for secret in ["private-command","private-token"] { assert!(!output.contains(secret)); }
        assert!(!home.home.join(".gemini/config/config.json").exists());
    }

    #[test]
    fn native_mcp_namespace_preserves_case_and_replaces_each_non_ascii_rune() {
        let home = TestHome::new("antigravity-namespace"); let root = home.home.join(".gemini/config/plugins/A.中文-_");
        write(&root,"plugin.json","{}"); write(&root,"mcp_config.json",r#"{"mcpServers":{"source-name":{"serverUrl":"https://private.invalid"}}}"#);
        let result=scan(); assert!(!result.unavailable); assert_eq!(result.mcps[0].server_id,"A___-__source-name");
        assert_eq!(result.plugins[0].name,"A.中文-_"); assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private.invalid"));
    }

    #[test]
    fn root_skill_marker_and_immediate_children_exclude_examples_and_bodies() {
        let home=TestHome::new("antigravity-skills"); let root=home.home.join(".gemini/config/plugins/example"); write(&root,"PLUGIN.JSON","{}");
        for path in ["skills/SKILL.md","skills/child/SKILL.md","examples/demo/SKILL.md","agents/worker.md","agents/nested/example.md","rules/AGENTS.md"] { write(&root,path,"private component body"); }
        let result=scan(); assert!(!result.unavailable); assert_eq!(result.plugins[0].components.len(),3);
        assert_eq!(result.plugins[0].components.iter().filter(|c| c.kind==PluginComponentKind::Skill).count(),1);
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private component body"));
    }

    #[test]
    fn materialized_app_builtins_and_global_packages_exclude_other_products() {
        let home=TestHome::new("antigravity-builtins");
        write(&home.home,".gemini/antigravity/builtin/skills/guide/SKILL.md","private builtin body");
        write(&home.home,".gemini/antigravity/builtin/plugins/shipped/plugin.json","{}");
        for path in [".gemini/antigravity-ide/builtin/plugins/ide/plugin.json",".gemini/antigravity-cli/builtin/plugins/cli/plugin.json",".gemini/extensions/gemini/gemini-extension.json"] { write(&home.home,path,r#"{"name":"foreign"}"#); }
        let result=scan(); assert!(!result.unavailable); assert_eq!(result.plugins.len(),2); assert!(result.plugins.iter().all(|p| p.presence==PluginPresence::Bundled && p.enabled.is_none()));
        assert!(result.plugins.iter().any(|p| p.plugin_id=="@builtin/skills" && p.version=="unknown"));
    }

    #[test]
    fn hook_metadata_keeps_named_components_without_commands_or_enablement_claims() {
        let home=TestHome::new("antigravity-hooks"); let root=home.home.join(".gemini/config/plugins/example"); write(&root,"plugin.json","{}");
        write(&root,"hooks.json",r#"{"guard":{"enabled":false,"PreToolUse":[{"hooks":[{"command":"private-script"}]}]},"empty":{"Stop":[]}}"#);
        let result=scan(); assert!(!result.unavailable); assert_eq!(result.plugins[0].components.len(),1); assert_eq!(result.plugins[0].components[0].name,"guard");
        assert!(!serde_json::to_string(&result.plugins).unwrap().contains("private-script"));
    }

    #[test]
    fn malformed_and_unsupported_selectors_retain_metadata_and_signal_incomplete_observation() {
        let home=TestHome::new("antigravity-closed"); let root=home.home.join(".gemini/config/plugins/example");
        write(&root,"plugin.json",r#"{"name":"example","$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"}"#);
        write(&root,"skills/hidden/SKILL.md","body"); let result=scan(); assert!(result.unavailable); assert_eq!(result.plugins.len(),1); assert!(result.plugins[0].components.is_empty());
        write(&root,"plugin.json","malformed"); let result=scan(); assert!(result.unavailable); assert_eq!(result.plugins[0].name,"example");
        write(&root,"plugin.json","{}"); write(&home.home,".gemini/config/plugins.json",r#"{"inherits":[{"path":"/unreviewed"}]}"#);
        assert!(scan().unavailable);
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_markers_components_and_ambiguous_case_markers_fail_closed() {
        let home=TestHome::new("antigravity-confined"); let root=home.home.join(".gemini/config/plugins/example"); write(&root,"plugin.json","{}");
        write(&home.home,"outside.json",r#"{"mcpServers":{"escaped":{"command":"private"}}}"#);
        std::os::unix::fs::symlink(home.home.join("outside.json"),root.join("mcp_config.json")).unwrap();
        let result=scan(); assert!(result.unavailable); assert!(result.mcps.is_empty());
        fs::remove_file(root.join("mcp_config.json")).unwrap(); write(&root,"PLUGIN.JSON","{}");
        // Case-insensitive filesystems cannot materialize this ambiguity.
        if fs::read_dir(&root).unwrap().count() == 2 { assert!(scan().unavailable); }
    }
}
