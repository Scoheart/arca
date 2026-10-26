//! One recovery-aware startup path for every Arca frontend.
//!
//! Startup recovers Arca-owned transactions and upgrades central metadata. It
//! never treats Agent files as migration input and never rewrites them. Agent
//! state is observed after startup through the regular inventory pipeline.

use super::gate::{BackendStatus, CapabilityDomain};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapStage {
    BrandMigration,
    GlobalWriteRecovery,
    SkillRecovery,
    AssetRecovery,
    SettingsMigration,
    AssetLayoutMigration,
    McpRegistryMigration,
    ModelProfileMigration,
    ModelProviderMigration,
}

impl BootstrapStage {
    pub fn code(self) -> &'static str {
        match self {
            Self::BrandMigration => "brand_migration",
            Self::GlobalWriteRecovery => "global_write_recovery",
            Self::SkillRecovery => "skill_recovery",
            Self::AssetRecovery => "asset_recovery",
            Self::SettingsMigration => "settings_migration",
            Self::AssetLayoutMigration => "asset_layout_migration",
            Self::McpRegistryMigration => "mcp_registry_migration",
            Self::ModelProfileMigration => "model_profile_migration",
            Self::ModelProviderMigration => "model_provider_migration",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapWarning {
    pub stage: BootstrapStage,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapReport {
    pub warnings: Vec<BootstrapWarning>,
    pub skill_updates_allowed: bool,
    pub status: BackendStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapError {
    pub stage: BootstrapStage,
    pub message: String,
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.stage.code(), self.message)
    }
}

impl std::error::Error for BootstrapError {}

enum BootstrapProgress {
    Ready {
        warnings: Vec<BootstrapWarning>,
        skill_updates_allowed: bool,
    },
    ModelCentralStateUnavailable {
        error: BootstrapError,
        warnings: Vec<BootstrapWarning>,
        skill_updates_allowed: bool,
    },
}

/// Recover incomplete Arca writes and migrate only Arca-owned central metadata.
/// External Agent edits are not errors and are deliberately absent from this
/// path; they become relationship observations after bootstrap.
pub fn bootstrap() -> BootstrapReport {
    let permit = super::gate::begin_bootstrap();
    if let Err(reason) = crate::brand::validate_roots() {
        let error = BootstrapError { stage: BootstrapStage::BrandMigration, message: reason.to_string() };
        let status = read_only_status(&error);
        permit.finish(status.clone());
        return degraded_report(error, status);
    }
    let _cross_process_guard = match super::gate::acquire_cross_process_mutation_lock() {
        Ok(guard) => guard,
        Err(_) => {
            let error = BootstrapError {
                stage: BootstrapStage::GlobalWriteRecovery,
                message: "failed to acquire the cross-process mutation lock".into(),
            };
            let status = read_only_status(&error);
            permit.finish(status.clone());
            return degraded_report(error, status);
        }
    };

    match bootstrap_unlocked() {
        Ok(BootstrapProgress::Ready {
            warnings,
            skill_updates_allowed,
        }) => {
            if skill_updates_allowed && warnings.is_empty() {
                if let Err(reason) = crate::brand::migrate_home() {
                    let error = BootstrapError { stage: BootstrapStage::BrandMigration, message: reason.to_string() };
                    let status = read_only_status(&error);
                    permit.finish(status.clone());
                    return degraded_report(error, status);
                }
            }
            let status = BackendStatus::Ready;
            permit.finish(status.clone());
            BootstrapReport {
                warnings,
                skill_updates_allowed,
                status,
            }
        }
        Ok(BootstrapProgress::ModelCentralStateUnavailable {
            error,
            mut warnings,
            skill_updates_allowed,
        }) => {
            let status = model_central_state_status(&error);
            permit.finish(status.clone());
            warnings.push(BootstrapWarning {
                stage: error.stage,
                message: error.message,
            });
            BootstrapReport {
                warnings,
                skill_updates_allowed,
                status,
            }
        }
        Err(error) => {
            let status = read_only_status(&error);
            permit.finish(status.clone());
            // Bootstrap failures constrain writes through BackendStatus, but
            // they must not prevent read-only Agent and asset queries from
            // starting. This is especially important for a CLI process that
            // races a live Desktop read lock: the command can still report all
            // healthy capabilities and the exact degraded stage.
            degraded_report(error, status)
        }
    }
}

fn bootstrap_unlocked() -> Result<BootstrapProgress, BootstrapError> {
    run_steps(vec![(
        BootstrapStage::GlobalWriteRecovery,
        Box::new(crate::safe_write::recover_global_mutation_intents),
    )])?;

    let mut warnings = Vec::new();
    let skill_updates_allowed =
        classify_skill_recovery(crate::resources::skill::recover_pending(), &mut warnings)?;

    if let Err(message) = crate::assets::recover_pending_asset_operations() {
        warnings.push(BootstrapWarning {
            stage: BootstrapStage::AssetRecovery,
            message,
        });
    }

    run_steps(vec![(
        BootstrapStage::SettingsMigration,
        Box::new(|| {
            crate::settings::migrate_if_needed()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }),
    )])?;

    for (stage, operation) in [
        (
            BootstrapStage::ModelProfileMigration,
            crate::assets::migrate_model_profiles_v2_if_needed as fn() -> Result<bool, String>,
        ),
        (
            BootstrapStage::ModelProviderMigration,
            crate::resources::model::migrate_model_providers_v3_if_needed,
        ),
    ] {
        if let Err(message) = operation() {
            let error = BootstrapError { stage, message };
            return if model_failure_requires_global_recovery(&error.message) {
                Err(error)
            } else {
                Ok(BootstrapProgress::ModelCentralStateUnavailable {
                    error,
                    warnings,
                    skill_updates_allowed,
                })
            };
        }
    }

    run_steps(vec![
        (
            BootstrapStage::AssetLayoutMigration,
            Box::new(|| {
                crate::settings::migrate_asset_layout_if_needed()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }),
        ),
        (
            BootstrapStage::McpRegistryMigration,
            Box::new(|| {
                crate::resources::mcp::registry::migrate_registry_to_sources()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }),
        ),
    ])?;
    Ok(BootstrapProgress::Ready {
        warnings,
        skill_updates_allowed,
    })
}

fn classify_skill_recovery(
    result: Result<(), crate::resources::skill::SkillError>,
    warnings: &mut Vec<BootstrapWarning>,
) -> Result<bool, BootstrapError> {
    match result {
        Ok(()) => Ok(true),
        Err(crate::resources::skill::SkillError::Conflict { .. }) => {
            warnings.push(BootstrapWarning {
                stage: BootstrapStage::SkillRecovery,
                message: "another Arca process is reading or updating Skills; unrelated capabilities remain available"
                    .into(),
            });
            Ok(false)
        }
        Err(error) => {
            warnings.push(BootstrapWarning {
                stage: BootstrapStage::SkillRecovery,
                message: error.into_command_parts().message,
            });
            Ok(false)
        }
    }
}

fn model_failure_requires_global_recovery(message: &str) -> bool {
    message == "recovery_required"
        || message.starts_with("recovery_required:")
        || message.contains("operation committed but")
}

fn model_central_state_status(error: &BootstrapError) -> BackendStatus {
    BackendStatus::CapabilityUnavailable {
        capability: CapabilityDomain::Model,
        stage: error.stage.code().into(),
        code: "model_central_state_unavailable".into(),
        message: "Arca central Model metadata could not be upgraded safely; Agent files were left untouched"
            .into(),
    }
}

fn read_only_status(error: &BootstrapError) -> BackendStatus {
    BackendStatus::ReadOnly {
        stage: error.stage.code().into(),
        message: error.message.clone(),
    }
}

type BootstrapStep<'a> = (BootstrapStage, Box<dyn FnOnce() -> Result<(), String> + 'a>);

fn run_steps(steps: Vec<BootstrapStep<'_>>) -> Result<(), BootstrapError> {
    for (stage, operation) in steps {
        operation().map_err(|message| BootstrapError { stage, message })?;
    }
    Ok(())
}

fn degraded_report(error: BootstrapError, status: BackendStatus) -> BootstrapReport {
    BootstrapReport {
        warnings: vec![BootstrapWarning {
            stage: error.stage,
            message: error.message,
        }],
        skill_updates_allowed: false,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assets::{ModelAgentSelection, ModelConsumptionRecord};
    use crate::domain::types::{ModelProfile, ModelProtocol};
    use crate::settings::{load_settings_strict, mutate_settings};
    use crate::testenv::TestHome;
    use std::cell::RefCell;
    use std::fs;

    fn failure(stage: BootstrapStage) -> BootstrapError {
        BootstrapError {
            stage,
            message: "broken journal".into(),
        }
    }

    fn legacy_profile() -> ModelProfile {
        ModelProfile {
            id: "legacy-profile".into(),
            provider_id: None,
            name: "Legacy".into(),
            provider: "custom".into(),
            model_vendor: Some("openai".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: "https://example.invalid/v1".into(),
            endpoint_path: String::new(),
            model: "gpt-external".into(),
            env_key: Some("EXAMPLE_API_KEY".into()),
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
        }
    }

    #[test]
    fn cli_recovery_keeps_queries_available_and_publishes_a_write_blocker() {
        let error = failure(BootstrapStage::AssetRecovery);
        let report = degraded_report(
            error,
            BackendStatus::ReadOnly {
                stage: "asset_recovery".into(),
                message: "broken journal".into(),
            },
        );
        assert!(!report.skill_updates_allowed);
        assert_eq!(report.warnings[0].stage, BootstrapStage::AssetRecovery);
        assert!(matches!(report.status, BackendStatus::ReadOnly { .. }));
    }

    #[test]
    fn desktop_failure_is_diagnostic_but_read_only() {
        let error = failure(BootstrapStage::SkillRecovery);
        let report = degraded_report(
            error,
            BackendStatus::ReadOnly {
                stage: "skill_recovery".into(),
                message: "broken journal".into(),
            },
        );
        assert!(!report.skill_updates_allowed);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn skill_lock_contention_is_degraded_not_recovery_damage() {
        let mut warnings = Vec::new();
        let allowed = classify_skill_recovery(
            Err(crate::resources::skill::SkillError::Conflict {
                message: "another Skills operation is still running".into(),
                path: String::new(),
            }),
            &mut warnings,
        )
        .unwrap();

        assert!(!allowed);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].stage, BootstrapStage::SkillRecovery);
    }

    #[test]
    fn ordered_stages_stop_at_the_first_failure() {
        let executed = RefCell::new(Vec::new());
        let result = run_steps(vec![
            (
                BootstrapStage::GlobalWriteRecovery,
                Box::new(|| {
                    executed.borrow_mut().push("global");
                    Ok(())
                }),
            ),
            (
                BootstrapStage::SkillRecovery,
                Box::new(|| {
                    executed.borrow_mut().push("skill");
                    Err("broken journal".into())
                }),
            ),
            (
                BootstrapStage::AssetRecovery,
                Box::new(|| {
                    executed.borrow_mut().push("asset");
                    Ok(())
                }),
            ),
        ]);
        assert_eq!(result.unwrap_err().stage, BootstrapStage::SkillRecovery);
        assert_eq!(*executed.borrow(), ["global", "skill"]);
    }

    #[test]
    fn model_schema_upgrade_never_rewrites_external_agent_state() {
        let home = TestHome::new("bootstrap-observed-model");
        let profile = legacy_profile();
        mutate_settings(|settings| {
            settings.version = Some(1);
            settings
                .model_profiles
                .get_or_insert_default()
                .insert(profile.id.clone(), profile.clone());
            settings.set_model_selection(
                "grok-build",
                ModelAgentSelection {
                    profiles: std::collections::BTreeMap::from([(
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
        let target = home.home.join(".grok/config.toml");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let external = b"[models]\ndefault = 'external'\n";
        fs::write(&target, external).unwrap();

        let report = bootstrap();

        assert!(matches!(report.status, BackendStatus::Ready));
        assert_eq!(fs::read(&target).unwrap(), external);
        assert_eq!(
            load_settings_strict().unwrap().version,
            Some(crate::settings::SETTINGS_VERSION)
        );
    }
}
