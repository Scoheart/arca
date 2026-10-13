//! A reviewed operation stays in its owning process until the caller commits
//! the exact returned plan. Secret drafts never need an on-disk handoff.
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

use clap::Subcommand;
use arca_core::application::operations::{CommitOperationRequest, OperationPlan, PlanOperationRequest};
use arca_core::application::ArcaCore;
use serde_json::{json, Value};

use crate::command::Cli;
use crate::output::{CliError, CommandOutput};
use crate::review::{cancel, cancel_preserving, commit_output, execute_operation, plan_summary};

#[derive(Debug, Subcommand)]
pub enum OperationCommand {
    /// Apply a typed Core PlanOperationRequest using the normal review flags.
    Apply { #[arg(long)] file: PathBuf },
    /// Emit a plan, then read one matching commit/cancel JSON line on stdin.
    /// Requires --json; holds the plan in this process while awaiting review.
    Review { #[arg(long)] file: PathBuf },
}

pub fn dispatch(cli: &Cli, command: &OperationCommand) -> Result<CommandOutput, CliError> {
    let (file, session) = match command {
        OperationCommand::Apply { file } => (file, false),
        OperationCommand::Review { file } => (file, true),
    };
    if session {
        cli.reject_mutation_options()?;
        if !cli.json || file == Path::new("-") {
            return Err(CliError::new("option_conflict",
                "operation review requires --json and a request file; stdin is reserved for the decision"));
        }
    } else { cli.mutation_options().validate()?; }
    let request: PlanOperationRequest = serde_json::from_value(crate::input::json_file(file)?)
        .map_err(|_| CliError::new("invalid_operation", "input must be a typed Core PlanOperationRequest"))?;
    let plan = ArcaCore::plan(request).map_err(CliError::from_core)?;
    if !session { return execute_operation("operation.apply", plan, cli.mutation_options()); }
    review_session(plan, &mut io::stdin().lock(), &mut io::stdout().lock())
}

fn review_session(
    plan: OperationPlan,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<CommandOutput, CliError> {
    let summary = plan_summary(&plan);
    if matches!(&plan, OperationPlan::Asset { plan } if !plan.can_commit) {
        return Err(cancel_preserving(&plan,
            CliError::new("operation_blocked", "resolve the plan warnings before committing")));
    }
    if !plan.has_changes() {
        cancel(&plan)?;
        return Ok(CommandOutput::new("operation.review", false,
            json!({"phase": "complete", "would_change": false, "plan": summary}), "No changes needed."));
    }
    let header = json!({"schema_version": 1, "ok": true, "command": "operation.review",
        "phase": "review", "changed": false, "data": {"plan": summary,
            "commit_decision": {"action": "commit", "request": commit_template(&plan)},
            "cancel_decision": {"action": "cancel"}}});
    if let Err(error) = writeln!(output, "{header}").and_then(|_| output.flush()) {
        return Err(cancel_preserving(&plan, CliError::private("output_failed", error.to_string())));
    }
    let decision = read_decision(input).map_err(|error| cancel_preserving(&plan, error))?;
    let Some(decision) = decision else {
        cancel(&plan)?;
        return Ok(CommandOutput::new("operation.review", false,
            json!({"phase": "complete", "cancelled": true, "plan": summary}), "Cancelled."));
    };
    let request = match decode_commit(&plan, decision) {
        Ok(None) => {
            cancel(&plan)?;
            return Ok(CommandOutput::new("operation.review", false,
                json!({"phase": "complete", "cancelled": true, "plan": summary}), "Cancelled."));
        }
        Ok(Some(request)) => request,
        Err(error) => return Err(cancel_preserving(&plan, error)),
    };
    let result = ArcaCore::commit(request).map_err(|error|
        cancel_preserving(&plan, CliError::from_core(error)))?;
    let inventory = commit_output(result, plan.operation_id())?;
    Ok(CommandOutput::new("operation.review", true,
        json!({"phase": "complete", "plan": summary, "result": inventory}), "Applied reviewed plan."))
}

fn commit_template(plan: &OperationPlan) -> CommitOperationRequest {
    match plan {
        OperationPlan::Asset { plan } => CommitOperationRequest::Asset {
            request: arca_core::application::assets::AssetCommitRequest {
                operation_id: plan.operation_id.clone(), candidate_hash: plan.candidate_hash.clone(),
            },
        },
        OperationPlan::Skill { plan } => CommitOperationRequest::Skill {
            kind: plan.kind.clone(),
            request: arca_core::application::skills::SkillCommitRequest {
                operation_id: plan.operation_id.clone(), candidate_hash: plan.candidate_hash.clone(),
                findings_confirmation: plan.requires_risk_override.then(|| plan.findings_hash.clone()),
            },
        },
    }
}

fn read_decision(input: &mut impl BufRead) -> Result<Option<Value>, CliError> {
    const LIMIT: u64 = 16 * 1024;
    let mut bytes = Vec::new();
    let length = input.take(LIMIT + 1).read_until(b'\n', &mut bytes)
        .map_err(|error| CliError::private("input_read_failed", error.to_string()))?;
    if length == 0 { return Ok(None); }
    if length as u64 > LIMIT {
        return Err(CliError::new("input_too_large", "review decision exceeds 16 KiB"));
    }
    serde_json::from_slice(&bytes).map(Some)
        .map_err(|_| CliError::new("invalid_json", "send one commit or cancel JSON object"))
}

fn decode_commit(plan: &OperationPlan, decision: Value) -> Result<Option<CommitOperationRequest>, CliError> {
    let invalid = || CliError::new("invalid_decision", "send action=cancel or action=commit with a typed request");
    let object = decision.as_object().ok_or_else(invalid)?;
    match object.get("action").and_then(Value::as_str) {
        Some("cancel") if object.len() == 1 => return Ok(None),
        Some("commit") if object.len() == 2 => {},
        _ => return Err(invalid()),
    }
    let request: CommitOperationRequest = serde_json::from_value(object.get("request").cloned().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let matches = match (plan, &request) {
        (OperationPlan::Asset { plan }, CommitOperationRequest::Asset { request }) =>
            request.operation_id == plan.operation_id && request.candidate_hash == plan.candidate_hash,
        (OperationPlan::Skill { plan }, CommitOperationRequest::Skill { kind, request }) =>
            kind == &plan.kind && request.operation_id == plan.operation_id
                && request.candidate_hash == plan.candidate_hash
                && (!plan.requires_risk_override || request.findings_confirmation.as_deref() == Some(plan.findings_hash.as_str())),
        _ => false,
    };
    if !matches {
        return Err(CliError::new("review_mismatch", "decision does not match the reviewed operation, candidate or findings"));
    }
    Ok(Some(request))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_are_bounded_and_eof_is_cancel() {
        assert!(read_decision(&mut &b""[..]).unwrap().is_none());
        assert_eq!(read_decision(&mut &b"{broken}\n"[..]).unwrap_err().code, "invalid_json");
        assert_eq!(read_decision(&mut &vec![b' '; 16 * 1024 + 1][..]).unwrap_err().code, "input_too_large");
        let mut input = &b"{\"action\":\"cancel\"}\nnext\n"[..];
        assert_eq!(read_decision(&mut input).unwrap(), Some(json!({"action":"cancel"})));
        assert_eq!(input, b"next\n");
    }
}
