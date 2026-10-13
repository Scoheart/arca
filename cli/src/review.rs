use std::collections::BTreeSet;
use std::io::{self, IsTerminal, Write};

use arca_core::application::assets::{AssetCommitRequest, AssetOperationPlan, DomainPlan};
use arca_core::application::operations::{
    CancelOperationRequest, CommitOperationRequest, OperationCommitResult, OperationPlan,
};
use arca_core::application::skills::SkillCommitRequest;
use arca_core::application::ArcaCore;
use serde_json::{json, Value};

use crate::output::{CliError, CommandOutput, Palette};
use crate::projection::{safe_consumption_inventory, safe_path, safe_skill_inventory, safe_target_incident};

#[derive(Debug, Clone, Copy)]
pub struct MutationOptions {
    pub json: bool,
    pub yes: bool,
    pub dry_run: bool,
    pub accept_risk: bool,
    pub no_color: bool,
}

impl MutationOptions {
    pub fn validate(self) -> Result<(), CliError> {
        if self.yes && self.dry_run {
            return Err(CliError::new(
                "option_conflict",
                "--yes and --dry-run cannot be used together",
            ));
        }
        if self.json && !self.yes && !self.dry_run {
            return Err(CliError::new(
                "confirmation_required",
                "JSON mutations require --yes or --dry-run",
            ));
        }
        Ok(())
    }

    fn require_noninteractive_choice(self) -> Result<(), CliError> {
        if !self.json && !self.yes && !self.dry_run && !io::stdin().is_terminal() {
            return Err(CliError::new(
                "confirmation_required",
                "non-interactive mutations require --yes or --dry-run",
            ));
        }
        Ok(())
    }
}

pub fn execute_operation(
    command: &'static str,
    plan: OperationPlan,
    options: MutationOptions,
) -> Result<CommandOutput, CliError> {
    if let Err(error) = options.validate() {
        return Err(cancel_preserving(&plan, error));
    }
    if let Some(error) = blocked_error(&plan) {
        return Err(cancel_preserving(&plan, error));
    }

    let summary = plan_summary(&plan);
    if !plan.has_changes() {
        cancel(&plan)?;
        return Ok(CommandOutput::new(
            command,
            false,
            json!({"dry_run": options.dry_run, "would_change": false, "plan": summary}),
            "No changes needed.",
        ));
    }

    if let Err(error) = options.require_noninteractive_choice() {
        return Err(cancel_preserving(&plan, error));
    }

    if options.dry_run {
        cancel(&plan)?;
        return Ok(CommandOutput::new(
            command,
            false,
            json!({"dry_run": true, "would_change": true, "plan": summary}),
            format!(
                "{}\n\nDry run complete; no changes made.",
                render_plan(&plan, Palette::new(options.no_color))
            ),
        ));
    }

    if let Some(error) = risk_confirmation_error(&plan, options.accept_risk) {
        return Err(cancel_preserving(&plan, error));
    }

    if !options.yes {
        println!("{}", render_plan(&plan, Palette::new(options.no_color)));
        if !confirm("Apply this plan? [y/N] ") {
            cancel(&plan)?;
            return Ok(CommandOutput::new(
                command,
                false,
                json!({"cancelled": true, "plan": summary}),
                "Cancelled; no changes made.",
            ));
        }
    }

    let cancel_request = cancel_request(&plan);
    let operation_id = plan.operation_id().to_string();
    let result = match commit(plan, options.accept_risk) {
        Ok(result) => result,
        Err(mut error) => {
            if let Err(cleanup_error) = ArcaCore::cancel(cancel_request) {
                error.details.insert(
                    "cleanup_error".into(),
                    json!({"code": cleanup_error.code, "message": cleanup_error.message}),
                );
                error = error.redact_json();
            }
            return Err(error);
        }
    };
    // A successful core commit can still leave individual targets pending.
    // Its durable desired state must not be treated as an uncommitted plan.
    let inventory = commit_output(result, &operation_id)?;
    Ok(CommandOutput::new(
        command,
        true,
        json!({"plan": summary, "result": inventory}),
        "Applied successfully.",
    ))
}

pub fn execute_direct_mutation<F>(
    command: &'static str,
    options: MutationOptions,
    summary: Value,
    review: &str,
    already_applied: bool,
    mutation: F,
) -> Result<CommandOutput, CliError>
where
    F: FnOnce() -> Result<(), CliError>,
{
    options.validate()?;
    if already_applied {
        return Ok(CommandOutput::new(
            command,
            false,
            json!({"dry_run": options.dry_run, "would_change": false, "plan": summary}),
            "No changes needed.",
        ));
    }
    options.require_noninteractive_choice()?;
    if options.dry_run {
        return Ok(CommandOutput::new(
            command,
            false,
            json!({"dry_run": true, "would_change": true, "plan": summary}),
            format!("{review}\n\nDry run complete; no changes made."),
        ));
    }
    if !options.yes {
        println!("{review}");
        if !confirm("Apply this change? [y/N] ") {
            return Ok(CommandOutput::new(
                command,
                false,
                json!({"cancelled": true, "plan": summary}),
                "Cancelled; no changes made.",
            ));
        }
    }
    mutation()?;
    Ok(CommandOutput::new(
        command,
        true,
        json!({"plan": summary}),
        "Applied successfully.",
    ))
}

fn confirm(question: &str) -> bool {
    print!("{question}");
    let _ = io::stdout().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn blocked_error(plan: &OperationPlan) -> Option<CliError> {
    match plan {
        OperationPlan::Asset { plan } if !plan.can_commit => Some(
            CliError::new(
                "operation_blocked",
                "operation is blocked by unresolved drift or conflict",
            )
            .with_detail("warnings", json!(plan.warnings)),
        ),
        _ => None,
    }
}

pub(crate) fn cancel(plan: &OperationPlan) -> Result<(), CliError> {
    ArcaCore::cancel(cancel_request(plan)).map_err(CliError::from_core)
}

fn cancel_request(plan: &OperationPlan) -> CancelOperationRequest {
    match plan {
        OperationPlan::Asset { plan } => CancelOperationRequest::Asset {
            operation_id: plan.operation_id.clone(),
        },
        OperationPlan::Skill { plan } => CancelOperationRequest::Skill {
            operation_id: plan.operation_id.clone(),
        },
    }
}

pub(crate) fn cancel_preserving(plan: &OperationPlan, mut error: CliError) -> CliError {
    if let Err(cleanup_error) = cancel(plan) {
        error.details.insert(
            "cleanup_error".into(),
            json!({"code": cleanup_error.code, "message": cleanup_error.message}),
        );
        error = error.redact_json();
    }
    error
}

fn risk_confirmation_error(plan: &OperationPlan, accept_risk: bool) -> Option<CliError> {
    match plan {
        OperationPlan::Skill { plan } if plan.requires_risk_override && !accept_risk => Some(
            CliError::new(
                "risk_confirmation_required",
                "review the Skill findings with --dry-run, then explicitly use --accept-risk to apply",
            )
            .with_detail("findings_hash", plan.findings_hash.clone()),
        ),
        _ => None,
    }
}

fn commit(plan: OperationPlan, accept_risk: bool) -> Result<OperationCommitResult, CliError> {
    match plan {
        OperationPlan::Asset { plan } => ArcaCore::commit(CommitOperationRequest::Asset {
            request: AssetCommitRequest {
                operation_id: plan.operation_id,
                candidate_hash: plan.candidate_hash,
            },
        }),
        OperationPlan::Skill { plan } => {
            let findings_confirmation = (plan.requires_risk_override && accept_risk)
                .then(|| plan.findings_hash.clone());
            ArcaCore::commit(CommitOperationRequest::Skill {
                kind: plan.kind.clone(),
                request: SkillCommitRequest {
                    operation_id: plan.operation_id,
                    candidate_hash: plan.candidate_hash,
                    findings_confirmation,
                },
            })
        }
    }
    .map_err(CliError::from_core)
}

pub(crate) fn commit_output(result: OperationCommitResult, operation_id: &str) -> Result<Value, CliError> {
    match result {
        OperationCommitResult::Asset {
            inventory,
            converged: true,
        } => Ok(safe_consumption_inventory(&inventory)),
        OperationCommitResult::Asset {
            inventory, converged: false,
        } => Err(CliError::new(
            "pending_convergence",
            "Arca saved the desired configuration, but the Agent target has not converged",
        )
        .with_detail("changed", true)
        .with_detail("operation_id", operation_id)
        .with_detail("target_incidents", json!(inventory.target_incidents.iter()
            .filter(|incident| incident.operation_id == operation_id)
            .map(safe_target_incident).collect::<Vec<_>>()))),
        OperationCommitResult::Skill { inventory } => Ok(safe_skill_inventory(&inventory)),
    }
}

pub(crate) fn plan_summary(plan: &OperationPlan) -> Value {
    match plan {
        OperationPlan::Asset { plan } => json!({
            "operation_id": plan.operation_id,
            "domain": asset_domain(plan),
            "affected_agents": plan.affected_agent_ids,
            "target_files": plan.target_files.iter().map(|path| safe_path(path)).collect::<Vec<_>>(),
            "central_changes": plan.central_changes,
            "relationship_changes": plan.relationship_changes,
            "model_state_changes": plan.model_state_changes,
            "consumption_state_changes": plan.consumption_state_changes.iter().map(|change| json!({
                "agent_id": change.agent_id,
                "asset": change.asset,
                "before_enabled": change.before_enabled,
                "after_enabled": change.after_enabled,
                "affected_agent_ids": change.affected_agent_ids,
                "target": change.target.as_ref().map(|target| json!({
                    "target_id": target.target_id,
                    "global_dir": safe_path(&target.global_dir),
                })),
            })).collect::<Vec<_>>(),
            "warnings": plan.warnings,
            "can_commit": plan.can_commit,
            "candidate_hash": plan.candidate_hash,
        }),
        OperationPlan::Skill { plan } => {
            let agents = plan
                .targets
                .iter()
                .flat_map(|target| target.affected_agent_ids.iter().cloned())
                .collect::<BTreeSet<_>>();
            json!({
                "operation_id": plan.operation_id,
                "domain": "skill",
                "kind": plan.kind,
                "skills": plan.skills.iter().map(|skill| json!({
                    "name": skill.manifest.name,
                    "risk": skill.risk,
                    "replace_existing": skill.replace_existing,
                })).collect::<Vec<_>>(),
                "affected_agents": agents,
                "targets": plan.targets.iter().map(|target| safe_path(&target.global_dir)).collect::<Vec<_>>(),
                "warnings": plan.warnings,
                "requires_risk_override": plan.requires_risk_override,
                "findings_hash": plan.findings_hash,
                "candidate_hash": plan.candidate_hash,
            })
        }
    }
}

fn asset_domain(plan: &AssetOperationPlan) -> &'static str {
    match &plan.domain_plan {
        DomainPlan::Mcp { .. } => "mcp",
        DomainPlan::Model { .. } => "model",
        DomainPlan::Skill { .. } => "skill",
        DomainPlan::AgentCapabilities { .. } => "agent",
    }
}

fn render_plan(plan: &OperationPlan, palette: Palette) -> String {
    match plan {
        OperationPlan::Asset { plan } => {
            let mut lines = vec![palette.bold("Review plan")];
            let standalone_enabled_changes = plan
                .consumption_state_changes
                .iter()
                .filter(|change| {
                    !matches!(
                        change.asset,
                        arca_core::application::assets::AssetRef::Model { .. }
                    )
                })
                .count();
            lines.push(format!("  domain: {}", asset_domain(plan)));
            lines.push(format!(
                "  Agents: {}",
                if plan.affected_agent_ids.is_empty() {
                    "none".into()
                } else {
                    plan.affected_agent_ids.join(", ")
                }
            ));
            lines.push(format!(
                "  targets: {}",
                if plan.target_files.is_empty() {
                    "none".into()
                } else {
                    plan.target_files.join(", ")
                }
            ));
            lines.push(format!(
                "  changes: {} central, {} relationships, {} enabled states, {} model states",
                plan.central_changes.len(),
                plan.relationship_changes.len(),
                standalone_enabled_changes,
                plan.model_state_changes.len()
            ));
            for change in &plan.central_changes {
                lines.push(format!(
                    "  central {:?} {}",
                    change.action,
                    asset_label(&change.asset)
                ));
                for summary in &change.summary {
                    lines.push(format!("    {summary}"));
                }
            }
            for change in &plan.relationship_changes {
                lines.push(format!(
                    "  relationship {:?} {} -> {}",
                    change.action,
                    asset_label(&change.asset),
                    change.agent_id
                ));
            }
            for change in &plan.consumption_state_changes {
                if matches!(
                    change.asset,
                    arca_core::application::assets::AssetRef::Model { .. }
                ) {
                    continue;
                }
                lines.push(format!(
                    "  enabled {} @ {}: {}→{}",
                    asset_label(&change.asset),
                    change.agent_id,
                    change.before_enabled,
                    change.after_enabled,
                ));
                if change.affected_agent_ids.len() > 1 {
                    lines.push(format!(
                        "    affected Agents: {}",
                        change.affected_agent_ids.join(", ")
                    ));
                }
                if let Some(target) = &change.target {
                    lines.push(format!(
                        "    shared target: {} ({})",
                        target.target_id, target.global_dir
                    ));
                }
            }
            for change in &plan.model_state_changes {
                lines.push(format!(
                    "  model {} @ {}: added {}→{}, enabled {}→{}, active {}→{} ({})",
                    change.profile_id,
                    change.agent_id,
                    change.before.added,
                    change.after.added,
                    change.before.enabled,
                    change.after.enabled,
                    change.before.active,
                    change.after.active,
                    change.reason,
                ));
                if let Some(fallback) = &change.fallback_profile_id {
                    lines.push(format!("    fallback: {fallback}"));
                }
            }
            for warning in &plan.warnings {
                lines.push(palette.yellow(&format!("  warning: {warning}")));
            }
            lines.join("\n")
        }
        OperationPlan::Skill { plan } => {
            let agents = plan
                .targets
                .iter()
                .flat_map(|target| target.affected_agent_ids.iter().cloned())
                .collect::<BTreeSet<_>>();
            let mut lines = vec![palette.bold("Review plan")];
            lines.push("  domain: skill".into());
            lines.push(format!(
                "  Skills: {}",
                plan.skills
                    .iter()
                    .map(|skill| skill.manifest.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            lines.push(format!(
                "  Agents: {}",
                agents.into_iter().collect::<Vec<_>>().join(", ")
            ));
            lines.push(format!(
                "  targets: {}",
                plan.targets
                    .iter()
                    .map(|target| target.global_dir.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            for warning in &plan.warnings {
                lines.push(palette.yellow(&format!("  warning: {warning}")));
            }
            for skill in &plan.skills {
                let risk = serde_json::to_string_pretty(&skill.risk)
                    .unwrap_or_else(|_| format!("{:?}", skill.risk));
                lines.push(format!(
                    "  risk findings for {}:\n{}",
                    skill.manifest.name, risk
                ));
            }
            lines.join("\n")
        }
    }
}

fn asset_label(asset: &arca_core::application::assets::AssetRef) -> String {
    match asset {
        arca_core::application::assets::AssetRef::Mcp { key } => format!("mcp:{key}"),
        arca_core::application::assets::AssetRef::Model { profile_id } => {
            format!("model:{profile_id}")
        }
        arca_core::application::assets::AssetRef::ModelProvider { provider_id } => {
            format!("model-provider:{provider_id}")
        }
        arca_core::application::assets::AssetRef::Skill { name } => format!("skill:{name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arca_core::application::assets::{
        AssetOperationKind, AssetRef, ConsumptionStateChange, ConsumptionTarget, ModelStateChange,
        ModelStateSnapshot,
    };
    use std::collections::BTreeMap;

    fn state_only_plan() -> OperationPlan {
        OperationPlan::Asset {
            plan: Box::new(AssetOperationPlan {
                operation_id: "00000000-0000-4000-8000-000000000001".into(),
                kind: AssetOperationKind::SetConsumption,
                domain_plan: DomainPlan::Skill {
                    before: BTreeMap::from([("codex".into(), vec!["risky".into()])]),
                    after: BTreeMap::from([("codex".into(), vec!["risky".into()])]),
                },
                central_changes: Vec::new(),
                relationship_changes: Vec::new(),
                model_state_changes: Vec::new(),
                consumption_state_changes: vec![ConsumptionStateChange {
                    agent_id: "codex".into(),
                    asset: AssetRef::Skill {
                        name: "risky".into(),
                    },
                    before_enabled: true,
                    after_enabled: false,
                    affected_agent_ids: vec!["codex".into(), "cursor".into()],
                    target: Some(ConsumptionTarget {
                        target_id: "agents-user".into(),
                        global_dir: "/private/secret/skills".into(),
                    }),
                }],
                target_files: vec!["/private/secret/skills/risky".into()],
                affected_agent_ids: vec!["codex".into(), "cursor".into()],
                warnings: Vec::new(),
                can_commit: true,
                candidate_hash: "candidate".into(),
            }),
        }
    }

    #[test]
    fn json_mutation_requires_explicit_noninteractive_choice() {
        let error = MutationOptions {
            json: true,
            yes: false,
            dry_run: false,
            accept_risk: false,
            no_color: true,
        }
        .validate()
        .unwrap_err();
        assert_eq!(error.code, "confirmation_required");
    }

    #[test]
    fn yes_and_dry_run_are_mutually_exclusive() {
        let error = MutationOptions {
            json: false,
            yes: true,
            dry_run: true,
            accept_risk: false,
            no_color: true,
        }
        .validate()
        .unwrap_err();
        assert_eq!(error.code, "option_conflict");
    }

    #[test]
    fn state_only_asset_plan_is_a_change_and_json_redacts_its_target() {
        let plan = state_only_plan();
        assert!(plan.has_changes());

        let summary = plan_summary(&plan);
        assert_eq!(
            summary["consumption_state_changes"][0]["target"]["global_dir"],
            "<absolute-path-redacted>"
        );
        assert_eq!(
            summary["consumption_state_changes"][0]["affected_agent_ids"],
            json!(["codex", "cursor"])
        );
    }

    #[test]
    fn human_review_renders_enabled_delta_and_shared_target_closure() {
        let rendered = render_plan(&state_only_plan(), Palette::new(true));
        assert!(rendered.contains("enabled skill:risky @ codex: true→false"));
        assert!(rendered.contains("affected Agents: codex, cursor"));
        assert!(rendered.contains("shared target: agents-user (/private/secret/skills)"));
        assert!(rendered.contains("1 enabled states"));
    }

    #[test]
    fn human_review_does_not_duplicate_model_enabled_delta() {
        let OperationPlan::Asset { mut plan } = state_only_plan() else {
            unreachable!();
        };
        plan.domain_plan = DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        };
        plan.consumption_state_changes[0].asset = AssetRef::Model {
            profile_id: "work".into(),
        };
        plan.consumption_state_changes[0].target = None;
        plan.model_state_changes = vec![ModelStateChange {
            agent_id: "codex".into(),
            profile_id: "work".into(),
            before: ModelStateSnapshot {
                added: true,
                enabled: true,
                active: false,
            },
            after: ModelStateSnapshot {
                added: true,
                enabled: false,
                active: false,
            },
            fallback_profile_id: None,
            reason: "model_disabled".into(),
        }];

        let rendered = render_plan(&OperationPlan::Asset { plan }, Palette::new(true));
        assert!(rendered.contains("0 enabled states, 1 model states"));
        assert!(rendered.contains("model work @ codex"));
        assert!(!rendered.contains("enabled Model"));
    }

    #[test]
    fn empty_model_plan_is_a_noop_without_legacy_confirmation_state() {
        let OperationPlan::Asset { mut plan } = state_only_plan() else {
            unreachable!();
        };
        plan.domain_plan = DomainPlan::Model {
            before: BTreeMap::new(),
            after: BTreeMap::new(),
        };
        plan.consumption_state_changes.clear();
        assert!(!plan.has_changes());
    }

    #[test]
    fn native_model_clear_detects_physical_changes_without_desired_relationships() {
        let OperationPlan::Asset { mut plan } = state_only_plan() else { unreachable!() };
        plan.kind = AssetOperationKind::ClearModels;
        plan.domain_plan = DomainPlan::Model { before: BTreeMap::new(), after: BTreeMap::new() };
        plan.consumption_state_changes.clear();
        assert!(plan.has_changes());
        plan.target_files.clear();
        assert!(!plan.has_changes());
    }

    #[test]
    fn generic_confirmation_does_not_authorize_skill_risk() {
        let mut plan = arca_core::application::skills::OperationPlan {
            operation_id: "skill-operation".into(),
            kind: arca_core::application::skills::SkillOperationKind::Install,
            skills: Vec::new(), targets: Vec::new(), settings_hash: "settings".into(),
            candidate_hash: "candidate".into(), findings_hash: "reviewed-findings".into(),
            requires_risk_override: true, warnings: Vec::new(),
        };
        let error = risk_confirmation_error(&OperationPlan::Skill { plan: plan.clone() }, false).unwrap();
        assert_eq!(error.code, "risk_confirmation_required");
        assert_eq!(error.details["findings_hash"], "reviewed-findings");
        assert!(risk_confirmation_error(&OperationPlan::Skill { plan: plan.clone() }, true).is_none());
        plan.requires_risk_override = false;
        assert!(risk_confirmation_error(&OperationPlan::Skill { plan }, false).is_none());
    }

    #[test]
    fn pending_commit_reports_durable_change_and_only_its_own_redacted_incidents() {
        use arca_core::application::assets::{AssetCapability, ConsumptionInventory, TargetIncident};
        let incident = TargetIncident {
            id: "failed-target".into(), operation_id: "committed".into(),
            capability: AssetCapability::Model, target_id: "codex-model".into(),
            target_path: "/private/provider-secret/config.toml".into(),
            affected_agent_ids: vec!["codex".into()], code: "target_write_failed".into(), retryable: true,
        };
        let mut unrelated = incident.clone();
        unrelated.operation_id = "other-operation".into();
        let error = commit_output(OperationCommitResult::Asset {
            inventory: ConsumptionInventory { target_incidents: vec![incident, unrelated], ..Default::default() },
            converged: false,
        }, "committed").unwrap_err();
        assert_eq!(error.code, "pending_convergence");
        assert_eq!(error.details["changed"], true);
        assert_eq!(error.details["operation_id"], "committed");
        assert_eq!(error.details["target_incidents"].as_array().unwrap().len(), 1);
        assert!(!json!(error.details).to_string().contains("provider-secret"));
    }

    #[test]
    fn direct_noop_preserves_dry_run_receipt_without_calling_mutation() {
        let output = execute_direct_mutation("agent.enable", MutationOptions {
            json: true, yes: false, dry_run: true, accept_risk: false, no_color: true,
        }, json!({}), "noop", true, || panic!("no-op must not mutate")).unwrap();
        assert!(!output.changed);
        assert_eq!(output.data["dry_run"], true);
        assert_eq!(output.data["would_change"], false);
    }
}
