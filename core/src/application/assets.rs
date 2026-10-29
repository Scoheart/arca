//! Cross-domain central asset use cases.

use super::gate::CapabilityDomain;
pub use crate::assets::{
    CompatibilityReason, CompatibilityView, McpAdoptionCandidate, McpAdoptionStatus,
    ModelAdoptionCandidate, ModelAdoptionStatus, ModelCredentialKind, PlanMcpAdoptionRequest,
    PlanModelAdoptionRequest,
};
pub use crate::domain::assets::*;
pub use crate::assets::inventory::InventoryScope;

fn asset_capability(asset: &AssetRef) -> CapabilityDomain {
    match asset {
        AssetRef::Mcp { .. } => CapabilityDomain::Mcp,
        AssetRef::Model { .. } | AssetRef::ModelProvider { .. } => CapabilityDomain::Model,
        AssetRef::Skill { .. } => CapabilityDomain::Skill,
    }
}

fn draft_capability(draft: &CentralAssetDraft) -> CapabilityDomain {
    match draft {
        CentralAssetDraft::Mcp { .. } => CapabilityDomain::Mcp,
        CentralAssetDraft::Model { .. } | CentralAssetDraft::ModelProvider { .. } => {
            CapabilityDomain::Model
        }
    }
}

fn selection_capability(selection: &AgentConsumptionSelection) -> CapabilityDomain {
    match selection {
        AgentConsumptionSelection::Mcp { .. } => CapabilityDomain::Mcp,
        AgentConsumptionSelection::Model { .. } => CapabilityDomain::Model,
        AgentConsumptionSelection::Skill { .. } => CapabilityDomain::Skill,
    }
}

fn operation_capability(operation_id: &str) -> Result<Option<CapabilityDomain>, String> {
    let operation = crate::assets::planner::load_operation(operation_id)?;
    Ok(match operation.plan.domain_plan {
        DomainPlan::Mcp { .. } => Some(CapabilityDomain::Mcp),
        DomainPlan::Model { .. } => Some(CapabilityDomain::Model),
        DomainPlan::Skill { .. } => Some(CapabilityDomain::Skill),
        DomainPlan::AgentCapabilities { .. } => None,
    })
}

fn operation_capability_if_present(
    operation_id: &str,
) -> Result<Option<Option<CapabilityDomain>>, String> {
    match operation_capability(operation_id) {
        Ok(capability) => Ok(Some(capability)),
        Err(error) if error == crate::assets::planner::OPERATION_UNAVAILABLE_ERROR => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn compatibility_for(agent_id: &str, asset: &AssetRef) -> Result<CompatibilityView, String> {
    super::gate::read(|| crate::assets::compatibility_for(agent_id, asset))
}

/// Return the read-only cross-domain desired/observed projection. Storage
/// migration and Model pointer reconciliation belong to [`super::bootstrap`].
pub fn list_inventory() -> Result<ConsumptionInventory, String> {
    super::gate::read(crate::assets::list_consumption_inventory)
}

/// Query only the requested capabilities and physical Agent targets. Scoped
/// revisions are read tokens and must not be passed to full convergence plans.
pub fn list_inventory_scoped(scope: InventoryScope) -> Result<ConsumptionInventory, String> {
    super::gate::read(|| crate::assets::inventory::list_consumption_inventory_scoped(&scope))
}

/// Host package metadata is independent of central transactions and secrets.
/// Do not acquire the workspace gate: a pending credential prompt in a Model
/// reader or writer must not prevent read-only host package discovery.
pub fn list_native_resources(agent_id: &str) -> crate::domain::plugin::NativeResourceInventory {
    crate::assets::inventory::list_native_resources(agent_id)
}

/// Both projections share one Skill scan, but retain independent failures so a
/// broken Skill directory cannot hide otherwise readable MCP/Model relations.
#[derive(serde::Serialize)]
pub struct ResourceObservation {
    pub skills: crate::domain::error::CoreResult<crate::resources::skill::SkillsInventory>,
    pub relationships: crate::domain::error::CoreResult<ConsumptionInventory>,
}

pub fn observe_resources() -> ResourceObservation {
    super::gate::read(|| {
        let (skills, relationships) = crate::assets::inventory::list_inventory_with_skills();
        ResourceObservation {
            skills: skills.map_err(super::error::from_skill),
            relationships: relationships.map_err(super::error::from_legacy),
        }
    })
}

pub fn list_mcp_adoption_candidates() -> Result<Vec<McpAdoptionCandidate>, String> {
    super::gate::read(crate::assets::list_mcp_adoption_candidates)
}

pub fn list_model_adoption_candidates() -> Result<Vec<ModelAdoptionCandidate>, String> {
    super::gate::read(crate::assets::list_model_adoption_candidates)
}

pub fn plan_update_central_asset(
    request: PlanUpdateCentralAssetRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = draft_capability(&request.draft);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_update_central_asset(request)
    })
}

pub fn plan_delete_central_asset(
    request: PlanDeleteCentralAssetRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = asset_capability(&request.asset);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_delete_central_asset(request)
    })
}

pub fn plan_mcp_adoption(request: PlanMcpAdoptionRequest) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Mcp, "asset_plan", || {
        crate::assets::plan_mcp_adoption(request)
    })
}

pub fn plan_model_adoption(
    request: PlanModelAdoptionRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Model, "asset_plan", || {
        crate::assets::plan_model_adoption(request)
    })
}

pub fn plan_set_agent_consumption(
    request: PlanSetAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = selection_capability(&request.selection);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_set_agent_consumption(request)
    })
}

pub fn plan_ensure_agent_consumption(
    request: PlanEnsureAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = selection_capability(&request.selection);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_ensure_agent_consumption(request)
    })
}

pub fn plan_remove_agent_consumption(
    request: PlanRemoveAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = selection_capability(&request.selection);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_remove_agent_consumption(request)
    })
}

pub fn plan_clear_agent_mcp(
    request: PlanClearAgentMcpRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Mcp, "asset_plan", || {
        crate::assets::plan_clear_agent_mcp(request)
    })
}

pub fn plan_clear_agent_models(
    request: crate::domain::assets::PlanClearAgentModelsRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Model, "asset_plan", || {
        crate::assets::plan_clear_agent_models(request)
    })
}

pub fn plan_set_asset_consumers(
    request: PlanSetAssetConsumersRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = asset_capability(&request.asset);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_set_asset_consumers(request)
    })
}

pub fn plan_update_asset_consumers(
    request: PlanUpdateAssetConsumersRequest,
) -> Result<AssetOperationPlan, String> {
    let capability = asset_capability(&request.asset);
    super::gate::prepare_for(capability, "asset_plan", || {
        crate::assets::plan_update_asset_consumers(request)
    })
}

pub fn plan_set_mcp_enabled(
    request: PlanSetMcpEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Mcp, "asset_plan", || {
        crate::assets::plan_set_mcp_enabled(request)
    })
}

pub fn plan_set_all_mcp_enabled(
    request: crate::domain::assets::PlanSetAllMcpEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Mcp, "asset_plan", || {
        crate::assets::plan_set_all_mcp_enabled(request)
    })
}

pub fn plan_set_skill_enabled(
    request: PlanSetSkillEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Skill, "asset_plan", || {
        crate::assets::plan_set_skill_enabled(request)
    })
}

pub fn plan_reapply_mcp(request: PlanReapplyMcpRequest) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Mcp, "asset_plan", || {
        crate::assets::plan_reapply_mcp(request)
    })
}

pub fn plan_reapply_model(request: PlanReapplyModelRequest) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Model, "asset_plan", || {
        crate::assets::plan_reapply_model(request)
    })
}

pub fn plan_reapply_skill(request: PlanReapplySkillRequest) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Skill, "asset_plan", || {
        crate::assets::plan_reapply_skill(request)
    })
}

pub fn plan_adopt_observed_skill(agent_id: &str, name: &str) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Skill, "asset_plan", || {
        crate::assets::plan_adopt_observed_skill(agent_id, name)
    })
}

pub fn plan_set_model_enabled(
    request: PlanSetModelEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Model, "asset_plan", || {
        crate::assets::plan_set_model_enabled(request)
    })
}

pub fn plan_set_active_model(
    request: PlanSetActiveModelRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare_for(CapabilityDomain::Model, "asset_plan", || {
        crate::assets::plan_set_active_model(request)
    })
}

pub fn plan_update_agent_capabilities(
    request: PlanUpdateAgentCapabilitiesRequest,
) -> Result<AssetOperationPlan, String> {
    super::gate::prepare("asset_plan", || {
        crate::assets::plan_update_agent_capabilities(request)
    })
}

pub fn commit_asset_operation(request: AssetCommitRequest) -> Result<ConsumptionInventory, String> {
    let capability = operation_capability(&request.operation_id)?;
    match capability {
        Some(capability) => super::gate::mutate_for(capability, "asset_commit", || {
            if operation_capability(&request.operation_id)? != Some(capability) {
                return Err("asset_operation_stale: operation capability changed".into());
            }
            crate::assets::commit_asset_operation(request)
        }),
        None => super::gate::mutate("asset_commit", || {
            if operation_capability(&request.operation_id)?.is_some() {
                return Err("asset_operation_stale: operation capability changed".into());
            }
            crate::assets::commit_asset_operation(request)
        }),
    }
}

pub fn cancel_asset_operation(operation_id: &str) -> Result<(), String> {
    match operation_capability_if_present(operation_id)? {
        None => super::gate::mutate("asset_cancel", || {
            crate::assets::cancel_asset_operation(operation_id)
        }),
        Some(Some(capability)) => super::gate::mutate_for(capability, "asset_cancel", || {
            match operation_capability_if_present(operation_id)? {
                None => crate::assets::cancel_asset_operation(operation_id),
                Some(Some(current)) if current == capability => {
                    crate::assets::cancel_asset_operation(operation_id)
                }
                _ => Err("asset_operation_stale: operation capability changed".into()),
            }
        }),
        Some(None) => {
            super::gate::mutate("asset_cancel", || {
                match operation_capability_if_present(operation_id)? {
                    None | Some(None) => crate::assets::cancel_asset_operation(operation_id),
                    Some(Some(_)) => {
                        Err("asset_operation_stale: operation capability changed".into())
                    }
                }
            })
        }
    }
}

pub fn recover_pending_asset_operations() -> Result<Vec<String>, String> {
    super::gate::mutate(
        "asset_recovery",
        crate::assets::recover_pending_asset_operations,
    )
}

pub fn migrate_model_profiles_v2_if_needed() -> Result<bool, String> {
    super::gate::mutate_for(
        CapabilityDomain::Model,
        "model_profile_migration",
        crate::assets::migrate_model_profiles_v2_if_needed,
    )
}

/// Import callers consume names only; avoid building an unused inventory after each entry.
pub fn commit_mcp_import(request: AssetCommitRequest) -> Result<(), String> {
    super::gate::mutate_for(CapabilityDomain::Mcp, "asset_commit", || {
        if operation_capability(&request.operation_id)? != Some(CapabilityDomain::Mcp) {
            return Err("asset_operation_stale: expected an MCP import operation".into());
        }
        crate::assets::commit_asset_operation_without_inventory(request)
    })
}
