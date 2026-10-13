//! Stable domain contracts shared by core application services and frontends.

use crate::domain::agents::AgentConfigurationPatch;
use crate::domain::mcp::OverridePatch;
use crate::domain::types::{ModelProfile, ModelProviderConfig, RegistryEntry};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Stable reference to one central asset. The tagged representation is shared
/// by Rust, Tauri, and the desktop; validation happens while decoding so an
/// invalid identity never reaches a domain adapter.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "domain", rename_all = "kebab-case")]
pub enum AssetRef {
    Mcp { key: String },
    Model { profile_id: String },
    ModelProvider { provider_id: String },
    Skill { name: String },
}

#[derive(Deserialize)]
#[serde(tag = "domain", rename_all = "kebab-case", deny_unknown_fields)]
enum UncheckedAssetRef {
    Mcp { key: String },
    Model { profile_id: String },
    ModelProvider { provider_id: String },
    Skill { name: String },
}

impl AssetRef {
    pub fn validate(&self) -> Result<(), SelectionError> {
        match self {
            Self::Mcp { key } => validate_mcp_asset_key(key),
            Self::Model { profile_id } => validate_nonempty("profile_id", profile_id),
            Self::ModelProvider { provider_id } => validate_nonempty("provider_id", provider_id),
            Self::Skill { name } => validate_nonempty("name", name),
        }
    }
}

impl<'de> Deserialize<'de> for AssetRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let unchecked = UncheckedAssetRef::deserialize(deserializer)?;
        let asset = match unchecked {
            UncheckedAssetRef::Mcp { key } => Self::Mcp { key },
            UncheckedAssetRef::Model { profile_id } => Self::Model { profile_id },
            UncheckedAssetRef::ModelProvider { provider_id } => Self::ModelProvider { provider_id },
            UncheckedAssetRef::Skill { name } => Self::Skill { name },
        };
        asset.validate().map_err(serde::de::Error::custom)?;
        Ok(asset)
    }
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), SelectionError> {
    if value.trim().is_empty() {
        return Err(SelectionError::InvalidIdentity {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

pub fn validate_mcp_asset_key(key: &str) -> Result<(), SelectionError> {
    let Some((name, transport)) = key.rsplit_once("::") else {
        return Err(SelectionError::InvalidMcpAssetKey(key.to_string()));
    };
    if name.trim().is_empty() || !matches!(transport, "stdio" | "http") {
        return Err(SelectionError::InvalidMcpAssetKey(key.to_string()));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConsumptionStatus {
    Synced,
    ExternalAdded,
    ExternalChanged,
    ExternalRemoved,
    Unparseable,
    Ambiguous,
    Unsupported,
}

/// Ownership is independent from what currently exists in an Agent target.
/// An externally changed managed relationship remains managed until the user
/// explicitly adopts, restores, or detaches it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OwnershipState {
    Managed,
    External,
}

/// Explicit ways to converge one Agent/asset relationship. These actions are
/// projected by core so frontends never infer mutation authority from a reason
/// string or a visual status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ConvergenceAction {
    AdoptObserved,
    RestoreDesired,
    Detach,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum AssetCapability {
    Mcp,
    Model,
    Skill,
}

/// A capability-local read failure. These diagnostics never imply global
/// read-only mode.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityDiagnostic {
    pub capability: AssetCapability,
    pub code: String,
}

/// One unresolved physical projection. Incidents are scoped to a capability
/// and target rather than copied onto every asset stored in that target.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TargetIncident {
    pub id: String,
    pub operation_id: String,
    pub capability: AssetCapability,
    pub target_id: String,
    pub target_path: String,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
    pub code: String,
    pub retryable: bool,
}

/// Physical destination behind a Skill relationship. Several Agents may read
/// the same target, so the target identity is carried separately from the
/// projected per-Agent rows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumptionTarget {
    pub target_id: String,
    pub global_dir: String,
}

/// Read projection. `desired` and `observed` remain explicit so missing and
/// external states cannot disappear from the inventory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumptionView {
    pub agent_id: String,
    pub asset: AssetRef,
    pub ownership: OwnershipState,
    pub desired: bool,
    pub observed: bool,
    /// Domain-specific enabled state. Present for managed MCP and Skill
    /// consumptions and for external MCP observations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Enabled state read from the Agent target. Keeping this separate from
    /// `enabled` prevents an external toggle from being flattened into Arca's
    /// desired state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_enabled: Option<bool>,
    /// Whether this Model Profile is the Agent's current primary model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    /// Desired current/default state for a Model Profile. `active` remains the
    /// observed Agent state so drift is visible instead of being flattened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desired_active: Option<bool>,
    pub status: ConsumptionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Stable identity for the exact observation behind this row. It never
    /// contains credentials or raw configuration values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    #[serde(default)]
    pub available_actions: Vec<ConvergenceAction>,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ConsumptionTarget>,
}

/// Desired relationships and read-only external observations are separated so
/// callers cannot accidentally treat discovery as ownership.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ConsumptionInventory {
    /// Hash of the normalized desired/observed projection. Convergence plans
    /// bind to this revision and re-scan before committing.
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub consumptions: Vec<ConsumptionView>,
    #[serde(default)]
    pub external: Vec<ConsumptionView>,
    #[serde(default)]
    pub capability_errors: Vec<CapabilityDiagnostic>,
    #[serde(default)]
    pub target_incidents: Vec<TargetIncident>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanConvergeConsumptionRequest {
    pub agent_id: String,
    pub asset: AssetRef,
    pub action: ConvergenceAction,
    pub observed_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConsumptionRecord {
    pub asset_key: String,
    pub enabled: bool,
    #[serde(default)]
    pub overrides: OverridePatch,
}

/// Per-target lifecycle state for a desired managed Skill relationship.
///
/// The relationship itself remains in `skill_assignments` while disabled so
/// turning a Skill off never becomes an implicit removal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillConsumptionRecord {
    pub name: String,
    pub target_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ApiKeyDelivery {
    #[default]
    Auto,
    Env,
    Command,
    AgentStore,
    Plaintext,
}

fn plaintext_delivery() -> ApiKeyDelivery {
    ApiKeyDelivery::Plaintext
}

fn is_plaintext_delivery(value: &ApiKeyDelivery) -> bool {
    matches!(value, ApiKeyDelivery::Plaintext)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ModelCredentialPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_override: Option<crate::domain::types::ApiKeySource>,
    #[serde(default)]
    pub delivery: ApiKeyDelivery,
}

/// One Arca-owned Model Profile installed for an Agent. Installation and the
/// Agent's active/default model are intentionally separate: an Agent may keep
/// several enabled profiles while only one is current. Frontends expose the
/// current pointer as the primary on/off control; `enabled` remains lifecycle
/// availability rather than a claim that several Models are in use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelConsumptionRecord {
    pub profile_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub credential: ModelCredentialPolicy,
    /// RFC3339 timestamp updated only when this Profile becomes current. It is
    /// used to choose a deterministic fallback when the current Profile is
    /// disabled or removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_selected_at: Option<String>,
}

/// Complete desired Model state for one Agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAgentSelection {
    #[serde(default)]
    pub profiles: BTreeMap<String, ModelConsumptionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<String>,
    /// How this Agent receives Provider API Keys when a Model is added.
    /// Unset settings resolve to plaintext; the user can change it per Agent.
    #[serde(
        default = "plaintext_delivery",
        skip_serializing_if = "is_plaintext_delivery"
    )]
    pub default_delivery: ApiKeyDelivery,
}

impl Default for ModelAgentSelection {
    fn default() -> Self {
        Self {
            profiles: BTreeMap::new(),
            active_profile_id: None,
            default_delivery: ApiKeyDelivery::Plaintext,
        }
    }
}

impl ModelAgentSelection {
    pub fn delivery_for(&self, profile_id: &str) -> ApiKeyDelivery {
        self.profiles
            .get(profile_id)
            .map(|record| record.credential.delivery.clone())
            .unwrap_or_else(|| self.default_delivery.clone())
    }

    pub fn inherited_credential(&self) -> ModelCredentialPolicy {
        ModelCredentialPolicy {
            source_override: None,
            delivery: self.default_delivery.clone(),
        }
    }
}

impl ModelAgentSelection {
    /// Keep the current pointer valid, falling back to the most recently used
    /// enabled Profile and then to a stable installed Profile. A non-empty
    /// managed selection always retains one enabled current Profile.
    pub fn normalize_active(&mut self) {
        let active_available = self.active_profile_id.as_ref().is_some_and(|active| {
            self.profiles
                .get(active)
                .is_some_and(|record| record.enabled)
        });
        if active_available {
            return;
        }
        let fallback = self
            .profiles
            .values()
            .filter(|record| record.enabled)
            .max_by(|left, right| {
                left.last_selected_at
                    .cmp(&right.last_selected_at)
                    .then_with(|| right.profile_id.cmp(&left.profile_id))
            })
            .or_else(|| {
                self.profiles.values().max_by(|left, right| {
                    left.last_selected_at
                        .cmp(&right.last_selected_at)
                        .then_with(|| right.profile_id.cmp(&left.profile_id))
                })
            })
            .map(|record| record.profile_id.clone());
        self.active_profile_id = fallback.clone();
        if let Some(profile_id) = fallback {
            if let Some(record) = self.profiles.get_mut(&profile_id) {
                record.enabled = true;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AssetOperationKind {
    SetConsumption,
    ClearMcp,
    ClearModels,
    UpdateAsset,
    DeleteAsset,
    Adopt,
    UpdateConfiguration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CentralAssetAction {
    Create,
    Update,
    Delete,
}

/// Secret-free projection of the central half of an asset operation. Domain
/// payloads stay with their adapter and are never copied into the review plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CentralAssetChange {
    pub asset: AssetRef,
    pub action: CentralAssetAction,
    #[serde(default)]
    pub summary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RelationshipAction {
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelationshipChange {
    pub agent_id: String,
    pub asset: AssetRef,
    pub action: RelationshipAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelStateSnapshot {
    pub added: bool,
    pub enabled: bool,
    pub active: bool,
}

/// Review projection for Model's four user-visible states. Relationship
/// changes alone cannot explain enable/disable or current-model fallback.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelStateChange {
    pub agent_id: String,
    pub profile_id: String,
    pub before: ModelStateSnapshot,
    pub after: ModelStateSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_profile_id: Option<String>,
    pub reason: String,
}

/// Cross-domain review projection for an enabled-state transition. The
/// domain plans retain their authoritative shapes; this delta makes MCP,
/// Model, and Skill toggles equally visible to every frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsumptionStateChange {
    pub agent_id: String,
    pub asset: AssetRef,
    pub before_enabled: bool,
    pub after_enabled: bool,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ConsumptionTarget>,
}

/// Domain-specific desired sets remain typed. The common coordinator owns
/// lifecycle and review, not MCP/Model/Skill payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DomainPlan {
    Mcp {
        before: BTreeMap<String, Vec<String>>,
        after: BTreeMap<String, Vec<String>>,
    },
    Model {
        before: BTreeMap<String, ModelAgentSelection>,
        after: BTreeMap<String, ModelAgentSelection>,
    },
    Skill {
        before: BTreeMap<String, Vec<String>>,
        after: BTreeMap<String, Vec<String>>,
    },
    AgentCapabilities {
        agent_id: String,
        before: Box<AgentConfigurationPatch>,
        after: Box<AgentConfigurationPatch>,
        skills_before: BTreeMap<String, Vec<String>>,
        skills_after: BTreeMap<String, Vec<String>>,
        #[serde(default)]
        affected_agent_ids: Vec<String>,
        #[serde(default)]
        migrated_skill_names: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetOperationPlan {
    pub operation_id: String,
    pub kind: AssetOperationKind,
    pub domain_plan: DomainPlan,
    #[serde(default)]
    pub central_changes: Vec<CentralAssetChange>,
    #[serde(default)]
    pub relationship_changes: Vec<RelationshipChange>,
    #[serde(default)]
    pub model_state_changes: Vec<ModelStateChange>,
    #[serde(default)]
    pub consumption_state_changes: Vec<ConsumptionStateChange>,
    #[serde(default)]
    pub target_files: Vec<String>,
    #[serde(default)]
    pub affected_agent_ids: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub can_commit: bool,
    pub candidate_hash: String,
}

impl AssetOperationPlan {
    /// Whether committing this plan has a logical or explicitly reviewed
    /// physical effect. Frontends must not infer this from assignments alone.
    pub fn has_changes(&self) -> bool {
        if self.kind == AssetOperationKind::ClearMcp
            || (self.kind == AssetOperationKind::ClearModels && !self.target_files.is_empty())
        {
            return true;
        }
        if !self.central_changes.is_empty()
            || !self.relationship_changes.is_empty()
            || !self.model_state_changes.is_empty()
            || !self.consumption_state_changes.is_empty()
        {
            return true;
        }
        match &self.domain_plan {
            DomainPlan::Mcp { before, after } | DomainPlan::Skill { before, after } => before != after,
            DomainPlan::Model { before, after } => before != after,
            DomainPlan::AgentCapabilities { before, after, .. } => before != after,
        }
    }
}

/// Drafts are accepted only by the central asset workspaces. MCP configuration
/// may contain headers or environment values, and Model credentials are secret,
/// so the planner binds them by hash and keeps the values out of persisted plans.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "domain", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CentralAssetDraft {
    Mcp {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        existing_key: Option<String>,
        entry: Box<RegistryEntry>,
    },
    Model {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        existing_id: Option<String>,
        profile: Box<ModelProfile>,
        /// `None` keeps an existing credential, `Some("")` clears it, and a
        /// non-empty value replaces it. The value is never persisted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credential: Option<String>,
    },
    ModelProvider {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        existing_id: Option<String>,
        provider: Box<ModelProviderConfig>,
        /// `None` keeps the shared credential, `Some("")` clears every child
        /// Profile credential, and a non-empty value replaces all of them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credential: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdateCentralAssetRequest {
    pub draft: CentralAssetDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanDeleteCentralAssetRequest {
    pub asset: AssetRef,
    /// MCP Registry can contain several source copies of one stable asset key.
    /// Deletion is bound to the reviewed user-owned source copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetAgentConsumptionRequest {
    pub agent_id: String,
    pub selection: AgentConsumptionSelection,
}

/// Ensure the listed assets are desired by an Agent without replacing its
/// other current relationships.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanEnsureAgentConsumptionRequest {
    pub agent_id: String,
    pub selection: AgentConsumptionSelection,
}

/// Remove the listed assets from one Agent without replacing its other
/// current relationships.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanRemoveAgentConsumptionRequest {
    pub agent_id: String,
    pub selection: AgentConsumptionSelection,
}

/// Clear every MCP entry from one Agent target, including disabled snapshots
/// and externally-created entries, without deleting central MCP assets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanClearAgentMcpRequest {
    pub agent_id: String,
}

/// Clear the selected Agent's complete Model source according to its declared
/// storage authority without deleting central Model assets or credentials.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanClearAgentModelsRequest {
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetMcpEnabledRequest {
    pub agent_id: String,
    pub asset_key: String,
    pub enabled: bool,
}

/// Enable or disable every MCP relationship currently managed by Arca for one
/// Agent. External, unadopted entries are intentionally outside this request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetAllMcpEnabledRequest {
    pub agent_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetSkillEnabledRequest {
    pub agent_id: String,
    pub name: String,
    pub enabled: bool,
    /// Required to disambiguate multiple physical copies for one Agent/name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanReapplyMcpRequest {
    pub asset_key: String,
    pub scope: McpReapplyScope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum McpReapplyScope {
    Agent { agent_id: String },
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanReapplyModelRequest {
    pub agent_id: String,
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanReapplySkillRequest {
    pub agent_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetModelEnabledRequest {
    pub agent_id: String,
    pub profile_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetActiveModelRequest {
    pub agent_id: String,
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdateAgentCapabilitiesRequest {
    pub agent_id: String,
    pub patch: AgentConfigurationPatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanSetAssetConsumersRequest {
    pub asset: AssetRef,
    pub agent_ids: Vec<String>,
}

/// Apply an atomic delta to one asset's desired consumers.
///
/// Frontends should prefer this over read-modify-write with
/// [`PlanSetAssetConsumersRequest`] when the user intent is additive/removal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdateAssetConsumersRequest {
    pub asset: AssetRef,
    #[serde(default)]
    pub add_agent_ids: Vec<String>,
    #[serde(default)]
    pub remove_agent_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AssetCommitRequest {
    pub operation_id: String,
    pub candidate_hash: String,
}

impl McpConsumptionRecord {
    pub fn validate(&self) -> Result<(), SelectionError> {
        validate_mcp_asset_key(&self.asset_key)
    }
}

impl ModelConsumptionRecord {
    pub fn validate(&self) -> Result<(), SelectionError> {
        validate_nonempty("profile_id", &self.profile_id)
    }
}

/// Complete desired selection for one Agent and one domain. Empty selections
/// mean unassign. Model accepts several installed Profiles; the current Profile
/// is changed through the explicit active-model operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AgentConsumptionSelection {
    Mcp { asset_keys: Vec<String> },
    Model { profile_ids: Vec<String> },
    Skill { names: Vec<String> },
}

impl AgentConsumptionSelection {
    pub fn normalize(self) -> Result<Self, SelectionError> {
        match self {
            Self::Mcp { asset_keys } => {
                for key in &asset_keys {
                    validate_mcp_asset_key(key)?;
                }
                Ok(Self::Mcp {
                    asset_keys: dedup_sorted(asset_keys),
                })
            }
            Self::Model { profile_ids } => {
                for profile_id in &profile_ids {
                    validate_nonempty("profile_id", profile_id)?;
                }
                Ok(Self::Model {
                    profile_ids: dedup_sorted(profile_ids),
                })
            }
            Self::Skill { names } => {
                for name in &names {
                    validate_nonempty("name", name)?;
                }
                Ok(Self::Skill {
                    names: dedup_sorted(names),
                })
            }
        }
    }
}

fn dedup_sorted(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    InvalidMcpAssetKey(String),
    InvalidIdentity { field: &'static str, value: String },
}

impl fmt::Display for SelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMcpAssetKey(key) => write!(
                f,
                "invalid MCP asset key {key:?}; expected name::stdio or name::http"
            ),
            Self::InvalidIdentity { field, .. } => write!(f, "{field} must not be empty"),
        }
    }
}

impl std::error::Error for SelectionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn asset_refs_use_stable_tagged_json() {
        let cases = [
            (
                AssetRef::Mcp {
                    key: "github::stdio".into(),
                },
                json!({"domain":"mcp","key":"github::stdio"}),
            ),
            (
                AssetRef::Model {
                    profile_id: "work".into(),
                },
                json!({"domain":"model","profile_id":"work"}),
            ),
            (
                AssetRef::Skill {
                    name: "review-changes".into(),
                },
                json!({"domain":"skill","name":"review-changes"}),
            ),
        ];

        for (asset, expected) in cases {
            assert_eq!(serde_json::to_value(&asset).unwrap(), expected);
            assert_eq!(serde_json::from_value::<AssetRef>(expected).unwrap(), asset);
        }
    }

    #[test]
    fn asset_ref_decode_rejects_invalid_identities() {
        for invalid in [
            json!({"domain":"mcp","key":"github"}),
            json!({"domain":"mcp","key":"github::websocket"}),
            json!({"domain":"mcp","key":"::stdio"}),
            json!({"domain":"model","profile_id":"  "}),
            json!({"domain":"skill","name":""}),
        ] {
            assert!(serde_json::from_value::<AssetRef>(invalid).is_err());
        }
    }

    #[test]
    fn many_selections_are_deduplicated_and_sorted() {
        assert_eq!(
            AgentConsumptionSelection::Mcp {
                asset_keys: vec!["z::http".into(), "a::stdio".into(), "z::http".into()]
            }
            .normalize()
            .unwrap(),
            AgentConsumptionSelection::Mcp {
                asset_keys: vec!["a::stdio".into(), "z::http".into()]
            }
        );
        assert_eq!(
            AgentConsumptionSelection::Skill {
                names: vec!["z".into(), "a".into(), "z".into()]
            }
            .normalize()
            .unwrap(),
            AgentConsumptionSelection::Skill {
                names: vec!["a".into(), "z".into()]
            }
        );
    }

    #[test]
    fn model_selection_accepts_multiple_profiles_and_deduplicates() {
        assert_eq!(
            AgentConsumptionSelection::Model {
                profile_ids: vec!["work".into(), "personal".into(), "work".into()],
            }
            .normalize()
            .unwrap(),
            AgentConsumptionSelection::Model {
                profile_ids: vec!["personal".into(), "work".into()],
            }
        );
    }

    #[test]
    fn model_selection_promotes_one_installed_profile_when_none_are_enabled() {
        let mut selection = ModelAgentSelection {
            profiles: BTreeMap::from([
                (
                    "backup".into(),
                    ModelConsumptionRecord {
                        profile_id: "backup".into(),
                        enabled: false,
                        credential: Default::default(),
                        last_selected_at: None,
                    },
                ),
                (
                    "work".into(),
                    ModelConsumptionRecord {
                        profile_id: "work".into(),
                        enabled: false,
                        credential: Default::default(),
                        last_selected_at: Some("2026-07-24T00:00:00Z".into()),
                    },
                ),
            ]),
            active_profile_id: None,
            ..Default::default()
        };

        selection.normalize_active();

        assert_eq!(selection.active_profile_id.as_deref(), Some("work"));
        assert!(selection.profiles["work"].enabled);
        assert!(!selection.profiles["backup"].enabled);
    }

    #[test]
    fn legacy_model_consumption_defaults_to_safe_automatic_delivery() {
        let record: ModelConsumptionRecord = serde_json::from_value(json!({
            "profile_id": "work",
            "enabled": true
        }))
        .unwrap();

        assert_eq!(record.credential.source_override, None);
        assert_eq!(record.credential.delivery, ApiKeyDelivery::Auto);
        assert_eq!(
            serde_json::to_value(record).unwrap()["credential"]["delivery"],
            "auto"
        );
    }

    #[test]
    fn agent_selection_defaults_to_plaintext_delivery() {
        let selection: ModelAgentSelection = serde_json::from_value(json!({
            "profiles": {},
        }))
        .unwrap();
        assert_eq!(selection.default_delivery, ApiKeyDelivery::Plaintext);
        assert_eq!(selection.delivery_for("missing"), ApiKeyDelivery::Plaintext);
        let encoded = serde_json::to_value(&selection).unwrap();
        assert!(encoded.get("default_delivery").is_none());
    }
}
