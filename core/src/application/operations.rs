//! One plan/commit/cancel entry point for every resource domain.

use crate::domain::assets::{
    AssetCommitRequest, AssetOperationPlan, ConsumptionInventory, PlanClearAgentMcpRequest,
    PlanClearAgentModelsRequest,
    PlanConvergeConsumptionRequest, PlanDeleteCentralAssetRequest,
    PlanEnsureAgentConsumptionRequest, PlanRemoveAgentConsumptionRequest,
    PlanSetActiveModelRequest, PlanSetAgentConsumptionRequest, PlanSetAllMcpEnabledRequest,
    PlanSetAssetConsumersRequest, PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest,
    PlanSetSkillEnabledRequest, PlanUpdateAgentCapabilitiesRequest,
    PlanUpdateAssetConsumersRequest, PlanUpdateCentralAssetRequest,
};
use crate::domain::error::CoreResult;
use crate::resources::skill::{
    OperationPlan as SkillOperationPlan, PlanAssignmentRequest, PlanRemoveRequest,
    PlanRepairRequest, PlanSkillAssetImportRequest, PlanSkillAssetInstallRequest,
    PlanUpdateRequest, SkillCommitRequest, SkillOperationKind, SkillsInventory,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "operation", content = "request", rename_all = "snake_case")]
pub enum PlanOperationRequest {
    ConvergeConsumption(PlanConvergeConsumptionRequest),
    UpdateCentralAsset(PlanUpdateCentralAssetRequest),
    DeleteCentralAsset(PlanDeleteCentralAssetRequest),
    SetAgentConsumption(PlanSetAgentConsumptionRequest),
    EnsureAgentConsumption(PlanEnsureAgentConsumptionRequest),
    RemoveAgentConsumption(PlanRemoveAgentConsumptionRequest),
    ClearAgentMcp(PlanClearAgentMcpRequest),
    ClearAgentModels(PlanClearAgentModelsRequest),
    SetAssetConsumers(PlanSetAssetConsumersRequest),
    UpdateAssetConsumers(PlanUpdateAssetConsumersRequest),
    SetMcpEnabled(PlanSetMcpEnabledRequest),
    SetAllMcpEnabled(PlanSetAllMcpEnabledRequest),
    SetSkillEnabled(PlanSetSkillEnabledRequest),
    SetModelEnabled(PlanSetModelEnabledRequest),
    SetActiveModel(PlanSetActiveModelRequest),
    UpdateAgentCapabilities(PlanUpdateAgentCapabilitiesRequest),
    InstallSkill(PlanSkillAssetInstallRequest),
    ImportSkill(PlanSkillAssetImportRequest),
    AssignSkill(PlanAssignmentRequest),
    UpdateSkill(PlanUpdateRequest),
    RemoveSkill(PlanRemoveRequest),
    RepairSkill(PlanRepairRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum OperationPlan {
    Asset { plan: Box<AssetOperationPlan> },
    Skill { plan: SkillOperationPlan },
}

impl OperationPlan {
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Asset { plan } => &plan.operation_id,
            Self::Skill { plan } => &plan.operation_id,
        }
    }

    pub fn has_changes(&self) -> bool {
        match self {
            Self::Asset { plan } => plan.has_changes(),
            // Skill lifecycle plans already carry an explicit validated intent.
            Self::Skill { .. } => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum CommitOperationRequest {
    Asset {
        request: AssetCommitRequest,
    },
    Skill {
        kind: SkillOperationKind,
        request: SkillCommitRequest,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum CancelOperationRequest {
    Asset { operation_id: String },
    Skill { operation_id: String },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "snake_case")]
pub enum OperationCommitResult {
    Asset {
        inventory: ConsumptionInventory,
        converged: bool,
    },
    Skill { inventory: SkillsInventory },
}

fn plan_operation_name(request: &PlanOperationRequest) -> &'static str {
    use PlanOperationRequest::*;
    match request {
        ConvergeConsumption(_) => "converge_consumption",
        UpdateCentralAsset(_) => "update_central_asset",
        DeleteCentralAsset(_) => "delete_central_asset",
        SetAgentConsumption(_) => "set_agent_consumption",
        EnsureAgentConsumption(_) => "ensure_agent_consumption",
        RemoveAgentConsumption(_) => "remove_agent_consumption",
        ClearAgentMcp(_) => "clear_agent_mcp",
        ClearAgentModels(_) => "clear_agent_models",
        SetAssetConsumers(_) => "set_asset_consumers",
        UpdateAssetConsumers(_) => "update_asset_consumers",
        SetMcpEnabled(_) => "set_mcp_enabled",
        SetAllMcpEnabled(_) => "set_all_mcp_enabled",
        SetSkillEnabled(_) => "set_skill_enabled",
        SetModelEnabled(_) => "set_model_enabled",
        SetActiveModel(_) => "set_active_model",
        UpdateAgentCapabilities(_) => "update_agent_capabilities",
        InstallSkill(_) => "install_skill",
        ImportSkill(_) => "import_skill",
        AssignSkill(_) => "assign_skill",
        UpdateSkill(_) => "update_skill",
        RemoveSkill(_) => "remove_skill",
        RepairSkill(_) => "repair_skill",
    }
}

pub fn plan(request: PlanOperationRequest) -> CoreResult<OperationPlan> {
    let started = std::time::Instant::now();
    let operation_name = plan_operation_name(&request);
    let outcome = plan_inner(request);
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &outcome {
        Ok(plan) => {
            let op_id = plan.operation_id().to_owned();
            crate::logging::set_current_operation_id(&op_id);
            if let OperationPlan::Asset { plan: asset_plan } = plan {
                if !asset_plan.can_commit {
                    crate::logging::LogEvent::warn(
                        "operations.plan.blocked",
                        format!("planned {operation_name} with blocking warnings"),
                    )
                    .code("operation_blocked")
                    .operation_id(&op_id)
                    .duration_ms(elapsed_ms)
                    .field("operation", operation_name)
                    .field("warnings_count", asset_plan.warnings.len() as u64)
                    .emit();
                } else {
                    crate::logging::LogEvent::info(
                        "operations.plan",
                        format!("planned {operation_name}"),
                    )
                    .operation_id(&op_id)
                    .duration_ms(elapsed_ms)
                    .field("operation", operation_name)
                    .field("has_changes", plan.has_changes())
                    .emit();
                }
            } else {
                crate::logging::LogEvent::info(
                    "operations.plan",
                    format!("planned {operation_name}"),
                )
                .operation_id(&op_id)
                .duration_ms(elapsed_ms)
                .field("operation", operation_name)
                .field("has_changes", plan.has_changes())
                .emit();
            }
        }
        Err(error) => {
            crate::logging::LogEvent::error("operations.plan", &error.message)
                .code(&error.code)
                .duration_ms(elapsed_ms)
                .field("operation", operation_name)
                .fields(error.details.clone())
                .emit();
        }
    }
    outcome
}

fn plan_inner(request: PlanOperationRequest) -> CoreResult<OperationPlan> {
    use PlanOperationRequest::*;

    let plan = match request {
        ConvergeConsumption(request) => return super::convergence::plan(request),
        UpdateCentralAsset(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_update_central_asset(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        DeleteCentralAsset(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_delete_central_asset(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetAgentConsumption(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_agent_consumption(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        EnsureAgentConsumption(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_ensure_agent_consumption(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        RemoveAgentConsumption(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_remove_agent_consumption(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        ClearAgentMcp(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_clear_agent_mcp(request).map_err(super::error::from_legacy)?,
            ),
        },
        ClearAgentModels(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_clear_agent_models(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetAssetConsumers(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_asset_consumers(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        UpdateAssetConsumers(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_update_asset_consumers(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetMcpEnabled(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_mcp_enabled(request).map_err(super::error::from_legacy)?,
            ),
        },
        SetAllMcpEnabled(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_all_mcp_enabled(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetSkillEnabled(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_skill_enabled(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetModelEnabled(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_model_enabled(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        SetActiveModel(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_set_active_model(request).map_err(super::error::from_legacy)?,
            ),
        },
        UpdateAgentCapabilities(request) => OperationPlan::Asset {
            plan: Box::new(
                super::assets::plan_update_agent_capabilities(request)
                    .map_err(super::error::from_legacy)?,
            ),
        },
        InstallSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_asset_install(request).map_err(super::error::from_skill)?,
        },
        ImportSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_asset_import(request).map_err(super::error::from_skill)?,
        },
        AssignSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_assignment(request).map_err(super::error::from_skill)?,
        },
        UpdateSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_update(request).map_err(super::error::from_skill)?,
        },
        RemoveSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_remove(request).map_err(super::error::from_skill)?,
        },
        RepairSkill(request) => OperationPlan::Skill {
            plan: super::skills::plan_repair(request).map_err(super::error::from_skill)?,
        },
    };
    Ok(plan)
}

pub fn commit(request: CommitOperationRequest) -> CoreResult<OperationCommitResult> {
    let started = std::time::Instant::now();
    let (domain, operation_id) = match &request {
        CommitOperationRequest::Asset { request } => ("asset", request.operation_id.clone()),
        CommitOperationRequest::Skill { request, .. } => ("skill", request.operation_id.clone()),
    };
    crate::logging::with_operation_scope(&operation_id, None, || {
        let outcome: CoreResult<OperationCommitResult> = match request {
            CommitOperationRequest::Asset { request } => {
                let op_id = request.operation_id.clone();
                let inventory = super::assets::commit_asset_operation(request)
                    .map_err(super::error::from_legacy)?;
                let converged = !inventory
                    .target_incidents
                    .iter()
                    .any(|incident| incident.operation_id == op_id);
                Ok(OperationCommitResult::Asset {
                    inventory,
                    converged,
                })
            }
            CommitOperationRequest::Skill { kind, request } => {
                let result = match kind {
                    SkillOperationKind::Install => super::skills::commit_install(request),
                    SkillOperationKind::Import => super::skills::commit_import(request),
                    SkillOperationKind::Update => super::skills::commit_update(request),
                    SkillOperationKind::Remove => super::skills::commit_remove(request),
                    SkillOperationKind::Assignment => super::skills::commit_assignment(request),
                    SkillOperationKind::Repair => super::skills::commit_repair(request),
                };
                Ok(OperationCommitResult::Skill {
                    inventory: result.map_err(super::error::from_skill)?,
                })
            }
        };
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &outcome {
            Ok(OperationCommitResult::Asset { converged, .. }) => {
                if *converged {
                    crate::logging::LogEvent::info("operations.commit", "committed asset operation")
                        .operation_id(&operation_id)
                        .duration_ms(elapsed_ms)
                        .field("domain", domain)
                        .field("converged", true)
                        .emit();
                } else {
                    crate::logging::LogEvent::warn(
                        "operations.commit.partial",
                        "central asset committed with target convergence incidents",
                    )
                    .code("target_convergence_failed")
                    .operation_id(&operation_id)
                    .duration_ms(elapsed_ms)
                    .field("domain", domain)
                    .field("converged", false)
                    .emit();
                }
            }
            Ok(OperationCommitResult::Skill { .. }) => {
                crate::logging::LogEvent::info("operations.commit", "committed skill operation")
                    .operation_id(&operation_id)
                    .duration_ms(elapsed_ms)
                    .field("domain", domain)
                    .emit();
            }
            Err(error) => {
                crate::logging::LogEvent::error("operations.commit", &error.message)
                    .code(&error.code)
                    .operation_id(&operation_id)
                    .duration_ms(elapsed_ms)
                    .field("domain", domain)
                    .fields(error.details.clone())
                    .emit();
            }
        }
        outcome
    })
}

pub fn cancel(request: CancelOperationRequest) -> CoreResult<()> {
    let (domain, operation_id) = match &request {
        CancelOperationRequest::Asset { operation_id } => ("asset", operation_id.clone()),
        CancelOperationRequest::Skill { operation_id } => ("skill", operation_id.clone()),
    };
    crate::logging::with_operation_scope(&operation_id, None, || {
        let outcome: CoreResult<()> = match request {
            CancelOperationRequest::Asset { operation_id } => {
                super::assets::cancel_asset_operation(&operation_id).map_err(super::error::from_legacy)
            }
            CancelOperationRequest::Skill { operation_id } => {
                super::skills::cancel_operation(&operation_id).map_err(super::error::from_skill)
            }
        };
        if let Err(ref error) = outcome {
            crate::logging::LogEvent::warn("operations.cancel", &error.message)
                .code(&error.code)
                .operation_id(&operation_id)
                .field("domain", domain)
                .emit();
        } else {
            crate::logging::LogEvent::info("operations.cancel", "cancelled operation")
                .operation_id(&operation_id)
                .field("domain", domain)
                .emit();
        }
        outcome
    })
}
