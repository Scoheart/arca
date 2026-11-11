//! Frontend use cases for Startup Agents lifecycle and switching.

use crate::domain::startup_agent::{
    AgentStartupAgentsStateView, StartupAgentDefinition,
};
use std::path::PathBuf;

pub fn list_central_startup_agents() -> Result<Vec<StartupAgentDefinition>, String> {
    crate::resources::startup_agent::list_central_startup_agents()
}

pub fn get_central_startup_agent(name: &str) -> Result<Option<StartupAgentDefinition>, String> {
    crate::resources::startup_agent::get_central_startup_agent(name)
}

pub fn save_central_startup_agent(definition: StartupAgentDefinition) -> Result<(), String> {
    crate::resources::startup_agent::save_central_startup_agent(definition)
}

pub fn delete_central_startup_agent(name: &str) -> Result<(), String> {
    crate::resources::startup_agent::delete_central_startup_agent(name)
}

pub fn get_agent_startup_agents_state(agent_id: &str) -> Result<AgentStartupAgentsStateView, String> {
    crate::resources::startup_agent::get_agent_startup_agents_state(agent_id)
}

pub fn set_active_startup_agent(agent_id: &str, startup_agent_id: Option<String>) -> Result<(), String> {
    crate::resources::startup_agent::set_active_startup_agent(agent_id, startup_agent_id)
}

pub fn project_startup_agent_to_agent(
    agent_id: &str,
    definition: &StartupAgentDefinition,
) -> Result<PathBuf, String> {
    crate::resources::startup_agent::project_startup_agent_to_agent(agent_id, definition)
}

pub fn remove_startup_agent_from_agent(agent_id: &str, name: &str) -> Result<(), String> {
    crate::resources::startup_agent::remove_startup_agent_from_agent(agent_id, name)
}

pub fn adopt_startup_agent(agent_id: &str, name: &str) -> Result<StartupAgentDefinition, String> {
    crate::resources::startup_agent::adopt_startup_agent(agent_id, name)
}
