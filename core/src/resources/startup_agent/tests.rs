use super::*;
use crate::domain::startup_agent::StartupAgentDefinition;
use crate::testenv::TestHome;

#[test]
fn test_central_startup_agent_crud() {
    let _home = TestHome::new("startup-agent-crud");

    // Initially empty
    let list = list_central_startup_agents().unwrap();
    assert!(list.is_empty());

    // Create
    let def = StartupAgentDefinition {
        name: "architect".into(),
        display_name: "Architect".into(),
        description: "Designs system architecture".into(),
        system_prompt: "You are an architect.".into(),
        model: Some("claude-sonnet".into()),
        tools: vec!["read_file".into()],
        mode: "primary".into(),
        readonly: false,
        created_at: "".into(),
        updated_at: "".into(),
    };
    save_central_startup_agent(def.clone()).unwrap();

    // Read back
    let fetched = get_central_startup_agent("architect").unwrap().expect("must exist");
    assert_eq!(fetched.name, "architect");
    assert_eq!(fetched.description, "Designs system architecture");
    assert_eq!(fetched.mode, "primary");

    // List
    let list2 = list_central_startup_agents().unwrap();
    assert_eq!(list2.len(), 1);
    assert_eq!(list2[0].name, "architect");

    // Delete
    delete_central_startup_agent("architect").unwrap();
    assert!(get_central_startup_agent("architect").unwrap().is_none());
    assert!(list_central_startup_agents().unwrap().is_empty());
}

#[test]
fn test_opencode_startup_agent_state_and_switching() {
    let _home = TestHome::new("opencode-startup-agent-state");

    // Initial state: OpenCode has builtins "build" and "plan", default is "build"
    let state = get_agent_startup_agents_state("opencode").unwrap();
    assert_eq!(state.agent_id, "opencode");
    assert_eq!(state.default_agent, Some("build".to_string()));
    assert_eq!(state.cli_flag, Some("--agent".to_string()));
    assert!(state.supports_config_persistence);

    // Check items have build and plan
    let build_item = state.items.iter().find(|i| i.id == "build").expect("build exists");
    assert!(build_item.is_active);
    assert!(build_item.is_default);

    let plan_item = state.items.iter().find(|i| i.id == "plan").expect("plan exists");
    assert!(!plan_item.is_active);
    assert!(!plan_item.is_default);

    // Switch to plan
    set_active_startup_agent("opencode", Some("plan".to_string())).unwrap();

    let state2 = get_agent_startup_agents_state("opencode").unwrap();
    assert_eq!(state2.active_agent, Some("plan".to_string()));
    let plan_item2 = state2.items.iter().find(|i| i.id == "plan").unwrap();
    assert!(plan_item2.is_active);
    let build_item2 = state2.items.iter().find(|i| i.id == "build").unwrap();
    assert!(!build_item2.is_active);

    // Reset to default
    set_active_startup_agent("opencode", None).unwrap();
    let state3 = get_agent_startup_agents_state("opencode").unwrap();
    assert_eq!(state3.active_agent, None);
    let build_item3 = state3.items.iter().find(|i| i.id == "build").unwrap();
    assert!(build_item3.is_active);
}
