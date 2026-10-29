//! Desired/observed central asset inventory.

use super::compatibility::{compatibility_for, McpCompatibilityResolver};
use super::model_migration::{
    exact_managed_model_observations_scoped, list_model_adoption_candidates_scoped, ModelAdoptionStatus,
};
use super::types::{
    AssetCapability, AssetRef, CapabilityDiagnostic, ConsumptionInventory, ConsumptionStatus,
    ConsumptionTarget, ConsumptionView, ConvergenceAction, OwnershipState,
};
use crate::resources::mcp::ops::scan_installed_scoped;
use crate::resources::model::{
    list_agents as list_model_agents, observe_active_model_for_settings, observe_external_model,
    observe_profile_consumption, ExternalModelObservedState, ModelObservedState,
    ObservedActiveModel,
};
use crate::resources::skill::{
    list_inventory as list_skills_inventory, InventoryState, SkillLocation, SkillsInventory,
};
use crate::settings::load_settings_strict;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// Read scope only: its revision identifies this projection and cannot replace
/// the full-inventory observed_revision required by convergence write plans.
/// Empty agent_ids includes every Agent; shared-target affected IDs remain intact.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InventoryScope {
    pub capability: Option<AssetCapability>,
    #[serde(default)]
    pub agent_ids: BTreeSet<String>,
}

impl InventoryScope {
    fn includes(&self, capability: AssetCapability) -> bool {
        self.capability.is_none_or(|selected| selected == capability)
    }

    fn includes_agent(&self, agent_id: &str) -> bool {
        self.agent_ids.is_empty() || self.agent_ids.contains(agent_id)
    }
}

pub fn list_consumption_inventory_scoped(scope: &InventoryScope) -> Result<ConsumptionInventory, String> {
    if scope == &InventoryScope::default() { return list_consumption_inventory(); }
    if !scope.includes(AssetCapability::Skill) {
        return list_consumption_inventory_inner(None, false, scope);
    }
    let skills = if scope.agent_ids.is_empty() {
        list_skills_inventory()
    } else {
        let settings = load_settings_strict().map_err(|error| error.to_string())?;
        crate::resources::skill::list_inventory_for_agents(&settings, &scope.agent_ids)
    };
    match skills {
        Ok(skills) => list_consumption_inventory_inner(Some(&skills), false, scope),
        Err(_) => list_consumption_inventory_inner(None, true, scope),
    }
}

pub fn list_consumption_inventory() -> Result<ConsumptionInventory, String> {
    list_inventory_with_skills().1
}

pub(crate) fn list_inventory_with_skills() -> (
    Result<SkillsInventory, crate::resources::skill::SkillError>,
    Result<ConsumptionInventory, String>,
) {
    let skills = list_skills_inventory();
    let relationships = match &skills {
        Ok(skills) => list_consumption_inventory_with_skills(skills),
        Err(_) => list_consumption_inventory_inner(None, true, &InventoryScope::default()),
    };
    (skills, relationships)
}

/// Build the relationship projection from an already loaded Skill inventory.
/// Workspace snapshots use this entry point so one refresh never scans Skill
/// targets twice.
pub fn list_consumption_inventory_with_skills(
    skills: &SkillsInventory,
) -> Result<ConsumptionInventory, String> {
    list_consumption_inventory_inner(Some(skills), false, &InventoryScope::default())
}

fn list_consumption_inventory_inner(
    skills: Option<&SkillsInventory>,
    skills_unavailable: bool,
    scope: &InventoryScope,
) -> Result<ConsumptionInventory, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let mut inventory = ConsumptionInventory {
        target_incidents: settings
            .target_incidents
            .as_ref()
            .into_iter()
            .flat_map(|incidents| incidents.values())
            .filter(|incident| scope.includes(incident.capability)
                && (scope.agent_ids.is_empty() || incident.affected_agent_ids.iter().any(|id| scope.includes_agent(id))))
            .cloned()
            .collect(),
        ..Default::default()
    };
    if scope.includes(AssetCapability::Mcp) {
        project_capability(&mut inventory, AssetCapability::Mcp, "mcp_observation_unavailable",
            |projection| project_mcps(&settings, projection, scope));
        project_host_resources(&mut inventory, scope);
    }
    if scope.includes(AssetCapability::Model) {
        project_capability(&mut inventory, AssetCapability::Model, "model_observation_unavailable",
            |projection| project_models(&settings, projection, scope));
    }
    if let Some(skills) = skills {
        project_capability(
            &mut inventory,
            AssetCapability::Skill,
            "skill_observation_unavailable",
            |projection| project_skills(&settings, skills, projection),
        );
        if skills.recovery_error.is_some() {
            inventory.capability_errors.push(CapabilityDiagnostic {
                capability: AssetCapability::Skill,
                code: "skill_recovery_required".into(),
            });
        }
    } else if skills_unavailable {
        inventory.capability_errors.push(CapabilityDiagnostic {
            capability: AssetCapability::Skill,
            code: "skill_inventory_unavailable".into(),
        });
    }
    inventory.consumptions.retain(|row| scope.includes_agent(&row.agent_id));
    inventory.external.retain(|row| scope.includes_agent(&row.agent_id));
    inventory.host_mcps.retain(|row| scope.includes_agent(&row.agent_id));
    inventory.native_plugins.retain(|row| scope.includes_agent(&row.agent_id));
    sort_inventory(&mut inventory);
    finalize_observation(&mut inventory);
    if scope != &InventoryScope::default() {
        let scoped = serde_json::to_vec(&("inventory-scope-v1", scope, &inventory.revision))
            .map_err(|error| error.to_string())?;
        inventory.revision = hex::encode(Sha256::digest(scoped));
    }
    Ok(inventory)
}

/// Build each capability in an isolated scratch projection. A reader is free
/// to discover malformed or concurrently changing Agent state, but it cannot
/// leave partial rows behind or prevent the other two capabilities from being
/// observed.
pub fn list_native_resources(agent_id: &str) -> crate::domain::plugin::NativeResourceInventory {
    let mut inventory = ConsumptionInventory::default();
    let scope = InventoryScope { capability: Some(AssetCapability::Mcp), agent_ids: BTreeSet::from([agent_id.to_string()]) };
    project_host_resources(&mut inventory, &scope);
    sort_inventory(&mut inventory);
    crate::domain::plugin::NativeResourceInventory {
        native_plugin_agents: inventory.native_plugin_agents,
        host_mcps: inventory.host_mcps,
        native_plugins: inventory.native_plugins,
        unavailable: !inventory.capability_errors.is_empty(),
    }
}

fn project_host_resources(inventory: &mut ConsumptionInventory, scope: &InventoryScope) {
    use crate::domain::plugin::{HostMcpObservation, HostResourceOrigin, PluginPresence, NativePluginObservation, PluginComponent, PluginComponentKind};
    let preferences = match crate::settings::load_agent_preferences() {
        Ok(preferences) => preferences,
        Err(_) => {
            inventory.capability_errors.push(CapabilityDiagnostic { capability: AssetCapability::Mcp, code: "mcp_host_observation_unavailable".into() });
            return;
        }
    };
    let mut agents = crate::agents::load_agents_from_settings(&preferences);
    agents.retain(|id, _| scope.includes_agent(id));
    inventory.native_plugin_agents = crate::resources::plugin::supported_agents(&agents);
    let bundled = crate::resources::mcp::builtin::observe(&agents);
    for item in bundled.items {
        inventory.native_plugins.push(NativePluginObservation {
            agent_id: item.agent_id.clone(), plugin_id: format!("builtin:{}", item.plugin_id),
            name: item.name.clone(), version: item.plugin_version, revision: item.app_version.clone(),
            presence: PluginPresence::Bundled, enabled: None, source_paths: vec![item.manifest_path],
            components: vec![PluginComponent { kind: PluginComponentKind::Mcp, name: item.name.clone(), source_path: item.source_path.clone(), native_id: Some(item.server_id.clone()) }],
            docs_url: item.docs_url.clone(), source_fingerprint: item.source_fingerprint.clone(),
        });
        inventory.host_mcps.push(HostMcpObservation {
            agent_id: item.agent_id, server_id: item.server_id, name: item.name, provider: item.provider,
            origin: HostResourceOrigin::Builtin, presence: PluginPresence::Bundled,
            versions: vec![item.app_version], source_paths: vec![item.source_path],
            docs_url: item.docs_url, source_fingerprint: item.source_fingerprint,
        });
    }
    let plugins = crate::resources::plugin::observe(&agents);
    inventory.native_plugins.extend(plugins.plugins);
    inventory.host_mcps.extend(plugins.mcps);
    if bundled.unavailable || plugins.unavailable {
        inventory.capability_errors.push(CapabilityDiagnostic {
            capability: AssetCapability::Mcp,
            code: "mcp_host_observation_unavailable".into(),
        });
    }
}

fn project_capability(
    inventory: &mut ConsumptionInventory,
    capability: AssetCapability,
    error_code: &str,
    project: impl FnOnce(&mut ConsumptionInventory) -> Result<(), String>,
) {
    let mut projection = ConsumptionInventory::default();
    match project(&mut projection) {
        Ok(()) => {
            inventory.consumptions.extend(projection.consumptions);
            inventory.external.extend(projection.external);
            inventory
                .capability_errors
                .extend(projection.capability_errors);
        }
        Err(_) => inventory.capability_errors.push(CapabilityDiagnostic {
            capability,
            code: error_code.into(),
        }),
    }
}

fn project_mcps(
    settings: &crate::settings::Settings,
    inventory: &mut ConsumptionInventory,
    scope: &InventoryScope,
) -> Result<(), String> {
    let observed = scan_installed_scoped(None, &scope.agent_ids);
    let mcp_compatibility = McpCompatibilityResolver::load();
    let mut consumed_observations = BTreeSet::new();

    for (agent_id, records) in settings.mcp_consumptions.iter().flatten() {
        if !scope.includes_agent(agent_id) { continue; }
        for (map_key, record) in records {
            let asset = AssetRef::Mcp {
                key: record.asset_key.clone(),
            };
            let compatibility = mcp_compatibility.resolve(agent_id, &record.asset_key).ok();
            let affected_agent_ids = compatibility
                .as_ref()
                .map(|view| view.affected_agent_ids.clone())
                .filter(|agents| !agents.is_empty())
                .unwrap_or_else(|| vec![agent_id.clone()]);
            let identity_matches = map_key == &record.asset_key;
            let match_index = observed.iter().position(|item| {
                item.agent == *agent_id
                    && item.scope == "global"
                    && format!("{}::{}", item.name, item.transport) == record.asset_key
            });
            let matching = match_index.map(|index| &observed[index]);
            let (status, reason, is_observed) = if compatibility.is_none() {
                (
                    ConsumptionStatus::Ambiguous,
                    Some("mcp_asset_identity_invalid".into()),
                    matching.is_some(),
                )
            } else if !identity_matches {
                (
                    ConsumptionStatus::Ambiguous,
                    Some("mcp_record_identity_mismatch".into()),
                    matching.is_some(),
                )
            } else if !mcp_compatibility.contains_asset(&record.asset_key) {
                (
                    ConsumptionStatus::Ambiguous,
                    Some("mcp_asset_missing".into()),
                    matching.is_some(),
                )
            } else if !compatibility.as_ref().is_some_and(|view| view.compatible) {
                (
                    ConsumptionStatus::Unsupported,
                    compatibility.and_then(|view| view.reason.map(|reason| reason.code)),
                    matching.is_some(),
                )
            } else {
                match matching {
                    None => (
                        ConsumptionStatus::ExternalRemoved,
                        Some("mcp_target_missing".into()),
                        false,
                    ),
                    Some(item) if item.enabled != record.enabled => (
                        ConsumptionStatus::ExternalChanged,
                        Some("mcp_enabled_state_drift".into()),
                        true,
                    ),
                    Some(item) if item.customized => (
                        ConsumptionStatus::ExternalChanged,
                        Some("mcp_config_drift".into()),
                        true,
                    ),
                    Some(_) => (ConsumptionStatus::Synced, None, true),
                }
            };
            if let Some(index) = match_index {
                consumed_observations.insert(index);
            }
            inventory.consumptions.push(ConsumptionView {
                agent_id: agent_id.clone(),
                asset,
                ownership: OwnershipState::Managed,
                desired: true,
                observed: is_observed,
                enabled: Some(record.enabled),
                observed_enabled: matching.map(|item| item.enabled),
                active: None,
                desired_active: None,
                status,
                reason,
                observation_id: Some(format!(
                    "mcp:{agent_id}:{}:{}",
                    record.asset_key,
                    matching
                        .map(|item| item.observation_fingerprint.as_str())
                        .unwrap_or("missing")
                )),
                available_actions: Vec::new(),
                affected_agent_ids,
                target: None,
            });
        }
    }

    for (index, item) in observed.into_iter().enumerate() {
        if consumed_observations.contains(&index) || item.scope != "global" {
            continue;
        }
        let key = format!("{}::{}", item.name, item.transport);
        inventory.external.push(ConsumptionView {
            agent_id: item.agent.clone(),
            asset: AssetRef::Mcp { key: key.clone() },
            ownership: OwnershipState::External,
            desired: false,
            observed: true,
            enabled: Some(item.enabled),
            observed_enabled: Some(item.enabled),
            active: None,
            desired_active: None,
            status: ConsumptionStatus::ExternalAdded,
            reason: Some(
                if mcp_compatibility.contains_asset(&key) && !item.customized {
                    "mcp_adoptable"
                } else if mcp_compatibility.contains_asset(&key) {
                    "mcp_external_customized"
                } else {
                    "mcp_external_unmanaged"
                }
                .into(),
            ),
            observation_id: Some(format!(
                "mcp:{}:{key}:{}",
                item.agent, item.observation_fingerprint
            )),
            available_actions: Vec::new(),
            affected_agent_ids: vec![item.agent],
            target: None,
        });
    }
    Ok(())
}

fn project_models(
    settings: &crate::settings::Settings,
    inventory: &mut ConsumptionInventory,
    scope: &InventoryScope,
) -> Result<(), String> {
    let model_agents = list_model_agents().into_iter().filter(|agent| scope.includes_agent(&agent.id)).collect::<Vec<_>>();
    let adoption_candidates = list_model_adoption_candidates_scoped(&scope.agent_ids)?;
    let exact_managed_observations = exact_managed_model_observations_scoped(settings, &scope.agent_ids)?;
    let observation_fingerprints: BTreeMap<_, _> = model_agents
        .iter()
        .map(|agent| {
            (
                agent.id.clone(),
                file_set_observation_fingerprint(&agent.config_paths),
            )
        })
        .collect();
    let assigned_agents: BTreeSet<String> = settings
        .model_consumptions
        .iter()
        .flatten()
        .map(|(agent_id, _)| agent_id.clone())
        .chain(
            settings
                .model_assignments
                .iter()
                .flatten()
                .map(|(agent_id, _)| agent_id.clone()),
        )
        .filter(|id| scope.includes_agent(id))
        .collect();
    let mut fallback_assigned_external = Vec::new();
    for agent_id in &assigned_agents {
        let observation_fingerprint = observation_fingerprints
            .get(agent_id.as_str())
            .map(String::as_str)
            .unwrap_or("unknown");
        let selection = settings.model_selection(agent_id);
        let observed_active = observe_active_model_for_settings(settings, agent_id);
        let observed_active_profile = match &observed_active {
            ObservedActiveModel::Managed(profile_id) => Some(profile_id.as_str()),
            _ => None,
        };
        for (profile_id, record) in &selection.profiles {
            let asset = AssetRef::Model {
                profile_id: profile_id.clone(),
            };
            let compatibility = compatibility_for(agent_id, &asset).ok();
            let Some(profile) = settings
                .model_profiles
                .as_ref()
                .and_then(|profiles| profiles.get(profile_id))
            else {
                inventory.consumptions.push(ConsumptionView {
                    agent_id: agent_id.clone(),
                    asset,
                    ownership: OwnershipState::Managed,
                    desired: true,
                    observed: false,
                    enabled: Some(record.enabled),
                    observed_enabled: None,
                    active: Some(observed_active_profile == Some(profile_id)),
                    desired_active: Some(
                        selection.active_profile_id.as_deref() == Some(profile_id.as_str()),
                    ),
                    status: ConsumptionStatus::Ambiguous,
                    reason: Some("model_profile_missing".into()),
                    observation_id: Some(format!(
                        "model:{agent_id}:{profile_id}:{observation_fingerprint}"
                    )),
                    available_actions: Vec::new(),
                    affected_agent_ids: vec![agent_id.clone()],
                    target: None,
                });
                continue;
            };
            let desired_active =
                selection.active_profile_id.as_deref() == Some(profile_id.as_str());
            let observed_is_active = observed_active_profile == Some(profile_id);
            let (observed, status, reason) = if compatibility.is_none() {
                (
                    false,
                    ConsumptionStatus::Ambiguous,
                    Some("model_asset_identity_invalid".into()),
                )
            } else if record.enabled && !compatibility.as_ref().is_some_and(|view| view.compatible)
            {
                (
                    false,
                    ConsumptionStatus::Unsupported,
                    compatibility.and_then(|view| view.reason.map(|reason| reason.code)),
                )
            } else {
                let observed_profile =
                    observe_profile_consumption(agent_id, profile, observed_is_active);
                let state = match (record.enabled, observed_profile) {
                    (_, Err(_)) => (
                        true,
                        ConsumptionStatus::Unparseable,
                        Some("model_target_unparseable".into()),
                    ),
                    (true, Ok(ModelObservedState::Synced)) => {
                        (true, ConsumptionStatus::Synced, None)
                    }
                    (true, Ok(ModelObservedState::Missing)) => (
                        false,
                        ConsumptionStatus::ExternalRemoved,
                        Some("model_target_missing".into()),
                    ),
                    (true, Ok(ModelObservedState::Drifted))
                        if exact_managed_observations
                            .contains(&(agent_id.clone(), profile_id.clone())) =>
                    {
                        (true, ConsumptionStatus::Synced, None)
                    }
                    (true, Ok(ModelObservedState::Drifted)) => (
                        true,
                        ConsumptionStatus::ExternalChanged,
                        Some("model_owned_fields_drift".into()),
                    ),
                    (true, Ok(ModelObservedState::Conflicted)) => (
                        true,
                        ConsumptionStatus::Ambiguous,
                        Some("model_target_conflicted".into()),
                    ),
                    (false, Ok(ModelObservedState::Missing)) => {
                        (false, ConsumptionStatus::Synced, None)
                    }
                    (false, Ok(ModelObservedState::Conflicted)) => (
                        true,
                        ConsumptionStatus::Ambiguous,
                        Some("model_target_conflicted".into()),
                    ),
                    (false, Ok(_)) => (
                        true,
                        ConsumptionStatus::ExternalChanged,
                        Some("model_disabled_state_drift".into()),
                    ),
                };
                if state.1 == ConsumptionStatus::Synced && desired_active != observed_is_active {
                    (
                        state.0,
                        ConsumptionStatus::ExternalChanged,
                        Some("model_active_state_drift".into()),
                    )
                } else {
                    state
                }
            };
            let available_actions = if status == ConsumptionStatus::ExternalChanged {
                let can_adopt = reason.as_deref() == Some("model_active_state_drift")
                    || adoption_candidates.iter().any(|candidate| {
                        candidate.agent_id == agent_id.as_str()
                            && candidate.managed_profile_id.as_deref() == Some(profile_id.as_str())
                            && candidate.status == ModelAdoptionStatus::Adoptable
                    });
                let mut actions =
                    vec![ConvergenceAction::RestoreDesired, ConvergenceAction::Detach];
                if can_adopt {
                    actions.insert(0, ConvergenceAction::AdoptObserved);
                }
                actions
            } else {
                Vec::new()
            };
            inventory.consumptions.push(ConsumptionView {
                agent_id: agent_id.clone(),
                asset,
                ownership: OwnershipState::Managed,
                desired: true,
                observed,
                enabled: Some(record.enabled),
                observed_enabled: Some(observed),
                active: Some(observed_is_active),
                desired_active: Some(desired_active),
                status,
                reason,
                observation_id: Some(format!(
                    "model:{agent_id}:{profile_id}:{observation_fingerprint}"
                )),
                available_actions,
                affected_agent_ids: vec![agent_id.clone()],
                target: None,
            });
        }
        let external_reason = match observed_active {
            ObservedActiveModel::External => {
                Some((ConsumptionStatus::ExternalAdded, "model_external_current"))
            }
            ObservedActiveModel::Conflicted => {
                Some((ConsumptionStatus::Ambiguous, "model_active_conflicted"))
            }
            _ => None,
        };
        if let Some((status, reason)) = external_reason {
            fallback_assigned_external.push((
                agent_id.clone(),
                status,
                reason.to_string(),
                observation_fingerprint.to_string(),
            ));
        }
    }

    let candidate_agents: BTreeSet<_> = adoption_candidates
        .iter()
        .map(|candidate| candidate.agent_id.clone())
        .collect();
    for candidate in adoption_candidates
        .iter()
        .filter(|candidate| candidate.managed_profile_id.is_none())
    {
        let (status, fallback_reason) = match candidate.status {
            ModelAdoptionStatus::Adoptable => (
                ConsumptionStatus::ExternalAdded,
                if candidate.active {
                    "model_external_current"
                } else {
                    "model_external_unmanaged"
                },
            ),
            ModelAdoptionStatus::NeedsCredential | ModelAdoptionStatus::Unsupported => {
                (ConsumptionStatus::Unsupported, "model_external_unsupported")
            }
            ModelAdoptionStatus::Conflicted => {
                (ConsumptionStatus::Ambiguous, "model_external_conflicted")
            }
        };
        inventory.external.push(ConsumptionView {
            agent_id: candidate.agent_id.clone(),
            asset: AssetRef::Model {
                profile_id: format!("external-{}", candidate.candidate_id),
            },
            ownership: OwnershipState::External,
            desired: false,
            observed: true,
            enabled: None,
            observed_enabled: candidate.enabled.or(Some(true)),
            active: Some(candidate.active),
            desired_active: Some(false),
            status,
            reason: Some(fallback_reason.into()),
            observation_id: Some(format!(
                "model:{}:{}:{}",
                candidate.agent_id, candidate.candidate_id, candidate.fingerprint
            )),
            available_actions: Vec::new(),
            affected_agent_ids: vec![candidate.agent_id.clone()],
            target: None,
        });
    }

    for (agent_id, status, reason, observation_fingerprint) in fallback_assigned_external {
        if candidate_agents.contains(&agent_id) {
            continue;
        }
        inventory.external.push(ConsumptionView {
            agent_id: agent_id.clone(),
            asset: AssetRef::Model {
                profile_id: format!("external-{agent_id}"),
            },
            ownership: OwnershipState::External,
            desired: false,
            observed: true,
            enabled: None,
            observed_enabled: Some(true),
            active: Some(true),
            desired_active: Some(false),
            status,
            reason: Some(reason),
            observation_id: Some(format!("model:{agent_id}:active:{observation_fingerprint}")),
            available_actions: Vec::new(),
            affected_agent_ids: vec![agent_id],
            target: None,
        });
    }

    for agent in model_agents.into_iter().filter(|agent| {
        agent.mode == "managed"
            && !assigned_agents.contains(&agent.id)
            && !candidate_agents.contains(&agent.id)
    }) {
        let (status, reason) = match observe_external_model(&agent.id) {
            Err(_) => (
                ConsumptionStatus::Unparseable,
                Some("model_target_unparseable".into()),
            ),
            Ok(ExternalModelObservedState::Absent) => continue,
            Ok(ExternalModelObservedState::Present) => (
                ConsumptionStatus::ExternalAdded,
                Some("model_external_unmanaged".into()),
            ),
            Ok(ExternalModelObservedState::Conflicted) => (
                ConsumptionStatus::Ambiguous,
                Some("model_external_conflicted".into()),
            ),
        };
        inventory.external.push(ConsumptionView {
            agent_id: agent.id.clone(),
            asset: AssetRef::Model {
                profile_id: format!("external-{}", agent.id),
            },
            ownership: OwnershipState::External,
            desired: false,
            observed: true,
            enabled: None,
            observed_enabled: Some(true),
            active: Some(true),
            desired_active: Some(false),
            status,
            reason,
            observation_id: Some(format!(
                "model:{}:active:{}",
                agent.id,
                observation_fingerprints
                    .get(agent.id.as_str())
                    .map(String::as_str)
                    .unwrap_or("unknown")
            )),
            available_actions: Vec::new(),
            affected_agent_ids: vec![agent.id],
            target: None,
        });
    }
    Ok(())
}

fn file_set_observation_fingerprint(paths: &[String]) -> String {
    let mut paths = paths.to_vec();
    paths.sort();
    let mut hasher = Sha256::new();
    for raw in paths {
        hasher.update(raw.len().to_le_bytes());
        hasher.update(raw.as_bytes());
        let path = crate::resources::mcp::scanner::expand_tilde(&raw);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                hasher.update(b"symlink");
                match fs::read_link(&path) {
                    Ok(target) => hasher.update(target.as_os_str().as_encoded_bytes()),
                    Err(error) => hasher.update(error.kind().to_string().as_bytes()),
                }
            }
            Ok(metadata) if metadata.is_file() => {
                hasher.update(b"file");
                match fs::read(&path) {
                    Ok(bytes) => hasher.update(bytes),
                    Err(error) => hasher.update(error.kind().to_string().as_bytes()),
                }
            }
            Ok(metadata) if metadata.is_dir() => hasher.update(b"directory"),
            Ok(_) => hasher.update(b"special"),
            Err(error) => hasher.update(error.kind().to_string().as_bytes()),
        }
    }
    hex::encode(hasher.finalize())
}

fn project_skills(
    settings: &crate::settings::Settings,
    skills: &SkillsInventory,
    inventory: &mut ConsumptionInventory,
) -> Result<(), String> {
    let targets: BTreeMap<_, _> = skills
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect();
    let target_items: BTreeMap<_, _> = skills
        .items
        .iter()
        .filter_map(|item| match &item.location {
            SkillLocation::AgentTarget { target_id, .. } => {
                Some(((target_id.as_str(), item.name.as_str()), item))
            }
            SkillLocation::Central => None,
        })
        .collect();
    let mut canonical_assignments = BTreeMap::<String, BTreeSet<String>>::new();
    for item in &skills.items {
        canonical_assignments
            .entry(item.name.clone())
            .or_default()
            .extend(item.assigned_target_ids.iter().cloned());
    }
    let mut desired_physical = BTreeSet::new();

    for (name, _) in settings.skill_assignments.iter().flatten() {
        let target_ids = canonical_assignments.get(name).cloned().unwrap_or_default();
        for target_id in target_ids {
            desired_physical.insert((target_id.clone(), name.clone()));
            let enabled = settings
                .skill_consumptions
                .as_ref()
                .and_then(|skills| skills.get(name))
                .and_then(|targets| targets.get(&target_id))
                .map(|record| record.enabled)
                .unwrap_or(true);
            let Some(target) = targets.get(target_id.as_str()) else {
                inventory.consumptions.push(ConsumptionView {
                    agent_id: target_id.clone(),
                    asset: AssetRef::Skill { name: name.clone() },
                    ownership: OwnershipState::Managed,
                    desired: true,
                    observed: false,
                    enabled: Some(enabled),
                    observed_enabled: None,
                    active: None,
                    desired_active: None,
                    status: ConsumptionStatus::Ambiguous,
                    reason: Some("skill_target_unknown".into()),
                    observation_id: Some(format!("skill:{target_id}:{name}")),
                    available_actions: Vec::new(),
                    affected_agent_ids: Vec::new(),
                    target: None,
                });
                continue;
            };
            let physical = target_items.get(&(target_id.as_str(), name.as_str()));
            let (physical_observed, physical_status, physical_reason) = match physical {
                None => (
                    false,
                    ConsumptionStatus::ExternalRemoved,
                    Some("skill_target_missing".into()),
                ),
                Some(item) if item.states.contains(&InventoryState::BrokenLink) => (
                    true,
                    ConsumptionStatus::ExternalRemoved,
                    Some("skill_broken_link".into()),
                ),
                Some(item) if item.states.contains(&InventoryState::ConflictingLink) => (
                    true,
                    ConsumptionStatus::Ambiguous,
                    Some("skill_conflicting_link".into()),
                ),
                Some(item) if item.states.contains(&InventoryState::LocallyModified) => (
                    true,
                    ConsumptionStatus::ExternalChanged,
                    Some("skill_local_modification".into()),
                ),
                Some(item) if item.states.contains(&InventoryState::External) => (
                    true,
                    ConsumptionStatus::ExternalChanged,
                    Some("skill_external_target".into()),
                ),
                Some(item) if item.states.contains(&InventoryState::Missing) => (
                    false,
                    ConsumptionStatus::ExternalRemoved,
                    Some("skill_target_missing".into()),
                ),
                Some(_) => (true, ConsumptionStatus::Synced, None),
            };
            let (observed, status, reason) = if enabled {
                (physical_observed, physical_status, physical_reason)
            } else if !physical_observed {
                (false, ConsumptionStatus::Synced, None)
            } else {
                (
                    true,
                    if physical_status == ConsumptionStatus::Ambiguous {
                        ConsumptionStatus::Ambiguous
                    } else {
                        ConsumptionStatus::ExternalChanged
                    },
                    if physical.is_some_and(|item| item.states.contains(&InventoryState::Assigned)) {
                        Some("skill_disabled_state_drift".into())
                    } else {
                        // A disabled relation does not turn an independent
                        // directory or foreign link into a removable Arca link.
                        physical_reason
                    },
                )
            };
            let agents = if target.affected_agent_ids.is_empty() {
                target.primary_agent_ids.clone()
            } else {
                target.affected_agent_ids.clone()
            };
            for agent_id in &agents {
                inventory.consumptions.push(ConsumptionView {
                    agent_id: agent_id.clone(),
                    asset: AssetRef::Skill { name: name.clone() },
                    ownership: OwnershipState::Managed,
                    desired: true,
                    observed,
                    enabled: Some(enabled),
                    observed_enabled: Some(physical_observed),
                    active: None,
                    desired_active: None,
                    status: status.clone(),
                    reason: reason.clone(),
                    observation_id: Some(item_observation_id(
                        "skill",
                        &target_id,
                        name,
                        physical.and_then(|item| item.content_hash.as_deref()),
                    )),
                    available_actions: Vec::new(),
                    affected_agent_ids: agents.clone(),
                    target: Some(ConsumptionTarget {
                        target_id: target.target_id.clone(),
                        global_dir: target.global_dir.clone(),
                    }),
                });
            }
        }
    }

    for item in &skills.items {
        let SkillLocation::AgentTarget {
            target_id,
            global_dir,
        } = &item.location
        else {
            continue;
        };
        if desired_physical.contains(&(target_id.clone(), item.name.clone())) {
            continue;
        }
        let Some(target) = targets.get(target_id.as_str()) else {
            continue;
        };
        let agents = if target.affected_agent_ids.is_empty() {
            target.primary_agent_ids.clone()
        } else {
            target.affected_agent_ids.clone()
        };
        for agent_id in &agents {
            inventory.external.push(ConsumptionView {
                agent_id: agent_id.clone(),
                asset: AssetRef::Skill {
                    name: item.name.clone(),
                },
                ownership: OwnershipState::External,
                desired: false,
                observed: true,
                enabled: None,
                observed_enabled: Some(true),
                active: None,
                desired_active: None,
                status: ConsumptionStatus::ExternalAdded,
                reason: Some("skill_external".into()),
                observation_id: Some(item.identity.clone()),
                available_actions: Vec::new(),
                affected_agent_ids: agents.clone(),
                target: Some(ConsumptionTarget {
                    target_id: target_id.clone(),
                    global_dir: global_dir.clone(),
                }),
            });
        }
    }
    Ok(())
}

fn sort_inventory(inventory: &mut ConsumptionInventory) {
    inventory.host_mcps.sort_by(|a, b| (&a.agent_id, &a.server_id, &a.presence).cmp(&(&b.agent_id, &b.server_id, &b.presence)));
    inventory.host_mcps.dedup();
    inventory.native_plugins.sort_by(|a, b| (&a.agent_id, &a.plugin_id, &a.version, &a.revision, &a.presence).cmp(&(&b.agent_id, &b.plugin_id, &b.version, &b.revision, &b.presence)));
    inventory.native_plugins.dedup();
    let sort = |items: &mut Vec<ConsumptionView>| {
        items.sort_by(|left, right| {
            left.agent_id
                .cmp(&right.agent_id)
                .then_with(|| left.asset.cmp(&right.asset))
        });
        items.dedup();
    };
    sort(&mut inventory.consumptions);
    sort(&mut inventory.external);
    inventory.target_incidents.sort();
    inventory.target_incidents.dedup();
}

fn item_observation_id(
    domain: &str,
    target_id: &str,
    asset_id: &str,
    content_hash: Option<&str>,
) -> String {
    format!(
        "{domain}:{target_id}:{asset_id}:{}",
        content_hash.unwrap_or("missing")
    )
}

fn finalize_observation(inventory: &mut ConsumptionInventory) {
    for item in inventory
        .consumptions
        .iter_mut()
        .chain(inventory.external.iter_mut())
    {
        if !item.available_actions.is_empty() {
            continue;
        }
        item.available_actions = match (&item.ownership, &item.status) {
            (OwnershipState::Managed, ConsumptionStatus::ExternalChanged)
                if matches!(item.asset, AssetRef::Skill { .. }) => {
                // Offer only convergence paths the Skill planner can perform.
                // Replacing an independent external copy belongs to explicit
                // central import, not adoption of the central baseline.
                match item.reason.as_deref() {
                    Some("skill_local_modification") => vec![ConvergenceAction::AdoptObserved, ConvergenceAction::Detach],
                    Some("skill_disabled_state_drift") => vec![ConvergenceAction::RestoreDesired, ConvergenceAction::Detach],
                    _ => vec![ConvergenceAction::Detach],
                }
            }
            (OwnershipState::External, ConsumptionStatus::ExternalAdded) => {
                vec![ConvergenceAction::AdoptObserved]
            }
            (OwnershipState::Managed, ConsumptionStatus::ExternalChanged) => vec![
                ConvergenceAction::AdoptObserved,
                ConvergenceAction::RestoreDesired,
                ConvergenceAction::Detach,
            ],
            (OwnershipState::Managed, ConsumptionStatus::ExternalRemoved) => {
                vec![ConvergenceAction::RestoreDesired, ConvergenceAction::Detach]
            }
            (OwnershipState::Managed, ConsumptionStatus::Unparseable)
            | (OwnershipState::Managed, ConsumptionStatus::Ambiguous)
            | (OwnershipState::Managed, ConsumptionStatus::Unsupported) => {
                vec![ConvergenceAction::Detach]
            }
            _ => Vec::new(),
        };
    }
    let bytes = serde_json::to_vec(&(
        &inventory.native_plugin_agents,
        &inventory.host_mcps,
        &inventory.native_plugins,
        &inventory.consumptions,
        &inventory.external,
        &inventory.capability_errors,
        &inventory.target_incidents,
    ))
    .expect("observation projection serializes");
    inventory.revision = hex::encode(Sha256::digest(bytes));
    inventory.observed_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
}

#[cfg(test)]
mod projection_isolation_tests {
    use super::*;

    #[test]
    fn mcp_scope_skips_other_agent_inputs_and_has_its_own_revision() {
        let home = crate::testenv::TestHome::new("inventory-agent-scope");
        crate::settings::mutate_settings(|settings| {
            let agents = settings.agents.get_or_insert_default();
            for id in ["audit-one", "audit-two"] {
                agents.insert(id.into(), crate::domain::types::AgentDefinition {
                    global: Some(format!("~/{id}.json")), format: "json".into(), key: "mcpServers".into(),
                    enabled: true, ..Default::default()
                });
            }
        }).unwrap();
        fs::write(home.home.join("audit-one.json"), r#"{"mcpServers":{"first":{"command":"one"}}}"#).unwrap();
        fs::write(home.home.join("audit-two.json"), r#"{"mcpServers":{"second":{"command":"two"}}}"#).unwrap();
        let scope = InventoryScope { capability: Some(AssetCapability::Mcp), agent_ids: BTreeSet::from(["audit-one".into()]) };
        let before = list_consumption_inventory_scoped(&scope).unwrap();
        assert_eq!(before.external.len(), 1);
        assert_eq!(before.external[0].agent_id, "audit-one");
        fs::write(home.home.join("audit-two.json"), r#"{"mcpServers":{"changed":{"command":"three"}}}"#).unwrap();
        let after = list_consumption_inventory_scoped(&scope).unwrap();
        assert_eq!(before.revision, after.revision);
        let full = list_consumption_inventory().unwrap();
        assert_ne!(full.revision, after.revision);
    }

    #[test]
    fn a_failed_capability_discards_its_partial_projection_and_keeps_others() {
        let mut inventory = ConsumptionInventory::default();

        project_capability(
            &mut inventory,
            AssetCapability::Mcp,
            "mcp_observation_unavailable",
            |partial| {
                partial.capability_errors.push(CapabilityDiagnostic {
                    capability: AssetCapability::Skill,
                    code: "must_not_leak".into(),
                });
                Err("one Agent file changed during the read".into())
            },
        );
        project_capability(
            &mut inventory,
            AssetCapability::Model,
            "model_observation_unavailable",
            |partial| {
                partial.capability_errors.push(CapabilityDiagnostic {
                    capability: AssetCapability::Model,
                    code: "model_local_diagnostic".into(),
                });
                Ok(())
            },
        );

        assert_eq!(
            inventory.capability_errors,
            vec![
                CapabilityDiagnostic {
                    capability: AssetCapability::Mcp,
                    code: "mcp_observation_unavailable".into(),
                },
                CapabilityDiagnostic {
                    capability: AssetCapability::Model,
                    code: "model_local_diagnostic".into(),
                },
            ]
        );
    }
}
