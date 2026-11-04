use arca_core::application::agents::list_capabilities;
use arca_core::testenv::TestHome;
use arca_core::types::AgentDefinition;
use std::collections::BTreeMap;

#[test]
fn every_audited_agent_keeps_its_configuration_capabilities_in_the_public_projection() {
    let _home = TestHome::new("all-agent-capabilities");
    let definitions: BTreeMap<String, AgentDefinition> = serde_json::from_str(include_str!("../../data/agents.json")).unwrap();
    let hosts: Vec<serde_json::Value> = serde_json::from_str(include_str!("../../data/native-plugin-hosts.json")).unwrap();
    let views = list_capabilities().unwrap().into_iter().map(|view| (view.identity.id.clone(), view)).collect::<BTreeMap<_, _>>();
    assert!(!definitions.is_empty());
    for (id, definition) in definitions {
        let view = views.get(&id).unwrap_or_else(|| panic!("{id}: the Agent disappeared from the public projection"));
        assert_eq!(view.identity.name, definition.name.as_deref().unwrap(), "{id}: identity must come from the registry");
        assert_eq!(view.capabilities.mcp.is_some(), definition.global.is_some(), "{id}: native resources must not imply a writer");
        if let Some(mcp) = &view.capabilities.mcp {
            assert!(mcp.writable, "{id}");
            assert_eq!(mcp.config_path, definition.global, "{id}");
            assert_eq!(mcp.key, definition.key, "{id}");
            assert_eq!(mcp.format, definition.format, "{id}");
        }
        assert_eq!(view.capabilities.skill.is_some(), definition.skills.is_some(), "{id}: plugin Skills must not imply a writable global directory");
        if let (Some(skill), Some(expected)) = (&view.capabilities.skill, &definition.skills) {
            assert_eq!(skill.global_dir, expected.global_dir, "{id}");
            assert_eq!(skill.target_id, expected.target_id, "{id}");
        }
        if let Some(model) = &view.capabilities.model {
            assert!(matches!(model.mode.as_str(), "managed" | "guided"), "{id}");
            if model.mode == "managed" { assert!(!model.config_paths.is_empty(), "{id}: a writer needs an actual configuration target"); }
        }
        println!("ARCA_AGENT_ASSET_AUDIT {}", serde_json::json!({
            "id": id, "name": view.identity.name, "category": view.identity.category,
            "mcp": view.capabilities.mcp, "model": view.capabilities.model, "skill": view.capabilities.skill,
            "native_plugin_observer": hosts.iter().any(|host| host["agent_id"].as_str() == Some(id.as_str())),
            "docs": definition.docs, "verified_at": definition.verified_at,
        }));
    }
}
