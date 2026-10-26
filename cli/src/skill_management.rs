//! Central Skill lifecycle commands over the shared Core plan/commit API.

use clap::{Args, Subcommand};
use arca_core::application::operations::PlanOperationRequest;
use arca_core::application::skills::{
    self, GithubEndpoints, PlanRemoveRequest, PlanRepairRequest, PlanSkillAssetImportRequest,
    PlanSkillAssetInstallRequest, PlanUpdateRequest, RepairKind, SkillSourceInput,
};
use arca_core::application::ArcaCore;
use serde_json::json;

use crate::command::{parse_identity, skill_error, Cli};
use crate::output::{CliError, CommandOutput};
use crate::review::{execute_direct_mutation, execute_operation};

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct SourceArgs {
    /// GitHub repository or tree URL understood by Core's source resolver.
    #[arg(long)]
    github: Option<String>,
    /// Local Skill or repository directory.
    #[arg(long)]
    path: Option<String>,
    /// Local supported archive.
    #[arg(long)]
    archive: Option<String>,
}

impl SourceArgs {
    fn input(&self) -> Result<SkillSourceInput, CliError> {
        match (&self.github, &self.path, &self.archive) {
            (Some(value), None, None) => Ok(SkillSourceInput::Github { value: value.clone() }),
            (None, Some(path), None) => Ok(SkillSourceInput::Local { path: path.clone() }),
            (None, None, Some(path)) => Ok(SkillSourceInput::Archive { path: path.clone() }),
            _ => Err(CliError::new("invalid_source", "choose exactly one of --github, --path or --archive")),
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum SkillManagementCommand {
    /// Inspect installable candidates; temporary source staging is cancelled before returning.
    InspectSource {
        #[command(flatten)]
        source: SourceArgs,
    },
    /// Install explicitly selected Skills into the central library; assignment is separate.
    Install {
        #[command(flatten)]
        source: SourceArgs,
        /// Exact candidate name from inspect-source. Repeat to select multiple Skills.
        #[arg(long = "name", required = true, value_parser = parse_identity)]
        names: Vec<String>,
        #[arg(long)]
        replace_conflicts: bool,
    },
    /// Import an external Skill identity from `arca discover skill` into the central library.
    Import {
        #[arg(value_parser = parse_identity)]
        identity: String,
        #[arg(long)]
        replace_conflicts: bool,
    },
    /// Update one central Skill from its recorded source.
    Update {
        #[arg(value_parser = parse_identity)]
        name: String,
        #[arg(long)]
        replace_local_changes: bool,
    },
    /// Remove a central Skill and its managed assignments.
    Remove {
        #[arg(value_parser = parse_identity)]
        name: String,
    },
    /// Repair a central Skill, or one shared physical target with --target.
    Repair {
        #[arg(value_parser = parse_identity)]
        name: String,
        #[arg(long, value_parser = parse_identity)]
        target: Option<String>,
    },
    /// Check upstream revisions and refresh Arca's update cache; does not install updates.
    CheckUpdates,
}

pub fn dispatch(cli: &Cli, command: &SkillManagementCommand) -> Result<CommandOutput, CliError> {
    let options = cli.mutation_options();
    if let SkillManagementCommand::InspectSource { source } = command {
        cli.reject_mutation_options()?;
        let resolution = skills::resolve_source(source.input()?, GithubEndpoints::production())
            .map_err(skill_error)?;
        skills::cancel_operation(&resolution.operation_id).map_err(skill_error)?;
        let data = json!({"resolved_revision": resolution.resolved_revision, "candidates": resolution.candidates});
        let human = resolution.candidates.iter().map(|candidate| {
            format!("{}\n  {}\n  {} files, {} bytes", candidate.name, candidate.description,
                candidate.file_count, candidate.total_bytes)
        }).collect::<Vec<_>>().join("\n");
        return Ok(CommandOutput::new("skill.inspect-source", false, data, human));
    }
    options.validate()?;
    let (name, request) = match command {
        SkillManagementCommand::Install { source, names, replace_conflicts } => {
            let resolution = skills::resolve_source(source.input()?, GithubEndpoints::production())
                .map_err(skill_error)?;
            let plan = ArcaCore::plan(PlanOperationRequest::InstallSkill(PlanSkillAssetInstallRequest {
                resolution_id: resolution.operation_id.clone(), skill_names: names.clone(),
                replace_conflicts: *replace_conflicts,
            }));
            let plan = match plan {
                Ok(plan) => plan,
                Err(error) => {
                    let mut error = CliError::from_core(error);
                    if let Err(cleanup) = skills::cancel_operation(&resolution.operation_id) {
                        error = error.with_detail("cleanup_error", skill_error(cleanup).code);
                    }
                    return Err(error);
                }
            };
            return execute_operation("skill.install", plan, options);
        }
        SkillManagementCommand::Import { identity, replace_conflicts } => (
            "skill.import", PlanOperationRequest::ImportSkill(PlanSkillAssetImportRequest {
                identity: identity.clone(), replace_conflicts: *replace_conflicts,
            }),
        ),
        SkillManagementCommand::Update { name, replace_local_changes } => (
            "skill.update", PlanOperationRequest::UpdateSkill(PlanUpdateRequest {
                skill_name: name.clone(), replace_local_changes: *replace_local_changes,
            }),
        ),
        SkillManagementCommand::Remove { name } => (
            "skill.remove", PlanOperationRequest::RemoveSkill(PlanRemoveRequest { skill_name: name.clone() }),
        ),
        SkillManagementCommand::Repair { name, target } => (
            "skill.repair", PlanOperationRequest::RepairSkill(PlanRepairRequest {
                skill_name: name.clone(),
                repair: target.as_ref().map(|id| RepairKind::Target { target_id: id.clone() }).unwrap_or(RepairKind::Central),
            }),
        ),
        SkillManagementCommand::CheckUpdates => {
            let mut outcome = None;
            let mut output = execute_direct_mutation(
                "skill.check-updates", options, json!({"refresh_update_cache": true}),
                "Check Skill source revisions and refresh the update cache. Installed Skills remain at their current revision.",
                false, || {
                    outcome = Some(skills::check_updates(true).map_err(skill_error)?);
                    Ok(())
                },
            )?;
            if let Some(outcome) = outcome {
                output.data["result"] = json!({
                    "performed": outcome.performed, "checked": outcome.checked,
                    "available": outcome.available, "skipped_pinned": outcome.skipped_pinned,
                    "failed_skills": outcome.errors.keys().collect::<Vec<_>>(),
                    "checked_at": outcome.checked_at,
                });
                output.human = format!("Checked {} Skills; {} updates available; {} checks failed.\n{}",
                    outcome.checked, outcome.available.len(), outcome.errors.len(), outcome.available.join("\n"));
            }
            return Ok(output);
        }
        SkillManagementCommand::InspectSource { .. } => unreachable!(),
    };
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    execute_operation(name, plan, options)
}
