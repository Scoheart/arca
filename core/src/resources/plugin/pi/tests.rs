use super::*;
use crate::testenv::TestHome;
use serde_json::json;

const CODE:&[u8]=b"#!/usr/bin/env node\nrequire('fs').writeFileSync(process.env.HOME+'/pi-executed-marker','executed');\n";
const PACKAGE:&[u8]=br#"{"name":"@earendil-works/pi-coding-agent","version":"1.2.0","bin":{"pi":"dist/bundle/cli.js"},"piConfig":{"configDir":".pi"}}"#;
fn write(p:&Path,b:&[u8]) {fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,b).unwrap();}
fn host() -> Host {serde_json::from_value(json!({"agent_id":"pi","adapter":"pi","docs_url":"https://pi.dev/docs/latest/packages","manifest_paths":["package.json"],"default_mcp_files":[],"mcp_id_prefix":"","roots":[]})).unwrap()}
fn paths(h:&TestHome) -> HomePaths {paths_from(&h.home,None).unwrap()}
fn settings(p:&HomePaths,v:Value) {write(&p.agent.join("settings.json"),&serde_json::to_vec(&v).unwrap());}
fn local(p:&Path,manifest:Value) {write(&p.join("package.json"),&serde_json::to_vec(&manifest).unwrap());}
fn scan(p:&HomePaths) -> PluginScan {let mut r=PluginScan::default();scan_sources(&host(),p,&mut r);normalize(&mut r);r}
fn guard(p:&str,b:&[u8]) -> Value {json!({"path":p,"bytes":b.len(),"sha256":hex::encode(Sha256::digest(b))})}
fn fixture_contract() -> Contract {serde_json::from_value(json!({"agent_id":"pi","command":"pi","version":"1.2.0","package_name":"@earendil-works/pi-coding-agent","entry_path":"dist/bundle/cli.js","config_dir":".pi",
    "guards":[guard("package.json",PACKAGE),guard("dist/bundle/cli.js",CODE),guard("dist/bundle/cli-runtime.js",b"bootstrap"),guard("dist/bundle/chunks/chunk-TJXZSVTD.js",b"kernel"),guard("dist/bundle/chunks/chunk-WJUNYZC5.js",b"config")],
    "builtin_extensions":[{"name":"llama.cpp","replaceable":false},{"name":"codemode","replaceable":true},{"name":"tool-search","replaceable":true},{"name":"mcp","replaceable":true}]
})).unwrap()}
fn runtime(h:&TestHome) -> (Contract,PathBuf) {
    let c=fixture_contract();let root=h.home.join("npm/pi");
    for (p,b) in [("package.json",PACKAGE),("dist/bundle/cli.js",CODE),("dist/bundle/cli-runtime.js",b"bootstrap".as_slice()),("dist/bundle/chunks/chunk-TJXZSVTD.js",b"kernel".as_slice()),("dist/bundle/chunks/chunk-WJUNYZC5.js",b"config".as_slice())]{write(&root.join(p),b);}
    (c,root)
}

#[test]
fn complete_public_gate_never_executes_entry_bootstrap_or_version_probe() {
    let h=TestHome::new("pi-public-gate");let(c,root)=runtime(&h);let selected=root.join(&c.entry_path);
    assert_eq!(verify_runtime(&c,&selected,None).unwrap(),root);
    for member in GUARD_PATHS {
        let old=fs::read(root.join(member)).unwrap();write(&root.join(member),b"changed source");assert!(verify_runtime(&c,&selected,None).is_err());write(&root.join(member),&old);
    }
    assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn package_dir_override_cannot_borrow_an_unverified_manifest_or_resource_root() {
    let h=TestHome::new("pi-package-override");let(c,root)=runtime(&h);let selected=root.join(&c.entry_path);
    assert!(verify_runtime(&c,&selected,Some(root.as_os_str())).is_ok());
    let other=h.home.join("other");write(&other.join("package.json"),PACKAGE);assert!(verify_runtime(&c,&selected,Some(other.as_os_str())).is_err());
}

#[test]
fn builtins_are_extensions_with_unknown_enablement_and_never_fabricated_mcp_servers() {
    let h=TestHome::new("pi-builtin-catalog");let(c,root)=runtime(&h);let mut r=PluginScan::default();bundled(&host(),&c,&root,&mut r);normalize(&mut r);
    assert_eq!(r.plugins.len(),4);assert!(r.mcps.is_empty());assert!(r.plugins.iter().all(|p|p.presence==PluginPresence::Bundled&&p.enabled.is_none()));
    assert!(r.plugins.iter().any(|p|p.plugin_id=="builtin:mcp"));
    assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn unregistered_managed_caches_never_become_global_installations() {
    let h=TestHome::new("pi-unregistered-cache");let p=paths(&h);let cache=p.agent.join("npm/node_modules/unregistered");local(&cache,json!({"name":"unregistered","version":"1.0.0","pi":{"extensions":["index.js"]}}));write(&cache.join("index.js"),CODE);
    assert!(scan(&p).plugins.is_empty());assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn registered_npm_git_and_local_paths_keep_separate_physical_identities() {
    let h=TestHome::new("pi-registered-sources");let p=paths(&h);let npm=p.agent.join("npm/node_modules/@scope/tools");let git=p.agent.join("git/github.com/owner/tools");let dir=h.home.join("local-tools");
    for root in [&npm,&git,&dir]{local(root,json!({"name":"same-display-name","version":"1.2.3","pi":{"extensions":["index.ts"],"skills":["skills"]}}));write(&root.join("index.ts"),CODE);write(&root.join("skills/s/SKILL.md"),b"PRIVATE_BODY");}
    settings(&p,json!({"packages":["npm:@scope/tools@1.2.3","git:github.com/owner/tools@v1",dir]}));let r=scan(&p);
    assert_eq!(r.plugins.len(),3);assert_eq!(r.plugins.iter().map(|p|p.plugin_id.as_str()).collect::<BTreeSet<_>>().len(),3);assert!(r.mcps.is_empty());
    assert!(r.plugins.iter().all(|p|p.components.iter().any(|c|c.kind==PluginComponentKind::Code)&&p.components.iter().any(|c|c.kind==PluginComponentKind::Skill)));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_BODY"));
}

#[test]
fn first_global_registration_wins_deduplication_without_changing_package_storage() {
    let h=TestHome::new("pi-global-precedence");let p=paths(&h);let root=p.agent.join("npm/node_modules/tools");local(&root,json!({"name":"tools","version":"1.0.0","pi":{}}));
    settings(&p,json!({"packages":["npm:tools@1.0.0","npm:tools@2.0.0"]}));let r=scan(&p);assert_eq!(r.plugins.len(),1);assert_eq!(r.plugins[0].presence,PluginPresence::Installed);
    assert_eq!(r.plugins[0].version,"1.0.0");
}

#[test]
fn missing_or_version_mismatched_npm_registrations_never_install_or_invoke_npm_fallbacks() {
    let h=TestHome::new("pi-no-install");let p=paths(&h);let cache=p.agent.join("npm/node_modules/tools");local(&cache,json!({"name":"tools","version":"1.0.0","pi":{"extensions":["index.js"]}}));write(&cache.join("index.js"),CODE);
    write(&h.home.join("bin/npm"),CODE);settings(&p,json!({"packages":["npm:tools@2.0.0","npm:missing","git:github.com/owner/missing"]}));let r=scan(&p);
    assert_eq!(r.plugins.len(),3);assert!(r.plugins.iter().all(|p|p.presence==PluginPresence::Declared));assert!(r.mcps.is_empty());assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn pi_manifest_presence_and_object_filters_preserve_distinct_native_fallbacks() {
    let h=TestHome::new("pi-manifest-conventions");let p=paths(&h);let root=h.home.join("p");local(&root,json!({"name":"p","pi":{}}));write(&root.join("skills/s/SKILL.md"),b"PRIVATE_BODY");
    settings(&p,json!({"packages":[root]}));let plain=scan(&p);assert!(!plain.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
    settings(&p,json!({"packages":[{"source":root}]}));let object=scan(&p);assert!(object.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
    local(&root,json!({"name":"p","pi":{"skills":[]}}));let explicit_empty=scan(&p);assert!(!explicit_empty.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Skill));
}

#[test]
fn module_candidates_and_bom_json_metadata_never_read_or_import_component_bodies() {
    let h=TestHome::new("pi-module-bodies");let p=paths(&h);write(&p.agent.join("settings.json"),b"\xef\xbb\xbf{\"packages\":[]}");write(&p.agent.join("extensions/a.ts"),CODE);
    write(&p.agent.join("prompts/review.md"),b"PRIVATE_PROMPT");write(&p.agent.join("themes/a.json"),b"PRIVATE_THEME");let r=scan(&p);
    assert_eq!(r.plugins.len(),1);assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Code));assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Command));assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Theme));
    assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn globs_filters_and_ignore_rules_remain_references_with_unknown_runtime_visibility() {
    let h=TestHome::new("pi-opaque-policy");let p=paths(&h);let root=h.home.join("p");local(&root,json!({"name":"p","pi":{"extensions":["src/**/*.ts"],"skills":["skills"]}}));write(&root.join("skills/s/SKILL.md"),b"PRIVATE_BODY");write(&root.join("skills/.gitignore"),b"PRIVATE_IGNORE_POLICY");
    settings(&p,json!({"packages":[{"source":root,"skills":[],"extensions":["!src/secret.ts"]}]}));let r=scan(&p);
    assert!(r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::PluginReference));assert!(r.plugins[0].enabled.is_none());assert!(r.plugins[0].components.iter().all(|c|c.native_id.is_none()));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[cfg(unix)]
#[test]
fn component_escapes_and_symlinks_never_read_private_files() {
    use std::os::unix::fs::symlink;
    let h=TestHome::new("pi-component-links");let p=paths(&h);let root=h.home.join("p");local(&root,json!({"name":"p","pi":{"extensions":["../private.js","link.js"]}}));write(&h.home.join("private.js"),b"PRIVATE_MODULE_BODY");symlink(h.home.join("private.js"),root.join("link.js")).unwrap();
    settings(&p,json!({"packages":[root]}));let r=scan(&p);assert!(r.unavailable);assert!(r.mcps.is_empty());assert!(!r.plugins[0].components.iter().any(|c|c.kind==PluginComponentKind::Code));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_MODULE_BODY"));
}

#[test]
fn malformed_private_metadata_does_not_export_unknown_fields_or_hide_other_packages() {
    let h=TestHome::new("pi-metadata-fields");let p=paths(&h);let root=h.home.join("p");local(&root,json!({"name":"p","pi":{"prompts":["prompts"]},"apiKey":"PRIVATE_KEY","mcp":{"command":"PRIVATE_COMMAND"}}));write(&root.join("prompts/review.md"),b"PRIVATE_BODY");
    settings(&p,json!({"packages":[{"source":false},root],"apiKey":"PRIVATE_KEY","env":{"TOKEN":"PRIVATE_ENV"}}));let r=scan(&p);
    assert!(r.unavailable);assert!(r.plugins.iter().any(|p|p.components.iter().any(|c|c.kind==PluginComponentKind::Command)));assert!(!serde_json::to_string(&r.plugins).unwrap().contains("PRIVATE_"));
}

#[test]
fn agent_dir_override_is_global_and_unresolved_relative_bases_are_rejected() {
    let h=TestHome::new("pi-agent-dir");let custom=h.home.join("custom");assert_eq!(paths_from(&h.home,Some(custom.as_os_str())).unwrap().agent,custom);
    assert_eq!(paths_from(&h.home,Some(OsStr::new("~/custom"))).unwrap().agent,h.home.join("custom"));
    assert!(paths_from(&h.home,Some(OsStr::new("relative"))).is_err());
}

#[test]
fn unsupported_cli_identity_always_suppresses_the_execution_fallback() {
    let h=TestHome::new("pi-version-fallback");let file=h.home.join("pi");write(&file,CODE);
    assert_eq!(super::super::passive_runtime_version("pi",&file),Some(None));assert!(!h.home.join("pi-executed-marker").exists());
}

#[test]
fn watchers_follow_declared_resource_scopes_without_walking_agent_or_package_roots() {
    let h=TestHome::new("pi-watch-scopes");let p=paths(&h);let root=h.home.join("p");
    local(&root,json!({"name":"p","pi":{"skills":["resources/skills"]}}));write(&root.join("resources/skills/s/SKILL.md"),b"PRIVATE_BODY");
    write(&root.join("unrelated/private.js"),CODE);write(&p.agent.join("sessions/private.json"),b"PRIVATE_SESSION");
    settings(&p,json!({"packages":[root]}));let mut result=PluginScan::default();let watches=scan_sources(&host(),&p,&mut result);
    assert!(watches.contains(&root.join("package.json")));assert!(watches.contains(&root.join("resources/skills")));
    assert!(!watches.contains(&root));assert!(!watches.contains(&p.agent));assert!(!watches.iter().any(|v|v.starts_with(root.join("unrelated"))||v.starts_with(p.agent.join("sessions"))));
}

#[test]
fn bare_npm_partials_are_wildcards_and_unsupported_compound_ranges_remain_unresolved() {
    assert_eq!(npm_matches(Some("1.2"),"1.2.8"),Some(true));assert_eq!(npm_matches(Some("1.2"),"1.3.0"),Some(false));
    assert_eq!(npm_matches(Some("^1.2.0"),"1.3.0"),Some(true));assert_eq!(npm_matches(Some("1.2.x"),"1.3.0"),Some(false));
    assert_eq!(npm_matches(Some(">=1.0.0 <2.0.0"),"1.2.0"),None);
}

#[test]
fn automatic_extension_index_ts_has_native_priority_without_importing_either_file() {
    let h=TestHome::new("pi-extension-index-priority");let p=paths(&h);
    write(&p.agent.join("extensions/index.ts"),CODE);write(&p.agent.join("extensions/index.js"),CODE);write(&p.agent.join("extensions/other.ts"),CODE);
    let result=scan(&p);let code=result.plugins[0].components.iter().filter(|c|c.kind==PluginComponentKind::Code).collect::<Vec<_>>();
    assert_eq!(code.len(),1);assert!(code[0].source_path.ends_with("/index.ts"));assert!(!h.home.join("pi-executed-marker").exists());
}
