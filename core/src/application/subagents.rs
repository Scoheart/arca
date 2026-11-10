//! Frontend use cases for the existing central Subagent resource lifecycle.
use crate::domain::subagent::SubagentDefinition;

pub fn list_central_subagents() -> Result<Vec<SubagentDefinition>, String> {
    crate::resources::subagent::list_central_subagents()
}

pub fn get_central_subagent(name: &str) -> Result<Option<SubagentDefinition>, String> {
    crate::resources::subagent::get_central_subagent(name)
}

pub fn save_central_subagent(definition: SubagentDefinition) -> Result<(), String> {
    crate::resources::subagent::save_central_subagent(definition)
}

pub fn delete_central_subagent(name: &str) -> Result<(), String> {
    crate::resources::subagent::delete_central_subagent(name)
}

pub fn adopt_subagent(agent_id: &str, name: &str) -> Result<SubagentDefinition, String> {
    crate::resources::subagent::adopt_subagent(agent_id, name)
}
