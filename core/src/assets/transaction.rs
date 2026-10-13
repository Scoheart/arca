//! Recoverable central commits with target-scoped convergence incidents.

use super::inventory::list_consumption_inventory;
use super::lifecycle::{clear_pending_payload, pending_payload, PendingAssetPayload};
use super::planner::{
    hash_file, hash_mcp_catalog, hash_settings_target, hash_skill_target_graph, hash_target,
    hash_targets, load_operation, operation_root, CredentialAction, LifecycleBinding,
    PersistedAssetOperation, SkillMigrationEntry,
};
use super::store::AssetStateStore;
use super::types::{
    AssetCapability, AssetCommitRequest, AssetOperationKind, AssetOperationPlan, AssetRef,
    ConsumptionInventory, DomainPlan, McpConsumptionRecord, ModelAgentSelection,
    RelationshipAction, SkillConsumptionRecord, TargetIncident,
};
use crate::domain::agents::ModelStorageAuthority;
use crate::paths::{mcp_catalog_file, model_catalog_file, settings_file, skill_catalog_file};
use crate::resources::mcp::ops;
use crate::resources::mcp::r#override::OverridePatch;
use crate::resources::mcp::registry::{
    delete_discovered_entry, delete_registry_entry, read_registry, read_registry_all,
    write_manual_entry,
};
#[cfg(test)]
use crate::resources::model::private_file_snapshot_exists;
use crate::resources::model::{
    agent_has_configured_models, apply_credential_update, apply_profile_consumption_target,
    apply_profile_consumption_with_credential_presence_target, apply_profile_target,
    clear_all_configured_models_for_targets, clear_credential_rollback,
    clear_private_file_snapshot, clear_profile_consumption_target, credential_present,
    credential_rollback_snapshot, credential_snapshot, delete_profile, delete_provider,
    load_private_file_snapshot, persist_credential_rollback, persist_private_file_snapshot,
    private_file_snapshot_subject, provider_credential_present, provider_credential_subject,
    restore_credential_snapshot, save_profile, save_provider_bundle, ModelTargetError,
};
use crate::resources::skill::{
    acquire_skills_lock, canonical_skill_assignments, declared_targets_for_agents, normalize_agent_selection,
    reapply_assignment_safely, release_assignment_links_safely,
    SkillsOperationLock, SkillsPaths,
};
use crate::safe_write::{
    acquire_settings_lock, begin_transaction_write_tracking_with_private_states,
    capture_parent_directory, create_transaction_symlink_if_missing,
    ensure_no_transaction_mutation_intents, load_transaction_write_states_with_private_paths,
    read_path_state_anchored, recover_global_mutation_intents,
    recover_transaction_mutation_intents_with_private_paths,
    remove_bytes_if_hash_unchanged_in_parent, remove_bytes_if_unchanged_in_parent,
    remove_symlink_if_unchanged_in_parent, resume_transaction_write_tracking_with_private_states,
    transaction_private_file_state, write_bytes_if_hash_unchanged_in_parent,
    write_bytes_if_unchanged_in_parent, write_symlink_if_unchanged_in_parent, AnchoredPathState,
    ParentDirectorySnapshot, PathIdentity, TransactionPathState,
};
#[cfg(test)]
use crate::safe_write::{begin_transaction_write_tracking, set_transaction_symlink};
use crate::settings::{load_settings_strict, mutate_settings, Settings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::{Zeroize, Zeroizing};

static COMMIT_LOCK: Mutex<()> = Mutex::new(());
const ROLLBACK_MANIFEST_VERSION: u32 = 3;
const PRIVATE_TARGET_LEDGER_VERSION: u32 = 1;
const TARGET_INCIDENT_MARKER: &str = "target-incident";
const ROLLBACK_COMPLETE_MARKER: &str = "rollback-complete";
const CLEANUP_AUTHORIZED_MARKER: &str = "cleanup-authorized";

const CENTRAL_COMMITTED_MARKER: &str = "central-committed";

fn central_snapshot_path(path: &Path) -> bool {
    path == settings_file() || path == model_catalog_file() || path == skill_catalog_file()
        || path.starts_with(crate::paths::mcp_assets_dir())
        || path.starts_with(crate::paths::legacy_sources_dir())
}

fn target_completion_marker(path: &Path) -> String {
    format!("target-complete-{}", hex::encode(Sha256::digest(path.to_string_lossy().as_bytes())))
}

fn target_state_digest(state: Option<&TransactionPathState>) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(&state).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)).into_bytes())
}

fn mark_central_committed(plan: &AssetOperationPlan) -> Result<(), String> {
    mark_operation_state(&plan.operation_id, CENTRAL_COMMITTED_MARKER, b"central-committed\n")
}

fn mark_target_completed(plan: &AssetOperationPlan, agent_id: &str) -> Result<(), String> {
    if !operation_state_marker_exists(&plan.operation_id, CENTRAL_COMMITTED_MARKER)? { return Ok(()); }
    let manifest = load_rollback_manifest(&plan.operation_id)?
        .ok_or_else(|| "target_recovery_required: rollback manifest is missing".to_string())?;
    let private = manifest.snapshots.iter().filter(|item| item.privacy == SnapshotPrivacy::Private)
        .map(|item| item.path.clone()).collect();
    let states = load_transaction_write_states_with_private_paths(
        &transaction_write_evidence_dir(&plan.operation_id), &private)?;
    let completed = target_paths_for_agent(plan, agent_id).iter().map(|target| expand_tilde_path(target))
        .filter(|path| !central_snapshot_path(path))
        .map(|path| target_state_digest(states.get(&path)).map(|digest| (path, digest)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    // One durable marker covers all files in an Agent write set. A crash while
    // publishing it cannot preserve only half of a multi-file target.
    let bytes = serde_json::to_vec(&completed).map_err(|error| error.to_string())?;
    mark_operation_state(&plan.operation_id, &target_completion_marker(Path::new(agent_id)), &bytes)
}

fn physical_completion_marker(directory: &Path) -> String {
    format!("physical-{}", target_completion_marker(directory))
}

fn mark_physical_target_completed(
    plan: &AssetOperationPlan,
    directory: &Path,
    paths: &[String],
) -> Result<(), String> {
    let states = load_transaction_write_states_with_private_paths(
        &transaction_write_evidence_dir(&plan.operation_id), &BTreeSet::new())?;
    let completed = paths.iter().map(|path| expand_tilde_path(path))
        .map(|path| target_state_digest(states.get(&path)).map(|digest| (path, digest)))
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let bytes = serde_json::to_vec(&completed).map_err(|error| error.to_string())?;
    mark_operation_state(&plan.operation_id, &physical_completion_marker(directory), &bytes)
}

fn completed_checkpoint_paths(
    plan: &AssetOperationPlan,
    marker: &str,
    expected: BTreeSet<PathBuf>,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<BTreeSet<PathBuf>, String> {
    if !operation_state_marker_exists(&plan.operation_id, marker)? { return Ok(BTreeSet::new()); }
    let completed: BTreeMap<PathBuf, Vec<u8>> = serde_json::from_slice(
        &fs::read(operation_root(&plan.operation_id).join(marker)).map_err(|error| error.to_string())?
    ).map_err(|_| "target_recovery_required: invalid target completion record".to_string())?;
    if completed.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err("target_recovery_required: completion target set changed".into());
    }
    for (path, digest) in &completed {
        if *digest != target_state_digest(states.get(path))? { return Ok(BTreeSet::new()); }
    }
    Ok(expected)
}

fn checkpoint_completed_paths(
    plan: &AssetOperationPlan,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<BTreeSet<PathBuf>, String> {
    let mut completed_paths = BTreeSet::new();
    for agent_id in &plan.affected_agent_ids {
        let marker = target_completion_marker(Path::new(agent_id));
        let expected = target_paths_for_agent(plan, agent_id).iter().map(|target| expand_tilde_path(target))
            .filter(|path| !central_snapshot_path(path)).collect::<BTreeSet<_>>();
        completed_paths.extend(completed_checkpoint_paths(plan, &marker, expected, states)?);
    }
    if matches!(plan.domain_plan, DomainPlan::Skill { .. }) {
        let mut groups = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
        for path in plan.target_files.iter().map(|path| expand_tilde_path(path)) {
            if central_snapshot_path(&path) { continue; }
            if let Some(parent) = path.parent() {
                groups.entry(parent.to_path_buf()).or_default().insert(path);
            }
        }
        for (directory, expected) in groups {
            completed_paths.extend(completed_checkpoint_paths(
                plan, &physical_completion_marker(&directory), expected, states)?);
        }
    }
    Ok(completed_paths)
}

/// Restore only incomplete physical write sets. Central authority and completed
/// target versions remain durable. Completion is bound to the final write state
/// so a later partial write to the same shared file cannot reuse an old marker.
fn settle_checkpoint_targets(
    plan: &AssetOperationPlan,
    snapshots: Vec<PathSnapshot>,
    states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Result<(), String> {
    let completed_paths = checkpoint_completed_paths(plan, states)?;
    let pending = snapshots.into_iter().filter(|snapshot| !central_snapshot_path(&snapshot.path)
        && !completed_paths.contains(&snapshot.path)).collect::<Vec<_>>();
    if !pending.is_empty() {
        let mut incomplete = plan.clone();
        incomplete.target_files.retain(|target| pending.iter().any(|snapshot| snapshot.path == expand_tilde_path(target)));
        incomplete.affected_agent_ids.retain(|agent_id| target_paths_for_agent(plan, agent_id).iter()
            .any(|target| incomplete.target_files.contains(target)));
        record_recovery_incidents(&incomplete)?;
    }
    let errors = if matches!(plan.domain_plan, DomainPlan::Skill { .. }) {
        let mut groups = BTreeMap::<PathBuf, Vec<PathSnapshot>>::new();
        for snapshot in pending {
            let parent = snapshot.path.parent().ok_or_else(|| "target_recovery_required: Skill target has no parent".to_string())?;
            groups.entry(parent.to_path_buf()).or_default().push(snapshot);
        }
        // A foreign edit blocks only its own physical directory. Keep the
        // all-path preflight within each directory, and recover healthy peers.
        groups.values().flat_map(|group| restore_snapshots_if_unchanged(group, states)).collect::<Vec<_>>()
    } else {
        restore_snapshots_if_unchanged(&pending, states)
    };
    if !errors.is_empty() {
        return Err(format!("target_recovery_required: {}", errors.join("; ")));
    }
    Ok(())
}

fn settle_live_checkpoints(plan: &AssetOperationPlan) -> Result<(), String> {
    if !operation_state_marker_exists(&plan.operation_id, CENTRAL_COMMITTED_MARKER)? { return Ok(()); }
    let snapshots = load_rollback_snapshots(&plan.operation_id)?
        .ok_or_else(|| "target_recovery_required: rollback snapshots are missing".to_string())?;
    let private = snapshots.iter().filter(|item| item.privacy == SnapshotPrivacy::Private)
        .map(|item| item.path.clone()).collect();
    let states = load_transaction_write_states_with_private_paths(
        &transaction_write_evidence_dir(&plan.operation_id), &private)?;
    settle_checkpoint_targets(plan, snapshots, &states)
}

#[cfg(test)]
thread_local! {
    static AFTER_PRIVATE_SNAPSHOT_PERSIST_HOOK:
        std::cell::RefCell<Option<Box<dyn FnOnce() -> Result<(), String>>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn set_after_private_snapshot_persist_hook(hook: impl FnOnce() -> Result<(), String> + 'static) {
    AFTER_PRIVATE_SNAPSHOT_PERSIST_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
fn run_after_private_snapshot_persist_hook() -> Result<(), String> {
    AFTER_PRIVATE_SNAPSHOT_PERSIST_HOOK
        .with(|slot| slot.borrow_mut().take().map_or(Ok(()), |hook| hook()))
}

#[cfg(not(test))]
fn run_after_private_snapshot_persist_hook() -> Result<(), String> {
    Ok(())
}

fn plan_capability(plan: &DomainPlan) -> AssetCapability {
    match plan {
        DomainPlan::Mcp { .. } => AssetCapability::Mcp,
        DomainPlan::Model { .. } => AssetCapability::Model,
        DomainPlan::Skill { .. } | DomainPlan::AgentCapabilities { .. } => AssetCapability::Skill,
    }
}

fn incident_id(capability: AssetCapability, target_path: &str) -> String {
    let material = format!("{capability:?}:{target_path}");
    format!(
        "target-{}",
        hex::encode(Sha256::digest(material.as_bytes()))
    )
}

fn target_paths_for_agent(plan: &AssetOperationPlan, agent_id: &str) -> Vec<String> {
    if plan.affected_agent_ids.len() <= 1 {
        return plan.target_files.clone();
    }
    let settings = load_settings_strict().ok();
    let agents = crate::agents::load_agents();
    let mut matched = plan
        .target_files
        .iter()
        .filter(|target| match &plan.domain_plan {
            DomainPlan::Mcp { .. } => agents
                .get(agent_id)
                .and_then(|agent| agent.global.as_ref())
                .is_some_and(|path| path == *target),
            DomainPlan::Model { .. } => settings
                .as_ref()
                .and_then(|settings| {
                    crate::resources::model::configured_path_strings_checked(settings, agent_id)
                        .ok()
                        .flatten()
                })
                .is_some_and(|configured| {
                    let target = crate::resources::mcp::scanner::expand_tilde(target);
                    configured.into_iter().any(|candidate| {
                        let candidate = crate::resources::mcp::scanner::expand_tilde(&candidate);
                        target == candidate || target.parent() == candidate.parent()
                    })
                }),
            DomainPlan::Skill { .. } | DomainPlan::AgentCapabilities { .. } => {
                agents.get(agent_id).and_then(|agent| agent.skills.as_ref()).is_some_and(|capability| {
                    let path = expand_tilde_path(target);
                    let Some(parent) = path.parent() else { return false; };
                    std::iter::once(&capability.global_dir).chain(capability.aliases.iter().map(|alias| &alias.global_dir))
                        .any(|root| expand_tilde_path(root) == parent || {
                            let declared = crate::resources::skill::canonical_skill_target_path(root).ok();
                            let observed = crate::resources::skill::canonical_skill_target_path(&parent.to_string_lossy()).ok();
                            declared.is_some() && declared == observed
                        })
                })
            }
        })
        .cloned()
        .collect::<Vec<_>>();
    if matched.is_empty() && !matches!(plan.domain_plan, DomainPlan::Skill { .. }) {
        matched = plan.target_files.clone();
    }
    matched.sort();
    matched.dedup();
    matched
}

fn record_target_incident(
    plan: &AssetOperationPlan,
    agent_id: &str,
    code: &str,
) -> Result<(), String> {
    let capability = plan_capability(&plan.domain_plan);
    let targets = target_paths_for_agent(plan, agent_id);
    mutate_settings(|settings| {
        let incidents = settings.target_incidents.get_or_insert_default();
        for target_path in &targets {
            let id = incident_id(capability, target_path);
            let mut affected_agent_ids = vec![agent_id.to_string()];
            if let Some(existing) = incidents.get(&id) {
                affected_agent_ids.extend(existing.affected_agent_ids.iter().cloned());
            }
            affected_agent_ids.sort();
            affected_agent_ids.dedup();
            incidents.insert(
                id.clone(),
                TargetIncident {
                    id: id.clone(),
                    operation_id: plan.operation_id.clone(),
                    capability,
                    target_id: id,
                    target_path: target_path.clone(),
                    affected_agent_ids,
                    code: code.to_string(),
                    retryable: true,
                },
            );
        }
    })
    .map_err(|error| error.to_string())
}

fn clear_target_incidents(plan: &AssetOperationPlan, agent_id: &str) -> Result<(), String> {
    let capability = plan_capability(&plan.domain_plan);
    let target_ids = target_paths_for_agent(plan, agent_id)
        .into_iter()
        .map(|target| incident_id(capability, &target))
        .collect::<BTreeSet<_>>();
    mutate_settings(|settings| {
        if let Some(incidents) = settings.target_incidents.as_mut() {
            incidents.retain(|id, _| !target_ids.contains(id));
            if incidents.is_empty() {
                settings.target_incidents = None;
            }
        }
    })
    .map_err(|error| error.to_string())?;
    mark_target_completed(plan, agent_id)
}

fn incident_operation_ids_for_plan(plan: &AssetOperationPlan) -> BTreeSet<String> {
    let capability = plan_capability(&plan.domain_plan);
    let affected_agents = plan
        .affected_agent_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    load_settings_strict()
        .ok()
        .and_then(|settings| settings.target_incidents)
        .into_iter()
        .flat_map(|incidents| incidents.into_values())
        .filter(|incident| {
            incident.capability == capability
                && incident
                    .affected_agent_ids
                    .iter()
                    .any(|agent_id| affected_agents.contains(agent_id))
        })
        .map(|incident| incident.operation_id)
        .collect()
}

fn cleanup_resolved_incident_operations(operation_ids: &BTreeSet<String>, current: &str) {
    let still_referenced = load_settings_strict()
        .ok()
        .and_then(|settings| settings.target_incidents)
        .into_iter()
        .flat_map(|incidents| incidents.into_values())
        .map(|incident| incident.operation_id)
        .collect::<BTreeSet<_>>();
    for operation_id in operation_ids {
        if operation_id != current && !still_referenced.contains(operation_id) {
            let canonical = uuid::Uuid::parse_str(operation_id)
                .ok()
                .map(|parsed| parsed.hyphenated().to_string());
            if canonical.as_deref() != Some(operation_id.as_str()) {
                continue;
            }
            let root = operation_root(operation_id);
            if fs::symlink_metadata(&root).is_ok_and(|metadata| {
                metadata.file_type().is_dir() && !metadata.file_type().is_symlink()
            }) {
                // A resolved target incident may still own a durable guard or
                // temp from an older rename-based writer. Reconcile and retire
                // that exact evidence before removing its operation journal;
                // otherwise the sibling artifacts would be orphaned forever.
                let manifest = load_rollback_manifest(operation_id).ok().flatten();
                let reconciled = manifest
                    .as_ref()
                    .map(|snapshots| {
                        snapshots
                            .snapshots
                            .iter()
                            .map(|snapshot| (snapshot.path.clone(), snapshot.parent.clone()))
                            .collect::<BTreeMap<_, _>>()
                    })
                    .is_some_and(|parents| {
                        let private_paths = manifest
                            .as_ref()
                            .expect("manifest exists when parents were derived")
                            .snapshots
                            .iter()
                            .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Private)
                            .map(|snapshot| snapshot.path.clone())
                            .collect::<BTreeSet<_>>();
                        recover_transaction_mutation_intents_with_private_paths(
                            &transaction_mutation_intent_dir(operation_id),
                            &parents,
                            &private_paths,
                        )
                        .and_then(|_| {
                            ensure_no_transaction_mutation_intents(
                                &transaction_mutation_intent_dir(operation_id),
                            )
                        })
                        .is_ok()
                    });
                if !reconciled {
                    continue;
                }
                if mark_operation_state(
                    operation_id,
                    CLEANUP_AUTHORIZED_MARKER,
                    b"cleanup-authorized\n",
                )
                .is_err()
                {
                    continue;
                }
                let Some(manifest) = manifest.as_ref() else {
                    continue;
                };
                let Ok(operation) = load_operation(operation_id) else {
                    continue;
                };
                if finalize_completed_operation_cleanup(
                    operation_id,
                    &root,
                    manifest,
                    &lifecycle_profile_ids(operation.lifecycle.as_ref()),
                )
                .is_err()
                {
                    continue;
                }
            }
        }
    }
}

fn private_transaction_paths(
    plan: &AssetOperationPlan,
    settings: &Settings,
) -> Result<BTreeSet<PathBuf>, String> {
    let mut private = claude_desktop_private_transaction_paths(plan, settings)?;
    // Models and MCP share this file. Protect the whole file even when only
    // mcpServers is edited, or when the last managed model is removed.
    let mut qoder_paths = ["qoder-desktop", "qoder-cli"].iter().flat_map(|agent| {
        settings.agent_config_paths.as_ref().and_then(|paths| paths.get(*agent))
            .and_then(|entry| entry.model_paths.clone()).unwrap_or_default()
    }).collect::<Vec<_>>();
    qoder_paths.push("~/.qoder/settings.json".into());
    let qoder_paths = qoder_paths.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).collect::<BTreeSet<_>>();
    private.extend(plan.target_files.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).filter(|path| qoder_paths.contains(path)));
    let mut zcode_paths = settings.agent_config_paths.as_ref()
        .and_then(|paths| paths.get("zcode"))
        .and_then(|entry| entry.model_paths.clone()).unwrap_or_default();
    zcode_paths.push("~/.zcode/v2/config.json".into());
    let zcode_paths = zcode_paths.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).collect::<BTreeSet<_>>();
    private.extend(plan.target_files.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).filter(|path| zcode_paths.contains(path)));
    // Desktop providers may already contain credentials written by Kilo.
    // Protect the complete file for MCP edits as well as Model edits.
    let mut kilo_paths = crate::resources::model::default_config_paths("kilo-desktop").unwrap_or_default();
    kilo_paths.extend(settings.agent_config_paths.as_ref()
        .and_then(|paths| paths.get("kilo-desktop"))
        .and_then(|entry| entry.model_paths.clone()).unwrap_or_default());
    if let Some(path) = settings.agents.as_ref().and_then(|agents| agents.get("kilo-desktop"))
        .and_then(|agent| agent.global.as_ref()) {
        kilo_paths.push(path.clone());
    }
    let kilo_paths = kilo_paths.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).collect::<BTreeSet<_>>();
    private.extend(plan.target_files.iter().map(|path| crate::resources::mcp::scanner::expand_tilde(path)).filter(|path| kilo_paths.contains(path)));
    Ok(private)
}

fn claude_desktop_private_transaction_paths(
    plan: &AssetOperationPlan,
    settings: &Settings,
) -> Result<BTreeSet<PathBuf>, String> {
    const CLAUDE_DESKTOP: &str = "claude-desktop";
    if !plan
        .affected_agent_ids
        .iter()
        .any(|agent_id| agent_id == CLAUDE_DESKTOP)
    {
        return Ok(BTreeSet::new());
    }
    let Some(configured) =
        crate::resources::model::configured_path_strings_checked(settings, CLAUDE_DESKTOP)?
    else {
        return Err("unsupported model Agent: claude-desktop".into());
    };
    let private = configured
        .get(3)
        .ok_or_else(|| {
            "claude_desktop_paths_invalid: expected exactly four target paths".to_string()
        })?
        .to_string();
    let private = crate::resources::mcp::scanner::expand_tilde(&private);
    let reviewed = plan
        .target_files
        .iter()
        .map(|path| crate::resources::mcp::scanner::expand_tilde(path))
        .collect::<BTreeSet<_>>();
    Ok(reviewed
        .contains(&private)
        .then_some(private)
        .into_iter()
        .collect())
}

pub fn commit_asset_operation(request: AssetCommitRequest) -> Result<ConsumptionInventory, String> {
    commit_asset_operation_with_hook(request, || Ok(()))
}

fn commit_asset_operation_with_hook<F>(
    request: AssetCommitRequest,
    after_preconditions: F,
) -> Result<ConsumptionInventory, String>
where
    F: FnOnce() -> Result<(), String>,
{
    commit_asset_operation_inner(request, after_preconditions, true)
}

/// Imports need a durable receipt, not an unused full workspace projection.
pub(crate) fn commit_asset_operation_without_inventory(request: AssetCommitRequest) -> Result<(), String> {
    commit_asset_operation_inner(request, || Ok(()), false).map(|_| ())
}

fn commit_asset_operation_inner<F>(
    request: AssetCommitRequest,
    after_preconditions: F,
    include_inventory: bool,
) -> Result<ConsumptionInventory, String>
where
    F: FnOnce() -> Result<(), String>,
{
    let _guard = COMMIT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    // All Asset commits take locks in the same order as standalone Skills
    // operations: Skills -> settings -> safe-write journal. This both removes
    // the historical lock inversion and lets Skill links be owned directly by
    // the outer Asset transaction.
    let skills_guard = acquire_asset_skills_lock()?;
    // Every cooperating Arca process already uses this lock for settings/source
    // mutations. Holding it across verify + apply closes the catalog and Agent
    // writer TOCTOU; nested settings mutations are reentrant on this thread.
    let _filesystem_guard = acquire_settings_lock(&settings_file())?;
    recover_global_mutation_intents()?;
    let persisted = load_operation(&request.operation_id)?;
    verify_request(&persisted, &request)?;
    if !persisted.plan.can_commit {
        return Err("asset_operation_blocked: resolve drift or conflict before commit".into());
    }
    verify_preconditions(&persisted)?;
    after_preconditions()?;
    let incident_operation_ids = incident_operation_ids_for_plan(&persisted.plan);
    let commit_settings = load_settings_strict().map_err(|error| error.to_string())?;
    let private_paths = private_transaction_paths(&persisted.plan, &commit_settings)?;

    let settings_path = settings_file();
    let target_paths = persisted
        .plan
        .target_files
        .iter()
        .map(|path| crate::resources::mcp::scanner::expand_tilde(path))
        .collect::<BTreeSet<_>>();
    let catalog_paths = [
        mcp_catalog_file(),
        model_catalog_file(),
        skill_catalog_file(),
    ];
    let mut snapshots = vec![PathSnapshot::capture(&settings_path)?];
    for path in &catalog_paths {
        snapshots.push(PathSnapshot::capture(path)?);
    }
    let skill_link_targets = matches!(
        persisted.plan.domain_plan,
        DomainPlan::Skill { .. } | DomainPlan::AgentCapabilities { .. }
    );
    let preserving_external_skill_targets =
        matches!(persisted.plan.domain_plan, DomainPlan::Skill { .. })
            && !persisted.plan.relationship_changes.is_empty()
            && persisted
                .plan
                .relationship_changes
                .iter()
                .all(|change| change.action == RelationshipAction::Remove);
    for path in target_paths {
        if path == settings_path || catalog_paths.contains(&path) {
            continue;
        }
        snapshots.push(if private_paths.contains(&path) {
            PathSnapshot::capture_private(&path)?
        } else if preserving_external_skill_targets {
            PathSnapshot::capture_any(&path)?
        } else if skill_link_targets {
            PathSnapshot::capture_link(&path)?
        } else {
            PathSnapshot::capture(&path)?
        });
    }
    verify_captured_snapshots(&persisted, &snapshots)?;
    validate_private_snapshot_aliases(&snapshots, &private_paths)?;
    let tracked_paths = snapshots
        .iter()
        .map(|snapshot| snapshot.path.clone())
        .collect::<Vec<_>>();
    let tracked_parent_snapshots = snapshots
        .iter()
        .map(|snapshot| (snapshot.path.clone(), snapshot.parent.clone()))
        .collect::<BTreeMap<_, _>>();
    let reviewed_states = snapshots
        .iter()
        .map(|snapshot| {
            snapshot
                .transaction_state()
                .map(|state| (snapshot.path.clone(), state))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let credential_backups = lifecycle_profile_ids(persisted.lifecycle.as_ref())
        .into_iter()
        .map(|profile_id| {
            let credential = credential_snapshot(&profile_id)?.map(Zeroizing::new);
            Ok((profile_id, credential))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for (profile_id, credential) in &credential_backups {
        persist_credential_rollback(
            &request.operation_id,
            profile_id,
            credential.as_ref().map(|value| value.as_slice()),
        )?;
    }
    if let Err(error) = persist_rollback_snapshots(&request.operation_id, &snapshots) {
        for (profile_id, _) in &credential_backups {
            clear_credential_rollback(&request.operation_id, profile_id).map_err(|cleanup| {
                format!(
                    "failed to persist rollback snapshots ({error}); Keychain rollback cleanup failed: {cleanup}"
                )
            })?;
        }
        return Err(error);
    }

    let write_tracker = begin_transaction_write_tracking_with_private_states(
        &transaction_write_evidence_dir(&request.operation_id),
        &tracked_paths,
        &private_paths,
        &tracked_parent_snapshots,
        &reviewed_states,
    )?;
    // A prior target incident is observable state, not a write prohibition.
    // Every new operation gets one real convergence attempt for its own target;
    // failures remain scoped incidents instead of silently skipping Agent I/O.
    let blocked_agents = BTreeSet::new();
    let applied = apply_operation(&persisted, &skills_guard, &blocked_agents)
        .and_then(|_| verify_operation(&persisted).map_err(ModelTargetError::from))
        .and_then(|_| settle_live_checkpoints(&persisted.plan).map_err(ModelTargetError::from))
        .and_then(|_| {
            mark_operation_committed(&request.operation_id).map_err(ModelTargetError::from)
        });
    let convergence_error = match applied {
        Ok(()) => None,
        Err(ModelTargetError::ConvergenceFailed(error)) => Some(error),
        Err(ModelTargetError::RecoveryRequired(error)) => {
            let claim_recovery = recover_transaction_mutation_intents_with_private_paths(
                &transaction_mutation_intent_dir(&request.operation_id),
                &tracked_parent_snapshots,
                &private_paths,
            );
            drop(write_tracker);
            let error = match claim_recovery {
                Ok(()) => error,
                Err(recovery) => {
                    format!("{error}; target claim recovery also failed: {recovery}")
                }
            };
            if localize_runtime_recovery_failure(&persisted.plan) {
                return Err(format!("target_recovery_required: {error}"));
            }
            return Err(format!("recovery_required: {error}"));
        }
    };
    if let Some(error) = convergence_error {
        if operation_state_marker_exists(&request.operation_id, CENTRAL_COMMITTED_MARKER)? {
            drop(write_tracker);
            recover_pending_asset_operation(&persisted, &operation_root(&request.operation_id))?;
            return Err(format!("target_convergence_failed: central state was saved; incomplete targets require retry ({error})"));
        }
        if let Err(recovery) = recover_transaction_mutation_intents_with_private_paths(
            &transaction_mutation_intent_dir(&request.operation_id),
            &tracked_parent_snapshots,
            &private_paths,
        ) {
            drop(write_tracker);
            if localize_runtime_recovery_failure(&persisted.plan) {
                return Err(format!(
                    "target_recovery_required: asset operation failed ({error}); target claim recovery failed: {recovery}"
                ));
            }
            return Err(format!(
                "recovery_required: asset operation failed ({error}); claim recovery failed: {recovery}"
            ));
        }
        // Claim recovery can restore the previous state of a target that was
        // written more than once in this transaction. Capture evidence only
        // after that reconciliation so rollback CAS uses the actual leaf now
        // present in the reviewed namespace.
        let written_states = write_tracker.states();
        let mut rollback_errors = restore_snapshots_if_unchanged(&snapshots, &written_states);
        if let Err(recovery) = ensure_no_transaction_mutation_intents(
            &transaction_mutation_intent_dir(&request.operation_id),
        ) {
            rollback_errors.push(recovery);
        }
        drop(write_tracker);
        // Keep the file/settings and credential rollback domains together. If
        // file ownership cannot be proven, leave the current credential and its
        // durable backup untouched for startup recovery/manual resolution.
        if rollback_errors.is_empty() {
            for (profile_id, credential) in &credential_backups {
                if let Err(rollback) = restore_credential_snapshot(
                    profile_id,
                    credential.as_ref().map(|value| value.as_slice()),
                ) {
                    rollback_errors.push(format!("failed to restore Model credential: {rollback}"));
                }
            }
        }
        if rollback_errors.is_empty() {
            if let Err(marker) = mark_operation_state(
                &request.operation_id,
                ROLLBACK_COMPLETE_MARKER,
                b"rolled-back\n",
            ) {
                rollback_errors.push(format!(
                    "failed to record completed rollback before cleanup: {marker}"
                ));
            }
        }
        if rollback_errors.is_empty() {
            match load_rollback_manifest(&request.operation_id) {
                Ok(Some(manifest)) => {
                    if let Err(cleanup) = finalize_completed_operation_cleanup(
                        &request.operation_id,
                        &operation_root(&request.operation_id),
                        &manifest,
                        &credential_backups
                            .iter()
                            .map(|(profile_id, _)| profile_id.clone())
                            .collect::<Vec<_>>(),
                    ) {
                        rollback_errors
                            .push(format!("failed to clean completed rollback: {cleanup}"));
                    }
                }
                Ok(None) => rollback_errors
                    .push("failed to clean completed rollback: manifest is missing".into()),
                Err(cleanup) => {
                    rollback_errors.push(format!("failed to clean completed rollback: {cleanup}"))
                }
            }
        }
        if rollback_errors.is_empty() {
            return Err(format!(
                "asset operation failed and was rolled back: {error}"
            ));
        }
        if localize_runtime_recovery_failure(&persisted.plan) {
            return Err(format!(
                "target_recovery_required: asset operation failed ({error}); target rollback failed: {}",
                rollback_errors.join("; ")
            ));
        }
        return Err(format!(
            "recovery_required: asset operation failed ({error}); rollback failed: {}",
            rollback_errors.join("; ")
        ));
    }
    drop(write_tracker);
    ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(
        &request.operation_id,
    ))?;

    let manifest = load_rollback_manifest(&request.operation_id)?
        .ok_or_else(|| "asset operation committed but rollback manifest is missing".to_string())?;
    finalize_completed_operation_cleanup(
        &request.operation_id,
        &operation_root(&request.operation_id),
        &manifest,
        &credential_backups
            .iter()
            .map(|(profile_id, _)| profile_id.clone())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| format!("asset operation committed but cleanup failed: {error}"))?;
    cleanup_resolved_incident_operations(&incident_operation_ids, &request.operation_id);
    if include_inventory { list_consumption_inventory() } else { Ok(ConsumptionInventory::default()) }
}

/// Recover every operation that had begun mutating durable state. Reviewed but
/// uncommitted plans have no rollback manifest and are safely cancelled after a
/// restart because secret-bearing drafts intentionally live only in memory.
pub fn recover_pending_asset_operations() -> Result<Vec<String>, String> {
    let _guard = COMMIT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _skills_guard = acquire_asset_skills_lock()?;
    let _filesystem_guard = acquire_settings_lock(&settings_file())?;
    recover_global_mutation_intents()?;
    let root = crate::paths::arca_dir().join("staging/consumption");
    let root_before = match fs::symlink_metadata(&root) {
        Ok(metadata) if metadata.file_type().is_dir() => metadata,
        Ok(_) => return Err("recovery_required: the Asset operation root is unsafe".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("recovery_required: {error}")),
    };
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => return Err(format!("recovery_required: {error}")),
    };
    let mut recovered = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("recovery_required: {error}"))?;
        let entry_type = entry
            .file_type()
            .map_err(|error| format!("recovery_required: {error}"))?;
        if entry_type.is_symlink() {
            return Err("recovery_required: an Asset operation root is unsafe".into());
        }
        if !entry_type.is_dir() {
            continue;
        }
        let operation_id = entry.file_name().to_string_lossy().into_owned();
        // An uncommitted plan can belong to a live Desktop confirmation while
        // a separate CLI process bootstraps. Recover only abandoned plans.
        let _abandoned_plan = if load_rollback_manifest(&operation_id)?.is_none() {
            match super::lease::claim_abandoned(&operation_id)? {
                Some(lease) => Some(lease),
                None => continue,
            }
        } else {
            None
        };
        let persisted =
            load_operation(&operation_id).map_err(|error| format!("recovery_required: {error}"))?;
        if operation_should_pause_for_target_incident(&operation_id)? {
            // Incidents are persisted before this marker. A later operation may
            // already have repaired some targets while peers remain pending.
            // Recreating every original incident would resurrect those resolved
            // targets (and overwrite newer incidents) on each startup.
            continue;
        }
        match recover_pending_asset_operation(&persisted, &entry.path()) {
            Ok(()) => {
                recovered.push(operation_id);
            }
            Err(error) if !persisted.plan.target_files.is_empty() => {
                // A target that changed after Arca wrote it is a local convergence
                // incident. Preserve the journal and rollback evidence, record
                // the exact affected target(s), and continue recovering peers.
                load_settings_strict().map_err(|settings_error| {
                    format!(
                        "recovery_required: target recovery failed ({error}); central settings are unavailable: {settings_error}"
                    )
                })?;
                record_recovery_incidents(&persisted.plan)?;
                let _ = write_private_file(
                    &operation_root(&persisted.plan.operation_id).join(TARGET_INCIDENT_MARKER),
                    b"recorded\n",
                );
            }
            Err(error) => return Err(error),
        }
    }
    let root_after = fs::symlink_metadata(&root).map_err(|error| {
        format!("recovery_required: failed to recheck Asset operations: {error}")
    })?;
    if !root_after.file_type().is_dir() || !same_directory_identity(&root_before, &root_after) {
        return Err("recovery_required: the Asset operation root changed during recovery".into());
    }
    Ok(recovered)
}

fn recover_pending_asset_operation(
    persisted: &PersistedAssetOperation,
    operation_path: &Path,
) -> Result<(), String> {
    let operation_id = &persisted.plan.operation_id;
    let profile_ids = lifecycle_profile_ids(persisted.lifecycle.as_ref());
    let private_ledger = load_private_target_ledger(operation_id)?;
    let Some(manifest) = load_rollback_manifest(operation_id)? else {
        ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(operation_id))?;
        for profile_id in &profile_ids {
            clear_credential_rollback(operation_id, profile_id)
                .map_err(|error| format!("recovery_required: {error}"))?;
        }
        if let Some(ledger) = private_ledger.as_ref() {
            clear_private_target_ledger_items(operation_id, ledger)
                .map_err(|error| format!("recovery_required: {error}"))?;
        }
        fs::remove_dir_all(operation_path)
            .map_err(|error| format!("recovery_required: {error}"))?;
        clear_pending_payload(operation_id);
        return Ok(());
    };

    let private_paths = manifest
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Private)
        .map(|snapshot| snapshot.path.clone())
        .collect::<BTreeSet<_>>();
    let ledger_paths = private_ledger
        .as_ref()
        .map(|ledger| {
            ledger
                .targets
                .iter()
                .map(|target| target.path.clone())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let reviewed_targets = persisted
        .plan
        .target_files
        .iter()
        .map(|path| crate::resources::mcp::scanner::expand_tilde(path))
        .collect::<BTreeSet<_>>();
    if private_paths != ledger_paths
        || !private_paths.is_subset(&reviewed_targets)
        || (!private_paths.is_empty()
            && !persisted
                .plan
                .affected_agent_ids
                .iter()
                .any(|agent_id| matches!(agent_id.as_str(), "zcode" | "claude-desktop" | "qoder-desktop" | "qoder-cli")))
    {
        return Err(
            "recovery_required: reviewed private transaction paths do not match rollback manifest"
                .into(),
        );
    }
    let parent_snapshots = manifest
        .snapshots
        .iter()
        .map(|snapshot| (snapshot.path.clone(), snapshot.parent.clone()))
        .collect::<BTreeMap<_, _>>();
    recover_transaction_mutation_intents_with_private_paths(
        &transaction_mutation_intent_dir(operation_id),
        &parent_snapshots,
        &private_paths,
    )?;
    ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(operation_id))?;

    if operation_commit_marker_exists(operation_id)?
        || operation_state_marker_exists(operation_id, ROLLBACK_COMPLETE_MARKER)?
        || operation_state_marker_exists(operation_id, CLEANUP_AUTHORIZED_MARKER)?
    {
        finalize_completed_operation_cleanup(operation_id, operation_path, &manifest, &profile_ids)
            .map_err(|error| format!("recovery_required: {error}"))?;
        return Ok(());
    }

    let snapshots = load_rollback_snapshots(operation_id)?
        .ok_or_else(|| "recovery_required: rollback manifest disappeared".to_string())?;
    let evidence_dir = transaction_write_evidence_dir(operation_id);
    let evidence_exists = match fs::symlink_metadata(&evidence_dir) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => true,
        Ok(_) => return Err("recovery_required: transaction write evidence is unsafe".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => return Err(format!("recovery_required: {error}")),
    };
    let written_states =
        load_transaction_write_states_with_private_paths(&evidence_dir, &private_paths)?;
    let mut credential_backups = Vec::new();
    for profile_id in &profile_ids {
        let snapshot = credential_rollback_snapshot(operation_id, profile_id)
            .map_err(|error| format!("recovery_required: {error}"))?
            .ok_or_else(|| {
                "recovery_required: Model credential rollback item is missing".to_string()
            })?;
        credential_backups.push((profile_id.clone(), snapshot.map(Zeroizing::new)));
    }
    // The commit marker is the only durable completion boundary. Desired-state
    // verification alone cannot prove that every physical target was written,
    // so every unmarked operation is rolled back when ownership still matches.
    let tracked_paths = snapshots
        .iter()
        .map(|snapshot| snapshot.path.clone())
        .collect::<Vec<_>>();
    let reviewed_states = snapshots
        .iter()
        .map(|snapshot| {
            snapshot
                .transaction_state()
                .map(|state| (snapshot.path.clone(), state))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let write_tracker = if evidence_exists {
        Some(resume_transaction_write_tracking_with_private_states(
            &evidence_dir,
            &tracked_paths,
            &private_paths,
            &parent_snapshots,
            &reviewed_states,
            &written_states,
        )?)
    } else {
        if !written_states.is_empty() {
            return Err(
                "recovery_required: write states exist without an evidence directory".into(),
            );
        }
        None
    };
    if operation_state_marker_exists(operation_id, CENTRAL_COMMITTED_MARKER)? {
        settle_checkpoint_targets(&persisted.plan, snapshots, &written_states)?;
        ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(operation_id))?;
        mark_operation_committed(operation_id)?;
        drop(write_tracker);
        return finalize_completed_operation_cleanup(operation_id, operation_path, &manifest, &profile_ids);
    }
    let mut rollback_errors = restore_snapshots_if_unchanged(&snapshots, &written_states);
    if let Err(error) =
        ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(operation_id))
    {
        rollback_errors.push(error);
    }
    drop(write_tracker);
    if !rollback_errors.is_empty() {
        return Err(format!(
            "target_recovery_required: {}",
            rollback_errors.join("; ")
        ));
    }
    for (profile_id, credential) in &credential_backups {
        restore_credential_snapshot(
            profile_id,
            credential.as_ref().map(|value| value.as_slice()),
        )
        .map_err(|error| format!("target_recovery_required: {error}"))?;
    }
    mark_operation_state(operation_id, ROLLBACK_COMPLETE_MARKER, b"rolled-back\n")
        .map_err(|error| format!("target_recovery_required: {error}"))?;
    ensure_no_transaction_mutation_intents(&transaction_mutation_intent_dir(operation_id))?;
    for profile_id in &profile_ids {
        clear_credential_rollback(operation_id, profile_id)
            .map_err(|error| format!("target_recovery_required: {error}"))?;
    }
    clear_private_snapshot_items(operation_id, &snapshots)
        .map_err(|error| format!("target_recovery_required: {error}"))?;
    fs::remove_dir_all(operation_path)
        .map_err(|error| format!("target_recovery_required: {error}"))?;
    clear_pending_payload(operation_id);
    Ok(())
}

fn target_incident_marker_exists(operation_id: &str) -> Result<bool, String> {
    let marker = operation_root(operation_id).join(TARGET_INCIDENT_MARKER);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err("recovery_required: Asset target incident marker is unsafe".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "recovery_required: failed to inspect Asset target incident marker: {error}"
        )),
    }
}

fn operation_should_pause_for_target_incident(operation_id: &str) -> Result<bool, String> {
    let terminal_cleanup = operation_commit_marker_exists(operation_id)?
        || operation_state_marker_exists(operation_id, ROLLBACK_COMPLETE_MARKER)?
        || operation_state_marker_exists(operation_id, CLEANUP_AUTHORIZED_MARKER)?;
    Ok(!terminal_cleanup && target_incident_marker_exists(operation_id)?)
}

fn operation_commit_marker_exists(operation_id: &str) -> Result<bool, String> {
    let marker = operation_commit_marker(operation_id);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err("recovery_required: Asset commit marker is unsafe".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "recovery_required: failed to inspect Asset commit marker: {error}"
        )),
    }
}

fn record_recovery_incidents(plan: &AssetOperationPlan) -> Result<(), String> {
    let mut pending = plan.clone();
    if matches!(plan.domain_plan, DomainPlan::Skill { .. })
        && operation_state_marker_exists(&plan.operation_id, CENTRAL_COMMITTED_MARKER)? {
        let states = load_transaction_write_states_with_private_paths(
            &transaction_write_evidence_dir(&plan.operation_id), &BTreeSet::new())?;
        let completed = checkpoint_completed_paths(plan, &states)?;
        pending.target_files.retain(|target| !completed.contains(&expand_tilde_path(target)));
        pending.affected_agent_ids.retain(|agent| target_paths_for_agent(plan, agent).iter()
            .any(|target| pending.target_files.contains(target)));
    }
    for agent_id in &pending.affected_agent_ids {
        record_target_incident(&pending, agent_id, "target_recovery_required")?;
    }
    Ok(())
}

fn localize_runtime_recovery_failure(plan: &AssetOperationPlan) -> bool {
    if plan.target_files.is_empty() || plan.affected_agent_ids.is_empty() {
        return false;
    }
    if load_settings_strict().is_err() || record_recovery_incidents(plan).is_err() {
        return false;
    }
    let _ = write_private_file(
        &operation_root(&plan.operation_id).join(TARGET_INCIDENT_MARKER),
        b"recorded\n",
    );
    true
}

fn acquire_asset_skills_lock() -> Result<SkillsOperationLock, String> {
    let paths = SkillsPaths::resolve_from_env().map_err(|error| format!("{error:?}"))?;
    paths
        .ensure_arca_root()
        .map_err(|error| format!("{error:?}"))?;
    acquire_skills_lock(&paths).map_err(|error| format!("{error:?}"))
}

#[cfg(unix)]
fn same_directory_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_directory_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    true
}

pub fn cancel_asset_operation(operation_id: &str) -> Result<(), String> {
    let _guard = COMMIT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _filesystem_guard = acquire_settings_lock(&settings_file())?;
    uuid::Uuid::parse_str(operation_id).map_err(|_| "invalid asset operation id".to_string())?;
    if !operation_root(operation_id).exists() {
        clear_pending_payload(operation_id);
        return Ok(());
    }
    let operation = load_operation(operation_id)?;
    if operation.plan.operation_id != operation_id {
        return Err("asset operation identity mismatch".into());
    }
    if load_rollback_manifest(operation_id)?.is_none() {
        if let Some(ledger) = load_private_target_ledger(operation_id)? {
            clear_private_target_ledger_items(operation_id, &ledger)?;
            remove_private_target_ledger(operation_id)?;
        }
    }
    if operation_has_recovery_evidence(operation_id, &operation)? {
        return Err(
            "recovery_required: the asset operation has started committing; recover it before cancelling"
                .into(),
        );
    }
    match fs::remove_dir_all(operation_root(operation_id)) {
        Ok(()) => {
            clear_pending_payload(operation_id);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            clear_pending_payload(operation_id);
            Ok(())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn operation_has_recovery_evidence(
    operation_id: &str,
    operation: &PersistedAssetOperation,
) -> Result<bool, String> {
    for path in [
        operation_root(operation_id).join("rollback"),
        operation_commit_marker(operation_id),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => {
                return Err("recovery_required: failed to inspect asset operation evidence".into());
            }
        }
    }
    for profile_id in lifecycle_profile_ids(operation.lifecycle.as_ref()) {
        match credential_rollback_snapshot(operation_id, &profile_id) {
            Ok(Some(_)) => return Ok(true),
            Ok(None) => {}
            Err(_) => {
                return Err(
                    "recovery_required: failed to inspect Model credential rollback evidence"
                        .into(),
                );
            }
        }
    }
    if load_private_target_ledger(operation_id)?.is_some() {
        // The durable ledger itself is recovery evidence. Do not downgrade a
        // locked/denied Keychain lookup to "no evidence" and retire the only
        // path binding capable of cleaning the private snapshot safely.
        return Ok(true);
    }
    Ok(false)
}

fn lifecycle_profile_ids(lifecycle: Option<&LifecycleBinding>) -> Vec<String> {
    match lifecycle {
        Some(LifecycleBinding::ModelUpsert { profile_id, .. })
        | Some(LifecycleBinding::ModelAdopt { profile_id, .. })
        | Some(LifecycleBinding::ModelDelete { profile_id }) => vec![profile_id.clone()],
        Some(LifecycleBinding::ModelProviderUpsert { provider_id, .. })
        | Some(LifecycleBinding::ModelProviderDelete { provider_id }) => {
            vec![provider_credential_subject(provider_id)]
        }
        _ => Vec::new(),
    }
}

fn operation_commit_marker(operation_id: &str) -> PathBuf {
    operation_root(operation_id).join("commit-complete")
}

fn operation_state_marker_exists(operation_id: &str, name: &str) -> Result<bool, String> {
    let marker = operation_root(operation_id).join(name);
    match fs::symlink_metadata(&marker) {
        Ok(metadata) if metadata.file_type().is_file() && !metadata.file_type().is_symlink() => {
            Ok(true)
        }
        Ok(_) => Err("recovery_required: Asset operation state marker is unsafe".into()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "recovery_required: failed to inspect Asset operation state marker: {error}"
        )),
    }
}

fn mark_operation_state(operation_id: &str, name: &str, content: &[u8]) -> Result<(), String> {
    let operation_directory = operation_root(operation_id);
    write_private_file(&operation_directory.join(name), content)?;
    sync_directory(&operation_directory)?;
    let parent = operation_directory
        .parent()
        .ok_or_else(|| "asset operation directory has no parent".to_string())?;
    sync_directory(parent)
}

fn transaction_write_evidence_dir(operation_id: &str) -> PathBuf {
    operation_root(operation_id).join("rollback/post")
}

fn transaction_mutation_intent_dir(operation_id: &str) -> PathBuf {
    operation_root(operation_id).join("rollback/claims")
}

fn mark_operation_committed(operation_id: &str) -> Result<(), String> {
    mark_operation_committed_with_barrier_hook(operation_id, |_| Ok(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitMarkerDurabilityBarrier {
    File,
    OperationDirectory,
    ParentDirectory,
}

fn mark_operation_committed_with_barrier_hook<F>(
    operation_id: &str,
    mut after_barrier: F,
) -> Result<(), String>
where
    F: FnMut(CommitMarkerDurabilityBarrier) -> Result<(), String>,
{
    let operation_directory = operation_root(operation_id);
    write_private_file(&operation_commit_marker(operation_id), b"committed\n")?;
    after_barrier(CommitMarkerDurabilityBarrier::File)?;

    sync_directory(&operation_directory)?;
    after_barrier(CommitMarkerDurabilityBarrier::OperationDirectory)?;

    let parent = operation_directory
        .parent()
        .ok_or_else(|| "asset operation directory has no parent".to_string())?;
    sync_directory(parent)?;
    after_barrier(CommitMarkerDurabilityBarrier::ParentDirectory)
}

/// A pure relationship removal releases Arca's desired ownership. Central
/// lifecycle operations and enabled-state edits do not get this exemption:
/// they still own and must reconcile their Agent bytes.
fn releases_relationship_ownership(plan: &AssetOperationPlan) -> bool {
    plan.kind == AssetOperationKind::SetConsumption
        && plan.central_changes.is_empty()
        && !plan.relationship_changes.is_empty()
        && plan
            .relationship_changes
            .iter()
            .all(|change| change.action == RelationshipAction::Remove)
}

fn apply_operation(
    persisted: &PersistedAssetOperation,
    skills_lock: &SkillsOperationLock,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), ModelTargetError> {
    let Some(lifecycle) = &persisted.lifecycle else {
        return apply_domain_plan(
            &persisted.plan,
            releases_relationship_ownership(&persisted.plan),
            skills_lock,
            blocked_agents,
        );
    };
    match lifecycle {
        LifecycleBinding::McpClear { agent_id } => {
            mutate_settings(|settings| {
                if let Some(consumptions) = settings.mcp_consumptions.as_mut() {
                    consumptions.remove(agent_id);
                    if consumptions.is_empty() {
                        settings.mcp_consumptions = None;
                    }
                }
                if let Some(disabled) = settings.disabled.as_mut() {
                    disabled.remove(agent_id);
                    if disabled.is_empty() {
                        settings.disabled = None;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
            match crate::resources::mcp::ops::clear_agent(agent_id).and_then(|_| {
                if crate::resources::mcp::ops::agent_has_entries(agent_id)? {
                    Err("target_convergence_failed: MCP entries reappeared after clear".into())
                } else {
                    Ok(())
                }
            }) {
                Ok(()) => clear_target_incidents(&persisted.plan, agent_id)?,
                Err(_) => {
                    record_target_incident(&persisted.plan, agent_id, "target_convergence_failed")?
                }
            }
            Ok(())
        }
        LifecycleBinding::ModelClear {
            agent_id,
            storage_authority,
            ..
        } => {
            let before = match &persisted.plan.domain_plan {
                DomainPlan::Model { before, .. } => {
                    before.get(agent_id).cloned().unwrap_or_default()
                }
                _ => return Err("Model clear requires a Model domain plan".into()),
            };
            mutate_settings(|settings| {
                if let Some(consumptions) = settings.model_consumptions.as_mut() {
                    consumptions.remove(agent_id);
                    if consumptions.is_empty() {
                        settings.model_consumptions = None;
                    }
                }
                if let Some(assignments) = settings.model_assignments.as_mut() {
                    assignments.remove(agent_id);
                    if assignments.is_empty() {
                        settings.model_assignments = None;
                    }
                }
            })
            .map_err(|error| error.to_string())?;
            let result: Result<(), ModelTargetError> = match storage_authority {
                ModelStorageAuthority::NativeRegistry => {
                    clear_all_configured_models_for_targets(agent_id, &persisted.plan.target_files)
                        .and_then(|_| {
                            if agent_has_configured_models(agent_id)? {
                                Err(
                            "target_convergence_failed: Model entries reappeared after clear"
                                .into(),
                        )
                            } else {
                                Ok(())
                            }
                        })
                        .map_err(ModelTargetError::from)
                }
                ModelStorageAuthority::ArcaMapping => {
                    let mut result = Ok(());
                    for (profile_id, _) in &before.profiles {
                        let active =
                            before.active_profile_id.as_deref() == Some(profile_id.as_str());
                        if let Err(error) =
                            clear_profile_consumption_target(agent_id, profile_id, active)
                        {
                            result = Err(error);
                            break;
                        }
                    }
                    result
                }
                ModelStorageAuthority::Guided => Err(ModelTargetError::ConvergenceFailed(
                    "model_agent_guided".into(),
                )),
            };
            match result {
                Ok(()) => clear_target_incidents(&persisted.plan, agent_id)?,
                Err(ModelTargetError::ConvergenceFailed(_)) => {
                    record_target_incident(&persisted.plan, agent_id, "target_convergence_failed")?
                }
                Err(error @ ModelTargetError::RecoveryRequired(_)) => return Err(error),
            }
            Ok(())
        }
        LifecycleBinding::McpUpsert {
            key,
            draft_hash,
            previous_key,
            previous_source_id,
        } => {
            let PendingAssetPayload::Mcp { entry } =
                require_pending_payload(&persisted.plan.operation_id)?
            else {
                return Err(
                    "asset_operation_expired: central MCP draft is unavailable; reopen the editor"
                        .into(),
                );
            };
            verify_payload_hash(&entry, draft_hash)?;
            if entry.key() != *key {
                return Err("asset_operation_stale: MCP draft identity changed".into());
            }
            write_manual_entry(&entry).map_err(|error| error.to_string())?;
            if let Some(previous_key) = previous_key {
                let previous_source_id = previous_source_id.as_deref().ok_or_else(|| {
                    "asset_operation_stale: MCP rename source is unavailable".to_string()
                })?;
                migrate_mcp_consumption_records(previous_key, key)?;
                delete_mcp_source_copy(previous_key, previous_source_id)?;
                apply_domain_plan(
                    &persisted.plan,
                    releases_relationship_ownership(&persisted.plan),
                    skills_lock,
                    blocked_agents,
                )
            } else {
                mark_central_committed(&persisted.plan)?;
                reapply_mcp_consumers(&persisted.plan, key, blocked_agents)
                    .map_err(ModelTargetError::from)
            }
        }
        LifecycleBinding::McpAdopt {
            key,
            draft_hash,
            enabled,
        } => {
            if let Some(draft_hash) = draft_hash {
                let PendingAssetPayload::Mcp { entry } =
                    require_pending_payload(&persisted.plan.operation_id)?
                else {
                    return Err(
                        "asset_operation_expired: imported MCP config is unavailable; reopen migration"
                            .into(),
                    );
                };
                verify_payload_hash(&entry, draft_hash)?;
                if entry.key() != *key {
                    return Err("asset_operation_stale: MCP migration identity changed".into());
                }
                write_manual_entry(&entry).map_err(|error| error.to_string())?;
            }
            apply_mcp_adoption(&persisted.plan, key, enabled).map_err(ModelTargetError::from)
        }
        LifecycleBinding::McpDelete {
            key,
            source_id,
            fallback_exists,
            effective_before,
        } => {
            delete_mcp_source_copy(key, source_id)?;
            if !effective_before {
                return Ok(());
            }
            if *fallback_exists {
                reapply_mcp_consumers(&persisted.plan, key, blocked_agents)
                    .map_err(ModelTargetError::from)
            } else {
                apply_domain_plan(
                    &persisted.plan,
                    releases_relationship_ownership(&persisted.plan),
                    skills_lock,
                    blocked_agents,
                )
            }
        }
        LifecycleBinding::McpEnabled {
            agent_id,
            asset_key,
            after,
            ..
        } => apply_mcp_enabled(&persisted.plan, agent_id, asset_key, *after, blocked_agents)
            .map_err(ModelTargetError::from),
        LifecycleBinding::McpEnabledBulk {
            agent_id,
            before,
            after,
        } => {
            for asset_key in before
                .iter()
                .filter_map(|(key, enabled)| (*enabled != *after).then_some(key))
            {
                apply_mcp_enabled(&persisted.plan, agent_id, asset_key, *after, blocked_agents)?;
            }
            Ok(())
        }
        LifecycleBinding::SkillEnabled {
            name,
            target_id,
            affected_agent_ids,
            after,
            ..
        } => apply_skill_enabled(
            &persisted.plan,
            name,
            target_id,
            affected_agent_ids,
            *after,
            skills_lock,
            blocked_agents,
        )
        .map_err(ModelTargetError::from),
        LifecycleBinding::McpReapply { key, agent_ids } => {
            reapply_mcp_consumers_for_agents(&persisted.plan, key, agent_ids, blocked_agents)
                .map_err(ModelTargetError::from)
        }
        LifecycleBinding::ModelReapply {
            agent_id,
            profile_id,
            enabled,
            active,
            credential_present,
        } => {
            if blocked_agents.contains(agent_id) {
                return Ok(());
            }
            let result = if *enabled {
                apply_profile_consumption_with_credential_presence_target(
                    agent_id,
                    profile_id,
                    *credential_present,
                    *active,
                )
                .map(|_| ())
            } else {
                clear_profile_consumption_target(agent_id, profile_id, *active)
            };
            match result {
                Ok(()) => clear_target_incidents(&persisted.plan, agent_id)?,
                Err(ModelTargetError::ConvergenceFailed(_)) => {
                    record_target_incident(&persisted.plan, agent_id, "target_convergence_failed")?
                }
                Err(error @ ModelTargetError::RecoveryRequired(_)) => return Err(error),
            }
            Ok(())
        }
        LifecycleBinding::SkillReapply {
            name,
            target_id,
            enabled,
            ..
        } => {
            let _ = enabled;
            let result = reapply_assignment_safely(name, target_id, skills_lock)
                .map_err(|error| format!("{error:?}"));
            for agent_id in &persisted.plan.affected_agent_ids {
                if result.is_ok() {
                    clear_target_incidents(&persisted.plan, agent_id)?;
                } else {
                    record_target_incident(&persisted.plan, agent_id, "target_convergence_failed")?;
                }
            }
            Ok(())
        }
        LifecycleBinding::SkillAdoptObserved { name, record } => {
            let inventory =
                crate::resources::skill::list_inventory().map_err(|error| format!("{error:?}"))?;
            let current = inventory.items.iter().find(|item| {
                item.name == *name
                    && matches!(
                        item.location,
                        crate::resources::skill::SkillLocation::Central
                    )
            });
            if current.and_then(|item| item.content_hash.as_ref()) != Some(&record.content_hash) {
                return Err(
                    "asset_operation_stale: Skill content changed after convergence review".into(),
                );
            }
            mutate_settings(|settings| {
                settings
                    .managed_skills
                    .get_or_insert_default()
                    .insert(name.clone(), record.as_ref().clone());
            })
            .map_err(|error| error.to_string())?;
            for agent_id in &persisted.plan.affected_agent_ids {
                clear_target_incidents(&persisted.plan, agent_id)?;
            }
            Ok(())
        }
        LifecycleBinding::ModelUpsert {
            profile_id,
            draft_hash,
            credential_action,
        } => {
            let PendingAssetPayload::Model {
                profile,
                credential,
            } = require_pending_payload(&persisted.plan.operation_id)?
            else {
                return Err("asset_operation_expired: Model draft or credential is unavailable; reopen the editor".into());
            };
            verify_payload_hash(&profile, draft_hash)?;
            if profile.id != *profile_id || credential_action_for(&credential) != *credential_action
            {
                return Err(
                    "asset_operation_stale: Model draft no longer matches the reviewed plan".into(),
                );
            }
            let desired_credential_present = match credential_action {
                CredentialAction::Keep => credential_present(profile_id),
                CredentialAction::Set => true,
                CredentialAction::Clear => false,
            };
            save_profile(*profile, None)?;
            apply_credential_update(profile_id, credential.as_deref().map(String::as_str))?;
            mark_central_committed(&persisted.plan)?;
            reapply_model_consumers(
                &persisted.plan,
                profile_id,
                desired_credential_present,
                blocked_agents,
            )?;
            Ok(())
        }
        LifecycleBinding::ModelProviderUpsert {
            provider_id,
            profile_ids,
            draft_hash,
            credential_action,
        } => {
            let PendingAssetPayload::ModelProvider {
                provider,
                profiles,
                credential,
            } = require_pending_payload(&persisted.plan.operation_id)?
            else {
                return Err("asset_operation_expired: Model Provider draft or credential is unavailable; reopen the editor".into());
            };
            verify_payload_hash(&(provider.as_ref(), &profiles), draft_hash)?;
            if provider.id != *provider_id
                || profiles.keys().cloned().collect::<BTreeSet<_>>() != *profile_ids
                || credential_action_for(&credential) != *credential_action
            {
                return Err(
                    "asset_operation_stale: Model Provider draft no longer matches the reviewed plan"
                        .into(),
                );
            }
            let desired_credential_present = match credential_action {
                CredentialAction::Keep => provider_credential_present(provider_id),
                CredentialAction::Set => true,
                CredentialAction::Clear => false,
            };
            save_provider_bundle(*provider, profiles)?;
            apply_credential_update(&provider_credential_subject(provider_id),
                credential.as_deref().map(String::as_str))?;
            mark_central_committed(&persisted.plan)?;
            reapply_model_consumer_bundle(&persisted.plan, profile_ids,
                desired_credential_present, blocked_agents)
        }
        LifecycleBinding::ModelProviderDelete { provider_id } => {
            delete_provider(provider_id).map_err(ModelTargetError::from)
        }
        LifecycleBinding::ModelAdopt {
            profile_id,
            draft_hash,
            credential_action,
        } => {
            let PendingAssetPayload::Model {
                profile,
                credential,
            } = require_pending_payload(&persisted.plan.operation_id)?
            else {
                return Err("asset_operation_expired: Model adoption payload is unavailable; refresh the observed state".into());
            };
            verify_payload_hash(&profile, draft_hash)?;
            if profile.id != *profile_id || credential_action_for(&credential) != *credential_action
            {
                return Err("asset_operation_stale: Model adoption payload changed".into());
            }
            save_profile(*profile, None)?;
            // Adoption changes Arca authority to match the reviewed Agent state.
            // It must never normalize or rewrite that state; doing so can erase
            // unrelated external models from a shared Agent configuration.
            let DomainPlan::Model { after, .. } = &persisted.plan.domain_plan else {
                return Err("asset operation domain mismatch".into());
            };
            mutate_settings(|settings| {
                for (agent_id, selection) in after {
                    settings.set_model_selection(agent_id, selection.clone());
                }
            })
            .map_err(|error| error.to_string())?;
            apply_credential_update(profile_id, credential.as_deref().map(String::as_str))?;
            for agent_id in &persisted.plan.affected_agent_ids {
                clear_target_incidents(&persisted.plan, agent_id)?;
            }
            Ok(())
        }
        LifecycleBinding::ModelDelete { profile_id } => {
            apply_domain_plan(
                &persisted.plan,
                releases_relationship_ownership(&persisted.plan),
                skills_lock,
                blocked_agents,
            )?;
            delete_profile(profile_id).map_err(ModelTargetError::from)
        }
        LifecycleBinding::AgentCapabilities {
            agent_id,
            after,
            skill_assignments_after,
            skill_migration,
        } => {
            for entry in skill_migration {
                if let Some(source) = &entry.source {
                    create_skill_migration_link(source, &entry.destination)?;
                }
            }
            crate::agents::apply_configuration_patch(
                agent_id,
                after,
                skill_assignments_after.clone(),
            )
            .map_err(ModelTargetError::from)
        }
    }
}

fn verify_operation(persisted: &PersistedAssetOperation) -> Result<(), String> {
    verify_postcondition(&persisted.plan, persisted.lifecycle.as_ref())?;
    let Some(lifecycle) = &persisted.lifecycle else {
        return Ok(());
    };
    match lifecycle {
        LifecycleBinding::McpClear { agent_id } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let has_consumptions = settings
                .mcp_consumptions
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .is_some_and(|records| !records.is_empty());
            let has_disabled = settings
                .disabled
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .is_some_and(|records| !records.is_empty());
            if has_consumptions || has_disabled {
                return Err("MCP clear desired state verification failed".into());
            }
            Ok(())
        }
        LifecycleBinding::ModelClear { agent_id, .. } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let has_consumptions = settings
                .model_consumptions
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .is_some_and(|records| !records.is_empty());
            let has_assignment = settings
                .model_assignments
                .as_ref()
                .is_some_and(|records| records.contains_key(agent_id));
            if has_consumptions || has_assignment {
                return Err("Model clear desired state verification failed".into());
            }
            Ok(())
        }
        LifecycleBinding::McpUpsert {
            key,
            draft_hash,
            previous_key,
            previous_source_id,
        } => {
            let entry = read_registry()
                .into_iter()
                .find(|entry| entry.key() == *key)
                .ok_or_else(|| "MCP central post-commit verification failed".to_string())?;
            // Provenance is normalized by the central writer and is not part of
            // the reviewed draft hash.
            let mut entry = entry;
            entry.origin = None;
            verify_payload_hash(&entry, draft_hash)?;
            if let Some(previous_key) = previous_key {
                let previous_source_id = previous_source_id
                    .as_deref()
                    .ok_or_else(|| "MCP rename source verification is unavailable".to_string())?;
                if mcp_source_copy_exists(previous_key, previous_source_id)
                    || read_registry()
                        .iter()
                        .any(|entry| entry.key() == *previous_key)
                {
                    return Err("old MCP identity still exists after rename".into());
                }
                let settings = load_settings_strict().map_err(|error| error.to_string())?;
                if settings
                    .mcp_consumptions
                    .iter()
                    .flatten()
                    .map(|(_, records)| records)
                    .any(|records| {
                        records.contains_key(previous_key)
                            || records
                                .get(key)
                                .is_some_and(|record| record.asset_key != *key)
                    })
                {
                    return Err("old MCP consumption identity still exists after rename".into());
                }
            }
            Ok(())
        }
        LifecycleBinding::McpAdopt {
            key,
            draft_hash,
            enabled,
        } => {
            if let Some(draft_hash) = draft_hash {
                let entry = read_registry()
                    .into_iter()
                    .find(|entry| entry.key() == *key)
                    .ok_or_else(|| "MCP central migration verification failed".to_string())?;
                let mut entry = entry;
                entry.origin = None;
                verify_payload_hash(&entry, draft_hash)?;
            }
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            for (agent_id, expected) in enabled {
                let actual = settings
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|records| records.get(agent_id))
                    .and_then(|records| records.get(key))
                    .map(|record| record.enabled);
                if actual != Some(*expected) {
                    return Err("MCP migration enabled state verification failed".into());
                }
            }
            Ok(())
        }
        LifecycleBinding::McpDelete {
            key,
            source_id,
            fallback_exists,
            effective_before,
        } => {
            if mcp_source_copy_exists(key, source_id) {
                return Err("MCP source copy still exists after deletion".into());
            }
            let effective_exists = read_registry().iter().any(|entry| entry.key() == *key);
            let expected_effective = !*effective_before || *fallback_exists;
            if effective_exists != expected_effective {
                return Err("MCP fallback state did not match the reviewed deletion".into());
            }
            Ok(())
        }
        LifecycleBinding::McpEnabled {
            agent_id,
            asset_key,
            after,
            ..
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let desired = settings
                .mcp_consumptions
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .and_then(|records| records.get(asset_key))
                .map(|record| record.enabled);
            if desired != Some(*after) {
                return Err("MCP enabled state did not match the reviewed change".into());
            }
            Ok(())
        }
        LifecycleBinding::McpEnabledBulk {
            agent_id,
            before,
            after,
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let records = settings
                .mcp_consumptions
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .ok_or_else(|| {
                    "MCP consumptions disappeared after bulk enabled-state update".to_string()
                })?;
            if before
                .keys()
                .any(|key| records.get(key).map(|record| record.enabled) != Some(*after))
            {
                return Err("MCP enabled states did not match the reviewed bulk change".into());
            }
            Ok(())
        }
        LifecycleBinding::SkillEnabled {
            name,
            target_id,
            affected_agent_ids,
            after,
            ..
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let desired = settings
                .skill_assignments
                .as_ref()
                .and_then(|assignments| assignments.get(name))
                .is_some_and(|targets| targets.contains(target_id));
            let enabled = settings
                .skill_consumptions
                .as_ref()
                .and_then(|skills| skills.get(name))
                .and_then(|targets| targets.get(target_id))
                .map(|record| record.enabled);
            if !desired || enabled != Some(*after) {
                return Err("Skill enabled state did not match the reviewed change".into());
            }
            let _ = affected_agent_ids;
            Ok(())
        }
        LifecycleBinding::McpReapply { key, agent_ids } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            for agent_id in agent_ids {
                let record = settings
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|records| records.get(agent_id))
                    .and_then(|records| records.get(key))
                    .filter(|record| record.asset_key == *key)
                    .ok_or_else(|| {
                        "MCP reapply consumption identity verification failed".to_string()
                    })?;
                let _ = record;
            }
            Ok(())
        }
        LifecycleBinding::ModelReapply {
            agent_id,
            profile_id,
            enabled,
            active,
            ..
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let selection = settings.model_selection(agent_id);
            if selection
                .profiles
                .get(profile_id)
                .is_none_or(|record| record.enabled != *enabled)
                || (selection.active_profile_id.as_deref() == Some(profile_id)) != *active
            {
                return Err("Model physical state did not match the reviewed reapply".into());
            }
            Ok(())
        }
        LifecycleBinding::SkillReapply {
            agent_id,
            name,
            target_id,
            affected_agent_ids,
            enabled,
        } => {
            if !affected_agent_ids
                .iter()
                .any(|affected| affected == agent_id)
            {
                return Err("Skill reapply omitted the selected Agent".into());
            }
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let assigned = settings
                .skill_assignments
                .as_ref()
                .and_then(|assignments| assignments.get(name))
                .is_some_and(|targets| targets.contains(target_id));
            let desired_enabled = settings
                .skill_consumptions
                .as_ref()
                .and_then(|skills| skills.get(name))
                .and_then(|targets| targets.get(target_id))
                .map(|record| record.enabled)
                .unwrap_or(true);
            if !assigned || desired_enabled != *enabled {
                return Err("Skill desired state did not match the reviewed reapply".into());
            }
            Ok(())
        }
        LifecycleBinding::SkillAdoptObserved { name, record } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            if settings
                .managed_skills
                .as_ref()
                .and_then(|records| records.get(name))
                != Some(record.as_ref())
            {
                return Err("Skill adopted baseline was not persisted".into());
            }
            let inventory =
                crate::resources::skill::list_inventory().map_err(|error| format!("{error:?}"))?;
            let current = inventory.items.iter().find(|item| {
                item.name == *name
                    && matches!(
                        item.location,
                        crate::resources::skill::SkillLocation::Central
                    )
            });
            if current.is_none_or(|item| {
                item.content_hash.as_ref() != Some(&record.content_hash)
                    || item
                        .states
                        .contains(&crate::resources::skill::InventoryState::LocallyModified)
            }) {
                return Err("Skill adopted baseline verification failed".into());
            }
            Ok(())
        }
        LifecycleBinding::ModelUpsert {
            profile_id,
            draft_hash,
            credential_action,
        } => {
            let profile = load_settings_strict()
                .map_err(|error| error.to_string())?
                .model_profiles
                .and_then(|profiles| profiles.get(profile_id).cloned())
                .ok_or_else(|| "Model Profile missing after commit".to_string())?;
            verify_payload_hash(&profile, draft_hash)?;
            match credential_action {
                CredentialAction::Keep => {}
                CredentialAction::Set if !credential_present(profile_id) => {
                    return Err("Model credential was not saved after commit".into())
                }
                CredentialAction::Clear if credential_present(profile_id) => {
                    return Err("Model credential was not cleared after commit".into())
                }
                CredentialAction::Set | CredentialAction::Clear => {}
            }
            Ok(())
        }
        LifecycleBinding::ModelProviderUpsert {
            provider_id,
            profile_ids,
            draft_hash,
            credential_action,
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let provider = settings
                .model_providers
                .as_ref()
                .and_then(|providers| providers.get(provider_id))
                .ok_or_else(|| "Model Provider missing after commit".to_string())?;
            let profiles = profile_ids
                .iter()
                .map(|profile_id| {
                    settings
                        .model_profiles
                        .as_ref()
                        .and_then(|profiles| profiles.get(profile_id))
                        .cloned()
                        .map(|profile| (profile_id.clone(), profile))
                        .ok_or_else(|| "Provider child Model missing after commit".to_string())
                })
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            verify_payload_hash(&(provider, &profiles), draft_hash)?;
            match credential_action {
                CredentialAction::Keep => {}
                CredentialAction::Set if !provider_credential_present(provider_id) => {
                    return Err("Provider credential was not saved after commit".into())
                }
                CredentialAction::Clear if provider_credential_present(provider_id) => {
                    return Err("Provider credential was not cleared after commit".into())
                }
                CredentialAction::Set | CredentialAction::Clear => {}
            }
            Ok(())
        }
        LifecycleBinding::ModelProviderDelete { provider_id } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            if settings
                .model_providers
                .as_ref()
                .is_some_and(|providers| providers.contains_key(provider_id))
            {
                return Err("Model Provider deletion postcondition failed".into());
            }
            if provider_credential_present(provider_id) {
                return Err("Model Provider credential still exists after deletion".into());
            }
            Ok(())
        }
        LifecycleBinding::ModelAdopt {
            profile_id,
            draft_hash,
            credential_action,
        } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            let profile = settings
                .model_profiles
                .as_ref()
                .and_then(|profiles| profiles.get(profile_id))
                .ok_or_else(|| "Model Profile missing after adoption".to_string())?;
            verify_payload_hash(profile, draft_hash)?;
            match credential_action {
                CredentialAction::Keep => {}
                CredentialAction::Set if !credential_present(profile_id) => {
                    return Err("Model credential was not saved after adoption".into())
                }
                CredentialAction::Clear if credential_present(profile_id) => {
                    return Err("Model credential was not cleared after adoption".into())
                }
                CredentialAction::Set | CredentialAction::Clear => {}
            }
            Ok(())
        }
        LifecycleBinding::ModelDelete { profile_id } => {
            let settings = load_settings_strict().map_err(|error| error.to_string())?;
            if settings
                .model_profiles
                .as_ref()
                .is_some_and(|profiles| profiles.contains_key(profile_id))
                || settings
                    .model_assignments
                    .as_ref()
                    .is_some_and(|assignments| assignments.values().any(|id| id == profile_id))
                || settings
                    .model_consumptions
                    .as_ref()
                    .is_some_and(|consumptions| {
                        consumptions
                            .values()
                            .any(|records| records.contains_key(profile_id))
                    })
            {
                return Err("Model Profile deletion postcondition failed".into());
            }
            if credential_present(profile_id) {
                return Err("Model credential still exists after deletion".into());
            }
            Ok(())
        }
        LifecycleBinding::AgentCapabilities {
            agent_id,
            after,
            skill_migration,
            ..
        } => {
            if crate::agents::current_configuration_patch(agent_id)? != *after {
                return Err("Agent capability configuration postcondition failed".into());
            }
            for entry in skill_migration {
                let actual = skill_content_hash(&entry.destination)?;
                if actual.as_deref() != Some(entry.content_hash.as_str()) {
                    return Err("Skills path migration postcondition failed".into());
                }
            }
            Ok(())
        }
    }
}

fn require_pending_payload(operation_id: &str) -> Result<PendingAssetPayload, String> {
    pending_payload(operation_id).ok_or_else(|| {
        "asset_operation_expired: sensitive central draft was not persisted; reopen the editor"
            .to_string()
    })
}

fn verify_payload_hash<T: serde::Serialize>(value: &T, expected: &str) -> Result<(), String> {
    if super::payload_hash::matches(value, expected)? {
        Ok(())
    } else {
        Err("asset_operation_stale: central draft changed after review".into())
    }
}

fn credential_action_for(credential: &Option<Zeroizing<String>>) -> CredentialAction {
    match credential.as_deref().map(String::as_str) {
        None => CredentialAction::Keep,
        Some("") => CredentialAction::Clear,
        Some(_) => CredentialAction::Set,
    }
}

fn delete_mcp_source_copy(key: &str, source_id: &str) -> Result<(), String> {
    let (name, transport) = split_mcp_key(key)?;
    match source_id {
        "manual" => delete_registry_entry(name, transport),
        "discovered" => delete_discovered_entry(name, transport),
        _ => return Err("asset_read_only: MCP source copy is not user-owned".into()),
    }
    .map_err(|error| error.to_string())
}

fn mcp_source_copy_exists(key: &str, source_id: &str) -> bool {
    read_registry_all().into_iter().any(|item| {
        item.entry.key() == key
            && item.entry.origin.as_ref().is_some_and(|origin| {
                origin.source.as_deref() == Some(source_id) || origin.kind == source_id
            })
    })
}

fn reapply_mcp_consumers(
    operation: &AssetOperationPlan,
    key: &str,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let DomainPlan::Mcp { after, .. } = &operation.domain_plan else {
        return Err("asset operation domain mismatch".into());
    };
    let agent_ids = after
        .iter()
        .filter(|(_, desired)| desired.iter().any(|candidate| candidate == key))
        .map(|(agent_id, _)| agent_id.clone())
        .collect::<Vec<_>>();
    reapply_mcp_consumers_for_agents(operation, key, &agent_ids, blocked_agents)
}

fn reapply_mcp_consumers_for_agents(
    operation: &AssetOperationPlan,
    key: &str,
    agent_ids: &[String],
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let (name, transport) = split_mcp_key(key)?;
    for agent_id in agent_ids {
        if blocked_agents.contains(agent_id) {
            continue;
        }
        let patch = settings
            .mcp_consumptions
            .as_ref()
            .and_then(|records| records.get(agent_id))
            .and_then(|records| records.get(key))
            .map(|record| record.overrides.clone())
            .unwrap_or_default();
        let result = (|| {
            ops::install(
                name,
                transport,
                "global",
                std::slice::from_ref(agent_id),
                None,
                &HashMap::from([(agent_id.clone(), patch)]),
            )
            .map_err(|errors| errors.join("; "))?;
            let enabled = settings
                .mcp_consumptions
                .as_ref()
                .and_then(|records| records.get(agent_id))
                .and_then(|records| records.get(key))
                .is_none_or(|record| record.enabled);
            if !enabled || ops::uses_native_enabled_state(agent_id) {
                let set_enabled = if enabled { ops::enable } else { ops::disable };
                set_enabled(
                    name,
                    transport,
                    "global",
                    std::slice::from_ref(agent_id),
                    None,
                )
                .map_err(|errors| errors.join("; "))?;
            }
            Ok::<_, String>(())
        })();
        if result.is_ok() {
            clear_target_incidents(operation, agent_id)?;
        } else {
            record_target_incident(operation, agent_id, "target_convergence_failed")?;
        }
    }
    Ok(())
}

fn migrate_mcp_consumption_records(old_key: &str, new_key: &str) -> Result<(), String> {
    mutate_settings(|settings| {
        for records in settings
            .mcp_consumptions
            .iter_mut()
            .flatten()
            .map(|(_, records)| records)
        {
            if records.contains_key(new_key) {
                return Err(format!(
                    "asset_identity_conflict: MCP consumption already contains {new_key}"
                ));
            }
            let Some(mut record) = records.remove(old_key) else {
                continue;
            };
            record.asset_key = new_key.to_string();
            records.insert(new_key.to_string(), record);
        }
        Ok(())
    })
    .map_err(|error| error.to_string())??;
    Ok(())
}

/// Adopt exact observed MCP copies without rewriting Agent files. The planner
/// already bound every target byte and verified that all copies match one
/// central config. Disabled observations remain in the existing snapshot store
/// and are recorded as disabled desired relationships.
fn apply_mcp_adoption(
    operation: &AssetOperationPlan,
    key: &str,
    enabled: &BTreeMap<String, bool>,
) -> Result<(), String> {
    let DomainPlan::Mcp { after, .. } = &operation.domain_plan else {
        return Err("asset operation domain mismatch".into());
    };
    mutate_settings(|settings| {
        let all = settings.mcp_consumptions.get_or_insert_default();
        for (agent_id, desired) in after {
            if !desired.iter().any(|candidate| candidate == key) {
                continue;
            }
            let records = all.entry(agent_id.clone()).or_default();
            let mut record = records.remove(key).unwrap_or(McpConsumptionRecord {
                asset_key: key.to_string(),
                enabled: true,
                overrides: OverridePatch::default(),
            });
            record.enabled = enabled.get(agent_id).copied().unwrap_or(true);
            records.insert(key.to_string(), record);
        }
    })
    .map_err(|error| error.to_string())?;
    for agent_id in after.keys() {
        clear_target_incidents(operation, agent_id)?;
    }
    Ok(())
}

fn reapply_model_consumers(
    operation: &AssetOperationPlan,
    profile_id: &str,
    credential_present: bool,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), ModelTargetError> {
    reapply_model_consumer_bundle(operation, &BTreeSet::from([profile_id.to_string()]),
        credential_present, blocked_agents)
}

fn reapply_model_consumer_bundle(
    operation: &AssetOperationPlan,
    profile_ids: &BTreeSet<String>,
    credential_present: bool,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), ModelTargetError> {
    let DomainPlan::Model { after, .. } = &operation.domain_plan else {
        return Err("asset operation domain mismatch".into());
    };
    for (agent_id, desired) in after {
        if blocked_agents.contains(agent_id) { continue; }
        let result = (|| {
            for profile_id in profile_ids {
                if desired.profiles.get(profile_id).is_some_and(|record| record.enabled) {
                    apply_profile_consumption_with_credential_presence_target(agent_id, profile_id,
                        credential_present, desired.active_profile_id.as_deref() == Some(profile_id))?;
                }
            }
            Ok::<_, ModelTargetError>(())
        })();
        match result {
            Ok(_) => clear_target_incidents(operation, agent_id)?,
            Err(ModelTargetError::ConvergenceFailed(_)) => {
                record_target_incident(operation, agent_id, "target_convergence_failed")?;
            }
            Err(error @ ModelTargetError::RecoveryRequired(_)) => return Err(error),
        }
    }
    Ok(())
}

fn verify_request(
    persisted: &PersistedAssetOperation,
    request: &AssetCommitRequest,
) -> Result<(), String> {
    if persisted.plan.operation_id != request.operation_id
        || persisted.plan.candidate_hash != request.candidate_hash
    {
        return Err("asset_operation_stale: confirmation does not match the reviewed plan".into());
    }
    Ok(())
}

fn verify_preconditions(persisted: &PersistedAssetOperation) -> Result<(), String> {
    if persisted.schema_version == 2 {
        let current_hash = hash_file(&settings_file());
        if persisted.settings_hash.as_deref() != Some(current_hash.as_str()) {
            return Err("asset_operation_stale: Arca settings changed after review".into());
        }
    } else {
        AssetStateStore::load()?.verify(&persisted.state_preconditions)?;
    }
    let settings_target_hash = if persisted.schema_version == 2 {
        hash_target(&settings_file())
    } else {
        hash_settings_target(&settings_file())
    };
    if settings_target_hash != persisted.settings_target_hash {
        return Err("asset_operation_stale: Arca settings target changed after review".into());
    }
    if hash_targets(&persisted.plan.target_files) != persisted.target_hashes {
        return Err("asset_operation_stale: an Agent target changed after review".into());
    }
    if persisted.schema_version == 2 {
        if let Some(expected) = &persisted.mcp_catalog_hash {
            if &hash_mcp_catalog()? != expected {
                return Err(
                    "asset_operation_stale: central MCP catalog changed after review".into(),
                );
            }
        }
        if let Some(expected) = &persisted.skill_target_graph_hash {
            if &hash_skill_target_graph()? != expected {
                return Err(
                    "asset_operation_stale: Skill target graph changed after review".into(),
                );
            }
        }
    }
    if let Some(LifecycleBinding::ModelReapply {
        profile_id,
        credential_present: reviewed,
        ..
    }) = &persisted.lifecycle
    {
        if credential_present(profile_id) != *reviewed {
            return Err(
                "asset_operation_stale: Model credential state changed after review".into(),
            );
        }
    }
    if let Some(LifecycleBinding::AgentCapabilities {
        skill_migration, ..
    }) = &persisted.lifecycle
    {
        verify_skill_migration_preconditions(skill_migration)?;
    }
    let current = load_settings_strict().map_err(|error| error.to_string())?;
    match &persisted.plan.domain_plan {
        DomainPlan::Mcp { before, .. } => {
            for (agent_id, expected) in before {
                let actual: Vec<String> = current
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|records| records.get(agent_id))
                    .map(|records| records.keys().cloned().collect())
                    .unwrap_or_default();
                if &actual != expected {
                    return Err("asset_operation_stale: MCP relationships changed".into());
                }
            }
        }
        DomainPlan::Model { before, .. } => {
            for (agent_id, expected) in before {
                let actual = current.model_selection(agent_id);
                if &actual != expected {
                    return Err("asset_operation_stale: Model relationship changed".into());
                }
            }
        }
        DomainPlan::Skill { .. } => {
            // Physical link and assignment preconditions are rechecked by the
            // existing Skills planner for every step.
        }
        DomainPlan::AgentCapabilities {
            agent_id, before, ..
        } => {
            let actual = crate::agents::current_configuration_patch(agent_id)?;
            if actual != **before {
                return Err("asset_operation_stale: Agent capability configuration changed".into());
            }
        }
    }
    Ok(())
}

fn verify_captured_snapshots(
    persisted: &PersistedAssetOperation,
    snapshots: &[PathSnapshot],
) -> Result<(), String> {
    let settings_path = settings_file();
    let settings = snapshots
        .iter()
        .find(|snapshot| snapshot.path == settings_path)
        .ok_or_else(|| "asset_operation_stale: settings snapshot is missing".to_string())?;
    let settings_target_changed = if persisted.schema_version == 2 {
        persisted.settings_hash.as_deref() != Some(settings.path_hash().as_str())
            || settings.target_fingerprint()? != persisted.settings_target_hash
    } else {
        hash_settings_target(&settings_path) != persisted.settings_target_hash
    };
    if settings_target_changed {
        return Err("asset_operation_stale: Arca settings changed while preparing commit".into());
    }
    if persisted.schema_version >= 3 {
        // The settings lock is still held. Recompute the semantic revisions
        // after snapshot capture so a non-cooperating write between the first
        // check and rollback preparation cannot escape review.
        AssetStateStore::load()?.verify(&persisted.state_preconditions)?;
    }

    let snapshots_by_path = snapshots
        .iter()
        .map(|snapshot| (snapshot.path.clone(), snapshot))
        .collect::<BTreeMap<_, _>>();
    for target in &persisted.plan.target_files {
        let path = crate::resources::mcp::scanner::expand_tilde(target);
        let snapshot = snapshots_by_path.get(&path).ok_or_else(|| {
            "asset_operation_stale: reviewed Agent target snapshot is missing".to_string()
        })?;
        let expected = persisted.target_hashes.get(target).ok_or_else(|| {
            "asset_operation_stale: reviewed Agent target fingerprint is missing".to_string()
        })?;
        if snapshot.target_fingerprint()? != *expected {
            return Err(
                "asset_operation_stale: an Agent target changed while preparing commit".into(),
            );
        }
    }
    Ok(())
}

fn verify_skill_migration_preconditions(entries: &[SkillMigrationEntry]) -> Result<(), String> {
    for entry in entries {
        match &entry.source {
            Some(source) => {
                if skill_content_hash(source)?.as_deref() != Some(entry.content_hash.as_str()) {
                    return Err("asset_operation_stale: a source Skill changed after review".into());
                }
                if fs::symlink_metadata(expand_tilde_path(&entry.destination)).is_ok() {
                    return Err(
                        "asset_operation_stale: a Skills migration destination appeared after review"
                            .into(),
                    );
                }
            }
            None => {
                if skill_content_hash(&entry.destination)?.as_deref()
                    != Some(entry.content_hash.as_str())
                {
                    return Err(
                        "asset_operation_stale: a destination Skill changed after review".into(),
                    );
                }
            }
        }
    }
    Ok(())
}

fn create_skill_migration_link(source: &str, destination: &str) -> Result<(), String> {
    let source = expand_tilde_path(source);
    let destination = expand_tilde_path(destination);
    let source = fs::canonicalize(source)
        .map_err(|_| "Skills migration source is unavailable".to_string())?;
    if !source.is_dir() {
        return Err("Skills migration source is not a directory".into());
    }
    create_transaction_symlink_if_missing(&destination, &source)
}

fn skill_content_hash(path: &str) -> Result<Option<String>, String> {
    let path = expand_tilde_path(path);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(_) => {}
    }
    let canonical = fs::canonicalize(path)
        .map_err(|_| "Skills migration path could not be resolved".to_string())?;
    if !canonical.is_dir() {
        return Err("Skills migration path is not a directory".into());
    }
    crate::resources::skill::hash_tree(&canonical)
        .map(Some)
        .map_err(|error| format!("{error:?}"))
}

fn expand_tilde_path(path: &str) -> PathBuf {
    crate::resources::mcp::scanner::expand_tilde(path)
}

fn apply_domain_plan(
    operation: &AssetOperationPlan,
    release_orphaned_relationships: bool,
    skills_lock: &SkillsOperationLock,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), ModelTargetError> {
    let plan = &operation.domain_plan;
    match plan {
        DomainPlan::Mcp { before, after } => apply_mcp(
            operation,
            before,
            after,
            release_orphaned_relationships,
            blocked_agents,
        )
        .map_err(ModelTargetError::from),
        DomainPlan::Model { before, after } => apply_model(
            operation,
            before,
            after,
            &model_relationship_releases(operation),
            &BTreeSet::new(),
            blocked_agents,
        ),
        DomainPlan::Skill { before, after } => apply_skill(
            operation, before, after, skills_lock, blocked_agents,
        )
        .map_err(ModelTargetError::from),
        DomainPlan::AgentCapabilities { .. } => Err(ModelTargetError::ConvergenceFailed(
            "asset operation requires a configuration lifecycle".into(),
        )),
    }
}

/// A pure relationship removal may release ownership when the Agent bytes no
/// longer match Arca authority. Synchronized entries are still removed
/// normally; missing central metadata or drifted bytes are preserved as
/// external observations. Toggle and update operations never enter this set.
fn model_relationship_releases(plan: &AssetOperationPlan) -> BTreeSet<(String, String)> {
    if !releases_relationship_ownership(plan) {
        return BTreeSet::new();
    }
    plan.relationship_changes
        .iter()
        .filter_map(|change| match (&change.asset, &change.action) {
            (AssetRef::Model { profile_id }, RelationshipAction::Remove) => {
                Some((change.agent_id.clone(), profile_id.clone()))
            }
            _ => None,
        })
        .collect()
}

fn apply_mcp(
    operation: &AssetOperationPlan,
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
    release_orphaned_relationships: bool,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    mutate_settings(|settings| {
        let all = settings.mcp_consumptions.get_or_insert_default();
        for agent_id in union_keys(before, after) {
            let existing = all.remove(agent_id).unwrap_or_default();
            let mut desired = BTreeMap::new();
            for key in after.get(agent_id).into_iter().flatten() {
                desired.insert(
                    key.clone(),
                    existing.get(key).cloned().unwrap_or(McpConsumptionRecord {
                        asset_key: key.clone(),
                        enabled: true,
                        overrides: OverridePatch::default(),
                    }),
                );
            }
            if !desired.is_empty() {
                all.insert(agent_id.clone(), desired);
            }
        }
    })
    .map_err(|error| error.to_string())?;
    if operation.kind == AssetOperationKind::SetConsumption { mark_central_committed(operation)?; }
    let central_keys: BTreeSet<String> = read_registry()
        .into_iter()
        .map(|entry| entry.key())
        .collect();
    let exact_observed: BTreeMap<(String, String), bool> = ops::scan_installed(None)
        .into_iter()
        .filter(|item| item.scope == "global" && !item.customized
            && (item.enabled || ops::uses_native_enabled_state(&item.agent)))
        .map(|item| ((item.agent, format!("{}::{}", item.name, item.transport)), item.enabled))
        .collect();
    for agent_id in union_keys(before, after) {
        if blocked_agents.contains(agent_id) {
            continue;
        }
        let left: BTreeSet<String> = before
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let right: BTreeSet<String> = after
            .get(agent_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let result = (|| {
            for key in left.difference(&right) {
                if release_orphaned_relationships
                    && (!central_keys.contains(key)
                        || !exact_observed.contains_key(&(agent_id.clone(), key.clone())))
                {
                    continue;
                }
                let (name, transport) = split_mcp_key(key)?;
                ops::delete(
                    name,
                    transport,
                    "global",
                    std::slice::from_ref(agent_id),
                    None,
                )
                .map_err(|errors| errors.join("; "))?;
            }
            // Reconcile the complete desired set, not only the relationship
            // delta. This lets an ordinary add/update repair a prior local
            // target incident instead of preserving missing historical entries.
            for key in &right {
                let desired_enabled = settings.mcp_consumptions.as_ref()
                    .and_then(|records| records.get(agent_id))
                    .and_then(|records| records.get(key))
                    .is_none_or(|record| record.enabled);
                if exact_observed.get(&(agent_id.clone(), key.clone())) == Some(&desired_enabled) {
                    continue;
                }
                let (name, transport) = split_mcp_key(key)?;
                let patch = settings
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|records| records.get(agent_id))
                    .and_then(|records| records.get(key))
                    .map(|record| record.overrides.clone())
                    .unwrap_or_default();
                ops::install(
                    name,
                    transport,
                    "global",
                    std::slice::from_ref(agent_id),
                    None,
                    &HashMap::from([(agent_id.clone(), patch)]),
                )
                .map_err(|errors| errors.join("; "))?;
                let enabled = settings
                    .mcp_consumptions
                    .as_ref()
                    .and_then(|records| records.get(agent_id))
                    .and_then(|records| records.get(key))
                    .is_none_or(|record| record.enabled);
                if !enabled || ops::uses_native_enabled_state(agent_id) {
                    let set_enabled = if enabled { ops::enable } else { ops::disable };
                    set_enabled(
                        name,
                        transport,
                        "global",
                        std::slice::from_ref(agent_id),
                        None,
                    )
                    .map_err(|errors| errors.join("; "))?;
                }
            }
            Ok::<_, String>(())
        })()
        .and_then(|_| {
            verify_agent_mcp_convergence(
                agent_id,
                &left,
                &right,
                &settings,
                release_orphaned_relationships,
            )
        });
        if result.is_ok() {
            clear_target_incidents(operation, agent_id)?;
        } else {
            record_target_incident(operation, agent_id, "target_convergence_failed")?;
        }
    }
    Ok(())
}

fn verify_agent_mcp_convergence(
    agent_id: &str,
    before: &BTreeSet<String>,
    desired: &BTreeSet<String>,
    settings: &crate::settings::Settings,
    release_orphaned_relationships: bool,
) -> Result<(), String> {
    let observed = ops::scan_installed(None)
        .into_iter()
        .filter(|item| item.scope == "global" && item.agent == agent_id)
        .map(|item| (format!("{}::{}", item.name, item.transport), item.enabled))
        .collect::<BTreeMap<_, _>>();
    for key in desired {
        let expected_enabled = settings
            .mcp_consumptions
            .as_ref()
            .and_then(|records| records.get(agent_id))
            .and_then(|records| records.get(key))
            .is_none_or(|record| record.enabled);
        if observed.get(key) != Some(&expected_enabled) {
            return Err(format!(
                "target_convergence_failed: {agent_id} MCP target does not contain {key} in its desired state"
            ));
        }
    }
    if !release_orphaned_relationships {
        for key in before.difference(desired) {
            if observed.contains_key(key) {
                return Err(format!(
                    "target_convergence_failed: {agent_id} MCP target still contains removed asset {key}"
                ));
            }
        }
    }
    Ok(())
}

fn apply_mcp_enabled(
    operation: &AssetOperationPlan,
    agent_id: &str,
    asset_key: &str,
    enabled: bool,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let updated = mutate_settings(|settings| {
        let Some(record) = settings
            .mcp_consumptions
            .as_mut()
            .and_then(|records| records.get_mut(agent_id))
            .and_then(|records| records.get_mut(asset_key))
        else {
            return false;
        };
        record.enabled = enabled;
        true
    })
    .map_err(|error| error.to_string())?;
    if !updated {
        return Err("MCP consumption disappeared during enabled-state update".into());
    }
    if blocked_agents.contains(agent_id) {
        return Ok(());
    }
    let (name, transport) = split_mcp_key(asset_key)?;
    let agents = [agent_id.to_string()];
    let result = if enabled {
        ops::enable(name, transport, "global", &agents, None)
    } else {
        ops::disable(name, transport, "global", &agents, None)
    }
    .and_then(|_| {
        let observed = ops::scan_installed(None).into_iter().find(|item| {
            item.scope == "global"
                && item.agent == agent_id
                && item.name == name
                && item.transport == transport
        });
        if observed.is_some_and(|item| item.enabled == enabled) {
            Ok(())
        } else {
            Err(vec![format!(
                "target_convergence_failed: {agent_id} MCP enabled state was not persisted"
            )])
        }
    });
    if result.is_ok() {
        clear_target_incidents(operation, agent_id)
    } else {
        record_target_incident(operation, agent_id, "target_convergence_failed")
    }
}

fn apply_skill_enabled(
    operation: &AssetOperationPlan,
    name: &str,
    target_id: &str,
    affected_agent_ids: &[String],
    enabled: bool,
    skills_lock: &SkillsOperationLock,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let persist_desired = || {
        mutate_settings(|settings| {
            settings
                .skill_assignments
                .get_or_insert_default()
                .entry(name.to_string())
                .or_default()
                .insert(target_id.to_string());
            settings
                .skill_consumptions
                .get_or_insert_default()
                .entry(name.to_string())
                .or_default()
                .insert(
                    target_id.to_string(),
                    SkillConsumptionRecord {
                        name: name.to_string(),
                        target_id: target_id.to_string(),
                        enabled,
                    },
                );
        })
        .map_err(|error| error.to_string())
    };
    persist_desired()?;
    mark_central_committed(operation)?;
    if affected_agent_ids.iter().any(|agent_id| blocked_agents.contains(agent_id)) {
        return Ok(());
    }
    let target = crate::resources::skill::list_inventory()
        .map_err(|error| format!("{error:?}"))?.targets.into_iter()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| "skill_target_unavailable: the reviewed target is unavailable".to_string())?;
    let result = reapply_assignment_safely(name, target_id, skills_lock);
    record_skill_physical_result(operation, &target.global_dir, affected_agent_ids, result.is_ok())
}

fn apply_model(
    operation: &AssetOperationPlan,
    before: &BTreeMap<String, ModelAgentSelection>,
    after: &BTreeMap<String, ModelAgentSelection>,
    relationship_releases: &BTreeSet<(String, String)>,
    preserve_agent_targets: &BTreeSet<String>,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), ModelTargetError> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let central_profile_ids: BTreeSet<String> = settings
        .model_profiles
        .iter()
        .flatten()
        .map(|(profile_id, _)| profile_id.clone())
        .collect();
    mutate_settings(|settings| {
        for agent_id in union_keys(before, after) {
            settings
                .set_model_selection(agent_id, after.get(agent_id).cloned().unwrap_or_default());
        }
    })
    .map_err(|error| error.to_string())?;
    let credential_policy_change = union_keys(before, after).iter().any(|agent_id| {
        let left = before.get(*agent_id).cloned().unwrap_or_default();
        let right = after.get(*agent_id).cloned().unwrap_or_default();
        left.default_delivery != right.default_delivery
            || right.profiles.keys().any(|id| left.delivery_for(id) != right.delivery_for(id))
    });
    // Credential policy updates retain their single-target atomic rollback.
    if !credential_policy_change && operation.kind == AssetOperationKind::SetConsumption {
        mark_central_committed(operation)?;
    }
    for agent_id in union_keys(before, after) {
        let left = before.get(agent_id).cloned().unwrap_or_default();
        let right = after.get(agent_id).cloned().unwrap_or_default();
        if preserve_agent_targets.contains(agent_id) || blocked_agents.contains(agent_id) {
            continue;
        }
        let result = (|| {
            let removed_or_disabled: Vec<String> = left
                .profiles
                .iter()
                .filter(|(profile_id, record)| {
                    record.enabled
                        && !right
                            .profiles
                            .get(*profile_id)
                            .is_some_and(|next| next.enabled)
                })
                .map(|(profile_id, _)| profile_id.clone())
                .collect();
            for profile_id in removed_or_disabled {
                let release_relationship =
                    relationship_releases.contains(&(agent_id.to_string(), profile_id.clone()));
                if release_relationship && !central_profile_ids.contains(&profile_id) {
                    continue;
                }
                if let Err(error) = clear_profile_consumption_target(
                    agent_id,
                    &profile_id,
                    left.active_profile_id.as_deref() == Some(profile_id.as_str()),
                ) {
                    match error {
                        ModelTargetError::ConvergenceFailed(message) => {
                            let released_as_external = release_relationship
                                && (message.starts_with("model_owned_fields_drift:")
                                    || message.starts_with("model_target_conflicted:"));
                            if !released_as_external {
                                return Err(ModelTargetError::ConvergenceFailed(message));
                            }
                        }
                        error @ ModelTargetError::RecoveryRequired(_) => return Err(error),
                    }
                }
            }
            for (profile_id, record) in &right.profiles {
                let was_enabled = left
                    .profiles
                    .get(profile_id)
                    .is_some_and(|previous| previous.enabled);
                let credential_changed = left.delivery_for(profile_id) != right.delivery_for(profile_id);
                if record.enabled && (!was_enabled || credential_changed) {
                    apply_profile_consumption_target(
                        agent_id,
                        profile_id,
                        right.active_profile_id.as_deref() == Some(profile_id),
                    )?;
                }
            }
            if left.active_profile_id != right.active_profile_id
                && right.active_profile_id.as_ref().is_some_and(|profile_id| {
                    left.profiles
                        .get(profile_id)
                        .is_some_and(|record| record.enabled)
                })
            {
                let profile_id = right.active_profile_id.as_deref().expect("checked above");
                apply_profile_target(agent_id, profile_id)?;
            }
            Ok::<_, ModelTargetError>(())
        })();
        match result {
            Ok(()) => clear_target_incidents(operation, agent_id)?,
            Err(ModelTargetError::ConvergenceFailed(message)) => {
                // This is one reviewed credential-policy write set. A later
                // profile failure must restore earlier writes, not merely the
                // central policy. The outer durable transaction owns rollback.
                if left.default_delivery != right.default_delivery
                    || right.profiles.keys().any(|id| left.delivery_for(id) != right.delivery_for(id)) {
                    return Err(ModelTargetError::ConvergenceFailed(message));
                }
                record_target_incident(operation, agent_id, "target_convergence_failed")?;
            }
            Err(error @ ModelTargetError::RecoveryRequired(_)) => return Err(error),
        }
    }
    Ok(())
}

fn apply_skill(
    operation: &AssetOperationPlan,
    before: &BTreeMap<String, Vec<String>>,
    after: &BTreeMap<String, Vec<String>>,
    skills_lock: &SkillsOperationLock,
    blocked_agents: &BTreeSet<String>,
) -> Result<(), String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let assignments = canonical_skill_assignments(&settings).map_err(|error| format!("{error:?}"))?;
    let inventory = crate::resources::skill::list_inventory().map_err(|error| format!("{error:?}"))?;
    let reviewed_paths = operation.target_files.iter().map(|path| expand_tilde_path(path)).collect::<BTreeSet<_>>();
    let changed_names = union_keys(before, after).into_iter().flat_map(|agent| {
        let left = before.get(agent).into_iter().flatten().cloned().collect::<BTreeSet<_>>();
        let right = after.get(agent).into_iter().flatten().cloned().collect::<BTreeSet<_>>();
        left.symmetric_difference(&right).cloned().collect::<Vec<_>>()
    }).collect::<BTreeSet<_>>();
    let mut groups = BTreeMap::<String, BTreeMap<String, bool>>::new();
    // A domain plan contains only the affected Agent closure, not every
    // consumer. Merge its physical delta into complete central authority.
    for name in changed_names {
        let assigned = assignments.get(&name).cloned().unwrap_or_default();
        let removed_agents = before.iter().filter(|(agent, names)| names.contains(&name)
            && !after.get(*agent).is_some_and(|names| names.contains(&name)))
            .map(|(agent, _)| agent.clone()).collect::<Vec<_>>();
        let desired_agents = after.iter().filter(|(_, names)| names.contains(&name))
            .map(|(agent, _)| agent.clone()).collect::<Vec<_>>();
        let declared = declared_targets_for_agents(&removed_agents).map_err(|error| format!("{error:?}"))?;
        // Normalize the whole reviewed closure once. Normalizing each Agent
        // separately invents redundant primary targets for shared aliases.
        let desired = normalize_agent_selection(&desired_agents).map_err(|error| format!("{error:?}"))?
            .into_iter().collect::<BTreeSet<_>>();
        let removals = assigned.intersection(&declared).filter(|target| !desired.contains(*target))
            .cloned().map(|target| (target, false));
        let additions = desired.difference(&assigned).cloned().map(|target| (target, true));
        for (target_id, enabled) in removals.chain(additions) {
            let target = inventory.targets.iter().find(|target| target.target_id == target_id)
                .ok_or_else(|| "skill_target_unavailable: the reviewed target is unavailable".to_string())?;
            let path = expand_tilde_path(&target.global_dir).join(&name);
            if !reviewed_paths.contains(&path) {
                return Err("asset_operation_stale: Skill delta exceeds the reviewed physical write set".into());
            }
            groups.entry(target_id).or_default().insert(name.clone(), enabled);
        }
    }
    // The reviewed write set also contains existing targets of changed Skills.
    // They require no new link mutation, but must be checkpointed unchanged so
    // a successful add on another target does not invent incidents for them.
    for target in &inventory.targets {
        let directory = expand_tilde_path(&target.global_dir);
        if operation.target_files.iter().any(|path|
            expand_tilde_path(path).parent() == Some(directory.as_path())) {
            groups.entry(target.target_id.clone()).or_default();
        }
    }
    persist_skill_desired(&groups)?;
    mark_central_committed(operation)?;
    for (target_id, changes) in groups {
        let target = inventory.targets.iter().find(|target| target.target_id == target_id)
            .ok_or_else(|| "skill_target_unavailable: the reviewed target is unavailable".to_string())?;
        let agents = if target.affected_agent_ids.is_empty() { &target.primary_agent_ids } else { &target.affected_agent_ids };
        if agents.iter().any(|agent| blocked_agents.contains(agent)) { continue; }
        if changes.is_empty() {
            // This target was captured only to bound the review. Preserve any
            // earlier incident: checkpointing untouched bytes is not a repair.
            let directory = expand_tilde_path(&target.global_dir);
            let paths = operation.target_files.iter().filter(|path|
                expand_tilde_path(path).parent() == Some(directory.as_path()))
                .cloned().collect::<Vec<_>>();
            mark_physical_target_completed(operation, &directory, &paths)?;
            continue;
        }
        let result = changes.into_iter().try_for_each(|(name, desired)| {
            if desired {
                reapply_assignment_safely(&name, &target_id, skills_lock).map(|_| ())
            } else {
                release_assignment_links_safely(&name, target, skills_lock).map(|_| ())
            }
        });
        record_skill_physical_result(operation, &target.global_dir, agents, result.is_ok())?;
    }
    Ok(())
}

fn record_skill_physical_result(
    operation: &AssetOperationPlan,
    global_dir: &str,
    agent_ids: &[String],
    success: bool,
) -> Result<(), String> {
    let directory = expand_tilde_path(global_dir);
    let paths = operation.target_files.iter().filter(|path|
        expand_tilde_path(path).parent() == Some(directory.as_path()))
        .cloned().collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("target_recovery_required: Skill target is outside the reviewed write set".into());
    }
    let mut target_plan = operation.clone();
    target_plan.target_files = paths.clone();
    target_plan.affected_agent_ids = agent_ids.to_vec();
    if success {
        let ids = paths.iter().map(|path| incident_id(AssetCapability::Skill, path)).collect::<BTreeSet<_>>();
        mutate_settings(|settings| {
            if let Some(incidents) = settings.target_incidents.as_mut() {
                incidents.retain(|id, _| !ids.contains(id));
                if incidents.is_empty() { settings.target_incidents = None; }
            }
        }).map_err(|error| error.to_string())?;
        mark_physical_target_completed(operation, &directory, &paths)
    } else {
        for agent_id in agent_ids {
            record_target_incident(&target_plan, agent_id, "target_convergence_failed")?;
        }
        Ok(())
    }
}

fn persist_skill_desired(
    target_changes: &BTreeMap<String, BTreeMap<String, bool>>,
) -> Result<(), String> {
    mutate_settings(|settings| {
        let assignments = settings.skill_assignments.get_or_insert_default();
        let consumptions = settings.skill_consumptions.get_or_insert_default();
        for (target_id, changes) in target_changes {
            for (name, desired) in changes {
                if *desired {
                    assignments.entry(name.clone()).or_default().insert(target_id.clone());
                    consumptions.entry(name.clone()).or_default().entry(target_id.clone())
                        .or_insert_with(|| SkillConsumptionRecord {
                            name: name.clone(), target_id: target_id.clone(), enabled: true,
                        });
                } else {
                    if let Some(targets) = assignments.get_mut(name) {
                        targets.remove(target_id);
                        if targets.is_empty() { assignments.remove(name); }
                    }
                    if let Some(records) = consumptions.get_mut(name) {
                        records.remove(target_id);
                        if records.is_empty() { consumptions.remove(name); }
                    }
                }
            }
        }
        if assignments.is_empty() { settings.skill_assignments = None; }
        if consumptions.is_empty() { settings.skill_consumptions = None; }
    }).map_err(|error| error.to_string())
}

fn verify_postcondition(
    plan: &AssetOperationPlan,
    lifecycle: Option<&LifecycleBinding>,
) -> Result<(), String> {
    // No Agent relationships are written by a central-only MCP import.
    // Central payload verification and CAS still run in the transaction.
    let central_mcp_only = matches!(&plan.domain_plan, DomainPlan::Mcp { before, after }
        if before.is_empty() && after.is_empty());
    let inventory = if central_mcp_only { ConsumptionInventory::default() }
        else { list_consumption_inventory()? };
    match &plan.domain_plan {
        DomainPlan::Mcp { after, .. } => {
            for (agent_id, expected) in after {
                verify_desired_many(&inventory, agent_id, expected, |asset| match asset {
                    AssetRef::Mcp { key } => Some(key.as_str()),
                    _ => None,
                })?;
            }
        }
        DomainPlan::Model { after, .. } => {
            for (agent_id, expected) in after {
                let actual: BTreeMap<_, _> = inventory
                    .consumptions
                    .iter()
                    .filter(|item| item.agent_id == *agent_id && item.desired)
                    .filter_map(|item| match &item.asset {
                        AssetRef::Model { profile_id } => Some((profile_id, item)),
                        _ => None,
                    })
                    .collect();
                if actual.len() != expected.profiles.len() {
                    return Err(format!(
                        "model post-commit verification failed: expected {} Profiles for {agent_id}, observed {}",
                        expected.profiles.len(),
                        actual.len()
                    ));
                }
                for (profile_id, record) in &expected.profiles {
                    let Some(item) = actual.get(profile_id) else {
                        return Err(format!(
                            "model post-commit verification failed: {profile_id} is missing for {agent_id}"
                        ));
                    };
                    let expected_active =
                        expected.active_profile_id.as_deref() == Some(profile_id.as_str());
                    let invalid = item.enabled != Some(record.enabled)
                        || item.desired_active != Some(expected_active);
                    if invalid {
                        return Err(format!(
                            "model post-commit verification failed: {profile_id} for {agent_id} is {:?} with enabled {:?} and active {:?}",
                            item.status, item.enabled, item.active
                        ));
                    }
                }
            }
        }
        DomainPlan::Skill { after, .. } => {
            for (agent_id, expected) in after {
                verify_desired_many(&inventory, agent_id, expected, |asset| match asset {
                    AssetRef::Skill { name } => Some(name.as_str()),
                    _ => None,
                })?;
            }
        }
        DomainPlan::AgentCapabilities {
            agent_id,
            after,
            skills_after,
            ..
        } => {
            if crate::agents::current_configuration_patch(agent_id)? != **after {
                return Err(
                    "Agent capability configuration post-commit verification failed".into(),
                );
            }
            for (affected_agent, expected) in skills_after {
                verify_desired_many(&inventory, affected_agent, expected, |asset| match asset {
                    AssetRef::Skill { name } => Some(name.as_str()),
                    _ => None,
                })?;
            }
        }
    }

    for ((agent_id, asset), desired) in expected_effects(plan, lifecycle) {
        let consumption = inventory
            .consumptions
            .iter()
            .find(|item| item.agent_id == agent_id && item.asset == asset && item.desired);
        if desired {
            if consumption.is_none() {
                return Err(format!(
                    "asset post-commit verification failed: missing desired {asset:?} for {agent_id}"
                ));
            }
        } else if consumption.is_some() {
            return Err("asset removal post-commit verification failed".into());
        }
    }
    Ok(())
}

fn verify_desired_many<'a, F>(
    inventory: &'a ConsumptionInventory,
    agent_id: &str,
    expected: &[String],
    identity: F,
) -> Result<(), String>
where
    F: Fn(&'a AssetRef) -> Option<&'a str>,
{
    let actual: BTreeSet<&str> = inventory
        .consumptions
        .iter()
        .filter(|item| item.agent_id == agent_id && item.desired)
        .filter_map(|item| identity(&item.asset))
        .collect();
    let expected: BTreeSet<&str> = expected.iter().map(String::as_str).collect();
    if actual != expected {
        return Err(format!(
            "asset post-commit verification failed for {agent_id}: expected {expected:?}, observed {actual:?}"
        ));
    }
    Ok(())
}

fn expected_effects(
    plan: &AssetOperationPlan,
    lifecycle: Option<&LifecycleBinding>,
) -> BTreeMap<(String, AssetRef), bool> {
    super::planner::effect_assets(
        &plan.domain_plan,
        &plan.central_changes,
        &plan.relationship_changes,
        &plan.consumption_state_changes,
        lifecycle,
    )
    .into_iter()
    .map(|(agent_id, asset)| {
        let desired = asset_desired_after(&plan.domain_plan, &agent_id, &asset);
        ((agent_id, asset), desired)
    })
    .collect()
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

fn split_mcp_key(key: &str) -> Result<(&str, &str), String> {
    key.rsplit_once("::")
        .filter(|(name, transport)| !name.is_empty() && matches!(*transport, "stdio" | "http"))
        .ok_or_else(|| format!("invalid MCP asset key: {key}"))
}

fn union_keys<'a, T>(
    left: &'a BTreeMap<String, T>,
    right: &'a BTreeMap<String, T>,
) -> BTreeSet<&'a String> {
    left.keys().chain(right.keys()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum SnapshotPrivacy {
    Ordinary,
    Private,
}

impl Default for SnapshotPrivacy {
    fn default() -> Self {
        Self::Ordinary
    }
}

#[derive(PartialEq, Eq)]
enum SnapshotKind {
    Missing,
    File {
        bytes: Zeroizing<Vec<u8>>,
        mode: Option<u32>,
    },
    PrivateFile {
        bytes: Zeroizing<Vec<u8>>,
        mode: Option<u32>,
    },
    PrivateDigest {
        content_hash: String,
        mode: Option<u32>,
    },
    Symlink {
        target: PathBuf,
    },
    Directory,
}

#[derive(PartialEq, Eq)]
struct PathSnapshot {
    path: PathBuf,
    parent: ParentDirectorySnapshot,
    kind: SnapshotKind,
    identity: PathIdentity,
    privacy: SnapshotPrivacy,
}

fn validate_private_snapshot_aliases(
    snapshots: &[PathSnapshot],
    private_paths: &BTreeSet<PathBuf>,
) -> Result<(), String> {
    for private_path in private_paths {
        let private = snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.path == *private_path && snapshot.privacy == SnapshotPrivacy::Private
            })
            .collect::<Vec<_>>();
        let [private] = private.as_slice() else {
            return Err(format!(
                "claude_desktop_private_target_alias: reviewed fourth role was not captured as the private target: {}",
                private_path.display()
            ));
        };
        for ordinary in snapshots
            .iter()
            .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Ordinary)
        {
            let exact_identity_alias = private.identity.is_exact()
                && ordinary.identity.is_exact()
                && private.identity == ordinary.identity;
            let canonical_alias = if matches!(private.kind, SnapshotKind::Missing)
                || matches!(ordinary.kind, SnapshotKind::Missing)
            {
                false
            } else {
                let private_canonical = fs::canonicalize(&private.path).map_err(|error| {
                    format!(
                        "claude_desktop_path_identity_failed: {}: {error}",
                        private.path.display()
                    )
                })?;
                let ordinary_canonical = fs::canonicalize(&ordinary.path).map_err(|error| {
                    format!(
                        "claude_desktop_path_identity_failed: {}: {error}",
                        ordinary.path.display()
                    )
                })?;
                private_canonical == ordinary_canonical
            };
            if exact_identity_alias || canonical_alias {
                return Err(format!(
                    "claude_desktop_private_target_alias: ordinary target {} aliases the reviewed private target",
                    ordinary.path.display()
                ));
            }
        }
    }
    Ok(())
}

impl std::fmt::Debug for PathSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match &self.kind {
            SnapshotKind::Missing => "missing",
            SnapshotKind::File { .. } => "file",
            SnapshotKind::PrivateFile { .. } => "private-file",
            SnapshotKind::PrivateDigest { .. } => "private-digest",
            SnapshotKind::Symlink { .. } => "symlink",
            SnapshotKind::Directory => "directory",
        };
        formatter
            .debug_struct("PathSnapshot")
            .field("path", &self.path)
            .field("kind", &kind)
            .field(
                "privacy",
                &match self.privacy {
                    SnapshotPrivacy::Ordinary => "ordinary",
                    SnapshotPrivacy::Private => "private",
                },
            )
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableSnapshotManifest {
    version: u32,
    snapshots: Vec<DurableSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateTargetLedger {
    version: u32,
    targets: Vec<PrivateTargetLedgerEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateTargetLedgerEntry {
    path: PathBuf,
    subject: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DurableSnapshot {
    path: PathBuf,
    parent: ParentDirectorySnapshot,
    identity: PathIdentity,
    #[serde(default)]
    privacy: SnapshotPrivacy,
    kind: DurableSnapshotKind,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum DurableSnapshotKind {
    Missing,
    File {
        backup: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    PrivateFile {
        content_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
        subject: String,
    },
    Symlink {
        target: PathBuf,
    },
    Directory,
}

impl PathSnapshot {
    /// Ordinary settings/configuration targets must never be symlinks: their
    /// writers follow links, while a link-only snapshot cannot restore the
    /// destination content.
    fn capture(path: &Path) -> Result<Self, String> {
        let snapshot = Self::capture_any(path)?;
        match &snapshot.kind {
            SnapshotKind::Missing | SnapshotKind::File { .. } => Ok(snapshot),
            SnapshotKind::PrivateFile { .. } | SnapshotKind::PrivateDigest { .. } => {
                unreachable!("ordinary capture cannot create a private snapshot")
            }
            SnapshotKind::Symlink { .. } => Err(format!(
                "asset_target_unsafe: refusing to snapshot symlinked configuration target: {}",
                path.display()
            )),
            SnapshotKind::Directory => Err(format!(
                "asset_target_unsafe: refusing to snapshot directory transaction target: {}",
                path.display()
            )),
        }
    }

    /// Managed Skill assignment/migration destinations are the sole link
    /// targets supported by the outer transaction.
    fn capture_link(path: &Path) -> Result<Self, String> {
        let snapshot = Self::capture_any(path)?;
        match &snapshot.kind {
            SnapshotKind::Missing | SnapshotKind::Symlink { .. } => Ok(snapshot),
            SnapshotKind::File { .. }
            | SnapshotKind::PrivateFile { .. }
            | SnapshotKind::PrivateDigest { .. }
            | SnapshotKind::Directory => Err(format!(
                "asset_target_unsafe: refusing non-link Skill transaction target: {}",
                path.display()
            )),
        }
    }

    fn capture_private(path: &Path) -> Result<Self, String> {
        let mut snapshot = Self::capture_any(path)?;
        snapshot.privacy = SnapshotPrivacy::Private;
        snapshot.kind = match snapshot.kind {
            SnapshotKind::Missing => SnapshotKind::Missing,
            SnapshotKind::File { bytes, mode } => SnapshotKind::PrivateFile { bytes, mode },
            SnapshotKind::Symlink { .. }
            | SnapshotKind::Directory
            | SnapshotKind::PrivateFile { .. }
            | SnapshotKind::PrivateDigest { .. } => {
                return Err(format!(
                    "asset_target_unsafe: refusing non-file private transaction target: {}",
                    path.display()
                ));
            }
        };
        Ok(snapshot)
    }

    fn capture_any(path: &Path) -> Result<Self, String> {
        let parent = capture_parent_directory(path)?;
        let (kind, identity) = match read_path_state_anchored(path, &parent)? {
            AnchoredPathState::Missing => (SnapshotKind::Missing, PathIdentity::unknown()),
            AnchoredPathState::File {
                bytes,
                mode,
                identity,
            } => (SnapshotKind::File { bytes, mode }, identity),
            AnchoredPathState::Symlink { target, identity } => {
                (SnapshotKind::Symlink { target }, identity)
            }
            AnchoredPathState::Directory { identity } => (SnapshotKind::Directory, identity),
            AnchoredPathState::Other { .. } => {
                return Err(format!("unsupported target type: {}", path.display()));
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            parent,
            kind,
            identity,
            privacy: SnapshotPrivacy::Ordinary,
        })
    }

    fn transaction_state(&self) -> Result<TransactionPathState, String> {
        match &self.kind {
            SnapshotKind::Missing if self.privacy == SnapshotPrivacy::Private => {
                Ok(TransactionPathState::PrivateMissing)
            }
            SnapshotKind::Missing => Ok(TransactionPathState::Missing),
            SnapshotKind::File { bytes, mode } if self.privacy == SnapshotPrivacy::Ordinary => {
                Ok(TransactionPathState::File {
                    bytes: bytes.to_vec(),
                    mode: *mode,
                    identity: self.identity,
                })
            }
            SnapshotKind::PrivateFile { bytes, mode }
                if self.privacy == SnapshotPrivacy::Private =>
            {
                transaction_private_file_state(bytes.as_slice(), *mode, self.identity)
            }
            SnapshotKind::PrivateDigest { content_hash, mode }
                if self.privacy == SnapshotPrivacy::Private =>
            {
                Ok(TransactionPathState::PrivateFile {
                    content_hash: content_hash.clone(),
                    mode: *mode,
                    identity: self.identity,
                })
            }
            SnapshotKind::Symlink { target } if self.privacy == SnapshotPrivacy::Ordinary => {
                Ok(TransactionPathState::Symlink {
                    target: target.clone(),
                    identity: self.identity,
                })
            }
            SnapshotKind::Directory => Err(format!(
                "asset_target_unsafe: refusing directory transaction target: {}",
                self.path.display()
            )),
            _ => Err(format!(
                "asset_target_unsafe: transaction snapshot privacy mismatch: {}",
                self.path.display()
            )),
        }
    }

    fn content_hash(&self) -> Option<String> {
        match &self.kind {
            SnapshotKind::File { bytes, .. } => Some(hex::encode(Sha256::digest(bytes))),
            SnapshotKind::PrivateFile { bytes, .. } => {
                Some(hex::encode(Sha256::digest(bytes.as_slice())))
            }
            SnapshotKind::PrivateDigest { content_hash, .. } => Some(content_hash.clone()),
            _ => None,
        }
    }

    fn target_fingerprint(&self) -> Result<String, String> {
        let leaf = match &self.kind {
            SnapshotKind::Missing => serde_json::json!({ "kind": "missing" }),
            SnapshotKind::File { mode, .. }
            | SnapshotKind::PrivateFile { mode, .. }
            | SnapshotKind::PrivateDigest { mode, .. } => serde_json::json!({
                "kind": "file",
                "content_hash": self.content_hash().expect("file snapshot has a hash"),
                "mode": mode,
                "identity": self.identity,
            }),
            SnapshotKind::Symlink { target } => serde_json::json!({
                "kind": "symlink",
                "target_hash": hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes())),
                "identity": self.identity,
            }),
            SnapshotKind::Directory => serde_json::json!({
                "kind": "directory",
                "identity": self.identity,
            }),
        };
        let parent = serde_json::to_vec(&self.parent).map_err(|error| error.to_string())?;
        let parent_hash = hex::encode(Sha256::digest(parent));
        let bytes = serde_json::to_vec(&(leaf, parent_hash)).map_err(|error| error.to_string())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }

    fn path_hash(&self) -> String {
        match &self.kind {
            SnapshotKind::Missing => "missing".into(),
            SnapshotKind::File { .. }
            | SnapshotKind::PrivateFile { .. }
            | SnapshotKind::PrivateDigest { .. } => {
                self.content_hash().expect("file snapshot has a hash")
            }
            SnapshotKind::Symlink { target } => {
                hex::encode(Sha256::digest(target.as_os_str().as_encoded_bytes()))
            }
            SnapshotKind::Directory => "directory".into(),
        }
    }

    fn from_transaction_state(
        original: &Self,
        state: &TransactionPathState,
    ) -> Result<Self, String> {
        let (kind, identity) = match (original.privacy, state) {
            (SnapshotPrivacy::Ordinary, TransactionPathState::Missing)
            | (SnapshotPrivacy::Private, TransactionPathState::PrivateMissing) => {
                (SnapshotKind::Missing, PathIdentity::unknown())
            }
            (
                SnapshotPrivacy::Ordinary,
                TransactionPathState::File {
                    bytes,
                    mode,
                    identity,
                },
            ) => (
                SnapshotKind::File {
                    bytes: Zeroizing::new(bytes.clone()),
                    mode: *mode,
                },
                *identity,
            ),
            (
                SnapshotPrivacy::Private,
                TransactionPathState::PrivateFile {
                    content_hash,
                    mode,
                    identity,
                },
            ) => (
                SnapshotKind::PrivateDigest {
                    content_hash: content_hash.clone(),
                    mode: *mode,
                },
                *identity,
            ),
            (SnapshotPrivacy::Ordinary, TransactionPathState::Symlink { target, identity }) => (
                SnapshotKind::Symlink {
                    target: target.clone(),
                },
                *identity,
            ),
            _ => {
                return Err(format!(
                    "transaction private/ordinary parity mismatch for {}",
                    original.path.display()
                ));
            }
        };
        Ok(Self {
            path: original.path.clone(),
            parent: original.parent.clone(),
            kind,
            identity,
            privacy: original.privacy,
        })
    }

    fn matches_current(&self, current: &AnchoredPathState) -> bool {
        match (&self.kind, current) {
            (SnapshotKind::Missing, AnchoredPathState::Missing) => true,
            (
                SnapshotKind::File { bytes, mode },
                AnchoredPathState::File {
                    bytes: current_bytes,
                    mode: current_mode,
                    identity,
                },
            ) => {
                self.privacy == SnapshotPrivacy::Ordinary
                    && bytes == current_bytes
                    && mode == current_mode
                    && (!self.identity.is_exact() || self.identity == *identity)
            }
            (
                SnapshotKind::PrivateFile { bytes, mode },
                AnchoredPathState::File {
                    bytes: current_bytes,
                    mode: current_mode,
                    identity,
                },
            ) => {
                self.privacy == SnapshotPrivacy::Private
                    && mode == current_mode
                    && self.identity.is_exact()
                    && self.identity == *identity
                    && hex::encode(Sha256::digest(current_bytes))
                        == hex::encode(Sha256::digest(bytes.as_slice()))
            }
            (
                SnapshotKind::PrivateDigest { content_hash, mode },
                AnchoredPathState::File {
                    bytes,
                    mode: current_mode,
                    identity,
                },
            ) => {
                self.privacy == SnapshotPrivacy::Private
                    && mode == current_mode
                    && self.identity.is_exact()
                    && self.identity == *identity
                    && hex::encode(Sha256::digest(bytes)) == *content_hash
            }
            (
                SnapshotKind::Symlink { target },
                AnchoredPathState::Symlink {
                    target: current_target,
                    identity,
                },
            ) => {
                self.privacy == SnapshotPrivacy::Ordinary
                    && target == current_target
                    && (!self.identity.is_exact() || self.identity == *identity)
            }
            (SnapshotKind::Directory, AnchoredPathState::Directory { identity }) => {
                !self.identity.is_exact() || self.identity == *identity
            }
            _ => false,
        }
    }

    fn restore_if_owned(&self, expected: Option<&Self>) -> Result<(), String> {
        if !self.validate_owned(expected)? {
            return Ok(());
        }
        let expected = expected.expect("validated changed state has write evidence");
        match (&self.kind, &expected.kind, self.privacy) {
            (SnapshotKind::Missing, SnapshotKind::Missing, _) => Ok(()),
            (
                SnapshotKind::Missing,
                SnapshotKind::File {
                    bytes,
                    mode: expected_mode,
                },
                SnapshotPrivacy::Ordinary,
            ) => {
                remove_bytes_if_unchanged_in_parent(&self.path, &self.parent, bytes, *expected_mode)
            }
            (
                SnapshotKind::Missing,
                SnapshotKind::PrivateDigest { content_hash, mode },
                SnapshotPrivacy::Private,
            ) => remove_bytes_if_hash_unchanged_in_parent(
                &self.path,
                &self.parent,
                content_hash,
                *mode,
                expected.identity,
            ),
            (
                SnapshotKind::Missing,
                SnapshotKind::Symlink { target },
                SnapshotPrivacy::Ordinary,
            ) => remove_symlink_if_unchanged_in_parent(&self.path, &self.parent, target),
            (
                SnapshotKind::File { bytes, mode },
                SnapshotKind::File {
                    bytes: expected,
                    mode: expected_mode,
                },
                SnapshotPrivacy::Ordinary,
            ) => write_bytes_if_unchanged_in_parent(
                &self.path,
                &self.parent,
                Some((expected, *expected_mode)),
                bytes,
                *mode,
            ),
            (
                SnapshotKind::PrivateFile { bytes, mode },
                SnapshotKind::PrivateDigest {
                    content_hash,
                    mode: expected_mode,
                },
                SnapshotPrivacy::Private,
            ) => write_bytes_if_hash_unchanged_in_parent(
                &self.path,
                &self.parent,
                Some((content_hash, *expected_mode, expected.identity)),
                bytes.as_slice(),
                *mode,
            ),
            (
                SnapshotKind::File { bytes, mode },
                SnapshotKind::Missing,
                SnapshotPrivacy::Ordinary,
            ) => write_bytes_if_unchanged_in_parent(&self.path, &self.parent, None, bytes, *mode),
            (
                SnapshotKind::PrivateFile { bytes, mode },
                SnapshotKind::Missing,
                SnapshotPrivacy::Private,
            ) => write_bytes_if_hash_unchanged_in_parent(
                &self.path,
                &self.parent,
                None,
                bytes.as_slice(),
                *mode,
            ),
            (
                SnapshotKind::Symlink { target },
                SnapshotKind::Missing,
                SnapshotPrivacy::Ordinary,
            ) => write_symlink_if_unchanged_in_parent(&self.path, &self.parent, None, target),
            (
                SnapshotKind::Symlink { target },
                SnapshotKind::Symlink {
                    target: expected_target,
                },
                SnapshotPrivacy::Ordinary,
            ) => write_symlink_if_unchanged_in_parent(
                &self.path,
                &self.parent,
                Some(expected_target.as_path()),
                target.as_path(),
            ),
            (_, SnapshotKind::Directory, _) | (SnapshotKind::Directory, _, _) => Err(format!(
                "refusing to roll back directory transaction target: {}",
                self.path.display()
            )),
            _ => Err(format!(
                "refusing to roll back {}: target type or privacy changed",
                self.path.display()
            )),
        }
    }

    fn validate_owned(&self, expected: Option<&Self>) -> Result<bool, String> {
        let mut current = read_path_state_anchored(&self.path, &self.parent)?;
        let result = (|| {
            if matches!(&current, AnchoredPathState::Other { .. }) {
                return Err(format!(
                    "refusing to roll back {}: unsupported target type",
                    self.path.display()
                ));
            }
            if self.matches_current(&current) {
                return Ok(false);
            }
            let expected = expected.ok_or_else(|| {
                format!(
                    "refusing to roll back {}: no transaction write evidence matches the changed target",
                    self.path.display()
                )
            })?;
            if self.path != expected.path
                || self.parent != expected.parent
                || self.privacy != expected.privacy
                || !expected.matches_current(&current)
            {
                return Err(format!(
                    "refusing to roll back {}: target changed after Arca wrote it",
                    self.path.display()
                ));
            }
            if matches!(&self.kind, SnapshotKind::Directory)
                || matches!(&expected.kind, SnapshotKind::Directory)
            {
                return Err(format!(
                    "refusing to roll back directory transaction target: {}",
                    self.path.display()
                ));
            }
            Ok(true)
        })();
        if self.privacy == SnapshotPrivacy::Private {
            if let AnchoredPathState::File { bytes, .. } = &mut current {
                bytes.zeroize();
            }
        }
        result
    }
}

fn restore_snapshots_if_unchanged(
    snapshots: &[PathSnapshot],
    written_states: &BTreeMap<PathBuf, TransactionPathState>,
) -> Vec<String> {
    let mut expected = Vec::with_capacity(snapshots.len());
    let mut conversion_errors = Vec::new();
    for snapshot in snapshots {
        match written_states
            .get(&snapshot.path)
            .map(|state| PathSnapshot::from_transaction_state(snapshot, state))
            .transpose()
        {
            Ok(expected_state) => expected.push(expected_state),
            Err(error) => {
                conversion_errors.push(error);
                expected.push(None);
            }
        }
    }
    if !conversion_errors.is_empty() {
        return conversion_errors;
    }
    let preflight_errors = snapshots
        .iter()
        .zip(&expected)
        .filter_map(|(snapshot, expected)| snapshot.validate_owned(expected.as_ref()).err())
        .collect::<Vec<_>>();
    if !preflight_errors.is_empty() {
        return preflight_errors;
    }

    let mut errors = Vec::new();
    for index in (0..snapshots.len()).rev() {
        if let Err(error) = snapshots[index].restore_if_owned(expected[index].as_ref()) {
            errors.push(error);
        }
    }
    errors
}

fn private_target_ledger_path(operation_id: &str) -> PathBuf {
    operation_root(operation_id).join("private-targets.json")
}

fn persist_private_target_ledger(
    operation_id: &str,
    snapshots: &[PathSnapshot],
) -> Result<(), String> {
    let targets = snapshots
        .iter()
        .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Private)
        .map(|snapshot| PrivateTargetLedgerEntry {
            path: snapshot.path.clone(),
            subject: private_file_snapshot_subject(operation_id, &snapshot.path),
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(());
    }
    let root = operation_root(operation_id);
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    set_private_dir(&root)?;
    let bytes = serde_json::to_vec_pretty(&PrivateTargetLedger {
        version: PRIVATE_TARGET_LEDGER_VERSION,
        targets,
    })
    .map_err(|error| error.to_string())?;
    write_private_file(&private_target_ledger_path(operation_id), &bytes)?;
    sync_directory(&root)?;
    if let Some(parent) = root.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

fn load_private_target_ledger(operation_id: &str) -> Result<Option<PrivateTargetLedger>, String> {
    let bytes = match fs::read(private_target_ledger_path(operation_id)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("recovery_required: {error}")),
    };
    let ledger: PrivateTargetLedger = serde_json::from_slice(&bytes)
        .map_err(|_| "recovery_required: invalid private target ledger".to_string())?;
    if ledger.version != PRIVATE_TARGET_LEDGER_VERSION {
        return Err("recovery_required: unsupported private target ledger".into());
    }
    let mut paths = BTreeSet::new();
    for target in &ledger.targets {
        if !paths.insert(target.path.clone())
            || target.subject != private_file_snapshot_subject(operation_id, &target.path)
        {
            return Err("recovery_required: invalid private target ledger binding".into());
        }
    }
    Ok(Some(ledger))
}

fn clear_private_target_ledger_items(
    operation_id: &str,
    ledger: &PrivateTargetLedger,
) -> Result<(), String> {
    cleanup_persisted_private_paths(
        operation_id,
        &ledger
            .targets
            .iter()
            .map(|target| target.path.clone())
            .collect::<Vec<_>>(),
    )
}

fn remove_private_target_ledger(operation_id: &str) -> Result<(), String> {
    match fs::remove_file(private_target_ledger_path(operation_id)) {
        Ok(()) => sync_directory(&operation_root(operation_id)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn persist_rollback_snapshots(
    operation_id: &str,
    snapshots: &[PathSnapshot],
) -> Result<(), String> {
    persist_private_target_ledger(operation_id, snapshots)?;
    let ledger_paths = load_private_target_ledger(operation_id)?
        .map(|ledger| {
            ledger
                .targets
                .into_iter()
                .map(|target| target.path)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let root = operation_root(operation_id).join("rollback");
    let persisted = (|| {
        match fs::remove_dir_all(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        set_private_dir(&root)?;
        let mut durable = Vec::with_capacity(snapshots.len());
        for (index, snapshot) in snapshots.iter().enumerate() {
            let kind = match (&snapshot.kind, snapshot.privacy) {
                (SnapshotKind::Missing, _) => DurableSnapshotKind::Missing,
                (SnapshotKind::Directory, SnapshotPrivacy::Ordinary) => {
                    DurableSnapshotKind::Directory
                }
                (SnapshotKind::Symlink { target }, SnapshotPrivacy::Ordinary) => {
                    DurableSnapshotKind::Symlink {
                        target: target.clone(),
                    }
                }
                (SnapshotKind::File { bytes, mode }, SnapshotPrivacy::Ordinary) => {
                    let backup = format!("{index}.bin");
                    write_private_file(&root.join(&backup), bytes)?;
                    DurableSnapshotKind::File {
                        backup,
                        mode: *mode,
                    }
                }
                (SnapshotKind::PrivateFile { bytes, mode }, SnapshotPrivacy::Private) => {
                    let content_hash = hex::encode(Sha256::digest(bytes.as_slice()));
                    let subject = persist_private_file_snapshot(
                        operation_id,
                        &snapshot.path,
                        bytes.as_slice(),
                    )?;
                    run_after_private_snapshot_persist_hook()?;
                    DurableSnapshotKind::PrivateFile {
                        content_hash,
                        mode: *mode,
                        subject,
                    }
                }
                (SnapshotKind::PrivateDigest { .. }, SnapshotPrivacy::Private) => {
                    return Err("cannot persist a post-state digest as rollback pre-state".into());
                }
                _ => {
                    return Err(format!(
                        "transaction snapshot privacy mismatch for {}",
                        snapshot.path.display()
                    ));
                }
            };
            durable.push(DurableSnapshot {
                path: snapshot.path.clone(),
                parent: snapshot.parent.clone(),
                identity: snapshot.identity,
                privacy: snapshot.privacy,
                kind,
            });
        }
        let manifest = serde_json::to_vec_pretty(&DurableSnapshotManifest {
            version: ROLLBACK_MANIFEST_VERSION,
            snapshots: durable,
        })
        .map_err(|error| error.to_string())?;
        // Manifest is written last: its presence proves every referenced
        // filesystem backup and Keychain item is durable before target mutation.
        write_private_file(&root.join("manifest.json"), &manifest)?;
        sync_directory(&root)?;
        if let Some(parent) = root.parent() {
            sync_directory(parent)?;
        }
        Ok::<_, String>(())
    })();
    if let Err(error) = persisted {
        return match cleanup_persisted_private_paths(operation_id, &ledger_paths) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&root);
                let _ = remove_private_target_ledger(operation_id);
                Err(error)
            }
            Err(cleanup) => Err(format!(
                "recovery_required: rollback snapshot persistence failed ({error}); private Keychain cleanup failed: {cleanup}"
            )),
        };
    }
    Ok(())
}

fn cleanup_persisted_private_paths(operation_id: &str, paths: &[PathBuf]) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in paths {
        if let Err(error) = clear_private_file_snapshot(operation_id, path) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn load_rollback_manifest(operation_id: &str) -> Result<Option<DurableSnapshotManifest>, String> {
    let root = operation_root(operation_id).join("rollback");
    let manifest = match fs::read(root.join("manifest.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("recovery_required: {error}")),
    };
    let manifest: DurableSnapshotManifest = serde_json::from_slice(&manifest)
        .map_err(|_| "recovery_required: invalid asset rollback manifest".to_string())?;
    if !matches!(manifest.version, 2 | ROLLBACK_MANIFEST_VERSION) {
        return Err("recovery_required: unsupported asset rollback manifest".into());
    }
    Ok(Some(manifest))
}

fn load_rollback_snapshots(operation_id: &str) -> Result<Option<Vec<PathSnapshot>>, String> {
    let root = operation_root(operation_id).join("rollback");
    let Some(manifest) = load_rollback_manifest(operation_id)? else {
        return Ok(None);
    };
    let mut snapshots = Vec::with_capacity(manifest.snapshots.len());
    for snapshot in manifest.snapshots {
        #[cfg(unix)]
        if !matches!(&snapshot.kind, DurableSnapshotKind::Missing) && !snapshot.identity.is_exact()
        {
            return Err("recovery_required: rollback snapshot has no exact target identity".into());
        }
        let kind = match (snapshot.privacy, snapshot.kind) {
            (_, DurableSnapshotKind::Missing) => SnapshotKind::Missing,
            (SnapshotPrivacy::Ordinary, DurableSnapshotKind::Directory) => SnapshotKind::Directory,
            (SnapshotPrivacy::Ordinary, DurableSnapshotKind::Symlink { target }) => {
                SnapshotKind::Symlink { target }
            }
            (SnapshotPrivacy::Ordinary, DurableSnapshotKind::File { backup, mode }) => {
                SnapshotKind::File {
                    bytes: Zeroizing::new(
                        fs::read(root.join(backup))
                            .map_err(|error| format!("recovery_required: {error}"))?,
                    ),
                    mode,
                }
            }
            (
                SnapshotPrivacy::Private,
                DurableSnapshotKind::PrivateFile {
                    content_hash,
                    mode,
                    subject,
                },
            ) => {
                if subject != private_file_snapshot_subject(operation_id, &snapshot.path) {
                    return Err(
                        "recovery_required: private rollback subject binding is invalid".into(),
                    );
                }
                let bytes = load_private_file_snapshot(operation_id, &snapshot.path, &content_hash)
                    .map_err(|error| format!("recovery_required: {error}"))?
                    .ok_or_else(|| {
                        "recovery_required: private rollback Keychain item is missing".to_string()
                    })?;
                SnapshotKind::PrivateFile { bytes, mode }
            }
            _ => {
                return Err(format!(
                    "recovery_required: rollback snapshot privacy mismatch: {}",
                    snapshot.path.display()
                ));
            }
        };
        snapshots.push(PathSnapshot {
            path: snapshot.path,
            parent: snapshot.parent,
            kind,
            identity: snapshot.identity,
            privacy: snapshot.privacy,
        });
    }
    Ok(Some(snapshots))
}

fn clear_private_snapshot_manifest_items(
    operation_id: &str,
    manifest: &DurableSnapshotManifest,
) -> Result<(), String> {
    let paths = manifest
        .snapshots
        .iter()
        .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Private)
        .map(|snapshot| snapshot.path.clone())
        .collect::<Vec<_>>();
    cleanup_persisted_private_paths(operation_id, &paths)
}

fn clear_private_snapshot_items(
    operation_id: &str,
    snapshots: &[PathSnapshot],
) -> Result<(), String> {
    let paths = snapshots
        .iter()
        .filter(|snapshot| snapshot.privacy == SnapshotPrivacy::Private)
        .map(|snapshot| snapshot.path.clone())
        .collect::<Vec<_>>();
    cleanup_persisted_private_paths(operation_id, &paths)
}

fn finalize_completed_operation_cleanup(
    operation_id: &str,
    operation_path: &Path,
    manifest: &DurableSnapshotManifest,
    profile_ids: &[String],
) -> Result<(), String> {
    for profile_id in profile_ids {
        clear_credential_rollback(operation_id, profile_id)?;
    }
    clear_private_snapshot_manifest_items(operation_id, manifest)?;
    fs::remove_dir_all(operation_path).map_err(|error| error.to_string())?;
    clear_pending_payload(operation_id);
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("failed to sync directory {}: {error}", path.display()))
}

fn set_private_dir(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{
        plan_set_active_model, plan_set_agent_consumption, plan_set_all_mcp_enabled,
        plan_set_mcp_enabled, plan_set_model_enabled, plan_update_central_asset,
        AgentConsumptionSelection, CentralAssetAction, CentralAssetDraft,
        PlanSetActiveModelRequest, PlanSetAgentConsumptionRequest, PlanSetAllMcpEnabledRequest,
        PlanSetMcpEnabledRequest, PlanSetModelEnabledRequest, PlanUpdateCentralAssetRequest,
    };
    use crate::domain::assets::ConsumptionStatus;
    use crate::domain::types::{
        ModelProfile, ModelProtocol, ModelProviderConfig, ModelProviderProtocolConfig,
        RegistryConfig, RegistryEntry, SourceDef, StdioConfig,
    };
    use crate::resources::mcp::registry::{read_registry_all, write_manual_entry};
    use crate::resources::mcp::sources::cached_path;
    use crate::resources::model::save_profile;
    use crate::testenv::TestHome;
    use serde_json::Value;

    #[test]
    fn credential_read_failure_cannot_create_an_absent_rollback_snapshot() {
        let _home = TestHome::new("credential-read-fail-closed");
        save_profile(model("before"), Some("fixture-original-key".into())).unwrap();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()),
                profile: Box::new(model("after")),
                credential: None,
            },
        }).unwrap();
        let before = fs::read(model_catalog_file()).unwrap();
        let failure = crate::resources::model::set_test_credential_read_error();
        let error = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id.clone(), candidate_hash: plan.candidate_hash,
        }).unwrap_err();
        assert!(error.starts_with("credential_read_failed:"));
        assert_eq!(fs::read(model_catalog_file()).unwrap(), before);
        assert!(load_rollback_manifest(&plan.operation_id).unwrap().is_none());
        drop(failure);
        assert_eq!(credential_snapshot("work").unwrap().unwrap(), b"fixture-original-key");
        assert!(credential_rollback_snapshot(&plan.operation_id, "work").unwrap().is_none());
        cancel_asset_operation(&plan.operation_id).unwrap();
    }

    #[test]
    fn recovery_preserves_a_live_review_and_cleans_it_after_owner_exit() {
        let _home = TestHome::new("asset-plan-owner-recovery");
        save_profile(model("before"), None).unwrap();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()), profile: Box::new(model("after")), credential: None,
            },
        }).unwrap();
        assert!(recover_pending_asset_operations().unwrap().is_empty());
        assert!(load_operation(&plan.operation_id).is_ok());
        super::super::lease::release(&plan.operation_id);
        assert_eq!(recover_pending_asset_operations().unwrap(), vec![plan.operation_id.clone()]);
        assert!(!operation_root(&plan.operation_id).exists());
    }

    #[test]
    fn skill_recovery_isolates_foreign_edits_and_never_marks_completed_peers() {
        let home = TestHome::new("skill-recovery-independent");
        let completed = home.home.join("complete/skill");
        let healthy = home.home.join("recoverable/skill");
        let foreign = home.home.join("foreign/skill");
        for path in [&completed, &healthy, &foreign] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "before").unwrap();
        }
        mutate_settings(|settings| {
            let overrides = settings.agent_config_paths.get_or_insert_default();
            for (agent, path) in [("claude-code", &completed), ("cline", &healthy), ("codex", &foreign)] {
                overrides.insert(agent.into(), crate::settings::AgentConfigPathOverride {
                    skills_global_dir: Some(path.parent().unwrap().to_string_lossy().into_owned()),
                    skills_alias_dirs: Some(Vec::new()), ..Default::default()
                });
            }
        }).unwrap();
        let paths = vec![settings_file(), completed.clone(), healthy.clone(), foreign.clone()];
        let snapshots = paths.iter().map(|path| PathSnapshot::capture(path).unwrap()).collect::<Vec<_>>();
        let mut plan = private_transaction_plan(paths[1..].iter().map(|path| path.to_string_lossy().into_owned()).collect());
        plan.domain_plan = DomainPlan::Skill { before: BTreeMap::new(), after: BTreeMap::new() };
        plan.affected_agent_ids = vec!["claude-code".into(), "cline".into(), "codex".into()];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracker = begin_transaction_write_tracking(&transaction_write_evidence_dir(&plan.operation_id), &paths,
            &parent_snapshots_for_snapshots(&snapshots)).unwrap();
        mark_central_committed(&plan).unwrap();
        crate::safe_write::write_if_unchanged(&completed, Some("before"), "complete").unwrap();
        mark_physical_target_completed(&plan, completed.parent().unwrap(), &[completed.to_string_lossy().into_owned()]).unwrap();
        for path in [&healthy, &foreign] {
            crate::safe_write::write_if_unchanged(path, Some("before"), "partial").unwrap();
        }
        fs::write(&foreign, "external-edit").unwrap();
        assert!(settle_checkpoint_targets(&plan, snapshots, &tracker.states()).is_err());
        assert_eq!(fs::read(&completed).unwrap(), b"complete");
        assert_eq!(fs::read(&healthy).unwrap(), b"before");
        assert_eq!(fs::read(&foreign).unwrap(), b"external-edit");
        // The outer startup error path receives the original complete plan.
        record_recovery_incidents(&plan).unwrap();
        let incidents = load_settings_strict().unwrap().target_incidents.unwrap();
        assert!(!incidents.is_empty());
        assert!(incidents.values().all(|incident| {
            incident.target_path != completed.to_string_lossy()
                && !incident.affected_agent_ids.contains(&"claude-code".to_string())
        }));
    }

    #[test]
    fn skill_checkpoints_preserve_a_complete_physical_target_and_restore_its_peer() {
        let home = TestHome::new("skill-physical-checkpoints");
        mutate_settings(|_| ()).unwrap();
        let first = home.home.join("agent-a/one");
        let sibling = home.home.join("agent-a/two");
        let other = home.home.join("agent-b/one");
        for path in [&first, &sibling, &other] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"before").unwrap();
        }
        let paths = vec![settings_file(), first.clone(), sibling.clone(), other.clone()];
        let snapshots = paths.iter().map(|path| PathSnapshot::capture(path).unwrap()).collect::<Vec<_>>();
        let mut plan = private_transaction_plan(paths[1..].iter().map(|path| path.to_string_lossy().into_owned()).collect());
        plan.domain_plan = DomainPlan::Skill { before: BTreeMap::new(), after: BTreeMap::new() };
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracker = begin_transaction_write_tracking(&transaction_write_evidence_dir(&plan.operation_id), &paths,
            &parent_snapshots_for_snapshots(&snapshots)).unwrap();
        mutate_settings(|settings| settings.imported = Some("durable-intent".into())).unwrap();
        mark_central_committed(&plan).unwrap();
        for path in [&first, &sibling] {
            crate::safe_write::write_if_unchanged(path, Some("before"), "complete").unwrap();
        }
        mark_physical_target_completed(&plan, first.parent().unwrap(), &[
            first.to_string_lossy().into_owned(), sibling.to_string_lossy().into_owned(),
        ]).unwrap();
        crate::safe_write::write_if_unchanged(&other, Some("before"), "partial").unwrap();
        settle_checkpoint_targets(&plan, snapshots, &tracker.states()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"complete");
        assert_eq!(fs::read(&sibling).unwrap(), b"complete");
        assert_eq!(fs::read(&other).unwrap(), b"before");
        assert_eq!(load_settings_strict().unwrap().imported.as_deref(), Some("durable-intent"));
    }

    #[test]
    fn qoder_cli_model_path_override_uses_private_snapshots() {
        let home = TestHome::new("qoder-cli-private-override");
        let target = home.home.join("custom-qoder/settings.json");
        let mut settings = Settings::default();
        settings.agent_config_paths = Some(BTreeMap::from([("qoder-cli".into(), crate::settings::AgentConfigPathOverride {
            model_paths: Some(vec![target.to_string_lossy().into_owned()]),
            ..Default::default()
        })]));
        let plan = private_transaction_plan(vec![target.to_string_lossy().into_owned()]);
        assert!(private_transaction_paths(&plan, &settings).unwrap().contains(&target));
    }

    #[test]
    fn kilo_desktop_model_and_mcp_overrides_always_use_private_snapshots() {
        let home = TestHome::new("kilo-desktop-private-targets");
        let default = crate::resources::mcp::scanner::expand_tilde(
            &crate::resources::model::default_config_paths("kilo-desktop").unwrap()[0]);
        let model = home.home.join("kilo-model/kilo.jsonc");
        let mcp = home.home.join("kilo-mcp/kilo.jsonc");
        let unrelated = home.home.join("other/config.json");
        let mut settings = Settings::default();
        settings.agent_config_paths = Some(BTreeMap::from([("kilo-desktop".into(), crate::settings::AgentConfigPathOverride {
            model_paths: Some(vec![model.to_string_lossy().into_owned()]), ..Default::default()
        })]));
        let mut definition = crate::agents::builtin_agents()["kilo-desktop"].clone();
        definition.global = Some(mcp.to_string_lossy().into_owned());
        settings.agents = Some(BTreeMap::from([("kilo-desktop".into(), definition)]));
        let plan = private_transaction_plan([&default, &model, &mcp, &unrelated].iter()
            .map(|path| path.to_string_lossy().into_owned()).collect());
        let private = private_transaction_paths(&plan, &settings).unwrap();
        for path in [&default, &model, &mcp] { assert!(private.contains(path)); }
        assert!(!private.contains(&unrelated));
    }

    #[test]
    fn qoder_private_journal_recovers_before_commit_and_after_commit() {
        let home = TestHome::new("qoder-private-recovery");
        let target = home.home.join(".qoder/settings.json");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"{\"providers\":{}}").unwrap();
        for agent in ["qoder-cli", "qoder-desktop"] {
            for committed in [false, true] {
                let mut plan = private_transaction_plan(vec![target.to_string_lossy().into_owned()]);
                plan.affected_agent_ids = vec![agent.into()];
                let persisted = PersistedAssetOperation {
                    schema_version: 3, plan,
                    settings_hash: None, state_preconditions: vec![],
                    settings_target_hash: String::new(), target_hashes: BTreeMap::new(),
                    mcp_catalog_hash: None, skill_target_graph_hash: None, lifecycle: None,
                };
                let operation_id = &persisted.plan.operation_id;
                persist_rollback_snapshots(operation_id, &[PathSnapshot::capture_private(&target).unwrap()]).unwrap();
                if committed { mark_operation_committed(operation_id).unwrap(); }
                recover_pending_asset_operation(&persisted, &operation_root(operation_id)).unwrap();
                assert!(!operation_root(operation_id).exists());
                assert_eq!(fs::read(&target).unwrap(), b"{\"providers\":{}}");
            }
        }
    }

    #[test]
    fn credential_delivery_preserves_current_and_rolls_back_all_profile_writes() {
        use crate::domain::assets::ApiKeyDelivery;
        use crate::domain::types::{ApiKeySource, AuthRequirement};
        let home = TestHome::new("credential-policy-atomic");
        let mut key_paths = Vec::new();
        for id in ["a", "b"] {
            let key_path = home.home.join(format!("key-{id}"));
            fs::write(&key_path, format!("fixture-key-{id}")).unwrap();
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let mut profile = model(id);
            profile.id = id.into();
            profile.provider_id = Some(format!("provider-{id}"));
            save_profile(profile.clone(), None).unwrap();
            mutate_settings(|settings| {
                let provider = settings.model_providers.as_mut().unwrap()
                    .get_mut(profile.provider_id.as_ref().unwrap()).unwrap();
                provider.api_key_source = Some(ApiKeySource::File { path: key_path.to_string_lossy().into_owned() });
                provider.auth_requirement = AuthRequirement::Required;
            }).unwrap();
            key_paths.push(key_path);
        }
        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Auto;
            settings.set_model_selection("opencode", selection);
        }).unwrap();
        let install = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "opencode".into(),
            selection: AgentConsumptionSelection::Model { profile_ids: vec!["a".into(), "b".into()] },
        }).unwrap();
        commit_asset_operation(AssetCommitRequest { operation_id: install.operation_id, candidate_hash: install.candidate_hash }).unwrap();
        let active = plan_set_active_model(PlanSetActiveModelRequest { agent_id: "opencode".into(), profile_id: "a".into() }).unwrap();
        commit_asset_operation(AssetCommitRequest { operation_id: active.operation_id, candidate_hash: active.candidate_hash }).unwrap();
        let before = load_settings_strict().unwrap().model_selection("opencode");
        let plan = crate::assets::planner::plan_model_credential_delivery("opencode", None, ApiKeyDelivery::Plaintext, true).unwrap();
        let files = plan.target_files.iter().map(|path| {
            let path = expand_tilde_path(path);
            let bytes = fs::read(&path).ok();
            (path, bytes)
        }).collect::<Vec<_>>();
        let error = commit_asset_operation_with_hook(AssetCommitRequest {
            operation_id: plan.operation_id, candidate_hash: plan.candidate_hash,
        }, || { fs::remove_file(&key_paths[1]).map_err(|error| error.to_string()) }).unwrap_err();
        assert!(!error.is_empty());
        assert_eq!(load_settings_strict().unwrap().model_selection("opencode"), before);
        for (path, bytes) in &files { assert_eq!(fs::read(path).ok(), *bytes); }
        fs::write(&key_paths[1], b"fixture-key-b").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_paths[1], fs::Permissions::from_mode(0o600)).unwrap();
        }
        crate::resources::model::set_agent_credential_delivery("opencode", ApiKeyDelivery::Plaintext, true).unwrap();
        let after = load_settings_strict().unwrap().model_selection("opencode");
        assert_eq!(after.active_profile_id, before.active_profile_id);
        assert!(after.profiles.values().all(|record| record.enabled));
    }

    #[test]
    fn checkpoint_recovery_preserves_central_and_completed_target_only() {
        let home = TestHome::new("target-checkpoint-recovery");
        mutate_settings(|_| ()).unwrap();
        let first = home.home.join("a/first.json");
        let second = home.home.join("b/second.json");
        fs::create_dir_all(first.parent().unwrap()).unwrap();
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::write(&first, b"first-before").unwrap();
        fs::write(&second, b"second-before").unwrap();
        let paths = vec![settings_file(), first.clone(), second.clone()];
        let snapshots = paths.iter().map(|path| PathSnapshot::capture(path).unwrap()).collect::<Vec<_>>();
        let mut plan = private_transaction_plan(vec![first.to_string_lossy().into_owned(), second.to_string_lossy().into_owned()]);
        plan.affected_agent_ids = vec!["qoder-desktop".into(), "grok-build".into()];
        mutate_settings(|settings| {
            let paths = settings.agent_config_paths.get_or_insert_default();
            for (id, path) in [("qoder-desktop", &first), ("grok-build", &second)] {
                paths.insert(id.into(), crate::settings::AgentConfigPathOverride {
                    model_paths: Some(vec![path.to_string_lossy().into_owned()]), ..Default::default()
                });
            }
        }).unwrap();
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracker = begin_transaction_write_tracking(&transaction_write_evidence_dir(&plan.operation_id), &paths,
            &parent_snapshots_for_snapshots(&snapshots)).unwrap();
        mutate_settings(|settings| settings.model_assignments = Some(BTreeMap::from([("codex".into(), "durable".into())]))).unwrap();
        mark_central_committed(&plan).unwrap();
        crate::safe_write::write_if_unchanged(&first, Some("first-before"), "first-after").unwrap();
        mark_target_completed(&plan, "qoder-desktop").unwrap();
        crate::safe_write::write_if_unchanged(&second, Some("second-before"), "second-partial").unwrap();
        settle_checkpoint_targets(&plan, snapshots, &tracker.states()).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"first-after");
        assert_eq!(fs::read(&second).unwrap(), b"second-before");
        let settings = load_settings_strict().unwrap();
        assert_eq!(settings.model_assignments.unwrap()["codex"], "durable");
        assert!(settings.target_incidents.unwrap().values().all(|incident| incident.target_path == second.to_string_lossy()));
        drop(tracker);
    }

    #[test]
    fn clear_agent_models_removes_native_pi_registry_even_without_arca_relationships() {
        let home = TestHome::new("clear-agent-models-native-pi");
        let models = home.home.join(".pi/agent/models.json");
        let settings = home.home.join(".pi/agent/settings.json");
        fs::create_dir_all(models.parent().unwrap()).unwrap();
        fs::write(
            &models,
            r#"{"providers":{"manual":{"models":[{"id":"external"}]}}}"#,
        )
        .unwrap();
        fs::write(
            &settings,
            r#"{"theme":"dark","defaultProvider":"manual","defaultModel":"external"}"#,
        )
        .unwrap();

        let plan = crate::assets::plan_clear_agent_models(
            crate::domain::assets::PlanClearAgentModelsRequest {
                agent_id: "pi".into(),
            },
        )
        .unwrap();
        assert_eq!(plan.kind, AssetOperationKind::ClearModels);
        assert!(plan.can_commit, "{:?}", plan.warnings);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("external")));

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        let models: Value = serde_json::from_str(&fs::read_to_string(models).unwrap()).unwrap();
        let settings: Value = serde_json::from_str(&fs::read_to_string(settings).unwrap()).unwrap();
        assert_eq!(models["providers"], serde_json::json!({}));
        assert!(settings.get("defaultProvider").is_none());
        assert!(settings.get("defaultModel").is_none());
        assert_eq!(settings["theme"], "dark");
    }

    fn model(model: &str) -> ModelProfile {
        ModelProfile {
            id: "work".into(),
            provider_id: Some("custom-provider".into()),
            name: "Work".into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: "https://example.invalid/v1".into(),
            endpoint_path: String::new(),
            model: model.into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(false),
        }
    }

    fn shared_provider_models() -> (ModelProviderConfig, ModelProfile, ModelProfile) {
        let provider = ModelProviderConfig {
            id: "shared-provider".into(),
            name: "Shared Provider".into(),
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
        let first = ModelProfile {
            id: "shared-openai".into(),
            provider_id: Some(provider.id.clone()),
            name: "Shared OpenAI".into(),
            provider: provider.provider.clone(),
            model_vendor: Some("openai".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: provider.base_url.clone(),
            endpoint_path: provider.protocols[&ModelProtocol::OpenaiResponses]
                .endpoint_path
                .clone(),
            model: "gpt-shared".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(true),
        };
        let second = ModelProfile {
            id: "shared-anthropic".into(),
            provider_id: Some(provider.id.clone()),
            name: "Shared Anthropic".into(),
            model_vendor: Some("anthropic".into()),
            protocol: ModelProtocol::AnthropicMessages,
            endpoint_path: provider.protocols[&ModelProtocol::AnthropicMessages]
                .endpoint_path
                .clone(),
            model: "claude-shared".into(),
            ..first.clone()
        };
        (provider, first, second)
    }

    fn install_shared_provider_models() -> (ModelProviderConfig, ModelProfile, ModelProfile) {
        let (provider, first, second) = shared_provider_models();
        save_profile(first.clone(), Some("old-secret".into())).unwrap();
        save_profile(second.clone(), Some("old-secret".into())).unwrap();
        (provider, first, second)
    }

    fn parent_snapshots_for_paths(paths: &[PathBuf]) -> BTreeMap<PathBuf, ParentDirectorySnapshot> {
        paths
            .iter()
            .map(|path| (path.clone(), capture_parent_directory(path).unwrap()))
            .collect()
    }

    fn parent_snapshots_for_snapshots(
        snapshots: &[PathSnapshot],
    ) -> BTreeMap<PathBuf, ParentDirectorySnapshot> {
        snapshots
            .iter()
            .map(|snapshot| (snapshot.path.clone(), snapshot.parent.clone()))
            .collect()
    }

    fn evidence_contains(root: &Path, needle: &[u8]) -> bool {
        let Ok(entries) = fs::read_dir(root) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                evidence_contains(&path, needle)
            } else {
                fs::read(path)
                    .ok()
                    .is_some_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
            }
        })
    }

    fn private_transaction_plan(target_files: Vec<String>) -> AssetOperationPlan {
        AssetOperationPlan {
            operation_id: "33333333-3333-4333-8333-333333333333".into(),
            kind: AssetOperationKind::SetConsumption,
            domain_plan: DomainPlan::Model {
                before: BTreeMap::new(),
                after: BTreeMap::new(),
            },
            central_changes: Vec::new(),
            relationship_changes: Vec::new(),
            model_state_changes: Vec::new(),
            consumption_state_changes: Vec::new(),
            target_files,
            affected_agent_ids: vec!["claude-desktop".into()],
            warnings: Vec::new(),
            can_commit: true,
            candidate_hash: "candidate".into(),
        }
    }

    fn save_private_claude_profile(id: &str, credential: &str) -> ModelProfile {
        let profile = ModelProfile {
            id: id.into(),
            provider_id: Some(format!("{id}-provider")),
            name: format!("{id} model"),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: "https://gateway.example.test".into(),
            endpoint_path: "/anthropic/v1/messages".into(),
            model: "claude-sonnet-4-6".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
        };
        save_profile(profile.clone(), Some(credential.into())).unwrap();
        profile
    }

    fn plan_private_claude_consumption(profile_id: &str) -> AssetOperationPlan {
        plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-desktop".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![profile_id.into()],
            },
        })
        .unwrap()
    }

    #[test]
    fn private_transaction_classifier_selects_only_reviewed_claude_desktop_profile() {
        let _home = TestHome::new("private-transaction-classifier");
        let fixed = crate::resources::model::default_config_paths("claude-desktop")
            .unwrap()
            .into_iter()
            .nth(3)
            .unwrap();
        let decoy = format!("~/other/{}", fixed.rsplit('/').next().unwrap());
        let plan = private_transaction_plan(vec![fixed.clone(), decoy.clone()]);

        let private = private_transaction_paths(&plan, &load_settings_strict().unwrap()).unwrap();

        assert_eq!(
            private,
            BTreeSet::from([crate::resources::mcp::scanner::expand_tilde(&fixed)])
        );
        assert!(!private.contains(&crate::resources::mcp::scanner::expand_tilde(&decoy)));
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_hardlinked_ordinary_target_fails_before_evidence_or_write() {
        use std::os::unix::fs::MetadataExt;

        let home = TestHome::new("private-transaction-hardlink-alias");
        let profile = save_private_claude_profile(
            "claude-private-hardlink-alias",
            "PRIVATE-ALIAS-NEW-CREDENTIAL",
        );
        let configured = crate::resources::model::default_config_paths("claude-desktop").unwrap();
        let ordinary = crate::resources::mcp::scanner::expand_tilde(&configured[0]);
        let private = crate::resources::mcp::scanner::expand_tilde(&configured[3]);
        let sentinel = b"{\"inferenceGatewayApiKey\":\"PRIVATE-ALIAS-PRESTATE-SENTINEL\"}";
        fs::create_dir_all(private.parent().unwrap()).unwrap();
        fs::write(&private, sentinel).unwrap();
        fs::create_dir_all(ordinary.parent().unwrap()).unwrap();
        fs::hard_link(&private, &ordinary).unwrap();
        let before = fs::metadata(&private).unwrap();
        assert_eq!(before.dev(), fs::metadata(&ordinary).unwrap().dev());
        assert_eq!(before.ino(), fs::metadata(&ordinary).unwrap().ino());
        let plan = plan_private_claude_consumption(&profile.id);
        let operation_id = plan.operation_id.clone();

        let error = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap_err();

        assert!(
            error.contains("claude_desktop_private_target_alias"),
            "{error}"
        );
        assert_eq!(fs::read(&ordinary).unwrap(), sentinel);
        assert_eq!(fs::read(&private).unwrap(), sentinel);
        assert!(!operation_root(&operation_id)
            .join("rollback/manifest.json")
            .exists());
        assert!(!private_target_ledger_path(&operation_id).exists());
        assert!(!operation_commit_marker(&operation_id).exists());
        assert!(credential_rollback_snapshot(&operation_id, &profile.id)
            .unwrap()
            .is_none());
        assert!(!private_file_snapshot_exists(&operation_id, &private));
        assert!(!evidence_contains(
            &home.home.join(".arca"),
            b"PRIVATE-ALIAS-PRESTATE-SENTINEL"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_adapter_rollback_failure_retains_recovery_evidence() {
        let _home = TestHome::new("private-transaction-rollback-failure");
        let profile = save_private_claude_profile(
            "claude-private-rollback-failure",
            "PRIVATE-ROLLBACK-NEW-CREDENTIAL",
        );
        let configured = crate::resources::model::default_config_paths("claude-desktop").unwrap();
        let paths = configured
            .iter()
            .map(|path| crate::resources::mcp::scanner::expand_tilde(path))
            .collect::<Vec<_>>();
        crate::resources::model::apply_profile("claude-desktop", &profile.id).unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        crate::domain::assets::ModelConsumptionRecord {
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
        fs::write(&paths[0], r#"{"deploymentMode":"native"}"#).unwrap();
        let meta: Value = serde_json::from_str(&fs::read_to_string(&paths[2]).unwrap()).unwrap();
        fs::write(&paths[2], serde_json::to_vec(&meta).unwrap()).unwrap();
        let plan =
            crate::assets::plan_reapply_model(crate::domain::assets::PlanReapplyModelRequest {
                agent_id: "claude-desktop".into(),
                profile_id: profile.id.clone(),
            })
            .unwrap();
        assert!(plan.can_commit, "{:?}", plan.warnings);
        let operation_id = plan.operation_id.clone();
        let meta_path = paths[2].clone();
        let private_path = paths[3].clone();
        let _hook =
            crate::resources::model::set_claude_desktop_config_write_hook(move |path, rollback| {
                if !rollback && path == meta_path {
                    return Err("injected forward write failure".into());
                }
                if rollback && path == private_path {
                    return Err("injected private rollback failure".into());
                }
                Ok(())
            });

        let error = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap_err();

        assert!(error.starts_with("target_recovery_required:"), "{error}");
        assert!(
            error.contains("injected private rollback failure"),
            "{error}"
        );
        assert!(operation_root(&operation_id)
            .join("rollback/manifest.json")
            .is_file());
        assert!(private_target_ledger_path(&operation_id).is_file());
        assert!(private_file_snapshot_exists(&operation_id, &paths[3]));
        assert!(!operation_commit_marker(&operation_id).exists());
        assert!(!operation_state_marker_exists(&operation_id, ROLLBACK_COMPLETE_MARKER).unwrap());
        drop(_hook);
        clear_private_file_snapshot(&operation_id, &paths[3]).unwrap();
        fs::remove_dir_all(operation_root(&operation_id)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_transaction_clear_rollback_failure_retains_recovery_evidence() {
        let _home = TestHome::new("private-transaction-clear-rollback");
        let profile = save_private_claude_profile(
            "claude-private-clear-rollback",
            "PRIVATE-CLEAR-ROLLBACK-CREDENTIAL",
        );
        let configured = crate::resources::model::default_config_paths("claude-desktop").unwrap();
        let paths = configured
            .iter()
            .map(|path| crate::resources::mcp::scanner::expand_tilde(path))
            .collect::<Vec<_>>();
        fs::create_dir_all(paths[2].parent().unwrap()).unwrap();
        fs::write(
            &paths[2],
            r#"{"appliedId":"default-id","entries":[{"id":"default-id","name":"Default"}]}"#,
        )
        .unwrap();
        crate::resources::model::apply_profile("claude-desktop", &profile.id).unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "claude-desktop",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        crate::domain::assets::ModelConsumptionRecord {
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
        let plan = crate::assets::plan_clear_agent_models(
            crate::domain::assets::PlanClearAgentModelsRequest {
                agent_id: "claude-desktop".into(),
            },
        )
        .unwrap();
        assert!(plan.can_commit, "{:?}", plan.warnings);
        let operation_id = plan.operation_id.clone();
        let meta_path = paths[2].clone();
        let private_path = paths[3].clone();
        let _hook =
            crate::resources::model::set_claude_desktop_config_write_hook(move |path, rollback| {
                if !rollback && path == private_path {
                    return Err("injected private clear failure".into());
                }
                if rollback && path == meta_path {
                    return Err("injected metadata rollback failure".into());
                }
                Ok(())
            });

        let error = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap_err();

        assert!(error.starts_with("target_recovery_required:"), "{error}");
        assert!(
            error.contains("injected metadata rollback failure"),
            "{error}"
        );
        assert!(operation_root(&operation_id)
            .join("rollback/manifest.json")
            .is_file());
        assert!(private_target_ledger_path(&operation_id).is_file());
        assert!(private_file_snapshot_exists(&operation_id, &paths[3]));
        assert!(!operation_commit_marker(&operation_id).exists());
        assert!(operation_root(&operation_id)
            .join(TARGET_INCIDENT_MARKER)
            .is_file());
        drop(_hook);
        clear_private_file_snapshot(&operation_id, &paths[3]).unwrap();
        fs::remove_dir_all(operation_root(&operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_snapshot_is_hash_only_and_restores_from_keychain() {
        let home = TestHome::new("private-transaction-keychain-restore");
        let operation_id = "44444444-4444-4444-8444-444444444444";
        let target = home.home.join("private-profile.json");
        let original = b"{\n\"inferenceGatewayApiKey\":\"PRIVATE-PRESTATE-SENTINEL\"\n}\n";
        let written = b"{\n\"inferenceGatewayApiKey\":\"PRIVATE-POSTSTATE-SENTINEL\"\n}\n";
        fs::write(&target, original).unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];

        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        assert!(!evidence_contains(
            &operation_root(operation_id),
            b"PRIVATE-PRESTATE-SENTINEL"
        ));
        let private_paths = BTreeSet::from([target.clone()]);
        let parents = BTreeMap::from([(target.clone(), snapshots[0].parent.clone())]);
        let reviewed =
            BTreeMap::from([(target.clone(), snapshots[0].transaction_state().unwrap())]);
        let tracker = begin_transaction_write_tracking_with_private_states(
            &transaction_write_evidence_dir(operation_id),
            std::slice::from_ref(&target),
            &private_paths,
            &parents,
            &reviewed,
        )
        .unwrap();
        crate::safe_write::write_private_if_unchanged(
            &target,
            Some(std::str::from_utf8(original).unwrap()),
            std::str::from_utf8(written).unwrap(),
        )
        .unwrap();
        let written_states = tracker.states();
        drop(tracker);
        assert!(!evidence_contains(
            &operation_root(operation_id),
            b"PRIVATE-POSTSTATE-SENTINEL"
        ));
        let loaded = load_rollback_snapshots(operation_id).unwrap().unwrap();

        assert!(restore_snapshots_if_unchanged(&loaded, &written_states).is_empty());
        assert_eq!(fs::read(&target).unwrap(), original);
        clear_private_snapshot_items(operation_id, &loaded).unwrap();
        assert!(!crate::resources::model::private_file_snapshot_exists(
            operation_id,
            &target
        ));
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_new_target_rollback_removes_only_matching_poststate() {
        let home = TestHome::new("private-transaction-new-target");
        let operation_id = "55555555-5555-4555-8555-555555555555";
        let target = home.home.join("new-private-profile.json");
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        fs::write(&target, b"PRIVATE-NEW-TARGET-SENTINEL").unwrap();
        let current = read_path_state_anchored(&target, &snapshots[0].parent).unwrap();
        let AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } = current
        else {
            panic!("private target should be a file");
        };
        let written_states = BTreeMap::from([(
            target.clone(),
            crate::safe_write::transaction_private_file_state(&bytes, mode, identity).unwrap(),
        )]);
        let loaded = load_rollback_snapshots(operation_id).unwrap().unwrap();

        assert!(restore_snapshots_if_unchanged(&loaded, &written_states).is_empty());
        assert!(!target.exists());
        clear_private_snapshot_items(operation_id, &loaded).unwrap();
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_external_edit_refuses_restore_and_retains_keychain_evidence() {
        let home = TestHome::new("private-transaction-external-edit");
        let operation_id = "66666666-6666-4666-8666-666666666666";
        let target = home.home.join("private-profile.json");
        fs::write(&target, b"PRIVATE-PRESTATE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        fs::write(&target, b"PRIVATE-Arca-POSTSTATE-SENTINEL").unwrap();
        let arca = read_path_state_anchored(&target, &snapshots[0].parent).unwrap();
        let AnchoredPathState::File {
            bytes,
            mode,
            identity,
        } = arca
        else {
            panic!("private target should be a file");
        };
        let written_states = BTreeMap::from([(
            target.clone(),
            crate::safe_write::transaction_private_file_state(&bytes, mode, identity).unwrap(),
        )]);
        fs::write(&target, b"PRIVATE-EXTERNAL-EDIT-SENTINEL").unwrap();
        let loaded = load_rollback_snapshots(operation_id).unwrap().unwrap();

        let errors = restore_snapshots_if_unchanged(&loaded, &written_states);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            fs::read(&target).unwrap(),
            b"PRIVATE-EXTERNAL-EDIT-SENTINEL"
        );
        assert!(crate::resources::model::private_file_snapshot_exists(
            operation_id,
            &target
        ));
        clear_private_snapshot_items(operation_id, &loaded).unwrap();
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_missing_keychain_prestate_fails_closed() {
        let home = TestHome::new("private-transaction-missing-keychain");
        let operation_id = "77777777-7777-4777-8777-777777777777";
        let target = home.home.join("private-profile.json");
        fs::write(&target, b"PRIVATE-PRESTATE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        crate::resources::model::clear_private_file_snapshot(operation_id, &target).unwrap();

        let error = match load_rollback_snapshots(operation_id) {
            Ok(_) => panic!("private rollback unexpectedly loaded without Keychain prestate"),
            Err(error) => error,
        };
        assert!(error.starts_with("recovery_required:"), "{error}");
        assert!(!error.contains("PRIVATE-PRESTATE-SENTINEL"));
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_no_manifest_ledger_cleans_keychain_evidence() {
        let home = TestHome::new("private-transaction-no-manifest-ledger");
        let operation_id = "88888888-8888-4888-8888-888888888888";
        let target = home.home.join("private-profile.json");
        let sentinel = b"PRIVATE-LEDGER-PRESTATE-SENTINEL";
        fs::write(&target, sentinel).unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_private_target_ledger(operation_id, &snapshots).unwrap();
        persist_private_file_snapshot(operation_id, &target, sentinel).unwrap();
        assert!(load_rollback_manifest(operation_id).unwrap().is_none());

        let ledger = load_private_target_ledger(operation_id)
            .unwrap()
            .expect("private target ledger");
        clear_private_target_ledger_items(operation_id, &ledger).unwrap();

        assert!(!private_file_snapshot_exists(operation_id, &target));
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_terminal_cleanup_never_hydrates_missing_keychain_item() {
        let home = TestHome::new("private-transaction-terminal-cleanup");
        let operation_id = "99999999-9999-4999-8999-999999999999";
        let target = home.home.join("private-profile.json");
        fs::write(&target, b"PRIVATE-TERMINAL-PRESTATE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        clear_private_file_snapshot(operation_id, &target).unwrap();
        mark_operation_state(operation_id, ROLLBACK_COMPLETE_MARKER, b"rolled-back\n").unwrap();
        let manifest = load_rollback_manifest(operation_id).unwrap().unwrap();

        finalize_completed_operation_cleanup(
            operation_id,
            &operation_root(operation_id),
            &manifest,
            &[],
        )
        .unwrap();

        assert!(!operation_root(operation_id).exists());
    }

    #[test]
    fn private_transaction_delete_failure_retains_keychain_ledger_and_root() {
        let home = TestHome::new("private-transaction-delete-failure");
        let operation_id = "91919191-9191-4919-8919-919191919191";
        let target = home.home.join("private-profile.json");
        fs::write(&target, b"PRIVATE-DELETE-FAILURE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&target).unwrap()];
        persist_rollback_snapshots(operation_id, &snapshots).unwrap();
        let manifest = load_rollback_manifest(operation_id).unwrap().unwrap();
        let _failure = crate::resources::model::set_test_credential_delete_error(
            "injected Keychain permission failure",
        );

        let error = finalize_completed_operation_cleanup(
            operation_id,
            &operation_root(operation_id),
            &manifest,
            &[],
        )
        .unwrap_err();

        assert!(error.contains("permission failure"), "{error}");
        assert!(private_target_ledger_path(operation_id).is_file());
        assert!(private_file_snapshot_exists(operation_id, &target));
        assert!(operation_root(operation_id).is_dir());
        drop(_failure);
        clear_private_file_snapshot(operation_id, &target).unwrap();
        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_cleanup_authorized_takes_precedence_over_target_incident() {
        let _home = TestHome::new("private-transaction-cleanup-precedence");
        let operation_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        fs::create_dir_all(operation_root(operation_id)).unwrap();
        write_private_file(
            &operation_root(operation_id).join(TARGET_INCIDENT_MARKER),
            b"incident\n",
        )
        .unwrap();
        mark_operation_state(
            operation_id,
            CLEANUP_AUTHORIZED_MARKER,
            b"cleanup-authorized\n",
        )
        .unwrap();

        assert!(!operation_should_pause_for_target_incident(operation_id).unwrap());

        fs::remove_dir_all(operation_root(operation_id)).unwrap();
    }

    #[test]
    fn private_transaction_real_claude_plan_commit_recovers_without_filesystem_secret_evidence() {
        let home = TestHome::new("private-transaction-real-claude-plan");
        let profile = ModelProfile {
            id: "claude-desktop-private-plan".into(),
            provider_id: Some("claude-desktop-private-provider".into()),
            name: "Claude Desktop Private Plan".into(),
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: "https://gateway.example.test".into(),
            endpoint_path: "/anthropic/v1/messages".into(),
            model: "claude-sonnet-4-6".into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
        };
        let new_secret = "PRIVATE-REAL-PLAN-NEW-SENTINEL";
        let old_secret = "PRIVATE-REAL-PLAN-OLD-SENTINEL";
        save_profile(profile.clone(), Some(new_secret.into())).unwrap();
        let defaults = crate::resources::model::default_config_paths("claude-desktop").unwrap();
        let private_path = crate::resources::mcp::scanner::expand_tilde(&defaults[3]);
        fs::create_dir_all(private_path.parent().unwrap()).unwrap();
        fs::write(
            &private_path,
            format!("{{\"inferenceGatewayApiKey\":\"{old_secret}\"}}"),
        )
        .unwrap();
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-desktop".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![profile.id.clone()],
            },
        })
        .unwrap();
        let persisted = load_operation(&plan.operation_id).unwrap();
        let settings = load_settings_strict().unwrap();
        let private_paths = private_transaction_paths(&plan, &settings).unwrap();
        assert_eq!(private_paths, BTreeSet::from([private_path.clone()]));
        let settings_path = settings_file();
        let catalog_paths = [
            mcp_catalog_file(),
            model_catalog_file(),
            skill_catalog_file(),
        ];
        let mut snapshots = vec![PathSnapshot::capture(&settings_path).unwrap()];
        snapshots.extend(
            catalog_paths
                .iter()
                .map(|path| PathSnapshot::capture(path).unwrap()),
        );
        snapshots.extend(plan.target_files.iter().filter_map(|target| {
            let path = crate::resources::mcp::scanner::expand_tilde(target);
            (path != settings_path && !catalog_paths.contains(&path)).then(|| {
                if private_paths.contains(&path) {
                    PathSnapshot::capture_private(&path).unwrap()
                } else {
                    PathSnapshot::capture(&path).unwrap()
                }
            })
        }));
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracked_paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone())
            .collect::<Vec<_>>();
        let parents = parent_snapshots_for_snapshots(&snapshots);
        let reviewed = snapshots
            .iter()
            .map(|snapshot| (snapshot.path.clone(), snapshot.transaction_state().unwrap()))
            .collect::<BTreeMap<_, _>>();
        let tracker = begin_transaction_write_tracking_with_private_states(
            &transaction_write_evidence_dir(&plan.operation_id),
            &tracked_paths,
            &private_paths,
            &parents,
            &reviewed,
        )
        .unwrap();
        let skills_guard = acquire_asset_skills_lock().unwrap();

        apply_operation(&persisted, &skills_guard, &BTreeSet::new()).unwrap();
        verify_operation(&persisted).unwrap();
        mark_operation_committed(&plan.operation_id).unwrap();
        assert!(!evidence_contains(
            &operation_root(&plan.operation_id),
            old_secret.as_bytes()
        ));
        assert!(!evidence_contains(
            &operation_root(&plan.operation_id),
            new_secret.as_bytes()
        ));
        drop(tracker);
        drop(skills_guard);

        recover_pending_asset_operations().unwrap();

        let committed_profile: Value =
            serde_json::from_str(&fs::read_to_string(&private_path).unwrap()).unwrap();
        assert_eq!(committed_profile["inferenceGatewayApiKey"], new_secret);
        assert!(!private_file_snapshot_exists(
            &plan.operation_id,
            &private_path
        ));
        assert!(!operation_root(&plan.operation_id).exists());
        assert!(home.home.exists());
    }

    #[test]
    fn private_transaction_overridden_path_no_manifest_startup_cleans_keychain() {
        let _home = TestHome::new("private-transaction-override-startup-cleanup");
        let profile = save_private_claude_profile(
            "claude-private-override-startup",
            "PRIVATE-OVERRIDE-STARTUP-SENTINEL",
        );
        let custom_paths = vec![
            "~/custom/claude-main.json".into(),
            "~/custom/claude-3p.json".into(),
            "~/custom/_meta.json".into(),
            "~/custom/private-profile.json".into(),
        ];
        mutate_settings(|settings| {
            settings.agent_config_paths.get_or_insert_default().insert(
                "claude-desktop".into(),
                crate::settings::AgentConfigPathOverride {
                    model_paths: Some(custom_paths.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();
        let plan = plan_private_claude_consumption(&profile.id);
        let private_path = crate::resources::mcp::scanner::expand_tilde(&custom_paths[3]);
        assert!(plan
            .target_files
            .iter()
            .any(|target| crate::resources::mcp::scanner::expand_tilde(target) == private_path));
        let snapshots = vec![PathSnapshot::capture_private(&private_path).unwrap()];
        persist_private_target_ledger(&plan.operation_id, &snapshots).unwrap();
        persist_private_file_snapshot(
            &plan.operation_id,
            &private_path,
            b"PRIVATE-OVERRIDE-PRESTATE-SENTINEL",
        )
        .unwrap();

        recover_pending_asset_operations().unwrap();

        assert!(!private_file_snapshot_exists(
            &plan.operation_id,
            &private_path
        ));
        assert!(!operation_root(&plan.operation_id).exists());
    }

    #[test]
    fn private_transaction_cancel_cleans_no_manifest_but_refuses_manifest_evidence() {
        let _home = TestHome::new("private-transaction-cancel-contract");
        let profile =
            save_private_claude_profile("claude-private-cancel", "PRIVATE-CANCEL-SENTINEL");
        let first = plan_private_claude_consumption(&profile.id);
        let private_path = crate::resources::mcp::scanner::expand_tilde(
            &crate::resources::model::default_config_paths("claude-desktop").unwrap()[3],
        );
        let snapshots = vec![PathSnapshot::capture_private(&private_path).unwrap()];
        persist_private_target_ledger(&first.operation_id, &snapshots).unwrap();
        persist_private_file_snapshot(
            &first.operation_id,
            &private_path,
            b"PRIVATE-CANCEL-NO-MANIFEST-SENTINEL",
        )
        .unwrap();

        cancel_asset_operation(&first.operation_id).unwrap();
        assert!(!private_file_snapshot_exists(
            &first.operation_id,
            &private_path
        ));
        assert!(!operation_root(&first.operation_id).exists());

        let second = plan_private_claude_consumption(&profile.id);
        persist_rollback_snapshots(&second.operation_id, &snapshots).unwrap();
        let error = cancel_asset_operation(&second.operation_id).unwrap_err();
        assert!(error.starts_with("recovery_required:"), "{error}");
        assert!(operation_root(&second.operation_id).exists());
        recover_pending_asset_operations().unwrap();
        assert!(!operation_root(&second.operation_id).exists());
    }

    #[test]
    fn private_transaction_injected_persistence_failure_cleans_keychain_and_ledger() {
        let home = TestHome::new("private-transaction-persist-failure");
        let operation_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        let private_path = home.home.join("private-profile.json");
        fs::write(&private_path, b"PRIVATE-PERSIST-FAILURE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&private_path).unwrap()];
        set_after_private_snapshot_persist_hook(|| Err("injected persistence failure".into()));

        let error = persist_rollback_snapshots(operation_id, &snapshots).unwrap_err();

        assert!(error.contains("injected persistence failure"), "{error}");
        assert!(!private_file_snapshot_exists(operation_id, &private_path));
        assert!(!private_target_ledger_path(operation_id).exists());
    }

    #[test]
    fn private_transaction_resolved_incident_cleanup_clears_keychain_before_root() {
        let home = TestHome::new("private-transaction-resolved-incident");
        let profile =
            save_private_claude_profile("claude-private-incident", "PRIVATE-INCIDENT-SENTINEL");
        let plan = plan_private_claude_consumption(&profile.id);
        let private_path = crate::resources::mcp::scanner::expand_tilde(
            &crate::resources::model::default_config_paths("claude-desktop").unwrap()[3],
        );
        fs::create_dir_all(private_path.parent().unwrap()).unwrap();
        fs::write(&private_path, b"PRIVATE-INCIDENT-PRESTATE-SENTINEL").unwrap();
        let snapshots = vec![PathSnapshot::capture_private(&private_path).unwrap()];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        record_target_incident(&plan, "claude-desktop", "target_recovery_required").unwrap();
        clear_target_incidents(&plan, "claude-desktop").unwrap();

        cleanup_resolved_incident_operations(
            &BTreeSet::from([plan.operation_id.clone()]),
            "different-current-operation",
        );

        assert!(!private_file_snapshot_exists(
            &plan.operation_id,
            &private_path
        ));
        assert!(!operation_root(&plan.operation_id).exists());
        assert!(home.home.exists());
    }

    #[test]
    fn transaction_snapshot_refuses_a_directory_without_deleting_it() {
        let home = TestHome::new("transaction-directory-snapshot");
        let directory = home.home.join("must-remain");
        fs::create_dir_all(&directory).unwrap();
        let sentinel = directory.join("sentinel.txt");
        fs::write(&sentinel, "keep").unwrap();

        let error = PathSnapshot::capture(&directory).unwrap_err();

        assert!(error.contains("refusing to snapshot directory"), "{error}");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn transaction_snapshot_refuses_a_symlinked_configuration() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("transaction-config-symlink");
        let destination = home.home.join("settings-target.json");
        let link = home.home.join("settings.json");
        fs::write(&destination, "must remain untouched").unwrap();
        symlink(&destination, &link).unwrap();

        let error = PathSnapshot::capture(&link).unwrap_err();

        assert!(error.contains("symlinked configuration"), "{error}");
        assert_eq!(
            fs::read_to_string(destination).unwrap(),
            "must remain untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rollback_refuses_a_replaced_skill_parent_without_touching_the_symlink_destination() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("transaction-skill-parent-swap");
        let parent = home.home.join(".agents/skills");
        fs::create_dir_all(&parent).unwrap();
        let target = parent.join("reviewed-skill");
        let central = home.home.join(".arca/assets/skills/items/reviewed-skill");
        let original = PathSnapshot::capture_link(&target).unwrap();
        let paths = vec![target.clone()];
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            &paths,
            &parent_snapshots_for_paths(&paths),
        )
        .unwrap();
        set_transaction_symlink(&target, Some(&central)).unwrap();

        let retained_parent = home.home.join("retained-skills-parent");
        fs::rename(&parent, &retained_parent).unwrap();
        let outside = home.home.join("outside-skills-parent");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &parent).unwrap();

        let written_states = tracker.states();
        drop(tracker);
        let errors = restore_snapshots_if_unchanged(&[original], &written_states);

        assert!(!errors.is_empty(), "a swapped parent must require recovery");
        assert!(
            errors
                .iter()
                .any(|error| error.contains("asset_target_unsafe")),
            "{errors:?}"
        );
        assert!(!outside.join("reviewed-skill").exists());
        assert_eq!(
            fs::read_link(retained_parent.join("reviewed-skill")).unwrap(),
            central
        );
    }

    #[cfg(unix)]
    #[test]
    fn forward_skill_link_refuses_a_parent_swap_after_snapshot() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("transaction-forward-skill-parent-swap");
        let parent = home.home.join(".agents/skills");
        fs::create_dir_all(&parent).unwrap();
        let target = parent.join("reviewed-skill");
        let central = home.home.join(".arca/assets/skills/items/reviewed-skill");
        fs::create_dir_all(&central).unwrap();
        let paths = vec![target.clone()];
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            &paths,
            &parent_snapshots_for_paths(&paths),
        )
        .unwrap();

        let retained_parent = home.home.join("retained-skills-parent");
        fs::rename(&parent, &retained_parent).unwrap();
        let outside = home.home.join("outside-skills-parent");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, &parent).unwrap();

        let error = create_transaction_symlink_if_missing(&target, &central).unwrap_err();
        drop(tracker);

        assert!(error.contains("asset_target_unsafe"), "{error}");
        assert!(!outside.join("reviewed-skill").exists());
        assert!(!retained_parent.join("reviewed-skill").exists());
    }

    #[test]
    fn rollback_preserves_an_external_edit_before_rollback_preparation() {
        let home = TestHome::new("transaction-rollback-cas");
        let target = home.home.join("config.json");
        fs::write(&target, "original").unwrap();
        let original = PathSnapshot::capture(&target).unwrap();
        let evidence = home.home.join("write-evidence");
        let paths = std::slice::from_ref(&target);
        let tracker =
            begin_transaction_write_tracking(&evidence, paths, &parent_snapshots_for_paths(paths))
                .unwrap();
        crate::safe_write::write_if_unchanged(&target, Some("original"), "arca-partial").unwrap();
        fs::write(&target, "external-edit").unwrap();
        let states = tracker.states();
        drop(tracker);
        let expected = states
            .get(&target)
            .map(|state| PathSnapshot::from_transaction_state(&original, state).unwrap());

        let error = original.restore_if_owned(expected.as_ref()).unwrap_err();

        assert!(error.contains("changed after Arca wrote it"), "{error}");
        assert_eq!(fs::read_to_string(target).unwrap(), "external-edit");
    }

    #[test]
    fn settings_edit_after_snapshot_is_never_written_over_or_rolled_back() {
        let home = TestHome::new("transaction-settings-post-snapshot-race");
        mutate_settings(|settings| settings.imported = Some("reviewed".into())).unwrap();
        let path = settings_file();
        let snapshots = vec![PathSnapshot::capture(&path).unwrap()];
        let tracked_paths = vec![path.clone()];
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        let external = fs::read_to_string(&path)
            .unwrap()
            .replace("reviewed", "external-update");
        fs::write(&path, &external).unwrap();

        let error = mutate_settings(|settings| settings.state = Some(serde_json::json!("arca")))
            .unwrap_err();
        assert!(
            error.to_string().contains("reviewed target changed"),
            "{error}"
        );
        let states = tracker.states();
        drop(tracker);

        let rollback_errors = restore_snapshots_if_unchanged(&snapshots, &states);
        assert_eq!(rollback_errors.len(), 1, "{rollback_errors:?}");
        assert_eq!(fs::read_to_string(&path).unwrap(), external);
    }

    #[test]
    fn rollback_restores_a_file_when_the_cas_state_matches() {
        let home = TestHome::new("transaction-rollback-success");
        let target = home.home.join("config.json");
        fs::write(&target, "original").unwrap();
        let original = PathSnapshot::capture(&target).unwrap();
        let evidence = home.home.join("write-evidence");
        let paths = std::slice::from_ref(&target);
        let tracker =
            begin_transaction_write_tracking(&evidence, paths, &parent_snapshots_for_paths(paths))
                .unwrap();
        crate::safe_write::write_if_unchanged(&target, Some("original"), "arca-partial").unwrap();
        let states = tracker.states();
        drop(tracker);
        let expected = states
            .get(&target)
            .map(|state| PathSnapshot::from_transaction_state(&original, state).unwrap());

        original.restore_if_owned(expected.as_ref()).unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[test]
    fn rollback_preflights_every_path_before_restoring_any_file() {
        let home = TestHome::new("transaction-rollback-preflight");
        let externally_edited = home.home.join("first.json");
        let still_owned = home.home.join("second.json");
        fs::write(&externally_edited, "first-original").unwrap();
        fs::write(&still_owned, "second-original").unwrap();
        let snapshots = vec![
            PathSnapshot::capture(&externally_edited).unwrap(),
            PathSnapshot::capture(&still_owned).unwrap(),
        ];
        let tracked_paths = vec![externally_edited.clone(), still_owned.clone()];
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        crate::safe_write::write_if_unchanged(
            &externally_edited,
            Some("first-original"),
            "first-arca",
        )
        .unwrap();
        crate::safe_write::write_if_unchanged(&still_owned, Some("second-original"), "second-arca")
            .unwrap();
        fs::write(&externally_edited, "external-edit").unwrap();
        let states = tracker.states();
        drop(tracker);

        let errors = restore_snapshots_if_unchanged(&snapshots, &states);

        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            fs::read_to_string(externally_edited).unwrap(),
            "external-edit"
        );
        assert_eq!(fs::read_to_string(still_owned).unwrap(), "second-arca");
    }

    #[test]
    fn mcp_commit_updates_target_and_relationship_together() {
        let _home = TestHome::new("consume-commit");
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
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let inventory = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        assert!(inventory.consumptions.iter().any(|item| {
            item.agent_id == "claude-code"
                && item.asset
                    == (AssetRef::Mcp {
                        key: "local::stdio".into(),
                    })
                && item.status == ConsumptionStatus::Synced
        }));
    }

    #[test]
    fn commit_rejects_a_leaf_edit_after_precondition_verification() {
        let _home = TestHome::new("consume-post-verify-leaf-race");
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
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let target = crate::resources::mcp::scanner::expand_tilde(&plan.target_files[0]);
        let external = r#"{"mcpServers":{"external":{"command":"must-survive"}}}"#;

        let error = commit_asset_operation_with_hook(
            AssetCommitRequest {
                operation_id: plan.operation_id,
                candidate_hash: plan.candidate_hash,
            },
            || {
                fs::create_dir_all(target.parent().unwrap()).map_err(|error| error.to_string())?;
                fs::write(&target, external).map_err(|error| error.to_string())
            },
        )
        .unwrap_err();

        assert!(error.contains("changed while preparing commit"), "{error}");
        assert_eq!(fs::read_to_string(target).unwrap(), external);
    }

    #[test]
    fn foreign_rollback_manifest_does_not_block_unrelated_asset_commit() {
        let _home = TestHome::new("consume-foreign-recovery");
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(RegistryEntry {
                    name: "blocked-by-recovery".into(),
                    description: String::new(),
                    tags: Vec::new(),
                    config: RegistryConfig {
                        stdio: Some(StdioConfig {
                            command: "blocked-server".into(),
                            args: None,
                            env: None,
                            cwd: None,
                        }),
                        http: None,
                    },
                    origin: None,
                    repo: None,
                }),
            },
        })
        .unwrap();
        let foreign_id = uuid::Uuid::new_v4().to_string();
        let foreign_rollback = operation_root(&foreign_id).join("rollback");
        fs::create_dir_all(&foreign_rollback).unwrap();
        fs::write(foreign_rollback.join("manifest.json"), b"{}").unwrap();

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert!(
            read_registry()
                .into_iter()
                .any(|entry| entry.name == "blocked-by-recovery"),
            "the unrelated central asset must remain writable"
        );
        assert!(foreign_rollback.join("manifest.json").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn unsafe_asset_rollback_marker_fails_closed() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("consume-unsafe-recovery");
        let foreign_id = uuid::Uuid::new_v4().to_string();
        let foreign_rollback = operation_root(&foreign_id).join("rollback");
        fs::create_dir_all(&foreign_rollback).unwrap();
        let outside = home.home.join("outside-manifest");
        fs::write(&outside, b"must-not-be-read").unwrap();
        symlink(&outside, foreign_rollback.join("manifest.json")).unwrap();

        let error = recover_pending_asset_operations().unwrap_err();

        assert!(error.starts_with("recovery_required:"), "{error}");
        assert_eq!(fs::read(outside).unwrap(), b"must-not-be-read");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_asset_operation_root_cannot_hide_pending_recovery() {
        use std::os::unix::fs::symlink;

        let home = TestHome::new("consume-root-symlink");
        let staging = home.home.join(".arca/staging");
        fs::create_dir_all(&staging).unwrap();
        let outside = home.home.join("outside-consumption");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, staging.join("consumption")).unwrap();

        let error = recover_pending_asset_operations().unwrap_err();

        assert!(error.starts_with("recovery_required:"), "{error}");
        assert!(outside.read_dir().unwrap().next().is_none());
    }

    #[test]
    fn subscribed_mcp_edit_creates_manual_override_without_mutating_source_cache() {
        let _home = TestHome::new("consume-subscription-override");
        let source = SourceDef::new_remote(
            "team-catalog".into(),
            "Team catalog".into(),
            "https://example.invalid/mcp.json".into(),
            "json".into(),
            "2026-07-25T00:00:00Z".into(),
        );
        mutate_settings(|settings| {
            settings
                .sources
                .get_or_insert_default()
                .push(source.clone());
        })
        .unwrap();
        let source_path = cached_path(&source).unwrap();
        fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let subscribed = r#"{"mcpServers":{"shared":{"command":"npx","args":["shared-mcp"]}}}"#;
        fs::write(&source_path, subscribed).unwrap();

        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: Some("shared::stdio".into()),
                entry: Box::new(RegistryEntry {
                    name: "shared".into(),
                    description: "Personal credentials".into(),
                    tags: Vec::new(),
                    config: RegistryConfig {
                        stdio: Some(StdioConfig {
                            command: "npx".into(),
                            args: Some(vec!["shared-mcp".into()]),
                            env: Some(HashMap::from([(
                                "SHARED_API_KEY".into(),
                                "user-secret".into(),
                            )])),
                            cwd: None,
                        }),
                        http: None,
                    },
                    origin: None,
                    repo: None,
                }),
            },
        })
        .unwrap();
        assert_eq!(plan.central_changes[0].action, CentralAssetAction::Create);

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        assert_eq!(fs::read_to_string(source_path).unwrap(), subscribed);
        let copies = read_registry_all()
            .into_iter()
            .filter(|item| item.entry.key() == "shared::stdio")
            .collect::<Vec<_>>();
        assert_eq!(copies.len(), 2);
        let effective = copies.iter().find(|item| item.in_effect).unwrap();
        assert_eq!(effective.entry.origin.as_ref().unwrap().kind, "manual");
        assert_eq!(
            effective
                .entry
                .config
                .stdio
                .as_ref()
                .unwrap()
                .env
                .as_ref()
                .unwrap()
                .get("SHARED_API_KEY")
                .map(String::as_str),
            Some("user-secret"),
        );
    }

    #[test]
    fn mcp_enabled_toggle_preserves_relationship_and_restores_snapshot() {
        let _home = TestHome::new("consume-enabled-toggle");
        write_manual_entry(&RegistryEntry {
            name: "local".into(),
            description: String::new(),
            tags: Vec::new(),
            config: RegistryConfig {
                stdio: Some(StdioConfig {
                    command: "local-server".into(),
                    args: Some(vec!["--keep-me".into()]),
                    env: None,
                    cwd: None,
                }),
                http: None,
            },
            origin: None,
            repo: None,
        })
        .unwrap();
        let added = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: added.operation_id,
            candidate_hash: added.candidate_hash,
        })
        .unwrap();

        for enabled in [false, true] {
            let plan = plan_set_mcp_enabled(PlanSetMcpEnabledRequest {
                agent_id: "claude-code".into(),
                asset_key: "local::stdio".into(),
                enabled,
            })
            .unwrap();
            let inventory = commit_asset_operation(AssetCommitRequest {
                operation_id: plan.operation_id,
                candidate_hash: plan.candidate_hash,
            })
            .unwrap();
            assert!(inventory.consumptions.iter().any(|item| {
                item.agent_id == "claude-code"
                    && item.asset
                        == (AssetRef::Mcp {
                            key: "local::stdio".into(),
                        })
                    && item.desired
                    && item.enabled == Some(enabled)
                    && item.status == ConsumptionStatus::Synced
            }));
        }
    }

    #[test]
    fn bulk_mcp_enabled_toggle_updates_every_managed_relationship() {
        let _home = TestHome::new("consume-enabled-toggle-all");
        for name in ["alpha", "beta"] {
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
        let added = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["alpha::stdio".into(), "beta::stdio".into()],
            },
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: added.operation_id,
            candidate_hash: added.candidate_hash,
        })
        .unwrap();

        for enabled in [false, true] {
            let plan = plan_set_all_mcp_enabled(PlanSetAllMcpEnabledRequest {
                agent_id: "claude-code".into(),
                enabled,
            })
            .unwrap();
            assert_eq!(plan.consumption_state_changes.len(), 2);
            let inventory = commit_asset_operation(AssetCommitRequest {
                operation_id: plan.operation_id,
                candidate_hash: plan.candidate_hash,
            })
            .unwrap();
            let rows = inventory
                .consumptions
                .iter()
                .filter(|item| {
                    item.agent_id == "claude-code" && matches!(item.asset, AssetRef::Mcp { .. })
                })
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), 2);
            assert!(rows.iter().all(|item| item.enabled == Some(enabled)));
        }
    }

    #[test]
    fn startup_recovery_rolls_back_a_partial_asset_operation() {
        let home = TestHome::new("consume-recovery-rollback");
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
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let target = home.home.join(".claude.json");
        let settings_before = fs::read(settings_file()).unwrap();
        let snapshots = vec![
            PathSnapshot::capture(&settings_file()).unwrap(),
            PathSnapshot::capture(&target).unwrap(),
        ];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracked_paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone())
            .collect::<Vec<_>>();
        let tracker = begin_transaction_write_tracking(
            &transaction_write_evidence_dir(&plan.operation_id),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        crate::safe_write::write_if_unchanged(&target, None, "partial mutation").unwrap();
        mutate_settings(|settings| {
            settings.mcp_consumptions.get_or_insert_default().insert(
                "claude-code".into(),
                BTreeMap::from([(
                    "local::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "local::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                )]),
            );
        })
        .unwrap();
        drop(tracker);

        let recovered = recover_pending_asset_operations().unwrap();
        assert_eq!(recovered, vec![plan.operation_id]);
        assert_eq!(fs::read(settings_file()).unwrap(), settings_before);
        assert!(!target.exists());
    }

    #[test]
    fn startup_recovery_localizes_an_externally_changed_target() {
        let home = TestHome::new("consume-target-incident");
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
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let target = home.home.join(".claude.json");
        let snapshots = vec![
            PathSnapshot::capture(&settings_file()).unwrap(),
            PathSnapshot::capture(&target).unwrap(),
        ];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracked_paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone())
            .collect::<Vec<_>>();
        let tracker = begin_transaction_write_tracking(
            &transaction_write_evidence_dir(&plan.operation_id),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        crate::safe_write::write_if_unchanged(&target, None, "partial mutation").unwrap();
        mutate_settings(|settings| {
            settings.mcp_consumptions.get_or_insert_default().insert(
                "claude-code".into(),
                BTreeMap::from([(
                    "local::stdio".into(),
                    McpConsumptionRecord {
                        asset_key: "local::stdio".into(),
                        enabled: true,
                        overrides: Default::default(),
                    },
                )]),
            );
        })
        .unwrap();
        drop(tracker);
        let external = r#"{"mcpServers":{"external":{"command":"must-survive"}}}"#;
        fs::write(&target, external).unwrap();

        assert!(recover_pending_asset_operations().unwrap().is_empty());

        assert_eq!(fs::read_to_string(&target).unwrap(), external);
        let settings = load_settings_strict().unwrap();
        assert!(settings.mcp_consumptions.unwrap()["claude-code"].contains_key("local::stdio"));
        let incidents = settings.target_incidents.unwrap();
        assert_eq!(incidents.len(), 1);
        let incident = incidents.values().next().unwrap();
        assert_eq!(incident.capability, AssetCapability::Mcp);
        assert_eq!(incident.affected_agent_ids, ["claude-code"]);
        assert_eq!(incident.code, "target_recovery_required");
        assert!(operation_root(&plan.operation_id)
            .join(TARGET_INCIDENT_MARKER)
            .is_file());
    }

    #[test]
    fn cancellation_preserves_started_transaction_recovery_evidence() {
        let _home = TestHome::new("consume-cancel-recovery-evidence");
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Mcp {
                existing_key: None,
                entry: Box::new(RegistryEntry {
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
                }),
            },
        })
        .unwrap();
        let settings_path = settings_file();
        let snapshots = vec![PathSnapshot::capture(&settings_path).unwrap()];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracker = begin_transaction_write_tracking(
            &transaction_write_evidence_dir(&plan.operation_id),
            std::slice::from_ref(&settings_path),
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        mutate_settings(|settings| settings.imported = Some("partial".into())).unwrap();
        drop(tracker);

        let error = cancel_asset_operation(&plan.operation_id).unwrap_err();

        assert!(error.starts_with("recovery_required:"), "{error}");
        let root = operation_root(&plan.operation_id);
        assert!(root.join("rollback/manifest.json").is_file());
        assert!(
            fs::read_dir(root.join("rollback/post"))
                .unwrap()
                .next()
                .is_some(),
            "post-write ownership evidence must remain available"
        );
        assert_eq!(
            recover_pending_asset_operations().unwrap(),
            vec![plan.operation_id]
        );
        assert!(!settings_path.exists());
    }

    #[test]
    fn commit_marker_crosses_every_durability_barrier_before_success() {
        let _home = TestHome::new("consume-commit-marker-durability");
        let operation_id = uuid::Uuid::new_v4().to_string();
        fs::create_dir_all(operation_root(&operation_id)).unwrap();
        let marker = operation_commit_marker(&operation_id);
        let mut observed = Vec::new();

        let error = mark_operation_committed_with_barrier_hook(&operation_id, |barrier| {
            assert_eq!(fs::read(&marker).unwrap(), b"committed\n");
            observed.push(barrier);
            if barrier == CommitMarkerDurabilityBarrier::OperationDirectory {
                return Err("simulated parent-directory sync interruption".into());
            }
            Ok(())
        })
        .unwrap_err();

        assert_eq!(error, "simulated parent-directory sync interruption");
        assert_eq!(
            observed,
            vec![
                CommitMarkerDurabilityBarrier::File,
                CommitMarkerDurabilityBarrier::OperationDirectory,
            ]
        );

        observed.clear();
        mark_operation_committed_with_barrier_hook(&operation_id, |barrier| {
            observed.push(barrier);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            observed,
            vec![
                CommitMarkerDurabilityBarrier::File,
                CommitMarkerDurabilityBarrier::OperationDirectory,
                CommitMarkerDurabilityBarrier::ParentDirectory,
            ]
        );
    }

    #[test]
    fn startup_recovery_finalizes_an_already_verified_commit() {
        let home = TestHome::new("consume-recovery-finalize");
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
        let plan = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "claude-code".into(),
            selection: AgentConsumptionSelection::Mcp {
                asset_keys: vec!["local::stdio".into()],
            },
        })
        .unwrap();
        let persisted = load_operation(&plan.operation_id).unwrap();
        let target = home.home.join(".claude.json");
        let snapshots = vec![
            PathSnapshot::capture(&settings_file()).unwrap(),
            PathSnapshot::capture(&target).unwrap(),
        ];
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let skills_guard = acquire_asset_skills_lock().unwrap();
        apply_operation(&persisted, &skills_guard, &BTreeSet::new()).unwrap();
        verify_operation(&persisted).unwrap();
        mark_operation_committed(&plan.operation_id).unwrap();
        drop(skills_guard);

        recover_pending_asset_operations().unwrap();
        assert!(target.exists());
        assert!(
            load_settings_strict().unwrap().mcp_consumptions.unwrap()["claude-code"]
                .contains_key("local::stdio")
        );
        assert!(!operation_root(&plan.operation_id).exists());
    }

    #[test]
    fn model_recovery_preserves_the_provider_keychain_value_before_commit_marker() {
        let _home = TestHome::new("consume-model-keychain-rollback");
        save_profile(model("old-model"), Some("old-secret".into())).unwrap();
        let settings_before = fs::read(settings_file()).unwrap();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()),
                profile: Box::new(model("new-model")),
                credential: None,
            },
        })
        .unwrap();
        let persisted = load_operation(&plan.operation_id).unwrap();
        let snapshots = vec![PathSnapshot::capture(&settings_file()).unwrap()];
        let old_credential = credential_snapshot("work").unwrap();
        persist_credential_rollback(&plan.operation_id, "work", old_credential.as_deref()).unwrap();
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracked_paths = snapshots
            .iter()
            .map(|snapshot| snapshot.path.clone())
            .collect::<Vec<_>>();
        let tracker = begin_transaction_write_tracking(
            &transaction_write_evidence_dir(&plan.operation_id),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        let skills_guard = acquire_asset_skills_lock().unwrap();
        apply_operation(&persisted, &skills_guard, &BTreeSet::new()).unwrap();
        drop(tracker);
        drop(skills_guard);
        assert_eq!(credential_snapshot("work").unwrap().unwrap(), b"old-secret");

        recover_pending_asset_operations().unwrap();
        assert_eq!(fs::read(settings_file()).unwrap(), settings_before);
        assert_eq!(credential_snapshot("work").unwrap().unwrap(), b"old-secret");
        assert!(!operation_root(&plan.operation_id).exists());
    }

    #[test]
    fn provider_commit_updates_every_child_model_and_credential() {
        let _home = TestHome::new("consume-provider-commit");
        let (mut provider, first, second) = install_shared_provider_models();
        provider.base_url = "https://new.example.test/".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider.clone()),
                credential: Some("new-secret".into()),
            },
        })
        .unwrap();

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        let settings = load_settings_strict().unwrap();
        let profiles = settings.model_profiles.unwrap();
        assert_eq!(profiles[&first.id].base_url, "https://new.example.test");
        assert_eq!(profiles[&first.id].endpoint_path, "/v1/responses");
        assert_eq!(profiles[&second.id].base_url, "https://new.example.test");
        assert_eq!(profiles[&second.id].endpoint_path, "/anthropic/v1/messages");
        assert_eq!(credential_snapshot(&first.id).unwrap().unwrap(), b"new-secret");
        assert_eq!(credential_snapshot(&second.id).unwrap().unwrap(), b"new-secret");
    }

    #[test]
    fn provider_update_repairs_only_its_reviewed_child_profiles() {
        let home = TestHome::new("consume-provider-scoped-children");
        let (mut provider, mut first, mut second) = install_shared_provider_models();
        provider.api_key_source = Some(crate::domain::types::ApiKeySource::Env {
            name: "SHARED_PROVIDER_API_KEY".into(),
        });
        provider.env_key = Some("SHARED_PROVIDER_API_KEY".into());
        first.env_key = provider.env_key.clone();
        second.env_key = provider.env_key.clone();
        mutate_settings(|settings| {
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            let profiles = settings.model_profiles.get_or_insert_default();
            profiles.insert(first.id.clone(), first.clone());
            profiles.insert(second.id.clone(), second.clone());
        })
        .unwrap();
        let mut third = model("third-model");
        third.id = "third-profile".into();
        third.provider_id = Some("other-provider".into());
        third.name = "Third Profile".into();
        save_profile(third.clone(), None).unwrap();

        let assignment = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![first.id.clone(), second.id.clone(), third.id.clone()],
            },
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: assignment.operation_id,
            candidate_hash: assignment.candidate_hash,
        })
        .unwrap();

        let target = home.home.join(".grok/config.toml");
        let drifted = fs::read_to_string(&target)
            .unwrap()
            .replace("gpt-shared", "first-reviewed-drift")
            .replace("third-model", "third-unreviewed-drift");
        fs::write(&target, &drifted).unwrap();

        provider.base_url = "https://new.example.test".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider),
                credential: None,
            },
        })
        .unwrap();
        assert!(plan.can_commit, "{:?}", plan.warnings);
        assert!(plan.central_changes.iter().any(|change| change.asset
            == (AssetRef::Model {
                profile_id: first.id.clone(),
            })));
        assert!(plan.central_changes.iter().any(|change| change.asset
            == (AssetRef::Model {
                profile_id: second.id.clone(),
            })));
        assert!(!plan.central_changes.iter().any(|change| change.asset
            == (AssetRef::Model {
                profile_id: third.id.clone(),
            })));
        assert!(!plan
            .warnings
            .iter()
            .any(|warning| warning.contains(&format!("model:{}", first.id))));
        assert!(!plan
            .warnings
            .iter()
            .any(|warning| warning.contains(&format!("model:{}", third.id))));

        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        let updated = fs::read_to_string(target).unwrap();
        assert!(!updated.contains("first-reviewed-drift"));
        assert!(updated.contains("gpt-shared"));
        assert!(updated.contains("https://new.example.test"));
        assert!(updated.contains("third-unreviewed-drift"));
        assert!(!updated.contains("third-model"));
    }

    #[test]
    fn provider_update_skips_a_disabled_child_consumer_and_preserves_its_payload() {
        let home = TestHome::new("consume-provider-disabled-child");
        let (mut provider, mut first, mut second) = install_shared_provider_models();
        provider.api_key_source = Some(crate::domain::types::ApiKeySource::Env {
            name: "SHARED_PROVIDER_API_KEY".into(),
        });
        provider.env_key = Some("SHARED_PROVIDER_API_KEY".into());
        first.env_key = provider.env_key.clone();
        second.env_key = provider.env_key.clone();
        mutate_settings(|settings| {
            settings
                .model_providers
                .get_or_insert_default()
                .insert(provider.id.clone(), provider.clone());
            let profiles = settings.model_profiles.get_or_insert_default();
            profiles.insert(first.id.clone(), first.clone());
            profiles.insert(second.id.clone(), second.clone());
        })
        .unwrap();

        let assignment = plan_set_agent_consumption(PlanSetAgentConsumptionRequest {
            agent_id: "grok-build".into(),
            selection: AgentConsumptionSelection::Model {
                profile_ids: vec![first.id.clone(), second.id.clone()],
            },
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: assignment.operation_id,
            candidate_hash: assignment.candidate_hash,
        })
        .unwrap();

        let make_first_active = plan_set_active_model(PlanSetActiveModelRequest {
            agent_id: "grok-build".into(),
            profile_id: first.id.clone(),
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: make_first_active.operation_id,
            candidate_hash: make_first_active.candidate_hash,
        })
        .unwrap();

        let target = home.home.join(".grok/config.toml");
        let managed = fs::read_to_string(&target).unwrap();
        let disable = plan_set_model_enabled(PlanSetModelEnabledRequest {
            agent_id: "grok-build".into(),
            profile_id: second.id.clone(),
            enabled: false,
        })
        .unwrap();
        commit_asset_operation(AssetCommitRequest {
            operation_id: disable.operation_id,
            candidate_hash: disable.candidate_hash,
        })
        .unwrap();
        let disabled_stray = managed.replace("claude-shared", "disabled-child-customization");
        fs::write(&target, &disabled_stray).unwrap();

        provider.base_url = "https://new.example.test".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider),
                credential: None,
            },
        })
        .unwrap();
        assert!(plan.can_commit, "{:?}", plan.warnings);
        assert!(!plan
            .warnings
            .iter()
            .any(|warning| warning.contains(&format!("model:{}", second.id))));

        let inventory = commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();
        let updated = fs::read_to_string(target).unwrap();
        assert!(updated.contains("disabled-child-customization"));
        assert!(inventory.consumptions.iter().any(|item| {
            item.agent_id == "grok-build"
                && item.asset
                    == (AssetRef::Model {
                        profile_id: second.id.clone(),
                    })
                && item.enabled == Some(false)
                && item.status == ConsumptionStatus::ExternalChanged
                && item.reason.as_deref() == Some("model_disabled_state_drift")
        }));
        let profiles = load_settings_strict().unwrap().model_profiles.unwrap();
        assert_eq!(profiles[&second.id].base_url, "https://new.example.test");
    }

    #[test]
    fn provider_recovery_rolls_back_every_child_model_and_credential() {
        let _home = TestHome::new("consume-provider-keychain-rollback");
        let (mut provider, first, second) = install_shared_provider_models();
        let settings_before = fs::read(settings_file()).unwrap();
        provider.base_url = "https://new.example.test".into();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::ModelProvider {
                existing_id: Some(provider.id.clone()),
                provider: Box::new(provider.clone()),
                credential: Some("new-secret".into()),
            },
        })
        .unwrap();
        let persisted = load_operation(&plan.operation_id).unwrap();
        let snapshots = vec![PathSnapshot::capture(&settings_file()).unwrap()];
        let subject = provider_credential_subject(&provider.id);
        let old_credential = credential_snapshot(&subject).unwrap();
        persist_credential_rollback(&plan.operation_id, &subject, old_credential.as_deref())
            .unwrap();
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let tracked_paths = vec![settings_file()];
        let tracker = begin_transaction_write_tracking(
            &transaction_write_evidence_dir(&plan.operation_id),
            &tracked_paths,
            &parent_snapshots_for_snapshots(&snapshots),
        )
        .unwrap();
        let skills_guard = acquire_asset_skills_lock().unwrap();
        apply_operation(&persisted, &skills_guard, &BTreeSet::new()).unwrap();
        drop(tracker);
        drop(skills_guard);
        assert_eq!(credential_snapshot(&first.id).unwrap().unwrap(), b"new-secret");
        assert_eq!(credential_snapshot(&second.id).unwrap().unwrap(), b"new-secret");

        recover_pending_asset_operations().unwrap();
        assert_eq!(fs::read(settings_file()).unwrap(), settings_before);
        assert_eq!(credential_snapshot(&first.id).unwrap().unwrap(), b"old-secret");
        assert_eq!(credential_snapshot(&second.id).unwrap().unwrap(), b"old-secret");
        assert!(!operation_root(&plan.operation_id).exists());
    }

    #[test]
    fn model_recovery_finalizes_metadata_without_mutating_provider_credential() {
        let _home = TestHome::new("consume-model-keychain-finalize");
        save_profile(model("old-model"), Some("old-secret".into())).unwrap();
        let plan = plan_update_central_asset(PlanUpdateCentralAssetRequest {
            draft: CentralAssetDraft::Model {
                existing_id: Some("work".into()),
                profile: Box::new(model("new-model")),
                credential: None,
            },
        })
        .unwrap();
        let persisted = load_operation(&plan.operation_id).unwrap();
        let snapshots = vec![PathSnapshot::capture(&settings_file()).unwrap()];
        let old_credential = credential_snapshot("work").unwrap();
        persist_credential_rollback(&plan.operation_id, "work", old_credential.as_deref()).unwrap();
        persist_rollback_snapshots(&plan.operation_id, &snapshots).unwrap();
        let skills_guard = acquire_asset_skills_lock().unwrap();
        apply_operation(&persisted, &skills_guard, &BTreeSet::new()).unwrap();
        verify_operation(&persisted).unwrap();
        mark_operation_committed(&plan.operation_id).unwrap();
        drop(skills_guard);

        recover_pending_asset_operations().unwrap();
        assert_eq!(credential_snapshot("work").unwrap().unwrap(), b"old-secret");
        assert_eq!(
            load_settings_strict().unwrap().model_profiles.unwrap()["work"].model,
            "new-model"
        );
        assert!(!operation_root(&plan.operation_id).exists());
        assert!(credential_rollback_snapshot(&plan.operation_id, "work")
            .unwrap()
            .is_none());
    }
}
