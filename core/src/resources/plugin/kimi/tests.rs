use super::*;
use crate::testenv::TestHome;
use serde_json::json;

fn host() -> Host {
    serde_json::from_value(json!({
        "agent_id":"kimi-code", "adapter":"kimi", "docs_url":"https://www.npmjs.com/package/@moonshot-ai/kimi-code/v/2.4.0",
        "manifest_paths":["kimi.plugin.json",".kimi-plugin/plugin.json"],
        "default_mcp_files":[], "mcp_id_prefix":"plugin-",
        "roots":[{"path":"~/.kimi-code/plugins","layout":"kimi_registry","presence":"installed"}]
    })).unwrap()
}

fn package(home: &TestHome, name: &str, manifest: Value) -> PathBuf {
    let root=home.home.join("packages").join(name);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("kimi.plugin.json"),serde_json::to_vec(&manifest).unwrap()).unwrap();
    root
}

fn scan(home: &TestHome, entries: Value, verified: bool) -> PluginScan {
    let root=home.home.join(".kimi-code/plugins");fs::create_dir_all(&root).unwrap();
    fs::write(root.join("installed.json"),serde_json::to_vec(&json!({"version":7,"plugins":entries})).unwrap()).unwrap();
    let mut result=PluginScan::default();
    packages::observe_registry(&host(),&root,verified,&mut result);
    normalize(&mut result);result
}

// Registry roots, rather than folder enumeration or manifest name, own identity.
#[test]
fn registered_root_and_case_preserve_native_mcp_identity_without_secrets() {
    let home=TestHome::new("kimi-registry");
    let root=package(&home,"registered",json!({"name":"different","interface":{"displayName":"A package"},"version":"1.2.3",
        "mcpServers":{" tool ":{"command":"node","args":["PRIVATE_ARG"],"env":{"TOKEN":"PRIVATE_ENV"}},
        "web":{"transport":"http","url":"https://user:PRIVATE_URL@example.invalid/mcp","headers":{"Authorization":"PRIVATE_HEADER"},"auth":"oauth"}},
        "systemPrompt":"PRIVATE_PROMPT","skillInstructions":"PRIVATE_INSTRUCTIONS"}));
    package(&home,"unregistered",json!({"name":"unregistered","mcpServers":{"bad":{"command":"bad"}}}));
    let result=scan(&home,json!([{"id":"CasePlug","root":root,"enabled":false}]),true);
    assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].plugin_id,"CasePlug");
    assert_eq!(result.plugins[0].name,"A package");assert_eq!(result.plugins[0].enabled,None);
    assert_eq!(result.mcps.iter().map(|m|m.server_id.as_str()).collect::<Vec<_>>(),vec!["plugin-CasePlug:tool","plugin-CasePlug:web"]);
    let visible=serde_json::to_string(&result.plugins).unwrap();
    for secret in ["PRIVATE_ARG","PRIVATE_ENV","PRIVATE_URL","PRIVATE_HEADER","PRIVATE_PROMPT","PRIVATE_INSTRUCTIONS"] {assert!(!visible.contains(secret));}
}

// A broken winning manifest must not expose an alternate manifest's resources.
#[test]
fn broken_root_manifest_does_not_fall_back_and_missing_roots_stay_declared() {
    let home=TestHome::new("kimi-precedence");let root=package(&home,"broken",json!({"name":"broken"}));
    fs::create_dir_all(root.join(".kimi-plugin")).unwrap();
    fs::write(root.join(".kimi-plugin/plugin.json"),r#"{"name":"alternate","mcpServers":{"bad":{"command":"bad"}}}"#).unwrap();
    fs::write(root.join("kimi.plugin.json"),"{").unwrap();
    let result=scan(&home,json!([{"id":"broken","root":root},{"id":"missing","root":home.home.join("missing")}]),true);
    assert!(result.unavailable);assert!(result.mcps.is_empty());assert_eq!(result.plugins.len(),2);
    assert!(result.plugins.iter().all(|p|p.presence==PluginPresence::Declared));
}

// Omitted Skill roots mean only root SKILL.md; null is a native manifest error.
#[test]
fn skill_fallback_is_root_only_and_explicit_empty_does_not_fall_back() {
    let home=TestHome::new("kimi-skills");let root=package(&home,"skills",json!({"name":"skills"}));
    fs::write(root.join("SKILL.md"),"PRIVATE_BODY").unwrap();
    fs::create_dir_all(root.join("skills/hidden")).unwrap();fs::write(root.join("skills/hidden/SKILL.md"),"PRIVATE_BODY").unwrap();
    let result=scan(&home,json!([{"id":"skills","root":root}]),true);
    let skills=result.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::Skill).collect::<Vec<_>>();
    assert_eq!(skills.len(),1);assert!(skills[0].source_path.ends_with("/SKILL.md"));
    fs::write(root.join("kimi.plugin.json"),r#"{"name":"skills","skills":[]}"#).unwrap();
    let result=scan(&home,json!([{"id":"skills","root":root}]),true);
    assert!(!result.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
    fs::write(root.join("kimi.plugin.json"),r#"{"name":"skills","skills":null,"mcpServers":{"tool":{"command":"tool"}}}"#).unwrap();
    let result=scan(&home,json!([{"id":"skills","root":root}]),true);
    assert!(result.unavailable);assert!(result.mcps.is_empty());
}

// Do not turn conditional nested sub-Skills or authored Commands into Skills.
#[test]
fn explicit_skill_roots_and_authored_components_keep_their_own_types() {
    let home=TestHome::new("kimi-components");let root=package(&home,"types",json!({"name":"types","skills":"./catalog","commands":"./commands",
        "hooks":[{"event":"SessionStart","command":"PRIVATE_HOOK"}],"sessionStart":{"skill":"PRIVATE_SELECTION"}}));
    for name in ["catalog/first","catalog/first/nested","catalog/container/second","commands/nested","agents"] {fs::create_dir_all(root.join(name)).unwrap();}
    for name in ["catalog/first/SKILL.md","catalog/first/nested/SKILL.md","catalog/container/second/SKILL.md","commands/nested/task.md","agents/helper.md"] {fs::write(root.join(name),"PRIVATE_BODY").unwrap();}
    let result=scan(&home,json!([{"id":"types","root":root}]),true);let c=&result.plugins[0].components;
    assert_eq!(c.iter().filter(|c|c.kind==PluginComponentKind::Skill).count(),2);
    assert!(c.iter().any(|c|c.kind==PluginComponentKind::PluginReference&&c.source_path.ends_with("first/nested/SKILL.md")));
    assert!(c.iter().any(|c|c.kind==PluginComponentKind::Command&&c.name=="nested/task"));
    assert!(c.iter().any(|c|c.kind==PluginComponentKind::Agent&&c.name=="helper"));
    assert!(c.iter().any(|c|c.kind==PluginComponentKind::Hook&&c.name=="SessionStart"));
    let visible=serde_json::to_string(c).unwrap();for secret in ["PRIVATE_BODY","PRIVATE_HOOK","PRIVATE_SELECTION"] {assert!(!visible.contains(secret));}
}

// Warnings on one independent component must not erase valid MCP declarations.
#[test]
fn invalid_commands_and_server_fields_retain_independent_valid_servers() {
    let home=TestHome::new("kimi-mcp-shapes");let root=package(&home,"mcp",json!({"name":"mcp","commands":42,"mcpServers":{
        "valid":{"command":"./bin/tool","cwd":"./work","args":[],"enabled":false},
        "sse":{"transport":"sse","url":"https://example.invalid/events"},
        "nullable":{"command":"node","env":null},"outside":{"command":"/bin/sh"},
        "timeout":{"command":"node","toolTimeoutMs":0},"wrong":{"transport":"streamable-http","url":"https://example.invalid"}}}));
    let result=scan(&home,json!([{"id":"mcp","root":root}]),true);
    assert!(result.unavailable);
    assert_eq!(result.mcps.iter().map(|m|m.server_id.as_str()).collect::<Vec<_>>(),vec!["plugin-mcp:sse","plugin-mcp:valid"]);
    assert_eq!(result.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::McpReference).count(),4);
}

// Native last-wins paths are not knowable without materialization/body parsing.
#[test]
fn duplicate_registry_ids_and_trimmed_server_names_do_not_manufacture_winners() {
    let home=TestHome::new("kimi-collisions");
    let first=package(&home,"first",json!({"name":"first","mcpServers":{"a":{"command":"a"}}}));
    let second=package(&home,"second",json!({"name":"second","mcpServers":{"b":{"command":"b"}}}));
    let result=scan(&home,json!([{"id":"same","root":first},{"id":"same","root":second}]),true);
    assert!(result.unavailable);assert!(result.mcps.is_empty());assert_eq!(result.plugins.len(),2);
    let root=package(&home,"keys",json!({"name":"keys","mcpServers":{" key":{"command":"a"},"key ":{"command":"b"},"other":{"command":"c"}}}));
    let result=scan(&home,json!([{"id":"keys","root":root}]),true);
    assert_eq!(result.mcps.len(),1);assert_eq!(result.mcps[0].server_id,"plugin-keys:other");assert!(result.unavailable);
}

#[test]
fn unverified_runtime_retains_registry_references_without_project_home_fallback() {
    let home=TestHome::new("kimi-unknown");let root=package(&home,"pending",json!({"name":"pending","mcpServers":{"a":{"command":"a"}}}));
    let result=scan(&home,json!([{"id":"pending","root":root}]),false);
    assert!(result.unavailable);assert!(result.mcps.is_empty());assert_eq!(result.plugins[0].presence,PluginPresence::Declared);
    for value in ["","relative","~/.kimi-code"] {assert_eq!(global_base(Some(value)),None);}
    assert_eq!(global_base(Some("/absolute/kimi")),Some(PathBuf::from("/absolute/kimi")));
}

#[cfg(unix)]
#[test]
fn symlinked_metadata_and_escaping_directories_cannot_become_native_resources() {
    use std::os::unix::fs::symlink;
    let home=TestHome::new("kimi-paths");let outside=package(&home,"outside",json!({"name":"outside","mcpServers":{"x":{"command":"x"}}}));
    let root=package(&home,"link",json!({"name":"link"}));fs::remove_file(root.join("kimi.plugin.json")).unwrap();
    symlink(outside.join("kimi.plugin.json"),root.join("kimi.plugin.json")).unwrap();
    let result=scan(&home,json!([{"id":"link","root":root}]),true);assert!(result.unavailable);assert!(result.mcps.is_empty());
    fs::remove_file(root.join("kimi.plugin.json")).unwrap();
    fs::write(root.join("kimi.plugin.json"),r#"{"name":"link","agents":"./../outside","mcpServers":{"x":{"command":"x"}}}"#).unwrap();
    let result=scan(&home,json!([{"id":"link","root":root}]),true);assert!(result.unavailable);assert!(result.mcps.is_empty());
}

// A version label must not cause the target Node entry or a fallback to run.
#[cfg(unix)]
#[test]
fn selected_npm_identity_is_passive_and_changed_or_wrapped_entries_block_fallback() {
    use std::os::unix::fs::{symlink,PermissionsExt};
    let home=TestHome::new("kimi-passive");
    let package=home.home.join("runtime/package");fs::create_dir_all(package.join("dist")).unwrap();
    let main=package.join("dist/main.mjs");fs::write(&main,"#!/usr/bin/env node\nfixture\n").unwrap();
    fs::set_permissions(&main,fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(package.join("package.json"),r#"{"name":"fixture","version":"1.0.0","bin":{"kimi":"dist/main.mjs"},"type":"module"}"#).unwrap();
    fs::write(home.home.join("bin/node"),"#!/bin/sh\nprintf executed > \"$HOME/executed-marker\"\n").unwrap();
    fs::set_permissions(home.home.join("bin/node"),fs::Permissions::from_mode(0o755)).unwrap();
    let selected=home.home.join("bin/kimi");symlink(&main,&selected).unwrap();
    let c=Contract {command:"kimi".into(),package_name:"fixture".into(),version:"1.0.0".into(),entry:"dist/main.mjs".into(),builtin_skills:vec![],members:vec![
        Member {path:"dist/main.mjs".into(),bytes:28,sha256:"d924c8168f2560da22a281781a6341c2fb7e9f3137b13eeb86daeb9d4278bc41".into()},
        Member {path:"package.json".into(),bytes:83,sha256:"887e056c8fbbb78e936dc6b9c8f74168f18cddab5abb31a9e7193bd841d52e9d".into()}]};
    let runtime=verified_runtime(&c,&selected,&mut ScanBudget::default()).unwrap();assert_eq!(runtime.root,package);
    assert_eq!(crate::application::agent_launch::runtime_version("kimi-code").unwrap(),None);
    assert!(!home.home.join("executed-marker").exists());
    fs::write(&main,"#!/usr/bin/env node\nchanged\n").unwrap();assert!(verified_runtime(&c,&selected,&mut ScanBudget::default()).is_err());
    assert_eq!(passive_runtime_version("kimi-code",&selected),Some(None));assert!(!home.home.join("executed-marker").exists());
}

#[test]
fn embedded_catalog_has_real_storage_paths_and_never_creates_builtin_mcps() {
    let home=TestHome::new("kimi-bundled");let runtime=Runtime {root:home.home.join("package"),entry:home.home.join("package/dist/main.mjs")};
    let mut result=PluginScan::default();bundled(&host(),contract(),&runtime,&mut result);normalize(&mut result);
    assert_eq!(result.plugins.len(),1);assert_eq!(result.plugins[0].presence,PluginPresence::Bundled);assert_eq!(result.plugins[0].enabled,None);
    assert_eq!(result.plugins[0].components.iter().map(|c|c.name.as_str()).collect::<Vec<_>>(),vec!["mcp-config","import-from-cc-codex","update-config","custom-theme","write-goal","check-kimi-code-docs","sub-skill","sub-skill.review","sub-skill.consolidate"]);
    assert!(result.plugins[0].components.iter().all(|c|c.kind==PluginComponentKind::Skill&&c.source_path.ends_with("/dist/main.mjs")&&c.native_id.is_none()));
    assert!(result.mcps.is_empty());
}
