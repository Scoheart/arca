//! Cross-domain asset planning.

use super::compatibility::{compatibility_for, require_mcp_selection_compatible};
use super::inventory::list_consumption_inventory;
use super::store::{AssetStateStore, StatePrecondition, StateSubject};
use super::types::{
    AgentConsumptionSelection, AssetOperationKind, AssetOperationPlan, AssetRef,
    CentralAssetChange, ConsumptionInventory, ConsumptionStateChange, ConsumptionStatus,
    ConsumptionTarget, DomainPlan, McpReapplyScope, ModelAgentSelection, ModelConsumptionRecord,
    ModelStateChange, ModelStateSnapshot, PlanClearAgentMcpRequest, PlanClearAgentModelsRequest,
    PlanEnsureAgentConsumptionRequest, PlanReapplyMcpRequest, PlanReapplyModelRequest,
    PlanReapplySkillRequest, PlanRemoveAgentConsumptionRequest, PlanSetActiveModelRequest,
    PlanSetAgentConsumptionRequest, PlanSetAllMcpEnabledRequest, PlanSetAssetConsumersRequest,
    PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest, PlanSetSkillEnabledRequest,
    PlanUpdateAgentCapabilitiesRequest, PlanUpdateAssetConsumersRequest, RelationshipAction,
    RelationshipChange,
};
use crate::agents::{
    builtin_agents, current_configuration_patch, load_agents, normalize_configuration_patch,
    AgentConfigurationPatch, SkillConfigurationPatch,
};
use crate::domain::agents::ModelStorageAuthority;
use crate::domain::skill::ManagedSkillRecord;
use crate::paths::{arca_dir, settings_file};
use crate::resources::mcp::scanner::{collapse_home, expand_tilde};
use crate::resources::skill::{
    assignment_reapply_state, canonical_skill_assignments, canonical_skill_target_path,
    declared_targets_for_agents, hash_tree, list_inventory as list_skills_inventory,
    list_inventory_for_settings, normalize_agent_selection, skill_agent_capability_for_settings,
    AssignmentReapplyState, InventoryState, SkillLocation, SkillsInventory,
};
use crate::safe_write::{
    capture_parent_directory, fingerprint_anchored_path_state, read_path_state_anchored,
};
use crate::settings::{load_settings_strict, AgentConfigPathOverride, Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const OPERATION_SCHEMA_VERSION: u32 = 3;
const LEGACY_OPERATION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PersistedAssetOperation {
    pub schema_version: u32,
    pub plan: AssetOperationPlan,
    /// v2 bound every operation to the complete settings file. Keep the field
    /// readable so an operation with durable rollback evidence from an older
    /// Arca can still recover safely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_hash: Option<String>,
    /// v3 binds only the semantic state actually reviewed by this operation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state_preconditions: Vec<StatePrecondition>,
    pub settings_target_hash: String,
    pub target_hashes: BTreeMap<String, String>,
    /// Canonical effective/source catalog projection used by MCP plans. This
    /// binds consumer writes to the exact central configuration that was
    /// reviewed, even when a source cache changes outside settings.json.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_catalog_hash: Option<String>,
    /// Canonical physical Skill target graph observed during review. Skill
    /// writers can affect every installed Agent sharing one target, so the
    /// reviewed operation must become stale when probes change that sharing
    /// graph even if settings and the target leaf itself are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_target_graph_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<LifecycleBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "domain", rename_all = "kebab-case")]
pub(crate) enum LifecycleBinding {
    McpClear {
        agent_id: String,
    },
    ModelClear {
        agent_id: String,
        storage_authority: ModelStorageAuthority,
        configured_count: usize,
        external_count: usize,
    },
    McpUpsert {
        key: String,
        draft_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_source_id: Option<String>,
    },
    McpAdopt {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        draft_hash: Option<String>,
        enabled: BTreeMap<String, bool>,
    },
    McpDelete {
        key: String,
        source_id: String,
        fallback_exists: bool,
        effective_before: bool,
    },
    McpEnabled {
        agent_id: String,
        asset_key: String,
        before: bool,
        after: bool,
    },
    McpEnabledBulk {
        agent_id: String,
        before: BTreeMap<String, bool>,
        after: bool,
    },
    SkillEnabled {
        agent_id: String,
        name: String,
        target_id: String,
        affected_agent_ids: Vec<String>,
        before: bool,
        after: bool,
    },
    McpReapply {
        key: String,
        agent_ids: Vec<String>,
    },
    ModelReapply {
        agent_id: String,
        profile_id: String,
        enabled: bool,
        active: bool,
        credential_present: bool,
    },
    SkillReapply {
        agent_id: String,
        name: String,
        target_id: String,
        affected_agent_ids: Vec<String>,
        enabled: bool,
    },
    SkillAdoptObserved {
        name: String,
        record: Box<ManagedSkillRecord>,
    },
    ModelUpsert {
        profile_id: String,
        draft_hash: String,
        credential_action: CredentialAction,
    },
    ModelProviderUpsert {
        provider_id: String,
        profile_ids: BTreeSet<String>,
        draft_hash: String,
        credential_action: CredentialAction,
    },
    ModelProviderDelete {
        provider_id: String,
    },
    ModelAdopt {
        profile_id: String,
        draft_hash: String,
        credential_action: CredentialAction,
    },
    ModelDelete {
        profile_id: String,
    },
    AgentCapabilities {
        agent_id: String,
        after: AgentConfigurationPatch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill_assignments_after: Option<BTreeMap<String, BTreeSet<String>>>,
        #[serde(default)]
        skill_migration: Vec<SkillMigrationEntry>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SkillMigrationEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub destination: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CredentialAction {
    Keep,
    Set,
    Clear,
}

pub fn plan_set_agent_consumption(
    request: PlanSetAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let selection = request
        .selection
        .normalize()
        .map_err(|error| error.to_string())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    plan_agent_consumption(request.agent_id, selection, &settings, None)
}

fn plan_agent_consumption(
    agent_id: String,
    selection: AgentConsumptionSelection,
    settings: &Settings,
    skill_inventory: Option<ConsumptionInventory>,
) -> Result<AssetOperationPlan, String> {
    let domain_plan = match selection {
        AgentConsumptionSelection::Mcp { asset_keys } => {
            validate_unique_mcp_names(&asset_keys)?;
            let before = current_mcp_selection(settings, &agent_id);
            require_mcp_selection_compatible(&agent_id, &asset_keys)?;
            DomainPlan::Mcp {
                before: BTreeMap::from([(agent_id.clone(), before)]),
                after: BTreeMap::from([(agent_id, asset_keys)]),
            }
        }
        AgentConsumptionSelection::Model { profile_ids } => {
            let before = settings.model_selection(&agent_id);
            for profile_id in profile_ids
                .iter()
                .filter(|profile_id| !before.profiles.contains_key(*profile_id))
            {
                require_compatible(
                    &agent_id,
                    &AssetRef::Model {
                        profile_id: profile_id.clone(),
                    },
                )?;
            }
            let desired: BTreeSet<_> = profile_ids.into_iter().collect();
            let mut after = before.clone();
            after
                .profiles
                .retain(|profile_id, _| desired.contains(profile_id));
            let inherited = after.inherited_credential();
            for profile_id in desired {
                after
                    .profiles
                    .entry(profile_id.clone())
                    .or_insert(ModelConsumptionRecord {
                        profile_id,
                        enabled: true,
                        credential: inherited.clone(),
                        last_selected_at: None,
                    });
            }
            crate::resources::model::normalize_model_selection(&agent_id, &mut after);
            validate_model_selection_contract(settings, &agent_id, &after)?;
            DomainPlan::Model {
                before: BTreeMap::from([(agent_id.clone(), before)]),
                after: BTreeMap::from([(agent_id, after)]),
            }
        }
        AgentConsumptionSelection::Skill { names } => {
            let inventory = match skill_inventory {
                Some(inventory) => inventory,
                None => list_consumption_inventory()?,
            };
            let before = skill_selection_from_settings(settings, &inventory, &agent_id)?;
            let domain_plan = collapse_unchanged_consumption_plan(skill_plan_for_agent(
                &inventory, settings, &agent_id, before, names,
            )?);
            return finalize_plan_with_inventory(
                AssetOperationKind::SetConsumption,
                domain_plan,
                Vec::new(),
                Vec::new(),
                None,
                &inventory,
            );
        }
    };
    finalize_plan(collapse_unchanged_consumption_plan(domain_plan))
}

pub fn plan_ensure_agent_consumption(
    request: PlanEnsureAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let requested = request
        .selection
        .normalize()
        .map_err(|error| error.to_string())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let mut skill_inventory = None;
    let selection = match requested {
        AgentConsumptionSelection::Mcp { asset_keys } => {
            let mut desired: BTreeSet<String> = current_mcp_selection(&settings, &request.agent_id)
                .into_iter()
                .collect();
            desired.extend(asset_keys);
            AgentConsumptionSelection::Mcp {
                asset_keys: desired.into_iter().collect(),
            }
        }
        AgentConsumptionSelection::Model { profile_ids } => {
            let mut desired: BTreeSet<String> = settings
                .model_selection(&request.agent_id)
                .profiles
                .into_keys()
                .collect();
            desired.extend(profile_ids);
            AgentConsumptionSelection::Model {
                profile_ids: desired.into_iter().collect(),
            }
        }
        AgentConsumptionSelection::Skill { names } => {
            let inventory = list_consumption_inventory()?;
            let mut desired: BTreeSet<String> =
                skill_selection_from_settings(&settings, &inventory, &request.agent_id)?
                    .into_iter()
                    .collect();
            desired.extend(names);
            skill_inventory = Some(inventory);
            AgentConsumptionSelection::Skill {
                names: desired.into_iter().collect(),
            }
        }
    };
    plan_agent_consumption(request.agent_id, selection, &settings, skill_inventory)
}

/// Plan an Agent-scoped removal from the current authoritative relationship
/// state. Keeping the read and subtraction inside core avoids a frontend
/// replacing relationships that were added after its last snapshot.
pub fn plan_remove_agent_consumption(
    request: PlanRemoveAgentConsumptionRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let requested = request
        .selection
        .normalize()
        .map_err(|error| error.to_string())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let mut skill_inventory = None;
    let selection = match requested {
        AgentConsumptionSelection::Mcp { asset_keys } => {
            let mut desired: BTreeSet<String> = current_mcp_selection(&settings, &request.agent_id)
                .into_iter()
                .collect();
            for asset_key in asset_keys {
                desired.remove(&asset_key);
            }
            AgentConsumptionSelection::Mcp {
                asset_keys: desired.into_iter().collect(),
            }
        }
        AgentConsumptionSelection::Model { profile_ids } => {
            let current = settings.model_selection(&request.agent_id);
            if !profile_ids
                .iter()
                .any(|profile_id| current.profiles.contains_key(profile_id))
            {
                // Removing an absent identity is a pure relationship no-op.
                // Observed Model drift is repaired only through the explicit
                // reapply lifecycle and must never enter this effect set.
                return finalize_plan(DomainPlan::Model {
                    before: BTreeMap::new(),
                    after: BTreeMap::new(),
                });
            }
            let mut desired: BTreeSet<String> = current.profiles.into_keys().collect();
            for profile_id in profile_ids {
                desired.remove(&profile_id);
            }
            AgentConsumptionSelection::Model {
                profile_ids: desired.into_iter().collect(),
            }
        }
        AgentConsumptionSelection::Skill { names } => {
            let inventory = list_consumption_inventory()?;
            let mut desired: BTreeSet<String> =
                skill_selection_from_settings(&settings, &inventory, &request.agent_id)?
                    .into_iter()
                    .collect();
            for name in names {
                desired.remove(&name);
            }
            skill_inventory = Some(inventory);
            AgentConsumptionSelection::Skill {
                names: desired.into_iter().collect(),
            }
        }
    };
    plan_agent_consumption(request.agent_id, selection, &settings, skill_inventory)
}

pub fn plan_clear_agent_mcp(
    request: PlanClearAgentMcpRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let agents = load_agents();
    let target = agents
        .get(&request.agent_id)
        .and_then(|agent| agent.global.clone())
        .ok_or_else(|| format!("unsupported MCP Agent: {}", request.agent_id))?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let before = current_mcp_selection(&settings, &request.agent_id);
    let inventory = list_consumption_inventory()?;
    let observed = inventory
        .consumptions
        .iter()
        .chain(inventory.external.iter())
        .any(|item| {
            item.agent_id == request.agent_id && matches!(item.asset, AssetRef::Mcp { .. })
        })
        || crate::resources::mcp::ops::agent_has_entries(&request.agent_id)?;
    let disabled = settings
        .disabled
        .as_ref()
        .and_then(|records| records.get(&request.agent_id))
        .is_some_and(|records| !records.is_empty());
    if before.is_empty() && !observed && !disabled {
        return finalize_plan(DomainPlan::Mcp {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    finalize_plan_with_inventory(
        AssetOperationKind::ClearMcp,
        DomainPlan::Mcp {
            before: BTreeMap::from([(request.agent_id.clone(), before)]),
            after: BTreeMap::from([(request.agent_id.clone(), Vec::new())]),
        },
        Vec::new(),
        vec![target],
        Some(LifecycleBinding::McpClear {
            agent_id: request.agent_id,
        }),
        &inventory,
    )
}

pub fn plan_clear_agent_models(
    request: PlanClearAgentModelsRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let capability = crate::resources::model::model_agent_capability(&request.agent_id)
        .ok_or_else(|| format!("unsupported Model Agent: {}", request.agent_id))?;
    if capability.storage_authority == ModelStorageAuthority::Guided {
        return Err(format!(
            "model_agent_guided: {} must be configured inside the Agent",
            request.agent_id
        ));
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let before = settings.model_selection(&request.agent_id);
    let mapped_count = before.profiles.len();
    let inventory = list_consumption_inventory()?;
    let managed_count = inventory
        .consumptions
        .iter()
        .filter(|item| {
            item.agent_id == request.agent_id
                && item.observed
                && matches!(&item.asset, AssetRef::Model { .. })
        })
        .count();
    let external_count = inventory
        .external
        .iter()
        .filter(|item| {
            item.agent_id == request.agent_id && matches!(&item.asset, AssetRef::Model { .. })
        })
        .count();
    let configured_count = match capability.storage_authority {
        ModelStorageAuthority::NativeRegistry => managed_count + external_count,
        ModelStorageAuthority::ArcaMapping => before.profiles.len(),
        ModelStorageAuthority::Guided => unreachable!(),
    };
    let domain_plan = DomainPlan::Model {
        before: BTreeMap::from([(request.agent_id.clone(), before)]),
        after: BTreeMap::from([(request.agent_id.clone(), ModelAgentSelection::default())]),
    };
    if configured_count == 0 && mapped_count == 0 {
        return finalize_plan_with_inventory(
            AssetOperationKind::ClearModels,
            domain_plan,
            Vec::new(),
            Vec::new(),
            None,
            &inventory,
        );
    }
    let target_files = if capability.storage_authority == ModelStorageAuthority::NativeRegistry {
        crate::resources::model::clear_all_configured_model_paths(&request.agent_id)?
    } else {
        Vec::new()
    };
    finalize_plan_with_inventory(
        AssetOperationKind::ClearModels,
        domain_plan,
        Vec::new(),
        target_files,
        Some(LifecycleBinding::ModelClear {
            agent_id: request.agent_id,
            storage_authority: capability.storage_authority,
            configured_count,
            external_count: if capability.storage_authority == ModelStorageAuthority::NativeRegistry
            {
                external_count
            } else {
                0
            },
        }),
        &inventory,
    )
}

pub fn plan_set_model_enabled(
    request: PlanSetModelEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let before = settings.model_selection(&request.agent_id);
    let mut after = before.clone();
    let record = after
        .profiles
        .get_mut(&request.profile_id)
        .ok_or_else(|| "model_consumption_missing: Model is not added to this Agent".to_string())?;
    if record.enabled == request.enabled {
        return finalize_plan(DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    if request.enabled {
        require_compatible(
            &request.agent_id,
            &AssetRef::Model {
                profile_id: request.profile_id.clone(),
            },
        )?;
    }
    if !request.enabled
        && before.active_profile_id.as_deref() == Some(request.profile_id.as_str())
        && !before
            .profiles
            .values()
            .any(|candidate| candidate.profile_id != request.profile_id && candidate.enabled)
    {
        return Err(
            "active_model_required: choose another current Model before disabling this one".into(),
        );
    }
    record.enabled = request.enabled;
    crate::resources::model::normalize_model_selection(&request.agent_id, &mut after);
    validate_model_selection_contract(&settings, &request.agent_id, &after)?;
    finalize_plan(DomainPlan::Model {
        before: BTreeMap::from([(request.agent_id.clone(), before)]),
        after: BTreeMap::from([(request.agent_id, after)]),
    })
}

/// Credential delivery changes preserve the independent enabled/current state.
pub(crate) fn plan_model_credential_delivery(
    agent_id: &str,
    profile_id: Option<&str>,
    delivery: crate::domain::assets::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(agent_id)?;
    require_enabled_agent(agent_id)?;
    if !crate::resources::model::credential::available_deliveries(agent_id).contains(&delivery) {
        return Err(format!("credential_delivery_unsupported: {agent_id} does not support {delivery:?}"));
    }
    if delivery == crate::domain::assets::ApiKeyDelivery::Plaintext && !confirm_plaintext {
        return Err("plaintext_confirmation_required: confirm the private plaintext Agent write".into());
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let before = settings.model_selection(agent_id);
    let mut after = before.clone();
    if let Some(profile_id) = profile_id {
        let record = after.profiles.get_mut(profile_id)
            .ok_or_else(|| format!("model_consumption_missing: {profile_id} is not added to {agent_id}"))?;
        record.credential.delivery = delivery;
    } else {
        after.default_delivery = delivery.clone();
        for record in after.profiles.values_mut() {
            record.credential.delivery = delivery.clone();
        }
    }
    validate_model_selection_contract(&settings, agent_id, &after)?;
    finalize_plan(DomainPlan::Model {
        before: BTreeMap::from([(agent_id.to_string(), before)]),
        after: BTreeMap::from([(agent_id.to_string(), after)]),
    })
}

pub fn plan_set_active_model(
    request: PlanSetActiveModelRequest,
) -> Result<AssetOperationPlan, String> {
    if !crate::resources::model::supports_global_model_selection(&request.agent_id) {
        return Err("model_selection_per_conversation: choose the model inside a Qoder Desktop conversation".into());
    }
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let before = settings.model_selection(&request.agent_id);
    let mut after = before.clone();
    validate_model_selection_contract(&settings, &request.agent_id, &after)?;
    if !before.profiles.contains_key(&request.profile_id) {
        return Err("model_consumption_missing: Model is not added to this Agent".into());
    }
    require_compatible(
        &request.agent_id,
        &AssetRef::Model {
            profile_id: request.profile_id.clone(),
        },
    )?;
    if before.active_profile_id.as_deref() == Some(request.profile_id.as_str()) {
        return finalize_plan(DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    after.active_profile_id = Some(request.profile_id.clone());
    if let Some(record) = after.profiles.get_mut(&request.profile_id) {
        record.enabled = true;
        record.last_selected_at =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }
    validate_model_selection_contract(&settings, &request.agent_id, &after)?;
    finalize_plan(DomainPlan::Model {
        before: BTreeMap::from([(request.agent_id.clone(), before)]),
        after: BTreeMap::from([(request.agent_id, after)]),
    })
}

/// Plan an MCP on/off transition without removing its desired relationship.
/// The unchanged DomainPlan keeps the asset assigned to the Agent; the bound
/// lifecycle mutation snapshots/restores the actual config during commit.
pub fn plan_set_mcp_enabled(
    request: PlanSetMcpEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    super::types::validate_mcp_asset_key(&request.asset_key).map_err(|error| error.to_string())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let records = settings
        .mcp_consumptions
        .as_ref()
        .and_then(|consumptions| consumptions.get(&request.agent_id))
        .ok_or_else(|| "mcp_consumption_missing: MCP is not assigned to this Agent".to_string())?;
    let record = records
        .get(&request.asset_key)
        .ok_or_else(|| "mcp_consumption_missing: MCP is not assigned to this Agent".to_string())?;
    if record.enabled == request.enabled {
        return finalize_plan(DomainPlan::Mcp {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    let selection: Vec<String> = records.keys().cloned().collect();
    let domain_plan = DomainPlan::Mcp {
        before: BTreeMap::from([(request.agent_id.clone(), selection.clone())]),
        after: BTreeMap::from([(request.agent_id.clone(), selection)]),
    };
    finalize_plan_with(
        AssetOperationKind::SetConsumption,
        domain_plan,
        Vec::new(),
        Vec::new(),
        Some(LifecycleBinding::McpEnabled {
            agent_id: request.agent_id,
            asset_key: request.asset_key,
            before: record.enabled,
            after: request.enabled,
        }),
    )
}

/// Plan one target-scoped transition for every MCP currently managed by Arca
/// for an Agent. Keeping this as one lifecycle operation lets the transaction
/// layer roll the shared Agent file back as a unit if any entry cannot change.
pub fn plan_set_all_mcp_enabled(
    request: PlanSetAllMcpEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let records = settings
        .mcp_consumptions
        .as_ref()
        .and_then(|consumptions| consumptions.get(&request.agent_id))
        .ok_or_else(|| "mcp_consumption_missing: Agent has no managed MCPs".to_string())?;
    if records.is_empty() {
        return Err("mcp_consumption_missing: Agent has no managed MCPs".into());
    }
    let before = records
        .iter()
        .map(|(key, record)| (key.clone(), record.enabled))
        .collect::<BTreeMap<_, _>>();
    let selection = records.keys().cloned().collect::<Vec<_>>();
    let domain_plan = DomainPlan::Mcp {
        before: BTreeMap::from([(request.agent_id.clone(), selection.clone())]),
        after: BTreeMap::from([(request.agent_id.clone(), selection)]),
    };
    if before.values().all(|enabled| *enabled == request.enabled) {
        return finalize_plan(domain_plan);
    }
    finalize_plan_with(
        AssetOperationKind::SetConsumption,
        domain_plan,
        Vec::new(),
        Vec::new(),
        Some(LifecycleBinding::McpEnabledBulk {
            agent_id: request.agent_id,
            before,
            after: request.enabled,
        }),
    )
}

/// Plan a managed Skill on/off transition without removing its desired
/// assignment. A physical Skill target may be shared by several Agents, so the
/// plan carries every affected Agent and commits the target once.
pub fn plan_set_skill_enabled(
    request: PlanSetSkillEnabledRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    let inventory = list_consumption_inventory()?;
    let rows = inventory
        .consumptions
        .iter()
        .filter(|item| {
            item.agent_id == request.agent_id
                && item.desired
                && item.asset
                    == (AssetRef::Skill {
                        name: request.name.clone(),
                    })
        }).collect::<Vec<_>>();
    let row = if let Some(target_id) = &request.target_id {
        rows.iter().copied().find(|row| row.target.as_ref().is_some_and(|target| &target.target_id == target_id))
            .ok_or_else(|| "skill_consumption_missing: the selected Skill target is not assigned to this Agent".to_string())?
    } else {
        match rows.as_slice() {
            [row] => *row,
            [] => return Err("skill_consumption_missing: Skill is not assigned to this Agent".into()),
            _ => return Err("skill_consumption_ambiguous: choose the physical target shown in Skill status".into()),
        }
    };
    let before = row
        .enabled
        .ok_or_else(|| "skill_consumption_unmanaged: Skill cannot be toggled".to_string())?;
    if before == request.enabled {
        return finalize_plan_with_inventory(
            AssetOperationKind::SetConsumption,
            DomainPlan::Skill {
                before: BTreeMap::new(),
                after: BTreeMap::new(),
            },
            Vec::new(),
            Vec::new(),
            None,
            &inventory,
        );
    }
    if row.status != ConsumptionStatus::Synced {
        return Err(format!(
            "skill_observation_requires_convergence: {}",
            row.reason.as_deref().unwrap_or("unresolved_drift")
        ));
    }
    let target = row
        .target
        .as_ref()
        .ok_or_else(|| "skill_target_missing: Skill target is unavailable".to_string())?;
    let target_file = format!("{}/{}", target.global_dir, request.name);
    let target_id = target.target_id.clone();
    let mut affected_agent_ids = row.affected_agent_ids.clone();
    if affected_agent_ids.is_empty() {
        affected_agent_ids.push(request.agent_id.clone());
    }
    affected_agent_ids.sort();
    affected_agent_ids.dedup();
    let mut selections = BTreeMap::new();
    for agent_id in &affected_agent_ids {
        selections.insert(
            agent_id.clone(),
            skill_selection_from_inventory(&inventory, agent_id),
        );
    }
    let domain_plan = DomainPlan::Skill {
        before: selections.clone(),
        after: selections,
    };
    finalize_plan_with_inventory(
        AssetOperationKind::SetConsumption,
        domain_plan,
        Vec::new(),
        vec![target_file],
        Some(LifecycleBinding::SkillEnabled {
            agent_id: request.agent_id,
            name: request.name,
            target_id,
            affected_agent_ids,
            before,
            after: request.enabled,
        }),
        &inventory,
    )
}

pub fn plan_reapply_mcp(request: PlanReapplyMcpRequest) -> Result<AssetOperationPlan, String> {
    request
        .asset_key
        .rsplit_once("::")
        .filter(|(name, transport)| {
            !name.trim().is_empty() && matches!(*transport, "stdio" | "http")
        })
        .ok_or_else(|| "invalid MCP asset key".to_string())?;
    if !crate::resources::mcp::registry::read_registry()
        .iter()
        .any(|entry| entry.key() == request.asset_key)
    {
        return Err("asset_operation_stale: central MCP asset no longer exists".into());
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let inventory = list_consumption_inventory()?;
    let asset = AssetRef::Mcp {
        key: request.asset_key.clone(),
    };
    let desired_rows = inventory
        .consumptions
        .iter()
        .filter(|row| row.desired && row.asset == asset)
        .collect::<Vec<_>>();
    let agent_ids = match request.scope {
        McpReapplyScope::Agent { agent_id } => {
            validate_agent_id(&agent_id)?;
            require_enabled_agent(&agent_id)?;
            require_compatible(&agent_id, &asset)?;
            let row = desired_rows
                .iter()
                .find(|row| row.agent_id == agent_id)
                .ok_or_else(|| {
                    "mcp_consumption_missing: MCP is not assigned to this Agent".to_string()
                })?;
            if row.status == ConsumptionStatus::Synced {
                return finalize_plan_with_inventory(
                    AssetOperationKind::UpdateAsset,
                    DomainPlan::Mcp {
                        before: BTreeMap::new(),
                        after: BTreeMap::new(),
                    },
                    Vec::new(),
                    Vec::new(),
                    None,
                    &inventory,
                );
            }
            vec![agent_id]
        }
        McpReapplyScope::All => {
            for row in &desired_rows {
                require_enabled_agent(&row.agent_id)?;
                require_compatible(&row.agent_id, &asset)?;
            }
            let mut agent_ids = desired_rows
                .iter()
                .filter(|row| row.status != ConsumptionStatus::Synced)
                .map(|row| row.agent_id.clone())
                .collect::<Vec<_>>();
            agent_ids.sort();
            agent_ids.dedup();
            if agent_ids.is_empty() {
                return finalize_plan_with_inventory(
                    AssetOperationKind::UpdateAsset,
                    DomainPlan::Mcp {
                        before: BTreeMap::new(),
                        after: BTreeMap::new(),
                    },
                    Vec::new(),
                    Vec::new(),
                    None,
                    &inventory,
                );
            }
            agent_ids
        }
    };
    let consumers = agent_ids
        .iter()
        .map(|agent_id| (agent_id.clone(), current_mcp_selection(&settings, agent_id)))
        .collect::<BTreeMap<_, _>>();
    let consumer_count = agent_ids.len();
    let domain_plan = DomainPlan::Mcp {
        before: consumers.clone(),
        after: consumers,
    };
    finalize_plan_with_inventory(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Mcp {
                key: request.asset_key.clone(),
            },
            action: crate::domain::assets::CentralAssetAction::Update,
            summary: vec![
                "重新同步 MCP 配置".into(),
                format!("将更新 {consumer_count} 个已明确选择的 Agent"),
                "中央配置保持不变".into(),
            ],
        }],
        Vec::new(),
        Some(LifecycleBinding::McpReapply {
            key: request.asset_key,
            agent_ids,
        }),
        &inventory,
    )
}

pub fn plan_reapply_model(request: PlanReapplyModelRequest) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    AssetRef::Model {
        profile_id: request.profile_id.clone(),
    }
    .validate()
    .map_err(|error| error.to_string())?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if !settings
        .model_profiles
        .as_ref()
        .is_some_and(|profiles| profiles.contains_key(&request.profile_id))
    {
        return Err(format!("model_profile_missing: {}", request.profile_id));
    }
    let selection = settings.model_selection(&request.agent_id);
    let record = selection
        .profiles
        .get(&request.profile_id)
        .ok_or_else(|| "model_consumption_missing: Model is not added to this Agent".to_string())?;
    let enabled = record.enabled;
    if enabled {
        require_compatible(
            &request.agent_id,
            &AssetRef::Model {
                profile_id: request.profile_id.clone(),
            },
        )?;
    }
    let inventory = list_consumption_inventory()?;
    let row = inventory
        .consumptions
        .iter()
        .find(|row| {
            row.agent_id == request.agent_id
                && row.asset
                    == (AssetRef::Model {
                        profile_id: request.profile_id.clone(),
                    })
                && row.desired
        })
        .ok_or_else(|| "model_consumption_missing: Model is not added to this Agent".to_string())?;
    if row.status == ConsumptionStatus::Synced {
        return finalize_plan(DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    let active = selection.active_profile_id.as_deref() == Some(request.profile_id.as_str());
    let credential_present = crate::resources::model::credential_present(&request.profile_id);
    if row.active != row.desired_active && !active {
        let desired = selection.active_profile_id.as_deref().unwrap_or("<none>");
        return Err(format!(
            "model_reapply_active_profile_required: {0} is not the desired current Profile; reapply {desired} for Agent {1} to repair the current pointer",
            request.profile_id, request.agent_id
        ));
    }
    if !enabled {
        let profile = settings
            .model_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(&request.profile_id))
            .expect("central Profile existence was checked");
        match crate::resources::model::observe_profile_consumption(
            &request.agent_id,
            profile,
            false,
        )? {
            crate::resources::model::ModelObservedState::Synced
            | crate::resources::model::ModelObservedState::Missing => {}
            crate::resources::model::ModelObservedState::Drifted
            | crate::resources::model::ModelObservedState::Conflicted => {
                return Err(
                    "model_reapply_unsafe_clear: the disabled Profile contains customized or ambiguous fields; inspect it or unassign it to release Arca ownership without overwriting those fields"
                        .into(),
                )
            }
        }
    }
    let domain_plan = DomainPlan::Model {
        before: BTreeMap::from([(request.agent_id.clone(), selection.clone())]),
        after: BTreeMap::from([(request.agent_id.clone(), selection)]),
    };
    finalize_plan_with(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Model {
                profile_id: request.profile_id.clone(),
            },
            action: crate::domain::assets::CentralAssetAction::Update,
            summary: if enabled {
                vec![
                    format!("重新同步 Model Profile 到 Agent {}", request.agent_id),
                    "中央 Model Profile 保持不变".into(),
                ]
            } else {
                vec![
                    format!(
                        "移除 Agent {} 上已禁用 Profile 的物理配置",
                        request.agent_id
                    ),
                    "中央 Model Profile 与禁用关系保持不变".into(),
                ]
            },
        }],
        Vec::new(),
        Some(LifecycleBinding::ModelReapply {
            agent_id: request.agent_id,
            profile_id: request.profile_id,
            enabled,
            active,
            credential_present,
        }),
    )
}

pub fn plan_reapply_skill(request: PlanReapplySkillRequest) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    require_enabled_agent(&request.agent_id)?;
    AssetRef::Skill {
        name: request.name.clone(),
    }
    .validate()
    .map_err(|error| error.to_string())?;
    let inventory = list_consumption_inventory()?;
    let row = inventory
        .consumptions
        .iter()
        .find(|row| {
            row.agent_id == request.agent_id
                && row.desired
                && row.asset
                    == (AssetRef::Skill {
                        name: request.name.clone(),
                    })
        })
        .ok_or_else(|| {
            "skill_consumption_missing: Skill is not assigned to this Agent".to_string()
        })?;
    if row.status == ConsumptionStatus::Synced {
        return finalize_plan(DomainPlan::Skill {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        });
    }
    let enabled = row
        .enabled
        .ok_or_else(|| "skill_consumption_unmanaged: Skill cannot be reapplied".to_string())?;
    let target = row
        .target
        .as_ref()
        .ok_or_else(|| "skill_target_missing: Skill target is unavailable".to_string())?;
    let repairable_reason = matches!(
        (enabled, row.reason.as_deref()),
        (true, Some("skill_target_missing" | "skill_broken_link"))
            | (false, Some("skill_disabled_state_drift"))
    );
    if !repairable_reason {
        return Err(format!(
            "skill_reapply_target_unsafe: {}",
            row.reason.as_deref().unwrap_or("unresolved_drift")
        ));
    }
    match assignment_reapply_state(&request.name, &target.target_id) {
        Ok(AssignmentReapplyState::Synced) => {
            return finalize_plan(DomainPlan::Skill {
                before: BTreeMap::new(),
                after: BTreeMap::new(),
            });
        }
        Ok(AssignmentReapplyState::Repairable) => {}
        Err(_) => {
            return Err(
                "skill_reapply_target_unsafe: refusing to replace an unverified Skill target"
                    .into(),
            );
        }
    }

    let mut affected_agent_ids = row.affected_agent_ids.clone();
    if affected_agent_ids.is_empty() {
        affected_agent_ids.push(request.agent_id.clone());
    }
    affected_agent_ids.sort();
    affected_agent_ids.dedup();
    let selections = affected_agent_ids
        .iter()
        .map(|agent_id| {
            (
                agent_id.clone(),
                skill_selection_from_inventory(&inventory, agent_id),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let domain_plan = DomainPlan::Skill {
        before: selections.clone(),
        after: selections,
    };
    let target_file = format!("{}/{}", target.global_dir, request.name);
    finalize_plan_with_inventory(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Skill {
                name: request.name.clone(),
            },
            action: crate::domain::assets::CentralAssetAction::Update,
            summary: vec![
                format!("重新同步 Skill 到 Agent {}", request.agent_id),
                "中央 Skill 保持不变".into(),
            ],
        }],
        vec![target_file],
        Some(LifecycleBinding::SkillReapply {
            agent_id: request.agent_id,
            name: request.name,
            target_id: target.target_id.clone(),
            affected_agent_ids,
            enabled,
        }),
        &inventory,
    )
}

/// Accept the current bytes of a managed central Skill as the new desired
/// revision. Agent links are left untouched; only the audited central record
/// changes, so other relationships immediately observe the same revision.
pub fn plan_adopt_observed_skill(agent_id: &str, name: &str) -> Result<AssetOperationPlan, String> {
    validate_agent_id(agent_id)?;
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let skills = list_skills_inventory().map_err(|error| format!("{error:?}"))?;
    let item = skills
        .items
        .iter()
        .find(|item| item.name == name && matches!(item.location, SkillLocation::Central))
        .ok_or_else(|| "skill_observation_stale: central Skill is unavailable".to_string())?;
    if !item.states.contains(&InventoryState::LocallyModified) {
        return Err(
            "skill_observation_stale: central Skill is no longer externally changed".into(),
        );
    }
    let mut record = settings
        .managed_skills
        .as_ref()
        .and_then(|records| records.get(name))
        .cloned()
        .ok_or_else(|| "skill_observation_stale: managed Skill record is missing".to_string())?;
    record.description = item.description.clone();
    record.content_kind = item.content_kind.clone();
    record.content_hash = item
        .content_hash
        .clone()
        .ok_or_else(|| "skill_observation_stale: Skill content hash is unavailable".to_string())?;
    // Inventory observes content without repeatedly auditing every managed
    // tree. Adoption must audit the new bytes, never reuse the previous risk.
    let central = crate::resources::skill::SkillsPaths::resolve_from_env()
        .map_err(|error| format!("{error:?}"))?.central_skill(name);
    record.risk = crate::resources::skill::audit_skill(&central)
        .map_err(|error| format!("{error:?}"))?;
    if hash_tree(&central).map_err(|error| format!("{error:?}"))? != record.content_hash {
        return Err("skill_observation_stale: Skill content changed during adoption review".into());
    }
    record.updated_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    let inventory = list_consumption_inventory()?;
    let affected = inventory
        .consumptions
        .iter()
        .filter(|row| row.asset == (AssetRef::Skill { name: name.into() }) && row.desired)
        .map(|row| row.agent_id.clone())
        .collect::<BTreeSet<_>>();
    if !affected.contains(agent_id) {
        return Err("skill_observation_stale: selected Agent no longer consumes this Skill".into());
    }
    let selections = affected
        .iter()
        .map(|affected_agent_id| {
            (
                affected_agent_id.clone(),
                skill_selection_from_inventory(&inventory, affected_agent_id),
            )
        })
        .collect::<BTreeMap<_, _>>();
    finalize_plan_with_inventory(
        AssetOperationKind::Adopt,
        DomainPlan::Skill {
            before: selections.clone(),
            after: selections,
        },
        vec![CentralAssetChange {
            asset: AssetRef::Skill { name: name.into() },
            action: crate::domain::assets::CentralAssetAction::Update,
            summary: vec![
                "采用当前中央 Skill 内容作为新的 Arca 基线".into(),
                "保留所有 Agent 关系与链接".into(),
            ],
        }],
        Vec::new(),
        Some(LifecycleBinding::SkillAdoptObserved {
            name: name.into(),
            record: Box::new(record),
        }),
        &inventory,
    )
}

pub fn plan_update_agent_capabilities(
    request: PlanUpdateAgentCapabilitiesRequest,
) -> Result<AssetOperationPlan, String> {
    validate_agent_id(&request.agent_id)?;
    let before = current_configuration_patch(&request.agent_id)?;
    let normalized = normalize_configuration_patch(&request.agent_id, request.patch)?;
    let mut after = before.clone();
    if normalized.mcp.is_some() {
        after.mcp = normalized.mcp;
    }
    if normalized.model.is_some() {
        after.model = normalized.model;
    }
    if normalized.skill.is_some() {
        after.skill = normalized.skill;
    }
    if before == after {
        return Err("agent_configuration_unchanged: 配置没有变化".into());
    }

    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if before.mcp != after.mcp
        && settings
            .mcp_consumptions
            .as_ref()
            .and_then(|consumptions| consumptions.get(&request.agent_id))
            .is_some_and(|records| !records.is_empty())
    {
        return Err(
            "agent_configuration_in_use: MCP capability 已有 desired resources；当前版本不支持安全迁移 MCP writer 路径或 key"
                .into(),
        );
    }
    if before.model != after.model
        && !settings
            .model_selection(&request.agent_id)
            .profiles
            .is_empty()
    {
        return Err(
            "agent_configuration_in_use: Model capability 已有 desired resources；当前版本不支持安全迁移 Model writer 路径"
                .into(),
        );
    }
    let mut skills_before = BTreeMap::new();
    let mut skills_after = BTreeMap::new();
    let mut skill_assignments_after = None;
    let mut skill_migration = Vec::new();
    let mut target_files = Vec::new();
    let mut configuration_affected_agents = BTreeSet::from([request.agent_id.clone()]);

    if before.skill != after.skill {
        let before_skill = before
            .skill
            .as_ref()
            .ok_or_else(|| "skill_target_unavailable: 当前 Agent 没有 Skills 目标".to_string())?;
        let after_skill = after
            .skill
            .as_ref()
            .ok_or_else(|| "skill_target_unavailable: 新 Skills 目标不可用".to_string())?;
        let old_capability = skill_agent_capability_for_settings(&settings, &request.agent_id)
            .map_err(|error| format!("{error:?}"))?
            .ok_or_else(|| "skill_target_unavailable: 当前 Agent 没有 Skills 目标".to_string())?;
        let old_inventory =
            list_inventory_for_settings(&settings).map_err(|error| format!("{error:?}"))?;
        let mut prospective = settings.clone();
        set_prospective_skill_paths(&mut prospective, &request.agent_id, after_skill)?;
        prospective.skill_assignments = None;
        let new_capability = skill_agent_capability_for_settings(&prospective, &request.agent_id)
            .map_err(|error| format!("{error:?}"))?
            .ok_or_else(|| "skill_target_unavailable: 新 Skills 目标不可用".to_string())?;
        configuration_affected_agents.extend(old_capability.affected_agent_ids.iter().cloned());
        configuration_affected_agents.extend(new_capability.affected_agent_ids.iter().cloned());
        let prospective_targets =
            list_inventory_for_settings(&prospective).map_err(|error| format!("{error:?}"))?;

        let old_path = canonical_skill_target_path(&before_skill.global_dir)
            .map_err(|error| format!("{error:?}"))?;
        let new_path = canonical_skill_target_path(&after_skill.global_dir)
            .map_err(|error| format!("{error:?}"))?;
        if old_path != new_path {
            skill_migration = plan_skill_target_merge(
                &old_inventory,
                &old_capability.target_id,
                &new_capability.target_id,
                &after_skill.global_dir,
                &prospective_targets,
            )?;
            target_files.extend(
                skill_migration
                    .iter()
                    .filter(|entry| entry.source.is_some())
                    .map(|entry| entry.destination.clone()),
            );
        }

        let previous_assignments =
            canonical_skill_assignments(&settings).map_err(|error| format!("{error:?}"))?;
        let mut assignments = BTreeMap::new();
        for (name, target_ids) in &previous_assignments {
            let mut remapped = BTreeSet::new();
            for target_id in target_ids {
                let old_target = old_inventory
                    .targets
                    .iter()
                    .find(|target| &target.target_id == target_id)
                    .ok_or_else(|| {
                        "skill_target_unavailable: 现有 Skills 目标已不可用".to_string()
                    })?;
                let target_path = canonical_skill_target_path(&old_target.global_dir)
                    .map_err(|error| format!("{error:?}"))?;
                if target_id == &old_capability.target_id && old_path != new_path {
                    remapped.insert(new_capability.target_id.clone());
                }
                if let Some(target) = prospective_targets.targets.iter().find(|target| {
                    canonical_skill_target_path(&target.global_dir)
                        .ok()
                        .as_ref()
                        == Some(&target_path)
                }) {
                    remapped.insert(target.target_id.clone());
                } else if target_id != &old_capability.target_id || old_path == new_path {
                    return Err(format!(
                        "skill_path_migration_conflict: {name} 仍分配到即将移除的 Skills 目录"
                    ));
                }
            }
            if !remapped.is_empty() {
                assignments.insert(name.clone(), remapped);
            }
        }
        prospective.skill_assignments = (!assignments.is_empty()).then_some(assignments.clone());
        let after_inventory =
            list_inventory_for_settings(&prospective).map_err(|error| format!("{error:?}"))?;
        skills_before = projected_skill_relationships(&previous_assignments, &old_inventory);
        skills_after = projected_skill_relationships(&assignments, &after_inventory);
        skill_assignments_after = Some(assignments);
    }

    let domain_plan = DomainPlan::AgentCapabilities {
        agent_id: request.agent_id.clone(),
        before: Box::new(before),
        after: Box::new(after.clone()),
        skills_before,
        skills_after,
        affected_agent_ids: configuration_affected_agents.into_iter().collect(),
        migrated_skill_names: skill_migration
            .iter()
            .filter_map(|entry| {
                Path::new(&entry.destination)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect(),
    };
    finalize_plan_with(
        AssetOperationKind::UpdateConfiguration,
        domain_plan,
        Vec::new(),
        target_files,
        Some(LifecycleBinding::AgentCapabilities {
            agent_id: request.agent_id,
            after,
            skill_assignments_after,
            skill_migration,
        }),
    )
}

fn set_prospective_skill_paths(
    settings: &mut Settings,
    agent_id: &str,
    configuration: &SkillConfigurationPatch,
) -> Result<(), String> {
    let defaults = builtin_agents()
        .get(agent_id)
        .and_then(|definition| definition.skills.as_ref())
        .cloned();
    let global_dir = configuration.global_dir.as_str();
    let default_aliases = defaults
        .as_ref()
        .map(|capability| {
            capability
                .aliases
                .iter()
                .map(|alias| alias.global_dir.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let overrides = settings.agent_config_paths.get_or_insert_default();
    let entry = overrides.entry(agent_id.to_string()).or_default();
    entry.skills_global_dir = (defaults
        .as_ref()
        .map(|capability| capability.global_dir.as_str())
        != Some(global_dir))
    .then(|| global_dir.to_string());
    entry.skills_alias_dirs =
        (default_aliases != configuration.alias_dirs).then(|| configuration.alias_dirs.clone());
    if entry == &AgentConfigPathOverride::default() {
        overrides.remove(agent_id);
    }
    if overrides.is_empty() {
        settings.agent_config_paths = None;
    }
    Ok(())
}

fn plan_skill_target_merge(
    old_inventory: &SkillsInventory,
    old_target_id: &str,
    new_target_id: &str,
    new_global_dir: &str,
    new_inventory: &SkillsInventory,
) -> Result<Vec<SkillMigrationEntry>, String> {
    let old_items: Vec<_> = old_inventory
        .items
        .iter()
        .filter(|item| {
            matches!(
                &item.location,
                SkillLocation::AgentTarget { target_id, .. } if target_id == old_target_id
            )
        })
        .collect();
    let new_items: BTreeMap<_, _> = new_inventory
        .items
        .iter()
        .filter_map(|item| match &item.location {
            SkillLocation::AgentTarget { target_id, .. } if target_id == new_target_id => {
                Some((item.name.as_str(), item))
            }
            _ => None,
        })
        .collect();
    let mut migration = Vec::new();
    for old_item in old_items {
        validate_migration_name(old_inventory, &old_item.name)?;
        validate_migration_item(old_item)?;
        let old_source = item_physical_path(old_item)?;
        let old_hash = hash_tree(&old_source).map_err(|error| format!("{error:?}"))?;
        let destination = format!("{new_global_dir}/{}", old_item.name);
        if let Some(new_item) = new_items.get(old_item.name.as_str()) {
            validate_migration_name(new_inventory, &old_item.name)?;
            validate_migration_item(new_item)?;
            let new_source = item_physical_path(new_item)?;
            let new_hash = hash_tree(&new_source).map_err(|error| format!("{error:?}"))?;
            if new_hash != old_hash {
                return Err(format!(
                    "skill_path_migration_conflict: {} 在新旧目录内容不同",
                    old_item.name
                ));
            }
            migration.push(SkillMigrationEntry {
                source: None,
                destination,
                content_hash: old_hash,
            });
        } else {
            migration.push(SkillMigrationEntry {
                source: Some(collapse_home(&old_source.to_string_lossy())),
                destination,
                content_hash: old_hash,
            });
        }
    }
    migration.sort_by(|left, right| left.destination.cmp(&right.destination));
    Ok(migration)
}

fn validate_migration_name(inventory: &SkillsInventory, name: &str) -> Result<(), String> {
    for item in inventory.items.iter().filter(|item| item.name == name) {
        validate_migration_item(item)?;
    }
    Ok(())
}

fn validate_migration_item(
    item: &crate::resources::skill::SkillInventoryItem,
) -> Result<(), String> {
    let blocked = [
        InventoryState::BrokenLink,
        InventoryState::ConflictingLink,
        InventoryState::Missing,
        InventoryState::LocallyModified,
    ]
    .iter()
    .find(|state| item.states.contains(state));
    if let Some(state) = blocked {
        return Err(format!(
            "skill_path_migration_conflict: {} 当前状态为 {:?}",
            item.name, state
        ));
    }
    Ok(())
}

fn item_physical_path(
    item: &crate::resources::skill::SkillInventoryItem,
) -> Result<PathBuf, String> {
    let SkillLocation::AgentTarget { global_dir, .. } = &item.location else {
        return Err("skill_path_migration_conflict: Skill 不在 Agent 目标中".into());
    };
    let path = expand_tilde(&format!("{global_dir}/{}", item.name));
    let canonical = fs::canonicalize(path)
        .map_err(|_| format!("skill_path_migration_conflict: {} 无法安全读取", item.name))?;
    if !canonical.is_dir() {
        return Err(format!(
            "skill_path_migration_conflict: {} 不是目录",
            item.name
        ));
    }
    Ok(canonical)
}

fn projected_skill_relationships(
    assignments: &BTreeMap<String, BTreeSet<String>>,
    inventory: &SkillsInventory,
) -> BTreeMap<String, Vec<String>> {
    let targets: BTreeMap<_, _> = inventory
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect();
    let mut projected = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, target_ids) in assignments {
        for target_id in target_ids {
            let Some(target) = targets.get(target_id.as_str()) else {
                continue;
            };
            let agents = if target.affected_agent_ids.is_empty() {
                &target.primary_agent_ids
            } else {
                &target.affected_agent_ids
            };
            for agent_id in agents {
                projected
                    .entry(agent_id.clone())
                    .or_default()
                    .insert(name.clone());
            }
        }
    }
    projected
        .into_iter()
        .map(|(agent_id, names)| (agent_id, names.into_iter().collect()))
        .collect()
}

pub fn plan_set_asset_consumers(
    request: PlanSetAssetConsumersRequest,
) -> Result<AssetOperationPlan, String> {
    let inventory = list_consumption_inventory()?;
    plan_asset_consumers(request, inventory)
}

fn plan_asset_consumers(
    request: PlanSetAssetConsumersRequest,
    inventory: ConsumptionInventory,
) -> Result<AssetOperationPlan, String> {
    request
        .asset
        .validate()
        .map_err(|error| error.to_string())?;
    let selected: BTreeSet<String> = request.agent_ids.into_iter().collect();
    for agent_id in &selected {
        validate_agent_id(agent_id)?;
        require_compatible(agent_id, &request.asset)?;
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let current: BTreeSet<String> = inventory
        .consumptions
        .iter()
        .filter(|item| item.asset == request.asset && item.desired)
        .map(|item| item.agent_id.clone())
        .collect();
    let affected: BTreeSet<String> = current.union(&selected).cloned().collect();

    let domain_plan = match &request.asset {
        AssetRef::Mcp { key } => {
            let mut before = BTreeMap::new();
            let mut after = BTreeMap::new();
            for agent_id in affected {
                let existing = current_mcp_selection(&settings, &agent_id);
                let mut desired: BTreeSet<String> = existing.iter().cloned().collect();
                if selected.contains(&agent_id) {
                    desired.insert(key.clone());
                } else {
                    desired.remove(key);
                }
                validate_unique_mcp_names(&desired.iter().cloned().collect::<Vec<_>>())?;
                before.insert(agent_id.clone(), existing);
                after.insert(agent_id, desired.into_iter().collect());
            }
            DomainPlan::Mcp { before, after }
        }
        AssetRef::Model { profile_id } => {
            let mut before = BTreeMap::new();
            let mut after = BTreeMap::new();
            for agent_id in affected {
                let existing = settings.model_selection(&agent_id);
                let mut desired = existing.clone();
                if selected.contains(&agent_id) {
                    let inherited = desired.inherited_credential();
                    desired
                        .profiles
                        .entry(profile_id.clone())
                        .or_insert(ModelConsumptionRecord {
                            profile_id: profile_id.clone(),
                            enabled: true,
                            credential: inherited,
                            last_selected_at: None,
                        });
                } else {
                    desired.profiles.remove(profile_id);
                }
                crate::resources::model::normalize_model_selection(&agent_id, &mut desired);
                validate_model_selection_contract(&settings, &agent_id, &desired)?;
                before.insert(agent_id.clone(), existing);
                after.insert(agent_id, desired);
            }
            DomainPlan::Model { before, after }
        }
        AssetRef::ModelProvider { .. } => {
            return Err(
                "unsupported_asset_consumption: Providers are referenced by Models, not Agents"
                    .into(),
            )
        }
        AssetRef::Skill { name } => {
            validate_closed_skill_consumers(name, &selected)?;
            let mut before = BTreeMap::new();
            let mut after = BTreeMap::new();
            for agent_id in affected {
                let existing = skill_selection_from_inventory(&inventory, &agent_id);
                let mut desired: BTreeSet<String> = existing.iter().cloned().collect();
                if selected.contains(&agent_id) {
                    desired.insert(name.clone());
                } else {
                    desired.remove(name);
                }
                before.insert(agent_id.clone(), existing);
                after.insert(agent_id, desired.into_iter().collect());
            }
            DomainPlan::Skill { before, after }
        }
    };
    let domain_plan = collapse_unchanged_consumption_plan(domain_plan);
    finalize_plan_with_inventory(
        AssetOperationKind::SetConsumption,
        domain_plan,
        Vec::new(),
        Vec::new(),
        None,
        &inventory,
    )
}

fn collapse_unchanged_consumption_plan(plan: DomainPlan) -> DomainPlan {
    match plan {
        DomainPlan::Mcp { before, after } if before == after => DomainPlan::Mcp {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        },
        DomainPlan::Model { before, after } if before == after => DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        },
        DomainPlan::Skill { before, after } if before == after => DomainPlan::Skill {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        },
        other => other,
    }
}

/// Plan an additive/removal consumer change from the current authoritative
/// relationship state. Keeping the read and plan inside one core call avoids
/// frontends accidentally replacing a concurrent consumer with a stale full
/// selection.
pub fn plan_update_asset_consumers(
    request: PlanUpdateAssetConsumersRequest,
) -> Result<AssetOperationPlan, String> {
    request
        .asset
        .validate()
        .map_err(|error| error.to_string())?;
    let additions: BTreeSet<String> = request.add_agent_ids.into_iter().collect();
    let removals: BTreeSet<String> = request.remove_agent_ids.into_iter().collect();
    if additions.is_empty() && removals.is_empty() {
        return Err("asset_consumer_delta_empty: at least one consumer change is required".into());
    }
    if let Some(agent_id) = additions.intersection(&removals).next() {
        return Err(format!(
            "asset_consumer_delta_conflict: Agent cannot be both added and removed: {agent_id}"
        ));
    }
    for agent_id in additions.union(&removals) {
        validate_agent_id(agent_id)?;
    }

    let inventory = list_consumption_inventory()?;
    let mut selected: BTreeSet<String> = inventory
        .consumptions
        .iter()
        .filter(|item| item.asset == request.asset && item.desired)
        .map(|item| item.agent_id.clone())
        .collect();
    selected.extend(additions);
    for agent_id in removals {
        selected.remove(&agent_id);
    }
    plan_asset_consumers(
        PlanSetAssetConsumersRequest {
            asset: request.asset,
            agent_ids: selected.into_iter().collect(),
        },
        inventory,
    )
}

pub(crate) fn validate_model_selection_contract(
    settings: &crate::settings::Settings,
    agent_id: &str,
    selection: &crate::domain::assets::ModelAgentSelection,
) -> Result<(), String> {
    let capability = crate::resources::model::model_agent_capability(agent_id)
        .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    if !capability.supports_multiple && selection.profiles.len() > 1 {
        return Err(format!(
            "model_single_profile_only: {} supports one Arca Model Profile",
            capability.name
        ));
    }
    let mut identities = BTreeMap::<String, String>::new();
    for (profile_id, record) in &selection.profiles {
        if !record.enabled {
            continue;
        }
        let profile = settings
            .model_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(profile_id))
            .ok_or_else(|| format!("model_profile_missing: {profile_id}"))?;
        let identity = match agent_id {
            "qwen-code" => format!(
                "{}::{}",
                match profile.protocol {
                    crate::domain::types::ModelProtocol::AnthropicMessages => "anthropic",
                    _ => "openai",
                },
                profile.model
            ),
            "factory-droid" => profile.model.clone(),
            _ => profile_id.clone(),
        };
        if let Some(existing) = identities.insert(identity.clone(), profile_id.clone()) {
            return Err(format!(
                "model_native_identity_conflict: {agent_id} cannot distinguish Profiles {existing} and {profile_id} ({identity})"
            ));
        }
    }
    Ok(())
}

fn validate_unique_mcp_names(keys: &[String]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for key in keys {
        let name = key
            .rsplit_once("::")
            .map(|(name, _)| name)
            .ok_or_else(|| format!("invalid MCP asset key: {key}"))?;
        if !names.insert(name) {
            return Err(format!(
                "mcp_identity_conflict: one Agent cannot consume two transport variants named {name}"
            ));
        }
    }
    Ok(())
}

fn skill_plan_for_agent(
    inventory: &ConsumptionInventory,
    settings: &Settings,
    agent_id: &str,
    current: Vec<String>,
    desired: Vec<String>,
) -> Result<DomainPlan, String> {
    let current_set: BTreeSet<String> = current.iter().cloned().collect();
    let desired_set: BTreeSet<String> = desired.iter().cloned().collect();
    let changed: Vec<String> = current_set
        .symmetric_difference(&desired_set)
        .cloned()
        .collect();
    let mut affected = BTreeSet::from([agent_id.to_string()]);
    for name in &changed {
        if desired_set.contains(name) {
            let compatibility =
                compatibility_for(agent_id, &AssetRef::Skill { name: name.clone() })?;
            if !compatibility.compatible {
                let reason = compatibility
                    .reason
                    .expect("incompatible Skill view has reason");
                return Err(format!("{}: {}", reason.code, reason.message));
            }
            affected.extend(compatibility.affected_agent_ids);
        } else {
            // Removal is authorized by the persisted relationship, not by
            // current central content or an install probe. This keeps stale
            // ownership releasable after either disappears while preserving
            // the complete physical-target closure.
            affected.extend(skill_removal_affected_agents(
                inventory, settings, agent_id, name,
            )?);
        }
    }
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for affected_agent in affected {
        let existing = skill_selection_from_settings(settings, inventory, &affected_agent)?;
        let mut next: BTreeSet<String> = existing.iter().cloned().collect();
        for name in &changed {
            if desired_set.contains(name) {
                next.insert(name.clone());
            } else {
                next.remove(name);
            }
        }
        before.insert(affected_agent.clone(), existing);
        after.insert(affected_agent, next.into_iter().collect());
    }
    Ok(DomainPlan::Skill { before, after })
}

fn skill_removal_affected_agents(
    inventory: &ConsumptionInventory,
    settings: &Settings,
    agent_id: &str,
    name: &str,
) -> Result<BTreeSet<String>, String> {
    let declared = declared_targets_for_agents(&[agent_id.to_string()])
        .map_err(|error| format!("{error:?}"))?;
    let assignments =
        canonical_skill_assignments(settings).map_err(|error| format!("{error:?}"))?;
    let assigned = assignments.get(name).cloned().unwrap_or_default();
    let target_ids: BTreeSet<String> = declared.intersection(&assigned).cloned().collect();
    let mut affected = BTreeSet::from([agent_id.to_string()]);

    for item in inventory.consumptions.iter().filter(|item| {
        item.desired
            && item.asset == (AssetRef::Skill { name: name.into() })
            && item
                .target
                .as_ref()
                .is_some_and(|target| target_ids.contains(&target.target_id))
    }) {
        affected.insert(item.agent_id.clone());
        affected.extend(item.affected_agent_ids.iter().cloned());
    }

    let skills = list_skills_inventory().map_err(|error| format!("{error:?}"))?;
    for target in skills
        .targets
        .iter()
        .filter(|target| target_ids.contains(&target.target_id))
    {
        if target.affected_agent_ids.is_empty() {
            affected.extend(target.primary_agent_ids.iter().cloned());
        } else {
            affected.extend(target.affected_agent_ids.iter().cloned());
        }
    }
    Ok(affected)
}

fn validate_closed_skill_consumers(name: &str, selected: &BTreeSet<String>) -> Result<(), String> {
    for agent_id in selected {
        let compatibility = compatibility_for(
            agent_id,
            &AssetRef::Skill {
                name: name.to_string(),
            },
        )?;
        let missing: Vec<_> = compatibility
            .affected_agent_ids
            .into_iter()
            .filter(|affected| !selected.contains(affected))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "skill_shared_target_conflict: {agent_id} shares one physical Skill target with {}",
                missing.join(", ")
            ));
        }
    }
    Ok(())
}

fn require_compatible(agent_id: &str, asset: &AssetRef) -> Result<(), String> {
    let view = compatibility_for(agent_id, asset)?;
    if view.compatible {
        Ok(())
    } else {
        let reason = view.reason.expect("incompatible view has reason");
        Err(format!("{}: {}", reason.code, reason.message))
    }
}

fn validate_agent_id(agent_id: &str) -> Result<(), String> {
    if agent_id.trim().is_empty() || !load_agents().contains_key(agent_id) {
        return Err(format!("unknown Agent: {agent_id}"));
    }
    Ok(())
}

fn require_enabled_agent(agent_id: &str) -> Result<(), String> {
    let agents = load_agents();
    let agent = agents
        .get(agent_id)
        .ok_or_else(|| format!("unknown Agent: {agent_id}"))?;
    if agent.enabled {
        Ok(())
    } else {
        Err(format!(
            "agent_disabled: Agent {agent_id} is disabled in Arca"
        ))
    }
}

fn current_mcp_selection(settings: &Settings, agent_id: &str) -> Vec<String> {
    settings
        .mcp_consumptions
        .as_ref()
        .and_then(|consumptions| consumptions.get(agent_id))
        .map(|records| records.keys().cloned().collect())
        .unwrap_or_default()
}

fn skill_selection_from_inventory(inventory: &ConsumptionInventory, agent_id: &str) -> Vec<String> {
    let names: BTreeSet<String> = inventory
        .consumptions
        .iter()
        .filter_map(|item| match &item.asset {
            AssetRef::Skill { name } if item.agent_id == agent_id && item.desired => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    names.into_iter().collect()
}

/// Resolve desired Skill ownership from the persisted physical-target graph.
/// The consumption projection intentionally follows live install probes, but
/// stale ownership must remain visible to an explicit unassign after an Agent
/// is uninstalled.
fn skill_selection_from_settings(
    settings: &Settings,
    inventory: &ConsumptionInventory,
    agent_id: &str,
) -> Result<Vec<String>, String> {
    if skill_agent_capability_for_settings(settings, agent_id)
        .map_err(|error| format!("{error:?}"))?
        .is_none()
    {
        return Ok(skill_selection_from_inventory(inventory, agent_id));
    }
    let declared = declared_targets_for_agents(&[agent_id.to_string()])
        .map_err(|error| format!("{error:?}"))?;
    let assignments =
        canonical_skill_assignments(settings).map_err(|error| format!("{error:?}"))?;
    Ok(assignments
        .into_iter()
        .filter_map(|(name, target_ids)| {
            target_ids
                .iter()
                .any(|target_id| declared.contains(target_id))
                .then_some(name)
        })
        .collect())
}

/// Shared MCP fields cannot satisfy conflicting per-Agent intentions. Reject
/// destructive partial plans before any write; the central consumer selector
/// can remove all owners together in one reviewed operation.
fn shared_mcp_conflicts(
    plan: &DomainPlan,
    lifecycle: Option<&LifecycleBinding>,
) -> Result<(Vec<String>, BTreeSet<StateSubject>), String> {
    let DomainPlan::Mcp { before, after } = plan else {
        return Ok((Vec::new(), BTreeSet::new()));
    };
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let definitions = load_agents();
    let mut conflicts = Vec::new();
    let mut subjects = BTreeSet::new();
    for agent_id in union_keys(before, after) {
        let Some(agent) = definitions.get(agent_id) else { continue; };
        let Some(path) = agent.global.as_deref() else { continue; };
        for (peer_id, peer) in &definitions {
            if peer_id == agent_id || peer.key != agent.key
                || !peer.global.as_deref().is_some_and(|other| expand_tilde(other) == expand_tilde(path)) {
                continue;
            }
            subjects.insert(StateSubject::AgentConsumption {
                capability: crate::domain::assets::AssetCapability::Mcp, agent_id: peer_id.clone(),
            });
            subjects.insert(StateSubject::AgentConfiguration {
                capability: crate::domain::assets::AssetCapability::Mcp, agent_id: peer_id.clone(),
            });
            let records = settings.mcp_consumptions.as_ref().and_then(|all| all.get(peer_id));
            for (key, record) in records.into_iter().flatten() {
                let peer_retains = after.get(peer_id).map_or(true, |keys| keys.contains(key));
                if !peer_retains { continue; }
                let removed = before.get(agent_id).is_some_and(|keys| keys.contains(key))
                    && !after.get(agent_id).is_some_and(|keys| keys.contains(key));
                let conflicts_with_peer = match lifecycle {
                    Some(LifecycleBinding::McpClear { agent_id: owner }) if owner == agent_id => true,
                    Some(LifecycleBinding::McpEnabled { agent_id: owner, asset_key, after, .. })
                        if owner == agent_id && asset_key == key => *after != record.enabled,
                    Some(LifecycleBinding::McpEnabledBulk { agent_id: owner, before, after })
                        if owner == agent_id && before.contains_key(key) => *after != record.enabled,
                    _ => removed && record.enabled,
                };
                if conflicts_with_peer {
                    conflicts.push(format!("shared_mcp_target_conflict: {agent_id} 与 {peer_id} 共用 {path}；{key} 仍被另一 Agent 使用。请在中央 MCP 的消费者选择器中统一解除消费。"));
                }
            }
        }
    }
    Ok((conflicts, subjects))
}

fn finalize_plan(domain_plan: DomainPlan) -> Result<AssetOperationPlan, String> {
    finalize_plan_with(
        AssetOperationKind::SetConsumption,
        domain_plan,
        Vec::new(),
        Vec::new(),
        None,
    )
}

pub(crate) fn finalize_plan_with(
    kind: AssetOperationKind,
    domain_plan: DomainPlan,
    central_changes: Vec<CentralAssetChange>,
    extra_target_files: Vec<String>,
    lifecycle: Option<LifecycleBinding>,
) -> Result<AssetOperationPlan, String> {
    let central_mcp_only = matches!(&domain_plan, DomainPlan::Mcp { before, after }
        if before.is_empty() && after.is_empty())
        && matches!(lifecycle.as_ref(), Some(LifecycleBinding::McpUpsert { .. }));
    let current_inventory = if central_mcp_only {
        ConsumptionInventory::default()
    } else {
        list_consumption_inventory()?
    };
    finalize_plan_with_inventory(
        kind,
        domain_plan,
        central_changes,
        extra_target_files,
        lifecycle,
        &current_inventory,
    )
}

fn finalize_plan_with_inventory(
    kind: AssetOperationKind,
    domain_plan: DomainPlan,
    central_changes: Vec<CentralAssetChange>,
    extra_target_files: Vec<String>,
    lifecycle: Option<LifecycleBinding>,
    current_inventory: &ConsumptionInventory,
) -> Result<AssetOperationPlan, String> {
    let relationship_changes = relationship_changes(&domain_plan);
    let model_state_changes = model_state_changes(&domain_plan);
    let consumption_state_changes =
        consumption_state_changes(&model_state_changes, lifecycle.as_ref(), current_inventory)?;
    let affected_agent_ids: Vec<String> = agents_for_plan(&domain_plan).into_iter().collect();
    let effects = effect_assets(
        &domain_plan,
        &central_changes,
        &relationship_changes,
        &consumption_state_changes,
        lifecycle.as_ref(),
    );
    let mut target_files = target_files(&domain_plan, lifecycle.as_ref(), &effects)?;
    target_files.extend(extra_target_files);
    target_files.sort();
    target_files.dedup();
    order_model_target_files(&domain_plan, &mut target_files)?;
    validate_transaction_targets(&domain_plan, &target_files)?;
    // A Provider save explicitly updates its own enabled child projections.
    // Existing owned-field differences are replaced by that desired state;
    // malformed/ambiguous targets and unrelated profiles remain blocked.
    let provider_replaces_owned_fields = |item: &super::types::ConsumptionView| {
        if let (Some(LifecycleBinding::ModelProviderUpsert { profile_ids, .. }),
                AssetRef::Model { profile_id }) = (lifecycle.as_ref(), &item.asset) {
            profile_ids.contains(profile_id)
                && item.status == ConsumptionStatus::ExternalChanged
                && item.reason.as_deref() == Some("model_owned_fields_drift")
        } else { false }
    };
    let observations: Vec<_> = current_inventory
        .consumptions
        .iter()
        .filter(|item| {
            effects.contains(&(item.agent_id.clone(), item.asset.clone()))
                && !provider_replaces_owned_fields(item)
                && matches!(
                    item.status,
                    ConsumptionStatus::ExternalChanged
                        | ConsumptionStatus::ExternalRemoved
                        | ConsumptionStatus::Unparseable
                        | ConsumptionStatus::Ambiguous
                        | ConsumptionStatus::Unsupported
                )
        })
        .map(consumption_warning)
        .collect();
    let relationship_removal = !relationship_changes.is_empty()
        && relationship_changes
            .iter()
            .all(|change| change.action == RelationshipAction::Remove);
    let explicit_restore = matches!(
        lifecycle.as_ref(),
        Some(
            LifecycleBinding::McpReapply { .. }
                | LifecycleBinding::ModelReapply { .. }
                | LifecycleBinding::SkillReapply { .. }
        )
    );
    let relation_target_write = matches!(
        kind,
        AssetOperationKind::SetConsumption
            | AssetOperationKind::ClearMcp
            | AssetOperationKind::ClearModels
    ) || (!central_changes.is_empty() && !effects.is_empty())
        || matches!(
            lifecycle.as_ref(),
            Some(
                LifecycleBinding::McpEnabled { .. }
                    | LifecycleBinding::McpEnabledBulk { .. }
                    | LifecycleBinding::SkillEnabled { .. }
                    | LifecycleBinding::McpReapply { .. }
                    | LifecycleBinding::ModelReapply { .. }
                    | LifecycleBinding::SkillReapply { .. }
            )
        );
    let mut blocked =
        if relation_target_write && !relationship_removal && kind != AssetOperationKind::Adopt {
            current_inventory
                .consumptions
                .iter()
                .filter(|item| {
                    effects.contains(&(item.agent_id.clone(), item.asset.clone()))
                        && !provider_replaces_owned_fields(item)
                        && if explicit_restore {
                            matches!(
                                item.status,
                                ConsumptionStatus::Unparseable | ConsumptionStatus::Ambiguous
                            ) || (item.status == ConsumptionStatus::Unsupported
                                && !unsupported_effect_is_safe_model_release(
                                    &item.agent_id,
                                    &item.asset,
                                    &relationship_changes,
                                    &consumption_state_changes,
                                    lifecycle.as_ref(),
                                ))
                        } else {
                            item.status != ConsumptionStatus::Synced
                                && !(item.status == ConsumptionStatus::Unsupported
                                    && unsupported_effect_is_safe_model_release(
                                        &item.agent_id,
                                        &item.asset,
                                        &relationship_changes,
                                        &consumption_state_changes,
                                        lifecycle.as_ref(),
                                    ))
                        }
                })
                .map(consumption_warning)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
    if relation_target_write && !relationship_removal && kind != AssetOperationKind::Adopt {
        for external in &current_inventory.external {
            if effects.iter().any(|(agent_id, asset)| {
                agent_id == &external.agent_id
                    && match (asset, &external.asset) {
                        (AssetRef::Mcp { key }, AssetRef::Mcp { key: observed }) => key == observed,
                        (AssetRef::Model { .. }, AssetRef::Model { .. }) => {
                            model_state_changes.iter().any(|change| {
                                change.agent_id == *agent_id
                                    && change.after.active
                                    && !change.before.active
                            })
                        }
                        (AssetRef::Skill { name }, AssetRef::Skill { name: observed }) => {
                            name == observed
                        }
                        _ => false,
                    }
            }) {
                blocked.push(consumption_warning(external));
            }
        }
    }
    let (shared_conflicts, shared_subjects) = shared_mcp_conflicts(&domain_plan, lifecycle.as_ref())?;
    blocked.extend(shared_conflicts);
    let mut removal_warnings = Vec::new();
    append_missing_skill_removal_warnings(&domain_plan, &effects, &mut removal_warnings)?;
    blocked.sort();
    blocked.dedup();
    let mut warnings = observations;
    append_model_credential_export_warning(&domain_plan, &mut warnings);
    if let Some(LifecycleBinding::ModelClear {
        storage_authority: ModelStorageAuthority::NativeRegistry,
        configured_count,
        external_count,
        ..
    }) = lifecycle.as_ref()
    {
        warnings.push(format!(
            "将删除 Agent 真实配置中的 {configured_count} 个 Model，其中 {external_count} 个是 external/手工配置"
        ));
    }
    warnings.extend(removal_warnings);
    warnings.extend(blocked.iter().cloned());
    warnings.sort();
    warnings.dedup();
    // External observations are normal state, never a global or domain gate.
    // A write that would replace one exact observation must be expressed as an
    // explicit convergence action. Releasing that relationship remains safe.
    let can_commit = blocked.is_empty();
    // Planning already persists a candidate under the Arca staging directory.
    // Materialize the stable Arca root before parent fingerprints are captured
    // so that persisting this very plan cannot make its own target hash stale.
    fs::create_dir_all(arca_dir()).map_err(|error| error.to_string())?;
    let mut subjects = state_subjects(&domain_plan, &central_changes, lifecycle.as_ref());
    subjects.extend(shared_subjects);
    let state_preconditions = AssetStateStore::load()?.capture(subjects)?;
    let settings_target_hash = hash_settings_target(&settings_file());
    let target_hashes = hash_targets(&target_files);
    let mut persisted = PersistedAssetOperation {
        schema_version: OPERATION_SCHEMA_VERSION,
        plan: AssetOperationPlan {
            operation_id: Uuid::new_v4().hyphenated().to_string(),
            kind,
            domain_plan,
            central_changes,
            relationship_changes,
            model_state_changes,
            consumption_state_changes,
            target_files,
            affected_agent_ids,
            warnings,
            can_commit,
            candidate_hash: String::new(),
        },
        settings_hash: None,
        state_preconditions,
        settings_target_hash,
        target_hashes,
        mcp_catalog_hash: None,
        skill_target_graph_hash: None,
        lifecycle,
    };
    persisted.plan.candidate_hash = candidate_hash(&persisted)?;
    let plan = persisted.plan.clone();
    persist_operation(&persisted)?;
    Ok(plan)
}

fn state_subjects(
    domain_plan: &DomainPlan,
    central_changes: &[CentralAssetChange],
    lifecycle: Option<&LifecycleBinding>,
) -> BTreeSet<StateSubject> {
    let mut subjects = BTreeSet::new();
    for change in central_changes {
        subjects.insert(StateSubject::CentralAsset {
            asset: change.asset.clone(),
        });
        subjects.insert(StateSubject::AssetConsumers {
            asset: change.asset.clone(),
        });
    }
    match domain_plan {
        DomainPlan::Mcp { before, after } => {
            for agent_id in before.keys().chain(after.keys()) {
                subjects.insert(StateSubject::AgentConsumption {
                    capability: crate::domain::assets::AssetCapability::Mcp,
                    agent_id: agent_id.clone(),
                });
                subjects.insert(StateSubject::AgentConfiguration {
                    capability: crate::domain::assets::AssetCapability::Mcp,
                    agent_id: agent_id.clone(),
                });
            }
            for key in before
                .values()
                .chain(after.values())
                .flatten()
                .collect::<BTreeSet<_>>()
            {
                subjects.insert(StateSubject::CentralAsset {
                    asset: AssetRef::Mcp { key: key.clone() },
                });
            }
        }
        DomainPlan::Model { before, after } => {
            for agent_id in before.keys().chain(after.keys()) {
                subjects.insert(StateSubject::AgentConsumption {
                    capability: crate::domain::assets::AssetCapability::Model,
                    agent_id: agent_id.clone(),
                });
                subjects.insert(StateSubject::AgentConfiguration {
                    capability: crate::domain::assets::AssetCapability::Model,
                    agent_id: agent_id.clone(),
                });
            }
            for profile_id in before
                .values()
                .chain(after.values())
                .flat_map(|selection| selection.profiles.keys())
                .collect::<BTreeSet<_>>()
            {
                subjects.insert(StateSubject::CentralAsset {
                    asset: AssetRef::Model {
                        profile_id: profile_id.clone(),
                    },
                });
            }
        }
        DomainPlan::Skill { before, after } => {
            for agent_id in before.keys().chain(after.keys()) {
                subjects.insert(StateSubject::AgentConsumption {
                    capability: crate::domain::assets::AssetCapability::Skill,
                    agent_id: agent_id.clone(),
                });
                subjects.insert(StateSubject::AgentConfiguration {
                    capability: crate::domain::assets::AssetCapability::Skill,
                    agent_id: agent_id.clone(),
                });
            }
            for name in before
                .values()
                .chain(after.values())
                .flatten()
                .collect::<BTreeSet<_>>()
            {
                subjects.insert(StateSubject::CentralAsset {
                    asset: AssetRef::Skill { name: name.clone() },
                });
            }
            subjects.insert(StateSubject::SkillTargetGraph);
        }
        DomainPlan::AgentCapabilities {
            agent_id,
            skills_before,
            skills_after,
            affected_agent_ids,
            ..
        } => {
            for capability in [
                crate::domain::assets::AssetCapability::Mcp,
                crate::domain::assets::AssetCapability::Model,
                crate::domain::assets::AssetCapability::Skill,
            ] {
                subjects.insert(StateSubject::AgentConfiguration {
                    capability,
                    agent_id: agent_id.clone(),
                });
                subjects.insert(StateSubject::AgentConsumption {
                    capability,
                    agent_id: agent_id.clone(),
                });
            }
            for affected in std::iter::once(agent_id).chain(affected_agent_ids) {
                subjects.insert(StateSubject::AgentConsumption {
                    capability: crate::domain::assets::AssetCapability::Skill,
                    agent_id: affected.clone(),
                });
            }
            for name in skills_before
                .values()
                .chain(skills_after.values())
                .flatten()
                .collect::<BTreeSet<_>>()
            {
                subjects.insert(StateSubject::CentralAsset {
                    asset: AssetRef::Skill { name: name.clone() },
                });
            }
            subjects.insert(StateSubject::SkillTargetGraph);
        }
    }
    match lifecycle {
        Some(LifecycleBinding::ModelUpsert { profile_id, .. })
        | Some(LifecycleBinding::ModelAdopt { profile_id, .. }) => {
            subjects.insert(StateSubject::ModelCatalog);
            subjects.insert(StateSubject::CredentialPresence {
                profile_id: profile_id.clone(),
            });
        }
        Some(LifecycleBinding::ModelProviderUpsert { provider_id, .. }) => {
            subjects.insert(StateSubject::CentralAsset {
                asset: AssetRef::ModelProvider {
                    provider_id: provider_id.clone(),
                },
            });
            subjects.insert(StateSubject::ModelProviderNames);
            subjects.insert(StateSubject::ProviderCredentialPresence {
                provider_id: provider_id.clone(),
            });
        }
        Some(LifecycleBinding::ModelProviderDelete { provider_id }) => {
            subjects.insert(StateSubject::CentralAsset {
                asset: AssetRef::ModelProvider {
                    provider_id: provider_id.clone(),
                },
            });
            subjects.insert(StateSubject::ProviderCredentialPresence {
                provider_id: provider_id.clone(),
            });
        }
        Some(LifecycleBinding::ModelDelete { profile_id })
        | Some(LifecycleBinding::ModelReapply { profile_id, .. }) => {
            subjects.insert(StateSubject::CredentialPresence {
                profile_id: profile_id.clone(),
            });
        }
        _ => {}
    }
    subjects
}

fn append_missing_skill_removal_warnings(
    plan: &DomainPlan,
    effects: &BTreeSet<(String, AssetRef)>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let missing_names: BTreeSet<String> = effects
        .iter()
        .filter_map(|(agent_id, asset)| match asset {
            AssetRef::Skill { name } if !asset_desired_after(plan, agent_id, asset) => {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    if missing_names.is_empty() {
        return Ok(());
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    for (agent_id, asset) in effects {
        let AssetRef::Skill { name } = asset else {
            continue;
        };
        if missing_names.contains(name) && !managed_skill_is_available(&settings, name) {
            warnings.push(format!(
                "{agent_id} / {}: skill_asset_missing",
                warning_asset_identity(asset)
            ));
        }
    }
    Ok(())
}

fn managed_skill_is_available(settings: &Settings, name: &str) -> bool {
    if !settings
        .managed_skills
        .as_ref()
        .is_some_and(|skills| skills.contains_key(name))
    {
        return false;
    }
    fs::symlink_metadata(crate::paths::skill_contents_dir().join(name))
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn validate_transaction_targets(plan: &DomainPlan, targets: &[String]) -> Result<(), String> {
    let skill_links_are_managed = matches!(plan, DomainPlan::Skill { .. });
    let skill_removal_only = skill_domain_is_removal_only(plan);
    for target in targets {
        let path = expand_tilde(target);
        capture_parent_directory(&path)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && !skill_removal_only =>
            {
                return Err(format!(
                    "asset_target_unsafe: refusing to use directory as a transaction file: {}",
                    path.display()
                ));
            }
            Ok(metadata) if metadata.file_type().is_symlink() && !skill_links_are_managed => {
                return Err(format!(
                    "asset_target_unsafe: symlink-backed configuration files require explicit migration: {}",
                    path.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "asset_target_unreadable: failed to inspect {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn skill_domain_is_removal_only(plan: &DomainPlan) -> bool {
    let DomainPlan::Skill { before, after } = plan else {
        return false;
    };
    let mut changed = false;
    for agent_id in union_keys(before, after) {
        let left: BTreeSet<_> = before
            .get(agent_id)
            .into_iter()
            .flatten()
            .cloned()
            .collect();
        let right: BTreeSet<_> = after.get(agent_id).into_iter().flatten().cloned().collect();
        if !right.is_subset(&left) {
            return false;
        }
        changed |= left != right;
    }
    changed
}

fn model_state_changes(plan: &DomainPlan) -> Vec<ModelStateChange> {
    let DomainPlan::Model { before, after } = plan else {
        return Vec::new();
    };
    let mut changes = Vec::new();
    for agent_id in union_keys(before, after) {
        let left = before.get(agent_id).cloned().unwrap_or_default();
        let right = after.get(agent_id).cloned().unwrap_or_default();
        let profile_ids: BTreeSet<String> = left
            .profiles
            .keys()
            .chain(right.profiles.keys())
            .cloned()
            .collect();
        for profile_id in profile_ids {
            let snapshot = |selection: &super::types::ModelAgentSelection| {
                let record = selection.profiles.get(&profile_id);
                ModelStateSnapshot {
                    added: record.is_some(),
                    enabled: record.is_some_and(|record| record.enabled),
                    active: selection.active_profile_id.as_deref() == Some(profile_id.as_str()),
                }
            };
            let before_state = snapshot(&left);
            let after_state = snapshot(&right);
            if before_state == after_state {
                continue;
            }
            let reason = if !before_state.added && after_state.added {
                "model_added"
            } else if before_state.added && !after_state.added {
                "model_removed"
            } else if before_state.enabled != after_state.enabled {
                if after_state.enabled {
                    "model_enabled"
                } else {
                    "model_disabled"
                }
            } else if before_state.active != after_state.active {
                if after_state.active {
                    "model_activated"
                } else {
                    "model_deactivated"
                }
            } else {
                "model_state_updated"
            };
            let fallback_profile_id = (before_state.active && !after_state.active)
                .then(|| right.active_profile_id.clone())
                .flatten();
            changes.push(ModelStateChange {
                agent_id: agent_id.clone(),
                profile_id,
                before: before_state,
                after: after_state,
                fallback_profile_id,
                reason: reason.into(),
            });
        }
    }
    changes
}

fn append_model_credential_export_warning(plan: &DomainPlan, warnings: &mut Vec<String>) {
    let DomainPlan::Model { before, after } = plan else {
        return;
    };
    let should_warn = union_keys(before, after)
        .into_iter()
        .filter(|agent_id| agent_id.as_str() == "claude-desktop")
        .any(|agent_id| {
            let left = before.get(agent_id).cloned().unwrap_or_default();
            let right = after.get(agent_id).cloned().unwrap_or_default();
            let profile_ids = left
                .profiles
                .keys()
                .chain(right.profiles.keys())
                .cloned()
                .collect::<BTreeSet<_>>();
            profile_ids.into_iter().any(|profile_id| {
                (!left.profiles.contains_key(&profile_id)
                    && right.profiles.contains_key(&profile_id))
                    || (left.active_profile_id.as_deref() != Some(profile_id.as_str())
                        && right.active_profile_id.as_deref() == Some(profile_id.as_str()))
                    || (left.active_profile_id.as_deref() == Some(profile_id.as_str())
                        && right.active_profile_id.as_deref() == Some(profile_id.as_str())
                        && left
                            .profiles
                            .get(&profile_id)
                            .is_some_and(|record| !record.enabled)
                        && right
                            .profiles
                            .get(&profile_id)
                            .is_some_and(|record| record.enabled))
            })
        });
    if should_warn {
        warnings.push("claude-desktop: model_credential_export_plaintext".into());
    }
}

fn order_model_target_files(plan: &DomainPlan, target_files: &mut [String]) -> Result<(), String> {
    let DomainPlan::Model { before, after } = plan else {
        return Ok(());
    };
    if !union_keys(before, after)
        .iter()
        .any(|agent_id| agent_id.as_str() == "claude-desktop")
    {
        return Ok(());
    }
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let order =
        crate::resources::model::configured_path_strings_checked(&settings, "claude-desktop")?
            .ok_or_else(|| "unsupported model Agent: claude-desktop".to_string())?;
    let positions = target_files
        .iter()
        .enumerate()
        .filter(|(_, path)| order.iter().any(|candidate| candidate == *path))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut recognized = positions
        .iter()
        .map(|index| target_files[*index].clone())
        .collect::<Vec<_>>();
    recognized.sort_by_key(|path| {
        order
            .iter()
            .position(|candidate| candidate == path)
            .expect("recognized Claude Desktop target has a configured role")
    });
    for (index, path) in positions.into_iter().zip(recognized) {
        target_files[index] = path;
    }
    Ok(())
}

fn consumption_state_changes(
    model_changes: &[ModelStateChange],
    lifecycle: Option<&LifecycleBinding>,
    inventory: &ConsumptionInventory,
) -> Result<Vec<ConsumptionStateChange>, String> {
    let mut changes = model_changes
        .iter()
        .filter(|change| {
            change.before.added
                && change.after.added
                && change.before.enabled != change.after.enabled
        })
        .map(|change| ConsumptionStateChange {
            agent_id: change.agent_id.clone(),
            asset: AssetRef::Model {
                profile_id: change.profile_id.clone(),
            },
            before_enabled: change.before.enabled,
            after_enabled: change.after.enabled,
            affected_agent_ids: vec![change.agent_id.clone()],
            target: None,
        })
        .collect::<Vec<_>>();

    match lifecycle {
        Some(LifecycleBinding::McpEnabled {
            agent_id,
            asset_key,
            before,
            after,
        }) => changes.push(ConsumptionStateChange {
            agent_id: agent_id.clone(),
            asset: AssetRef::Mcp {
                key: asset_key.clone(),
            },
            before_enabled: *before,
            after_enabled: *after,
            affected_agent_ids: vec![agent_id.clone()],
            target: None,
        }),
        Some(LifecycleBinding::McpEnabledBulk {
            agent_id,
            before,
            after,
        }) => {
            changes.extend(
                before
                    .iter()
                    .filter(|(_, enabled)| **enabled != *after)
                    .map(|(asset_key, enabled)| ConsumptionStateChange {
                        agent_id: agent_id.clone(),
                        asset: AssetRef::Mcp {
                            key: asset_key.clone(),
                        },
                        before_enabled: *enabled,
                        after_enabled: *after,
                        affected_agent_ids: vec![agent_id.clone()],
                        target: None,
                    }),
            );
        }
        Some(LifecycleBinding::SkillEnabled {
            agent_id,
            name,
            target_id,
            affected_agent_ids,
            before,
            after,
        }) => {
            let target = inventory
                .consumptions
                .iter()
                .filter(|item| {
                    item.asset == (AssetRef::Skill { name: name.clone() })
                        && item
                            .target
                            .as_ref()
                            .is_some_and(|target| target.target_id == *target_id)
                })
                .find_map(|item| item.target.clone())
                .ok_or_else(|| {
                    "skill_target_missing: reviewed Skill target is unavailable".to_string()
                })?;
            let mut affected = affected_agent_ids.clone();
            affected.sort();
            affected.dedup();
            changes.push(ConsumptionStateChange {
                agent_id: agent_id.clone(),
                asset: AssetRef::Skill { name: name.clone() },
                before_enabled: *before,
                after_enabled: *after,
                affected_agent_ids: affected,
                target: Some(ConsumptionTarget {
                    target_id: target.target_id,
                    global_dir: target.global_dir,
                }),
            });
        }
        _ => {}
    }
    changes.sort_by(|left, right| {
        left.agent_id
            .cmp(&right.agent_id)
            .then_with(|| left.asset.cmp(&right.asset))
    });
    Ok(changes)
}

fn relationship_changes(plan: &DomainPlan) -> Vec<RelationshipChange> {
    let mut changes = Vec::new();
    match plan {
        DomainPlan::Mcp { before, after } => {
            for agent_id in union_keys(before, after) {
                diff_many(
                    agent_id,
                    before.get(agent_id).cloned().unwrap_or_default(),
                    after.get(agent_id).cloned().unwrap_or_default(),
                    |key| AssetRef::Mcp { key },
                    &mut changes,
                );
            }
        }
        DomainPlan::Skill { before, after } => {
            for agent_id in union_keys(before, after) {
                diff_many(
                    agent_id,
                    before.get(agent_id).cloned().unwrap_or_default(),
                    after.get(agent_id).cloned().unwrap_or_default(),
                    |name| AssetRef::Skill { name },
                    &mut changes,
                );
            }
        }
        DomainPlan::Model { before, after } => {
            for agent_id in union_keys(before, after) {
                let left = before.get(agent_id).cloned().unwrap_or_default();
                let right = after.get(agent_id).cloned().unwrap_or_default();
                diff_many(
                    agent_id,
                    left.profiles.keys().cloned().collect(),
                    right.profiles.keys().cloned().collect(),
                    |profile_id| AssetRef::Model { profile_id },
                    &mut changes,
                );
            }
        }
        DomainPlan::AgentCapabilities {
            skills_before,
            skills_after,
            ..
        } => {
            for agent_id in union_keys(skills_before, skills_after) {
                diff_many(
                    agent_id,
                    skills_before.get(agent_id).cloned().unwrap_or_default(),
                    skills_after.get(agent_id).cloned().unwrap_or_default(),
                    |name| AssetRef::Skill { name },
                    &mut changes,
                );
            }
        }
    }
    changes.sort_by(|left, right| {
        left.agent_id
            .cmp(&right.agent_id)
            .then_with(|| left.asset.cmp(&right.asset))
            .then_with(|| format!("{:?}", left.action).cmp(&format!("{:?}", right.action)))
    });
    changes
}

fn diff_many<F>(
    agent_id: &str,
    before: Vec<String>,
    after: Vec<String>,
    asset: F,
    out: &mut Vec<RelationshipChange>,
) where
    F: Fn(String) -> AssetRef,
{
    let before: BTreeSet<String> = before.into_iter().collect();
    let after: BTreeSet<String> = after.into_iter().collect();
    for identity in before.difference(&after) {
        out.push(RelationshipChange {
            agent_id: agent_id.to_owned(),
            asset: asset(identity.clone()),
            action: RelationshipAction::Remove,
        });
    }
    for identity in after.difference(&before) {
        out.push(RelationshipChange {
            agent_id: agent_id.to_owned(),
            asset: asset(identity.clone()),
            action: RelationshipAction::Add,
        });
    }
}

fn union_keys<'a, T>(
    left: &'a BTreeMap<String, T>,
    right: &'a BTreeMap<String, T>,
) -> BTreeSet<&'a String> {
    left.keys().chain(right.keys()).collect()
}

fn agents_for_plan(plan: &DomainPlan) -> BTreeSet<String> {
    match plan {
        DomainPlan::Mcp { before, after } | DomainPlan::Skill { before, after } => {
            before.keys().chain(after.keys()).cloned().collect()
        }
        DomainPlan::Model { before, after } => before.keys().chain(after.keys()).cloned().collect(),
        DomainPlan::AgentCapabilities {
            agent_id,
            skills_before,
            skills_after,
            affected_agent_ids,
            ..
        } => skills_before
            .keys()
            .chain(skills_after.keys())
            .cloned()
            .chain(affected_agent_ids.iter().cloned())
            .chain(std::iter::once(agent_id.clone()))
            .collect(),
    }
}

fn domain_matches(plan: &DomainPlan, asset: &AssetRef) -> bool {
    matches!(
        (plan, asset),
        (DomainPlan::Mcp { .. }, AssetRef::Mcp { .. })
            | (DomainPlan::Model { .. }, AssetRef::Model { .. })
            | (DomainPlan::Skill { .. }, AssetRef::Skill { .. })
            | (DomainPlan::AgentCapabilities { .. }, AssetRef::Skill { .. })
    )
}

fn warning_asset_identity(asset: &AssetRef) -> String {
    match asset {
        AssetRef::Mcp { key } => format!("mcp:{key}"),
        AssetRef::Model { profile_id } => format!("model:{profile_id}"),
        AssetRef::ModelProvider { provider_id } => format!("model-provider:{provider_id}"),
        AssetRef::Skill { name } => format!("skill:{name}"),
    }
}

fn consumption_warning(item: &super::types::ConsumptionView) -> String {
    format!(
        "{} / {}: {}",
        item.agent_id,
        warning_asset_identity(&item.asset),
        item.reason.as_deref().unwrap_or("unresolved_drift")
    )
}

fn unsupported_effect_is_safe_model_release(
    agent_id: &str,
    asset: &AssetRef,
    relationship_changes: &[RelationshipChange],
    state_changes: &[ConsumptionStateChange],
    lifecycle: Option<&LifecycleBinding>,
) -> bool {
    let AssetRef::Model { profile_id } = asset else {
        return false;
    };
    relationship_changes.iter().any(|change| {
        change.agent_id == agent_id
            && change.asset == *asset
            && change.action == RelationshipAction::Remove
    }) || state_changes.iter().any(|change| {
        change.agent_id == agent_id
            && change.asset == *asset
            && change.before_enabled
            && !change.after_enabled
    }) || matches!(
        lifecycle,
        Some(LifecycleBinding::ModelReapply {
            agent_id: lifecycle_agent_id,
            profile_id: lifecycle_profile_id,
            enabled: false,
            ..
        }) if lifecycle_agent_id == agent_id && lifecycle_profile_id == profile_id
    )
}

pub(crate) fn effect_assets(
    plan: &DomainPlan,
    central_changes: &[CentralAssetChange],
    relationship_changes: &[RelationshipChange],
    state_changes: &[ConsumptionStateChange],
    lifecycle: Option<&LifecycleBinding>,
) -> BTreeSet<(String, AssetRef)> {
    let mut effects: BTreeSet<(String, AssetRef)> = relationship_changes
        .iter()
        .map(|change| (change.agent_id.clone(), change.asset.clone()))
        .collect();
    for change in state_changes {
        effects.extend(
            change
                .affected_agent_ids
                .iter()
                .cloned()
                .map(|agent_id| (agent_id, change.asset.clone())),
        );
    }
    if let DomainPlan::Model { before, after } = plan {
        for agent_id in union_keys(before, after) {
            let left = before.get(agent_id).cloned().unwrap_or_default();
            let right = after.get(agent_id).cloned().unwrap_or_default();
            if left == right {
                // Relationship commands are idempotent. Repairing observed
                // Model state is a separate, explicit lifecycle operation.
                continue;
            }
            let profile_ids: BTreeSet<String> = left
                .profiles
                .keys()
                .chain(right.profiles.keys())
                .cloned()
                .collect();
            effects.extend(profile_ids.into_iter().filter_map(|profile_id| {
                let left_record = left.profiles.get(&profile_id);
                let right_record = right.profiles.get(&profile_id);
                let became_active = left.active_profile_id.as_deref() != Some(profile_id.as_str())
                    && right.active_profile_id.as_deref() == Some(profile_id.as_str());
                // Switching current writes the new current Profile and the
                // shared pointer. The old current payload is not a physical
                // effect unless its relationship/enabled record also changed.
                let enabled_payload_changed = left_record != right_record
                    && (left_record.is_some_and(|record| record.enabled)
                        || right_record.is_some_and(|record| record.enabled));
                (enabled_payload_changed || became_active)
                    .then(|| (agent_id.clone(), AssetRef::Model { profile_id }))
            }));
        }
    }
    let agents = agents_for_plan(plan);
    for change in central_changes {
        if !domain_matches(plan, &change.asset) {
            continue;
        }
        effects.extend(
            agents
                .iter()
                .filter(|agent_id| {
                    central_asset_is_consumed_before_or_after(plan, agent_id, &change.asset)
                })
                .cloned()
                .map(|agent_id| (agent_id, change.asset.clone())),
        );
    }
    if let Some(LifecycleBinding::ModelReapply {
        agent_id,
        profile_id,
        ..
    }) = lifecycle
    {
        // Disabled Model reapply intentionally clears a stray exact managed
        // entry, so it remains a physical effect even though the relationship
        // itself is disabled and ordinary central updates skip it.
        effects.insert((
            agent_id.clone(),
            AssetRef::Model {
                profile_id: profile_id.clone(),
            },
        ));
    }
    effects
}

fn central_asset_is_consumed_before_or_after(
    plan: &DomainPlan,
    agent_id: &str,
    asset: &AssetRef,
) -> bool {
    match (plan, asset) {
        (DomainPlan::Mcp { before, after }, AssetRef::Mcp { key }) => {
            before.get(agent_id).is_some_and(|keys| keys.contains(key))
                || after.get(agent_id).is_some_and(|keys| keys.contains(key))
        }
        (DomainPlan::Model { before, after }, AssetRef::Model { profile_id }) => {
            before
                .get(agent_id)
                .and_then(|selection| selection.profiles.get(profile_id))
                .is_some_and(|record| record.enabled)
                || after
                    .get(agent_id)
                    .and_then(|selection| selection.profiles.get(profile_id))
                    .is_some_and(|record| record.enabled)
        }
        (DomainPlan::Skill { before, after }, AssetRef::Skill { name }) => {
            before
                .get(agent_id)
                .is_some_and(|names| names.contains(name))
                || after
                    .get(agent_id)
                    .is_some_and(|names| names.contains(name))
        }
        (
            DomainPlan::AgentCapabilities {
                skills_before,
                skills_after,
                ..
            },
            AssetRef::Skill { name },
        ) => {
            skills_before
                .get(agent_id)
                .is_some_and(|names| names.contains(name))
                || skills_after
                    .get(agent_id)
                    .is_some_and(|names| names.contains(name))
        }
        _ => false,
    }
}

fn asset_desired_after(plan: &DomainPlan, agent_id: &str, asset: &AssetRef) -> bool {
    match (plan, asset) {
        (DomainPlan::Mcp { after, .. }, AssetRef::Mcp { key }) => {
            after.get(agent_id).is_some_and(|keys| keys.contains(key))
        }
        (DomainPlan::Model { after, .. }, AssetRef::Model { profile_id }) => after
            .get(agent_id)
            .is_some_and(|selection| selection.profiles.contains_key(profile_id)),
        (DomainPlan::Skill { after, .. }, AssetRef::Skill { name }) => after
            .get(agent_id)
            .is_some_and(|names| names.contains(name)),
        (DomainPlan::AgentCapabilities { skills_after, .. }, AssetRef::Skill { name }) => {
            skills_after
                .get(agent_id)
                .is_some_and(|names| names.contains(name))
        }
        _ => false,
    }
}

fn target_files(
    plan: &DomainPlan,
    lifecycle: Option<&LifecycleBinding>,
    effects: &BTreeSet<(String, AssetRef)>,
) -> Result<Vec<String>, String> {
    if let Some(LifecycleBinding::ModelReapply {
        agent_id,
        profile_id,
        active,
        ..
    }) = lifecycle
    {
        let settings = load_settings_strict().map_err(|error| error.to_string())?;
        let mut paths =
            crate::resources::model::configured_path_strings_checked(&settings, agent_id)?
                .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
        if agent_id == "pi" && !active {
            paths.truncate(1);
        }
        return Ok(crate::resources::model::adapters::target_files(
            agent_id,
            &paths,
            std::slice::from_ref(profile_id),
            settings.model_profiles.as_ref().unwrap_or(&BTreeMap::new()),
        ));
    }
    let mut files = BTreeSet::new();
    match plan {
        DomainPlan::Mcp { .. } => {
            let agents = load_agents();
            for agent_id in agents_for_plan(plan) {
                if let Some(path) = agents.get(&agent_id).and_then(|agent| agent.global.clone()) {
                    files.insert(path);
                }
            }
        }
        DomainPlan::Model { before, after } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            for agent_id in agents_for_plan(plan) {
                let profile_ids = effects
                    .iter()
                    .filter_map(|(effect_agent_id, asset)| match asset {
                        AssetRef::Model { profile_id } if effect_agent_id == &agent_id => {
                            Some(profile_id.clone())
                        }
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>();
                if profile_ids.is_empty() {
                    continue;
                }
                let mut paths =
                    crate::resources::model::configured_path_strings_checked(&settings, &agent_id)?
                        .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
                let left = before.get(&agent_id).cloned().unwrap_or_default();
                let right = after.get(&agent_id).cloned().unwrap_or_default();
                if agent_id == "pi"
                    && left != right
                    && left.active_profile_id == right.active_profile_id
                {
                    paths.truncate(1);
                }
                for path in crate::resources::model::adapters::target_files(
                    &agent_id,
                    &paths,
                    &profile_ids.into_iter().collect::<Vec<_>>(),
                    settings.model_profiles.as_ref().unwrap_or(&BTreeMap::new()),
                ) {
                    files.insert(path);
                }
            }
        }
        DomainPlan::Skill { before, after } => {
            let skills = list_skills_inventory().map_err(|error| format!("{error:?}"))?;
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let assignments =
                canonical_skill_assignments(&settings).map_err(|error| format!("{error:?}"))?;
            let changed_names = changed_skill_names(before, after);

            for name in changed_names {
                let desired_agents = after
                    .iter()
                    .filter(|(_, names)| names.contains(&name))
                    .map(|(agent_id, _)| agent_id.clone())
                    .collect::<Vec<_>>();
                let mut touched_target_ids = assignments.get(&name).cloned().unwrap_or_default();
                touched_target_ids.extend(
                    normalize_agent_selection(&desired_agents)
                        .map_err(|error| format!("{error:?}"))?,
                );
                touched_target_ids.extend(
                    skills
                        .items
                        .iter()
                        .filter(|item| item.name == name)
                        .flat_map(|item| item.assigned_target_ids.iter().cloned()),
                );

                for target in &skills.targets {
                    if touched_target_ids.contains(&target.target_id) {
                        files.insert(format!("{}/{}", target.global_dir, name));
                    }
                }
            }
        }
        DomainPlan::AgentCapabilities { .. } => {}
    }
    Ok(files.into_iter().collect())
}

fn changed_skill_names(
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|agent_id| {
            let left: BTreeSet<String> = before
                .get(agent_id)
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            let right: BTreeSet<String> =
                after.get(agent_id).into_iter().flatten().cloned().collect();
            left.symmetric_difference(&right)
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn hash_targets(targets: &[String]) -> BTreeMap<String, String> {
    targets
        .iter()
        .map(|target| {
            let path = expand_tilde(target);
            (target.clone(), hash_target(&path))
        })
        .collect()
}

pub(crate) fn hash_target(path: &Path) -> String {
    capture_parent_directory(path)
        .and_then(|parent| {
            let state = read_path_state_anchored(path, &parent)?;
            fingerprint_anchored_path_state(&state, &parent)
        })
        .unwrap_or_else(|error| hex::encode(Sha256::digest(format!("error:{error}").as_bytes())))
}

/// Fingerprint the guarded settings namespace without binding an Asset plan to
/// settings contents or the inode produced by an unrelated atomic settings
/// update. Semantic state is covered by typed StatePreconditions; this guard
/// owns only the parent directory identity and the requirement that the leaf
/// remain a regular-file slot.
pub(crate) fn hash_settings_target(path: &Path) -> String {
    capture_parent_directory(path)
        .and_then(|parent| {
            let state = read_path_state_anchored(path, &parent)?;
            let leaf = match state {
                crate::safe_write::AnchoredPathState::Missing
                | crate::safe_write::AnchoredPathState::File { .. } => "regular-or-missing",
                crate::safe_write::AnchoredPathState::Symlink { .. } => "symlink",
                crate::safe_write::AnchoredPathState::Directory { .. } => "directory",
                crate::safe_write::AnchoredPathState::Other { .. } => "other",
            };
            let bytes = serde_json::to_vec(&(leaf, parent)).map_err(|error| error.to_string())?;
            Ok(hex::encode(Sha256::digest(bytes)))
        })
        .unwrap_or_else(|error| hex::encode(Sha256::digest(format!("error:{error}").as_bytes())))
}

pub(crate) fn hash_file(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => hex::encode(Sha256::digest(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => "missing".into(),
        Err(error) => format!("error:{:?}", error.kind()),
    }
}

pub(crate) fn hash_mcp_catalog() -> Result<String, String> {
    // Converting through Value canonicalizes object keys that originated in
    // HashMaps (headers/env) before hashing.
    let catalog = serde_json::to_value(crate::resources::mcp::registry::read_registry_all())
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&catalog).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn hash_skill_target_graph() -> Result<String, String> {
    let inventory = list_skills_inventory().map_err(|error| format!("{error:?}"))?;
    let mut targets = inventory
        .targets
        .into_iter()
        .map(|target| {
            (
                target.target_id,
                target.global_dir,
                target
                    .affected_agent_ids
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<Vec<_>>();
    targets.sort();
    let bytes = serde_json::to_vec(&targets).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(crate) fn operation_root(operation_id: &str) -> PathBuf {
    arca_dir().join("staging/consumption").join(operation_id)
}

fn persist_operation(operation: &PersistedAssetOperation) -> Result<(), String> {
    let root = operation_root(&operation.plan.operation_id);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    let path = root.join("plan.json");
    let bytes = serde_json::to_vec(operation).map_err(|error| error.to_string())?;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    super::lease::retain(&operation.plan.operation_id)?;
    Ok(())
}

pub(crate) const OPERATION_UNAVAILABLE_ERROR: &str = "asset operation is unavailable or expired";

pub(crate) fn load_operation(operation_id: &str) -> Result<PersistedAssetOperation, String> {
    Uuid::parse_str(operation_id).map_err(|_| "invalid asset operation id".to_string())?;
    let bytes = fs::read(operation_root(operation_id).join("plan.json"))
        .map_err(|_| OPERATION_UNAVAILABLE_ERROR.to_string())?;
    let operation: PersistedAssetOperation = serde_json::from_slice(&bytes)
        .map_err(|_| "asset operation plan is invalid".to_string())?;
    let canonical = serde_json::to_vec(&operation)
        .map_err(|_| "asset operation plan is invalid".to_string())?;
    if !matches!(
        operation.schema_version,
        LEGACY_OPERATION_SCHEMA_VERSION | OPERATION_SCHEMA_VERSION
    ) || operation.plan.operation_id != operation_id
        || canonical != bytes
        || candidate_hash(&operation)? != operation.plan.candidate_hash
    {
        return Err("asset_operation_stale: reviewed plan failed integrity validation".into());
    }
    Ok(operation)
}

fn candidate_hash(operation: &PersistedAssetOperation) -> Result<String, String> {
    if operation.schema_version == LEGACY_OPERATION_SCHEMA_VERSION {
        return legacy_candidate_hash(operation);
    }
    let plan = &operation.plan;
    let material = serde_json::to_vec(&(
        &plan.kind,
        &plan.domain_plan,
        &plan.central_changes,
        &plan.relationship_changes,
        &plan.model_state_changes,
        &plan.consumption_state_changes,
        &plan.target_files,
        &plan.affected_agent_ids,
        &plan.warnings,
        plan.can_commit,
        &operation.settings_target_hash,
        &operation.target_hashes,
        &operation.state_preconditions,
        &operation.lifecycle,
    ))
    .map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(material)))
}

fn legacy_candidate_hash(operation: &PersistedAssetOperation) -> Result<String, String> {
    let plan = &operation.plan;
    let settings_hash = operation
        .settings_hash
        .as_deref()
        .ok_or_else(|| "asset operation plan is invalid".to_string())?;
    let material = serde_json::to_vec(&(
        &plan.kind,
        &plan.domain_plan,
        &plan.central_changes,
        &plan.relationship_changes,
        &plan.model_state_changes,
        &plan.consumption_state_changes,
        &plan.target_files,
        &plan.affected_agent_ids,
        &plan.warnings,
        plan.can_commit,
        settings_hash,
        &operation.target_hashes,
        &operation.mcp_catalog_hash,
        &operation.skill_target_graph_hash,
        &operation.lifecycle,
    ))
    .map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(material)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{McpConfigurationPatch, ModelConfigurationPatch};
    use crate::assets::{commit_asset_operation, AssetCommitRequest};
    use crate::domain::assets::{McpConsumptionRecord, ModelAgentSelection};
    use crate::domain::types::{
        HttpConfig, ModelProfile, ModelProtocol, RegistryConfig, RegistryEntry, StdioConfig,
    };
    use crate::resources::mcp::registry::write_manual_entry;
    use crate::settings::mutate_settings;
    use crate::testenv::TestHome;

    #[test]
    fn shared_client_mcp_requires_a_closed_removal_plan() {
        for agents in [["qoder-cli", "qoder-desktop"], ["kimi-code", "kimi-code-desktop"], ["codex", "codex-desktop"]] {
            let _home = TestHome::new("shared-client-removal");
            let key = "shared::stdio".to_string();
            mutate_settings(|settings| {
                settings.mcp_consumptions = Some(agents.iter().map(|id| (id.to_string(),
                    BTreeMap::from([(key.clone(), McpConsumptionRecord {
                        asset_key: key.clone(), enabled: true, overrides: Default::default(),
                    })]))).collect());
            }).unwrap();
            let partial = DomainPlan::Mcp {
                before: BTreeMap::from([(agents[1].into(), vec![key.clone()])]),
                after: BTreeMap::from([(agents[1].into(), vec![])]),
            };
            let (conflicts, subjects) = shared_mcp_conflicts(&partial, None).unwrap();
            assert!(!conflicts.is_empty());
            assert!(subjects.contains(&StateSubject::AgentConsumption {
                capability: crate::domain::assets::AssetCapability::Mcp, agent_id: agents[0].into(),
            }));
            let closed = DomainPlan::Mcp {
                before: agents.iter().map(|id| (id.to_string(), vec![key.clone()])).collect(),
                after: agents.iter().map(|id| (id.to_string(), vec![])).collect(),
            };
            assert!(shared_mcp_conflicts(&closed, None).unwrap().0.is_empty());
        }
    }

    #[test]
    fn legacy_v2_reviewed_operations_remain_loadable_and_committable() {
        let _home = TestHome::new("asset-plan-v2-compatibility");
        write_manual_entry(&RegistryEntry {
            name: "legacy-plan".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "legacy-plan-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["legacy-plan::stdio".into()],
            },
        })
        .unwrap();
        let mut persisted = load_operation(&plan.operation_id).unwrap();
        persisted.schema_version = LEGACY_OPERATION_SCHEMA_VERSION;
        persisted.settings_hash = Some(hash_file(&settings_file()));
        persisted.settings_target_hash = hash_target(&settings_file());
        persisted.state_preconditions.clear();
        persisted.mcp_catalog_hash = Some(hash_mcp_catalog().unwrap());
        persisted.skill_target_graph_hash = None;
        persisted.plan.candidate_hash = legacy_candidate_hash(&persisted).unwrap();
        let path = operation_root(&persisted.plan.operation_id).join("plan.json");
        fs::write(&path, serde_json::to_vec(&persisted).unwrap()).unwrap();

        let loaded = load_operation(&persisted.plan.operation_id).unwrap();
        assert_eq!(loaded.schema_version, LEGACY_OPERATION_SCHEMA_VERSION);
        commit_asset_operation(AssetCommitRequest {
            operation_id: loaded.plan.operation_id,
            candidate_hash: loaded.plan.candidate_hash,
        })
        .unwrap();
    }

    fn write_external_skill(root: &Path, name: &str, description: &str) {
        let skill = root.join(name);
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n"),
        )
        .unwrap();
    }

    fn write_local_mcp_for_target_safety() {
        write_manual_entry(&RegistryEntry {
            name: "target-safety".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "target-safety-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
    }

    #[test]
    fn mcp_plan_rejects_a_directory_target_without_touching_its_contents() {
        let home = TestHome::new("consume-directory-target");
        write_local_mcp_for_target_safety();
        let target = home.home.join(".claude.json");
        fs::create_dir_all(&target).unwrap();
        let sentinel = target.join("keep.txt");
        fs::write(&sentinel, "keep").unwrap();

        let error = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["target-safety::stdio".into()],
            },
        })
        .unwrap_err();

        assert!(error.contains("refusing to use directory"), "{error}");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn mcp_plan_rejects_a_symlink_backed_configuration_file() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("consume-symlink-target");
        write_local_mcp_for_target_safety();
        let destination = home.home.join("real-claude.json");
        let target = home.home.join(".claude.json");
        fs::write(&destination, "{\"untouched\":true}").unwrap();
        symlink(&destination, &target).unwrap();

        let error = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["target-safety::stdio".into()],
            },
        })
        .unwrap_err();

        assert!(error.contains("symlink-backed"), "{error}");
        assert_eq!(
            fs::read_to_string(destination).unwrap(),
            "{\"untouched\":true}"
        );
    }

    #[test]
    fn mcp_plan_has_stable_typed_diff_and_private_persistence() {
        let home = TestHome::new("consume-plan");
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        mutate_settings(|settings| settings.mcp_consumptions = None).unwrap();

        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();

        assert!(plan.can_commit);
        assert_eq!(plan.relationship_changes.len(), 1);
        assert_eq!(plan.relationship_changes[0].action, RelationshipAction::Add);
        let persisted = fs::read_to_string(
            home.home
                .join(".arca/staging/consumption")
                .join(&plan.operation_id)
                .join("plan.json"),
        )
        .unwrap();
        assert!(!persisted.contains(home.home.to_string_lossy().as_ref()));
    }

    #[test]
    fn persisted_asset_plan_rejects_semantic_tampering_with_the_reviewed_candidate() {
        let _home = TestHome::new("consume-plan-integrity");
        write_local_mcp_for_target_safety();
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["target-safety::stdio".into()],
            },
        })
        .unwrap();
        let path = operation_root(&plan.operation_id).join("plan.json");
        let mut persisted: PersistedAssetOperation =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let DomainPlan::Mcp { after, .. } = &mut persisted.plan.domain_plan else {
            unreachable!();
        };
        after.get_mut("claude-code").unwrap().clear();
        fs::write(&path, serde_json::to_vec(&persisted).unwrap()).unwrap();

        let error = load_operation(&plan.operation_id).unwrap_err();
        assert!(error.starts_with("asset_operation_stale:"), "{error}");
    }

    #[test]
    fn persisted_asset_plan_rejects_noncanonical_bytes() {
        let _home = TestHome::new("consume-plan-canonical");
        write_local_mcp_for_target_safety();
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["target-safety::stdio".into()],
            },
        })
        .unwrap();
        let path = operation_root(&plan.operation_id).join("plan.json");
        let mut bytes = fs::read(&path).unwrap();
        bytes.push(b'\n');
        fs::write(&path, bytes).unwrap();

        let error = load_operation(&plan.operation_id).unwrap_err();
        assert!(error.starts_with("asset_operation_stale:"), "{error}");
    }

    #[test]
    fn agent_and_asset_entrypoints_produce_the_same_candidate() {
        let _home = TestHome::new("consume-plan-equivalent");
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        let agent = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let asset = plan_set_asset_consumers(PlanSetAssetConsumersRequest {
            asset: AssetRef::Mcp {
                key: "local::stdio".into(),
            },
            agent_ids: vec!["claude-code".into()],
        })
        .unwrap();
        assert_eq!(agent.domain_plan, asset.domain_plan);
        assert_eq!(agent.relationship_changes, asset.relationship_changes);
        assert_eq!(agent.target_files, asset.target_files);
        assert_eq!(agent.candidate_hash, asset.candidate_hash);
    }

    #[test]
    fn consumer_delta_preserves_existing_consumers() {
        let _home = TestHome::new("consume-plan-delta");
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("codex".into())
                .or_default()
                .insert(
                    "local::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "local::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                );
        })
        .unwrap();

        let plan = plan_update_asset_consumers(PlanUpdateAssetConsumersRequest {
            asset: AssetRef::Mcp {
                key: "local::stdio".into(),
            },
            add_agent_ids: vec!["claude-code".into()],
            remove_agent_ids: Vec::new(),
        })
        .unwrap();

        let DomainPlan::Mcp { after, .. } = plan.domain_plan else {
            panic!("expected MCP plan");
        };
        assert!(after["codex"].contains(&"local::stdio".to_string()));
        assert!(after["claude-code"].contains(&"local::stdio".to_string()));
    }

    #[test]
    fn ensure_agent_consumption_preserves_unrelated_assets() {
        let _home = TestHome::new("consume-plan-ensure");
        for name in ["first", "second"] {
            write_manual_entry(&RegistryEntry {
                name: name.into(),
                description: String::new(),
                tags: Vec::new(),
                config: RegistryConfig {
                    stdio: Some(StdioConfig {
                        command: format!("{name}-server"),
                        args: None,
                        env: None,
                        cwd: None,
                    }),
                    http: None,
                },
                origin: None,
                repo: None,
            })
            .unwrap();
        }
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("codex".into())
                .or_default()
                .insert(
                    "first::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "first::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                );
        })
        .unwrap();

        let plan = plan_ensure_agent_consumption(PlanEnsureAgentConsumptionRequest {
            agent_id: "codex".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["second::stdio".into()],
            },
        })
        .unwrap();

        let DomainPlan::Mcp { after, .. } = plan.domain_plan else {
            panic!("expected MCP plan");
        };
        assert_eq!(
            after["codex"],
            vec!["first::stdio".to_string(), "second::stdio".to_string()]
        );
    }

    #[test]
    fn capability_plan_rejects_in_use_mcp_writer_path_changes() {
        let _home = TestHome::new("capability-plan-mcp-path-in-use");
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("codex".into())
                .or_default()
                .insert(
                    "fixture::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "fixture::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                );
        })
        .unwrap();

        let error = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "codex".into(),
            patch: AgentConfigurationPatch {
                mcp: Some(McpConfigurationPatch {
                    path: "~/.codex/moved.toml".into(),
                    key: None,
                }),
                ..AgentConfigurationPatch::default()
            },
        })
        .unwrap_err();

        assert!(error.starts_with("agent_configuration_in_use:"), "{error}");
    }

    #[test]
    fn capability_plan_stales_when_mcp_consumption_appears_after_review() {
        let _home = TestHome::new("capability-plan-mcp-consumer-race");
        write_manual_entry(&RegistryEntry {
            name: "fixture".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "fixture-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        let plan = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "codex".into(),
            patch: AgentConfigurationPatch {
                mcp: Some(McpConfigurationPatch {
                    path: "~/.codex/moved.toml".into(),
                    key: None,
                }),
                ..AgentConfigurationPatch::default()
            },
        })
        .unwrap();

        let relationship = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "codex".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["fixture::stdio".into()],
            },
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: relationship.operation_id,
            candidate_hash: relationship.candidate_hash,
        })
        .unwrap();

        let error = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap_err();
        assert_eq!(
            error,
            "asset_operation_stale: an Agent consumption changed after review"
        );
    }

    #[test]
    fn capability_plan_rejects_in_use_model_writer_path_changes() {
        let _home = TestHome::new("capability-plan-model-path-in-use");
        mutate_settings(|settings| {
            settings.set_model_selection(
                "grok-build",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        "fixture".into(),
                        ModelConsumptionRecord {
                            profile_id: "fixture".into(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some("fixture".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let error = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "grok-build".into(),
            patch: AgentConfigurationPatch {
                model: Some(ModelConfigurationPatch {
                    paths: vec!["~/.grok/moved.toml".into()],
                }),
                ..AgentConfigurationPatch::default()
            },
        })
        .unwrap_err();

        assert!(error.starts_with("agent_configuration_in_use:"), "{error}");
    }

    #[test]
    fn claude_desktop_capability_plan_rejects_duplicate_model_roles() {
        let _home = TestHome::new("capability-plan-claude-duplicate");
        let duplicate = "~/.claude-duplicate/shared.json".to_string();

        let error = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "claude-desktop".into(),
            patch: AgentConfigurationPatch {
                model: Some(ModelConfigurationPatch {
                    paths: vec![
                        duplicate.clone(),
                        "~/.claude-duplicate/deployment.json".into(),
                        "~/.claude-duplicate/meta.json".into(),
                        duplicate,
                    ],
                }),
                ..AgentConfigurationPatch::default()
            },
        })
        .unwrap_err();

        assert!(error.contains("distinct"), "{error}");
    }

    #[test]
    fn one_agent_cannot_select_two_transports_with_the_same_mcp_name() {
        let _home = TestHome::new("consume-mcp-name-conflict");
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: None,
                http: Some(HttpConfig {
                    kind: "http".into(),
                    url: "https://example.invalid/mcp".into(),
                    headers: None,
                }),
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        let error = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into(), "local::http".into()],
            },
        })
        .unwrap_err();
        assert!(error.starts_with("mcp_identity_conflict:"));
    }

    #[test]
    fn asset_entrypoint_cannot_add_a_second_profile_to_a_single_model_agent() {
        let _home = TestHome::new("consume-model-single-profile");
        let profile = |id: &str| ModelProfile {
            id: id.into(),
            provider_id: None,
            name: id.into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: format!("https://{id}.example.invalid/v1"),
            endpoint_path: String::new(),
            model: format!("{id}-model"),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.model_profiles.get_or_insert_default().extend([
                ("work".into(), profile("work")),
                ("other".into(), profile("other")),
            ]);
            settings.set_model_selection(
                "claude-code",
                crate::assets::ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        "work".into(),
                        ModelConsumptionRecord {
                            profile_id: "work".into(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some("work".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let error = plan_set_asset_consumers(PlanSetAssetConsumersRequest {
            asset: AssetRef::Model {
                profile_id: "other".into(),
            },
            agent_ids: vec!["claude-code".into()],
        })
        .unwrap_err();

        assert!(error.starts_with("model_single_profile_only:"), "{error}");
    }

    #[test]
    fn qwen_rejects_profiles_with_the_same_native_selection_identity() {
        let _home = TestHome::new("consume-model-qwen-identity");
        let profile = |id: &str| ModelProfile {
            id: id.into(),
            provider_id: None,
            name: id.into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiCompletions,
            base_url: format!("https://{id}.example.invalid/v1"),
            endpoint_path: String::new(),
            model: "shared-model".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.model_profiles.get_or_insert_default().extend([
                ("first".into(), profile("first")),
                ("second".into(), profile("second")),
            ]);
        })
        .unwrap();

        let error = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "qwen-code".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["first".into(), "second".into()],
            },
        })
        .unwrap_err();

        assert!(
            error.starts_with("model_native_identity_conflict:"),
            "{error}"
        );
    }

    #[test]
    fn pi_can_add_a_backup_profile_without_taking_over_external_current() {
        let home = TestHome::new("consume-model-pi-backup");
        let profile = |id: &str, model: &str| ModelProfile {
            id: id.into(),
            provider_id: None,
            name: id.into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: format!("https://{id}.example.invalid/v1"),
            endpoint_path: String::new(),
            model: model.into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.model_profiles.get_or_insert_default().extend([
                ("current".into(), profile("current", "current-model")),
                ("backup".into(), profile("backup", "backup-model")),
            ]);
            settings.set_model_selection(
                "pi",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        "current".into(),
                        ModelConsumptionRecord {
                            profile_id: "current".into(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some("current".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();
        crate::resources::model::apply_profile("pi", "current").unwrap();
        let pi_settings = home.home.join(".pi/agent/settings.json");
        fs::write(
            &pi_settings,
            r#"{"defaultProvider":"external","defaultModel":"external-model"}"#,
        )
        .unwrap();
        let external_defaults = fs::read_to_string(&pi_settings).unwrap();

        let plan = plan_ensure_agent_consumption(PlanEnsureAgentConsumptionRequest {
            agent_id: "pi".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["backup".into()],
            },
        })
        .unwrap();

        assert!(plan.can_commit, "{:?}", plan.warnings);
        assert!(plan.warnings.is_empty(), "{:?}", plan.warnings);
        assert_eq!(plan.target_files, vec!["~/.pi/agent/models.json"]);
        assert!(plan.model_state_changes.iter().any(|change| {
            change.profile_id == "backup"
                && change.after.enabled
                && !change.after.active
                && change.reason == "model_added"
        }));

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert_eq!(fs::read_to_string(pi_settings).unwrap(), external_defaults);
        assert!(fs::read_to_string(home.home.join(".pi/agent/models.json"))
            .unwrap()
            .contains("backup-model"));
        let selection = load_settings_strict().unwrap().model_selection("pi");
        assert_eq!(selection.active_profile_id.as_deref(), Some("current"));
        assert!(selection.profiles["backup"].enabled);
    }

    #[test]
    fn current_model_cannot_be_disabled_without_an_enabled_successor() {
        let _home = TestHome::new("consume-model-current-required");
        let profile = ModelProfile {
            id: "current".into(),
            provider_id: None,
            name: "Current".into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: "https://current.example.invalid/v1".into(),
            endpoint_path: String::new(),
            model: "current-model".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings
                .model_profiles
                .get_or_insert_default()
                .insert(profile.id.clone(), profile);
            settings.set_model_selection(
                "pi",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        "current".into(),
                        ModelConsumptionRecord {
                            profile_id: "current".into(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some("current".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let error = plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "pi".into(),
            profile_id: "current".into(),
            enabled: false,
        })
        .unwrap_err();

        assert!(error.starts_with("active_model_required:"), "{error}");
    }

    #[test]
    fn pi_switches_to_a_disabled_backup_and_updates_native_defaults_atomically() {
        let home = TestHome::new("consume-model-pi-switch-backup");
        let profile = |id: &str, model: &str| ModelProfile {
            id: id.into(),
            provider_id: None,
            name: id.into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: format!("https://{id}.example.invalid/v1"),
            endpoint_path: String::new(),
            model: model.into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.model_profiles.get_or_insert_default().extend([
                ("current".into(), profile("current", "current-model")),
                ("backup".into(), profile("backup", "backup-model")),
            ]);
            settings.set_model_selection(
                "pi",
                ModelAgentSelection {
                    profiles: BTreeMap::from([
                        (
                            "current".into(),
                            ModelConsumptionRecord {
                                profile_id: "current".into(),
                                enabled: true,
                                credential: Default::default(),
                                last_selected_at: None,
                            },
                        ),
                        (
                            "backup".into(),
                            ModelConsumptionRecord {
                                profile_id: "backup".into(),
                                enabled: false,
                                credential: Default::default(),
                                last_selected_at: None,
                            },
                        ),
                    ]),
                    active_profile_id: Some("current".into()),
                    ..Default::default()
                },
            );
        })
        .unwrap();
        crate::resources::model::apply_profile("pi", "current").unwrap();

        let plan = plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "pi".into(),
            profile_id: "backup".into(),
        })
        .unwrap();
        assert_eq!(
            plan.target_files,
            vec!["~/.pi/agent/models.json", "~/.pi/agent/settings.json"]
        );
        assert!(plan.model_state_changes.iter().any(|change| {
            change.profile_id == "backup"
                && !change.before.enabled
                && !change.before.active
                && change.after.enabled
                && change.after.active
        }));

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        let selection = load_settings_strict().unwrap().model_selection("pi");
        assert_eq!(selection.active_profile_id.as_deref(), Some("backup"));
        assert!(selection.profiles["backup"].enabled);
        assert!(selection.profiles["current"].enabled);
        let native: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(home.home.join(".pi/agent/settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(native["defaultProvider"], "arca-backup");
        assert_eq!(native["defaultModel"], "backup-model");
        let providers: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(home.home.join(".pi/agent/models.json")).unwrap(),
        )
        .unwrap();
        assert!(providers["providers"].get("arca-current").is_some());
        assert!(providers["providers"].get("arca-backup").is_some());
    }

    fn claude_desktop_provider_profile() -> ModelProfile {
        ModelProfile {
            id: "claude-desktop-planner-test".into(),
            name: "Claude Desktop planner test".into(),
            provider_id: Some("claude-desktop-planner-provider".into()),
            provider: "custom".into(),
            model_vendor: Some("anthropic".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: "https://gateway.example.test".into(),
            endpoint_path: "/anthropic/v1/messages".into(),
            model: "claude-sonnet-test".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(true),
        }
    }

    #[test]
    fn claude_desktop_model_add_warns_about_plaintext_credential_export() {
        let home = TestHome::new("planner-claude-desktop-credential-warning");
        let profile = claude_desktop_provider_profile();
        let fake_credential = "fake-claude-desktop-credential";
        crate::resources::model::save_profile(profile.clone(), Some(fake_credential.into()))
            .unwrap();

        let plan = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id.clone()],
                },
            },
        )
        .unwrap();

        assert_eq!(
            plan.warnings,
            vec!["claude-desktop: model_credential_export_plaintext"]
        );
        assert_eq!(
            plan.target_files,
            vec![
                "~/Library/Application Support/Claude/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
                "~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json",
            ]
        );
        let serialized = fs::read_to_string(
            home.home
                .join(".arca/staging/consumption")
                .join(&plan.operation_id)
                .join("plan.json"),
        )
        .unwrap();
        assert!(!serialized.contains(fake_credential));
    }

    #[test]
    fn claude_desktop_plaintext_export_warning_is_scoped_to_add_or_activation() {
        let _home = TestHome::new("planner-claude-desktop-credential-warning-scope");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();

        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: profile.id.clone(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some(profile.id.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let removal = crate::application::assets::plan_remove_agent_consumption(
            PlanRemoveAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id.clone()],
                },
            },
        )
        .unwrap();
        assert!(!removal
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));

        let no_op = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id.clone()],
                },
            },
        )
        .unwrap();
        assert!(!no_op
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));

        let unrelated = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-code".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id],
                },
            },
        )
        .unwrap();
        assert!(!unrelated
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));
    }

    #[test]
    fn claude_desktop_model_targets_follow_configured_override_order() {
        let _home = TestHome::new("planner-claude-desktop-configured-target-order");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();
        let configured_paths = vec![
            "~/.arca-claude/primary.json".into(),
            "~/.arca-claude/deployment.json".into(),
            "~/.arca-claude/meta.json".into(),
            "~/.arca-claude/profile.json".into(),
        ];
        mutate_settings(|settings| {
            settings.agent_config_paths.get_or_insert_default().insert(
                "claude-desktop".into(),
                AgentConfigPathOverride {
                    model_paths: Some(configured_paths.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let plan = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id],
                },
            },
        )
        .unwrap();

        assert_eq!(plan.target_files, configured_paths);
    }

    #[test]
    fn claude_desktop_duplicate_override_paths_fail_planning() {
        let _home = TestHome::new("planner-claude-desktop-duplicate-targets");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();
        let duplicate = "~/.arca-claude/shared.json".to_string();
        mutate_settings(|settings| {
            settings.agent_config_paths.get_or_insert_default().insert(
                "claude-desktop".into(),
                AgentConfigPathOverride {
                    model_paths: Some(vec![
                        duplicate.clone(),
                        "~/.arca-claude/deployment.json".into(),
                        "~/.arca-claude/meta.json".into(),
                        duplicate.clone(),
                    ]),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let error = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![profile.id],
                },
            },
        )
        .unwrap_err();

        assert!(error.contains("distinct"), "{error}");
    }

    #[test]
    fn claude_desktop_model_target_order_does_not_reorder_mixed_agent_targets() {
        let _home = TestHome::new("planner-claude-desktop-mixed-target-order");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();

        let plan =
            crate::application::assets::plan_set_asset_consumers(PlanSetAssetConsumersRequest {
                asset: AssetRef::Model {
                    profile_id: profile.id,
                },
                agent_ids: vec!["claude-desktop".into(), "pi".into()],
            })
            .unwrap();

        assert_eq!(
            plan.target_files,
            vec![
                "~/.pi/agent/models.json",
                "~/.pi/agent/settings.json",
                "~/Library/Application Support/Claude/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
                "~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json",
            ]
        );
    }

    #[test]
    fn claude_desktop_reenabling_the_active_disabled_model_warns_about_export() {
        let _home = TestHome::new("planner-claude-desktop-reenable-active-warning");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: profile.id.clone(),
                            enabled: false,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some(profile.id.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let plan = crate::application::assets::plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "claude-desktop".into(),
            profile_id: profile.id,
            enabled: true,
        })
        .unwrap();

        assert!(plan
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));
    }

    #[test]
    fn claude_desktop_model_replacement_warns_when_the_active_pointer_changes() {
        let _home = TestHome::new("planner-claude-desktop-active-pointer-warning");
        let first = claude_desktop_provider_profile();
        crate::resources::model::save_profile(first.clone(), Some("first-fake-credential".into()))
            .unwrap();
        let mut second = first.clone();
        second.id = "claude-desktop-planner-test-second".into();
        second.name = "Claude Desktop planner test second".into();
        second.model = "claude-opus-test".into();
        crate::resources::model::save_profile(
            second.clone(),
            Some("second-fake-credential".into()),
        )
        .unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        first.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: first.id.clone(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some(first.id),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let plan = crate::application::assets::plan_set_agent_consumption(
            PlanSetAgentConsumptionRequest {
                agent_id: "claude-desktop".into(),
                selection: AgentConsumptionSelection::Model {
                    profile_ids: vec![second.id.clone()],
                },
            },
        )
        .unwrap();

        let DomainPlan::Model { after, .. } = &plan.domain_plan else {
            panic!("expected Model plan");
        };
        assert_eq!(
            after["claude-desktop"].active_profile_id.as_deref(),
            Some(second.id.as_str())
        );
        assert!(plan
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));
    }

    #[test]
    fn claude_desktop_public_activation_warns_without_a_relationship_add() {
        let _home = TestHome::new("planner-claude-desktop-public-activation-warning");
        let profile = claude_desktop_provider_profile();
        crate::resources::model::save_profile(profile.clone(), Some("fake-credential".into()))
            .unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: profile.id.clone(),
                            enabled: true,
                            credential: Default::default(),
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: None,
                    ..Default::default()
                },
            );
        })
        .unwrap();

        let plan = crate::application::assets::plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "claude-desktop".into(),
            profile_id: profile.id,
        })
        .unwrap();

        assert!(plan.relationship_changes.is_empty());
        assert!(plan
            .warnings
            .contains(&"claude-desktop: model_credential_export_plaintext".into()));
    }

    #[test]
    fn configuration_plan_migrates_external_skills_before_switching_the_path() {
        let home = TestHome::new("configuration-skill-migration");
        fs::create_dir_all(home.home.join(".codex")).unwrap();
        write_external_skill(
            &home.home.join(".agents/skills"),
            "shared-notes",
            "Shared notes",
        );
        mutate_settings(|settings| {
            settings.skill_assignments = Some(
                [(
                    "shared-notes".into(),
                    ["agents-user".into()].into_iter().collect(),
                )]
                .into_iter()
                .collect(),
            );
        })
        .unwrap();
        let mut patch = current_configuration_patch("codex").unwrap();
        patch.skill.as_mut().unwrap().global_dir = "~/.codex-private/skills".into();

        let plan = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "codex".into(),
            patch,
        })
        .unwrap();
        assert_eq!(plan.kind, AssetOperationKind::UpdateConfiguration);
        assert_eq!(plan.affected_agent_ids, vec!["codex"]);
        assert_eq!(
            plan.target_files,
            vec!["~/.codex-private/skills/shared-notes"]
        );

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        let migrated = home.home.join(".codex-private/skills/shared-notes");
        assert!(fs::symlink_metadata(&migrated)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            crate::agents::load_agents()["codex"]
                .skills
                .as_ref()
                .unwrap()
                .target_id,
            "codex-configured"
        );
        assert_eq!(
            load_settings_strict().unwrap().skill_assignments.unwrap()["shared-notes"],
            ["codex-configured".into()].into_iter().collect()
        );
    }

    #[test]
    fn configuration_plan_adds_a_skills_compatibility_directory_without_moving_primary_content() {
        let home = TestHome::new("configuration-skill-alias");
        fs::create_dir_all(home.home.join(".codex")).unwrap();
        let mut patch = current_configuration_patch("codex").unwrap();
        patch.skill.as_mut().unwrap().alias_dirs = vec!["~/.codex/skills".into()];

        let plan = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "codex".into(),
            patch,
        })
        .unwrap();
        assert!(plan.target_files.is_empty());
        let DomainPlan::AgentCapabilities { before, after, .. } = &plan.domain_plan else {
            panic!("expected Agent capability plan");
        };
        assert!(before.skill.as_ref().unwrap().alias_dirs.is_empty());
        assert_eq!(
            after.skill.as_ref().unwrap().alias_dirs,
            ["~/.codex/skills"]
        );

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert_eq!(
            crate::agents::load_agents()["codex"]
                .skills
                .as_ref()
                .unwrap()
                .aliases[0]
                .global_dir,
            "~/.codex/skills"
        );
    }

    #[test]
    fn configuration_plan_commits_a_key_only_override_without_touching_mcp_files() {
        let _home = TestHome::new("configuration-mcp-key-only");
        let mut patch = current_configuration_patch("codex").unwrap();
        patch.mcp.as_mut().unwrap().key = Some("custom.mcpServers".into());

        let plan = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "codex".into(),
            patch,
        })
        .unwrap();

        assert!(plan.target_files.is_empty());
        let DomainPlan::AgentCapabilities { before, after, .. } = &plan.domain_plan else {
            panic!("expected Agent capability plan");
        };
        let before_mcp = before.mcp.as_ref().unwrap();
        let after_mcp = after.mcp.as_ref().unwrap();
        assert_eq!(before_mcp.key.as_deref(), Some("mcp_servers"));
        assert_eq!(after_mcp.key.as_deref(), Some("custom.mcpServers"));
        assert_eq!(before_mcp.path, after_mcp.path);

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert_eq!(
            crate::agents::load_agents()["codex"].key,
            "custom.mcpServers"
        );
    }

    #[test]
    fn new_model_consumptions_inherit_agent_plaintext_default() {
        use crate::domain::assets::ApiKeyDelivery;

        let _home = TestHome::new("consume-model-inherit-plaintext");
        crate::resources::model::save_profile(
            ModelProfile {
                id: "work".into(),
                provider_id: Some("custom-provider".into()),
                name: "Work".into(),
                provider: "custom".into(),
                model_vendor: None,
                native_ids: Default::default(),
                protocol: ModelProtocol::OpenaiCompletions,
                base_url: "https://example.invalid".into(),
                endpoint_path: String::new(),
                model: "model".into(),
                env_key: None,
                context_window: None,
                max_output_tokens: None,
                reasoning: Some(false),
            },
            None,
        )
        .unwrap();
        crate::resources::model::apply_credential_update("work", Some("secret")).unwrap();

        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "opencode".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec!["work".into()],
            },
        })
        .unwrap();

        match plan.domain_plan {
            DomainPlan::Model { after, .. } => {
                let selection = after.get("opencode").expect("opencode selection");
                assert_eq!(selection.default_delivery, ApiKeyDelivery::Plaintext);
                assert_eq!(
                    selection.profiles["work"].credential.delivery,
                    ApiKeyDelivery::Plaintext
                );
            }
            other => panic!("expected model plan, got {other:?}"),
        }
    }

    #[test]
    fn configuration_plan_blocks_same_name_different_skill_content() {
        let home = TestHome::new("configuration-skill-conflict");
        fs::create_dir_all(home.home.join(".codex")).unwrap();
        fs::create_dir_all(home.home.join("Library/Application Support/Cursor")).unwrap();
        write_external_skill(
            &home.home.join(".cursor/skills"),
            "clash",
            "Private version",
        );
        write_external_skill(&home.home.join(".agents/skills"), "clash", "Shared version");
        let mut patch = current_configuration_patch("cursor").unwrap();
        patch.skill.as_mut().unwrap().global_dir = "~/.agents/skills".into();

        let error = plan_update_agent_capabilities(PlanUpdateAgentCapabilitiesRequest {
            agent_id: "cursor".into(),
            patch,
        })
        .unwrap_err();

        assert!(
            error.starts_with("skill_path_migration_conflict:"),
            "{error}"
        );
        assert_eq!(
            crate::agents::load_agents()["cursor"]
                .skills
                .as_ref()
                .unwrap()
                .global_dir,
            "~/.cursor/skills"
        );
    }
}
