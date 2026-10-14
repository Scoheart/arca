use super::files::validate_staging_candidate;
use super::inventory::{
    declared_targets_for_agents,
    normalize_agent_selection_with_required_target, normalize_assignment_enable,
};
use super::source::{load_staged_resolution, stage_private_candidate, stage_recorded_skill};
use super::staging::StagingRoot;
use super::transaction::{
    acquire_skills_lock, lexical_absolute, validate_operation_id,
    SkillsOperationLock,
};
use super::{
    audit_skill, diff_trees, execute_transaction, findings_digest, has_pending_recovery, hash_tree,
    io_error, list_inventory, normalize_agent_selection, validate_candidate, DirectoryMutation,
    FileChangeKind, InventoryState, LinkMutation, LinkState, ManagedSkillRecord, OperationPlan,
    PlanAssignmentRequest, PlanImportRequest, PlanInstallRequest, PlanRemoveRequest,
    PlanRepairRequest, PlanSkillAssetImportRequest, PlanSkillAssetInstallRequest,
    PlanUpdateRequest, PlannedLinkState, PlannedSkill, PlannedTarget, RepairKind, RiskLevel,
    SkillCommitRequest, SkillError, SkillFileChange, SkillManifest, SkillOperationKind,
    SkillSettingsSnapshot, SkillSource, SkillSourceResolution, SkillTargetView, SkillUpdateState,
    SkillsInventory, SkillsPaths, TransactionOrder, TransactionSpec,
};
use crate::settings::{load_settings_strict, Settings};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const PLAN_SCHEMA_VERSION: u32 = 1;
const PLAN_FILE: &str = "plan.json";
const MAX_PLAN_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PersistedPlan {
    schema_version: u32,
    plan: OperationPlan,
    input: PersistedPlanInput,
    expected_central: Vec<ExpectedCentral>,
    expected_links: Vec<ExpectedLink>,
    expected_target_roots: Vec<ExpectedTargetRoot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedPlanInput {
    Install {
        request: PlanInstallRequest,
        resolution: SkillSourceResolution,
    },
    Import {
        request: PlanImportRequest,
        source_target_id: String,
        source_name: String,
        original_path: String,
        backup_path: String,
    },
    Assignment {
        request: PlanAssignmentRequest,
    },
    Update {
        request: PlanUpdateRequest,
        resolution: SkillSourceResolution,
        backup_path: String,
    },
    Remove {
        request: PlanRemoveRequest,
        backup_path: String,
    },
    Repair {
        request: PlanRepairRequest,
        resolution: Option<SkillSourceResolution>,
        changed_source: bool,
        backup_path: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct ExpectedCentral {
    skill_name: String,
    content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ExpectedLink {
    skill_name: String,
    target_id: String,
    state: LinkState,
    desired_managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct ExpectedTargetRoot {
    target_id: String,
    root_path: String,
    anchor_path: String,
    anchor_device: u64,
    anchor_inode: u64,
    anchor_mode: u32,
    remaining_components: Vec<String>,
}

#[derive(Serialize)]
struct CandidateBinding<'a> {
    operation_id: &'a str,
    kind: &'a SkillOperationKind,
    skills: Vec<BoundSkill<'a>>,
    requested_agent_ids: Vec<&'a str>,
    target_ids: Vec<&'a str>,
    replace_conflicts: bool,
    assignment_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lifecycle: Option<LifecycleBinding<'a>>,
    expected_central: &'a [ExpectedCentral],
    expected_links: &'a [ExpectedLink],
    expected_target_roots: &'a [ExpectedTargetRoot],
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LifecycleBinding<'a> {
    Update {
        replace_local_changes: bool,
        backup_path: &'a str,
    },
    Remove {
        backup_path: &'a str,
    },
    Repair {
        repair: &'a RepairKind,
        changed_source: bool,
        backup_path: &'a Option<String>,
    },
}

#[derive(Serialize)]
struct BoundSkill<'a> {
    name: &'a str,
    existing_source: &'a Option<SkillSource>,
    source: &'a SkillSource,
    resolved_revision: &'a Option<String>,
    content_hash: &'a str,
    replace_existing: bool,
}

pub fn plan_install(request: PlanInstallRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_install_inner(request))
}

/// Central asset intake never accepts Agent ids. Assignment is a separate
/// consumption operation after the asset exists in `managed_skills`.
pub fn plan_asset_install(
    request: PlanSkillAssetInstallRequest,
) -> Result<OperationPlan, SkillError> {
    plan_install(PlanInstallRequest {
        resolution_id: request.resolution_id,
        skill_names: request.skill_names,
        agent_ids: Vec::new(),
        replace_conflicts: request.replace_conflicts,
    })
}

fn plan_install_inner(request: PlanInstallRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    validate_operation_id(&request.resolution_id)?;
    let paths = SkillsPaths::resolve_from_env()?;
    let resolution = load_staged_resolution(&paths, &request.resolution_id)?;
    let persisted = build_install_plan(request, resolution)?;
    persist_plan(&paths, &persisted)?;
    Ok(persisted.plan)
}

pub fn commit_install(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Install))
}

pub fn plan_import(request: PlanImportRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_import_inner(request))
}

pub fn plan_asset_import(
    request: PlanSkillAssetImportRequest,
) -> Result<OperationPlan, SkillError> {
    plan_import(PlanImportRequest {
        identity: request.identity,
        agent_ids: Vec::new(),
        replace_conflicts: request.replace_conflicts,
    })
}

fn plan_import_inner(request: PlanImportRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    let paths = SkillsPaths::resolve_from_env()?;
    let inventory = list_inventory()?;
    let external = external_item(&paths, &inventory, &request.identity)?;
    let operation_id = Uuid::new_v4().hyphenated().to_string();
    create_operation_root(&paths, &operation_id)?;
    let result = (|| {
        let operation = StagingRoot::open(&paths)?.open_operation(&operation_id)?;
        let candidates = operation.create_private_directory("candidates")?;
        let staged = candidates.create_directory(&external.name)?;
        let before_hash = hash_tree(&external.path)?;
        stage_private_candidate(&external.path, &staged)?;
        let staged_hash = validate_staging_candidate(&staged)?.content_hash;
        if hash_tree(&external.path)? != before_hash || staged_hash != before_hash {
            return Err(SkillError::PlanStale {
                message: "the external Skill changed while it was staged".into(),
            });
        }
        let backup = paths.backups_skills_dir().join(format!(
            "import-{}-{}",
            crate::paths::backup_timestamp(),
            &operation_id[..8]
        ));
        let backup = backup.join(&external.name);
        let persisted = build_import_plan(
            request,
            operation_id.clone(),
            external.target_id,
            external.name,
            collapse_home(&external.path, paths.user_home()),
            collapse_home(&backup, paths.user_home()),
        )?;
        persist_plan(&paths, &persisted)?;
        Ok(persisted.plan)
    })();
    if result.is_err() {
        remove_unjournaled_operation(&paths, &operation_id)?;
    }
    result
}

pub fn commit_import(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Import))
}

pub fn plan_assignment(request: PlanAssignmentRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_assignment_inner(request))
}

fn plan_assignment_inner(request: PlanAssignmentRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    let paths = SkillsPaths::resolve_from_env()?;
    let operation_id = Uuid::new_v4().hyphenated().to_string();
    create_operation_root(&paths, &operation_id)?;
    let result = (|| {
        let persisted = build_assignment_plan(request, operation_id.clone())?;
        persist_plan(&paths, &persisted)?;
        Ok(persisted.plan)
    })();
    if result.is_err() {
        remove_unjournaled_operation(&paths, &operation_id)?;
    }
    result
}

pub fn commit_assignment(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Assignment))
}

fn apply_asset_link_mutations(mutations: &[LinkMutation]) -> Result<Vec<PathBuf>, SkillError> {
    let mut changed = Vec::new();
    for mutation in mutations {
        if mutation.backup.is_some() || matches!(mutation.expected, LinkState::Directory { .. }) {
            return Err(invalid_source_error(
                "an Asset relationship cannot replace a Skill directory",
            ));
        }
        let did_change = crate::safe_write::set_transaction_symlink(
            &mutation.path,
            mutation.desired_target.as_deref(),
        )
        .map_err(|message| SkillError::Io {
            message: super::capped_message(message),
            path: None,
        })?;
        if did_change {
            changed.push(mutation.path.clone());
        }
    }
    Ok(changed)
}

/// Release reviewed physical-target ownership without deleting external data.
/// The outer asset operation supplies hash-bound target ids. This transaction
/// removes only a link that resolves to the exact central Skill path; all
/// ordinary files/directories and other links remain untouched and become external after
/// settings ownership is cleared. The same path handles clean and orphaned
/// unassigns, so drift cannot turn a reviewable removal into a commit-time trap.
pub(crate) fn release_assignment_links_safely(
    skill_name: &str,
    reviewed_target: &SkillTargetView,
    skills_lock: &SkillsOperationLock,
) -> Result<Vec<PathBuf>, SkillError> {
    ensure_recovery_clear()?;
    let paths = SkillsPaths::resolve_from_env()?;
    let central = paths.central_skill(skill_name);
    // Authority was already removed. Reuse the target captured by the outer
    // reviewed transaction: an uninstalled Agent's final target disappears
    // from the new inventory immediately after its last assignment is removed.
    let path = target_path(&paths, reviewed_target, skill_name)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&path, error)),
    };
    if !metadata.file_type().is_symlink() {
        return Ok(Vec::new());
    }
    let raw_target = fs::read_link(&path).map_err(|error| io_error(&path, error))?;
    let resolved = match fs::canonicalize(&path) {
        Ok(resolved) => resolved,
        Err(error) if error.kind() == ErrorKind::NotFound || is_symlink_loop(&error) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(io_error(&path, error)),
    };
    if fs::canonicalize(&central).ok().as_ref() != Some(&resolved) {
        return Ok(Vec::new());
    }
    let expected = match fs::metadata(&path) {
        Ok(_) => LinkState::ManagedSymlink { target: raw_target },
        Err(error) if error.kind() == ErrorKind::NotFound || is_symlink_loop(&error) => {
            LinkState::BrokenSymlink { target: raw_target }
        }
        Err(error) => return Err(io_error(&path, error)),
    };
    let link_mutations = [LinkMutation {
        path,
        expected,
        desired_target: None,
        backup: None,
    }];

    let _ = skills_lock;
    apply_asset_link_mutations(&link_mutations)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssignmentReapplyState {
    Synced,
    Repairable,
}

/// Inspect one already-desired, enabled Skill target without weakening link
/// ownership. An enabled relationship may recreate an absent entry or an
/// exact broken managed link; a disabled relationship may remove only the
/// exact managed link. Directories, ordinary files, and links to any other
/// destination remain foreign even when settings still declare the relation.
pub(crate) fn assignment_reapply_state(
    skill_name: &str,
    target_id: &str,
) -> Result<AssignmentReapplyState, SkillError> {
    let (_, path, central, state, enabled) = assignment_reapply_context(skill_name, target_id)?;
    match state {
        LinkState::ManagedSymlink { .. } if enabled => Ok(AssignmentReapplyState::Synced),
        LinkState::Missing if !enabled => Ok(AssignmentReapplyState::Synced),
        LinkState::ManagedSymlink { .. } | LinkState::Missing => {
            Ok(AssignmentReapplyState::Repairable)
        }
        LinkState::BrokenSymlink { ref target }
            if broken_link_targets_central(target, &path, &central)? =>
        {
            Ok(AssignmentReapplyState::Repairable)
        }
        LinkState::BrokenSymlink { .. }
        | LinkState::Directory { .. }
        | LinkState::UnknownSymlink { .. } => {
            conflict_result("reapply would replace a foreign Agent Skill target")
        }
    }
}

/// Recreate one reviewed Skill link while preserving foreign data. The outer
/// asset transaction binds settings and target hashes; this inner Skills
/// transaction rechecks the exact link state under the Skills lock.
pub(crate) fn reapply_assignment_safely(
    skill_name: &str,
    target_id: &str,
    skills_lock: &SkillsOperationLock,
) -> Result<Vec<PathBuf>, SkillError> {
    let (settings, path, central, state, enabled) =
        assignment_reapply_context(skill_name, target_id)?;
    let repairable = match &state {
        LinkState::ManagedSymlink { .. } if enabled => return Ok(Vec::new()),
        LinkState::Missing if !enabled => return Ok(Vec::new()),
        LinkState::ManagedSymlink { .. } | LinkState::Missing => true,
        LinkState::BrokenSymlink { target } => {
            broken_link_targets_central(target, &path, &central)?
        }
        LinkState::Directory { .. } | LinkState::UnknownSymlink { .. } => false,
    };
    if !repairable {
        return conflict_result("reapply would replace a foreign Agent Skill target");
    }

    let spec = TransactionSpec {
        operation_id: Uuid::new_v4().hyphenated().to_string(),
        order: TransactionOrder::ContentThenLinks,
        directory_mutations: Vec::new(),
        link_mutations: vec![LinkMutation {
            path: path.clone(),
            expected: state,
            desired_target: enabled.then_some(central),
            backup: None,
        }],
        settings_before: settings.clone(),
        settings_after: settings,
    };
    let _ = skills_lock;
    apply_asset_link_mutations(&spec.link_mutations)
}

fn assignment_reapply_context(
    skill_name: &str,
    target_id: &str,
) -> Result<(SkillSettingsSnapshot, PathBuf, PathBuf, LinkState, bool), SkillError> {
    ensure_recovery_clear()?;
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let record = managed_record(&settings, skill_name)?;
    let assigned = settings
        .skill_assignments
        .as_ref()
        .and_then(|assignments| assignments.get(skill_name))
        .is_some_and(|targets| targets.contains(target_id));
    if !assigned {
        return conflict_result("only an assigned managed Skill target can be reapplied");
    }
    let enabled = settings
        .skill_consumptions
        .as_ref()
        .and_then(|skills| skills.get(skill_name))
        .and_then(|targets| targets.get(target_id))
        .map(|record| record.enabled)
        .unwrap_or(true);
    let central = paths.central_skill(skill_name);
    if enabled {
        let validated = validate_candidate(&central)
            .map_err(|_| conflict_error("the managed central Skill cannot authorize reapply"))?;
        if validated.manifest.name != skill_name || validated.content_hash != record.content_hash {
            return conflict_result("the managed central Skill hash cannot authorize reapply");
        }
    }
    let inventory = list_inventory()?;
    let target = inventory
        .targets
        .iter()
        .find(|target| target.target_id == target_id)
        .ok_or_else(|| invalid_source_error("the assigned Skill target is unavailable"))?;
    let path = target_path(&paths, target, skill_name)?;
    let state = inspect_link(&path, &central, &paths)?;
    Ok((settings, path, central, state, enabled))
}

fn broken_link_targets_central(
    raw_target: &Path,
    path: &Path,
    central: &Path,
) -> Result<bool, SkillError> {
    let resolved = if raw_target.is_absolute() {
        lexical_absolute(raw_target)?
    } else {
        lexical_absolute(
            &path
                .parent()
                .ok_or_else(|| invalid_source_error("a Skill link has no target parent"))?
                .join(raw_target),
        )?
    };
    Ok(resolved == lexical_absolute(central)?)
}

pub fn plan_update(request: PlanUpdateRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_update_inner(request))
}

pub fn commit_update(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Update))
}

pub fn plan_remove(request: PlanRemoveRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_remove_inner(request))
}

pub fn commit_remove(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Remove))
}

pub fn plan_repair(request: PlanRepairRequest) -> Result<OperationPlan, SkillError> {
    sanitize_result(plan_repair_inner(request))
}

pub fn commit_repair(request: SkillCommitRequest) -> Result<SkillsInventory, SkillError> {
    sanitize_result(commit_plan(request, SkillOperationKind::Repair))
}

fn plan_update_inner(request: PlanUpdateRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    let settings = current_settings_snapshot()?;
    let record = managed_record(&settings, &request.skill_name)?.clone();
    let revision = match &record.source {
        SkillSource::Github { pinned: true, .. } | SkillSource::Imported { .. } => {
            return conflict_result("this pinned Skill source does not have ordinary updates")
        }
        SkillSource::Github { .. } => {
            if !record.update.available {
                return conflict_result("no reviewed GitHub update is available for this Skill");
            }
            record.update.resolved_revision.as_deref().ok_or_else(|| {
                invalid_source_error("the reviewed GitHub update revision is unavailable")
            })?
        }
        SkillSource::Local { .. } | SkillSource::Archive { .. } => "",
    };
    let resolution = stage_recorded_skill(
        &record.source,
        (!revision.is_empty()).then_some(revision),
        &request.skill_name,
        super::GithubEndpoints::production(),
    )?;
    let paths = SkillsPaths::resolve_from_env()?;
    let operation_id = resolution.operation_id.clone();
    let backup_path = paths
        .backups_skills_dir()
        .join(format!(
            "update-{}-{}",
            crate::paths::backup_timestamp(),
            &operation_id[..8]
        ))
        .join(&request.skill_name);
    let result = (|| {
        let persisted = build_update_plan(
            request,
            resolution,
            collapse_home(&backup_path, paths.user_home()),
        )?;
        persist_plan(&paths, &persisted)?;
        Ok(persisted.plan)
    })();
    if result.is_err() {
        remove_unjournaled_operation(&paths, &operation_id)?;
    }
    result
}

fn plan_remove_inner(request: PlanRemoveRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    let paths = SkillsPaths::resolve_from_env()?;
    let operation_id = Uuid::new_v4().hyphenated().to_string();
    create_operation_root(&paths, &operation_id)?;
    let backup_path = paths
        .backups_skills_dir()
        .join(format!(
            "remove-{}-{}",
            crate::paths::backup_timestamp(),
            &operation_id[..8]
        ))
        .join(&request.skill_name);
    let result = (|| {
        let persisted = build_remove_plan(
            request,
            operation_id.clone(),
            collapse_home(&backup_path, paths.user_home()),
        )?;
        persist_plan(&paths, &persisted)?;
        Ok(persisted.plan)
    })();
    if result.is_err() {
        remove_unjournaled_operation(&paths, &operation_id)?;
    }
    result
}

fn plan_repair_inner(request: PlanRepairRequest) -> Result<OperationPlan, SkillError> {
    ensure_recovery_clear()?;
    match &request.repair {
        RepairKind::Target { .. } => {
            let paths = SkillsPaths::resolve_from_env()?;
            let operation_id = Uuid::new_v4().hyphenated().to_string();
            create_operation_root(&paths, &operation_id)?;
            let result = (|| {
                let persisted = build_target_repair_plan(request, operation_id.clone())?;
                persist_plan(&paths, &persisted)?;
                Ok(persisted.plan)
            })();
            if result.is_err() {
                remove_unjournaled_operation(&paths, &operation_id)?;
            }
            result
        }
        RepairKind::Central => {
            let settings = current_settings_snapshot()?;
            let record = managed_record(&settings, &request.skill_name)?.clone();
            let revision = match &record.source {
                SkillSource::Github { .. } => record.resolved_revision.as_deref(),
                SkillSource::Local { .. }
                | SkillSource::Archive { .. }
                | SkillSource::Imported { .. } => None,
            };
            let resolution = stage_recorded_skill(
                &record.source,
                revision,
                &request.skill_name,
                super::GithubEndpoints::production(),
            )?;
            let paths = SkillsPaths::resolve_from_env()?;
            let operation_id = resolution.operation_id.clone();
            let backup_path = paths
                .backups_skills_dir()
                .join(format!(
                    "repair-{}-{}",
                    crate::paths::backup_timestamp(),
                    &operation_id[..8]
                ))
                .join(&request.skill_name);
            let result = (|| {
                let persisted = build_central_repair_plan(
                    request,
                    resolution,
                    collapse_home(&backup_path, paths.user_home()),
                    false,
                )?;
                persist_plan(&paths, &persisted)?;
                Ok(persisted.plan)
            })();
            if result.is_err() {
                remove_unjournaled_operation(&paths, &operation_id)?;
            }
            result
        }
    }
}

pub fn cancel_operation(operation_id: &str) -> Result<(), SkillError> {
    sanitize_result(cancel_operation_inner(operation_id))
}

fn cancel_operation_inner(operation_id: &str) -> Result<(), SkillError> {
    validate_operation_id(operation_id)?;
    let paths = SkillsPaths::resolve_from_env()?;
    let _lock = acquire_skills_lock(&paths)?;
    cancel_operation_under_lock(&paths, operation_id)
}

fn cancel_operation_under_lock(paths: &SkillsPaths, operation_id: &str) -> Result<(), SkillError> {
    let journal = paths
        .journals_skills_dir()
        .join(format!("{operation_id}.json"));
    match fs::symlink_metadata(&journal) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&journal, error)),
        Ok(_) => {
            return Err(SkillError::RecoveryRequired {
                message: "an active Skills journal prevents cancellation".into(),
            })
        }
    }
    StagingRoot::open(paths)?
        .remove_operation_if_exists(operation_id)
        .map(|_| ())
}

fn build_install_plan(
    request: PlanInstallRequest,
    resolution: SkillSourceResolution,
) -> Result<PersistedPlan, SkillError> {
    if resolution.operation_id != request.resolution_id {
        return invalid_source("the staged resolution does not match the requested operation");
    }
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let operation = StagingRoot::open(&paths)?.open_operation(&resolution.operation_id)?;
    let staged_candidates = operation.root_directory()?.open_directory("candidates")?;
    let selected_names = selected_skill_names(&request.skill_names)?;
    let candidate_summaries = resolution
        .candidates
        .iter()
        .map(|candidate| (candidate.name.as_str(), candidate))
        .collect::<BTreeMap<_, _>>();
    for name in &selected_names {
        if !candidate_summaries.contains_key(name.as_str()) {
            return invalid_source("a selected Skill is not part of the staged resolution");
        }
    }
    let desired_target_ids = normalize_agent_selection(&request.agent_ids)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    // Central replacement changes content, not existing consumption. Only
    // explicitly requested new targets belong to this intake's write set.
    let mut target_views = selected_target_views(
        &inventory,
        &desired_target_ids.iter().cloned().collect::<Vec<_>>(),
    )?;
    let mut expected_central = Vec::new();
    let mut expected_links = Vec::new();
    let mut skills = Vec::new();

    for name in selected_names {
        let summary = candidate_summaries[&name.as_str()];
        let candidate = staged_candidates.open_directory(&name)?;
        let validated = validate_staging_candidate(&candidate)?;
        if validated.content_hash != summary.content_hash || validated.manifest.name != summary.name
        {
            return stale("a staged Skill candidate changed after resolution");
        }
        let central = paths.central_skill(&name);
        let central_hash = inspect_central(&central)?;
        if central_hash.is_some() && !request.replace_conflicts {
            return conflict("central Skill content already exists");
        }
        expected_central.push(ExpectedCentral {
            skill_name: name.clone(),
            content_hash: central_hash.clone(),
        });
        for target in &target_views {
            if target_consumption_is_disabled(&settings, &name, &target.target_id) {
                continue;
            }
            let state = inspect_link(&target_path(&paths, target, &name)?, &central, &paths)?;
            let desired_managed = true;
            if is_link_conflict(&state) {
                return conflict("an Agent Skill target conflicts with this install");
            }
            expected_links.push(ExpectedLink {
                skill_name: name.clone(),
                target_id: target.target_id.clone(),
                state,
                desired_managed,
            });
        }
        let source = candidate_source(&resolution.source, &summary.relative_path);
        let risk = audit_skill(candidate.path())?;
        let files = diff_trees(
            central_hash.as_ref().map(|_| central.as_path()),
            candidate.path(),
        )?;
        skills.push(PlannedSkill {
            manifest: validated.manifest,
            existing_source: central_hash
                .as_ref()
                .and_then(|_| managed_source(&settings, &name)),
            source,
            resolved_revision: resolution.resolved_revision.clone(),
            files,
            risk,
            existing_states: existing_states(&inventory, &name),
            replace_existing: central_hash.is_some(),
            content_hash: validated.content_hash,
        });
    }

    target_views.retain(|target| expected_links.iter().any(|link| link.target_id == target.target_id));
    let targets = planned_targets(&target_views, &expected_links);
    let warnings = plan_warnings(&targets, &request.agent_ids);
    let plan = new_plan(
        resolution.operation_id.clone(),
        SkillOperationKind::Install,
        skills,
        targets,
        settings_hash(&settings)?,
        warnings,
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Install {
            request,
            resolution,
        },
        expected_central,
        expected_links,
        expected_target_roots: Vec::new(),
    })
}

#[derive(Debug)]
struct ExternalItem {
    name: String,
    target_id: String,
    path: PathBuf,
}

fn external_item(
    paths: &SkillsPaths,
    inventory: &SkillsInventory,
    identity: &str,
) -> Result<ExternalItem, SkillError> {
    let item = inventory
        .items
        .iter()
        .find(|item| item.identity == identity)
        .ok_or_else(|| invalid_source_error("the selected external Skill is unavailable"))?;
    if !item.states.contains(&InventoryState::External) {
        return conflict_result("only an external Agent Skill directory can be imported");
    }
    let super::SkillLocation::AgentTarget {
        target_id,
        global_dir,
    } = &item.location
    else {
        return conflict_result("only an external Agent Skill directory can be imported");
    };
    let root = paths
        .expand_user(global_dir)
        .ok_or_else(|| invalid_source_error("the selected Agent target is unavailable"))?;
    let path = root.join(&item.name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| invalid_source_error("the selected external Skill is unavailable"))?;
    if !metadata.file_type().is_dir() {
        return conflict_result("only a real external Skill directory can be imported");
    }
    validate_candidate(&path)?;
    Ok(ExternalItem {
        name: item.name.clone(),
        target_id: target_id.clone(),
        path,
    })
}

fn build_import_plan(
    request: PlanImportRequest,
    operation_id: String,
    source_target_id: String,
    source_name: String,
    original_path: String,
    backup_path: String,
) -> Result<PersistedPlan, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let operation = StagingRoot::open(&paths)?.open_operation(&operation_id)?;
    let external = external_item(&paths, &inventory, &request.identity)?;
    if external.name != source_name || external.target_id != source_target_id {
        return stale("the selected external Skill moved after review");
    }
    if collapse_home(&external.path, paths.user_home()) != original_path {
        return stale("the selected external Skill path changed after review");
    }
    let staged = operation
        .root_directory()?
        .open_directory("candidates")?
        .open_directory(&source_name)?;
    let validated = validate_staging_candidate(&staged)?;
    if hash_tree(&external.path)? != validated.content_hash {
        return stale("the external Skill changed after review");
    }
    let desired_target_ids =
        normalize_agent_selection_with_required_target(&request.agent_ids, &source_target_id)?
            .into_iter()
            .collect::<BTreeSet<_>>();
    let mut target_views = selected_target_views(
        &inventory,
        &desired_target_ids.iter().cloned().collect::<Vec<_>>(),
    )?;
    let central = paths.central_skill(&source_name);
    let central_hash = inspect_central(&central)?;
    if central_hash.is_some() && !request.replace_conflicts {
        return conflict_result("central Skill content already exists");
    }
    let mut expected_links = Vec::new();
    for target in &target_views {
        let is_source = target.target_id == source_target_id;
        let disabled = target_consumption_is_disabled(&settings, &source_name, &target.target_id);
        if disabled && !is_source { continue; }
        let state = inspect_link(
            &target_path(&paths, target, &source_name)?,
            &central,
            &paths,
        )?;
        let desired_managed = !disabled;
        if is_source && !matches!(state, LinkState::Directory { .. }) {
            return stale("the selected external Skill changed type after review");
        }
        if desired_managed && !is_source && is_link_conflict(&state) {
            let identical_external = matches!(
                &state,
                LinkState::Directory { tree_hash } if tree_hash == &validated.content_hash
            );
            if !identical_external {
                return conflict_result("an Agent Skill target conflicts with this import");
            }
        }
        expected_links.push(ExpectedLink {
            skill_name: source_name.clone(),
            target_id: target.target_id.clone(),
            state,
            desired_managed,
        });
    }
    let source = SkillSource::Imported {
        original_path: original_path.clone(),
        backup_path: backup_path.clone(),
    };
    let risk = audit_skill(staged.path())?;
    let files = diff_trees(
        central_hash.as_ref().map(|_| central.as_path()),
        staged.path(),
    )?;
    let skill = PlannedSkill {
        manifest: validated.manifest,
        existing_source: central_hash
            .as_ref()
            .and_then(|_| managed_source(&settings, &source_name)),
        source,
        resolved_revision: None,
        files,
        risk,
        existing_states: existing_states(&inventory, &source_name),
        replace_existing: central_hash.is_some(),
        content_hash: validated.content_hash,
    };
    target_views.retain(|target| expected_links.iter().any(|link| link.target_id == target.target_id));
    let targets = planned_targets(&target_views, &expected_links);
    let warnings = plan_warnings(&targets, &request.agent_ids);
    let plan = new_plan(
        operation_id,
        SkillOperationKind::Import,
        vec![skill],
        targets,
        settings_hash(&settings)?,
        warnings,
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Import {
            request,
            source_target_id,
            source_name: source_name.clone(),
            original_path,
            backup_path,
        },
        expected_central: vec![ExpectedCentral {
            skill_name: source_name,
            content_hash: central_hash,
        }],
        expected_links,
        expected_target_roots: Vec::new(),
    })
}

fn build_assignment_plan(
    request: PlanAssignmentRequest,
    operation_id: String,
) -> Result<PersistedPlan, SkillError> {
    if request.agent_ids.is_empty() {
        return invalid_source("an assignment requires at least one installed Agent");
    }
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let records = settings
        .managed_skills
        .as_ref()
        .ok_or_else(|| invalid_source_error("the managed Skill is unavailable"))?;
    let record = records
        .get(&request.skill_name)
        .ok_or_else(|| invalid_source_error("the managed Skill is unavailable"))?;
    let central = paths.central_skill(&request.skill_name);
    let validated = validate_candidate(&central)?;
    if validated.content_hash != record.content_hash {
        return conflict_result("the managed Skill was locally modified");
    }
    let prior_target_ids = known_assigned_target_ids(&settings, &inventory, &request.skill_name);
    let desired_target_ids = if request.enabled {
        normalize_assignment_enable(&request.agent_ids, &prior_target_ids)?
            .into_iter()
            .collect::<BTreeSet<_>>()
    } else {
        let removed = assigned_targets_for_agents(&prior_target_ids, &request)?;
        prior_target_ids
            .difference(&removed)
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let touched_target_ids = prior_target_ids
        .union(&desired_target_ids)
        .cloned()
        .collect::<Vec<_>>();
    let target_views = selected_target_views(&inventory, &touched_target_ids)?;
    let mut expected_links = Vec::new();
    for target in &target_views {
        let link_path = target_path(&paths, target, &request.skill_name)?;
        let state = inspect_link(&link_path, &central, &paths)?;
        let desired_managed = desired_target_ids.contains(&target.target_id);
        if desired_managed && is_link_conflict(&state) {
            return conflict_result("assignment would overwrite an unreviewed Agent Skill target");
        }
        if !desired_managed && !is_safe_absent_transition(&state) {
            return disable_conflict(&state, &link_path, paths.user_home());
        }
        expected_links.push(ExpectedLink {
            skill_name: request.skill_name.clone(),
            target_id: target.target_id.clone(),
            state,
            desired_managed,
        });
    }
    let risk = audit_skill(&central)?;
    let skill = PlannedSkill {
        manifest: validated.manifest,
        existing_source: None,
        source: record.source.clone(),
        resolved_revision: record.resolved_revision.clone(),
        files: Vec::new(),
        risk,
        existing_states: existing_states(&inventory, &request.skill_name),
        replace_existing: false,
        content_hash: validated.content_hash,
    };
    let targets = planned_targets(&target_views, &expected_links);
    let warnings = plan_warnings(&targets, &request.agent_ids);
    let plan = new_plan(
        operation_id,
        SkillOperationKind::Assignment,
        vec![skill],
        targets,
        settings_hash(&settings)?,
        warnings,
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Assignment { request },
        expected_central: vec![ExpectedCentral {
            skill_name: record.name.clone(),
            content_hash: Some(record.content_hash.clone()),
        }],
        expected_links,
        expected_target_roots: Vec::new(),
    })
}

fn build_update_plan(
    request: PlanUpdateRequest,
    resolution: SkillSourceResolution,
    backup_path: String,
) -> Result<PersistedPlan, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let record = managed_record(&settings, &request.skill_name)?;
    if resolution.source != record.source || resolution.candidates.len() != 1 {
        return stale("the recorded Skill source changed while its update was staged");
    }
    if matches!(record.source, SkillSource::Github { .. }) {
        if !record.update.available
            || record.update.resolved_revision != resolution.resolved_revision
        {
            return stale("the reviewed GitHub update changed while it was staged");
        }
        if resolution.resolved_revision == record.resolved_revision {
            return conflict_result("the staged GitHub revision is already installed");
        }
    }
    let summary = &resolution.candidates[0];
    if summary.name != request.skill_name {
        return stale("the staged update does not contain the reviewed named Skill");
    }
    let operation = StagingRoot::open(&paths)?.open_operation(&resolution.operation_id)?;
    let candidate = operation
        .root_directory()?
        .open_directory("candidates")?
        .open_directory(&request.skill_name)?;
    let validated = validate_staging_candidate(&candidate)?;
    if validated.manifest.name != request.skill_name
        || validated.content_hash != summary.content_hash
    {
        return stale("the staged update changed before review");
    }
    let staged_name = summary.name.clone();
    let central = paths.central_skill(&request.skill_name);
    let central_hash = inspect_central(&central)?
        .ok_or_else(|| conflict_error("the managed central Skill is missing"))?;
    if central_hash != record.content_hash && !request.replace_local_changes {
        return conflict_result(
            "the managed Skill has local changes that require explicit replacement",
        );
    }
    let content_unchanged = validated.content_hash == central_hash;
    if content_unchanged && !matches!(record.source, SkillSource::Github { .. }) {
        return conflict_result("the staged Skill content is already installed");
    }
    let risk = audit_skill(candidate.path())?;
    let files = diff_trees(Some(&central), candidate.path())?;
    let skill = PlannedSkill {
        manifest: validated.manifest,
        existing_source: Some(record.source.clone()),
        source: record.source.clone(),
        resolved_revision: resolution.resolved_revision.clone(),
        files,
        risk,
        existing_states: existing_states(&inventory, &request.skill_name),
        replace_existing: !content_unchanged,
        content_hash: validated.content_hash,
    };
    let plan = new_plan(
        resolution.operation_id.clone(),
        SkillOperationKind::Update,
        vec![skill],
        Vec::new(),
        settings_hash(&settings)?,
        if content_unchanged { vec!["Source revision changed; Skill content is unchanged. Only the revision record will be updated.".into()] } else { Vec::new() },
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Update {
            request,
            resolution,
            backup_path,
        },
        expected_central: vec![ExpectedCentral {
            skill_name: staged_name,
            content_hash: Some(central_hash),
        }],
        expected_links: Vec::new(),
        expected_target_roots: Vec::new(),
    })
}

fn build_remove_plan(
    request: PlanRemoveRequest,
    operation_id: String,
    backup_path: String,
) -> Result<PersistedPlan, SkillError> {
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let record = managed_record(&settings, &request.skill_name)?;
    let central = paths.central_skill(&request.skill_name);
    let central_hash = inspect_central(&central)?;
    let (manifest, risk, content_hash, files) = if let Some(actual_hash) = &central_hash {
        let (manifest, risk) = match validate_candidate(&central) {
            Ok(validated) if validated.manifest.name == request.skill_name => {
                (validated.manifest, audit_skill(&central)?)
            }
            Err(SkillError::InvalidManifest { .. }) | Ok(_) => {
                (manifest_from_record(record), record.risk.clone())
            }
            Err(error) => return Err(error),
        };
        (
            manifest,
            risk,
            actual_hash.clone(),
            removed_file_changes(&central)?,
        )
    } else {
        (
            manifest_from_record(record),
            record.risk.clone(),
            record.content_hash.clone(),
            Vec::new(),
        )
    };
    let mut expected_links = Vec::new();
    let mut target_views = Vec::new();
    for target in &inventory.targets {
        let path = target_path(&paths, target, &request.skill_name)?;
        let state = match inspect_link(&path, &central, &paths) {
            Ok(state) => state,
            Err(SkillError::Conflict { .. }) => continue,
            Err(error) => return Err(error),
        };
        let exact = match &state {
            LinkState::ManagedSymlink { .. } => true,
            LinkState::BrokenSymlink { target } => broken_link_targets_central(target, &path, &central)?,
            _ => false,
        };
        if exact {
            target_views.push(target.clone());
            expected_links.push(ExpectedLink {
                skill_name: request.skill_name.clone(),
                target_id: target.target_id.clone(),
                state,
                desired_managed: false,
            });
        }
    }
    let targets = planned_targets(&target_views, &expected_links);
    let skill = PlannedSkill {
        manifest,
        existing_source: None,
        source: record.source.clone(),
        resolved_revision: record.resolved_revision.clone(),
        files,
        risk,
        existing_states: existing_states(&inventory, &request.skill_name),
        replace_existing: central_hash.is_some() || !expected_links.is_empty(),
        content_hash,
    };
    let plan = new_plan(
        operation_id,
        SkillOperationKind::Remove,
        vec![skill],
        targets,
        settings_hash(&settings)?,
        Vec::new(),
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Remove {
            request: request.clone(),
            backup_path,
        },
        expected_central: vec![ExpectedCentral {
            skill_name: request.skill_name,
            content_hash: central_hash,
        }],
        expected_links,
        expected_target_roots: Vec::new(),
    })
}

fn build_target_repair_plan(
    request: PlanRepairRequest,
    operation_id: String,
) -> Result<PersistedPlan, SkillError> {
    let RepairKind::Target { target_id } = &request.repair else {
        return invalid_source("a target repair requires a target id");
    };
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let record = managed_record(&settings, &request.skill_name)?;
    let assigned = settings
        .skill_assignments
        .as_ref()
        .and_then(|rows| rows.get(&request.skill_name))
        .is_some_and(|targets| targets.contains(target_id));
    if !assigned {
        return conflict_result("only an assigned managed Skill target can be repaired");
    }
    if settings
        .skill_consumptions
        .as_ref()
        .and_then(|skills| skills.get(&request.skill_name))
        .and_then(|targets| targets.get(target_id))
        .is_some_and(|record| !record.enabled)
    {
        return conflict_result("a disabled managed Skill target does not require repair");
    }
    let central = paths.central_skill(&request.skill_name);
    let validated = validate_candidate(&central)
        .map_err(|_| conflict_error("the managed central Skill cannot authorize target repair"))?;
    if validated.manifest.name != request.skill_name
        || validated.content_hash != record.content_hash
    {
        return conflict_result("the managed central Skill hash cannot authorize target repair");
    }
    let target = inventory
        .targets
        .iter()
        .find(|target| &target.target_id == target_id)
        .ok_or_else(|| invalid_source_error("the assigned Skill target is unavailable"))?
        .clone();
    let state = inspect_link(
        &target_path(&paths, &target, &request.skill_name)?,
        &central,
        &paths,
    )?;
    let repairable = matches!(state, LinkState::Missing)
        || matches!(&state, LinkState::BrokenSymlink { target } if target == &central);
    if !repairable {
        return conflict_result("the assigned Skill target is not an authorized broken link");
    }
    let expected_links = vec![ExpectedLink {
        skill_name: request.skill_name.clone(),
        target_id: target.target_id.clone(),
        state,
        desired_managed: true,
    }];
    let skill = PlannedSkill {
        manifest: validated.manifest,
        existing_source: None,
        source: record.source.clone(),
        resolved_revision: record.resolved_revision.clone(),
        files: Vec::new(),
        risk: audit_skill(&central)?,
        existing_states: existing_states(&inventory, &request.skill_name),
        replace_existing: false,
        content_hash: validated.content_hash,
    };
    let plan = new_plan(
        operation_id,
        SkillOperationKind::Repair,
        vec![skill],
        planned_targets(&[target], &expected_links),
        settings_hash(&settings)?,
        Vec::new(),
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Repair {
            request: request.clone(),
            resolution: None,
            changed_source: false,
            backup_path: None,
        },
        expected_central: vec![ExpectedCentral {
            skill_name: request.skill_name,
            content_hash: Some(record.content_hash.clone()),
        }],
        expected_links,
        expected_target_roots: Vec::new(),
    })
}

fn build_central_repair_plan(
    request: PlanRepairRequest,
    resolution: SkillSourceResolution,
    backup_path: String,
    allow_reappeared: bool,
) -> Result<PersistedPlan, SkillError> {
    if request.repair != RepairKind::Central {
        return invalid_source("a central repair cannot stage a target repair");
    }
    let paths = SkillsPaths::resolve_from_env()?;
    let settings = current_settings_snapshot()?;
    let inventory = list_inventory()?;
    let record = managed_record(&settings, &request.skill_name)?;
    if resolution.source != record.source || resolution.candidates.len() != 1 {
        return stale("the recorded source changed while central repair was staged");
    }
    if matches!(record.source, SkillSource::Github { .. })
        && resolution.resolved_revision != record.resolved_revision
    {
        return stale("central repair did not use the recorded immutable revision");
    }
    let summary = &resolution.candidates[0];
    if summary.name != request.skill_name {
        return stale("central repair staged a different named Skill");
    }
    let operation = StagingRoot::open(&paths)?.open_operation(&resolution.operation_id)?;
    let candidate = operation
        .root_directory()?
        .open_directory("candidates")?
        .open_directory(&request.skill_name)?;
    let validated = validate_staging_candidate(&candidate)?;
    if validated.manifest.name != request.skill_name
        || validated.content_hash != summary.content_hash
    {
        return stale("the staged central repair candidate changed before review");
    }
    let staged_name = summary.name.clone();
    let central = paths.central_skill(&request.skill_name);
    let central_hash = inspect_central(&central)?;
    if central_hash.is_some() && !allow_reappeared {
        return conflict_result("central repair requires missing managed content");
    }
    let changed_source = validated.content_hash != record.content_hash;
    if changed_source && matches!(record.source, SkillSource::Imported { .. }) {
        return conflict_result("the imported backup no longer matches its recorded hash");
    }
    let risk = audit_skill(candidate.path())?;
    let files = diff_trees(None, candidate.path())?;
    let mut warnings = Vec::new();
    if changed_source {
        warnings.push("changed-source recovery: the recorded source content changed".into());
    }
    let skill = PlannedSkill {
        manifest: validated.manifest,
        existing_source: central_hash.as_ref().map(|_| record.source.clone()),
        source: record.source.clone(),
        resolved_revision: resolution.resolved_revision.clone(),
        files,
        risk,
        existing_states: existing_states(&inventory, &request.skill_name),
        replace_existing: changed_source,
        content_hash: validated.content_hash,
    };
    let plan = new_plan(
        resolution.operation_id.clone(),
        SkillOperationKind::Repair,
        vec![skill],
        Vec::new(),
        settings_hash(&settings)?,
        warnings,
    )?;
    finalize_plan(PersistedPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        plan,
        input: PersistedPlanInput::Repair {
            request,
            resolution: Some(resolution),
            changed_source,
            backup_path: Some(backup_path),
        },
        expected_central: vec![ExpectedCentral {
            skill_name: staged_name,
            content_hash: central_hash,
        }],
        expected_links: Vec::new(),
        expected_target_roots: Vec::new(),
    })
}

fn assigned_targets_for_agents(
    assigned_target_ids: &BTreeSet<String>,
    request: &PlanAssignmentRequest,
) -> Result<BTreeSet<String>, SkillError> {
    let declared = declared_targets_for_agents(&request.agent_ids)?;
    Ok(assigned_target_ids
        .intersection(&declared)
        .cloned()
        .collect())
}

fn known_assigned_target_ids(
    settings: &SkillSettingsSnapshot,
    inventory: &SkillsInventory,
    skill_name: &str,
) -> BTreeSet<String> {
    let mut target_ids: BTreeSet<String> = inventory
        .items
        .iter()
        .filter(|item| item.name == skill_name)
        .flat_map(|item| item.assigned_target_ids.iter().cloned())
        .collect();
    if target_ids.is_empty() {
        target_ids = assigned_target_ids(settings, skill_name);
    }
    target_ids.extend(inventory.items.iter().filter_map(|item| {
        if item.name != skill_name || !item.states.contains(&InventoryState::Assigned) {
            return None;
        }
        match &item.location {
            super::SkillLocation::AgentTarget { target_id, .. } => Some(target_id.clone()),
            super::SkillLocation::Central => None,
        }
    }));
    target_ids
}

fn target_consumption_is_disabled(settings: &SkillSettingsSnapshot, name: &str, target_id: &str) -> bool {
    assigned_target_ids(settings, name).contains(target_id)
        && settings.skill_consumptions.as_ref().and_then(|skills| skills.get(name))
            .and_then(|records| records.get(target_id)).is_some_and(|record| !record.enabled)
}

fn assigned_target_ids(settings: &SkillSettingsSnapshot, skill_name: &str) -> BTreeSet<String> {
    settings
        .skill_assignments
        .as_ref()
        .and_then(|assignments| assignments.get(skill_name))
        .cloned()
        .unwrap_or_default()
}

fn managed_record<'a>(
    settings: &'a SkillSettingsSnapshot,
    skill_name: &str,
) -> Result<&'a ManagedSkillRecord, SkillError> {
    settings
        .managed_skills
        .as_ref()
        .and_then(|records| records.get(skill_name))
        .ok_or_else(|| invalid_source_error("the managed Skill is unavailable"))
}

fn managed_source(settings: &SkillSettingsSnapshot, skill_name: &str) -> Option<SkillSource> {
    settings
        .managed_skills
        .as_ref()
        .and_then(|records| records.get(skill_name))
        .map(|record| record.source.clone())
}

fn staged_candidate(paths: &SkillsPaths, operation_id: &str, skill_name: &str) -> PathBuf {
    paths
        .staging_skills_dir()
        .join(operation_id)
        .join("candidates")
        .join(skill_name)
}

fn manifest_from_record(record: &ManagedSkillRecord) -> SkillManifest {
    SkillManifest {
        name: record.name.clone(),
        description: record.description.clone(),
        license: None,
        compatibility: None,
        metadata: BTreeMap::new(),
        allowed_tools: None,
    }
}

fn removed_file_changes(root: &Path) -> Result<Vec<SkillFileChange>, SkillError> {
    super::inspect_tree(root).map(|files| {
        files
            .into_iter()
            .map(|file| SkillFileChange {
                path: file.path,
                kind: FileChangeKind::Removed,
                before_hash: Some(file.sha256),
                after_hash: None,
                unified_diff: None,
                diff_truncated: false,
            })
            .collect()
    })
}

fn commit_plan(
    request: SkillCommitRequest,
    expected_kind: SkillOperationKind,
) -> Result<SkillsInventory, SkillError> {
    validate_operation_id(&request.operation_id)?;
    let paths = SkillsPaths::resolve_from_env()?;
    let persisted = load_plan(&paths, &request.operation_id)?;
    if persisted.plan.operation_id != request.operation_id
        || persisted.plan.kind != expected_kind
        || request.candidate_hash != persisted.plan.candidate_hash
    {
        return Err(stale_error(
            "the reviewed Skills plan does not match this commit",
        ));
    }
    let rebuilt = rebuild_plan(&persisted).map_err(revalidation_error)?;
    let effective =
        if rebuilt == persisted || central_repair_reappearance_matches(&persisted, &rebuilt)? {
            rebuilt
        } else {
            return Err(stale_error("the reviewed Skills plan is stale"));
        };
    if persisted.plan.requires_risk_override
        && request.findings_confirmation.as_deref() != Some(persisted.plan.findings_hash.as_str())
    {
        return Err(SkillError::ConfirmationRequired {
            message: "high-risk Skill findings require exact confirmation".into(),
            findings_hash: persisted.plan.findings_hash.clone(),
        });
    }
    let spec = transaction_spec(&paths, &effective)?;
    execute_transaction(spec)?;
    list_inventory()
}

fn central_repair_reappearance_matches(
    reviewed: &PersistedPlan,
    rebuilt: &PersistedPlan,
) -> Result<bool, SkillError> {
    let (
        PersistedPlanInput::Repair {
            request: reviewed_request,
            resolution: Some(_),
            backup_path: Some(_),
            ..
        },
        PersistedPlanInput::Repair {
            request: rebuilt_request,
            resolution: Some(_),
            backup_path: Some(_),
            ..
        },
    ) = (&reviewed.input, &rebuilt.input)
    else {
        return Ok(false);
    };
    if reviewed_request.repair != RepairKind::Central
        || rebuilt_request.repair != RepairKind::Central
        || reviewed.expected_central.len() != 1
        || rebuilt.expected_central.len() != 1
        || reviewed.expected_central[0].skill_name != rebuilt.expected_central[0].skill_name
        || reviewed.expected_central[0].content_hash.is_some()
        || rebuilt.expected_central[0].content_hash.is_none()
        || reviewed.plan.skills.len() != 1
        || rebuilt.plan.skills.len() != 1
        || reviewed.plan.skills[0].manifest.name != rebuilt.plan.skills[0].manifest.name
        || !central_reappearance_states_match(
            &reviewed.plan.skills[0].existing_states,
            &rebuilt.plan.skills[0].existing_states,
        )
        || candidate_hash(rebuilt)? != rebuilt.plan.candidate_hash
    {
        return Ok(false);
    }

    let mut normalized = rebuilt.clone();
    normalized.expected_central = reviewed.expected_central.clone();
    normalized.plan.skills[0].existing_states = reviewed.plan.skills[0].existing_states.clone();
    normalized.plan.skills[0].existing_source = reviewed.plan.skills[0].existing_source.clone();
    normalized.plan.candidate_hash = candidate_hash(&normalized)?;
    Ok(&normalized == reviewed)
}

fn central_reappearance_states_match(
    reviewed: &BTreeSet<InventoryState>,
    rebuilt: &BTreeSet<InventoryState>,
) -> bool {
    if !reviewed.contains(&InventoryState::Missing)
        || reviewed.contains(&InventoryState::Managed)
        || reviewed.contains(&InventoryState::LocallyModified)
        || rebuilt.contains(&InventoryState::Missing)
        || (!rebuilt.contains(&InventoryState::Managed)
            && !rebuilt.contains(&InventoryState::LocallyModified))
    {
        return false;
    }
    let normalize = |states: &BTreeSet<InventoryState>| {
        states
            .iter()
            .filter(|state| {
                !matches!(
                    state,
                    InventoryState::Missing
                        | InventoryState::Managed
                        | InventoryState::LocallyModified
                )
            })
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    normalize(reviewed) == normalize(rebuilt)
}

fn revalidation_error(error: SkillError) -> SkillError {
    match error {
        SkillError::InvalidManifest { .. }
        | SkillError::UnsafePath { .. }
        | SkillError::InvalidSource { .. }
        | SkillError::Conflict { .. } => {
            stale_error("reviewed Skill candidates or targets changed after planning")
        }
        other => other,
    }
}

fn rebuild_plan(persisted: &PersistedPlan) -> Result<PersistedPlan, SkillError> {
    match &persisted.input {
        PersistedPlanInput::Install {
            request,
            resolution,
        } => {
            let paths = SkillsPaths::resolve_from_env()?;
            let reloaded = load_staged_resolution(&paths, &persisted.plan.operation_id)?;
            if &reloaded != resolution {
                return Err(stale_error(
                    "the staged Skill resolution changed after review",
                ));
            }
            build_install_plan(request.clone(), reloaded)
        }
        PersistedPlanInput::Import {
            request,
            source_target_id,
            source_name,
            original_path,
            backup_path,
        } => build_import_plan(
            request.clone(),
            persisted.plan.operation_id.clone(),
            source_target_id.clone(),
            source_name.clone(),
            original_path.clone(),
            backup_path.clone(),
        ),
        PersistedPlanInput::Assignment { request } => {
            build_assignment_plan(request.clone(), persisted.plan.operation_id.clone())
        }
        PersistedPlanInput::Update {
            request,
            resolution,
            backup_path,
        } => {
            let paths = SkillsPaths::resolve_from_env()?;
            let reloaded = load_staged_resolution(&paths, &persisted.plan.operation_id)?;
            if &reloaded != resolution {
                return Err(stale_error("the staged Skill update changed after review"));
            }
            build_update_plan(request.clone(), reloaded, backup_path.clone())
        }
        PersistedPlanInput::Remove {
            request,
            backup_path,
        } => build_remove_plan(
            request.clone(),
            persisted.plan.operation_id.clone(),
            backup_path.clone(),
        ),
        PersistedPlanInput::Repair {
            request,
            resolution,
            backup_path,
            ..
        } => match resolution {
            Some(expected) => {
                let paths = SkillsPaths::resolve_from_env()?;
                let reloaded = load_staged_resolution(&paths, &persisted.plan.operation_id)?;
                if &reloaded != expected {
                    return Err(stale_error(
                        "the staged central repair changed after review",
                    ));
                }
                let backup_path = backup_path.clone().ok_or_else(|| {
                    invalid_source_error("the reviewed central repair backup path is unavailable")
                })?;
                build_central_repair_plan(request.clone(), reloaded, backup_path, true)
            }
            None => build_target_repair_plan(request.clone(), persisted.plan.operation_id.clone()),
        },
    }
}

fn transaction_spec(
    paths: &SkillsPaths,
    persisted: &PersistedPlan,
) -> Result<TransactionSpec, SkillError> {
    let settings_before = current_settings_snapshot()?;
    if settings_hash(&settings_before)? != persisted.plan.settings_hash {
        return Err(stale_error("Skills settings changed after review"));
    }
    let mut settings_after = settings_before.clone();
    match &persisted.input {
        PersistedPlanInput::Install { .. } | PersistedPlanInput::Import { .. } => {
            install_settings_after(paths, persisted, &mut settings_after)?;
        }
        PersistedPlanInput::Assignment { request } => {
            assignment_settings_after(persisted, request, &mut settings_after)?;
        }
        PersistedPlanInput::Update { .. } => {
            replacement_settings_after(paths, persisted, &mut settings_after)?;
        }
        PersistedPlanInput::Remove { request, .. } => {
            remove_settings_after(request, &mut settings_after);
        }
        PersistedPlanInput::Repair {
            request,
            resolution,
            ..
        } => {
            if resolution.is_some() {
                replacement_settings_after(paths, persisted, &mut settings_after)?;
            } else {
                repair_target_settings_after(request, &mut settings_after);
            }
        }
    }
    reconcile_skill_consumptions(&mut settings_after);

    let mut directory_mutations = Vec::new();
    let replacement_operation = matches!(
        &persisted.input,
        PersistedPlanInput::Install { .. }
            | PersistedPlanInput::Import { .. }
            | PersistedPlanInput::Update { .. }
            | PersistedPlanInput::Repair {
                resolution: Some(_),
                ..
            }
    );
    if replacement_operation {
        for expected in &persisted.expected_central {
            // A repository revision can advance without changing this Skill.
            // The staged tree and installed hash were revalidated together;
            // keep its inode and links intact while committing source metadata.
            let unchanged_update = matches!(persisted.input, PersistedPlanInput::Update { .. })
                && persisted.plan.skills.iter().any(|skill|
                    skill.manifest.name == expected.skill_name
                        && expected.content_hash.as_ref() == Some(&skill.content_hash));
            if unchanged_update { continue; }
            let (backup, retain_backup) = match &persisted.input {
                PersistedPlanInput::Update { backup_path, .. } => (
                    paths.expand_user(backup_path).ok_or_else(|| {
                        invalid_source_error("the reviewed update backup path is invalid")
                    })?,
                    true,
                ),
                PersistedPlanInput::Repair {
                    request,
                    resolution: Some(_),
                    backup_path: Some(backup_path),
                    ..
                } if request.repair == RepairKind::Central && expected.content_hash.is_some() => (
                    paths.expand_user(backup_path).ok_or_else(|| {
                        invalid_source_error("the reviewed central repair backup path is invalid")
                    })?,
                    true,
                ),
                PersistedPlanInput::Install { .. } | PersistedPlanInput::Import { .. }
                    if expected.content_hash.is_some() =>
                {
                    (
                        central_backup_path(
                            paths,
                            &persisted.plan.operation_id,
                            &expected.skill_name,
                        )?,
                        true,
                    )
                }
                _ => (
                    central_backup_path(paths, &persisted.plan.operation_id, &expected.skill_name)?,
                    false,
                ),
            };
            directory_mutations.push(DirectoryMutation {
                replacement: Some(staged_candidate(
                    paths,
                    &persisted.plan.operation_id,
                    &expected.skill_name,
                )),
                destination: paths.central_skill(&expected.skill_name),
                backup,
                expected_before_hash: expected.content_hash.clone(),
                retain_backup,
            });
        }
    } else if let PersistedPlanInput::Remove { backup_path, .. } = &persisted.input {
        for expected in &persisted.expected_central {
            if expected.content_hash.is_none() {
                continue;
            }
            directory_mutations.push(DirectoryMutation {
                replacement: None,
                destination: paths.central_skill(&expected.skill_name),
                backup: paths.expand_user(backup_path).ok_or_else(|| {
                    invalid_source_error("the reviewed removal backup path is invalid")
                })?,
                expected_before_hash: expected.content_hash.clone(),
                retain_backup: true,
            });
        }
    }

    let target_by_id = persisted
        .plan
        .targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let mut link_mutations = Vec::new();
    for expected in &persisted.expected_links {
        let target = target_by_id
            .get(expected.target_id.as_str())
            .ok_or_else(|| invalid_source_error("a planned Skill target is unavailable"))?;
        let path = paths
            .expand_user(&target.global_dir)
            .ok_or_else(|| invalid_source_error("a planned Skill target path is unavailable"))?
            .join(&expected.skill_name);
        let desired_target = if expected.desired_managed {
            Some(paths.central_skill(&expected.skill_name))
        } else {
            None
        };
        let backup = if matches!(expected.state, LinkState::Directory { .. }) {
            Some(import_or_replacement_backup(paths, persisted, expected)?)
        } else {
            None
        };
        link_mutations.push(LinkMutation {
            path,
            expected: expected.state.clone(),
            desired_target,
            backup,
        });
    }
    Ok(TransactionSpec {
        operation_id: persisted.plan.operation_id.clone(),
        order: if matches!(&persisted.input, PersistedPlanInput::Remove { .. }) {
            TransactionOrder::LinksThenContent
        } else {
            TransactionOrder::ContentThenLinks
        },
        directory_mutations,
        link_mutations,
        settings_before,
        settings_after,
    })
}

fn reconcile_skill_consumptions(settings: &mut SkillSettingsSnapshot) {
    let assignments = settings.skill_assignments.clone().unwrap_or_default();
    let mut consumptions = settings.skill_consumptions.take().unwrap_or_default();
    consumptions.retain(|name, targets| {
        let Some(assigned_targets) = assignments.get(name) else {
            return false;
        };
        targets.retain(|target_id, _| assigned_targets.contains(target_id));
        true
    });
    for (name, target_ids) in assignments {
        let records = consumptions.entry(name.clone()).or_default();
        for target_id in target_ids {
            records.entry(target_id.clone()).or_insert_with(|| {
                crate::domain::assets::SkillConsumptionRecord {
                    name: name.clone(),
                    target_id,
                    enabled: true,
                }
            });
        }
    }
    settings.skill_consumptions = (!consumptions.is_empty()).then_some(consumptions);
}

fn central_backup_path(
    paths: &SkillsPaths,
    operation_id: &str,
    skill_name: &str,
) -> Result<PathBuf, SkillError> {
    validate_operation_id(operation_id)?;
    if !valid_skill_name(skill_name) {
        return Err(invalid_source_error(
            "a planned central Skill backup name is invalid",
        ));
    }
    Ok(paths
        .backups_skills_dir()
        .join(format!("{operation_id}-central-{skill_name}")))
}

fn install_settings_after(
    paths: &SkillsPaths,
    persisted: &PersistedPlan,
    settings: &mut SkillSettingsSnapshot,
) -> Result<(), SkillError> {
    let operation = StagingRoot::open(paths)?.open_operation(&persisted.plan.operation_id)?;
    let staged_candidates = operation.root_directory()?.open_directory("candidates")?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let records = settings.managed_skills.get_or_insert_default();
    let assignments = settings.skill_assignments.get_or_insert_default();
    let mut enabled_targets = Vec::new();
    for skill in &persisted.plan.skills {
        let candidate = staged_candidates.open_directory(&skill.manifest.name)?;
        let validated = validate_staging_candidate(&candidate)?;
        if validated.content_hash != skill.content_hash {
            return Err(stale_error(
                "a staged Skill candidate changed before commit",
            ));
        }
        let installed_at = records
            .get(&skill.manifest.name)
            .map(|record| record.installed_at.clone())
            .unwrap_or_else(|| now.clone());
        records.insert(
            skill.manifest.name.clone(),
            ManagedSkillRecord {
                name: skill.manifest.name.clone(),
                description: skill.manifest.description.clone(),
                content_kind: validated.content_kind,
                source: skill.source.clone(),
                resolved_revision: skill.resolved_revision.clone(),
                content_hash: skill.content_hash.clone(),
                installed_at,
                updated_at: now.clone(),
                risk: skill.risk.clone(),
                update: SkillUpdateState::default(),
            },
        );
        let desired_target_ids = persisted
            .expected_links
            .iter()
            .filter(|link| link.skill_name == skill.manifest.name && link.desired_managed)
            .map(|link| link.target_id.clone())
            .collect::<BTreeSet<_>>();
        if !desired_target_ids.is_empty() {
            assignments.entry(skill.manifest.name.clone()).or_default().extend(desired_target_ids.iter().cloned());
            enabled_targets.push((skill.manifest.name.clone(), desired_target_ids));
        }
    }
    if assignments.is_empty() {
        settings.skill_assignments = None;
    }
    for (name, target_ids) in enabled_targets {
        mark_skill_targets_enabled(settings, &name, &target_ids);
    }
    Ok(())
}

fn assignment_settings_after(
    persisted: &PersistedPlan,
    request: &PlanAssignmentRequest,
    settings: &mut SkillSettingsSnapshot,
) -> Result<(), SkillError> {
    let assignments = settings.skill_assignments.get_or_insert_default();
    let desired_target_ids = persisted
        .expected_links
        .iter()
        .filter(|link| link.skill_name == request.skill_name && link.desired_managed)
        .map(|link| link.target_id.clone())
        .collect::<BTreeSet<_>>();
    if desired_target_ids.is_empty() {
        assignments.remove(&request.skill_name);
    } else {
        assignments.insert(request.skill_name.clone(), desired_target_ids);
    }
    if assignments.is_empty() {
        settings.skill_assignments = None;
    }
    if request.enabled {
        let enabled_target_ids = normalize_agent_selection(&request.agent_ids)?
            .into_iter()
            .collect::<BTreeSet<_>>();
        mark_skill_targets_enabled(settings, &request.skill_name, &enabled_target_ids);
    }
    Ok(())
}

fn mark_skill_targets_enabled(
    settings: &mut SkillSettingsSnapshot,
    name: &str,
    target_ids: &BTreeSet<String>,
) {
    if target_ids.is_empty() {
        return;
    }
    let records = settings
        .skill_consumptions
        .get_or_insert_default()
        .entry(name.to_string())
        .or_default();
    for target_id in target_ids {
        records.insert(
            target_id.clone(),
            crate::domain::assets::SkillConsumptionRecord {
                name: name.to_string(),
                target_id: target_id.clone(),
                enabled: true,
            },
        );
    }
}

fn replacement_settings_after(
    paths: &SkillsPaths,
    persisted: &PersistedPlan,
    settings: &mut SkillSettingsSnapshot,
) -> Result<(), SkillError> {
    let operation = StagingRoot::open(paths)?.open_operation(&persisted.plan.operation_id)?;
    let staged_candidates = operation.root_directory()?.open_directory("candidates")?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let records = settings.managed_skills.as_mut().ok_or_else(|| {
        invalid_source_error("the reviewed managed Skill settings are unavailable")
    })?;
    for skill in &persisted.plan.skills {
        let candidate = staged_candidates.open_directory(&skill.manifest.name)?;
        let validated = validate_staging_candidate(&candidate)?;
        if validated.manifest.name != skill.manifest.name
            || validated.content_hash != skill.content_hash
        {
            return Err(stale_error(
                "a staged Skill replacement changed before commit",
            ));
        }
        let record = records.get_mut(&skill.manifest.name).ok_or_else(|| {
            invalid_source_error("the reviewed managed Skill record is unavailable")
        })?;
        record.description = skill.manifest.description.clone();
        record.content_kind = validated.content_kind;
        record.source = skill.source.clone();
        record.resolved_revision = skill.resolved_revision.clone();
        record.content_hash = skill.content_hash.clone();
        record.updated_at = now.clone();
        record.risk = skill.risk.clone();
        let preserves_known_update = matches!(
            &persisted.input,
            PersistedPlanInput::Repair {
                resolution: Some(_),
                ..
            }
        ) && matches!(&skill.source, SkillSource::Github { .. });
        if !preserves_known_update {
            record.update.available = false;
            record.update.resolved_revision = match &skill.source {
                SkillSource::Github { .. } => skill.resolved_revision.clone(),
                SkillSource::Local { .. } | SkillSource::Archive { .. } => {
                    Some(skill.content_hash.clone())
                }
                SkillSource::Imported { .. } => None,
            };
            record.update.error = None;
            record.update.retry_at = None;
        }
    }
    Ok(())
}

fn remove_settings_after(request: &PlanRemoveRequest, settings: &mut SkillSettingsSnapshot) {
    if let Some(records) = settings.managed_skills.as_mut() {
        records.remove(&request.skill_name);
        if records.is_empty() {
            settings.managed_skills = None;
        }
    }
    if let Some(assignments) = settings.skill_assignments.as_mut() {
        assignments.remove(&request.skill_name);
        if assignments.is_empty() {
            settings.skill_assignments = None;
        }
    }
}

fn repair_target_settings_after(request: &PlanRepairRequest, settings: &mut SkillSettingsSnapshot) {
    let RepairKind::Target { target_id } = &request.repair else {
        return;
    };
    settings
        .skill_assignments
        .get_or_insert_default()
        .entry(request.skill_name.clone())
        .or_default()
        .insert(target_id.clone());
}

fn import_or_replacement_backup(
    paths: &SkillsPaths,
    persisted: &PersistedPlan,
    expected: &ExpectedLink,
) -> Result<PathBuf, SkillError> {
    if let PersistedPlanInput::Import {
        source_target_id,
        source_name,
        backup_path,
        ..
    } = &persisted.input
    {
        if source_target_id == &expected.target_id && source_name == &expected.skill_name {
            return paths
                .expand_user(backup_path)
                .ok_or_else(|| invalid_source_error("the reviewed import backup path is invalid"));
        }
    }
    Ok(paths
        .backups_skills_dir()
        .join(&persisted.plan.operation_id)
        .join("targets")
        .join(&expected.target_id)
        .join(&expected.skill_name))
}

fn new_plan(
    operation_id: String,
    kind: SkillOperationKind,
    mut skills: Vec<PlannedSkill>,
    mut targets: Vec<PlannedTarget>,
    settings_hash: String,
    mut warnings: Vec<String>,
) -> Result<OperationPlan, SkillError> {
    skills.sort_by(|left, right| left.manifest.name.cmp(&right.manifest.name));
    targets.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    warnings.sort();
    let findings_hash = aggregate_findings_hash(&skills)?;
    Ok(OperationPlan {
        operation_id,
        kind,
        skills,
        targets,
        settings_hash,
        candidate_hash: String::new(),
        findings_hash,
        // Derived from the complete persisted operation in `finalize_plan`.
        requires_risk_override: false,
        warnings,
    })
}

fn finalize_plan(mut persisted: PersistedPlan) -> Result<PersistedPlan, SkillError> {
    persisted.expected_central.sort();
    persisted.expected_links.sort_by(|left, right| {
        left.skill_name
            .cmp(&right.skill_name)
            .then(left.target_id.cmp(&right.target_id))
    });
    persisted.expected_target_roots =
        expected_target_roots(&SkillsPaths::resolve_from_env()?, &persisted.plan.targets)?;
    persisted.expected_target_roots.sort();
    // Risk approval is a property of the complete operation, not of a builder
    // that happens to contain a high-risk Skill. Derive it in the same place
    // used by load-time integrity validation so creation and verification
    // cannot drift apart. Only operations that introduce or replace central
    // content require a findings token; assignment and authority-reducing
    // operations reuse the exact central content version already approved.
    persisted.plan.requires_risk_override = expected_risk_override(&persisted);
    persisted.plan.candidate_hash = candidate_hash(&persisted)?;
    Ok(persisted)
}

fn candidate_hash(persisted: &PersistedPlan) -> Result<String, SkillError> {
    let mut requested_agent_ids = match &persisted.input {
        PersistedPlanInput::Install { request, .. } => request
            .agent_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        PersistedPlanInput::Import { request, .. } => request
            .agent_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        PersistedPlanInput::Assignment { request } => request
            .agent_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        PersistedPlanInput::Update { .. }
        | PersistedPlanInput::Remove { .. }
        | PersistedPlanInput::Repair { .. } => Vec::new(),
    };
    requested_agent_ids.sort();
    let (replace_conflicts, assignment_enabled) = match &persisted.input {
        PersistedPlanInput::Install { request, .. } => (request.replace_conflicts, None),
        PersistedPlanInput::Import { request, .. } => (request.replace_conflicts, None),
        PersistedPlanInput::Assignment { request } => (false, Some(request.enabled)),
        PersistedPlanInput::Update { .. }
        | PersistedPlanInput::Remove { .. }
        | PersistedPlanInput::Repair { .. } => (false, None),
    };
    let lifecycle = match &persisted.input {
        PersistedPlanInput::Update {
            request,
            backup_path,
            ..
        } => Some(LifecycleBinding::Update {
            replace_local_changes: request.replace_local_changes,
            backup_path,
        }),
        PersistedPlanInput::Remove { backup_path, .. } => {
            Some(LifecycleBinding::Remove { backup_path })
        }
        PersistedPlanInput::Repair {
            request,
            changed_source,
            backup_path,
            ..
        } => Some(LifecycleBinding::Repair {
            repair: &request.repair,
            changed_source: *changed_source,
            backup_path,
        }),
        PersistedPlanInput::Install { .. }
        | PersistedPlanInput::Import { .. }
        | PersistedPlanInput::Assignment { .. } => None,
    };
    let binding = CandidateBinding {
        operation_id: &persisted.plan.operation_id,
        kind: &persisted.plan.kind,
        skills: persisted
            .plan
            .skills
            .iter()
            .map(|skill| BoundSkill {
                name: &skill.manifest.name,
                existing_source: &skill.existing_source,
                source: &skill.source,
                resolved_revision: &skill.resolved_revision,
                content_hash: &skill.content_hash,
                replace_existing: skill.replace_existing,
            })
            .collect(),
        requested_agent_ids,
        target_ids: persisted
            .plan
            .targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect(),
        replace_conflicts,
        assignment_enabled,
        lifecycle,
        expected_central: &persisted.expected_central,
        expected_links: &persisted.expected_links,
        expected_target_roots: &persisted.expected_target_roots,
    };
    canonical_hash(&binding)
}

fn aggregate_findings_hash(skills: &[PlannedSkill]) -> Result<String, SkillError> {
    let digests = skills
        .iter()
        .map(|skill| {
            findings_digest(&skill.risk).map(|digest| (skill.manifest.name.clone(), digest))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let [(.., digest)] = digests.as_slice() {
        return Ok(digest.clone());
    }
    canonical_hash(&digests)
}

fn current_settings_snapshot() -> Result<SkillSettingsSnapshot, SkillError> {
    let settings = load_settings_strict().map_err(|_| SkillError::Io {
        message: "Arca settings could not be read safely".into(),
        path: None,
    })?;
    Ok(snapshot_from_settings(&settings))
}

fn snapshot_from_settings(settings: &Settings) -> SkillSettingsSnapshot {
    SkillSettingsSnapshot {
        managed_skills: settings.managed_skills.clone(),
        skill_assignments: settings.skill_assignments.clone(),
        skill_consumptions: settings.skill_consumptions.clone(),
        skill_update_checked_at: settings.skill_update_checked_at.clone(),
    }
}

fn settings_hash(settings: &SkillSettingsSnapshot) -> Result<String, SkillError> {
    canonical_hash(settings)
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<String, SkillError> {
    let bytes = serde_json::to_vec(value).map_err(|_| SkillError::InvalidSource {
        message: "canonical Skills plan data could not be encoded".into(),
    })?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn selected_skill_names(names: &[String]) -> Result<Vec<String>, SkillError> {
    if names.is_empty() {
        return invalid_source("an install requires at least one selected Skill");
    }
    let mut selected = BTreeSet::new();
    for name in names {
        if !valid_skill_name(name) || !selected.insert(name.clone()) {
            return invalid_source("the selected Skill names are invalid or duplicated");
        }
    }
    Ok(selected.into_iter().collect())
}

fn valid_skill_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn candidate_source(source: &SkillSource, relative_path: &str) -> SkillSource {
    let join = |base: &str| match (base.is_empty(), relative_path.is_empty()) {
        (true, _) => relative_path.to_owned(),
        (_, true) => base.to_owned(),
        (false, false) => format!("{base}/{relative_path}"),
    };
    match source {
        SkillSource::Github {
            owner,
            repo,
            subpath,
            requested_ref,
            pinned,
        } => SkillSource::Github {
            owner: owner.clone(),
            repo: repo.clone(),
            subpath: join(subpath),
            requested_ref: requested_ref.clone(),
            pinned: *pinned,
        },
        SkillSource::Local { path, subpath } => SkillSource::Local {
            path: path.clone(),
            subpath: join(subpath),
        },
        SkillSource::Archive { path, subpath } => SkillSource::Archive {
            path: path.clone(),
            subpath: join(subpath),
        },
        SkillSource::Imported { .. } => source.clone(),
    }
}

fn selected_target_views(
    inventory: &SkillsInventory,
    target_ids: &[String],
) -> Result<Vec<SkillTargetView>, SkillError> {
    let mut views = Vec::new();
    for target_id in target_ids {
        let target = inventory
            .targets
            .iter()
            .find(|target| &target.target_id == target_id)
            .ok_or_else(|| {
                invalid_source_error("a normalized Agent Skill target is unavailable")
            })?;
        views.push(target.clone());
    }
    views.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    Ok(views)
}

fn target_path(
    paths: &SkillsPaths,
    target: &SkillTargetView,
    skill_name: &str,
) -> Result<PathBuf, SkillError> {
    paths
        .expand_user(&target.global_dir)
        .filter(|path| path.is_absolute())
        .map(|path| path.join(skill_name))
        .ok_or_else(|| invalid_source_error("a verified Agent Skill target path is invalid"))
}

fn expected_target_roots(
    paths: &SkillsPaths,
    targets: &[PlannedTarget],
) -> Result<Vec<ExpectedTargetRoot>, SkillError> {
    targets
        .iter()
        .map(|target| expected_target_root(paths, target))
        .collect()
}

fn expected_target_root(
    paths: &SkillsPaths,
    target: &PlannedTarget,
) -> Result<ExpectedTargetRoot, SkillError> {
    let root = paths
        .expand_user(&target.global_dir)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| invalid_source_error("a verified Agent Skill target path is invalid"))?;
    let root_path = normalized_physical_path(&root)?;
    let mut anchor = root.clone();
    let mut remaining_components = Vec::new();

    loop {
        match fs::symlink_metadata(&anchor) {
            Ok(metadata) => {
                if !metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                    return Err(SkillError::UnsafePath {
                        message: "a verified Agent Skill target parent is not a directory".into(),
                        path: String::new(),
                    });
                }
                let canonical =
                    fs::canonicalize(&anchor).map_err(|error| io_error(&anchor, error))?;
                let canonical_metadata = fs::symlink_metadata(&canonical)
                    .map_err(|error| io_error(&canonical, error))?;
                if !canonical_metadata.file_type().is_dir() {
                    return Err(SkillError::UnsafePath {
                        message: "a verified Agent Skill target parent is not a directory".into(),
                        path: String::new(),
                    });
                }
                remaining_components.reverse();
                let (anchor_device, anchor_inode, anchor_mode) =
                    directory_identity(&canonical_metadata);
                return Ok(ExpectedTargetRoot {
                    target_id: target.target_id.clone(),
                    root_path,
                    anchor_path: normalized_physical_path(&canonical)?,
                    anchor_device,
                    anchor_inode,
                    anchor_mode,
                    remaining_components,
                });
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let component = anchor
                    .file_name()
                    .and_then(|value| value.to_str())
                    .filter(|value| !value.is_empty() && !matches!(*value, "." | ".."))
                    .ok_or_else(|| {
                        invalid_source_error("a verified Agent Skill target path is invalid")
                    })?;
                remaining_components.push(component.to_owned());
                anchor = anchor
                    .parent()
                    .ok_or_else(|| {
                        invalid_source_error("a verified Agent Skill target path is invalid")
                    })?
                    .to_path_buf();
            }
            Err(error) => return Err(io_error(&anchor, error)),
        }
    }
}

fn normalized_physical_path(path: &Path) -> Result<String, SkillError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return invalid_source("a verified Agent Skill target path is invalid");
    }
    path.to_str()
        .map(|value| value.replace('\\', "/"))
        .ok_or_else(|| invalid_source_error("a verified Agent Skill target path is not UTF-8"))
}

#[cfg(unix)]
fn directory_identity(metadata: &fs::Metadata) -> (u64, u64, u32) {
    (metadata.dev(), metadata.ino(), metadata.mode())
}

#[cfg(not(unix))]
fn directory_identity(metadata: &fs::Metadata) -> (u64, u64, u32) {
    (
        0,
        metadata.len(),
        u32::from(metadata.permissions().readonly()),
    )
}

fn inspect_central(path: &Path) -> Result<Option<String>, SkillError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path, error)),
        Ok(metadata) if metadata.file_type().is_dir() => hash_tree(path).map(Some),
        Ok(_) => conflict_result("central Skill content has an unsupported type"),
    }
}

fn inspect_link(path: &Path, central: &Path, paths: &SkillsPaths) -> Result<LinkState, SkillError> {
    let metadata = match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(LinkState::Missing),
        Err(error) => return Err(io_error(path, error)),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_dir() {
        return Ok(LinkState::Directory {
            tree_hash: hash_tree(path)?,
        });
    }
    if !metadata.file_type().is_symlink() {
        return conflict_result("an Agent Skill target has an unsupported type");
    }
    let raw_target = fs::read_link(path).map_err(|error| io_error(path, error))?;
    match fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound || is_symlink_loop(&error) => {
            Ok(LinkState::BrokenSymlink { target: raw_target })
        }
        Err(error) => Err(io_error(path, error)),
        Ok(_) => {
            let resolved = fs::canonicalize(path).map_err(|error| io_error(path, error))?;
            let central_resolved = fs::canonicalize(central).ok();
            if central_resolved.as_ref() == Some(&resolved) {
                Ok(LinkState::ManagedSymlink { target: raw_target })
            } else {
                let central_root = fs::canonicalize(paths.skills_dir()).ok();
                if central_root
                    .as_ref()
                    .is_some_and(|root| resolved == *root || resolved.starts_with(root))
                {
                    return conflict_result(
                        "an Agent Skill link points to different managed content",
                    );
                }
                Ok(LinkState::UnknownSymlink { target: raw_target })
            }
        }
    }
}

#[cfg(unix)]
fn is_symlink_loop(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error())
}

#[cfg(not(unix))]
fn is_symlink_loop(_error: &std::io::Error) -> bool {
    false
}

fn is_link_conflict(state: &LinkState) -> bool {
    matches!(
        state,
        LinkState::BrokenSymlink { .. }
            | LinkState::Directory { .. }
            | LinkState::UnknownSymlink { .. }
    )
}

fn is_safe_absent_transition(state: &LinkState) -> bool {
    matches!(state, LinkState::Missing | LinkState::ManagedSymlink { .. })
}

fn disable_conflict<T>(state: &LinkState, path: &Path, home: &Path) -> Result<T, SkillError> {
    let reason = match state {
        LinkState::Directory { .. } => "an unmanaged Skill directory cannot be disabled",
        LinkState::BrokenSymlink { .. } => "a broken Skill link cannot be disabled",
        LinkState::UnknownSymlink { .. } => {
            "a Skill link that points to unmanaged content cannot be disabled"
        }
        LinkState::Missing | LinkState::ManagedSymlink { .. } => {
            "only an exact managed Skill link can be disabled"
        }
    };
    Err(SkillError::Conflict {
        message: super::capped_message(reason),
        path: collapse_home(path, home),
    })
}

fn existing_states(inventory: &SkillsInventory, name: &str) -> BTreeSet<InventoryState> {
    inventory
        .items
        .iter()
        .filter(|item| item.name == name)
        .flat_map(|item| item.states.iter().cloned())
        .collect()
}

fn planned_targets(
    target_views: &[SkillTargetView],
    expected_links: &[ExpectedLink],
) -> Vec<PlannedTarget> {
    target_views
        .iter()
        .map(|target| {
            let expected = expected_links
                .iter()
                .filter(|link| link.target_id == target.target_id)
                .map(|link| planned_link_state(&link.state))
                .max_by_key(planned_link_priority)
                .unwrap_or(PlannedLinkState::Missing);
            PlannedTarget {
                target_id: target.target_id.clone(),
                global_dir: target.global_dir.clone(),
                expected,
                primary_agent_ids: target.primary_agent_ids.clone(),
                affected_agent_ids: target.affected_agent_ids.clone(),
            }
        })
        .collect()
}

fn planned_link_state(state: &LinkState) -> PlannedLinkState {
    match state {
        LinkState::Missing => PlannedLinkState::Missing,
        LinkState::ManagedSymlink { .. } => PlannedLinkState::Managed,
        LinkState::BrokenSymlink { .. } => PlannedLinkState::Broken,
        LinkState::Directory { .. } => PlannedLinkState::Directory,
        LinkState::UnknownSymlink { .. } => PlannedLinkState::UnknownSymlink,
    }
}

fn planned_link_priority(state: &PlannedLinkState) -> u8 {
    match state {
        PlannedLinkState::Missing => 0,
        PlannedLinkState::Managed => 1,
        PlannedLinkState::Broken => 2,
        PlannedLinkState::UnknownSymlink => 3,
        PlannedLinkState::Directory => 4,
    }
}

fn plan_warnings(targets: &[PlannedTarget], selected_agent_ids: &[String]) -> Vec<String> {
    let selected = selected_agent_ids.iter().collect::<BTreeSet<_>>();
    targets
        .iter()
        .filter_map(|target| {
            let shared = target
                .affected_agent_ids
                .iter()
                .filter(|agent_id| !selected.contains(agent_id))
                .cloned()
                .collect::<Vec<_>>();
            (!shared.is_empty()).then(|| {
                format!(
                    "Target {} also affects installed Agents: {}",
                    target.target_id,
                    shared.join(", ")
                )
            })
        })
        .collect()
}

fn ensure_recovery_clear() -> Result<(), SkillError> {
    if has_pending_recovery()? {
        return Err(SkillError::RecoveryRequired {
            message: "a pending Skills operation must be recovered before planning".into(),
        });
    }
    Ok(())
}

fn create_operation_root(paths: &SkillsPaths, operation_id: &str) -> Result<PathBuf, SkillError> {
    validate_operation_id(operation_id)?;
    Ok(StagingRoot::open_or_create(paths)?
        .create_operation(operation_id)?
        .path()
        .to_path_buf())
}

fn persist_plan(paths: &SkillsPaths, persisted: &PersistedPlan) -> Result<(), SkillError> {
    let bytes = serde_json::to_vec(persisted).map_err(|_| SkillError::InvalidSource {
        message: "the Skills plan could not be encoded safely".into(),
    })?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        return Err(SkillError::LimitExceeded {
            limit: "operation_plan".into(),
            actual: bytes.len() as u64,
            allowed: MAX_PLAN_BYTES,
        });
    }
    StagingRoot::open(paths)?
        .open_operation(&persisted.plan.operation_id)?
        .write_private_atomic(PLAN_FILE, &bytes, MAX_PLAN_BYTES)
}

fn load_plan(paths: &SkillsPaths, operation_id: &str) -> Result<PersistedPlan, SkillError> {
    let bytes = StagingRoot::open(paths)?
        .open_operation(operation_id)?
        .read_private(PLAN_FILE, MAX_PLAN_BYTES)?;
    let persisted: PersistedPlan = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_source_error("the reviewed Skills plan is malformed"))?;
    let canonical = serde_json::to_vec(&persisted)
        .map_err(|_| invalid_source_error("the reviewed Skills plan is malformed"))?;
    if canonical != bytes {
        return invalid_source("the reviewed Skills plan is not canonical");
    }
    if persisted.schema_version != PLAN_SCHEMA_VERSION
        || persisted.plan.operation_id != operation_id
        || candidate_hash(&persisted)? != persisted.plan.candidate_hash
        || aggregate_findings_hash(&persisted.plan.skills)? != persisted.plan.findings_hash
        || persisted.plan.requires_risk_override != expected_risk_override(&persisted)
    {
        return Err(stale_error(
            "the reviewed Skills plan failed integrity validation",
        ));
    }
    Ok(persisted)
}

fn remove_unjournaled_operation(paths: &SkillsPaths, operation_id: &str) -> Result<(), SkillError> {
    StagingRoot::open(paths)?
        .remove_operation_if_exists(operation_id)
        .map(|_| ())
}

fn expected_risk_override(persisted: &PersistedPlan) -> bool {
    let content_enters_central = matches!(
        &persisted.input,
        PersistedPlanInput::Install { .. }
            | PersistedPlanInput::Import { .. }
            | PersistedPlanInput::Update { .. }
            | PersistedPlanInput::Repair {
                resolution: Some(_),
                ..
            }
    );
    content_enters_central
        && persisted
            .plan
            .skills
            .iter()
            .any(|skill| skill.risk.level == RiskLevel::High)
}

fn collapse_home(path: &Path, home: &Path) -> String {
    if path == home {
        return "~".into();
    }
    if let Ok(relative) = path.strip_prefix(home) {
        return format!("~/{}", normalized_path(relative));
    }
    normalized_path(path)
}

fn normalized_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::RootDir => Some(String::new()),
            Component::Prefix(prefix) => Some(prefix.as_os_str().to_string_lossy().into_owned()),
            Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
            Component::CurDir | Component::ParentDir => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn sanitize_result<T>(result: Result<T, SkillError>) -> Result<T, SkillError> {
    result.map_err(sanitize_error)
}

fn sanitize_error(error: SkillError) -> SkillError {
    match error {
        SkillError::InvalidManifest { message, .. } => SkillError::InvalidManifest {
            message: super::capped_message(message),
            path: String::new(),
        },
        SkillError::UnsafePath { message, .. } => SkillError::UnsafePath {
            message: super::capped_message(message),
            path: String::new(),
        },
        SkillError::Conflict { message, path } => SkillError::Conflict {
            message: super::capped_message(message),
            // Public operation errors may retain an explicitly home-collapsed
            // target path. Raw absolute paths from lower layers remain hidden.
            path: if path == "~" || path.starts_with("~/") {
                super::capped_message(path)
            } else {
                String::new()
            },
        },
        SkillError::Io { message, .. } => SkillError::Io {
            message: super::capped_message(message),
            path: None,
        },
        other => other,
    }
}

fn invalid_source<T>(message: &str) -> Result<T, SkillError> {
    Err(invalid_source_error(message))
}

fn invalid_source_error(message: &str) -> SkillError {
    SkillError::InvalidSource {
        message: super::capped_message(message),
    }
}

fn conflict(message: &str) -> Result<PersistedPlan, SkillError> {
    conflict_result(message)
}

fn conflict_result<T>(message: &str) -> Result<T, SkillError> {
    Err(conflict_error(message))
}

fn conflict_error(message: &str) -> SkillError {
    SkillError::Conflict {
        message: super::capped_message(message),
        path: String::new(),
    }
}

fn stale<T>(message: &str) -> Result<T, SkillError> {
    Err(stale_error(message))
}

fn stale_error(message: &str) -> SkillError {
    SkillError::PlanStale {
        message: super::capped_message(message),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::resources::skill::{
        execute_transaction_with_failpoint, resolve_source, Failpoint, GithubEndpoints,
        SkillSourceInput,
    };
    use crate::testenv::TestHome;

    fn install_assigned_fixture(home: &TestHome, skill_name: &str) -> SkillsPaths {
        fs::create_dir_all(home.home.join(".claude")).unwrap();
        let source = home.home.join("source").join(skill_name);
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            format!("---\nname: {skill_name}\ndescription: Assignment fixture\n---\n"),
        )
        .unwrap();
        let resolution = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let plan = plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id,
            skill_names: vec![skill_name.into()],
            agent_ids: vec!["claude-code".into()],
            replace_conflicts: false,
        })
        .unwrap();
        commit_install(plan.confirmation()).unwrap();
        SkillsPaths::from_env().unwrap()
    }

    #[test]
    fn github_revision_only_update_preserves_content_inode_and_disabled_consumption() {
        let home = TestHome::new("github-metadata-update");
        let name = "metadata-only";
        let paths = install_assigned_fixture(&home, name);
        let central = paths.central_skill(name);
        let inode = fs::metadata(&central).unwrap().ino();
        let link = home.home.join(".claude/skills").join(name);
        fs::remove_file(&link).unwrap();
        let old_sha = "1111111111111111111111111111111111111111";
        let new_sha = "2222222222222222222222222222222222222222";
        let source = SkillSource::Github {
            owner: "acme".into(), repo: "skills".into(), subpath: format!("catalog/{name}"),
            requested_ref: "main".into(), pinned: false,
        };
        crate::settings::mutate_settings(|settings| {
            let record = settings.managed_skills.as_mut().unwrap().get_mut(name).unwrap();
            record.source = source.clone();
            record.resolved_revision = Some(old_sha.into());
            record.update.available = true;
            record.update.resolved_revision = Some(new_sha.into());
            settings.skill_consumptions.as_mut().unwrap().get_mut(name).unwrap()
                .get_mut("claude-user").unwrap().enabled = false;
        }).unwrap();
        let before = current_settings_snapshot().unwrap();
        let mut resolution = resolve_source(SkillSourceInput::Local {
            path: home.home.join("source").join(name).to_string_lossy().into_owned(),
        }, GithubEndpoints::production()).unwrap();
        resolution.source = source;
        resolution.resolved_revision = Some(new_sha.into());
        let operation = StagingRoot::open(&paths).unwrap().open_operation(&resolution.operation_id).unwrap();
        operation.write_private_atomic("resolution.json", &serde_json::to_vec(&resolution).unwrap(), MAX_PLAN_BYTES).unwrap();
        let persisted = build_update_plan(PlanUpdateRequest {
            skill_name: name.into(), replace_local_changes: false,
        }, resolution, format!("~/.arca/backups/skills/update-metadata/{name}")).unwrap();
        assert!(!persisted.plan.skills[0].replace_existing);
        assert!(persisted.plan.skills[0].files.is_empty());
        assert!(!persisted.plan.warnings.is_empty());
        let spec = transaction_spec(&paths, &persisted).unwrap();
        assert!(spec.directory_mutations.is_empty());
        assert!(spec.link_mutations.is_empty());
        let manifest = central.join("SKILL.md");
        let original = fs::read(&manifest).unwrap();
        fs::write(&manifest, "---\nname: metadata-only\ndescription: Changed after review\n---\n").unwrap();
        assert!(matches!(execute_transaction(spec), Err(SkillError::PlanStale { .. })));
        fs::write(&manifest, original).unwrap();
        persist_plan(&paths, &persisted).unwrap();
        commit_update(persisted.plan.confirmation()).unwrap();
        let after = current_settings_snapshot().unwrap();
        let record = &after.managed_skills.as_ref().unwrap()[name];
        assert_eq!(record.resolved_revision.as_deref(), Some(new_sha));
        assert_eq!(record.update.resolved_revision.as_deref(), Some(new_sha));
        assert!(!record.update.available);
        assert_eq!(after.skill_assignments, before.skill_assignments);
        assert_eq!(after.skill_consumptions, before.skill_consumptions);
        assert_eq!(fs::metadata(central).unwrap().ino(), inode);
        assert!(fs::symlink_metadata(link).is_err());
    }

    #[test]
    fn assignment_disable_accepts_managed_and_safely_missing_links() {
        let home = TestHome::new("assignment-disable-safe");
        install_assigned_fixture(&home, "disable-managed");
        let managed_path = home.home.join(".claude/skills/disable-managed");
        assert!(fs::symlink_metadata(&managed_path)
            .unwrap()
            .file_type()
            .is_symlink());

        let managed_plan = plan_assignment(PlanAssignmentRequest {
            skill_name: "disable-managed".into(),
            agent_ids: vec!["claude-code".into()],
            enabled: false,
        })
        .unwrap();
        commit_assignment(managed_plan.confirmation()).unwrap();
        assert!(!managed_path.exists());

        let paths = install_assigned_fixture(&home, "disable-missing");
        let missing_path = home.home.join(".claude/skills/disable-missing");
        fs::remove_file(&missing_path).unwrap();
        let missing_plan = plan_assignment(PlanAssignmentRequest {
            skill_name: "disable-missing".into(),
            agent_ids: vec!["claude-code".into()],
            enabled: false,
        })
        .unwrap();
        commit_assignment(missing_plan.confirmation()).unwrap();
        assert!(!missing_path.exists());
        assert!(paths.central_skill("disable-missing").is_dir());
    }

    #[test]
    fn assignment_disable_conflicts_explain_unsafe_link_state_and_target_path() {
        let home = TestHome::new("assignment-disable-conflicts");
        let path = home.home.join(".agents/skills/dws");
        let cases = [
            (
                LinkState::Directory {
                    tree_hash: "hash".into(),
                },
                "unmanaged Skill directory",
            ),
            (
                LinkState::BrokenSymlink {
                    target: PathBuf::from("missing"),
                },
                "broken Skill link",
            ),
            (
                LinkState::UnknownSymlink {
                    target: PathBuf::from("external"),
                },
                "unmanaged content",
            ),
        ];

        for (state, reason) in cases {
            let error = disable_conflict::<()>(&state, &path, &home.home).unwrap_err();
            let sanitized = sanitize_error(error);
            assert!(matches!(
                sanitized,
                SkillError::Conflict { ref message, ref path }
                    if message.contains(reason) && path == "~/.agents/skills/dws"
            ));
        }
    }

    #[test]
    fn central_backup_specs_retain_replaced_content_but_not_empty_rollback_paths() {
        let home = TestHome::new("ops-central-backup");
        let skill_name = "backup-anchor";
        let first_source = home.home.join("source/first/backup-anchor");
        fs::create_dir_all(&first_source).unwrap();
        fs::write(
            first_source.join("SKILL.md"),
            "---\nname: backup-anchor\ndescription: First fixture\n---\n",
        )
        .unwrap();
        let first_resolution = resolve_source(
            SkillSourceInput::Local {
                path: first_source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let first_plan = plan_install(PlanInstallRequest {
            resolution_id: first_resolution.operation_id,
            skill_names: vec![skill_name.into()],
            agent_ids: Vec::new(),
            replace_conflicts: false,
        })
        .unwrap();
        let paths = SkillsPaths::from_env().unwrap();
        let first_persisted = load_plan(&paths, &first_plan.operation_id).unwrap();
        let first_spec = transaction_spec(&paths, &first_persisted).unwrap();
        let first_mutation = &first_spec.directory_mutations[0];
        let first_backup = paths
            .backups_skills_dir()
            .join(format!("{}-central-{skill_name}", first_plan.operation_id));

        assert!(first_mutation.expected_before_hash.is_none());
        assert!(!first_mutation.retain_backup);
        assert_eq!(first_mutation.backup, first_backup);
        assert!(first_backup.parent().unwrap().is_dir());
        execute_transaction(first_spec).unwrap();
        assert!(!first_backup.exists());

        let second_source = home.home.join("source/second/backup-anchor");
        fs::create_dir_all(&second_source).unwrap();
        fs::write(
            second_source.join("SKILL.md"),
            "---\nname: backup-anchor\ndescription: Second fixture\n---\n",
        )
        .unwrap();
        let second_resolution = resolve_source(
            SkillSourceInput::Local {
                path: second_source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let second_plan = plan_install(PlanInstallRequest {
            resolution_id: second_resolution.operation_id,
            skill_names: vec![skill_name.into()],
            agent_ids: Vec::new(),
            replace_conflicts: true,
        })
        .unwrap();
        let second_persisted = load_plan(&paths, &second_plan.operation_id).unwrap();
        let mut different_existing_source = second_persisted.clone();
        different_existing_source.plan.skills[0].existing_source = Some(SkillSource::Github {
            owner: "different".into(),
            repo: "source".into(),
            subpath: skill_name.into(),
            requested_ref: "main".into(),
            pinned: false,
        });
        assert_ne!(
            candidate_hash(&second_persisted).unwrap(),
            candidate_hash(&different_existing_source).unwrap()
        );
        let second_spec = transaction_spec(&paths, &second_persisted).unwrap();
        let second_mutation = &second_spec.directory_mutations[0];
        let second_backup = paths
            .backups_skills_dir()
            .join(format!("{}-central-{skill_name}", second_plan.operation_id));

        assert!(second_mutation.expected_before_hash.is_some());
        assert!(second_mutation.retain_backup);
        assert_eq!(second_mutation.backup, second_backup);
        assert!(second_backup.parent().unwrap().is_dir());
        execute_transaction(second_spec).unwrap();
        assert!(second_backup.exists());
        assert!(
            fs::read_to_string(paths.central_skill(skill_name).join("SKILL.md"))
                .unwrap()
                .contains("Second fixture")
        );
    }

    #[test]
    fn install_transaction_failure_rolls_back_central_links_and_settings_together() {
        let home = TestHome::new("ops-rollback");
        fs::create_dir_all(home.home.join(".claude")).unwrap();
        fs::create_dir_all(home.home.join("Library/Application Support/Cursor")).unwrap();
        let source = home.home.join("source/rollback-all");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: rollback-all\ndescription: Rollback fixture\n---\n",
        )
        .unwrap();
        let resolution = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let plan = plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id,
            skill_names: vec!["rollback-all".into()],
            agent_ids: vec!["claude-code".into(), "cursor".into()],
            replace_conflicts: false,
        })
        .unwrap();
        let paths = SkillsPaths::from_env().unwrap();
        let persisted = load_plan(&paths, &plan.operation_id).unwrap();
        let before_settings = current_settings_snapshot().unwrap();
        let spec = transaction_spec(&paths, &persisted).unwrap();
        let untouched_parent = home.home.join(".cursor/skills");
        assert!(!untouched_parent.exists());

        let error =
            execute_transaction_with_failpoint(spec, Some(Failpoint::AfterFirstLink)).unwrap_err();

        assert!(matches!(error, SkillError::Io { .. }));
        assert!(!paths.central_skill("rollback-all").exists());
        assert!(!home.home.join(".claude/skills/rollback-all").exists());
        assert!(!home.home.join(".cursor/skills/rollback-all").exists());
        assert!(!untouched_parent.exists());
        assert_eq!(current_settings_snapshot().unwrap(), before_settings);
        assert!(!paths.staging_skills_dir().join(&plan.operation_id).exists());
        assert!(paths.journals_skills_dir().is_dir());
        assert_eq!(fs::read_dir(paths.journals_skills_dir()).unwrap().count(), 0);
    }

    #[test]
    fn remove_transaction_failure_rolls_back_links_content_and_settings_together() {
        let home = TestHome::new("remove-rollback");
        fs::create_dir_all(home.home.join(".claude")).unwrap();
        fs::create_dir_all(home.home.join("Library/Application Support/Cursor")).unwrap();
        let source = home.home.join("source/remove-rollback");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: remove-rollback\ndescription: Remove rollback fixture\n---\n",
        )
        .unwrap();
        let resolution = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let install = plan_install(PlanInstallRequest {
            resolution_id: resolution.operation_id,
            skill_names: vec!["remove-rollback".into()],
            agent_ids: vec!["claude-code".into(), "cursor".into()],
            replace_conflicts: false,
        })
        .unwrap();
        commit_install(install.confirmation()).unwrap();

        let plan = plan_remove(PlanRemoveRequest {
            skill_name: "remove-rollback".into(),
        })
        .unwrap();
        let paths = SkillsPaths::from_env().unwrap();
        let persisted = load_plan(&paths, &plan.operation_id).unwrap();
        let before_settings = current_settings_snapshot().unwrap();
        let before_hash = hash_tree(&paths.central_skill("remove-rollback")).unwrap();
        let spec = transaction_spec(&paths, &persisted).unwrap();

        let error =
            execute_transaction_with_failpoint(spec, Some(Failpoint::AfterFirstLink)).unwrap_err();

        assert!(matches!(error, SkillError::Io { .. }));
        assert_eq!(
            hash_tree(&paths.central_skill("remove-rollback")).unwrap(),
            before_hash
        );
        for target in [
            home.home.join(".claude/skills/remove-rollback"),
            home.home.join(".cursor/skills/remove-rollback"),
        ] {
            assert!(fs::symlink_metadata(target)
                .unwrap()
                .file_type()
                .is_symlink());
        }
        assert_eq!(current_settings_snapshot().unwrap(), before_settings);
        assert!(!paths.staging_skills_dir().join(&plan.operation_id).exists());
        assert!(paths.journals_skills_dir().is_dir());
        assert_eq!(fs::read_dir(paths.journals_skills_dir()).unwrap().count(), 0);
    }

    #[test]
    fn github_central_repair_preserves_a_known_newer_update() {
        let home = TestHome::new("repair-preserves-update");
        let source = home.home.join("source/repair-preserves-update");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: repair-preserves-update\ndescription: Repair update fixture\n---\n",
        )
        .unwrap();
        let installed = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let install = plan_install(PlanInstallRequest {
            resolution_id: installed.operation_id,
            skill_names: vec!["repair-preserves-update".into()],
            agent_ids: Vec::new(),
            replace_conflicts: false,
        })
        .unwrap();
        commit_install(install.confirmation()).unwrap();

        let old_sha = "1111111111111111111111111111111111111111";
        let new_sha = "2222222222222222222222222222222222222222";
        let github_source = SkillSource::Github {
            owner: "acme".into(),
            repo: "skills".into(),
            subpath: "repair-preserves-update".into(),
            requested_ref: "main".into(),
            pinned: false,
        };
        crate::settings::mutate_settings(|settings| {
            let record = settings
                .managed_skills
                .as_mut()
                .unwrap()
                .get_mut("repair-preserves-update")
                .unwrap();
            record.source = github_source.clone();
            record.resolved_revision = Some(old_sha.into());
            record.update.available = true;
            record.update.resolved_revision = Some(new_sha.into());
            record.update.checked_at = Some("2026-07-17T08:00:00Z".into());
            record.update.etag = Some("\"newer\"".into());
        })
        .unwrap();
        let paths = SkillsPaths::from_env().unwrap();
        fs::remove_dir_all(paths.central_skill("repair-preserves-update")).unwrap();

        let mut resolution = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        resolution.source = github_source;
        resolution.resolved_revision = Some(old_sha.into());
        let persisted = build_central_repair_plan(
            PlanRepairRequest {
                skill_name: "repair-preserves-update".into(),
                repair: RepairKind::Central,
            },
            resolution,
            "~/.arca/backups/skills/repair-test/repair-preserves-update".into(),
            false,
        )
        .unwrap();
        let mut settings = current_settings_snapshot().unwrap();
        let expected = settings.managed_skills.as_ref().unwrap()["repair-preserves-update"]
            .update
            .clone();

        replacement_settings_after(&paths, &persisted, &mut settings).unwrap();

        assert_eq!(
            settings.managed_skills.unwrap()["repair-preserves-update"].update,
            expected
        );
    }

    #[test]
    fn github_update_plan_rejects_a_checked_revision_that_advanced_while_staging() {
        let home = TestHome::new("update-advanced-while-staging");
        let installed_source = home.home.join("source/installed/review-changes");
        fs::create_dir_all(&installed_source).unwrap();
        fs::write(
            installed_source.join("SKILL.md"),
            "---\nname: review-changes\ndescription: Installed fixture\n---\n",
        )
        .unwrap();
        let installed = resolve_source(
            SkillSourceInput::Local {
                path: installed_source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let install = plan_install(PlanInstallRequest {
            resolution_id: installed.operation_id,
            skill_names: vec!["review-changes".into()],
            agent_ids: Vec::new(),
            replace_conflicts: false,
        })
        .unwrap();
        commit_install(install.confirmation()).unwrap();

        let staged_source = home.home.join("source/staged/review-changes");
        fs::create_dir_all(&staged_source).unwrap();
        fs::write(
            staged_source.join("SKILL.md"),
            "---\nname: review-changes\ndescription: Staged fixture\n---\n",
        )
        .unwrap();
        let mut resolution = resolve_source(
            SkillSourceInput::Local {
                path: staged_source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        let old_sha = "1111111111111111111111111111111111111111";
        let reviewed_sha = "2222222222222222222222222222222222222222";
        let advanced_sha = "3333333333333333333333333333333333333333";
        let github_source = SkillSource::Github {
            owner: "acme".into(),
            repo: "skills".into(),
            subpath: "catalog/review-changes".into(),
            requested_ref: "main".into(),
            pinned: false,
        };
        resolution.source = github_source.clone();
        resolution.resolved_revision = Some(reviewed_sha.into());
        crate::settings::mutate_settings(|settings| {
            let record = settings
                .managed_skills
                .as_mut()
                .unwrap()
                .get_mut("review-changes")
                .unwrap();
            record.source = github_source;
            record.resolved_revision = Some(old_sha.into());
            record.update.available = true;
            record.update.resolved_revision = Some(advanced_sha.into());
        })
        .unwrap();

        assert!(matches!(
            build_update_plan(
                PlanUpdateRequest {
                    skill_name: "review-changes".into(),
                    replace_local_changes: false,
                },
                resolution,
                "~/.arca/backups/skills/update-test/review-changes".into(),
            ),
            Err(SkillError::PlanStale { .. }) | Err(SkillError::Conflict { .. })
        ));
    }
}
