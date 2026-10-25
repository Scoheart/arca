use arca_core::application::assets::{
    AssetCommitRequest, AssetOperationPlan, ConsumptionInventory, ModelAdoptionCandidate,
    PlanDeleteCentralAssetRequest, PlanSetAgentConsumptionRequest, PlanSetAssetConsumersRequest,
    PlanUpdateAgentCapabilitiesRequest, PlanUpdateCentralAssetRequest,
};
use arca_core::application::mcp::catalog::{
    read_registry, read_registry_all, user_override_keys, CatalogItem,
};
use arca_core::application::operations::{
    CancelOperationRequest, CommitOperationRequest, OperationCommitResult,
    OperationPlan as UnifiedOperationPlan, PlanOperationRequest,
};
use arca_core::application::skills::{
    GithubEndpoints, OperationPlan, PlanImportRequest, PlanRemoveRequest, PlanRepairRequest,
    PlanSkillAssetImportRequest, PlanSkillAssetInstallRequest, PlanUpdateRequest, SkillAgentView,
    SkillCommitRequest, SkillDetail, SkillError, SkillSourceInput, SkillSourceResolution,
    SkillsInventory, UpdateCheckOutcome,
};
use arca_core::application::{BackendStatus, ArcaCore};
use arca_core::domain::error::{CoreError, CoreResult};
use arca_core::domain::types::RegistryEntry;
use serde_json::Value;
use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::io::Write;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AssetCommandError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
}

impl From<String> for AssetCommandError {
    fn from(error: String) -> Self {
        let error = core_error_from_legacy(error, "asset_operation_failed");
        Self {
            code: error.code,
            message: error.message,
            details: error.details,
        }
    }
}

fn core_error_from_legacy(error: String, fallback_code: &str) -> CoreError {
    let (code, message) = error
        .split_once(':')
        .filter(|(code, _)| {
            !code.is_empty()
                && code
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '_')
        })
        .map(|(code, message)| (code.trim(), message.trim()))
        .unwrap_or((fallback_code, error.as_str()));
    enrich_backend_error(CoreError::new(code, message))
}

fn enrich_backend_error(mut error: CoreError) -> CoreError {
    match ArcaCore::backend_status() {
        BackendStatus::Starting if error.code == "backend_initializing" => {
            error
                .details
                .insert("backend_state".into(), Value::String("starting".into()));
        }
        BackendStatus::ReadOnly { stage, .. } if error.code == "recovery_required" => {
            error
                .details
                .insert("backend_state".into(), Value::String("read_only".into()));
            error.details.insert("stage".into(), Value::String(stage));
        }
        BackendStatus::CapabilityUnavailable {
            capability,
            stage,
            code,
            ..
        } if error.code == code => {
            error.details.insert(
                "backend_state".into(),
                Value::String("capability_unavailable".into()),
            );
            error
                .details
                .insert("capability".into(), serde_json::json!(capability));
            error.details.insert("stage".into(), Value::String(stage));
        }
        _ => {}
    }
    error
}

async fn asset_blocking<T, F>(operation: F) -> Result<T, AssetCommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| AssetCommandError {
            code: "worker_failed".into(),
            message: "后台任务失败，请重试。".into(),
            details: BTreeMap::new(),
        })?
        .map_err(Into::into)
}

async fn core_blocking<T, F>(operation: F) -> CoreResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CoreResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| CoreError::new("worker_failed", "后台任务失败，请重试。"))?
}

async fn blocking_command<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(operation).await
        .map_err(|_| "worker_failed: Background query failed".to_owned())
}

#[tauri::command]
pub async fn get_registry_snapshot() -> Result<arca_core::application::mcp::catalog::RegistrySnapshot, String> {
    blocking_command(arca_core::application::mcp::catalog::read_registry_snapshot).await
}

#[tauri::command]
pub async fn get_resource_observation() -> Result<arca_core::application::assets::ResourceObservation, String> {
    blocking_command(arca_core::application::assets::observe_resources).await
}

#[tauri::command]
pub async fn get_workspace_snapshot(
) -> CoreResult<arca_core::application::workspace::WorkspaceSnapshot> {
    core_blocking(arca_core::application::ArcaCore::snapshot).await
}

#[tauri::command]
pub async fn get_backend_status() -> Result<BackendStatus, String> {
    blocking_command(move || {
        ArcaCore::backend_status()
    }).await
}

#[tauri::command]
pub async fn unlock_model_credentials(retry: bool) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::models::unlock_credential_session(retry)
    }).await.map_err(|_| "credential_session_unavailable: Credential session worker failed".to_string())?
}

#[tauri::command]
pub fn get_model_credential_session_status() -> serde_json::Value {
    arca_core::application::models::credential_session_status()
}

/// Write user-requested text to the native clipboard. macOS WebViews can
/// reject `navigator.clipboard` for a dev origin, while `pbcopy` uses the
/// system clipboard service directly. The text is never logged or persisted.
#[tauri::command]
pub async fn copy_to_clipboard(text: String) -> Result<(), String> {
    blocking_command(move || {
        if text.contains('\0') {
            return Err("clipboard_invalid: text contains a NUL byte".into());
        }
        #[cfg(target_os = "macos")]
        {
            let mut process = Command::new("/usr/bin/pbcopy")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("clipboard_unavailable: {error}"))?;
            let mut input = process
                .stdin
                .take()
                .ok_or_else(|| "clipboard_unavailable: pbcopy stdin unavailable".to_string())?;
            input
                .write_all(text.as_bytes())
                .map_err(|error| format!("clipboard_write_failed: {error}"))?;
            drop(input);
            let status = process
                .wait()
                .map_err(|error| format!("clipboard_unavailable: {error}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("clipboard_write_failed: pbcopy exited unsuccessfully".into())
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err("clipboard_unsupported: native clipboard integration is unavailable".into())
        }
    }).await?
}

#[tauri::command]
pub async fn list_agent_capabilities(
) -> CoreResult<Vec<arca_core::application::agents::AgentCapabilityView>> {
    core_blocking(arca_core::application::agents::list_capabilities).await
}

#[tauri::command]
pub async fn plan_operation(request: PlanOperationRequest) -> CoreResult<UnifiedOperationPlan> {
    core_blocking(move || arca_core::application::ArcaCore::plan(request)).await
}

#[tauri::command]
pub async fn commit_operation(
    request: CommitOperationRequest,
) -> CoreResult<OperationCommitResult> {
    core_blocking(move || arca_core::application::ArcaCore::commit(request)).await
}

#[tauri::command]
pub async fn cancel_operation(request: CancelOperationRequest) -> CoreResult<()> {
    core_blocking(move || arca_core::application::ArcaCore::cancel(request)).await
}

#[tauri::command]
pub async fn list_consumption_inventory() -> Result<ConsumptionInventory, AssetCommandError> {
    asset_blocking(arca_core::application::assets::list_inventory).await
}

#[tauri::command]
pub async fn list_model_adoption_candidates(
) -> Result<Vec<ModelAdoptionCandidate>, AssetCommandError> {
    asset_blocking(arca_core::application::assets::list_model_adoption_candidates).await
}

#[tauri::command]
pub async fn plan_set_agent_consumption(
    request: PlanSetAgentConsumptionRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_agent_consumption(request)).await
}

#[tauri::command]
pub async fn plan_set_mcp_enabled(
    request: arca_core::application::assets::PlanSetMcpEnabledRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_mcp_enabled(request)).await
}

#[tauri::command]
pub async fn plan_set_skill_enabled(
    request: arca_core::application::assets::PlanSetSkillEnabledRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_skill_enabled(request)).await
}

#[tauri::command]
pub async fn plan_set_model_enabled(
    request: arca_core::application::assets::PlanSetModelEnabledRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_model_enabled(request)).await
}

#[tauri::command]
pub async fn plan_set_active_model(
    request: arca_core::application::assets::PlanSetActiveModelRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_active_model(request)).await
}

#[tauri::command]
pub async fn plan_set_asset_consumers(
    request: PlanSetAssetConsumersRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_set_asset_consumers(request)).await
}

#[tauri::command]
pub async fn plan_update_agent_capabilities(
    request: PlanUpdateAgentCapabilitiesRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_update_agent_capabilities(request))
        .await
}

#[tauri::command]
pub async fn plan_update_central_asset(
    request: PlanUpdateCentralAssetRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_update_central_asset(request)).await
}

#[tauri::command]
pub async fn plan_delete_central_asset(
    request: PlanDeleteCentralAssetRequest,
) -> Result<AssetOperationPlan, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::plan_delete_central_asset(request)).await
}

#[tauri::command]
pub async fn commit_asset_operation(
    request: AssetCommitRequest,
) -> Result<ConsumptionInventory, AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::commit_asset_operation(request)).await
}

#[tauri::command]
pub async fn cancel_asset_operation(operation_id: String) -> Result<(), AssetCommandError> {
    asset_blocking(move || arca_core::application::assets::cancel_asset_operation(&operation_id))
        .await
}

// ── User-level Skills ────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillCommandError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub findings_hash: Option<String>,
}

impl From<SkillError> for SkillCommandError {
    fn from(error: SkillError) -> Self {
        let mut parts = error.into_command_parts();
        let embedded_gate_code = parts
            .message
            .split_once(':')
            .filter(|(code, _)| matches!(*code, "backend_initializing" | "recovery_required"))
            .map(|(code, message)| (code.to_string(), message.trim_start().to_string()));
        let (code, message) = embedded_gate_code
            .unwrap_or_else(|| (parts.code.to_string(), std::mem::take(&mut parts.message)));
        let core = enrich_backend_error(CoreError::new(code, &message));
        Self {
            code: core.code,
            message,
            details: core.details,
            retry_at: parts.retry_at,
            findings_hash: parts.findings_hash,
        }
    }
}

fn worker_error<T: std::fmt::Display>(_error: T) -> SkillCommandError {
    SkillCommandError {
        code: "worker_failed".into(),
        message: "后台任务失败，请重试。".into(),
        details: BTreeMap::new(),
        retry_at: None,
        findings_hash: None,
    }
}

fn dialog_path_error<T: std::fmt::Display>(_error: T) -> SkillCommandError {
    SkillCommandError {
        code: "invalid_local_folder".into(),
        message: "无法读取所选本地文件夹。".into(),
        details: BTreeMap::new(),
        retry_at: None,
        findings_hash: None,
    }
}

async fn skill_blocking<T, F>(operation: F) -> Result<T, SkillCommandError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SkillError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(worker_error)?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_skills_inventory() -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(arca_core::application::skills::list_inventory).await
}

#[tauri::command]
pub async fn list_skill_agents() -> Result<Vec<SkillAgentView>, SkillCommandError> {
    skill_blocking(arca_core::application::skills::list_skill_agents).await
}

#[tauri::command]
pub async fn get_skill_detail(identity: String) -> Result<SkillDetail, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::get_skill_detail(&identity)).await
}

#[tauri::command]
pub async fn resolve_skill_source(
    value: String,
) -> Result<SkillSourceResolution, SkillCommandError> {
    skill_blocking(move || {
        arca_core::application::skills::resolve_source(
            SkillSourceInput::Github { value },
            GithubEndpoints::production(),
        )
    })
    .await
}

#[tauri::command]
pub async fn resolve_local_skill_source_dialog(
    app: tauri::AppHandle,
) -> Result<Option<SkillSourceResolution>, SkillCommandError> {
    use tauri_plugin_dialog::DialogExt;

    let picked =
        tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
            .await
            .map_err(worker_error)?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let path = path.into_path().map_err(dialog_path_error)?;
    let value = path
        .to_str()
        .ok_or_else(|| SkillCommandError {
            code: "invalid_local_folder".into(),
            message: "所选本地文件夹路径不是有效 UTF-8。".into(),
            details: BTreeMap::new(),
            retry_at: None,
            findings_hash: None,
        })?
        .to_owned();

    skill_blocking(move || {
        arca_core::application::skills::resolve_source(
            SkillSourceInput::Local { path: value },
            GithubEndpoints::production(),
        )
    })
    .await
    .map(Some)
}

#[tauri::command]
pub async fn resolve_archive_skill_source_dialog(
    app: tauri::AppHandle,
) -> Result<Option<SkillSourceResolution>, SkillCommandError> {
    use tauri_plugin_dialog::DialogExt;

    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("Skill 压缩包", &["zip", "tar", "tgz", "gz"])
            .blocking_pick_file()
    })
    .await
    .map_err(worker_error)?;
    let Some(path) = picked else {
        return Ok(None);
    };
    let path = path.into_path().map_err(dialog_path_error)?;
    let value = path
        .to_str()
        .ok_or_else(|| SkillCommandError {
            code: "invalid_archive_file".into(),
            message: "所选压缩包路径不是有效 UTF-8。".into(),
            details: BTreeMap::new(),
            retry_at: None,
            findings_hash: None,
        })?
        .to_owned();

    skill_blocking(move || {
        arca_core::application::skills::resolve_source(
            SkillSourceInput::Archive { path: value },
            GithubEndpoints::production(),
        )
    })
    .await
    .map(Some)
}

#[tauri::command]
pub async fn plan_skill_asset_install(
    request: PlanSkillAssetInstallRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_asset_install(request)).await
}

#[tauri::command]
pub async fn commit_skill_install(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_install(request)).await
}

#[tauri::command]
pub async fn plan_skill_import(
    request: PlanImportRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_import(request)).await
}

#[tauri::command]
pub async fn plan_skill_asset_import(
    request: PlanSkillAssetImportRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_asset_import(request)).await
}

#[tauri::command]
pub async fn commit_skill_import(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_import(request)).await
}

#[tauri::command]
pub async fn plan_skill_update(
    request: PlanUpdateRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_update(request)).await
}

#[tauri::command]
pub async fn commit_skill_update(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_update(request)).await
}

#[tauri::command]
pub async fn plan_skill_remove(
    request: PlanRemoveRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_remove(request)).await
}

#[tauri::command]
pub async fn commit_skill_remove(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_remove(request)).await
}

#[tauri::command]
pub async fn commit_skill_assignment(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_assignment(request)).await
}

#[tauri::command]
pub async fn plan_skill_repair(
    request: PlanRepairRequest,
) -> Result<OperationPlan, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::plan_repair(request)).await
}

#[tauri::command]
pub async fn commit_skill_repair(
    request: SkillCommitRequest,
) -> Result<SkillsInventory, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::commit_repair(request)).await
}

#[tauri::command]
pub async fn check_skill_updates(manual: bool) -> Result<UpdateCheckOutcome, SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::check_updates(manual)).await
}

#[tauri::command]
pub async fn cancel_skill_operation(operation_id: String) -> Result<(), SkillCommandError> {
    skill_blocking(move || arca_core::application::skills::cancel_operation(&operation_id)).await
}

// ── Model endpoint profiles ─────────────────────────────────────────────

#[tauri::command]
pub async fn export_model_curl(profile_id: String, include_api_key: bool) -> Result<String, String> {
    blocking_command(move || arca_core::application::models::export_curl(&profile_id, include_api_key)).await?
}

#[tauri::command]
pub async fn list_model_profiles() -> Result<Vec<arca_core::application::models::ModelProfileView>, String> {
    // Profile enumeration checks Keychain and may wait for the workspace gate.
    // Never perform that work on the WebView event thread.
    tauri::async_runtime::spawn_blocking(arca_core::application::models::list_profiles)
        .await
        .map_err(|_| "model_profiles_worker_failed: Background profile query failed".to_owned())
}

#[tauri::command]
pub fn list_model_providers() -> &'static [arca_core::application::models::ModelProviderView] {
    arca_core::application::models::list_providers()
}

#[tauri::command]
pub async fn list_model_provider_instances(
) -> Result<Vec<arca_core::application::models::ModelProviderInstanceView>, String> {
    blocking_command(move || {
        arca_core::application::models::list_provider_instances()
    }).await
}

#[tauri::command]
pub async fn discover_provider_models(
    provider_id: String,
) -> Result<Vec<arca_core::application::models::ProviderModelSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::models::discover_provider_models(&provider_id)
    })
    .await
    .map_err(|_| "model_discovery_worker_failed: Background model discovery failed".to_owned())?
}

#[tauri::command]
pub async fn reveal_model_provider_credential(provider_id: String) -> Result<String, String> {
    blocking_command(move || {
        arca_core::application::models::reveal_provider_credential(&provider_id)
    }).await?
}

#[tauri::command]
pub fn infer_model_provider(base_url: String) -> String {
    arca_core::application::models::infer_provider(&base_url)
}

#[tauri::command]
pub async fn validate_model_credential_source(
    source: arca_core::domain::types::ApiKeySource,
) -> Result<arca_core::application::models::CredentialValidationView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::models::validate_credential_source(&source)
    })
    .await
    .map_err(|_| "credential_validation_worker_failed: credential validation worker failed".to_owned())?
}

#[tauri::command]
pub async fn list_model_agents() -> Result<Vec<arca_core::application::models::ModelAgentView>, String> {
    tauri::async_runtime::spawn_blocking(arca_core::application::models::list_agent_capabilities)
        .await
        .map_err(|_| "model_agents_worker_failed: Background capability query failed".to_owned())?
}

#[tauri::command]
pub async fn set_model_credential_delivery(
    agent_id: String,
    profile_id: String,
    delivery: arca_core::application::models::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<arca_core::resources::model::ModelApplyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::models::set_credential_delivery(
            &agent_id,
            &profile_id,
            delivery,
            confirm_plaintext,
        )
    })
    .await
    .map_err(|_| "model_credential_worker_failed: credential delivery worker failed".to_owned())?
}

#[tauri::command]
pub async fn set_agent_credential_delivery(
    agent_id: String,
    delivery: arca_core::application::models::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<arca_core::resources::model::ModelApplyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::models::set_agent_credential_delivery(
            &agent_id,
            delivery,
            confirm_plaintext,
        )
    })
    .await
    .map_err(|_| "model_credential_worker_failed: credential delivery worker failed".to_owned())?
}

#[tauri::command]
pub async fn list_registry() -> Result<Vec<RegistryEntry>, String> {
    blocking_command(move || {
        // Read user overrides from settings.registry merged over builtin — same source
        // scan_installed / apply_install resolve against, so the UI stays consistent.
        read_registry()
    }).await
}

/// Every entry copy from all enabled sources (not deduped), each flagged with
/// whether it's the in-effect (winning) copy. Drives the Registry's "全部" /
/// per-source views that must show shadowed copies too.
#[tauri::command]
pub async fn list_registry_all() -> Result<Vec<CatalogItem>, String> {
    blocking_command(move || {
        read_registry_all()
    }).await
}

/// Composite keys (`name::transport`) of registry entries that currently have a
/// user override.
#[tauri::command]
pub async fn list_custom_registry_keys() -> Result<Vec<String>, String> {
    blocking_command(move || {
        user_override_keys()
    }).await
}

#[derive(serde::Serialize)]
pub struct PastedMcpSummary {
    name: String,
    transport: &'static str,
}

/// Read-only preview using the same core parser as import. Return identities
/// only: credentials and connection values never enter the preview response.
#[tauri::command]
pub async fn preview_pasted_config(text: String) -> CoreResult<Vec<PastedMcpSummary>> {
    tauri::async_runtime::spawn_blocking(move || {
        arca_core::application::mcp::operations::parse_pasted_entries(&text)
            .map(|entries| entries.into_iter().map(|entry| {
                let transport = entry.transport();
                PastedMcpSummary { name: entry.name, transport }
            }).collect())
            .map_err(|error| core_error_from_legacy(error, "invalid_config"))
    }).await.map_err(|_| CoreError::new("worker_failed", "配置识别失败，请重试。"))?
}

/// Parse a pasted config blob (JSON, TOML or YAML) and add every MCP server it contains
/// to the managed "manual" source. Returns the names that were added.
#[tauri::command]
pub async fn import_pasted_config(text: String) -> CoreResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(move || import_pasted_config_blocking(text))
        .await.map_err(|_| CoreError::new("worker_failed", "后台导入任务失败，请重试。"))?
}

fn import_pasted_config_blocking(text: String) -> CoreResult<Vec<String>> {
    let entries = arca_core::application::mcp::operations::parse_pasted_entries(&text)
        .map_err(|error| core_error_from_legacy(error, "invalid_config"))?;
    let mut known_keys: std::collections::BTreeSet<_> = read_registry().iter().map(|entry| entry.key()).collect();
    let mut names = Vec::new();
    let entry_count = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        let key = entry.key();
        let existing_key = known_keys.contains(&key).then(|| key.clone());
        let plan = arca_core::application::assets::plan_update_central_asset(
            PlanUpdateCentralAssetRequest {
                draft: arca_core::application::assets::CentralAssetDraft::Mcp {
                    existing_key,
                    entry: Box::new(entry),
                },
            },
        )
        .map_err(|error| core_error_from_legacy(error, "asset_operation_failed"))?;
        if !plan.can_commit {
            let _ = arca_core::application::assets::cancel_asset_operation(&plan.operation_id);
            return Err(CoreError::new(
                "asset_operation_blocked",
                "粘贴导入与当前资产状态冲突；请先在资源界面收敛对应消费关系",
            ));
        }
        let saved_names = plan.central_changes
                .iter()
                .filter_map(|change| match &change.asset {
                    arca_core::application::assets::AssetRef::Mcp { key } => {
                        key.rsplit_once("::").map(|(name, _)| name.to_string())
                    }
                    _ => None,
                }).collect::<Vec<_>>();
        if let Err(error) = arca_core::application::assets::commit_mcp_import(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        }) {
            let mut error = core_error_from_legacy(error, "asset_operation_failed");
            if error.code == "target_convergence_failed" {
                names.extend(saved_names);
                error.details.insert("central_saved".into(), Value::Bool(true));
                error.details.insert("saved_count".into(), serde_json::json!(names.len()));
                error.details.insert("remaining_count".into(), serde_json::json!(entry_count - index - 1));
            }
            return Err(error);
        }
        names.extend(saved_names);
        known_keys.insert(key);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

// ── Catalog sources (subscribe remote / add local) ────────────────────────
// Orchestration lives in `arca_core::application::mcp::sources`; these are thin command wrappers.

use arca_core::application::mcp::sources::{self, SourceView};

#[tauri::command]
pub async fn list_sources() -> Result<Vec<SourceView>, String> {
    blocking_command(move || {
        sources::list_views()
    }).await
}

#[tauri::command]
pub async fn subscribe_source(url: String, name: Option<String>) -> CoreResult<SourceView> {
    core_blocking(move || {
        sources::subscribe(url, name)
            .map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

/// Add a local source from an explicit path.
#[tauri::command]
pub async fn add_local_source(path: String, name: Option<String>) -> CoreResult<SourceView> {
    core_blocking(move || {
        sources::add_local(path, name)
            .map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

/// Open a native file picker and add the chosen file as a local source. Returns
/// `None` if the user cancels. Desktop-only (native dialog); delegates to core.
///
/// **Must not block the main thread.** Sync Tauri commands run on the main
/// thread, and `blocking_pick_file` there deadlocks: the panel needs the main
/// run loop to process the user's click, but the thread is parked waiting for
/// that very click → beachball/hang. So this is an `async` command (runs off the
/// main thread) and the blocking pick is pushed onto a worker via
/// `spawn_blocking`, leaving the main thread free to drive the panel.
#[tauri::command]
pub async fn add_local_source_dialog(app: tauri::AppHandle) -> CoreResult<Option<SourceView>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("MCP 配置", &["json", "toml"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| CoreError::new("worker_failed", "后台任务失败，请重试。"))?;
    let Some(fp) = picked else { return Ok(None) };
    sources::add_local(fp.to_string(), None)
        .map(Some)
        .map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
}

/// Export the complete effective catalog to a JSON file the user picks via a
/// native save dialog. Returns the written path, or `None` if the user cancels.
/// Async + `spawn_blocking` for the same main-thread reason as the picker above.
#[tauri::command]
pub async fn export_effective_dialog(app: tauri::AppHandle) -> CoreResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let content = arca_core::application::mcp::operations::export_effective()
        .map_err(|error| core_error_from_legacy(error, "export_failed"))?;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("MCP 配置", &["json"])
            .set_file_name("arca-effective.json")
            .blocking_save_file()
    })
    .await
    .map_err(|_| CoreError::new("worker_failed", "后台任务失败，请重试。"))?;
    let Some(fp) = picked else { return Ok(None) };
    let path = fp
        .into_path()
        .map_err(|error| CoreError::new("invalid_export_path", error.to_string()))?;
    ArcaCore::independent_host_mutation("desktop_export", move || {
        std::fs::write(&path, content)
            .map_err(|error| CoreError::new("export_failed", error.to_string()))?;
        Ok(Some(path.display().to_string()))
    })
}

/// Add the bundled curated collection as an opt-in local source.
#[tauri::command]
pub async fn add_builtin_collection() -> CoreResult<SourceView> {
    core_blocking(move || {
        sources::add_official()
            .map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

#[tauri::command]
pub async fn refresh_source(id: String) -> CoreResult<SourceView> {
    core_blocking(move || {
        sources::refresh(id).map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

#[tauri::command]
pub async fn set_source_enabled(id: String, enabled: bool) -> CoreResult<()> {
    core_blocking(move || {
        sources::set_enabled(id, enabled)
            .map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

#[tauri::command]
pub async fn remove_source(id: String) -> CoreResult<()> {
    core_blocking(move || {
        sources::remove(id).map_err(|error| core_error_from_legacy(error, "source_operation_failed"))
    }).await
}

use arca_core::application::agents::{load_agents, AgentInfo};
use arca_core::domain::types::AgentDefinition;

/// 新增一个自定义 agent，持久化到 settings.agents（在内置/已有定义之上合并）。
/// id 为空或已存在时报错，避免误覆盖内置 agent。
#[tauri::command]
pub async fn add_agent(id: String, def: AgentDefinition) -> CoreResult<()> {
    core_blocking(move || {
        arca_core::application::agents::put(id, def, false)
            .map_err(|error| core_error_from_legacy(error, "agent_operation_failed"))
    }).await
}

/// 编辑一个已存在 agent 的配置（路径 / 格式 / key），覆盖写回 settings.agents。
#[tauri::command]
pub async fn update_agent(id: String, def: AgentDefinition) -> CoreResult<()> {
    core_blocking(move || {
        arca_core::application::agents::put(id, def, true)
            .map_err(|error| core_error_from_legacy(error, "agent_operation_failed"))
    }).await
}

#[tauri::command]
pub async fn list_agents() -> Result<Vec<AgentInfo>, String> {
    blocking_command(move || {
        arca_core::application::agents::list_infos()
    }).await
}

#[tauri::command]
pub async fn get_pinned_agents() -> Result<Vec<String>, String> {
    blocking_command(move || {
        arca_core::application::ui::get_pinned_agents()
    }).await?
}

#[tauri::command]
pub async fn set_pinned_agents(agent_ids: Vec<String>) -> CoreResult<Vec<String>> {
    core_blocking(move || {
        arca_core::application::ui::set_pinned_agents(agent_ids)
            .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
    }).await
}

#[tauri::command]
pub async fn get_ui_locale() -> Result<Option<String>, String> {
    blocking_command(move || {
        arca_core::application::ui::get_ui_locale()
    }).await?
}

#[tauri::command]
pub async fn set_ui_locale(locale: Option<String>) -> CoreResult<Option<String>> {
    core_blocking(move || {
        arca_core::application::ui::set_ui_locale(locale)
            .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
    }).await
}

#[tauri::command]
pub async fn list_mcp_icon_preferences(
) -> CoreResult<BTreeMap<String, arca_core::application::ui::McpIconPreferenceView>> {
    core_blocking(|| {
        arca_core::application::ui::list_mcp_icon_preferences()
            .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
    })
    .await
}

#[tauri::command]
pub async fn set_mcp_builtin_icon(
    asset_key: String,
    icon_id: String,
) -> CoreResult<BTreeMap<String, arca_core::application::ui::McpIconPreferenceView>> {
    core_blocking(move || {
        arca_core::application::ui::set_mcp_builtin_icon(asset_key, icon_id)
            .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
    }).await
}

#[tauri::command]
pub async fn reset_mcp_icon(
    asset_key: String,
) -> CoreResult<BTreeMap<String, arca_core::application::ui::McpIconPreferenceView>> {
    core_blocking(move || {
        arca_core::application::ui::reset_mcp_icon(asset_key)
            .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
    }).await
}

#[tauri::command]
pub async fn import_mcp_icon_dialog(
    app: tauri::AppHandle,
    asset_key: String,
) -> CoreResult<Option<BTreeMap<String, arca_core::application::ui::McpIconPreferenceView>>> {
    use tauri_plugin_dialog::DialogExt;
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("MCP 图标", &["png", "jpg", "jpeg", "webp"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| CoreError::new("worker_failed", "后台任务失败，请重试。"))?;
    let Some(file_path) = picked else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|error| CoreError::new("invalid_icon_path", error.to_string()))?;
    arca_core::application::ui::import_mcp_icon(asset_key, path)
        .map(Some)
        .map_err(|error| core_error_from_legacy(error, "ui_preference_failed"))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProxySettingsView {
    pub proxy_url: Option<String>,
}

#[tauri::command]
pub async fn get_proxy_settings() -> Result<ProxySettingsView, String> {
    blocking_command(move || {
        arca_core::application::network::get_proxy_settings().map(|settings| ProxySettingsView {
            proxy_url: settings.proxy_url,
        })
    }).await?
}

#[tauri::command]
pub async fn set_proxy_settings(proxy_url: Option<String>) -> CoreResult<ProxySettingsView> {
    core_blocking(move || {
        arca_core::application::network::set_proxy_url(proxy_url)
            .map(|settings| ProxySettingsView {
                proxy_url: settings.proxy_url,
            })
            .map_err(|error| core_error_from_legacy(error, "network_settings_failed"))
    }).await
}

pub use arca_core::application::mcp::operations::InstalledMcp;

/// 扫描全局配置文件，返回「谁装在哪」。Arca 当前不管理项目级配置。
#[tauri::command]
pub async fn scan_installed() -> Result<Vec<InstalledMcp>, String> {
    blocking_command(move || {
        arca_core::application::mcp::operations::scan_installed(None)
    }).await
}

use arca_core::application::mcp::operations::effective_config;
use arca_core::application::mcp::operations::{resolve_entry, target_file};
use arca_core::domain::mcp::OverridePatch;
use std::collections::HashMap;

#[derive(serde::Deserialize)]
pub struct PatchInput {
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}
impl PatchInput {
    fn to_patch(&self) -> OverridePatch {
        OverridePatch {
            args: self.args.clone(),
            env: self.env.clone(),
            url: self.url.clone(),
            headers: self.headers.clone(),
        }
    }
}

#[derive(serde::Deserialize)]
pub struct InstallRequest {
    pub server_name: String,
    /// Transport variant to resolve ("stdio" | "http"). Defaults to stdio for
    /// older callers. The on-disk app config is still keyed by `server_name`.
    #[serde(default = "default_transport")]
    pub transport: String,
    pub agents: Vec<String>,
    #[serde(default)]
    pub overrides: HashMap<String, PatchInput>, // agentId -> patch
}

fn default_transport() -> String {
    "stdio".to_string()
}

#[derive(serde::Serialize)]
pub struct PlannedWrite {
    pub agent: String,
    pub file_path: String,
    pub config_json: String,
}

#[tauri::command]
pub async fn preview_install(req: InstallRequest) -> Result<Vec<PlannedWrite>, String> {
    blocking_command(move || {
        let entry = resolve_entry(&req.server_name, &req.transport)?;
        let agents = load_agents();
        let mut out = Vec::new();
        for agent_id in &req.agents {
            let def = agents
                .get(agent_id)
                .ok_or_else(|| format!("{agent_id}: unknown Agent"))?;
            if !arca_core::application::agents::supports_transport(agent_id, &req.transport) {
                return Err(format!(
                    "{agent_id}: {} transport is not supported by this agent",
                    req.transport
                ));
            }
            let path = target_file(def, "global", None)
                .ok_or_else(|| format!("{agent_id}: global config path is unavailable"))?;
            let patch = req.overrides.get(agent_id).map(|p| p.to_patch());
            let cfg = effective_config(&entry, patch.as_ref())
                .ok_or_else(|| format!("no config for {}", req.server_name))?;
            out.push(PlannedWrite {
                agent: agent_id.clone(),
                file_path: path.display().to_string(),
                config_json: serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?,
            });
        }
        Ok(out)
    }).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use arca_core::domain::types::{McpConfig, StdioConfig};

    #[test]
    fn preview_returns_planned_write_for_seeded_server() {
        use arca_core::domain::types::{RegistryConfig, RegistryEntry, StdioConfig};
        // No built-in catalog anymore: seed a manual entry through the real store
        // (a managed local source) in an isolated ~/.arca, then preview it.
        let _th = arca_core::testenv::TestHome::new("preview");
        let plan = arca_core::application::assets::plan_update_central_asset(
            PlanUpdateCentralAssetRequest {
                draft: arca_core::application::assets::CentralAssetDraft::Mcp {
                    existing_key: None,
                    entry: Box::new(RegistryEntry {
                        name: "seeded".into(),
                        description: String::new(),
                        tags: vec![],
                        config: RegistryConfig {
                            stdio: Some(StdioConfig {
                                command: "npx".into(),
                                args: Some(vec!["-y".into(), "seeded".into()]),
                                env: None,
                                cwd: None,
                            }),
                            http: None,
                        },
                        origin: None,
                        repo: None,
                    }),
                },
            },
        )
        .unwrap();
        arca_core::application::assets::commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        // Legacy callers may still send project fields. Serde ignores them and
        // the command resolves the global path unconditionally.
        let req: InstallRequest = serde_json::from_value(serde_json::json!({
            "server_name": "seeded",
            "transport": "stdio",
            "scope": "project",
            "project_dir": "/tmp/must-not-be-used",
            "agents": ["claude-code"],
            "overrides": {}
        }))
        .unwrap();
        let plan = tauri::async_runtime::block_on(preview_install(req)).unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].agent, "claude-code");
        assert!(plan[0].file_path.ends_with(".claude.json"));
        assert!(!plan[0].file_path.contains("must-not-be-used"));
        assert!(plan[0].config_json.contains("command"));
    }

    #[test]
    fn customized_comparison_uses_partial_eq() {
        // 验证 customized 比较逻辑：base != scanned.config → customized=true
        let base = McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into(), "server".into()]),
            env: None,
            cwd: None,
        });
        let same = McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into(), "server".into()]),
            env: None,
            cwd: None,
        });
        let modified = McpConfig::Stdio(StdioConfig {
            command: "npx".into(),
            args: Some(vec!["-y".into(), "server".into()]),
            env: Some(std::collections::HashMap::from([(
                "KEY".into(),
                "val".into(),
            )])),
            cwd: None,
        });
        // 未修改 → customized = false
        assert!(!(base != same));
        // 已修改 → customized = true
        assert!(base != modified);
    }
}
