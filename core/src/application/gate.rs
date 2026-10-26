//! Process-wide consistency and backend-readiness gate.
//!
//! Domain engines retain their own narrow locks for crash safety. This gate
//! coordinates frontend-facing operations in one process. Shared recovery
//! uncertainty remains a hard write boundary, while a capability-local Model,
//! MCP, or Skill blocker disables only that domain.

use crate::domain::error::{CoreError, CoreResult};
use crate::resources::skill::SkillError;
use crate::safe_write::{acquire_settings_lock, SettingsLock};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDomain {
    Mcp,
    Model,
    Skill,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BackendStatus {
    Starting,
    Ready,
    CapabilityUnavailable {
        capability: CapabilityDomain,
        stage: String,
        code: String,
        message: String,
    },
    ReadOnly {
        stage: String,
        message: String,
    },
}

impl BackendStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

static WORKSPACE_GATE: RwLock<()> = RwLock::new(());
static BACKEND_STATUS: RwLock<BackendStatus> = RwLock::new(BackendStatus::Starting);
static TEST_LIFECYCLE_GATE: Mutex<()> = Mutex::new(());

/// Read-only operations are deliberately available in every backend state.
pub(crate) fn query<T>(operation: impl FnOnce() -> T) -> T {
    let _guard = WORKSPACE_GATE
        .read()
        .unwrap_or_else(|error| error.into_inner());
    operation()
}

/// Compatibility name for existing query services.
pub(crate) fn read<T>(operation: impl FnOnce() -> T) -> T {
    query(operation)
}

/// A shared preparation may write private staging across capability domains.
/// Domain-specific callers should use [`prepare_for`] so a local blocker does
/// not unnecessarily disable unrelated capabilities.
pub(crate) fn prepare<R: GatedResult>(stage: &'static str, operation: impl FnOnce() -> R) -> R {
    mutate_scoped(MutationScope::Shared, stage, operation)
}

pub(crate) fn prepare_for<R: GatedResult>(
    capability: CapabilityDomain,
    stage: &'static str,
    operation: impl FnOnce() -> R,
) -> R {
    mutate_scoped(MutationScope::Capability(capability), stage, operation)
}

/// Existing application writers enter through this compatibility name. The
/// generic result adapter preserves their public error types while enforcing
/// one readiness policy for all domains.
pub(crate) fn write<R: GatedResult>(operation: impl FnOnce() -> R) -> R {
    mutate_scoped(MutationScope::Shared, "application_mutation", operation)
}

pub(crate) fn write_for<R: GatedResult>(
    capability: CapabilityDomain,
    operation: impl FnOnce() -> R,
) -> R {
    mutate_scoped(
        MutationScope::Capability(capability),
        "application_mutation",
        operation,
    )
}

/// A preference or host-integration write that does not consume MCP, Model,
/// or Skill state. Target-scoped incidents never apply to this scope.
pub(crate) fn write_independent<R: GatedResult>(operation: impl FnOnce() -> R) -> R {
    mutate_scoped(
        MutationScope::Independent,
        "application_mutation",
        operation,
    )
}

pub(crate) fn mutate<R: GatedResult>(stage: &'static str, operation: impl FnOnce() -> R) -> R {
    mutate_scoped(MutationScope::Shared, stage, operation)
}

pub(crate) fn mutate_for<R: GatedResult>(
    capability: CapabilityDomain,
    stage: &'static str,
    operation: impl FnOnce() -> R,
) -> R {
    mutate_scoped(MutationScope::Capability(capability), stage, operation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MutationScope {
    Independent,
    Shared,
    Capability(CapabilityDomain),
}

fn mutate_scoped<R: GatedResult>(
    scope: MutationScope,
    stage: &'static str,
    operation: impl FnOnce() -> R,
) -> R {
    let _guard = WORKSPACE_GATE
        .write()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(error) = blocker_for_status(&current_status(), scope) {
        return R::blocked(error);
    }
    let _cross_process_guard = match acquire_cross_process_mutation_lock() {
        Ok(guard) => guard,
        Err(_) => return R::blocked(mutation_coordination_error()),
    };
    if let Some(error) = mutation_blocker(scope) {
        return R::blocked(error);
    }

    let result = operation();
    if let Some(message) = result.recovery_required() {
        latch_read_only(stage, message);
    }
    result
}

#[cfg(test)]
pub(crate) fn mutate_core<T>(
    stage: &'static str,
    operation: impl FnOnce() -> CoreResult<T>,
) -> CoreResult<T> {
    mutate(stage, operation)
}

pub(crate) fn mutate_independent_core<T>(
    stage: &'static str,
    operation: impl FnOnce() -> CoreResult<T>,
) -> CoreResult<T> {
    mutate_scoped(MutationScope::Independent, stage, operation)
}

pub fn status() -> BackendStatus {
    let _guard = WORKSPACE_GATE
        .read()
        .unwrap_or_else(|error| error.into_inner());
    current_status()
}

/// Startup is the only privileged writer allowed while the backend is
/// `Starting` or `ReadOnly`. The permit holds the exclusive gate across every
/// recovery and migration stage.
pub(crate) fn begin_bootstrap() -> BootstrapPermit {
    let guard = WORKSPACE_GATE
        .write()
        .unwrap_or_else(|error| error.into_inner());
    set_status(BackendStatus::Starting);
    BootstrapPermit {
        _guard: guard,
        completed: false,
    }
}

pub(crate) struct BootstrapPermit {
    _guard: RwLockWriteGuard<'static, ()>,
    completed: bool,
}

impl BootstrapPermit {
    pub(crate) fn finish(mut self, status: BackendStatus) {
        debug_assert!(!matches!(status, BackendStatus::Starting));
        set_status(status);
        self.completed = true;
    }
}

impl Drop for BootstrapPermit {
    fn drop(&mut self) {
        if !self.completed {
            set_status(BackendStatus::ReadOnly {
                stage: "bootstrap".into(),
                message: "backend bootstrap ended before publishing a final state".into(),
            });
        }
    }
}

fn current_status() -> BackendStatus {
    BACKEND_STATUS
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

fn set_status(status: BackendStatus) {
    *BACKEND_STATUS
        .write()
        .unwrap_or_else(|error| error.into_inner()) = status;
}

fn latch_read_only(stage: &str, message: String) {
    let mut status = BACKEND_STATUS
        .write()
        .unwrap_or_else(|error| error.into_inner());
    *status = latched_status(&status, stage, message);
}

fn mutation_blocker(scope: MutationScope) -> Option<CoreError> {
    let status = current_status();
    if let Some(error) = blocker_for_status(&status, scope) {
        return Some(error);
    }
    let pending = crate::safe_write::pending_global_mutation_error()?;
    latch_read_only("durable_recovery", pending);
    blocker_for_status(&current_status(), scope)
}

pub(crate) fn acquire_cross_process_mutation_lock() -> Result<SettingsLock, String> {
    acquire_settings_lock(
        &crate::paths::arca_dir()
            .join("locks")
            .join("application-mutation"),
    )
}

fn mutation_coordination_error() -> CoreError {
    CoreError {
        code: "mutation_busy".into(),
        message: "another Arca process is mutating shared state; retry this operation".into(),
        details: BTreeMap::new(),
        retry_at: None,
        confirmation: None,
    }
}

fn latched_status(current: &BackendStatus, stage: &str, message: String) -> BackendStatus {
    match current {
        BackendStatus::ReadOnly { .. } => current.clone(),
        BackendStatus::Starting
        | BackendStatus::Ready
        | BackendStatus::CapabilityUnavailable { .. } => BackendStatus::ReadOnly {
            stage: stage.into(),
            message,
        },
    }
}

/// TestHome already serializes the process-wide HOME/ARCA_HOME environment for
/// its entire lifetime. Use that same scope to make application mutations
/// explicitly ready and restore the prior lifecycle state before releasing the
/// environment lock.
pub(crate) struct TestReadyGuard {
    previous: Option<BackendStatus>,
    _lifecycle_guard: MutexGuard<'static, ()>,
}

impl TestReadyGuard {
    pub(crate) fn enter() -> Self {
        Self::enter_status(BackendStatus::Ready)
    }

    fn enter_status(next: BackendStatus) -> Self {
        let lifecycle_guard = TEST_LIFECYCLE_GATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _guard = WORKSPACE_GATE
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let previous = current_status();
        set_status(next);
        Self {
            previous: Some(previous),
            _lifecycle_guard: lifecycle_guard,
        }
    }

    pub(crate) fn restore(mut self) {
        self.restore_inner();
    }

    fn restore_inner(&mut self) {
        let Some(previous) = self.previous.take() else {
            return;
        };
        let _guard = WORKSPACE_GATE
            .write()
            .unwrap_or_else(|error| error.into_inner());
        set_status(previous);
    }
}

impl Drop for TestReadyGuard {
    fn drop(&mut self) {
        self.restore_inner();
    }
}

fn blocker_for_status(status: &BackendStatus, scope: MutationScope) -> Option<CoreError> {
    match status {
        BackendStatus::Starting => {
            let mut details = BTreeMap::new();
            details.insert("backend_state".into(), Value::String("starting".into()));
            Some(CoreError {
                code: "backend_initializing".into(),
                message: "Arca backend startup has not completed".into(),
                details,
                retry_at: None,
                confirmation: None,
            })
        }
        BackendStatus::Ready => None,
        BackendStatus::CapabilityUnavailable {
            capability,
            stage,
            code,
            message,
        } if scope_is_blocked(scope, &[*capability]) => {
            let mut details = BTreeMap::new();
            details.insert(
                "backend_state".into(),
                Value::String("capability_unavailable".into()),
            );
            details.insert("capability".into(), serde_json::json!(capability));
            details.insert("stage".into(), Value::String(stage.clone()));
            Some(CoreError {
                code: code.clone(),
                message: message.clone(),
                details,
                retry_at: None,
                confirmation: None,
            })
        }
        BackendStatus::CapabilityUnavailable { .. } => None,
        BackendStatus::ReadOnly { stage, message } => {
            let mut details = BTreeMap::new();
            details.insert("backend_state".into(), Value::String("read_only".into()));
            details.insert("stage".into(), Value::String(stage.clone()));
            Some(CoreError {
                code: "recovery_required".into(),
                message: format!("Arca is read-only because {stage} failed: {message}"),
                details,
                retry_at: None,
                confirmation: None,
            })
        }
    }
}

fn scope_is_blocked(scope: MutationScope, blocked: &[CapabilityDomain]) -> bool {
    match scope {
        MutationScope::Independent => false,
        MutationScope::Shared => true,
        MutationScope::Capability(capability) => blocked.contains(&capability),
    }
}

pub(crate) trait GatedResult {
    fn blocked(error: CoreError) -> Self;
    fn recovery_required(&self) -> Option<String>;
}

impl<T> GatedResult for CoreResult<T> {
    fn blocked(error: CoreError) -> Self {
        Err(error)
    }

    fn recovery_required(&self) -> Option<String> {
        self.as_ref()
            .err()
            .filter(|error| error.code == "recovery_required")
            .map(|error| error.message.clone())
    }
}

impl<T> GatedResult for Result<T, String> {
    fn blocked(error: CoreError) -> Self {
        Err(format!("{}: {}", error.code, error.message))
    }

    fn recovery_required(&self) -> Option<String> {
        let error = self.as_ref().err()?;
        if error == "recovery_required" {
            return Some(error.clone());
        }
        if let Some(message) = error
            .strip_prefix("recovery_required:")
            .map(|message| message.trim().to_string())
        {
            return Some(message);
        }
        None
    }
}

impl<T> GatedResult for Result<T, SkillError> {
    fn blocked(error: CoreError) -> Self {
        if error.code == "recovery_required" {
            Err(SkillError::RecoveryRequired {
                message: error.message,
            })
        } else {
            Err(SkillError::Conflict {
                message: error.message,
                path: String::new(),
            })
        }
    }

    fn recovery_required(&self) -> Option<String> {
        match self {
            Err(SkillError::RecoveryRequired { message }) => Some(message.clone()),
            Err(_) | Ok(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn status_serialization_is_stable_and_tagged() {
        assert_eq!(
            serde_json::to_value(BackendStatus::Starting).unwrap(),
            serde_json::json!({"state": "starting"})
        );
        assert_eq!(
            serde_json::to_value(BackendStatus::ReadOnly {
                stage: "asset_recovery".into(),
                message: "broken journal".into(),
            })
            .unwrap(),
            serde_json::json!({
                "state": "read_only",
                "stage": "asset_recovery",
                "message": "broken journal",
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn cross_process_mutation_lock_never_follows_a_symlinked_lock_directory() {
        use std::os::unix::fs::symlink;

        let home = crate::testenv::TestHome::new("gate-lock-symlink");
        let arca = home.home.join(".arca");
        let outside = home.home.join("outside-locks");
        std::fs::create_dir(&arca).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, arca.join("locks")).unwrap();

        let Err(error) = acquire_cross_process_mutation_lock() else {
            panic!("symlinked application lock directory was accepted");
        };

        assert!(error.contains("unsafe"), "{error}");
        assert!(!outside.join("application-mutation.lockfile").exists());
    }

    #[test]
    fn starting_and_read_only_have_structured_blockers_but_ready_does_not() {
        let starting = blocker_for_status(&BackendStatus::Starting, MutationScope::Shared).unwrap();
        assert_eq!(starting.code, "backend_initializing");
        assert_eq!(starting.details["backend_state"], "starting");

        let read_only = blocker_for_status(
            &BackendStatus::ReadOnly {
                stage: "skill_recovery".into(),
                message: "unsafe journal".into(),
            },
            MutationScope::Shared,
        )
        .unwrap();
        assert_eq!(read_only.code, "recovery_required");
        assert_eq!(read_only.details["stage"], "skill_recovery");
        assert!(blocker_for_status(&BackendStatus::Ready, MutationScope::Shared).is_none());
    }

    #[test]
    fn model_blockers_do_not_disable_mcp_or_skill_mutations() {
        let unavailable = BackendStatus::CapabilityUnavailable {
            capability: CapabilityDomain::Model,
            stage: "model_profile_migration".into(),
            code: "migration_hard_blocked".into(),
            message: "repair Model target".into(),
        };
        assert!(blocker_for_status(
            &unavailable,
            MutationScope::Capability(CapabilityDomain::Model)
        )
        .is_some());
        assert!(blocker_for_status(
            &unavailable,
            MutationScope::Capability(CapabilityDomain::Mcp)
        )
        .is_none());
        assert!(blocker_for_status(&unavailable, MutationScope::Independent).is_none());
    }

    #[test]
    fn capability_status_executes_only_unrelated_and_unblocked_domain_writes() {
        let _home = crate::testenv::TestHome::new("gate-capability-isolation");
        set_status(BackendStatus::CapabilityUnavailable {
            capability: CapabilityDomain::Model,
            stage: "model_profile_migration".into(),
            code: "model_central_state_unavailable".into(),
            message: "central Model state is unavailable".into(),
        });

        let independent_called = Cell::new(false);
        let independent: Result<(), String> = write_independent(|| {
            independent_called.set(true);
            Ok(())
        });
        assert!(independent.is_ok());
        assert!(independent_called.get());

        let mcp_called = Cell::new(false);
        let mcp: Result<(), String> = write_for(CapabilityDomain::Mcp, || {
            mcp_called.set(true);
            Ok(())
        });
        assert!(mcp.is_ok());
        assert!(mcp_called.get());

        let model_called = Cell::new(false);
        let model: Result<(), String> = write_for(CapabilityDomain::Model, || {
            model_called.set(true);
            Ok(())
        });
        assert!(model.is_err());
        assert!(!model_called.get());

        let shared_called = Cell::new(false);
        let shared: Result<(), String> = write(|| {
            shared_called.set(true);
            Ok(())
        });
        assert!(shared.is_err());
        assert!(!shared_called.get());
    }

    #[test]
    fn model_credential_entrypoints_remain_available_during_unrelated_capability_failures() {
        let _home = crate::testenv::TestHome::new("gate-model-credential-isolation");
        for capability in [CapabilityDomain::Mcp, CapabilityDomain::Skill] {
            set_status(BackendStatus::CapabilityUnavailable {
                capability, stage: "unrelated_stage".into(), code: "unrelated_failure".into(),
                message: "unrelated capability unavailable".into(),
            });
            super::super::models::set_agent_credential_delivery(
                "opencode", crate::domain::assets::ApiKeyDelivery::Auto, false,
            ).unwrap();
            let error = super::super::models::set_credential_delivery(
                "opencode", "missing-profile", crate::domain::assets::ApiKeyDelivery::Auto, false,
            ).unwrap_err();
            assert!(error.starts_with("model_consumption_missing:"), "{error}");
            let plan = super::super::models::plan_credential_delivery(
                "opencode", None, crate::domain::assets::ApiKeyDelivery::Auto, false,
            ).unwrap();
            super::super::assets::cancel_asset_operation(&plan.operation_id).unwrap();
        }
    }

    #[test]
    fn every_supported_error_shape_identifies_recovery_required() {
        let core: CoreResult<()> = Err(CoreError::new("recovery_required", "core"));
        assert_eq!(core.recovery_required().as_deref(), Some("core"));

        let legacy: Result<(), String> = Err("recovery_required: legacy".into());
        assert_eq!(legacy.recovery_required().as_deref(), Some("legacy"));

        let skill: Result<(), SkillError> = Err(SkillError::RecoveryRequired {
            message: "skill".into(),
        });
        assert_eq!(skill.recovery_required().as_deref(), Some("skill"));
    }

    #[cfg(unix)]
    #[test]
    fn ordinary_skill_error_does_not_inherit_an_unrelated_global_claim() {
        let _home = crate::testenv::TestHome::new("gate-skill-claim");
        let _claim = crate::safe_write::install_test_global_mutation_claim().unwrap();
        let skill: Result<(), SkillError> = Err(SkillError::Conflict {
            message: "stale target".into(),
            path: String::new(),
        });

        assert!(skill.recovery_required().is_none());
    }

    #[test]
    fn recovery_latch_preserves_the_first_blocker() {
        let first = latched_status(
            &BackendStatus::Ready,
            "asset_commit",
            "rollback failed".into(),
        );
        assert_eq!(
            first,
            BackendStatus::ReadOnly {
                stage: "asset_commit".into(),
                message: "rollback failed".into(),
            }
        );
        assert_eq!(
            latched_status(&first, "later", "second failure".into()),
            first
        );
    }

    #[test]
    fn shared_recovery_supersedes_a_capability_only_blocker() {
        let review = BackendStatus::CapabilityUnavailable {
            capability: CapabilityDomain::Model,
            stage: "model_profile_migration".into(),
            code: "model_central_state_unavailable".into(),
            message: "central Model state is unavailable".into(),
        };
        assert_eq!(
            latched_status(&review, "asset_commit", "rollback failed".into()),
            BackendStatus::ReadOnly {
                stage: "asset_commit".into(),
                message: "rollback failed".into(),
            }
        );
    }

    #[test]
    fn skill_mutation_contention_remains_retryable_not_recovery_required() {
        let blocked =
            <Result<(), SkillError> as GatedResult>::blocked(mutation_coordination_error());

        assert!(matches!(blocked, Err(SkillError::Conflict { .. })));
    }

    #[test]
    fn read_only_allows_queries_but_never_runs_preparations_or_mutations() {
        let _status = TestReadyGuard::enter_status(BackendStatus::ReadOnly {
            stage: "asset_recovery".into(),
            message: "broken journal".into(),
        });
        let query_called = Cell::new(false);
        query(|| query_called.set(true));
        assert!(query_called.get());

        let prepare_called = Cell::new(false);
        let prepare_result: Result<(), String> = prepare("asset_plan", || {
            prepare_called.set(true);
            Ok(())
        });
        assert!(!prepare_called.get());
        assert!(prepare_result
            .unwrap_err()
            .starts_with("recovery_required:"));

        let mutation_called = Cell::new(false);
        let mutation_result: CoreResult<()> = mutate_core("test_mutation", || {
            mutation_called.set(true);
            Ok(())
        });
        assert!(!mutation_called.get());
        assert_eq!(mutation_result.unwrap_err().code, "recovery_required");

        let independent_called = Cell::new(false);
        let independent_result: Result<(), String> = write_independent(|| {
            independent_called.set(true);
            Ok(())
        });
        assert!(!independent_called.get());
        assert!(independent_result
            .unwrap_err()
            .starts_with("recovery_required:"));
    }

    #[test]
    fn runtime_recovery_error_latches_backend_read_only() {
        let _home = crate::testenv::TestHome::new("gate-runtime-recovery");
        let result: CoreResult<()> = mutate_core("asset_commit", || {
            Err(CoreError::new("recovery_required", "rollback failed"))
        });
        assert_eq!(result.unwrap_err().code, "recovery_required");
        assert_eq!(
            status(),
            BackendStatus::ReadOnly {
                stage: "asset_commit".into(),
                message: "rollback failed".into(),
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn pending_global_claim_blocks_the_operation_and_latches_read_only() {
        let _home = crate::testenv::TestHome::new("gate-global-claim");
        let _claim = crate::safe_write::install_test_global_mutation_claim().unwrap();
        let called = Cell::new(false);

        let result: CoreResult<()> = mutate_core("test_mutation", || {
            called.set(true);
            Ok(())
        });

        assert!(!called.get());
        assert_eq!(result.unwrap_err().code, "recovery_required");
        assert!(matches!(
            status(),
            BackendStatus::ReadOnly { ref stage, .. } if stage == "durable_recovery"
        ));
    }

    #[test]
    fn pending_asset_manifest_does_not_block_unrelated_application_mutation() {
        let _home = crate::testenv::TestHome::new("gate-asset-recovery");
        let foreign_id = uuid::Uuid::new_v4().to_string();
        let rollback = crate::assets::planner::operation_root(&foreign_id).join("rollback");
        std::fs::create_dir_all(&rollback).unwrap();
        std::fs::write(rollback.join("manifest.json"), b"{}").unwrap();
        let called = Cell::new(false);

        let result: CoreResult<()> = mutate_core("unrelated_mutation", || {
            called.set(true);
            Ok(())
        });

        assert!(called.get());
        assert!(result.is_ok());
        assert_eq!(status(), BackendStatus::Ready);
    }

    #[test]
    fn starting_blocks_mutations_without_running_them() {
        let _status = TestReadyGuard::enter_status(BackendStatus::Starting);
        let called = Cell::new(false);
        let result: CoreResult<()> = mutate_core("test_mutation", || {
            called.set(true);
            Ok(())
        });
        assert!(!called.get());
        assert_eq!(result.unwrap_err().code, "backend_initializing");
    }
}
