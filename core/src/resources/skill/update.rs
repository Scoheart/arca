use super::files::validate_candidate_anchored_private;
use super::source::{
    check_github_revision, open_recorded_local_skill, stage_recorded_skill,
    validate_github_revision_source, GithubRevisionStatus,
};
use super::transaction::acquire_skills_lock;
use super::{
    capped_message, GithubEndpoints, SkillError, SkillSource, SkillsPaths, UpdateCheckOutcome,
};
use crate::settings::{load_settings_strict, mutate_settings};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

const UPDATE_INTERVAL_HOURS: i64 = 24;

#[derive(Debug)]
enum ProbeResult {
    Pinned(Result<(), SkillError>),
    Github(Result<GithubRevisionStatus, SkillError>),
    Local(Result<String, SkillError>),
}

#[derive(Debug)]
struct Probe {
    name: String,
    source: SkillSource,
    resolved_revision: Option<String>,
    content_hash: String,
    update: super::SkillUpdateState,
    result: ProbeResult,
}

pub fn check_updates(manual: bool) -> Result<UpdateCheckOutcome, SkillError> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    check_updates_with(manual, &now, GithubEndpoints::production())
}

pub fn check_updates_if_due() -> Result<UpdateCheckOutcome, SkillError> {
    check_updates(false)
}

#[doc(hidden)]
pub fn check_updates_with(
    manual: bool,
    now: &str,
    endpoints: GithubEndpoints,
) -> Result<UpdateCheckOutcome, SkillError> {
    check_updates_with_reconcile_hook(manual, now, endpoints, || {})
}

fn check_updates_with_reconcile_hook<F>(
    manual: bool,
    now: &str,
    endpoints: GithubEndpoints,
    before_reconcile: F,
) -> Result<UpdateCheckOutcome, SkillError>
where
    F: FnOnce(),
{
    let prepared = prepare_update_check_at(manual, now)?;
    let probed = probe_update_check(prepared, endpoints)?;
    before_reconcile();
    reconcile_update_check(probed)
}

/// Preparation snapshots metadata only. Callers release the application gate
/// before probing sources and reacquire it only to reconcile these records.
pub(crate) struct PreparedUpdateCheck {
    now: String,
    previous_checked_at: Option<String>,
    records: Vec<(String, super::ManagedSkillRecord)>,
    skipped: Option<UpdateCheckOutcome>,
}

pub(crate) struct ProbedUpdateCheck {
    now: String,
    previous_checked_at: Option<String>,
    probes: Vec<Probe>,
    skipped: Option<UpdateCheckOutcome>,
}

pub(crate) fn prepare_update_check(manual: bool) -> Result<PreparedUpdateCheck, SkillError> {
    prepare_update_check_at(manual, &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn prepare_update_check_at(manual: bool, now: &str) -> Result<PreparedUpdateCheck, SkillError> {
    let now_parsed = DateTime::parse_from_rfc3339(now).map_err(|_| SkillError::InvalidSource {
        message: "the update-check clock is not a valid RFC 3339 timestamp".into(),
    })?;
    let settings = load_settings_strict().map_err(settings_read_error)?;
    if !manual
        && settings
            .skill_update_checked_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|previous| {
                let elapsed = now_parsed.signed_duration_since(previous);
                elapsed >= Duration::zero() && elapsed < Duration::hours(UPDATE_INTERVAL_HOURS)
            })
    {
        return Ok(PreparedUpdateCheck { now: now.to_owned(), previous_checked_at: settings.skill_update_checked_at.clone(), records: Vec::new(), skipped: Some(UpdateCheckOutcome {
            performed: false,
            checked: 0,
            available: Vec::new(),
            skipped_pinned: Vec::new(),
            errors: Default::default(),
            checked_at: settings.skill_update_checked_at,
        }) });
    }

    Ok(PreparedUpdateCheck {
        now: now.to_owned(),
        previous_checked_at: settings.skill_update_checked_at,
        records: settings.managed_skills.unwrap_or_default().into_iter().collect(),
        skipped: None,
    })
}

type GithubProbeKey = (String, String, String, Option<String>);
type GithubProbeCell = Arc<OnceLock<Result<GithubRevisionStatus, SkillError>>>;

pub(crate) fn probe_update_check(
    prepared: PreparedUpdateCheck,
    endpoints: GithubEndpoints,
) -> Result<ProbedUpdateCheck, SkillError> {
    if prepared.skipped.is_some() {
        return Ok(ProbedUpdateCheck { now: prepared.now, previous_checked_at: prepared.previous_checked_at, probes: Vec::new(), skipped: prepared.skipped });
    }
    let read_paths = SkillsPaths::resolve_from_env()?;
    // Skills in one repository often share a revision. One cell per request
    // deduplicates those requests, including errors, without holding a mutex
    // while waiting on the network. Conditional requests with different ETags
    // remain separate so a 304 is never applied to a different cached state.
    let github = Mutex::new(BTreeMap::<GithubProbeKey, GithubProbeCell>::new());
    let probe = |(name, record): &(String, super::ManagedSkillRecord)| {
        let result = match &record.source {
            SkillSource::Github { pinned: true, .. } => ProbeResult::Pinned(
                validate_pinned_github_record(&record.source, record.resolved_revision.as_deref())),
            SkillSource::Imported { .. } => ProbeResult::Pinned(Ok(())),
            SkillSource::Github { owner, repo, requested_ref, .. } => {
                let result = validate_github_revision_source(&record.source).and_then(|_| {
                    let key = (owner.clone(), repo.clone(), requested_ref.clone(), record.update.etag.clone());
                    let cell = github.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
                        .entry(key).or_default().clone();
                    cell.get_or_init(|| check_github_revision(&record.source, record.update.etag.as_deref(), &endpoints)).clone()
                });
                ProbeResult::Github(result)
            }
            SkillSource::Local { .. } => ProbeResult::Local(local_source_hash(&read_paths, &record.source, name)),
            SkillSource::Archive { .. } => ProbeResult::Local(archive_source_hash(&record.source, name, endpoints.clone())),
        };
        Probe { name: name.clone(), source: record.source.clone(), resolved_revision: record.resolved_revision.clone(),
            content_hash: record.content_hash.clone(), update: record.update.clone(), result }
    };
    const PROBE_CONCURRENCY: usize = 4;
    let probes = std::thread::scope(|scope| {
        let handles = prepared.records.chunks(prepared.records.len().div_ceil(PROBE_CONCURRENCY).max(1))
            .map(|records| {
                let probe = &probe;
                scope.spawn(move || records.iter().map(probe).collect::<Vec<_>>())
            }).collect::<Vec<_>>();
        handles.into_iter().map(|handle| handle.join().map_err(|_| SkillError::Io {
            message: "a Skills update probe could not complete".into(), path: None,
        })).collect::<Result<Vec<_>, _>>().map(|batches| batches.into_iter().flatten().collect())
    })?;
    Ok(ProbedUpdateCheck { now: prepared.now, previous_checked_at: prepared.previous_checked_at, probes, skipped: None })
}

pub(crate) fn reconcile_update_check(probed: ProbedUpdateCheck) -> Result<UpdateCheckOutcome, SkillError> {
    if let Some(skipped) = probed.skipped { return Ok(skipped); }
    let now = probed.now.as_str();
    let probes = probed.probes;
    // Network and local-tree reads above intentionally happen without the
    // cross-process operation lock. The lock only protects the short compare
    // and persist phase, where settings are re-read by mutate_settings.
    let paths = SkillsPaths::from_env()?;
    let _lock = acquire_skills_lock(&paths)?;
    let mut outcome = UpdateCheckOutcome {
        performed: true,
        checked: 0,
        available: Vec::new(),
        skipped_pinned: Vec::new(),
        errors: Default::default(),
        checked_at: Some(now.to_owned()),
    };
    mutate_settings(|current| {
        let records = current.managed_skills.get_or_insert_default();
        let mut complete = records.len() == probes.len();
        for probe in probes {
            let Some(record) = records.get_mut(&probe.name) else {
                complete = false;
                continue;
            };
            if record.source != probe.source
                || record.resolved_revision != probe.resolved_revision
                || record.content_hash != probe.content_hash
                || record.update != probe.update
            {
                complete = false;
                continue;
            }
            match probe.result {
                ProbeResult::Pinned(Ok(())) => outcome.skipped_pinned.push(probe.name),
                ProbeResult::Pinned(Err(error)) => {
                    record_probe_error(&mut outcome, &probe.name, &mut record.update, error, now)
                }
                ProbeResult::Github(result) => {
                    outcome.checked += 1;
                    match result {
                        Ok(GithubRevisionStatus::NotModified { etag }) => {
                            record.update.checked_at = Some(now.to_owned());
                            record.update.etag = etag.or_else(|| record.update.etag.clone());
                            record.update.error = None;
                            record.update.retry_at = None;
                            if record.update.available {
                                outcome.available.push(probe.name);
                            }
                        }
                        Ok(GithubRevisionStatus::Resolved { sha, etag }) => {
                            let available = record.resolved_revision.as_deref() != Some(&sha);
                            record.update.available = available;
                            record.update.checked_at = Some(now.to_owned());
                            record.update.resolved_revision = Some(sha);
                            // A fresh representation cannot reuse a validator for old bytes.
                            record.update.etag = etag;
                            record.update.error = None;
                            record.update.retry_at = None;
                            if available {
                                outcome.available.push(probe.name);
                            }
                        }
                        Err(error) => record_probe_error(
                            &mut outcome,
                            &probe.name,
                            &mut record.update,
                            error,
                            now,
                        ),
                    }
                }
                ProbeResult::Local(result) => {
                    outcome.checked += 1;
                    match result {
                        Ok(hash) => {
                            let available = record.content_hash != hash;
                            record.update.available = available;
                            record.update.checked_at = Some(now.to_owned());
                            record.update.resolved_revision = Some(hash);
                            record.update.etag = None;
                            record.update.error = None;
                            record.update.retry_at = None;
                            if available {
                                outcome.available.push(probe.name);
                            }
                        }
                        Err(error) => record_probe_error(
                            &mut outcome,
                            &probe.name,
                            &mut record.update,
                            error,
                            now,
                        ),
                    }
                }
            }
        }
        // A concurrent install/update must not be hidden for the next 24 h,
        // and an older in-flight check must not move the global clock backwards.
        let newer_check = current.skill_update_checked_at.as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .zip(DateTime::parse_from_rfc3339(now).ok())
            .is_some_and(|(previous, checked)| previous > checked);
        let clock_unchanged = current.skill_update_checked_at == probed.previous_checked_at;
        // Correct a future timestamp from a clock rollback only when it is
        // still the one preparation observed, never over a concurrent check.
        if complete && (clock_unchanged || !newer_check) {
            current.skill_update_checked_at = Some(now.to_owned());
        }
        outcome.checked_at = current.skill_update_checked_at.clone();
    })
    .map_err(settings_write_error)?;

    outcome.available.sort();
    outcome.skipped_pinned.sort();
    Ok(outcome)
}

fn validate_pinned_github_record(
    source: &SkillSource,
    installed_revision: Option<&str>,
) -> Result<(), SkillError> {
    validate_github_revision_source(source)?;
    let SkillSource::Github { requested_ref, .. } = source else {
        return Err(SkillError::InvalidSource {
            message: "the pinned GitHub Skill source is invalid".into(),
        });
    };
    let canonical = requested_ref.to_ascii_lowercase();
    if requested_ref != &canonical || installed_revision != Some(canonical.as_str()) {
        return Err(SkillError::InvalidSource {
            message: "the pinned GitHub Skill revision is inconsistent".into(),
        });
    }
    Ok(())
}

fn local_source_hash(
    paths: &SkillsPaths,
    source: &SkillSource,
    expected_name: &str,
) -> Result<String, SkillError> {
    let SkillSource::Local { .. } = source else {
        return Err(SkillError::InvalidSource {
            message: "the recorded local Skill source is invalid".into(),
        });
    };
    let candidate = open_recorded_local_skill(paths, source)?;
    let validated = validate_candidate_anchored_private(&candidate)?;
    if validated.manifest.name != expected_name {
        return Err(SkillError::InvalidSource {
            message: "the recorded local Skill name no longer matches its source".into(),
        });
    }
    Ok(validated.content_hash)
}

fn archive_source_hash(
    source: &SkillSource,
    expected_name: &str,
    endpoints: GithubEndpoints,
) -> Result<String, SkillError> {
    if !matches!(source, SkillSource::Archive { .. }) {
        return Err(SkillError::InvalidSource {
            message: "the recorded archive Skill source is invalid".into(),
        });
    }
    let resolution = stage_recorded_skill(source, None, expected_name, endpoints)?;
    let hash = resolution
        .candidates
        .first()
        .map(|candidate| candidate.content_hash.clone())
        .ok_or_else(|| SkillError::InvalidSource {
            message: "the recorded archive no longer contains the Skill".into(),
        });
    let cleanup = super::ops::cancel_operation(&resolution.operation_id);
    match (hash, cleanup) {
        (Ok(hash), Ok(())) => Ok(hash),
        (Err(error), Ok(())) => Err(error),
        (_, Err(error)) => Err(error),
    }
}

fn record_probe_error(
    outcome: &mut UpdateCheckOutcome,
    name: &str,
    state: &mut super::SkillUpdateState,
    error: SkillError,
    now: &str,
) {
    let (message, retry_at) = display_probe_error(error);
    state.checked_at = Some(now.to_owned());
    state.error = Some(message.clone());
    state.retry_at = retry_at;
    outcome.errors.insert(name.to_owned(), message);
}

fn display_probe_error(error: SkillError) -> (String, Option<String>) {
    match error {
        SkillError::Network { message, retry_at } => (capped_message(message), retry_at),
        SkillError::InvalidSource { message }
        | SkillError::PlanStale { message }
        | SkillError::RecoveryRequired { message } => (capped_message(message), None),
        SkillError::InvalidManifest { message, .. }
        | SkillError::UnsafePath { message, .. }
        | SkillError::Conflict { message, .. }
        | SkillError::Io { message, .. } => (capped_message(message), None),
        SkillError::LimitExceeded {
            limit,
            actual,
            allowed,
        } => (
            capped_message(format!("{limit} limit exceeded: {actual} > {allowed}")),
            None,
        ),
        SkillError::ConfirmationRequired { message, .. } => (capped_message(message), None),
    }
}

fn settings_read_error(_error: std::io::Error) -> SkillError {
    SkillError::Io {
        message: "Arca settings could not be read safely".into(),
        path: None,
    }
}

fn settings_write_error(_error: std::io::Error) -> SkillError {
    SkillError::Io {
        message: "Skills update state could not be saved safely".into(),
        path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::skill::{
        ManagedSkillRecord, RiskLevel, SkillContentKind, SkillRiskSummary, SkillUpdateState,
    };
    use crate::settings::{load_settings, mutate_settings};
    use crate::testenv::TestHome;
    use std::fs;

    fn local_probe_fixture(name: &str) -> TestHome {
        let home = TestHome::new(name);
        let source = home.home.join("source/review-changes");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: review-changes\ndescription: Local probe fixture\n---\n",
        )
        .unwrap();
        mutate_settings(|settings| {
            settings.managed_skills = Some(
                [(
                    "review-changes".into(),
                    ManagedSkillRecord {
                        name: "review-changes".into(),
                        description: "Installed fixture".into(),
                        content_kind: SkillContentKind::Instructions,
                        source: SkillSource::Local {
                            path: "~/source".into(),
                            subpath: "review-changes".into(),
                        },
                        resolved_revision: None,
                        content_hash: "installed-content-hash".into(),
                        installed_at: "2026-07-17T00:00:00Z".into(),
                        updated_at: "2026-07-17T00:00:00Z".into(),
                        risk: SkillRiskSummary {
                            level: RiskLevel::Low,
                            findings: Vec::new(),
                            finding_count: 0,
                            findings_truncated: false,
                        },
                        update: SkillUpdateState::default(),
                    },
                )]
                .into(),
            );
        })
        .unwrap();
        home
    }

    #[test]
    fn a_completed_check_corrects_an_unchanged_future_clock_without_rechecking_forever() {
        let _home = local_probe_fixture("update-future-clock-recovery");
        mutate_settings(|settings| settings.skill_update_checked_at = Some("2030-01-01T00:00:00Z".into())).unwrap();
        let now = "2026-10-05T08:00:00Z";
        assert!(check_updates_with(false, now, GithubEndpoints::production()).unwrap().performed);
        assert_eq!(load_settings().skill_update_checked_at.as_deref(), Some(now));
        assert!(!check_updates_with(false, now, GithubEndpoints::production()).unwrap().performed);
    }

    #[test]
    fn an_inflight_check_does_not_roll_back_a_newer_global_clock() {
        let _home = local_probe_fixture("update-concurrent-clock");
        check_updates_with_reconcile_hook(true, "2026-10-05T08:00:00Z", GithubEndpoints::production(), || {
            mutate_settings(|settings| settings.skill_update_checked_at = Some("2026-10-05T09:00:00Z".into())).unwrap();
        }).unwrap();
        assert_eq!(load_settings().skill_update_checked_at.as_deref(), Some("2026-10-05T09:00:00Z"));
    }

    #[test]
    fn local_probe_is_discarded_when_content_hash_changes_before_reconciliation() {
        let _home = local_probe_fixture("update-local-content-race");
        let outcome = check_updates_with_reconcile_hook(
            true,
            "2026-07-17T08:00:00Z",
            GithubEndpoints::production(),
            || {
                mutate_settings(|settings| {
                    settings
                        .managed_skills
                        .as_mut()
                        .unwrap()
                        .get_mut("review-changes")
                        .unwrap()
                        .content_hash = "concurrent-content-hash".into();
                })
                .unwrap();
            },
        )
        .unwrap();

        assert_eq!(outcome.checked, 0);
        let record = &load_settings().managed_skills.unwrap()["review-changes"];
        assert_eq!(record.content_hash, "concurrent-content-hash");
        assert_eq!(record.update, SkillUpdateState::default());
    }

    #[test]
    fn local_probe_is_discarded_when_update_state_changes_before_reconciliation() {
        let _home = local_probe_fixture("update-local-state-race");
        let concurrent = SkillUpdateState {
            available: true,
            checked_at: Some("2026-07-17T07:59:59Z".into()),
            resolved_revision: Some("concurrent-revision".into()),
            etag: Some("\"concurrent\"".into()),
            error: None,
            retry_at: None,
        };
        let expected = concurrent.clone();
        let outcome = check_updates_with_reconcile_hook(
            true,
            "2026-07-17T08:00:00Z",
            GithubEndpoints::production(),
            move || {
                mutate_settings(|settings| {
                    settings
                        .managed_skills
                        .as_mut()
                        .unwrap()
                        .get_mut("review-changes")
                        .unwrap()
                        .update = concurrent;
                })
                .unwrap();
            },
        )
        .unwrap();

        assert_eq!(outcome.checked, 0);
        assert_eq!(
            load_settings().managed_skills.unwrap()["review-changes"].update,
            expected
        );
    }
}
