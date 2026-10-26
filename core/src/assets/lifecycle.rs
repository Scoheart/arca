//! Central asset lifecycle planning.

use super::planner::{finalize_plan_with, CredentialAction, LifecycleBinding};
use super::types::{
    AssetOperationKind, AssetOperationPlan, AssetRef, CentralAssetAction, CentralAssetChange,
    CentralAssetDraft, DomainPlan, ModelAgentSelection, ModelConsumptionRecord,
    PlanDeleteCentralAssetRequest, PlanUpdateCentralAssetRequest,
};
use crate::domain::types::{ModelProfile, ModelProviderConfig, RegistryEntry};
use crate::paths::local_sources_dir;
use crate::resources::mcp::registry::{read_registry, read_registry_all};
use crate::resources::model::{
    credential_snapshot, migrated_profiles_v2, model_agent_capability, prepare_profile_draft,
    prepare_provider_draft, profile_credential_issue, profile_endpoint_compatibility_issue,
    provider_credential_present, provider_profiles, restore_credential_snapshot,
};
use crate::settings::{load_settings_strict, Settings};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use zeroize::Zeroizing;

#[cfg(test)]
use super::types::AssetCommitRequest;

#[derive(Clone)]
pub(crate) enum PendingAssetPayload {
    Mcp {
        entry: Box<RegistryEntry>,
    },
    Model {
        profile: Box<ModelProfile>,
        credential: Option<Zeroizing<String>>,
    },
    ModelProvider {
        provider: Box<ModelProviderConfig>,
        profiles: BTreeMap<String, ModelProfile>,
        credential: Option<Zeroizing<String>>,
    },
}

static PENDING_PAYLOADS: LazyLock<Mutex<BTreeMap<String, PendingAssetPayload>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub(crate) fn pending_payload(operation_id: &str) -> Option<PendingAssetPayload> {
    PENDING_PAYLOADS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(operation_id)
        .cloned()
}

pub(crate) fn clear_pending_payload(operation_id: &str) {
    super::lease::release(operation_id);
    PENDING_PAYLOADS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(operation_id);
}

pub(crate) fn store_pending_mcp_entry(operation_id: &str, entry: RegistryEntry) {
    PENDING_PAYLOADS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            operation_id.to_string(),
            PendingAssetPayload::Mcp {
                entry: Box::new(entry),
            },
        );
}

pub(crate) fn store_pending_model_profile(
    operation_id: &str,
    profile: ModelProfile,
    credential: Option<String>,
) {
    store_pending_model_profile_secret(operation_id, profile, credential.map(Zeroizing::new));
}

pub(crate) fn store_pending_model_profile_secret(
    operation_id: &str,
    profile: ModelProfile,
    credential: Option<Zeroizing<String>>,
) {
    PENDING_PAYLOADS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            operation_id.to_string(),
            PendingAssetPayload::Model {
                profile: Box::new(profile),
                credential,
            },
        );
}

fn remap_model_selection(
    selection: ModelAgentSelection,
    id_map: &BTreeMap<String, String>,
) -> Result<ModelAgentSelection, String> {
    let profiles = selection
        .profiles
        .into_iter()
        .map(|(old_id, record)| {
            let new_id = id_map
                .get(&old_id)
                .ok_or_else(|| format!("model_schema_migration_missing_profile: {old_id}"))?
                .clone();
            Ok((
                new_id.clone(),
                ModelConsumptionRecord {
                    profile_id: new_id,
                    enabled: record.enabled,
                    credential: record.credential,
                    last_selected_at: record.last_selected_at,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let active_profile_id = selection
        .active_profile_id
        .map(|old_id| {
            id_map
                .get(&old_id)
                .cloned()
                .ok_or_else(|| format!("model_schema_migration_missing_profile: {old_id}"))
        })
        .transpose()?;
    Ok(ModelAgentSelection {
        profiles,
        active_profile_id,
        default_delivery: selection.default_delivery,
    })
}

pub fn migrate_model_profiles_v2_if_needed() -> Result<bool, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if settings.version.unwrap_or_default() >= 2 {
        return Ok(false);
    }
    let (id_map, profiles) = migrated_profiles_v2(&settings)?;
    let agent_ids = settings
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
        .collect::<BTreeSet<_>>();
    let selections = agent_ids
        .into_iter()
        .map(|agent_id| {
            remap_model_selection(settings.model_selection(&agent_id), &id_map)
                .map(|selection| (agent_id, selection))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;

    // Copy credentials before publishing the new central identities. Legacy
    // subjects are intentionally retained: an Agent file that still references
    // one is observed external state, not migration damage.
    for (old_id, new_id) in &id_map {
        if let Some(credential) = credential_snapshot(old_id)? {
            restore_credential_snapshot(new_id, Some(&credential))?;
        }
    }
    crate::settings::mutate_settings_checked(|current| {
        if current.version != settings.version
            || current.model_profiles != settings.model_profiles
            || current.model_consumptions != settings.model_consumptions
            || current.model_assignments != settings.model_assignments
        {
            return Err(std::io::Error::other(
                "asset_operation_stale: Model settings changed during schema migration",
            ));
        }
        current.model_profiles = (!profiles.is_empty()).then_some(profiles.clone());
        current.model_consumptions = None;
        current.model_assignments = None;
        for (agent_id, selection) in &selections {
            current.set_model_selection(agent_id, selection.clone());
        }
        current.version = Some(2);
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(true)
}

pub fn plan_update_central_asset(
    request: PlanUpdateCentralAssetRequest,
) -> Result<AssetOperationPlan, String> {
    match request.draft {
        CentralAssetDraft::Mcp {
            existing_key,
            entry,
        } => plan_mcp_upsert(existing_key, *entry),
        CentralAssetDraft::Model {
            existing_id,
            profile,
            credential,
        } => plan_model_upsert(existing_id, *profile, credential),
        CentralAssetDraft::ModelProvider {
            existing_id,
            provider,
            credential,
        } => plan_model_provider_upsert(existing_id, *provider, credential),
    }
}

pub fn plan_delete_central_asset(
    request: PlanDeleteCentralAssetRequest,
) -> Result<AssetOperationPlan, String> {
    request
        .asset
        .validate()
        .map_err(|error| error.to_string())?;
    match request.asset {
        AssetRef::Mcp { key } => plan_mcp_delete(key, request.source_id),
        AssetRef::Model { profile_id } => {
            if request.source_id.is_some() {
                return Err(
                    "invalid_asset_source: Model assets do not have Registry sources".into(),
                );
            }
            plan_model_delete(profile_id)
        }
        AssetRef::ModelProvider { provider_id } => {
            if request.source_id.is_some() {
                return Err(
                    "invalid_asset_source: Model Provider assets do not have Registry sources"
                        .into(),
                );
            }
            plan_model_provider_delete(provider_id)
        }
        AssetRef::Skill { .. } => Err(
            "unsupported_asset_lifecycle: Skills use the verified Skill update/remove transaction"
                .into(),
        ),
    }
}

fn plan_mcp_upsert(
    existing_key: Option<String>,
    mut entry: RegistryEntry,
) -> Result<AssetOperationPlan, String> {
    validate_mcp_entry(&entry)?;
    let key = entry.key();
    let effective = read_registry();
    let mut previous_key = None;
    let mut previous_source_id = None;
    let action = match existing_key.as_deref() {
        Some(existing_key) if existing_key == key => {
            let existing = effective
                .iter()
                .find(|candidate| candidate.key() == key)
                .ok_or_else(|| {
                    "asset_operation_stale: central MCP asset no longer exists".to_string()
                })?;
            if is_user_owned(existing) {
                CentralAssetAction::Update
            } else {
                // Remote/local sources remain immutable. Editing one creates a
                // higher-priority manual entry with the same composite key.
                CentralAssetAction::Create
            }
        }
        Some(existing_key) => {
            let existing_transport = existing_key
                .rsplit_once("::")
                .map(|(_, transport)| transport)
                .ok_or_else(|| "invalid_asset: MCP identity is malformed".to_string())?;
            let new_transport = key
                .rsplit_once("::")
                .map(|(_, transport)| transport)
                .ok_or_else(|| "invalid_asset: MCP identity is malformed".to_string())?;
            if existing_transport != new_transport {
                return Err(
                    "asset_transport_change: MCP transport cannot change during a rename".into(),
                );
            }
            let existing = effective
                .iter()
                .find(|candidate| candidate.key() == existing_key)
                .ok_or_else(|| {
                    "asset_operation_stale: central MCP asset no longer exists".to_string()
                })?;
            if effective.iter().any(|candidate| candidate.key() == key) {
                return Err(
                    "asset_identity_conflict: a central MCP with this name and transport already exists"
                        .into(),
                );
            }
            if !is_user_owned(existing) {
                return Err(
                    "asset_read_only: source-backed MCPs cannot be renamed; create a manual MCP instead"
                        .into(),
                );
            }
            let copies = read_registry_all()
                .into_iter()
                .filter(|item| item.entry.key() == existing_key)
                .collect::<Vec<_>>();
            if copies.len() != 1 {
                return Err(
                    "asset_identity_rename_conflict: the old MCP identity has another source copy; remove the fallback before renaming"
                        .into(),
                );
            }
            let source_id = source_id_for(existing);
            if !matches!(source_id.as_str(), "manual" | "discovered") {
                return Err("asset_read_only: MCP source copy is not user-owned".into());
            }
            previous_key = Some(existing_key.to_string());
            previous_source_id = Some(source_id);
            CentralAssetAction::Create
        }
        None => {
            if effective.iter().any(|candidate| candidate.key() == key) {
                return Err("asset_identity_conflict: a central MCP with this name and transport already exists".into());
            }
            CentralAssetAction::Create
        }
    };
    // The central writer owns provenance. Keeping a stale discovered origin in
    // the pending payload would make review hashes depend on a field it replaces.
    entry.origin = None;

    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let domain_plan = previous_key
        .as_deref()
        .map(|old_key| mcp_renamed_consumers(&settings, old_key, &key))
        .unwrap_or_else(|| mcp_unchanged_consumers(&settings, &key));
    let consumer_count = domain_agent_count(&domain_plan);
    let draft_hash = hash_serializable(&entry)?;
    let lifecycle = LifecycleBinding::McpUpsert {
        key: key.clone(),
        draft_hash,
        previous_key: previous_key.clone(),
        previous_source_id: previous_source_id.clone(),
    };
    let central_changes = if let Some(old_key) = previous_key.as_ref() {
        vec![
            CentralAssetChange {
                asset: AssetRef::Mcp {
                    key: old_key.clone(),
                },
                action: CentralAssetAction::Delete,
                summary: vec![format!("将 MCP 重命名为 {key}")],
            },
            CentralAssetChange {
                asset: AssetRef::Mcp { key: key.clone() },
                action: CentralAssetAction::Create,
                summary: vec![
                    format!("从 {old_key} 重命名"),
                    agent_sync_summary(consumer_count),
                ],
            },
        ]
    } else {
        vec![CentralAssetChange {
            asset: AssetRef::Mcp { key: key.clone() },
            action: action.clone(),
            summary: central_upsert_summary("中央 MCP 配置", &action, consumer_count),
        }]
    };
    let mut target_files = vec![display_path(&local_sources_dir().join("manual.json"))];
    if previous_source_id.as_deref() == Some("discovered") {
        target_files.push(display_path(&local_sources_dir().join("discovered.json")));
    }
    let plan = finalize_plan_with(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        central_changes,
        target_files,
        Some(lifecycle),
    )?;
    store_pending_mcp_entry(&plan.operation_id, entry);
    Ok(plan)
}

fn plan_model_upsert(
    existing_id: Option<String>,
    profile: ModelProfile,
    credential: Option<String>,
) -> Result<AssetOperationPlan, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if profile.provider_id.as_deref().is_none_or(str::is_empty) {
        return Err("model_provider_required: create a Provider before adding a Model".into());
    }
    if credential.is_some() {
        return Err(
            "model_credential_owned_by_provider: edit the Provider to change its API Key".into(),
        );
    }
    let profile = prepare_profile_draft(&settings, existing_id.as_deref(), profile)?;
    let existing = settings
        .model_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(&profile.id));
    let action = match existing_id {
        Some(_) => {
            if existing.is_none() {
                return Err("asset_operation_stale: Model Profile no longer exists".into());
            }
            CentralAssetAction::Update
        }
        None => {
            if existing.is_some() {
                return Err(
                    "asset_identity_conflict: a Model Profile with this id already exists".into(),
                );
            }
            CentralAssetAction::Create
        }
    };

    let credential_action = CredentialAction::Keep;
    let desired_credential_present = profile
        .provider_id
        .as_deref()
        .is_some_and(provider_credential_present);
    let domain_plan = model_unchanged_consumers(&settings, &profile, desired_credential_present)?;
    let consumer_count = domain_agent_count(&domain_plan);
    let draft_hash = hash_serializable(&profile)?;
    let lifecycle = LifecycleBinding::ModelUpsert {
        profile_id: profile.id.clone(),
        draft_hash,
        credential_action: credential_action.clone(),
    };
    let summary = model_upsert_summary(&action, &credential_action, consumer_count);
    let plan = finalize_plan_with(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Model {
                profile_id: profile.id.clone(),
            },
            action,
            summary,
        }],
        Vec::new(),
        Some(lifecycle),
    )?;
    store_pending_model_profile(&plan.operation_id, profile, credential);
    Ok(plan)
}

fn plan_model_provider_upsert(
    existing_id: Option<String>,
    provider: ModelProviderConfig,
    credential: Option<String>,
) -> Result<AssetOperationPlan, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let provider = prepare_provider_draft(&settings, existing_id.as_deref(), provider)?;
    let action = if existing_id.is_some() {
        CentralAssetAction::Update
    } else {
        CentralAssetAction::Create
    };
    let profiles = provider_profiles(&settings, &provider)?;
    let profile_ids = profiles.keys().cloned().collect::<BTreeSet<_>>();
    let credential_action = match credential.as_deref() {
        None => CredentialAction::Keep,
        Some("") => CredentialAction::Clear,
        Some(_) => CredentialAction::Set,
    };
    let desired_credential_present = match credential_action {
        CredentialAction::Keep => provider_credential_present(&provider.id),
        CredentialAction::Set => true,
        CredentialAction::Clear => false,
    };
    let domain_plan =
        model_bundle_unchanged_consumers(&settings, &profiles, desired_credential_present)?;
    let consumer_count = domain_agent_count(&domain_plan);
    let draft_hash = hash_serializable(&(provider.clone(), profiles.clone()))?;
    let lifecycle = LifecycleBinding::ModelProviderUpsert {
        provider_id: provider.id.clone(),
        profile_ids: profile_ids.clone(),
        draft_hash,
        credential_action: credential_action.clone(),
    };
    let summary =
        model_provider_upsert_summary(profile_ids.len(), &credential_action, consumer_count);
    let mut central_changes = vec![CentralAssetChange {
        asset: AssetRef::ModelProvider {
            provider_id: provider.id.clone(),
        },
        action,
        summary,
    }];
    central_changes.extend(profile_ids.iter().map(|profile_id| CentralAssetChange {
        asset: AssetRef::Model {
            profile_id: profile_id.clone(),
        },
        action: CentralAssetAction::Update,
        summary: vec![format!(
            "随共享 Provider「{}」更新 Model Profile「{profile_id}」",
            provider.name
        )],
    }));
    let plan = finalize_plan_with(
        AssetOperationKind::UpdateAsset,
        domain_plan,
        central_changes,
        Vec::new(),
        Some(lifecycle),
    )?;
    PENDING_PAYLOADS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            plan.operation_id.clone(),
            PendingAssetPayload::ModelProvider {
                provider: Box::new(provider),
                profiles,
                credential: credential.map(Zeroizing::new),
            },
        );
    Ok(plan)
}

fn plan_model_provider_delete(provider_id: String) -> Result<AssetOperationPlan, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let provider = settings
        .model_providers
        .as_ref()
        .and_then(|providers| providers.get(&provider_id))
        .ok_or_else(|| "asset_operation_stale: Model Provider no longer exists".to_string())?;
    let referenced = settings
        .model_profiles
        .iter()
        .flatten()
        .filter(|(_, profile)| profile.provider_id.as_deref() == Some(provider_id.as_str()))
        .map(|(_, profile)| profile.name.clone())
        .collect::<Vec<_>>();
    if !referenced.is_empty() {
        return Err(format!(
            "model_provider_in_use: delete or switch these Models first: {}",
            referenced.join(", ")
        ));
    }
    finalize_plan_with(
        AssetOperationKind::DeleteAsset,
        DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        },
        vec![CentralAssetChange {
            asset: AssetRef::ModelProvider {
                provider_id: provider_id.clone(),
            },
            action: CentralAssetAction::Delete,
            summary: vec![format!("删除 Provider「{}」", provider.name)],
        }],
        Vec::new(),
        Some(LifecycleBinding::ModelProviderDelete { provider_id }),
    )
}

fn plan_mcp_delete(key: String, source_id: Option<String>) -> Result<AssetOperationPlan, String> {
    let source_id = source_id.ok_or_else(|| {
        "invalid_asset_source: deleting an MCP source copy requires source_id".to_string()
    })?;
    if !matches!(source_id.as_str(), "manual" | "discovered") {
        return Err(
            "asset_read_only: source-owned MCP assets must be removed from their source".into(),
        );
    }
    let copies: Vec<_> = read_registry_all()
        .into_iter()
        .filter(|item| item.entry.key() == key)
        .collect();
    let target = copies
        .iter()
        .find(|item| source_id_for(&item.entry) == source_id)
        .ok_or_else(|| {
            "asset_operation_stale: the reviewed MCP source copy no longer exists".to_string()
        })?;
    let effective_before = target.in_effect;
    let fallback_exists = effective_before
        && copies
            .iter()
            .any(|item| source_id_for(&item.entry) != source_id);
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let domain_plan = if effective_before {
        mcp_delete_consumers(&settings, &key, fallback_exists)
    } else {
        DomainPlan::Mcp {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        }
    };
    let consumer_count = domain_agent_count(&domain_plan);
    let summary = mcp_delete_summary(effective_before, fallback_exists, consumer_count);
    finalize_plan_with(
        AssetOperationKind::DeleteAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Mcp { key: key.clone() },
            action: CentralAssetAction::Delete,
            summary,
        }],
        vec![display_path(
            &local_sources_dir().join(format!("{source_id}.json")),
        )],
        Some(LifecycleBinding::McpDelete {
            key,
            source_id,
            fallback_exists,
            effective_before,
        }),
    )
}

fn plan_model_delete(profile_id: String) -> Result<AssetOperationPlan, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    if !settings
        .model_profiles
        .as_ref()
        .is_some_and(|profiles| profiles.contains_key(&profile_id))
    {
        return Err("asset_operation_stale: Model Profile no longer exists".into());
    }
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    let agent_ids: BTreeSet<String> = settings
        .model_consumptions
        .iter()
        .flatten()
        .map(|(agent_id, _)| agent_id.clone())
        .chain(
            settings
                .model_assignments
                .iter()
                .flatten()
                .map(|(id, _)| id.clone()),
        )
        .collect();
    for agent_id in agent_ids {
        let existing = settings.model_selection(&agent_id);
        if !existing.profiles.contains_key(&profile_id) {
            continue;
        }
        let mut desired = existing.clone();
        desired.profiles.remove(&profile_id);
        crate::resources::model::normalize_model_selection(&agent_id, &mut desired);
        before.insert(agent_id.clone(), existing);
        after.insert(agent_id, desired);
    }
    let domain_plan = DomainPlan::Model { before, after };
    let consumer_count = domain_agent_count(&domain_plan);
    let summary = model_delete_summary(consumer_count, false);
    finalize_plan_with(
        AssetOperationKind::DeleteAsset,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Model {
                profile_id: profile_id.clone(),
            },
            action: CentralAssetAction::Delete,
            summary,
        }],
        Vec::new(),
        Some(LifecycleBinding::ModelDelete { profile_id }),
    )
}

fn validate_mcp_entry(entry: &RegistryEntry) -> Result<(), String> {
    if entry.name.trim().is_empty() {
        return Err("invalid_asset: MCP name is required".into());
    }
    match (&entry.config.stdio, &entry.config.http) {
        (Some(config), None) if !config.command.trim().is_empty() => Ok(()),
        (None, Some(config))
            if !config.url.trim().is_empty()
                && (config.url.starts_with("https://") || config.url.starts_with("http://")) =>
        {
            Ok(())
        }
        (Some(_), Some(_)) => Err("invalid_asset: MCP must contain exactly one transport".into()),
        _ => Err("invalid_asset: MCP transport configuration is incomplete".into()),
    }
}

fn is_user_owned(entry: &RegistryEntry) -> bool {
    matches!(
        entry.origin.as_ref().map(|origin| origin.kind.as_str()),
        Some("manual" | "discovered")
    )
}

fn source_id_for(entry: &RegistryEntry) -> String {
    entry
        .origin
        .as_ref()
        .and_then(|origin| origin.source.clone())
        .or_else(|| entry.origin.as_ref().map(|origin| origin.kind.clone()))
        .unwrap_or_default()
}

fn mcp_unchanged_consumers(settings: &Settings, key: &str) -> DomainPlan {
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for (agent_id, records) in settings.mcp_consumptions.iter().flatten() {
        if records.contains_key(key) {
            let selection: Vec<String> = records.keys().cloned().collect();
            before.insert(agent_id.clone(), selection.clone());
            after.insert(agent_id.clone(), selection);
        }
    }
    DomainPlan::Mcp { before, after }
}

fn mcp_renamed_consumers(settings: &Settings, old_key: &str, new_key: &str) -> DomainPlan {
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for (agent_id, records) in settings.mcp_consumptions.iter().flatten() {
        if !records.contains_key(old_key) {
            continue;
        }
        let selection: Vec<String> = records.keys().cloned().collect();
        let desired = selection
            .iter()
            .map(|candidate| {
                if candidate == old_key {
                    new_key.to_string()
                } else {
                    candidate.clone()
                }
            })
            .collect();
        before.insert(agent_id.clone(), selection);
        after.insert(agent_id.clone(), desired);
    }
    DomainPlan::Mcp { before, after }
}

fn mcp_delete_consumers(settings: &Settings, key: &str, keep_relationships: bool) -> DomainPlan {
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for (agent_id, records) in settings.mcp_consumptions.iter().flatten() {
        if !records.contains_key(key) {
            continue;
        }
        let selection: Vec<String> = records.keys().cloned().collect();
        let desired = if keep_relationships {
            selection.clone()
        } else {
            selection
                .iter()
                .filter(|candidate| candidate.as_str() != key)
                .cloned()
                .collect()
        };
        before.insert(agent_id.clone(), selection);
        after.insert(agent_id.clone(), desired);
    }
    DomainPlan::Mcp { before, after }
}

fn model_unchanged_consumers(
    settings: &Settings,
    profile: &ModelProfile,
    desired_credential_present: bool,
) -> Result<DomainPlan, String> {
    let mut candidate_settings = settings.clone();
    candidate_settings
        .model_profiles
        .get_or_insert_default()
        .insert(profile.id.clone(), profile.clone());
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    let agent_ids: BTreeSet<String> = settings
        .model_consumptions
        .iter()
        .flatten()
        .map(|(agent_id, _)| agent_id.clone())
        .chain(
            settings
                .model_assignments
                .iter()
                .flatten()
                .map(|(id, _)| id.clone()),
        )
        .collect();
    for agent_id in agent_ids {
        let selection = settings.model_selection(&agent_id);
        if !selection.profiles.contains_key(&profile.id) {
            continue;
        }
        let capability = model_agent_capability(&agent_id).ok_or_else(|| {
            format!("model_agent_unsupported: {agent_id} has no managed Model writer")
        })?;
        if capability.mode != "managed"
            || !capability.supported_protocols.contains(&profile.protocol)
        {
            return Err(format!(
                "model_protocol_unsupported: the edited Profile is incompatible with {agent_id}"
            ));
        }
        if let Some((code, message)) = profile_endpoint_compatibility_issue(&agent_id, profile) {
            return Err(format!("{code}: {message}"));
        }
        if let Some((code, message)) = profile_credential_issue(
            &agent_id,
            profile,
            desired_credential_present,
            &selection.delivery_for(&profile.id),
        ) {
            return Err(format!("{code}: {message}"));
        }
        super::planner::validate_model_selection_contract(
            &candidate_settings,
            &agent_id,
            &selection,
        )?;
        before.insert(agent_id.clone(), selection.clone());
        after.insert(agent_id, selection);
    }
    Ok(DomainPlan::Model { before, after })
}

fn model_bundle_unchanged_consumers(
    settings: &Settings,
    profiles: &BTreeMap<String, ModelProfile>,
    desired_credential_present: bool,
) -> Result<DomainPlan, String> {
    let mut candidate_settings = settings.clone();
    candidate_settings
        .model_profiles
        .get_or_insert_default()
        .extend(profiles.clone());
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    let agent_ids: BTreeSet<String> = settings
        .model_consumptions
        .iter()
        .flatten()
        .map(|(agent_id, _)| agent_id.clone())
        .chain(
            settings
                .model_assignments
                .iter()
                .flatten()
                .map(|(id, _)| id.clone()),
        )
        .collect();
    for agent_id in agent_ids {
        let selection = settings.model_selection(&agent_id);
        let affected = selection
            .profiles
            .keys()
            .filter_map(|profile_id| profiles.get(profile_id))
            .collect::<Vec<_>>();
        if affected.is_empty() {
            continue;
        }
        let capability = model_agent_capability(&agent_id).ok_or_else(|| {
            format!("model_agent_unsupported: {agent_id} has no managed Model writer")
        })?;
        for profile in affected {
            if capability.mode != "managed"
                || !capability.supported_protocols.contains(&profile.protocol)
            {
                return Err(format!(
                    "model_protocol_unsupported: the edited Provider is incompatible with {agent_id}"
                ));
            }
            if let Some((code, message)) = profile_endpoint_compatibility_issue(&agent_id, profile)
            {
                return Err(format!("{code}: {message}"));
            }
            if let Some((code, message)) = profile_credential_issue(
                &agent_id,
                profile,
                desired_credential_present,
                &selection.delivery_for(&profile.id),
            ) {
                return Err(format!("{code}: {message}"));
            }
        }
        super::planner::validate_model_selection_contract(
            &candidate_settings,
            &agent_id,
            &selection,
        )?;
        before.insert(agent_id.clone(), selection.clone());
        after.insert(agent_id, selection);
    }
    Ok(DomainPlan::Model { before, after })
}

fn domain_agent_count(plan: &DomainPlan) -> usize {
    match plan {
        DomainPlan::Mcp { before, after } | DomainPlan::Skill { before, after } => before
            .keys()
            .chain(after.keys())
            .collect::<BTreeSet<_>>()
            .len(),
        DomainPlan::Model { before, after } => before
            .keys()
            .chain(after.keys())
            .collect::<BTreeSet<_>>()
            .len(),
        DomainPlan::AgentCapabilities {
            agent_id,
            skills_before,
            skills_after,
            affected_agent_ids,
            ..
        } => skills_before
            .keys()
            .chain(skills_after.keys())
            .map(String::as_str)
            .chain(affected_agent_ids.iter().map(String::as_str))
            .chain(std::iter::once(agent_id.as_str()))
            .collect::<BTreeSet<_>>()
            .len(),
    }
}

fn central_upsert_summary(
    central_label: &str,
    action: &CentralAssetAction,
    consumer_count: usize,
) -> Vec<String> {
    let verb = match action {
        CentralAssetAction::Create => "创建",
        CentralAssetAction::Update => "更新",
        CentralAssetAction::Delete => "删除",
    };
    let prefix = if consumer_count == 0 { "仅" } else { "" };
    vec![
        format!("{prefix}{verb}{central_label}"),
        agent_sync_summary(consumer_count),
    ]
}

fn model_upsert_summary(
    action: &CentralAssetAction,
    credential_action: &CredentialAction,
    consumer_count: usize,
) -> Vec<String> {
    let mut summary = central_upsert_summary("中央模型配置", action, consumer_count);
    if let Some(credential_summary) = match credential_action {
        CredentialAction::Keep => None,
        CredentialAction::Set if matches!(action, CentralAssetAction::Create) => {
            Some("将 API Key 保存到钥匙串")
        }
        CredentialAction::Set => Some("更新钥匙串中的 API Key"),
        CredentialAction::Clear => Some("清除钥匙串中的 API Key"),
    } {
        summary.insert(1, credential_summary.into());
    }
    summary
}

fn model_provider_upsert_summary(
    model_count: usize,
    credential_action: &CredentialAction,
    consumer_count: usize,
) -> Vec<String> {
    let mut summary = vec![format!("更新共享 Provider 及其 {model_count} 个 Model")];
    if let Some(credential_summary) = match credential_action {
        CredentialAction::Keep => None,
        CredentialAction::Set => Some("统一更新 Provider API Key"),
        CredentialAction::Clear => Some("清除 Provider API Key"),
    } {
        summary.push(credential_summary.into());
    }
    summary.push(agent_sync_summary(consumer_count));
    summary
}

fn model_delete_summary(consumer_count: usize, _removes_provider: bool) -> Vec<String> {
    let mut summary = vec![
        "删除中央模型配置".into(),
        "保留关联的 Provider 与 API Key".into(),
    ];
    summary.push(if consumer_count == 0 {
        "当前没有已关联的 Agent，只删除中央配置".into()
    } else {
        format!("将从 {consumer_count} 个已关联 Agent 移除")
    });
    summary
}

fn mcp_delete_summary(
    effective_before: bool,
    fallback_exists: bool,
    consumer_count: usize,
) -> Vec<String> {
    if !effective_before {
        return vec![
            "删除这份 MCP 配置".into(),
            "当前生效的 MCP 配置与 Agent 关联不会改变".into(),
        ];
    }
    if fallback_exists {
        return vec![
            "删除当前使用的 MCP 配置".into(),
            "改用同名的其他 MCP 配置".into(),
            agent_sync_summary(consumer_count),
        ];
    }
    vec![
        "删除中央 MCP 配置".into(),
        if consumer_count == 0 {
            "当前没有已关联的 Agent，只删除中央配置".into()
        } else {
            format!("将从 {consumer_count} 个已关联 Agent 移除并解除关联")
        },
    ]
}

fn agent_sync_summary(consumer_count: usize) -> String {
    if consumer_count == 0 {
        "当前没有已关联的 Agent，无需同步".into()
    } else {
        format!("将同步到 {consumer_count} 个已关联 Agent")
    }
}

fn hash_serializable<T: serde::Serialize>(value: &T) -> Result<String, String> {
    super::payload_hash::hash(value)
}

fn display_path(path: &Path) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.display().to_string();
    };
    path.strip_prefix(home)
        .map(|relative| format!("~/{}", relative.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{
        ModelProtocol, ModelProviderConfig, ModelProviderProtocolConfig, RegistryConfig,
        StdioConfig,
    };
    use crate::resources::mcp::registry::write_manual_entry;
    use crate::settings::mutate_settings;
    use crate::testenv::TestHome;
    use std::fs;

    fn mcp(command: &str) -> RegistryEntry {
        RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: command.into(),
                    args: None,
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        }
    }

    fn shared_provider_models() -> (ModelProviderConfig, ModelProfile, ModelProfile) {
        let provider = ModelProviderConfig {
            id: "team-gateway".into(),
            name: "Team Gateway".into(),
            provider: "custom".into(),
            base_url: "https://old.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([
                (
                    ModelProtocol::OpenaiResponses,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/v1/responses".into(),
                    },
                ),
                (
                    ModelProtocol::AnthropicMessages,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/anthropic/v1/messages".into(),
                    },
                ),
            ]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let mut first = ModelProfile {
            id: "team-openai".into(),
            provider_id: Some(provider.id.clone()),
            name: "Team OpenAI".into(),
            provider: "custom".into(),
            model_vendor: Some("openai".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: provider.base_url.clone(),
            endpoint_path: provider.protocols[&ModelProtocol::OpenaiResponses]
                .endpoint_path
                .clone(),
            model: "gpt-team".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(true),
        };
        let second = ModelProfile {
            id: "team-anthropic".into(),
            provider_id: Some(provider.id.clone()),
            name: "Team Anthropic".into(),
            protocol: ModelProtocol::AnthropicMessages,
            endpoint_path: provider.protocols[&ModelProtocol::AnthropicMessages]
                .endpoint_path
                .clone(),
            model: "claude-team".into(),
            model_vendor: Some("anthropic".into()),
            ..first.clone()
        };
        first.native_ids.clear();
        (provider, first, second)
    }

    #[test]
    fn persisted_mcp_plan_contains_no_draft_values() {
        let home = TestHome::new("lifecycle-mcp-secret");
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(mcp("secret-command-value")),
            },
        })
        .unwrap();
        assert_eq!(
            plan.central_changes[0].summary,
            vec!["仅创建中央 MCP 配置", "当前没有已关联的 Agent，无需同步"]
        );
        let persisted = std::fs::read_to_string(
            home.home
                .join(".arca/staging/consumption")
                .join(plan.operation_id)
                .join("plan.json"),
        )
        .unwrap();
        assert!(!persisted.contains("secret-command-value"));
    }

    #[test]
    fn model_edit_keeps_every_consumer_in_the_plan() {
        let _home = TestHome::new("lifecycle-model-consumers");
        let profile = ModelProfile {
            id: "work".into(),
            provider_id: Some("custom-provider".into()),
            name: "Work".into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: "https://old.invalid".into(),
            endpoint_path: String::new(),
            model: "old".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        crate::resources::model::save_profile(profile.clone(), None).unwrap();
        mutate_settings(|settings| {
            settings
                .model_assignments
                .get_or_insert_default()
                .insert("codex".into(), profile.id.clone());
        })
        .unwrap();
        let mut edited = profile;
        edited.model = "new".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()),
                profile: Box::new(edited),
                credential: None,
            },
        })
        .unwrap();
        assert_eq!(plan.affected_agent_ids, vec!["codex"]);
        assert!(plan.relationship_changes.is_empty());
        assert_eq!(plan.kind, AssetOperationKind::UpdateAsset);
        assert_eq!(
            plan.central_changes[0].summary,
            vec!["更新中央模型配置", "将同步到 1 个已关联 Agent"]
        );
    }

    #[test]
    fn provider_edit_plans_every_child_model_and_consumer() {
        let _home = TestHome::new("lifecycle-model-provider-consumers");
        let (provider, first, second) = shared_provider_models();
        mutate_settings(|settings| {
            settings.version = Some(3);
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            settings.model_profiles = Some(BTreeMap::from([
                (first.id.clone(), first.clone()),
                (second.id.clone(), second.clone()),
            ]));
            settings
                .model_assignments
                .get_or_insert_default()
                .insert("codex".into(), first.id.clone());
            settings
                .model_assignments
                .get_or_insert_default()
                .insert("claude-code".into(), second.id.clone());
        })
        .unwrap();

        let mut edited = provider;
        edited.base_url = "https://new.example.test".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(edited.id.clone()),
                provider: Box::new(edited),
                credential: None,
            },
        })
        .unwrap();

        assert_eq!(plan.affected_agent_ids, vec!["claude-code", "codex"]);
        assert_eq!(plan.central_changes.len(), 3);
        assert!(plan.relationship_changes.is_empty());
        assert_eq!(plan.central_changes[0].action, CentralAssetAction::Update);
        assert_eq!(
            plan.central_changes[0].asset,
            AssetRef::ModelProvider {
                provider_id: "team-gateway".into(),
            }
        );
        assert_eq!(
            plan.central_changes[0].summary,
            vec![
                "更新共享 Provider 及其 2 个 Model",
                "将同步到 2 个已关联 Agent",
            ]
        );
        assert_eq!(
            plan.central_changes[1..]
                .iter()
                .map(|change| change.asset.clone())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                AssetRef::Model {
                    profile_id: first.id,
                },
                AssetRef::Model {
                    profile_id: second.id,
                },
            ])
        );
    }

    #[test]
    fn provider_edit_blocks_an_unrepresentable_endpoint_path_before_touching_agent_files() {
        let home = TestHome::new("lifecycle-provider-path-block");
        let (provider, first, second) = shared_provider_models();
        mutate_settings(|settings| {
            settings.version = Some(crate::settings::SETTINGS_VERSION);
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            settings.model_profiles = Some(BTreeMap::from([
                (first.id.clone(), first.clone()),
                (second.id.clone(), second),
            ]));
            settings
                .model_assignments
                .get_or_insert_default()
                .insert("codex".into(), first.id.clone());
        })
        .unwrap();
        let target = home.home.join(".codex/config.toml");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "sentinel = \"unchanged\"\n").unwrap();

        let mut edited = provider;
        edited.protocols.insert(
            ModelProtocol::OpenaiResponses,
            ModelProviderProtocolConfig {
                endpoint_path: "/custom/invoke".into(),
            },
        );
        let error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(edited.id.clone()),
                provider: Box::new(edited),
                credential: None,
            },
        })
        .unwrap_err();

        assert!(
            error.starts_with("model_endpoint_path_unsupported:"),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "sentinel = \"unchanged\"\n"
        );
    }

    #[test]
    fn provider_edit_can_keep_or_replace_one_shared_credential() {
        let _home = TestHome::new("lifecycle-model-provider-shared-credential");
        let (provider, first, second) = shared_provider_models();
        mutate_settings(|settings| {
            settings.version = Some(3);
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            settings.model_profiles = Some(BTreeMap::from([
                (first.id.clone(), first.clone()),
                (second.id.clone(), second.clone()),
            ]));
        })
        .unwrap();
        crate::resources::model::apply_credential_update(&first.id, Some("shared-secret")).unwrap();

        let keep = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider.clone()),
                credential: None,
            },
        })
        .unwrap();
        assert!(keep.can_commit);

        let replace = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider),
                credential: Some("new-shared-secret".into()),
            },
        })
        .unwrap();
        assert!(replace.can_commit);
    }

    #[test]
    fn provider_can_exist_without_models_and_is_deleted_explicitly() {
        let _home = TestHome::new("lifecycle-independent-provider");
        let create = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: None,
                provider: Box::new(ModelProviderConfig {
                    id: String::new(),
                    name: "Independent Gateway".into(),
                    provider: "custom".into(),
                    base_url: "https://gateway.example.test".into(),
                    model_catalog_url: None,
                    portal_url: None,
                    protocols: BTreeMap::from([(
                        ModelProtocol::OpenaiResponses,
                        ModelProviderProtocolConfig {
                            endpoint_path: "/v1/responses".into(),
                        },
                    )]),
                    auth_requirement: crate::domain::types::AuthRequirement::Optional,
                    api_key_source: None,
                    env_key: Some("TEAM_GATEWAY_KEY".into()),
                }),
                credential: Some("provider-secret".into()),
            },
        })
        .unwrap();
        let AssetRef::ModelProvider { provider_id } = &create.central_changes[0].asset else {
            panic!("expected Provider central asset");
        };
        let provider_id = provider_id.clone();
        crate::assets::commit_asset_operation(AssetCommitRequest {
            operation_id: create.operation_id,
            candidate_hash: create.candidate_hash,
        })
        .unwrap();

        let settings = load_settings_strict().unwrap();
        assert!(settings
            .model_providers
            .as_ref()
            .unwrap()
            .contains_key(&provider_id));
        assert!(settings
            .model_profiles
            .as_ref()
            .is_none_or(BTreeMap::is_empty));
        assert!(provider_credential_present(&provider_id));

        let delete = plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::ModelProvider {
                provider_id: provider_id.clone(),
            },
            source_id: None,
        })
        .unwrap();
        crate::assets::commit_asset_operation(AssetCommitRequest {
            operation_id: delete.operation_id,
            candidate_hash: delete.candidate_hash,
        })
        .unwrap();
        assert!(!load_settings_strict()
            .unwrap()
            .model_providers
            .as_ref()
            .is_some_and(|providers| providers.contains_key(&provider_id)));
        assert!(!provider_credential_present(&provider_id));
    }

    #[test]
    fn provider_delete_is_blocked_while_a_model_references_it() {
        let _home = TestHome::new("lifecycle-provider-delete-blocked");
        let (provider, first, _) = shared_provider_models();
        crate::resources::model::save_profile(first.clone(), None).unwrap();
        let error = plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::ModelProvider {
                provider_id: provider.id,
            },
            source_id: None,
        })
        .unwrap_err();
        assert!(error.contains(&first.name), "{error}");
        assert!(error.starts_with("model_provider_in_use:"), "{error}");
    }

    #[test]
    fn adding_a_provider_model_inherits_the_shared_credential_and_cannot_change_it() {
        let _home = TestHome::new("lifecycle-add-provider-model-shared-credential");
        let (provider, first, second) = shared_provider_models();
        mutate_settings(|settings| {
            settings.version = Some(3);
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            settings.model_profiles = Some(BTreeMap::from([
                (first.id.clone(), first.clone()),
                (second.id.clone(), second.clone()),
            ]));
        })
        .unwrap();
        let mut third = first.clone();
        third.id.clear();
        third.name = "Third Model".into();
        third.model = "gpt-third".into();
        crate::resources::model::apply_credential_update(&first.id, Some("shared-secret")).unwrap();

        let inherited = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: None,
                profile: Box::new(third.clone()),
                credential: None,
            },
        })
        .unwrap();
        assert!(inherited.can_commit);

        let change_error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: None,
                profile: Box::new(third),
                credential: Some("different-secret".into()),
            },
        })
        .unwrap_err();
        assert_eq!(
            change_error,
            "model_credential_owned_by_provider: edit the Provider to change its API Key"
        );
    }

    #[test]
    fn model_review_copy_explains_credentials_and_zero_agent_scope() {
        assert_eq!(
            model_upsert_summary(&CentralAssetAction::Create, &CredentialAction::Set, 0),
            vec![
                "仅创建中央模型配置",
                "将 API Key 保存到钥匙串",
                "当前没有已关联的 Agent，无需同步",
            ]
        );
        assert_eq!(
            model_upsert_summary(&CentralAssetAction::Update, &CredentialAction::Clear, 2),
            vec![
                "更新中央模型配置",
                "清除钥匙串中的 API Key",
                "将同步到 2 个已关联 Agent",
            ]
        );
        assert_eq!(
            model_delete_summary(0, false),
            vec![
                "删除中央模型配置",
                "保留关联的 Provider 与 API Key",
                "当前没有已关联的 Agent，只删除中央配置",
            ]
        );
        assert_eq!(
            model_delete_summary(1, true),
            vec![
                "删除中央模型配置",
                "保留关联的 Provider 与 API Key",
                "将从 1 个已关联 Agent 移除",
            ]
        );
    }

    #[test]
    fn mcp_delete_review_copy_distinguishes_fallback_and_inactive_copies() {
        assert_eq!(
            mcp_delete_summary(true, true, 2),
            vec![
                "删除当前使用的 MCP 配置",
                "改用同名的其他 MCP 配置",
                "将同步到 2 个已关联 Agent",
            ]
        );
        assert_eq!(
            mcp_delete_summary(false, false, 0),
            vec![
                "删除这份 MCP 配置",
                "当前生效的 MCP 配置与 Agent 关联不会改变",
            ]
        );
        assert_eq!(
            mcp_delete_summary(true, false, 0),
            vec![
                "删除中央 MCP 配置",
                "当前没有已关联的 Agent，只删除中央配置",
            ]
        );
    }

    #[test]
    fn model_edit_rejects_a_credential_owned_by_its_provider() {
        let _home = TestHome::new("lifecycle-grok-model-credential");
        let profile = ModelProfile {
            id: "work".into(),
            provider_id: Some("openrouter-provider".into()),
            name: "Work".into(),
            provider: "openrouter".into(),
            model_vendor: Some("provider".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiCompletions,
            base_url: "https://openrouter.ai/api/v1".into(),
            endpoint_path: String::new(),
            model: "provider/model".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        crate::resources::model::save_profile(profile.clone(), None).unwrap();
        mutate_settings(|settings| {
            settings
                .model_assignments
                .get_or_insert_default()
                .insert("grok-build".into(), profile.id.clone());
        })
        .unwrap();

        let error = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some(profile.id.clone()),
                profile: Box::new(profile),
                credential: Some("test-credential".into()),
            },
        })
        .unwrap_err();

        assert_eq!(
            error,
            "model_credential_owned_by_provider: edit the Provider to change its API Key"
        );
    }

    #[test]
    fn deleting_mcp_plans_relationship_cleanup() {
        let _home = TestHome::new("lifecycle-mcp-delete");
        write_manual_entry(&mcp("local-server")).unwrap();
        mutate_settings(|settings| {
            settings
                .mcp_consumptions
                .get_or_insert_default()
                .entry("claude-code".into())
                .or_default()
                .insert(
                    "local::stdio".into(),
                    crate::assets::McpConsumptionRecord {
                        asset_key: "local::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                );
        })
        .unwrap();
        let plan = plan_delete_central_asset(PlanDeleteCentralAssetRequest {
            asset: AssetRef::Mcp {
                key: "local::stdio".into(),
            },
            source_id: Some("manual".into()),
        })
        .unwrap();
        assert_eq!(plan.relationship_changes.len(), 1);
        assert_eq!(plan.kind, AssetOperationKind::DeleteAsset);
    }

    #[test]
    fn model_schema_v2_migrates_identity_metadata_and_keychain_atomically() {
        let _home = TestHome::new("model-schema-v2-central");
        let legacy = ModelProfile {
            id: "openrouter-free".into(),
            provider_id: None,
            name: "OpenRouter Free".into(),
            provider: String::new(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiCompletions,
            base_url: "https://openrouter.ai/api/v1".into(),
            endpoint_path: String::new(),
            model: "openrouter/free".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.version = Some(1);
            settings
                .extra
                .insert("future".into(), serde_json::json!({"keep": true}));
            settings
                .model_profiles
                .get_or_insert_default()
                .insert(legacy.id.clone(), legacy.clone());
        })
        .unwrap();
        crate::resources::model::apply_credential_update(&legacy.id, Some("test-secret")).unwrap();

        assert!(migrate_model_profiles_v2_if_needed().unwrap());

        let settings = load_settings_strict().unwrap();
        assert_eq!(settings.version, Some(2));
        assert_eq!(settings.extra["future"]["keep"], true);
        let profiles = settings.model_profiles.unwrap();
        assert_eq!(profiles.len(), 1);
        let profile = profiles.values().next().unwrap();
        assert_ne!(profile.id, legacy.id);
        assert!(profile.id.starts_with("openrouter-openrouter-free-"));
        assert_eq!(profile.provider, "openrouter");
        assert_eq!(profile.model_vendor.as_deref(), Some("openrouter"));
        // The migration copies central authority first and deliberately keeps
        // the live legacy credential available to the untouched Agent config.
        assert!(crate::resources::model::credential_present(&legacy.id));
        assert!(crate::resources::model::credential_present(&profile.id));
    }

    #[test]
    fn model_schema_v2_preserves_a_managed_agent_provider_identity() {
        let _home = TestHome::new("model-schema-v2-consumer");
        let legacy = ModelProfile {
            id: "legacy-router".into(),
            provider_id: None,
            name: "Legacy Router".into(),
            provider: String::new(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiCompletions,
            base_url: "https://openrouter.ai/api/v1".into(),
            endpoint_path: String::new(),
            model: "openrouter/free".into(),
            env_key: Some("OPENROUTER_API_KEY".into()),
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        };
        mutate_settings(|settings| {
            settings.version = Some(1);
            settings
                .model_profiles
                .get_or_insert_default()
                .insert(legacy.id.clone(), legacy.clone());
        })
        .unwrap();
        crate::resources::model::apply_profile("grok-build", &legacy.id).unwrap();
        let target = _home.home.join(".grok/config.toml");
        let before = fs::read(&target).unwrap();

        assert!(migrate_model_profiles_v2_if_needed().unwrap());
        assert_eq!(fs::read(&target).unwrap(), before);

        let settings = load_settings_strict().unwrap();
        let profile = settings
            .model_profiles
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_ne!(profile.id, legacy.id);
        assert_eq!(
            settings
                .model_selection("grok-build")
                .active_profile_id
                .as_deref(),
            Some(profile.id.as_str())
        );
        assert_eq!(
            crate::resources::model::observe_profile("grok-build", profile).unwrap(),
            crate::resources::model::ModelObservedState::Drifted
        );
    }
}
