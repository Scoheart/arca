//! Frontend use cases for the existing Hook resource lifecycle and projection.
use crate::domain::hook::HookDefinition;
use std::path::PathBuf;

pub fn list_central_hooks() -> Result<Vec<HookDefinition>, String> {
    crate::resources::hook::list_central_hooks()
}

pub fn get_central_hook(name: &str) -> Result<Option<HookDefinition>, String> {
    crate::resources::hook::get_central_hook(name)
}

pub fn save_central_hook(definition: HookDefinition) -> Result<(), String> {
    crate::resources::hook::save_central_hook(definition)
}

pub fn delete_central_hook(name: &str) -> Result<(), String> {
    crate::resources::hook::delete_central_hook(name)
}

pub fn list_agent_hooks(agent_id: &str) -> Result<Vec<HookDefinition>, String> {
    crate::resources::hook::list_agent_hooks(agent_id)
}

pub fn project_hook_to_agent(agent_id: &str, definition: &HookDefinition) -> Result<PathBuf, String> {
    crate::resources::hook::project_hook_to_agent(agent_id, definition)
}

pub fn remove_hook_from_agent(agent_id: &str, name: &str) -> Result<(), String> {
    crate::resources::hook::remove_hook_from_agent(agent_id, name)
}

pub fn adopt_hook(agent_id: &str, name: &str) -> Result<HookDefinition, String> {
    crate::resources::hook::adopt_hook(agent_id, name)
}
