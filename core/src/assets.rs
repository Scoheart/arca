//! Central asset application services.
//!
//! A central MCP, Model, or Skill owns its lifecycle independently. This
//! module projects the desired relationship and observed Agent state without
//! flattening the domain-specific asset formats into an untyped payload.

pub mod compatibility;
pub mod inventory;
pub mod lifecycle;
mod lease;
pub mod migration;
pub mod model_migration;
pub mod observation;
mod payload_hash;
pub mod planner;
pub(crate) mod store;
pub mod transaction;
pub mod types {
    pub use crate::domain::assets::*;
}

pub use compatibility::{compatibility_for, CompatibilityReason, CompatibilityView};
pub use inventory::{list_consumption_inventory, list_consumption_inventory_with_skills};
pub use lifecycle::{
    plan_delete_central_asset, plan_update_central_asset,
};
pub use migration::{
    list_mcp_adoption_candidates, plan_mcp_adoption, McpAdoptionCandidate, McpAdoptionStatus,
    PlanMcpAdoptionRequest,
};
pub use model_migration::{
    list_model_adoption_candidates, plan_model_adoption, ModelAdoptionCandidate,
    ModelAdoptionStatus, ModelCredentialKind, PlanModelAdoptionRequest,
};
pub use observation::{observation_watch_targets, ObservationDomain, ObservationWatchTarget};
pub use planner::{
    plan_adopt_observed_skill, plan_clear_agent_mcp, plan_clear_agent_models,
    plan_ensure_agent_consumption,
    plan_reapply_mcp, plan_reapply_model, plan_reapply_skill, plan_remove_agent_consumption,
    plan_set_active_model, plan_set_agent_consumption, plan_set_all_mcp_enabled,
    plan_set_asset_consumers, plan_set_mcp_enabled, plan_set_model_enabled, plan_set_skill_enabled,
    plan_update_agent_capabilities, plan_update_asset_consumers,
};
pub use transaction::{
    cancel_asset_operation, commit_asset_operation, recover_pending_asset_operations,
};
pub use types::{
    AgentConsumptionSelection, AssetCommitRequest, AssetOperationKind, AssetOperationPlan,
    AssetRef, CentralAssetAction, CentralAssetChange, CentralAssetDraft, ConsumptionInventory,
    ConsumptionStateChange, ConsumptionStatus, ConsumptionTarget, ConsumptionView,
    ConvergenceAction, DomainPlan, McpConsumptionRecord, McpReapplyScope, ModelAgentSelection,
    ModelConsumptionRecord, ModelStateChange, ModelStateSnapshot, OwnershipState,
    PlanClearAgentModelsRequest, PlanConvergeConsumptionRequest, PlanDeleteCentralAssetRequest,
    PlanEnsureAgentConsumptionRequest, PlanReapplyMcpRequest, PlanReapplyModelRequest,
    PlanReapplySkillRequest, PlanRemoveAgentConsumptionRequest, PlanSetActiveModelRequest,
    PlanSetAgentConsumptionRequest, PlanSetAllMcpEnabledRequest, PlanSetAssetConsumersRequest,
    PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest, PlanSetSkillEnabledRequest,
    PlanUpdateAgentCapabilitiesRequest, PlanUpdateAssetConsumersRequest,
    PlanUpdateCentralAssetRequest, RelationshipAction, RelationshipChange, SelectionError,
    SkillConsumptionRecord, TargetIncident,
};

pub(crate) use transaction::commit_asset_operation_without_inventory;
