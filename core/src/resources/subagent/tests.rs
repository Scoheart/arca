#[cfg(test)]
mod tests {
    use crate::domain::subagent::SubagentDefinition;
    use crate::resources::subagent::*;
    use crate::testenv::TestHome;

    fn sample_subagent(name: &str) -> SubagentDefinition {
        SubagentDefinition {
            name: name.to_string(),
            display_name: format!("Display {name}"),
            description: format!("Description for {name}"),
            system_prompt: format!("You are {name}, a specialized assistant."),
            model: Some("fast".to_string()),
            tools: vec!["read_file".to_string(), "run_command".to_string()],
            mcp_dependencies: vec!["github".to_string()],
            readonly: false,
            allow_subagents: true,
            created_at: "2026-11-02T00:00:00Z".to_string(),
            updated_at: "2026-11-02T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn central_subagent_crud_lifecycle() {
        let _home = TestHome::new("subagent-crud");
        let subagent = sample_subagent("planner");

        save_central_subagent(subagent.clone()).expect("save central subagent");
        let list = list_central_subagents().expect("list central subagents");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "planner");
        assert_eq!(list[0].display_name, "Display planner");

        let fetched = get_central_subagent("planner").expect("get central subagent").expect("exists");
        assert_eq!(fetched.name, subagent.name);
        assert_eq!(fetched.display_name, subagent.display_name);
        assert_eq!(fetched.system_prompt, subagent.system_prompt);
        assert_eq!(fetched.tools, subagent.tools);
        assert_eq!(fetched.mcp_dependencies, subagent.mcp_dependencies);
        assert_eq!(fetched.model, subagent.model);

        delete_central_subagent("planner").expect("delete central subagent");
        let list_after = list_central_subagents().expect("list central subagents after delete");
        assert!(list_after.is_empty());
    }

    #[test]
    fn project_to_claude_code_and_antigravity() {
        let home = TestHome::new("subagent-projection");
        let subagent = sample_subagent("qa-engineer");
        save_central_subagent(subagent.clone()).expect("save central subagent");

        project_subagent_to_agent("claude-code", &subagent).expect("project to claude-code");
        let claude_file = home.home.join(".claude/agents/qa-engineer.md");
        assert!(claude_file.exists());
        let claude_content = std::fs::read_to_string(&claude_file).unwrap();
        assert!(claude_content.contains("name: qa-engineer"));
        assert!(claude_content.contains("You are qa-engineer"));

        project_subagent_to_agent("antigravity", &subagent).expect("project to antigravity");
        let ag_file = home.home.join(".gemini/config/subagents/qa-engineer.json");
        assert!(ag_file.exists());
        let ag_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&ag_file).unwrap()).unwrap();
        assert_eq!(ag_json["Name"], "qa-engineer");
        assert_eq!(ag_json["SystemPrompt"], "You are qa-engineer, a specialized assistant.");

        // Clean removal
        remove_subagent_from_agent("claude-code", "qa-engineer").expect("remove from claude");
        assert!(!claude_file.exists());

        remove_subagent_from_agent("antigravity", "qa-engineer").expect("remove from ag");
        assert!(!ag_file.exists());
    }
}
