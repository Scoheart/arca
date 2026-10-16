//! Thin native-document IPC. Core owns discovery, validation and every write.
use arca_core::{application::instructions as core, domain::error::{CoreError, CoreResult}};
async fn worker<T: Send + 'static>(f: impl FnOnce() -> CoreResult<T> + Send + 'static) -> CoreResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|_| CoreError::new("instruction_worker_failed", "The native instruction editor worker failed. Please retry."))?
}
#[tauri::command]
pub async fn instruction_list(agent_id: String) -> CoreResult<core::InstructionInventory> { worker(move || core::list(&agent_id)).await }
#[tauri::command]
pub async fn instruction_get_paths(agent_id: String) -> CoreResult<core::InstructionPathConfiguration> { worker(move || core::get_paths(&agent_id)).await }
#[tauri::command]
pub async fn instruction_set_paths(agent_id: String, expected_revision: String, paths: Vec<core::InstructionPathUpdate>) -> CoreResult<core::InstructionPathConfiguration> {
    worker(move || core::set_paths(&agent_id, &expected_revision, paths)).await
}
#[tauri::command]
pub async fn instruction_read(agent_id: String, file_id: String) -> CoreResult<core::InstructionSnapshot> { worker(move || core::read(&agent_id, &file_id)).await }
#[tauri::command]
pub async fn instruction_prepare_save(agent_id: String, file_id: String, expected_revision: String, text: String) -> CoreResult<core::InstructionEditPlan> {
    worker(move || core::prepare_save(&agent_id, &file_id, &expected_revision, &text)).await
}
#[tauri::command]
pub async fn instruction_prepare_create(agent_id: String, inventory_revision: String, text: String) -> CoreResult<core::InstructionEditPlan> {
    worker(move || core::prepare_create(&agent_id, &inventory_revision, &text)).await
}
#[tauri::command]
pub async fn instruction_commit(operation_id: String, candidate_hash: String) -> CoreResult<core::InstructionSnapshot> {
    worker(move || core::commit(&operation_id, &candidate_hash)).await
}
