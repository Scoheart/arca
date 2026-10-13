use super::anchored::{AnchoredFileKind, AnchoredIdentity, AnchoredRoot};
use super::files::{
    copy_tree_anchored_private_to_staging, copy_tree_secure_private_to_staging,
    validate_candidate_anchored_private, validate_staging_candidate,
};
use super::staging::{StagingDirectory, StagingOperation, StagingRoot};
use super::{
    capped_message, io_error, SkillCandidateSummary, SkillError, SkillSource, SkillSourceInput,
    SkillSourceResolution, SkillsPaths, MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, MAX_DOWNLOAD_BYTES,
    MAX_SINGLE_FILE_BYTES,
};
use chrono::{Datelike, SecondsFormat, TimeZone, Utc};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
#[cfg(test)]
use std::fs::OpenOptions;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tar::Archive;
use url::Url;
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive};

#[cfg(all(test, unix))]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

const MAX_SOURCE_BYTES: usize = 4096;
const MAX_REF_PROBES: usize = 16;
const MAX_REDIRECTS: usize = 5;
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_RESOLUTION_BYTES: u64 = 1024 * 1024;
const RESOLUTION_FILE: &str = "resolution.json";
const TAR_BLOCK_BYTES: u64 = 512;
const MAX_SYMLINK_HOPS: usize = 64;

#[derive(Debug, Clone)]
pub struct GithubEndpoints {
    pub api_base: Url,
    pub archive_base: Url,
    allowed_hosts: BTreeSet<String>,
    allow_http_loopback: bool,
}

impl GithubEndpoints {
    pub fn production() -> Self {
        Self {
            api_base: Url::parse("https://api.github.com/").unwrap(),
            archive_base: Url::parse("https://codeload.github.com/").unwrap(),
            allowed_hosts: ["github.com", "api.github.com", "codeload.github.com"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            allow_http_loopback: false,
        }
    }

    #[doc(hidden)]
    pub fn for_test(api_base: Url, archive_base: Url) -> Self {
        let hosts = [&api_base, &archive_base]
            .into_iter()
            .map(|url| url.host_str().expect("test endpoint host").to_owned())
            .collect::<BTreeSet<_>>();
        assert!(hosts
            .iter()
            .all(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")));
        Self {
            api_base,
            archive_base,
            allowed_hosts: hosts,
            allow_http_loopback: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GithubRevisionStatus {
    NotModified { etag: Option<String> },
    Resolved { sha: String, etag: Option<String> },
}

pub(crate) fn check_github_revision(
    source: &SkillSource,
    previous_etag: Option<&str>,
    endpoints: &GithubEndpoints,
) -> Result<GithubRevisionStatus, SkillError> {
    validate_github_revision_source(source)?;
    let SkillSource::Github {
        owner,
        repo,
        requested_ref,
        ..
    } = source
    else {
        return invalid_source("only a GitHub source has revision metadata");
    };
    validate_endpoint_base(&endpoints.api_base, endpoints)?;
    let url = endpoint_url(
        &endpoints.api_base,
        &["repos", owner, repo, "commits", requested_ref],
    )?;
    let agent = github_agent()?;
    let response = fetch_response_with_etag(&agent, url, endpoints, previous_etag)?;
    let etag = bounded_response_header(&response, "ETag");
    if response.status() == 304 {
        return Ok(GithubRevisionStatus::NotModified { etag });
    }
    ensure_public_status(&response, "revision")?;
    #[derive(Deserialize)]
    struct Commit {
        sha: String,
    }
    let commit: Commit = read_json(response)?;
    if !is_sha(&commit.sha) {
        return invalid_source("GitHub returned a non-canonical commit SHA");
    }
    Ok(GithubRevisionStatus::Resolved {
        sha: commit.sha.to_ascii_lowercase(),
        etag,
    })
}

pub(crate) fn validate_github_revision_source(source: &SkillSource) -> Result<(), SkillError> {
    let SkillSource::Github {
        owner,
        repo,
        subpath,
        requested_ref,
        pinned,
    } = source
    else {
        return invalid_source("only a GitHub source has revision metadata");
    };
    validate_repository_components(owner, repo)?;
    validate_relative_source_path(subpath)?;
    decode_api_ref(requested_ref)?;
    if *pinned != is_sha(requested_ref) {
        return invalid_source("the recorded GitHub pinned state is inconsistent");
    }
    Ok(())
}

pub fn resolve_source(
    input: SkillSourceInput,
    endpoints: GithubEndpoints,
) -> Result<SkillSourceResolution, SkillError> {
    let paths = SkillsPaths::resolve_from_env().map_err(sanitize_resolution_error)?;
    let operation_id = Uuid::new_v4().hyphenated().to_string();
    let staging = StagingRoot::open_or_create(&paths).map_err(sanitize_resolution_error)?;
    let operation = StagedOperationOwner::new(
        staging
            .create_operation(&operation_id)
            .map_err(sanitize_resolution_error)?,
    );
    let result = match input {
        SkillSourceInput::Github { value } => {
            resolve_github(&value, &endpoints, &operation_id, operation.operation())
        }
        SkillSourceInput::Local { path } => {
            resolve_local(&path, &paths, &operation_id, operation.operation())
        }
        SkillSourceInput::Archive { path } => {
            resolve_archive(&path, &paths, &operation_id, operation.operation())
        }
    };
    let result = result.and_then(|resolution| {
        persist_staged_resolution(operation.operation(), &resolution)?;
        Ok(resolution)
    });
    operation.finish(result).map_err(sanitize_resolution_error)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagedResolutionDocument {
    operation_id: String,
    source: SkillSource,
    resolved_revision: Option<String>,
    candidates: Vec<SkillCandidateSummary>,
}

impl From<&SkillSourceResolution> for StagedResolutionDocument {
    fn from(resolution: &SkillSourceResolution) -> Self {
        Self {
            operation_id: resolution.operation_id.clone(),
            source: resolution.source.clone(),
            resolved_revision: resolution.resolved_revision.clone(),
            candidates: resolution.candidates.clone(),
        }
    }
}

fn persist_staged_resolution(
    operation: &StagingOperation,
    resolution: &SkillSourceResolution,
) -> Result<(), SkillError> {
    validate_canonical_operation_id(&resolution.operation_id)?;
    let document = StagedResolutionDocument::from(resolution);
    let bytes = serde_json::to_vec(&document).map_err(|_| {
        invalid_source_error("the staged Skill resolution could not be encoded safely")
    })?;
    if bytes.len() as u64 > MAX_RESOLUTION_BYTES {
        return Err(SkillError::LimitExceeded {
            limit: "resolution_metadata".into(),
            actual: bytes.len() as u64,
            allowed: MAX_RESOLUTION_BYTES,
        });
    }
    operation.write_private_atomic(RESOLUTION_FILE, &bytes, MAX_RESOLUTION_BYTES)
}

pub(crate) fn load_staged_resolution(
    paths: &SkillsPaths,
    operation_id: &str,
) -> Result<SkillSourceResolution, SkillError> {
    validate_canonical_operation_id(operation_id)?;
    let staging = StagingRoot::open(paths)?;
    let operation = staging.open_operation(operation_id)?;
    let operation_root = operation.root_directory()?;
    let bytes = operation.read_private(RESOLUTION_FILE, MAX_RESOLUTION_BYTES)?;
    let document: StagedResolutionDocument = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_source_error("the staged Skill resolution metadata is malformed"))?;
    let canonical = serde_json::to_vec(&document)
        .map_err(|_| invalid_source_error("the staged Skill resolution metadata is malformed"))?;
    if canonical != bytes {
        return invalid_source("the staged Skill resolution metadata is not canonical");
    }
    if document.operation_id != operation_id {
        return invalid_source("the staged Skill resolution id does not match its operation");
    }
    validate_staged_resolution_document(paths, &operation, &operation_root, &document)?;
    Ok(SkillSourceResolution {
        operation_id: document.operation_id,
        source: document.source,
        resolved_revision: document.resolved_revision,
        candidates: document.candidates,
    })
}

pub(crate) fn stage_private_candidate(
    source: &Path,
    destination: &StagingDirectory,
) -> Result<(), SkillError> {
    copy_tree_secure_private_to_staging(source, destination)?;
    validate_staging_candidate(destination).map(|_| ())
}

pub(crate) fn open_recorded_local_skill(
    paths: &SkillsPaths,
    source: &SkillSource,
) -> Result<AnchoredRoot, SkillError> {
    validate_persisted_source(paths, source, &None)?;
    let SkillSource::Local { path, subpath } = source else {
        return invalid_source("only a Local source has a recorded filesystem subpath");
    };
    let selected = paths
        .expand_user(path)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| invalid_source_error("the recorded local Skill path is invalid"))?;
    let root = fs::canonicalize(&selected)
        .map_err(|_| invalid_source_error("the recorded local Skill path is unavailable"))?;
    open_anchored_directory(&AnchoredRoot::open(&root)?, subpath)
        .map_err(|_| invalid_source_error("the recorded local Skill subpath is unavailable"))
}

pub(crate) fn stage_recorded_skill(
    source: &SkillSource,
    resolved_revision: Option<&str>,
    expected_name: &str,
    endpoints: GithubEndpoints,
) -> Result<SkillSourceResolution, SkillError> {
    if !valid_staged_skill_name(expected_name) {
        return invalid_source("the recorded Skill name is invalid");
    }
    let paths = SkillsPaths::resolve_from_env().map_err(sanitize_resolution_error)?;
    validate_persisted_source(&paths, source, &resolved_revision.map(str::to_owned))
        .map_err(sanitize_resolution_error)?;
    let operation_id = Uuid::new_v4().hyphenated().to_string();
    let staging = StagingRoot::open_or_create(&paths).map_err(sanitize_resolution_error)?;
    let operation = StagedOperationOwner::new(
        staging
            .create_operation(&operation_id)
            .map_err(sanitize_resolution_error)?,
    );
    let result = stage_recorded_skill_inner(
        source,
        resolved_revision,
        expected_name,
        &endpoints,
        &paths,
        &operation_id,
        operation.operation(),
    )
    .and_then(|resolution| {
        persist_staged_resolution(operation.operation(), &resolution)?;
        Ok(resolution)
    });
    operation.finish(result).map_err(sanitize_resolution_error)
}

fn stage_recorded_skill_inner(
    source: &SkillSource,
    resolved_revision: Option<&str>,
    expected_name: &str,
    endpoints: &GithubEndpoints,
    paths: &SkillsPaths,
    operation_id: &str,
    operation: &StagingOperation,
) -> Result<SkillSourceResolution, SkillError> {
    let (summary, revision) = match source {
        SkillSource::Github {
            owner,
            repo,
            subpath,
            ..
        } => {
            let revision = resolved_revision.ok_or_else(|| {
                invalid_source_error("a recorded GitHub Skill requires an immutable revision")
            })?;
            validate_endpoint_base(&endpoints.archive_base, endpoints)?;
            let operation_root = operation.root_directory()?;
            let agent = github_agent()?;
            download_archive(&agent, owner, repo, revision, endpoints, &operation_root)?;
            let archive_root = operation.create_private_directory("archive")?;
            let repository_root = extract_staged_archive(&operation_root, &archive_root)?;
            let requested_root = repository_root.open_directory(subpath)?;
            let summary =
                stage_single_candidate(&requested_root.anchored_root()?, operation, expected_name)?;
            operation.remove_private_directory("archive")?;
            (summary, Some(revision.to_owned()))
        }
        SkillSource::Local { .. } => {
            let candidate = open_recorded_local_skill(paths, source)?;
            (
                stage_single_candidate(&candidate, operation, expected_name)?,
                None,
            )
        }
        SkillSource::Archive { path, subpath } => {
            let archive_root = stage_local_archive(path, paths, operation)?;
            let candidate = open_anchored_directory(&archive_root.anchored_root()?, subpath)
                .map_err(|_| {
                    invalid_source_error("the recorded archive Skill subpath is unavailable")
                })?;
            let summary = stage_single_candidate(&candidate, operation, expected_name)?;
            operation.remove_private_directory("archive")?;
            (summary, None)
        }
        SkillSource::Imported { backup_path, .. } => {
            let backup = paths
                .expand_user(backup_path)
                .filter(|path| path.is_absolute())
                .ok_or_else(|| invalid_source_error("the imported Skill backup path is invalid"))?;
            let backup_root = paths.backups_skills_dir();
            if backup == backup_root || !backup.starts_with(&backup_root) {
                return invalid_source("the imported Skill backup is outside Arca backups");
            }
            let metadata = fs::symlink_metadata(&backup).map_err(|_| {
                invalid_source_error("the imported Skill backup path is unavailable")
            })?;
            if !metadata.file_type().is_dir() {
                return invalid_source("the imported Skill backup is not a real directory");
            }
            let canonical_root = fs::canonicalize(&backup_root)
                .map_err(|_| invalid_source_error("the Arca Skill backup root is unavailable"))?;
            let candidate = fs::canonicalize(&backup).map_err(|_| {
                invalid_source_error("the imported Skill backup path is unavailable")
            })?;
            if candidate == canonical_root || !candidate.starts_with(&canonical_root) {
                return invalid_source("the imported Skill backup escapes Arca backups");
            }
            (
                stage_single_candidate(&AnchoredRoot::open(&candidate)?, operation, expected_name)?,
                None,
            )
        }
    };
    Ok(SkillSourceResolution {
        operation_id: operation_id.to_owned(),
        source: source.clone(),
        resolved_revision: revision,
        candidates: vec![summary],
    })
}

fn stage_single_candidate(
    source: &AnchoredRoot,
    operation: &StagingOperation,
    expected_name: &str,
) -> Result<SkillCandidateSummary, SkillError> {
    let before = validate_candidate_anchored_private(source)?;
    if before.manifest.name != expected_name {
        return invalid_source("the recorded source no longer contains the named Skill");
    }
    let candidates_root = operation.create_private_directory("candidates")?;
    let destination = candidates_root.create_directory(expected_name)?;
    copy_tree_anchored_private_to_staging(source, &destination)?;
    let staged = validate_staging_candidate(&destination)?;
    if staged.manifest.name != before.manifest.name
        || staged.content_hash != before.content_hash
        || staged.total_bytes != before.total_bytes
    {
        return Err(SkillError::PlanStale {
            message: "the recorded Skill changed while its snapshot was staged".into(),
        });
    }
    Ok(SkillCandidateSummary {
        name: staged.manifest.name,
        description: staged.manifest.description,
        relative_path: String::new(),
        content_kind: staged.content_kind,
        content_hash: staged.content_hash,
        file_count: staged.files.len() as u64,
        total_bytes: staged.total_bytes,
    })
}

fn validate_canonical_operation_id(value: &str) -> Result<(), SkillError> {
    let parsed = uuid::Uuid::parse_str(value)
        .map_err(|_| invalid_source_error("the Skills operation id is invalid"))?;
    if parsed.hyphenated().to_string() != value {
        return invalid_source("the Skills operation id is invalid");
    }
    Ok(())
}

fn validate_staged_resolution_document(
    paths: &SkillsPaths,
    operation: &StagingOperation,
    operation_root: &StagingDirectory,
    document: &StagedResolutionDocument,
) -> Result<(), SkillError> {
    validate_persisted_source(paths, &document.source, &document.resolved_revision)?;
    if document.candidates.is_empty() {
        return invalid_source("the staged Skill resolution contains no candidates");
    }
    let mut sorted = document.candidates.clone();
    sorted.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.relative_path.cmp(&right.relative_path))
    });
    if sorted != document.candidates {
        return invalid_source("the staged Skill candidates are not canonically ordered");
    }

    let candidates_root = operation_root
        .open_directory("candidates")
        .map_err(|_| invalid_source_error("the staged Skill candidates are unavailable"))?;
    let mut expected_names = BTreeSet::new();
    for summary in &document.candidates {
        if !valid_staged_skill_name(&summary.name)
            || !valid_relative_candidate_path(&summary.relative_path)
            || summary.content_hash.len() != 64
            || !summary
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || summary.file_count == 0
            || !expected_names.insert(summary.name.clone())
        {
            return invalid_source("the staged Skill candidate metadata is invalid");
        }
        let candidate = candidates_root
            .open_directory(&summary.name)
            .map_err(|_| invalid_source_error("a staged Skill candidate no longer validates"))?;
        let validated = validate_staging_candidate(&candidate)
            .map_err(|_| invalid_source_error("a staged Skill candidate no longer validates"))?;
        if validated.manifest.name != summary.name
            || validated.manifest.description != summary.description
            || validated.content_kind != summary.content_kind
            || validated.content_hash != summary.content_hash
            || validated.files.len() as u64 != summary.file_count
            || validated.total_bytes != summary.total_bytes
        {
            return Err(SkillError::PlanStale {
                message: "a staged Skill candidate changed after resolution".into(),
            });
        }
    }
    let observed_names = operation
        .list_private_directory("candidates")?
        .into_iter()
        .map(|name| {
            if !valid_staged_skill_name(&name) {
                return invalid_source("a staged Skill candidate entry is invalid");
            }
            Ok(name)
        })
        .collect::<Result<BTreeSet<_>, SkillError>>()?;
    if observed_names != expected_names {
        return invalid_source("the staged Skill candidate set changed after resolution");
    }
    Ok(())
}

fn validate_persisted_source(
    paths: &SkillsPaths,
    source: &SkillSource,
    resolved_revision: &Option<String>,
) -> Result<(), SkillError> {
    match (source, resolved_revision) {
        (
            SkillSource::Github {
                owner,
                repo,
                subpath,
                requested_ref,
                pinned,
            },
            Some(revision),
        ) => {
            validate_repository_components(owner, repo)?;
            validate_relative_source_path(subpath)?;
            decode_api_ref(requested_ref)?;
            if *pinned != is_sha(requested_ref) {
                return invalid_source("the staged GitHub pinned state is inconsistent");
            }
            if !is_sha(revision) || revision.bytes().any(|byte| byte.is_ascii_uppercase()) {
                return invalid_source("the staged GitHub revision is not canonical");
            }
            if *pinned && requested_ref != revision {
                return invalid_source("the staged pinned GitHub revision is inconsistent");
            }
            Ok(())
        }
        (SkillSource::Local { path, subpath }, None) => {
            validate_source_text(path)?;
            if path.contains(['\0', '\\']) {
                return invalid_source("the staged local source path is not canonical");
            }
            let expanded = paths
                .expand_user(path)
                .ok_or_else(|| invalid_source_error("the staged local source path is invalid"))?;
            let normalized = lexical_absolute(&expanded)?;
            if collapse_home(&normalized, paths.user_home()) != *path {
                return invalid_source("the staged local source path is not canonical");
            }
            validate_relative_source_path(subpath)
        }
        (SkillSource::Archive { path, subpath }, None) => {
            validate_source_text(path)?;
            if path.contains(['\0', '\\']) {
                return invalid_source("the staged archive source path is not canonical");
            }
            archive_format(Path::new(path))?;
            let expanded = paths
                .expand_user(path)
                .ok_or_else(|| invalid_source_error("the staged archive source path is invalid"))?;
            let normalized = lexical_absolute(&expanded)?;
            if collapse_home(&normalized, paths.user_home()) != *path {
                return invalid_source("the staged archive source path is not canonical");
            }
            validate_relative_source_path(subpath)
        }
        (
            SkillSource::Imported {
                original_path,
                backup_path,
            },
            None,
        ) => {
            let mut normalized = Vec::with_capacity(2);
            for value in [original_path, backup_path] {
                validate_source_text(value)?;
                if value.contains(['\0', '\\']) {
                    return invalid_source("the staged imported source path is not canonical");
                }
                let expanded = paths.expand_user(value).ok_or_else(|| {
                    invalid_source_error("the staged imported source path is invalid")
                })?;
                let absolute = lexical_absolute(&expanded)?;
                if collapse_home(&absolute, paths.user_home()) != *value {
                    return invalid_source("the staged imported source path is not canonical");
                }
                normalized.push(absolute);
            }
            let backup = &normalized[1];
            let backup_root = paths.backups_skills_dir();
            if backup == &backup_root || !backup.starts_with(&backup_root) {
                return invalid_source("the imported Skill backup is outside Arca backups");
            }
            Ok(())
        }
        _ => invalid_source("the staged Skill source revision is inconsistent"),
    }
}

fn validate_relative_source_path(value: &str) -> Result<(), SkillError> {
    if value.is_empty() {
        return Ok(());
    }
    validate_source_text(value)?;
    if value.starts_with('/') || value.contains(['\\', '\0']) {
        return invalid_source("the staged Skill source subpath is not canonical");
    }
    for component in value.split('/') {
        validate_decoded_source_component(component)?;
    }
    Ok(())
}

fn valid_staged_skill_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

fn valid_relative_candidate_path(value: &str) -> bool {
    value.is_empty()
        || (!value.starts_with('/')
            && !value.contains('\\')
            && value
                .split('/')
                .all(|component| !component.is_empty() && !matches!(component, "." | "..")))
}

struct StagedOperationOwner {
    operation: StagingOperation,
    armed: bool,
}

impl StagedOperationOwner {
    fn new(operation: StagingOperation) -> Self {
        Self {
            operation,
            armed: true,
        }
    }

    fn operation(&self) -> &StagingOperation {
        &self.operation
    }

    fn finish<T>(mut self, result: Result<T, SkillError>) -> Result<T, SkillError> {
        self.armed = false;
        let Err(original) = result else {
            return result;
        };
        self.operation
            .remove()
            .map_err(|_| SkillError::RecoveryRequired {
                message: "a failed staged source could not be removed; manual recovery is required"
                    .into(),
            })?;
        Err(original)
    }
}

impl Drop for StagedOperationOwner {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.operation.remove();
        }
    }
}

#[cfg(test)]
struct OperationDirectory {
    path: PathBuf,
    armed: bool,
}

#[cfg(test)]
impl OperationDirectory {
    #[cfg(test)]
    fn create(path: PathBuf) -> Result<Self, SkillError> {
        create_private_directory(&path)?;
        Self::adopt(path)
    }

    fn adopt(path: PathBuf) -> Result<Self, SkillError> {
        let owner = Self { path, armed: true };
        if let Err(error) = owner.verify_setup() {
            return owner.finish(Err(error));
        }
        Ok(owner)
    }

    fn verify_setup(&self) -> Result<(), SkillError> {
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|error| io_error(&self.path, error))?;
        if !metadata.file_type().is_dir() {
            return invalid_source("the staged source operation root is not a private directory");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return invalid_source("the staged source operation root is not private");
            }
        }
        Ok(())
    }

    fn finish<T>(mut self, result: Result<T, SkillError>) -> Result<T, SkillError> {
        self.armed = false;
        finish_operation_with_cleanup(result, &self.path, |path| fs::remove_dir_all(path))
    }
}

#[cfg(test)]
impl Drop for OperationDirectory {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
fn finish_operation_with_cleanup<T, F>(
    result: Result<T, SkillError>,
    operation: &Path,
    cleanup: F,
) -> Result<T, SkillError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let Err(original) = result else {
        return result;
    };
    cleanup(operation).map_err(|_| SkillError::RecoveryRequired {
        message: "a failed staged source could not be removed; manual recovery is required".into(),
    })?;
    Err(original)
}

struct ParsedGithub {
    owner: String,
    repo: String,
    tree: Option<Vec<String>>,
}

struct ResolvedGithub {
    requested_ref: String,
    subpath: String,
    pinned: bool,
    sha: String,
}

fn resolve_github(
    value: &str,
    endpoints: &GithubEndpoints,
    operation_id: &str,
    operation: &StagingOperation,
) -> Result<SkillSourceResolution, SkillError> {
    let parsed = parse_github_source(value)?;
    validate_endpoint_base(&endpoints.api_base, endpoints)?;
    validate_endpoint_base(&endpoints.archive_base, endpoints)?;
    let agent = github_agent()?;
    let resolved = resolve_github_metadata(&agent, &parsed, endpoints)?;
    let operation_root = operation.root_directory()?;
    download_archive(
        &agent,
        &parsed.owner,
        &parsed.repo,
        &resolved.sha,
        endpoints,
        &operation_root,
    )?;
    let archive_root = operation.create_private_directory("archive")?;
    let repository_root = extract_staged_archive(&operation_root, &archive_root)?;
    let requested_root = repository_root.open_directory(&resolved.subpath)?;
    let candidates = stage_candidates_anchored(&requested_root.anchored_root()?, operation)?;
    operation.remove_private_directory("archive")?;
    Ok(SkillSourceResolution {
        operation_id: operation_id.to_owned(),
        source: SkillSource::Github {
            owner: parsed.owner,
            repo: parsed.repo,
            subpath: resolved.subpath,
            requested_ref: resolved.requested_ref,
            pinned: resolved.pinned,
        },
        resolved_revision: Some(resolved.sha),
        candidates,
    })
}

fn resolve_local(
    value: &str,
    paths: &SkillsPaths,
    operation_id: &str,
    operation: &StagingOperation,
) -> Result<SkillSourceResolution, SkillError> {
    validate_source_text(value)?;
    if value.contains('\0') {
        return invalid_source("local source paths cannot contain NUL bytes");
    }
    let selected = paths
        .expand_user(value)
        .ok_or_else(|| invalid_source_error("the local source path is not valid"))?;
    let selected = lexical_absolute(&selected)?;
    let canonical = fs::canonicalize(&selected).map_err(|error| io_error(&selected, error))?;
    let metadata = fs::metadata(&canonical).map_err(|error| io_error(&canonical, error))?;
    if !metadata.is_dir() {
        return Err(SkillError::InvalidSource {
            message: "the selected local source must be a directory".into(),
        });
    }
    let display_path = collapse_home(&selected, paths.user_home());
    let candidates = stage_candidates_anchored(&AnchoredRoot::open(&canonical)?, operation)?;
    Ok(SkillSourceResolution {
        operation_id: operation_id.to_owned(),
        source: SkillSource::Local {
            path: display_path,
            subpath: String::new(),
        },
        resolved_revision: None,
        candidates,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalArchiveFormat {
    Zip,
    TarGz,
    Tar,
}

fn archive_format(path: &Path) -> Result<LocalArchiveFormat, SkillError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid_source_error("the archive filename is not valid UTF-8"))?
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        Ok(LocalArchiveFormat::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Ok(LocalArchiveFormat::TarGz)
    } else if name.ends_with(".tar") {
        Ok(LocalArchiveFormat::Tar)
    } else {
        invalid_source("Skill archives must use .zip, .tar.gz, .tgz, or .tar")
    }
}

fn open_local_archive(
    value: &str,
    paths: &SkillsPaths,
) -> Result<(File, LocalArchiveFormat, String), SkillError> {
    validate_source_text(value)?;
    if value.contains('\0') {
        return invalid_source("archive source paths cannot contain NUL bytes");
    }
    let selected = paths
        .expand_user(value)
        .ok_or_else(|| invalid_source_error("the archive source path is not valid"))?;
    let selected = lexical_absolute(&selected)?;
    let format = archive_format(&selected)?;
    let canonical = fs::canonicalize(&selected).map_err(|error| io_error(&selected, error))?;
    let file = File::open(&canonical).map_err(|error| io_error(&canonical, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error(&canonical, error))?;
    if !metadata.is_file() {
        return invalid_source("the selected Skill archive must be a regular file");
    }
    enforce_limit("download", metadata.len(), MAX_DOWNLOAD_BYTES)?;
    Ok((file, format, collapse_home(&selected, paths.user_home())))
}

fn copy_local_archive(
    mut source: File,
    destination: &StagingDirectory,
    name: &str,
) -> Result<(), SkillError> {
    let declared = source
        .metadata()
        .map_err(|error| io_error(destination.path(), error))?
        .len();
    enforce_limit("download", declared, MAX_DOWNLOAD_BYTES)?;
    let mut output = destination.create_file(name, 0)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let remaining = MAX_DOWNLOAD_BYTES.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(
                "download",
                total.saturating_add(1),
                MAX_DOWNLOAD_BYTES,
            ));
        }
        let read = source
            .read(&mut buffer[..requested])
            .map_err(|error| io_error(destination.path(), error))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        enforce_limit("download", total, MAX_DOWNLOAD_BYTES)?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(destination.path(), error))?;
    }
    if total != declared {
        return invalid_source("the selected Skill archive changed while it was staged");
    }
    output
        .flush()
        .map_err(|error| io_error(destination.path(), error))
}

fn stage_local_archive(
    value: &str,
    paths: &SkillsPaths,
    operation: &StagingOperation,
) -> Result<StagingDirectory, SkillError> {
    let (file, format, _) = open_local_archive(value, paths)?;
    let operation_root = operation.root_directory()?;
    let archive_root = operation.create_private_directory("archive")?;
    match format {
        LocalArchiveFormat::Zip => {
            copy_local_archive(file, &operation_root, "source.zip")?;
            let staged = operation_root.open_file("source.zip")?;
            materialize_zip_file(staged, &archive_root)?;
            operation_root.remove_file("source.zip")?;
        }
        LocalArchiveFormat::TarGz => {
            copy_local_archive(file, &operation_root, "source.tar.gz")?;
            decompress_staged_archive(&operation_root)?;
            operation_root.remove_file("source.tar.gz")?;
            let raw_tar = operation_root.open_file("source.tar")?;
            let preflight = preflight_tar_file(raw_tar, &operation_root.path().join("source.tar"))?;
            let raw_tar = operation_root.open_file("source.tar")?;
            materialize_archive_file(raw_tar, &archive_root, &preflight.normalized_paths, false)?;
            operation_root.remove_file("source.tar")?;
        }
        LocalArchiveFormat::Tar => {
            copy_local_archive(file, &operation_root, "source.tar")?;
            let raw_tar = operation_root.open_file("source.tar")?;
            let preflight = preflight_tar_file(raw_tar, &operation_root.path().join("source.tar"))?;
            let raw_tar = operation_root.open_file("source.tar")?;
            materialize_archive_file(raw_tar, &archive_root, &preflight.normalized_paths, false)?;
            operation_root.remove_file("source.tar")?;
        }
    }
    Ok(archive_root)
}

fn resolve_archive(
    value: &str,
    paths: &SkillsPaths,
    operation_id: &str,
    operation: &StagingOperation,
) -> Result<SkillSourceResolution, SkillError> {
    let (_, _, display_path) = open_local_archive(value, paths)?;
    let archive_root = stage_local_archive(&display_path, paths, operation)?;
    let candidates = stage_candidates_anchored(&archive_root.anchored_root()?, operation)?;
    operation.remove_private_directory("archive")?;
    Ok(SkillSourceResolution {
        operation_id: operation_id.to_owned(),
        source: SkillSource::Archive {
            path: display_path,
            subpath: String::new(),
        },
        resolved_revision: None,
        candidates,
    })
}

fn parse_github_source(value: &str) -> Result<ParsedGithub, SkillError> {
    validate_source_text(value)?;
    if value.contains('\\') {
        return invalid_source("GitHub source paths cannot contain backslashes");
    }
    if !value.contains("://") {
        if value.contains(['@', ':', '?', '#']) {
            return invalid_source("only public owner/repository GitHub sources are supported");
        }
        let raw = value.split('/').collect::<Vec<_>>();
        if raw.len() != 2 {
            return invalid_source("GitHub shorthand must be owner/repository");
        }
        let owner = decode_source_component(raw[0])?;
        let repo = decode_source_component(raw[1])?;
        validate_repository_components(&owner, &repo)?;
        return Ok(ParsedGithub {
            owner,
            repo,
            tree: None,
        });
    }

    let url = Url::parse(value)
        .map_err(|_| invalid_source_error("the GitHub source URL is not valid"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return invalid_source("only credential-free public https://github.com URLs are supported");
    }
    let raw_path = source_url_path(value)?;
    let mut raw = raw_path.split('/').skip(1).collect::<Vec<_>>();
    if raw.last() == Some(&"") {
        raw.pop();
    }
    if raw.iter().any(|component| component.is_empty()) || raw.len() < 2 {
        return invalid_source("the GitHub URL must contain an owner and repository");
    }
    let components = raw
        .into_iter()
        .map(decode_source_component)
        .collect::<Result<Vec<_>, _>>()?;
    let owner = components[0].clone();
    let repo = components[1].clone();
    validate_repository_components(&owner, &repo)?;
    let tree = if components.len() == 2 {
        None
    } else {
        if components.get(2).map(String::as_str) != Some("tree") || components.len() == 3 {
            return invalid_source("GitHub URLs may only select a repository or tree path");
        }
        let tree = components[3..].to_vec();
        if tree.len() > MAX_REF_PROBES && !is_sha(&tree[0]) {
            return invalid_source("the tree URL would require too many ref probes");
        }
        Some(tree)
    };
    Ok(ParsedGithub { owner, repo, tree })
}

fn validate_source_text(value: &str) -> Result<(), SkillError> {
    if value.is_empty() || value.len() > MAX_SOURCE_BYTES || value.trim() != value {
        return invalid_source("the source must contain 1 to 4096 bytes without outer whitespace");
    }
    Ok(())
}

fn source_url_path(value: &str) -> Result<&str, SkillError> {
    let authority = value
        .find("://")
        .map(|index| index + 3)
        .ok_or_else(|| invalid_source_error("the GitHub source URL is not valid"))?;
    let path = value[authority..].find('/').map(|index| authority + index);
    Ok(path.map(|index| &value[index..]).unwrap_or("/"))
}

fn decode_source_component(value: &str) -> Result<String, SkillError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(high) = bytes.get(index + 1).and_then(|value| hex_value(*value)) else {
                return invalid_source("source path percent encoding is not canonical");
            };
            let Some(low) = bytes.get(index + 2).and_then(|value| hex_value(*value)) else {
                return invalid_source("source path percent encoding is not canonical");
            };
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded)
        .map_err(|_| invalid_source_error("source path components must be valid UTF-8"))?;
    validate_decoded_source_component(&decoded)?;
    Ok(decoded)
}

fn validate_decoded_source_component(value: &str) -> Result<(), SkillError> {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.contains(['/', '\\', '\0'])
        || value.chars().any(char::is_control)
    {
        return invalid_source("source path components are not safe");
    }
    Ok(())
}

fn validate_repository_components(owner: &str, repo: &str) -> Result<(), SkillError> {
    validate_decoded_source_component(owner)?;
    validate_decoded_source_component(repo)?;
    if owner.is_empty()
        || repo.is_empty()
        || repo.to_ascii_lowercase().ends_with(".git")
        || [owner, repo].into_iter().any(|component| {
            component
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        })
    {
        return invalid_source("the GitHub owner or repository is not valid");
    }
    Ok(())
}

fn resolve_github_metadata(
    agent: &ureq::Agent,
    parsed: &ParsedGithub,
    endpoints: &GithubEndpoints,
) -> Result<ResolvedGithub, SkillError> {
    if let Some(tree) = &parsed.tree {
        if is_sha(&tree[0]) {
            let sha = resolve_commit(agent, parsed, &tree[0], endpoints)?.ok_or_else(|| {
                invalid_source_error("the requested public GitHub revision was not found")
            })?;
            return Ok(ResolvedGithub {
                requested_ref: tree[0].clone(),
                subpath: tree[1..].join("/"),
                pinned: true,
                sha,
            });
        }
        for length in (1..=tree.len()).rev().take(MAX_REF_PROBES) {
            let requested_ref = tree[..length].join("/");
            if let Some(sha) = resolve_commit(agent, parsed, &requested_ref, endpoints)? {
                return Ok(ResolvedGithub {
                    requested_ref,
                    subpath: tree[length..].join("/"),
                    pinned: false,
                    sha,
                });
            }
        }
        return invalid_source("the requested public GitHub revision was not found");
    }

    let url = endpoint_url(&endpoints.api_base, &["repos", &parsed.owner, &parsed.repo])?;
    let response = fetch_response(agent, url, endpoints)?;
    ensure_public_status(&response, "repository")?;
    #[derive(Deserialize)]
    struct Repository {
        default_branch: String,
    }
    let repository: Repository = read_json(response)?;
    let requested_ref = decode_api_ref(&repository.default_branch)?;
    let sha = resolve_commit(agent, parsed, &requested_ref, endpoints)?.ok_or_else(|| {
        invalid_source_error("the default branch of the public repository was not found")
    })?;
    Ok(ResolvedGithub {
        requested_ref,
        subpath: String::new(),
        pinned: false,
        sha,
    })
}

fn resolve_commit(
    agent: &ureq::Agent,
    parsed: &ParsedGithub,
    requested_ref: &str,
    endpoints: &GithubEndpoints,
) -> Result<Option<String>, SkillError> {
    let url = endpoint_url(
        &endpoints.api_base,
        &[
            "repos",
            &parsed.owner,
            &parsed.repo,
            "commits",
            requested_ref,
        ],
    )?;
    let response = fetch_response(agent, url, endpoints)?;
    if response.status() == 404 {
        return Ok(None);
    }
    ensure_public_status(&response, "revision")?;
    #[derive(Deserialize)]
    struct Commit {
        sha: String,
    }
    let commit: Commit = read_json(response)?;
    if !is_sha(&commit.sha) {
        return invalid_source("GitHub returned a non-canonical commit SHA");
    }
    Ok(Some(commit.sha.to_ascii_lowercase()))
}

fn decode_api_ref(value: &str) -> Result<String, SkillError> {
    if value.is_empty()
        || value.len() > MAX_SOURCE_BYTES
        || value.contains(['\0', '\\'])
        || value
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return invalid_source("GitHub returned an invalid default branch");
    }
    for component in value.split('/') {
        validate_decoded_source_component(component)?;
    }
    Ok(value.to_owned())
}

fn is_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn github_agent() -> Result<ureq::Agent, SkillError> {
    crate::network::build_ureq_agent(
        ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_body(Some(Duration::from_secs(30))),
    )
    .map_err(|message| SkillError::Network {
        message,
        retry_at: None,
    })
}

fn endpoint_url(base: &Url, segments: &[&str]) -> Result<Url, SkillError> {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    let mut path = url.path_segments_mut().map_err(|_| {
        invalid_source_error("the GitHub endpoint base cannot contain path segments")
    })?;
    path.pop_if_empty();
    path.extend(segments.iter().copied());
    drop(path);
    Ok(url)
}

fn validate_endpoint_base(url: &Url, endpoints: &GithubEndpoints) -> Result<(), SkillError> {
    validate_request_url(url, endpoints)
}

fn validate_request_url(url: &Url, endpoints: &GithubEndpoints) -> Result<(), SkillError> {
    let Some(host) = url.host_str() else {
        return invalid_source("GitHub endpoint URLs must have a host");
    };
    if !endpoints.allowed_hosts.contains(host)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return invalid_source("a GitHub request or redirect left the approved public hosts");
    }
    if url.scheme() == "https"
        && (endpoints.allow_http_loopback || url.port_or_known_default() == Some(443))
    {
        return Ok(());
    }
    if endpoints.allow_http_loopback
        && url.scheme() == "http"
        && matches!(host, "127.0.0.1" | "localhost" | "::1")
    {
        return Ok(());
    }
    invalid_source("GitHub requests and redirects must use HTTPS")
}

fn fetch_response(
    agent: &ureq::Agent,
    url: Url,
    endpoints: &GithubEndpoints,
) -> Result<ureq::http::Response<ureq::Body>, SkillError> {
    fetch_response_with_etag(agent, url, endpoints, None)
}

fn fetch_response_with_etag(
    agent: &ureq::Agent,
    mut url: Url,
    endpoints: &GithubEndpoints,
    previous_etag: Option<&str>,
) -> Result<ureq::http::Response<ureq::Body>, SkillError> {
    let previous_etag = previous_etag
        .map(|value| {
            bounded_header_value(value)
                .ok_or_else(|| invalid_source_error("the stored GitHub ETag is invalid"))
        })
        .transpose()?;
    let mut redirects = 0_usize;
    loop {
        validate_request_url(&url, endpoints)?;
        let request = agent
            .get(url.as_str())
            .header("User-Agent", concat!("Arca/", env!("CARGO_PKG_VERSION")))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        let request = match previous_etag.as_deref() {
            Some(etag) => request.header("If-None-Match", etag),
            None => request,
        };
        let response = request.call().map_err(|_| SkillError::Network {
            message: "the public GitHub request could not be completed".into(),
            retry_at: None,
        })?;
        if matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            if redirects >= MAX_REDIRECTS {
                return invalid_source("the GitHub redirect limit was exceeded");
            }
            let location = response
                .headers()
                .get("Location")
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty() && value.len() <= MAX_SOURCE_BYTES)
                .ok_or_else(|| {
                    invalid_source_error("the GitHub redirect is missing a safe location")
                })?;
            url = url
                .join(location)
                .map_err(|_| invalid_source_error("the GitHub redirect location is invalid"))?;
            redirects += 1;
            continue;
        }
        return Ok(response);
    }
}

fn ensure_public_status(
    response: &ureq::http::Response<ureq::Body>,
    resource: &str,
) -> Result<(), SkillError> {
    let (rate_limited, retry_at) = rate_limit_evidence(response);
    match response.status().as_u16() {
        200..=299 => Ok(()),
        403 if rate_limited => Err(SkillError::Network {
            message: "GitHub rate-limited the public source request".into(),
            retry_at,
        }),
        401 | 403 | 404 => invalid_source(&format!(
            "the {resource} is unavailable as an unauthenticated public GitHub source"
        )),
        429 => Err(SkillError::Network {
            message: "GitHub rate-limited the public source request".into(),
            retry_at,
        }),
        _ => Err(SkillError::Network {
            message: "GitHub returned an unsuccessful response for the public source".into(),
            retry_at: None,
        }),
    }
}

fn rate_limit_evidence(response: &ureq::http::Response<ureq::Body>) -> (bool, Option<String>) {
    let remaining_zero = bounded_response_header(response, "X-RateLimit-Remaining")
        .is_some_and(|value| value == "0");
    let retry_after = bounded_response_header(response, "Retry-After");
    let reset = bounded_response_header(response, "X-RateLimit-Reset")
        .and_then(|value| github_reset_retry_at(&value));
    let rate_limited = remaining_zero || retry_after.is_some() || reset.is_some();
    let retry_at = retry_after.as_deref().and_then(|value| github_retry_after_at(value, Utc::now()));
    (rate_limited, reset.or(retry_at))
}

/// HTTP Retry-After is a delay in seconds or an HTTP date, never an opaque
/// retry_at value. Return the same RFC3339 contract as X-RateLimit-Reset.
fn github_retry_after_at(value: &str, now: chrono::DateTime<Utc>) -> Option<String> {
    let retry = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        let delay = value.parse::<i64>().ok().and_then(chrono::TimeDelta::try_seconds)?;
        now.checked_add_signed(delay)?
    } else {
        chrono::DateTime::parse_from_rfc2822(value).ok()?.with_timezone(&Utc)
    };
    if !(0..=9999).contains(&retry.year()) { return None; }
    Some(retry.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn github_reset_retry_at(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let seconds = value.parse::<i64>().ok()?;
    let reset = Utc.timestamp_opt(seconds, 0).single()?;
    if reset.year() > 9999 {
        return None;
    }
    Some(reset.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn bounded_response_header(
    response: &ureq::http::Response<ureq::Body>,
    name: &str,
) -> Option<String> {
    bounded_header_value(response.headers().get(name)?.to_str().ok()?)
}

fn bounded_header_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return None;
    }
    Some(capped_message(value))
}

fn read_json<T: for<'de> Deserialize<'de>>(
    response: ureq::http::Response<ureq::Body>,
) -> Result<T, SkillError> {
    let bytes = read_response_bounded(response, MAX_METADATA_BYTES, "metadata")?;
    serde_json::from_slice(&bytes)
        .map_err(|_| invalid_source_error("GitHub returned invalid public source metadata"))
}

fn read_response_bounded(
    mut response: ureq::http::Response<ureq::Body>,
    maximum: u64,
    limit: &'static str,
) -> Result<Vec<u8>, SkillError> {
    if let Some(length) = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
    {
        let length = length
            .parse::<u64>()
            .map_err(|_| invalid_source_error("the response Content-Length is invalid"))?;
        enforce_limit(limit, length, maximum)?;
    }
    let mut reader = response.body_mut().as_reader();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let remaining = maximum.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(limit, total.saturating_add(1), maximum));
        }
        let read = reader
            .read(&mut buffer[..requested])
            .map_err(|_| SkillError::Network {
                message: "the public GitHub response ended unexpectedly".into(),
                retry_at: None,
            })?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        enforce_limit(limit, total, maximum)?;
        bytes.extend_from_slice(&buffer[..read]);
    }
    Ok(bytes)
}

fn download_archive(
    agent: &ureq::Agent,
    owner: &str,
    repo: &str,
    sha: &str,
    endpoints: &GithubEndpoints,
    destination: &StagingDirectory,
) -> Result<(), SkillError> {
    let url = endpoint_url(&endpoints.archive_base, &[owner, repo, "tar.gz", sha])?;
    let mut response = fetch_response(agent, url, endpoints)?;
    ensure_public_status(&response, "archive")?;
    let declared = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| invalid_source_error("the archive Content-Length is invalid"))
        })
        .transpose()?;
    if let Some(declared) = declared {
        enforce_limit("download", declared, MAX_DOWNLOAD_BYTES)?;
    }
    let mut source = response.body_mut().as_reader();
    let mut output = destination.create_file("source.tar.gz", 0)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let remaining = MAX_DOWNLOAD_BYTES.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(
                "download",
                total.saturating_add(1),
                MAX_DOWNLOAD_BYTES,
            ));
        }
        let read = source
            .read(&mut buffer[..requested])
            .map_err(|_| SkillError::Network {
                message: "the public GitHub archive download ended unexpectedly".into(),
                retry_at: None,
            })?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        enforce_limit("download", total, MAX_DOWNLOAD_BYTES)?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(destination.path(), error))?;
    }
    if declared.is_some_and(|declared| declared != total) {
        return invalid_source("the archive size did not match its Content-Length");
    }
    output
        .flush()
        .map_err(|error| io_error(destination.path(), error))
}

struct PendingSymlink {
    relative: String,
    target: String,
    destination: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlannedNodeKind {
    Directory,
    File,
    Symlink,
}

#[cfg(test)]
fn extract_archive(download: &Path, destination: &Path) -> Result<PathBuf, SkillError> {
    let raw_tar = destination
        .parent()
        .ok_or_else(|| invalid_source_error("the archive staging layout is invalid"))?
        .join("source.tar");
    decompress_archive(download, &raw_tar)?;
    remove_transient_file(download)?;
    let preflight = preflight_tar(&raw_tar)?;
    let repository_root = materialize_archive(&raw_tar, destination, &preflight.normalized_paths)?;
    remove_transient_file(&raw_tar)?;
    Ok(repository_root)
}

fn extract_staged_archive(
    operation: &StagingDirectory,
    destination: &StagingDirectory,
) -> Result<StagingDirectory, SkillError> {
    decompress_staged_archive(operation)?;
    operation.remove_file("source.tar.gz")?;
    let raw_tar = operation.open_file("source.tar")?;
    let preflight = preflight_tar_file(raw_tar, &operation.path().join("source.tar"))?;
    let raw_tar = operation.open_file("source.tar")?;
    let repository_root =
        materialize_staged_archive(raw_tar, destination, &preflight.normalized_paths)?;
    operation.remove_file("source.tar")?;
    Ok(repository_root)
}

#[cfg(test)]
fn remove_transient_file(path: &Path) -> Result<(), SkillError> {
    remove_transient_with(path, |path| fs::remove_file(path))
}

#[cfg(test)]
fn remove_transient_with<F>(path: &Path, remove: F) -> Result<(), SkillError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    remove(path).map_err(|error| io_error(path, error))
}

#[cfg(test)]
fn decompress_archive(download: &Path, raw_tar: &Path) -> Result<(), SkillError> {
    let file = File::open(download).map_err(|error| io_error(download, error))?;
    let mut decoder = GzDecoder::new(file);
    let mut output = create_private_file(raw_tar, 0)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let remaining = MAX_ARCHIVE_BYTES.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(
                "archive",
                total.saturating_add(1),
                MAX_ARCHIVE_BYTES,
            ));
        }
        let read = decoder
            .read(&mut buffer[..requested])
            .map_err(|_| invalid_source_error("the gzip archive is malformed"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        enforce_limit("archive", total, MAX_ARCHIVE_BYTES)?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(raw_tar, error))?;
    }
    output.flush().map_err(|error| io_error(raw_tar, error))
}

fn decompress_staged_archive(operation: &StagingDirectory) -> Result<(), SkillError> {
    let file = operation.open_file("source.tar.gz")?;
    let mut decoder = GzDecoder::new(file);
    let mut output = operation.create_file("source.tar", 0)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let remaining = MAX_ARCHIVE_BYTES.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(
                "archive",
                total.saturating_add(1),
                MAX_ARCHIVE_BYTES,
            ));
        }
        let read = decoder
            .read(&mut buffer[..requested])
            .map_err(|_| invalid_source_error("the gzip archive is malformed"))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        enforce_limit("archive", total, MAX_ARCHIVE_BYTES)?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| io_error(operation.path(), error))?;
    }
    output
        .flush()
        .map_err(|error| io_error(operation.path(), error))
}

struct TarPreflight {
    normalized_paths: Vec<String>,
}

#[cfg(test)]
fn preflight_tar(raw_tar: &Path) -> Result<TarPreflight, SkillError> {
    let file = File::open(raw_tar).map_err(|error| io_error(raw_tar, error))?;
    preflight_tar_file(file, raw_tar)
}

fn preflight_tar_file(mut file: File, raw_tar: &Path) -> Result<TarPreflight, SkillError> {
    let length = file
        .metadata()
        .map_err(|error| io_error(raw_tar, error))?
        .len();
    if length % TAR_BLOCK_BYTES != 0 {
        return invalid_source("the tar archive has invalid 512-byte framing");
    }

    let mut offset = 0_u64;
    let mut physical_headers = 0_u64;
    let mut unsupported_extension = false;
    let mut github_global_pax_seen = false;
    let mut normalized_paths = Vec::new();
    let mut block = [0_u8; TAR_BLOCK_BYTES as usize];
    while offset < length {
        read_tar_block(&mut file, &mut block)?;
        offset = offset.saturating_add(TAR_BLOCK_BYTES);
        if is_zero_tar_block(&block) {
            if offset >= length {
                return invalid_source("the tar archive is missing its second end marker");
            }
            read_tar_block(&mut file, &mut block)?;
            offset = offset.saturating_add(TAR_BLOCK_BYTES);
            if !is_zero_tar_block(&block) {
                return invalid_source("the tar archive has a torn end marker");
            }
            while offset < length {
                read_tar_block(&mut file, &mut block)?;
                offset = offset.saturating_add(TAR_BLOCK_BYTES);
                if !is_zero_tar_block(&block) {
                    return invalid_source("the tar archive contains non-zero trailing data");
                }
            }
            return if unsupported_extension {
                invalid_source(
                    "unsupported archive extension: PAX, GNU long-name/link, and sparse records are not accepted",
                )
            } else {
                Ok(TarPreflight { normalized_paths })
            };
        }

        physical_headers = physical_headers.saturating_add(1);
        enforce_limit("entries", physical_headers, MAX_ARCHIVE_ENTRIES)?;
        validate_tar_checksum(&block)?;
        validate_posix_ustar_header(&block)?;
        let size = parse_tar_octal(&block[124..136], "size", true)?;
        let kind = block[156];
        if matches!(kind, 0 | b'0' | b'7') {
            enforce_limit("single_file", size, MAX_SINGLE_FILE_BYTES)?;
        }
        if kind == b'g' {
            if github_global_pax_seen || !normalized_paths.is_empty() {
                return invalid_source(
                    "the GitHub global PAX header must appear exactly once first",
                );
            }
            validate_github_global_pax_header(&mut file, &block, size)?;
            github_global_pax_seen = true;
        } else if matches!(kind, b'x' | b'L' | b'K' | b'S') {
            unsupported_extension = true;
        } else {
            normalized_paths.push(normalize_raw_ustar_path(&block, kind)?);
        }
        if kind == b'2' {
            validate_tar_string_padding(&block[157..257], "symlink target")?;
        }

        let padded = size
            .checked_add(TAR_BLOCK_BYTES - 1)
            .map(|value| value / TAR_BLOCK_BYTES * TAR_BLOCK_BYTES)
            .ok_or_else(|| invalid_source_error("the tar entry size is invalid"))?;
        let end = offset
            .checked_add(padded)
            .ok_or_else(|| invalid_source_error("the tar entry size is invalid"))?;
        if end > length {
            return invalid_source("the tar entry size exceeds the available framed data");
        }
        file.seek(SeekFrom::Start(end))
            .map_err(|error| io_error(raw_tar, error))?;
        offset = end;
    }
    invalid_source("the tar archive is missing its two end markers")
}

fn validate_github_global_pax_header(
    file: &mut File,
    block: &[u8; 512],
    size: u64,
) -> Result<(), SkillError> {
    let name = tar_string_bytes(&block[..100], "header name")?;
    let prefix = tar_string_bytes(&block[345..500], "header prefix")?;
    if name != b"pax_global_header" || !prefix.is_empty() {
        return invalid_source("unsupported archive extension: unrecognized global PAX header");
    }
    enforce_limit("global_pax", size, MAX_SOURCE_BYTES as u64)?;
    let mut payload = vec![0_u8; size as usize];
    file.read_exact(&mut payload)
        .map_err(|_| invalid_source_error("the global PAX header is truncated"))?;

    let mut records = tar::PaxExtensions::new(&payload);
    let Some(record) = records.next() else {
        return invalid_source("unsupported archive extension: empty global PAX header");
    };
    let record = record.map_err(|_| {
        invalid_source_error("unsupported archive extension: malformed global PAX header")
    })?;
    if record.key_bytes() != b"comment"
        || record.value_bytes().len() != 40
        || !record.value_bytes().iter().all(u8::is_ascii_hexdigit)
        || records.next().is_some()
    {
        return invalid_source(
            "unsupported archive extension: global PAX metadata is not a commit comment",
        );
    }
    Ok(())
}

fn read_tar_block(file: &mut File, block: &mut [u8; 512]) -> Result<(), SkillError> {
    file.read_exact(block)
        .map_err(|_| invalid_source_error("the tar archive is truncated inside a 512-byte block"))
}

fn is_zero_tar_block(block: &[u8; 512]) -> bool {
    block.iter().all(|byte| *byte == 0)
}

fn validate_tar_checksum(block: &[u8; 512]) -> Result<(), SkillError> {
    let stored = parse_tar_octal(&block[148..156], "checksum", false)?;
    let calculated = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                b' ' as u64
            } else {
                *byte as u64
            }
        })
        .sum::<u64>();
    if stored != calculated {
        return invalid_source("the tar header checksum is invalid");
    }
    Ok(())
}

fn validate_posix_ustar_header(block: &[u8; 512]) -> Result<(), SkillError> {
    if &block[257..263] != b"ustar\0" || &block[263..265] != b"00" {
        return invalid_source(
            "unsupported tar header dialect: only canonical POSIX USTAR headers are accepted",
        );
    }
    if block[500..512].iter().any(|byte| *byte != 0) {
        return invalid_source("unsupported tar header layout: POSIX USTAR padding must be zero");
    }
    tar_string_bytes(&block[..100], "header name")?;
    tar_string_bytes(&block[345..500], "header prefix")?;
    Ok(())
}

fn normalize_raw_ustar_path(block: &[u8; 512], kind: u8) -> Result<String, SkillError> {
    let name = tar_string_bytes(&block[..100], "header name")?;
    let prefix = tar_string_bytes(&block[345..500], "header prefix")?;
    let mut path = Vec::with_capacity(prefix.len() + usize::from(!prefix.is_empty()) + name.len());
    if !prefix.is_empty() {
        path.extend_from_slice(prefix);
        path.push(b'/');
    }
    path.extend_from_slice(name);
    Ok(normalize_archive_path(&path, kind == b'5')?.join("/"))
}

fn parse_tar_octal(
    field: &[u8],
    label: &'static str,
    empty_is_zero: bool,
) -> Result<u64, SkillError> {
    if field.first().is_some_and(|byte| byte & 0x80 != 0) {
        return invalid_source(&format!("the tar header {label} is not strict octal"));
    }
    let start = field
        .iter()
        .position(|byte| *byte != b' ')
        .unwrap_or(field.len());
    if start == field.len() || field[start] == 0 {
        if field[start..].iter().any(|byte| !matches!(byte, b' ' | 0)) {
            return invalid_source(&format!("the tar header {label} is not strict octal"));
        }
        return if empty_is_zero {
            Ok(0)
        } else {
            invalid_source(&format!("the tar header {label} is missing"))
        };
    }

    let mut value = 0_u64;
    let mut end = start;
    while end < field.len() && !matches!(field[end], b' ' | 0) {
        let byte = field[end];
        if !(b'0'..=b'7').contains(&byte) {
            return invalid_source(&format!("the tar header {label} is not strict octal"));
        }
        value = value
            .checked_mul(8)
            .and_then(|value| value.checked_add((byte - b'0') as u64))
            .ok_or_else(|| invalid_source_error(&format!("the tar header {label} is invalid")))?;
        end += 1;
    }
    if end == start || field[end..].iter().any(|byte| !matches!(byte, b' ' | 0)) {
        return invalid_source(&format!("the tar header {label} is not strict octal"));
    }
    Ok(value)
}

fn tar_string_bytes<'a>(field: &'a [u8], label: &'static str) -> Result<&'a [u8], SkillError> {
    if let Some(end) = field.iter().position(|byte| *byte == 0) {
        if field[end + 1..].iter().any(|byte| *byte != 0) {
            return invalid_source(&format!("the tar {label} contains embedded NUL data"));
        }
        return Ok(&field[..end]);
    }
    Ok(field)
}

fn validate_tar_string_padding(field: &[u8], label: &'static str) -> Result<(), SkillError> {
    tar_string_bytes(field, label).map(|_| ())
}

trait ArchiveDestination {
    fn diagnostic_path(&self, relative: &str) -> PathBuf;
    fn ensure_directory(&self, relative: &str) -> Result<(), SkillError>;
    fn create_file(&self, relative: &str, executable_bits: u32) -> Result<File, SkillError>;
    fn create_symlink(&self, relative: &str, target: &str) -> Result<(), SkillError>;
    fn anchored_root(&self) -> Result<AnchoredRoot, SkillError>;
}

#[cfg(test)]
struct PathArchiveDestination<'a> {
    root: &'a Path,
}

#[cfg(test)]
impl ArchiveDestination for PathArchiveDestination<'_> {
    fn diagnostic_path(&self, relative: &str) -> PathBuf {
        join_normalized(self.root, relative)
    }

    fn ensure_directory(&self, relative: &str) -> Result<(), SkillError> {
        ensure_archive_directory(&self.diagnostic_path(relative))
    }

    fn create_file(&self, relative: &str, executable_bits: u32) -> Result<File, SkillError> {
        create_private_file(&self.diagnostic_path(relative), executable_bits)
    }

    fn create_symlink(&self, relative: &str, target: &str) -> Result<(), SkillError> {
        create_archive_symlink(target, &self.diagnostic_path(relative))
    }

    fn anchored_root(&self) -> Result<AnchoredRoot, SkillError> {
        AnchoredRoot::open(self.root)
    }
}

impl ArchiveDestination for StagingDirectory {
    fn diagnostic_path(&self, relative: &str) -> PathBuf {
        join_normalized(self.path(), relative)
    }

    fn ensure_directory(&self, relative: &str) -> Result<(), SkillError> {
        StagingDirectory::ensure_directory(self, relative).map(|_| ())
    }

    fn create_file(&self, relative: &str, executable_bits: u32) -> Result<File, SkillError> {
        StagingDirectory::create_file(self, relative, executable_bits)
    }

    fn create_symlink(&self, relative: &str, target: &str) -> Result<(), SkillError> {
        StagingDirectory::create_symlink(self, relative, target)
    }

    fn anchored_root(&self) -> Result<AnchoredRoot, SkillError> {
        StagingDirectory::anchored_root(self)
    }
}

#[cfg(test)]
fn materialize_archive(
    raw_tar: &Path,
    destination: &Path,
    normalized_paths: &[String],
) -> Result<PathBuf, SkillError> {
    let file = File::open(raw_tar).map_err(|error| io_error(raw_tar, error))?;
    let destination = PathArchiveDestination { root: destination };
    let root = materialize_archive_file(file, &destination, normalized_paths, true)?;
    Ok(destination.diagnostic_path(&root))
}

fn materialize_staged_archive(
    raw_tar: File,
    destination: &StagingDirectory,
    normalized_paths: &[String],
) -> Result<StagingDirectory, SkillError> {
    let root = materialize_archive_file(raw_tar, destination, normalized_paths, true)?;
    destination.open_directory(&root)
}

fn materialize_archive_file(
    file: File,
    destination: &impl ArchiveDestination,
    normalized_paths: &[String],
    require_single_root: bool,
) -> Result<String, SkillError> {
    let mut archive = Archive::new(file);
    let entries = archive.entries().map_err(|_| archive_read_error())?;
    let mut seen = BTreeSet::new();
    let mut roots = BTreeSet::new();
    let mut pending_symlinks = Vec::new();
    let mut planned_nodes = BTreeMap::new();
    let mut entry_count = 0_u64;
    let mut content_bytes = 0_u64;

    for entry in entries {
        let mut entry = entry.map_err(|_| archive_read_error())?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        entry_count = entry_count.saturating_add(1);
        enforce_limit("entries", entry_count, MAX_ARCHIVE_ENTRIES)?;
        let planned_kind = if kind.is_file() {
            PlannedNodeKind::File
        } else if kind.is_dir() {
            PlannedNodeKind::Directory
        } else if kind.is_symlink() {
            PlannedNodeKind::Symlink
        } else if kind.is_hard_link() {
            return invalid_source("hard links are not allowed in Skill archives");
        } else {
            return invalid_source("special files are not allowed in Skill archives");
        };
        let components = normalize_archive_path(entry.path_bytes().as_ref(), kind.is_dir())?;
        let relative = components.join("/");
        if normalized_paths.get((entry_count - 1) as usize) != Some(&relative) {
            return invalid_source("the raw and high-level tar parsers disagreed on an entry path");
        }
        if !seen.insert(relative.clone()) {
            return invalid_source("the archive contains duplicate entry paths");
        }
        roots.insert(components[0].clone());
        if require_single_root && roots.len() > 1 {
            return invalid_source("the archive contains multiple repository roots");
        }
        record_planned_node(&mut planned_nodes, &components, planned_kind)?;
        let destination_path = destination.diagnostic_path(&relative);
        ensure_archive_destination_parents(destination, &components[..components.len() - 1])?;
        let declared = entry.size();
        if planned_kind == PlannedNodeKind::File {
            enforce_limit("single_file", declared, MAX_SINGLE_FILE_BYTES)?;
            content_bytes = content_bytes.saturating_add(declared);
            enforce_limit("archive", content_bytes, MAX_ARCHIVE_BYTES)?;
            let archive_mode = entry.header().mode().unwrap_or(0o644);
            let mut output = destination
                .create_file(&relative, archive_mode & 0o111)
                .map_err(|_| invalid_source_error("archive entries collide after normalization"))?;
            let actual = copy_archive_entry(&mut entry, &mut output, declared)?;
            if actual != declared {
                return invalid_source("an archive entry size did not match its header");
            }
            output
                .flush()
                .map_err(|error| io_error(&destination_path, error))?;
        } else if planned_kind == PlannedNodeKind::Directory {
            if declared != 0 {
                return invalid_source("archive directory entries must have zero size");
            }
            destination.ensure_directory(&relative)?;
        } else {
            if declared != 0 {
                return invalid_source("archive symlink entries must have zero size");
            }
            let target = entry
                .link_name_bytes()
                .ok_or_else(|| invalid_source_error("an archive symlink is missing its target"))?;
            if target.is_empty() || target.len() > MAX_SOURCE_BYTES || target.contains(&0) {
                return invalid_source("an archive symlink target is not valid");
            }
            let target = std::str::from_utf8(target.as_ref())
                .map_err(|_| invalid_source_error("archive symlink targets must be valid UTF-8"))?
                .to_owned();
            pending_symlinks.push(PendingSymlink {
                relative,
                target,
                destination: destination_path,
            });
        }
    }
    if entry_count as usize != normalized_paths.len() {
        return invalid_source("the raw and high-level tar parsers disagreed on the entry count");
    }
    let root = if require_single_root {
        roots
            .into_iter()
            .next()
            .ok_or_else(|| invalid_source_error("the archive is empty"))?
    } else {
        if roots.is_empty() {
            return invalid_source("the archive is empty");
        }
        String::new()
    };
    validate_planned_symlink_graph(&planned_nodes, &pending_symlinks)?;
    for link in &pending_symlinks {
        destination.create_symlink(&link.relative, &link.target)?;
    }
    validate_archive_symlinks_anchored(&destination.anchored_root()?, &pending_symlinks)?;
    Ok(root)
}

fn materialize_zip_file(file: File, destination: &StagingDirectory) -> Result<(), SkillError> {
    let mut archive = ZipArchive::new(file)
        .map_err(|_| invalid_source_error("the selected ZIP archive is malformed"))?;
    enforce_limit("entries", archive.len() as u64, MAX_ARCHIVE_ENTRIES)?;
    let mut seen = BTreeSet::new();
    let mut planned_nodes = BTreeMap::new();
    let mut content_bytes = 0_u64;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| invalid_source_error("the selected ZIP archive could not be read"))?;
        if entry.encrypted() {
            return invalid_source("encrypted ZIP entries are not supported");
        }
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return invalid_source("the ZIP archive uses an unsupported compression method");
        }
        if entry.is_symlink() {
            return invalid_source("ZIP symlinks are not allowed in Skill archives");
        }

        let directory = entry.is_dir();
        let mode = entry
            .unix_mode()
            .unwrap_or(if directory { 0o040755 } else { 0o100644 });
        let file_type = mode & 0o170000;
        let allowed_type = if directory { 0o040000 } else { 0o100000 };
        if file_type != 0 && file_type != allowed_type {
            return invalid_source("special files are not allowed in Skill archives");
        }

        let components = normalize_archive_path(entry.name_raw(), directory)?;
        let relative = components.join("/");
        if !seen.insert(relative.clone()) {
            return invalid_source("the archive contains duplicate entry paths");
        }
        let planned_kind = if directory {
            PlannedNodeKind::Directory
        } else {
            PlannedNodeKind::File
        };
        record_planned_node(&mut planned_nodes, &components, planned_kind)?;
        ensure_archive_destination_parents(destination, &components[..components.len() - 1])?;

        if directory {
            if entry.size() != 0 {
                return invalid_source("archive directory entries must have zero size");
            }
            destination.ensure_directory(&relative)?;
            continue;
        }

        let declared = entry.size();
        enforce_limit("single_file", declared, MAX_SINGLE_FILE_BYTES)?;
        content_bytes = content_bytes.saturating_add(declared);
        enforce_limit("archive", content_bytes, MAX_ARCHIVE_BYTES)?;
        let mut output = destination.create_file(&relative, mode & 0o111)?;
        let actual = copy_archive_entry(&mut entry, &mut output, declared)?;
        if actual != declared {
            return invalid_source("an archive entry size did not match its header");
        }
        output
            .flush()
            .map_err(|error| io_error(&destination.diagnostic_path(&relative), error))?;
    }

    if seen.is_empty() {
        return invalid_source("the archive is empty");
    }
    Ok(())
}

fn record_planned_node(
    nodes: &mut BTreeMap<String, PlannedNodeKind>,
    components: &[String],
    kind: PlannedNodeKind,
) -> Result<(), SkillError> {
    for index in 1..components.len() {
        let parent = components[..index].join("/");
        match nodes.get(&parent) {
            Some(PlannedNodeKind::Directory) => {}
            Some(_) => {
                return invalid_source(
                    "an archive entry traverses a planned file or symlink parent",
                )
            }
            None => {
                nodes.insert(parent, PlannedNodeKind::Directory);
            }
        }
    }

    let path = components.join("/");
    match nodes.get(&path) {
        Some(PlannedNodeKind::Directory) if kind == PlannedNodeKind::Directory => Ok(()),
        Some(_) => invalid_source("archive entries collide in the planned extraction graph"),
        None => {
            nodes.insert(path, kind);
            Ok(())
        }
    }
}

fn validate_planned_symlink_graph(
    nodes: &BTreeMap<String, PlannedNodeKind>,
    links: &[PendingSymlink],
) -> Result<(), SkillError> {
    let link_targets = links
        .iter()
        .map(|link| (link.relative.as_str(), link.target.as_str()))
        .collect::<BTreeMap<_, _>>();
    for link in links {
        let target = normalize_planned_link_target(&link.relative, &link.target)?;
        resolve_planned_link_target(nodes, &link_targets, target)?;
    }
    Ok(())
}

fn normalize_planned_link_target(link_path: &str, target: &str) -> Result<Vec<String>, SkillError> {
    if target.is_empty()
        || target.len() > MAX_SOURCE_BYTES
        || target.starts_with('/')
        || target.contains(['\0', '\\'])
    {
        return invalid_source("archive symlink targets must be safe relative UTF-8 paths");
    }

    let mut normalized = link_path.split('/').map(str::to_owned).collect::<Vec<_>>();
    normalized.pop();
    for component in target.split('/') {
        match component {
            "" => return invalid_source("archive symlink targets must use canonical separators"),
            "." => {}
            ".." => {
                if normalized.pop().is_none() {
                    return invalid_source("an archive symlink target escapes the extracted tree");
                }
            }
            component => normalized.push(component.to_owned()),
        }
    }
    if normalized.is_empty() {
        return invalid_source("an archive symlink target escapes the extracted tree");
    }
    Ok(normalized)
}

fn resolve_planned_link_target(
    nodes: &BTreeMap<String, PlannedNodeKind>,
    links: &BTreeMap<&str, &str>,
    mut target: Vec<String>,
) -> Result<(), SkillError> {
    let mut visited = BTreeSet::new();
    let mut hops = 0_usize;
    loop {
        let state = target.join("/");
        if !visited.insert(state) {
            return invalid_source("the archive symlink graph contains a cycle");
        }

        let mut followed = false;
        for index in 1..=target.len() {
            let prefix = target[..index].join("/");
            match nodes.get(&prefix) {
                Some(PlannedNodeKind::Directory) => {}
                Some(PlannedNodeKind::File) if index == target.len() => return Ok(()),
                Some(PlannedNodeKind::File) => {
                    return invalid_source("an archive symlink target traverses a regular file")
                }
                Some(PlannedNodeKind::Symlink) => {
                    hops = hops.saturating_add(1);
                    if hops > MAX_SYMLINK_HOPS {
                        return invalid_source("the archive symlink graph exceeds the hop limit");
                    }
                    let nested_target = links.get(prefix.as_str()).ok_or_else(|| {
                        invalid_source_error("the archive symlink graph is incomplete")
                    })?;
                    let mut resolved = normalize_planned_link_target(&prefix, nested_target)?;
                    resolved.extend(target[index..].iter().cloned());
                    target = resolved;
                    followed = true;
                    break;
                }
                None => return invalid_source("an archive symlink target is dangling"),
            }
        }
        if !followed {
            return Ok(());
        }
    }
}

fn normalize_archive_path(bytes: &[u8], directory: bool) -> Result<Vec<String>, SkillError> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES || bytes.contains(&0) {
        return invalid_source("an archive entry path is not valid");
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid_source_error("archive entry paths must be valid UTF-8"))?;
    if text.starts_with(['/', '\\']) {
        return invalid_source("absolute archive entry paths are not allowed");
    }
    let text = if directory {
        text.strip_suffix('/').unwrap_or(text)
    } else {
        text
    };
    let components = text
        .split('/')
        .map(|component| component.to_owned())
        .collect::<Vec<_>>();
    if components.is_empty()
        || components.iter().any(|component| {
            component.is_empty()
                || matches!(component.as_str(), "." | "..")
                || component.contains(['\\', '\0'])
        })
        || !Path::new(text)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return invalid_source("archive entry paths must use safe canonical components");
    }
    Ok(components)
}

fn ensure_archive_destination_parents(
    destination: &impl ArchiveDestination,
    components: &[String],
) -> Result<(), SkillError> {
    for index in 1..=components.len() {
        destination.ensure_directory(&components[..index].join("/"))?;
    }
    Ok(())
}

#[cfg(test)]
fn ensure_archive_directory(path: &Path) -> Result<(), SkillError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => return Ok(()),
        Ok(_) => return invalid_source("archive entries collide after normalization"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(path, error)),
    }
    create_private_directory(path)
}

fn copy_archive_entry(
    entry: &mut impl Read,
    output: &mut File,
    declared: u64,
) -> Result<u64, SkillError> {
    let mut actual = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let remaining = MAX_SINGLE_FILE_BYTES
            .saturating_add(1)
            .saturating_sub(actual);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(limit_error(
                "single_file",
                actual.saturating_add(1),
                MAX_SINGLE_FILE_BYTES,
            ));
        }
        let read = entry
            .read(&mut buffer[..requested])
            .map_err(|_| archive_read_error())?;
        if read == 0 {
            break;
        }
        actual = actual.saturating_add(read as u64);
        enforce_limit("single_file", actual, MAX_SINGLE_FILE_BYTES)?;
        output
            .write_all(&buffer[..read])
            .map_err(|_| invalid_source_error("an archive entry could not be staged"))?;
    }
    if actual != declared {
        return invalid_source("an archive entry size did not match its header");
    }
    Ok(actual)
}

fn archive_read_error() -> SkillError {
    invalid_source_error("the preflight-approved archive could not be materialized")
}

#[cfg(all(test, unix))]
fn create_archive_symlink(target: &str, destination: &Path) -> Result<(), SkillError> {
    std::os::unix::fs::symlink(target, destination)
        .map_err(|_| invalid_source_error("archive symlink entries collide after normalization"))
}

#[cfg(all(test, windows))]
fn create_archive_symlink(target: &str, destination: &Path) -> Result<(), SkillError> {
    let resolved = destination
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(target);
    let result = if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    };
    result.map_err(|_| invalid_source_error("archive symlink entries collide after normalization"))
}

#[cfg(all(test, not(unix), not(windows)))]
fn create_archive_symlink(_target: &str, _destination: &Path) -> Result<(), SkillError> {
    invalid_source("secure archive symlink extraction is unavailable on this platform")
}

#[cfg(unix)]
fn validate_archive_symlinks_anchored(
    anchored: &AnchoredRoot,
    links: &[PendingSymlink],
) -> Result<(), SkillError> {
    for link in links {
        anchored
            .validate_symlink_target(&link.relative, &link.target, &link.destination)
            .map_err(|error| match error {
                error @ SkillError::LimitExceeded { .. } => error,
                _ => invalid_source_error(
                    "an archive symlink does not resolve safely inside the extracted tree",
                ),
            })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_archive_symlinks_anchored(
    _root: &AnchoredRoot,
    links: &[PendingSymlink],
) -> Result<(), SkillError> {
    if links.is_empty() {
        Ok(())
    } else {
        invalid_source("secure archive symlink validation is unavailable on this platform")
    }
}

fn stage_candidates_anchored(
    source_root: &AnchoredRoot,
    operation: &StagingOperation,
) -> Result<Vec<SkillCandidateSummary>, SkillError> {
    let discovered = discover_candidates_anchored(source_root)?;
    if discovered.is_empty() {
        return invalid_source("the selected source contains no valid Skill candidates");
    }
    reject_nested_candidates(&discovered)?;
    let mut prepared = Vec::with_capacity(discovered.len());
    let mut names = BTreeMap::new();
    let mut aggregate = 0_u64;
    let mut first_invalid_manifest = None;
    for relative in discovered {
        let candidate = open_anchored_directory(source_root, &relative)?;
        let validated = match validate_candidate_anchored_private(&candidate) {
            Ok(validated) => validated,
            Err(error @ SkillError::InvalidManifest { .. }) => {
                first_invalid_manifest.get_or_insert(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(previous) = names.insert(validated.manifest.name.clone(), relative.clone()) {
            return Err(SkillError::Conflict {
                message: format!(
                    "duplicate Skill name '{}' was found at '{}' and '{}'",
                    validated.manifest.name, previous, relative
                ),
                path: String::new(),
            });
        }
        aggregate = aggregate.saturating_add(validated.total_bytes);
        enforce_limit("archive", aggregate, MAX_ARCHIVE_BYTES)?;
        prepared.push((relative, candidate, validated));
    }
    if prepared.is_empty() {
        return Err(first_invalid_manifest.unwrap_or_else(|| {
            invalid_source_error("the selected source contains no valid Skill candidates")
        }));
    }
    prepared.sort_by(|left, right| {
        left.2
            .manifest
            .name
            .cmp(&right.2.manifest.name)
            .then_with(|| left.0.cmp(&right.0))
    });

    let candidates_root = operation.create_private_directory("candidates")?;
    let mut summaries = Vec::with_capacity(prepared.len());
    for (relative, source, before) in prepared {
        let destination = candidates_root.create_directory(&before.manifest.name)?;
        copy_tree_anchored_private_to_staging(&source, &destination)?;
        let staged = validate_staging_candidate(&destination)?;
        if staged.manifest.name != before.manifest.name
            || staged.content_hash != before.content_hash
            || staged.total_bytes != before.total_bytes
        {
            return Err(SkillError::Conflict {
                message: "a Skill candidate changed while its snapshot was staged".into(),
                path: String::new(),
            });
        }
        summaries.push(SkillCandidateSummary {
            name: staged.manifest.name,
            description: staged.manifest.description,
            relative_path: relative,
            content_kind: staged.content_kind,
            content_hash: staged.content_hash,
            file_count: staged.files.len() as u64,
            total_bytes: staged.total_bytes,
        });
    }
    Ok(summaries)
}

fn discover_candidates_anchored(anchored: &AnchoredRoot) -> Result<Vec<String>, SkillError> {
    let root_directory = anchored.root_directory()?;
    if directory_has_manifest(anchored, &root_directory, anchored.canonical_path())? {
        return Ok(vec![String::new()]);
    }
    let mut count = 0_u64;
    let mut pending = vec![(root_directory, String::new())];
    let mut candidates = Vec::new();
    while let Some((directory, relative)) = pending.pop() {
        let directory_path = join_normalized(anchored.canonical_path(), &relative);
        let names = anchored.read_directory_budgeted(
            &directory,
            &directory_path,
            count,
            MAX_ARCHIVE_ENTRIES,
            "entries",
        )?;
        count = count.saturating_add(names.len() as u64);
        let mut has_manifest = false;
        let mut children = Vec::new();
        for name in names {
            let name_text = std::str::from_utf8(name.to_bytes()).map_err(|_| {
                invalid_source_error("Skill source entry paths must be valid UTF-8")
            })?;
            let child_relative = if relative.is_empty() {
                name_text.to_owned()
            } else {
                format!("{relative}/{name_text}")
            };
            let child_path = join_normalized(anchored.canonical_path(), &child_relative);
            let identity = anchored.stat_entry(&directory, &name, &child_path)?;
            if name_text == "SKILL.md" {
                has_manifest = true;
            }
            if identity.kind == AnchoredFileKind::Directory {
                let child =
                    anchored.open_directory_entry(&directory, &name, &identity, &child_path)?;
                children.push((child, child_relative));
            }
        }
        if has_manifest {
            candidates.push(relative.clone());
        }
        children.reverse();
        pending.extend(children);
    }
    candidates.sort();
    Ok(candidates)
}

fn open_anchored_directory(
    root: &AnchoredRoot,
    relative: &str,
) -> Result<AnchoredRoot, SkillError> {
    if relative.is_empty() {
        return root.try_clone();
    }
    let mut directory = root.root_directory()?;
    let mut path = root.canonical_path().to_path_buf();
    let mut final_identity: Option<AnchoredIdentity> = None;
    for component in relative.split('/') {
        let name = CString::new(component)
            .map_err(|_| invalid_source_error("a Skill candidate path is invalid"))?;
        path.push(component);
        let identity = root.stat_entry(&directory, &name, &path)?;
        directory = root.open_directory_entry(&directory, &name, &identity, &path)?;
        final_identity = Some(identity);
    }
    AnchoredRoot::from_open_directory(
        directory,
        path,
        &final_identity.expect("non-empty candidate paths have an identity"),
    )
}

fn directory_has_manifest(
    root: &AnchoredRoot,
    directory: &File,
    path: &Path,
) -> Result<bool, SkillError> {
    let names = root.read_directory_budgeted(directory, path, 0, MAX_ARCHIVE_ENTRIES, "entries")?;
    Ok(names.iter().any(|name| name.to_bytes() == b"SKILL.md"))
}

fn reject_nested_candidates(candidates: &[String]) -> Result<(), SkillError> {
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.is_empty() {
            continue;
        }
        let prefix = format!("{candidate}/");
        if candidates
            .iter()
            .skip(index + 1)
            .any(|other| other.starts_with(&prefix))
        {
            return Err(SkillError::Conflict {
                message: "nested Skill candidates would create overlapping snapshots".into(),
                path: String::new(),
            });
        }
    }
    Ok(())
}

fn join_normalized(root: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .filter(|component| !component.is_empty())
        .fold(root.to_path_buf(), |path, component| path.join(component))
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
    path.to_string_lossy().replace('\\', "/")
}

fn lexical_absolute(path: &Path) -> Result<PathBuf, SkillError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| io_error(path, error))?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str())
            }
        }
    }
    Ok(normalized)
}

#[cfg(all(test, unix))]
fn private_directory_builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
}

#[cfg(all(test, unix))]
fn private_file_options(executable_bits: u32) -> OpenOptions {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600 | (executable_bits & 0o111));
    options
}

#[cfg(all(test, unix))]
fn create_private_directory(path: &Path) -> Result<(), SkillError> {
    private_directory_builder()
        .create(path)
        .map_err(|error| io_error(path, error))
}

#[cfg(all(test, not(unix)))]
fn create_private_directory(_path: &Path) -> Result<(), SkillError> {
    invalid_source("private Skill source staging is unavailable on this platform")
}

#[cfg(all(test, unix))]
fn create_private_file(path: &Path, executable_bits: u32) -> Result<File, SkillError> {
    private_file_options(executable_bits)
        .open(path)
        .map_err(|error| io_error(path, error))
}

#[cfg(all(test, not(unix)))]
fn create_private_file(_path: &Path, _executable_bits: u32) -> Result<File, SkillError> {
    invalid_source("private Skill source staging is unavailable on this platform")
}

fn enforce_limit(limit: &'static str, actual: u64, allowed: u64) -> Result<(), SkillError> {
    if actual > allowed {
        return Err(limit_error(limit, actual, allowed));
    }
    Ok(())
}

fn limit_error(limit: &'static str, actual: u64, allowed: u64) -> SkillError {
    SkillError::LimitExceeded {
        limit: limit.into(),
        actual,
        allowed,
    }
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn invalid_source<T>(message: &str) -> Result<T, SkillError> {
    Err(invalid_source_error(message))
}

fn invalid_source_error(message: &str) -> SkillError {
    SkillError::InvalidSource {
        message: capped_message(message),
    }
}

fn sanitize_resolution_error(error: SkillError) -> SkillError {
    match error {
        SkillError::InvalidManifest { message, .. } => SkillError::InvalidManifest {
            message: capped_message(message),
            path: String::new(),
        },
        SkillError::UnsafePath { message, .. } => SkillError::UnsafePath {
            message: capped_message(message),
            path: String::new(),
        },
        SkillError::Conflict { message, .. } => SkillError::Conflict {
            message: capped_message(message),
            path: String::new(),
        },
        SkillError::Io { message, .. } => SkillError::Io {
            message: capped_message(message),
            path: None,
        },
        SkillError::InvalidSource { message } => SkillError::InvalidSource {
            message: capped_message(message),
        },
        SkillError::Network { message, retry_at } => SkillError::Network {
            message: capped_message(message),
            retry_at: retry_at.map(capped_message),
        },
        SkillError::PlanStale { message } => SkillError::PlanStale {
            message: capped_message(message),
        },
        SkillError::ConfirmationRequired {
            message,
            findings_hash,
        } => SkillError::ConfirmationRequired {
            message: capped_message(message),
            findings_hash,
        },
        SkillError::RecoveryRequired { message } => SkillError::RecoveryRequired {
            message: capped_message(message),
        },
        error @ SkillError::LimitExceeded { .. } => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Cursor;
    use std::net::TcpListener;
    use std::thread;
    use tar::{Builder, EntryType, Header};
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn retry_after_is_a_machine_readable_time_and_rejects_overflow_or_private_text() {
        let now = Utc.with_ymd_and_hms(2026, 10, 5, 8, 0, 0).unwrap();
        assert_eq!(github_retry_after_at("120", now).as_deref(), Some("2026-10-05T08:02:00Z"));
        assert_eq!(github_retry_after_at("Wed, 21 Oct 2015 07:28:00 GMT", now).as_deref(), Some("2015-10-21T07:28:00Z"));
        for invalid in ["-1", "private-retry-value", "99999999999999999999999", "253402300800"] {
            assert!(github_retry_after_at(invalid, now).is_none());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_later_invalid_link_is_rejected_before_any_symlink_is_created() {
        let th = TestHome::new("skill-source-link-preflight");
        let download = th.home.join("source.tar.gz");
        let destination = th.home.join("archive");
        create_private_directory(&destination).unwrap();

        let encoder = GzEncoder::new(File::create(&download).unwrap(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_test_file(&mut archive, "repo/guide.md", b"guide");
        append_test_link(&mut archive, "repo/first", "guide.md");
        append_test_link(&mut archive, "repo/later-invalid", "missing.md");
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();

        assert!(matches!(
            extract_archive(&download, &destination),
            Err(SkillError::InvalidSource { .. }) | Err(SkillError::UnsafePath { .. })
        ));
        let symlinks = walk_symlinks(&destination);
        assert!(
            symlinks.is_empty(),
            "link validation materialized symlinks before the graph was approved: {symlinks:?}"
        );
    }

    #[test]
    fn github_global_pax_comment_is_accepted_without_weakening_entry_paths() {
        let th = TestHome::new("skill-source-github-global-pax");
        let download = th.home.join("source.tar.gz");
        let destination = th.home.join("archive");
        create_private_directory(&destination).unwrap();

        let encoder = GzEncoder::new(File::create(&download).unwrap(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_github_global_pax_comment(&mut archive, "fa0fa64bdc967915dc8399e803be67759e1e62b8");
        append_test_file(&mut archive, "skills-main/SKILL.md", b"skill");
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();

        let root = extract_archive(&download, &destination).unwrap();
        assert_eq!(root, destination.join("skills-main"));
        assert_eq!(fs::read(root.join("SKILL.md")).unwrap(), b"skill");
    }

    #[test]
    fn global_pax_metadata_other_than_a_commit_comment_is_rejected() {
        let th = TestHome::new("skill-source-unsafe-global-pax");
        let download = th.home.join("source.tar.gz");
        let destination = th.home.join("archive");
        create_private_directory(&destination).unwrap();

        let encoder = GzEncoder::new(File::create(&download).unwrap(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_github_global_pax_comment(&mut archive, "not-a-commit");
        append_test_file(&mut archive, "skills-main/SKILL.md", b"skill");
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();

        assert!(matches!(
            extract_archive(&download, &destination),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    #[test]
    fn a_repository_keeps_valid_skills_when_another_manifest_is_invalid() {
        let th = TestHome::new("skill-source-skip-invalid-manifest");
        let source = th.home.join("source");
        fs::create_dir_all(source.join("valid-skill")).unwrap();
        fs::create_dir_all(source.join("too-verbose")).unwrap();
        fs::write(
            source.join("valid-skill/SKILL.md"),
            "---\nname: valid-skill\ndescription: Valid fixture\n---\n",
        )
        .unwrap();
        fs::write(
            source.join("too-verbose/SKILL.md"),
            format!(
                "---\nname: too-verbose\ndescription: {}\n---\n",
                "x".repeat(1025)
            ),
        )
        .unwrap();

        let resolution = resolve_source(
            SkillSourceInput::Local {
                path: source.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        assert_eq!(resolution.candidates.len(), 1);
        assert_eq!(resolution.candidates[0].name, "valid-skill");
        crate::resources::skill::cancel_operation(&resolution.operation_id).unwrap();
    }

    #[test]
    fn zip_archive_resolves_and_replays_a_recorded_candidate() {
        let th = TestHome::new("skill-source-zip-archive");
        let archive_path = th.home.join("skills.zip");
        let mut archive = ZipWriter::new(File::create(&archive_path).unwrap());
        archive
            .start_file(
                "collection/review-changes/SKILL.md",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        archive
            .write_all(b"---\nname: review-changes\ndescription: Review a change set\n---\n")
            .unwrap();
        archive.finish().unwrap();

        let resolution = resolve_source(
            SkillSourceInput::Archive {
                path: archive_path.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        assert_eq!(resolution.candidates.len(), 1);
        assert_eq!(resolution.candidates[0].name, "review-changes");
        assert_eq!(
            resolution.candidates[0].relative_path,
            "collection/review-changes"
        );
        crate::resources::skill::cancel_operation(&resolution.operation_id).unwrap();

        let recorded = SkillSource::Archive {
            path: collapse_home(&archive_path, &th.home),
            subpath: "collection/review-changes".into(),
        };
        let replayed = stage_recorded_skill(
            &recorded,
            None,
            "review-changes",
            GithubEndpoints::production(),
        )
        .unwrap();
        assert_eq!(replayed.candidates[0].name, "review-changes");
        crate::resources::skill::cancel_operation(&replayed.operation_id).unwrap();
    }

    #[test]
    fn tar_archives_resolve_skill_directories() {
        let th = TestHome::new("skill-source-tar-archive");
        let archive_path = th.home.join("single-skill.tar");
        let mut archive = Builder::new(File::create(&archive_path).unwrap());
        append_test_file(
            &mut archive,
            "single-skill/SKILL.md",
            b"---\nname: single-skill\ndescription: Single archive Skill\n---\n",
        );
        archive.finish().unwrap();

        let resolution = resolve_source(
            SkillSourceInput::Archive {
                path: archive_path.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        assert_eq!(resolution.candidates[0].name, "single-skill");
        assert_eq!(resolution.candidates[0].relative_path, "single-skill");
        crate::resources::skill::cancel_operation(&resolution.operation_id).unwrap();
    }

    #[test]
    fn compressed_tar_archives_resolve_skill_directories() {
        let th = TestHome::new("skill-source-tar-gz-archive");
        let archive_path = th.home.join("compressed-skills.tgz");
        let encoder = GzEncoder::new(File::create(&archive_path).unwrap(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_test_file(
            &mut archive,
            "compressed-skill/SKILL.md",
            b"---\nname: compressed-skill\ndescription: Compressed archive Skill\n---\n",
        );
        let encoder = archive.into_inner().unwrap();
        encoder.finish().unwrap();

        let resolution = resolve_source(
            SkillSourceInput::Archive {
                path: archive_path.to_string_lossy().into_owned(),
            },
            GithubEndpoints::production(),
        )
        .unwrap();
        assert_eq!(resolution.candidates[0].name, "compressed-skill");
        crate::resources::skill::cancel_operation(&resolution.operation_id).unwrap();

        assert_eq!(
            archive_format(Path::new("skill.tar.gz")).unwrap(),
            LocalArchiveFormat::TarGz
        );
    }

    #[test]
    fn zip_archive_path_traversal_is_rejected() {
        let th = TestHome::new("skill-source-zip-traversal");
        let archive_path = th.home.join("unsafe.zip");
        let mut archive = ZipWriter::new(File::create(&archive_path).unwrap());
        archive
            .start_file("../escape/SKILL.md", SimpleFileOptions::default())
            .unwrap();
        archive
            .write_all(b"---\nname: escape\ndescription: Unsafe\n---\n")
            .unwrap();
        archive.finish().unwrap();

        assert!(matches!(
            resolve_source(
                SkillSourceInput::Archive {
                    path: archive_path.to_string_lossy().into_owned(),
                },
                GithubEndpoints::production(),
            ),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    #[test]
    fn production_requests_and_redirects_require_effective_https_port_443() {
        let production = GithubEndpoints::production();
        assert!(validate_request_url(
            &Url::parse("https://api.github.com:443/repos/acme/skills").unwrap(),
            &production,
        )
        .is_ok());
        assert!(validate_request_url(
            &Url::parse("https://api.github.com:444/repos/acme/skills").unwrap(),
            &production,
        )
        .is_err());
        assert!(validate_request_url(
            &Url::parse("https://codeload.github.com:8443/acme/skills/tar.gz/sha").unwrap(),
            &production,
        )
        .is_err());

        let loopback = Url::parse("http://127.0.0.1:43123/").unwrap();
        let tests = GithubEndpoints::for_test(loopback.clone(), loopback.clone());
        assert!(validate_request_url(&loopback, &tests).is_ok());
    }

    #[test]
    fn retry_headers_are_trimmed_and_reject_unbounded_or_control_values() {
        assert_eq!(
            bounded_header_value(" 1893456000 "),
            Some("1893456000".into())
        );
        assert_eq!(bounded_header_value(&"9".repeat(513)), None);
        assert_eq!(bounded_header_value("12\t34"), None);
    }

    #[cfg(unix)]
    #[test]
    fn private_builders_apply_modes_at_the_create_syscall() {
        use std::os::unix::fs::PermissionsExt;

        let th = TestHome::new("skill-source-private-builders");
        let directory = th.home.join("directory");
        private_directory_builder().create(&directory).unwrap();
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let regular = th.home.join("regular");
        private_file_options(0).open(&regular).unwrap();
        assert_eq!(
            fs::metadata(&regular).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let executable = th.home.join("executable");
        private_file_options(0o111).open(&executable).unwrap();
        assert_eq!(
            fs::metadata(&executable).unwrap().permissions().mode() & 0o777,
            0o711
        );
    }

    #[test]
    fn transient_removal_failure_triggers_owned_full_root_cleanup_without_path_leakage() {
        let th = TestHome::new("skill-source-transient-removal");
        let operation = th.home.join("private-operation");
        let owner = OperationDirectory::create(operation.clone()).unwrap();
        let transient = operation.join("source.tar");
        fs::write(&transient, b"transient").unwrap();

        let removal = remove_transient_with(&transient, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "transient removal denied",
            ))
        });
        assert!(
            owner.armed,
            "transient cleanup disarmed the operation owner"
        );
        let error = owner
            .finish(removal)
            .map_err(sanitize_resolution_error)
            .unwrap_err();

        assert!(
            !operation.exists(),
            "the owned operation root was not removed"
        );
        assert!(matches!(error, SkillError::Io { path: None, .. }));
        assert!(!format!("{error:?}").contains(th.home.to_string_lossy().as_ref()));
    }

    #[test]
    fn whole_root_cleanup_failure_takes_precedence_without_leaking_paths() {
        let th = TestHome::new("skill-source-cleanup-precedence");
        let operation = th.home.join("private-operation");
        let transient = operation.join("source.tar");
        let original = remove_transient_with(&transient, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "transient removal denied",
            ))
        })
        .unwrap_err();

        assert_eq!(
            finish_operation_with_cleanup(Err::<(), _>(original.clone()), &operation, |_| Ok(()))
                .unwrap_err(),
            original
        );
        let error = finish_operation_with_cleanup(Err::<(), _>(original), &operation, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cleanup denied",
            ))
        })
        .unwrap_err();
        let SkillError::RecoveryRequired { message } = error else {
            panic!("expected recovery-required cleanup error");
        };
        assert!(!message.contains(operation.to_string_lossy().as_ref()));
        assert!(message.len() <= 512);
    }

    #[test]
    fn operation_owner_is_armed_immediately_after_creation() {
        let th = TestHome::new("skill-source-operation-owner");
        let operation = th.home.join("operation");
        let owner = OperationDirectory::create(operation.clone()).unwrap();
        fs::write(operation.join("partial"), b"partial").unwrap();
        drop(owner);
        assert!(!operation.exists());
    }

    #[test]
    fn recorded_github_skill_stages_only_named_subpath_at_immutable_sha() {
        let th = TestHome::new("recorded-github-skill");
        let sha = "2222222222222222222222222222222222222222";
        let body = b"---\nname: review\ndescription: Review fixture\n---\n";
        let encoder = GzEncoder::new(Vec::new(), Compression::default());
        let mut archive = Builder::new(encoder);
        append_test_file(
            &mut archive,
            &format!("skills-{sha}/catalog/review/SKILL.md"),
            body,
        );
        append_test_file(
            &mut archive,
            &format!("skills-{sha}/catalog/unrelated/SKILL.md"),
            b"---\nname: unrelated\ndescription: Unrelated fixture\n---\n",
        );
        let encoder = archive.into_inner().unwrap();
        let archive = encoder.finish().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            let first_line = String::from_utf8_lossy(&request)
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                archive.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&archive).unwrap();
            first_line
        });
        let base = Url::parse(&format!("http://{address}/")).unwrap();
        let resolution = stage_recorded_skill(
            &SkillSource::Github {
                owner: "acme".into(),
                repo: "skills".into(),
                subpath: "catalog/review".into(),
                requested_ref: "main".into(),
                pinned: false,
            },
            Some(sha),
            "review",
            GithubEndpoints::for_test(base.clone(), base),
        )
        .unwrap();

        assert_eq!(
            server.join().unwrap(),
            format!("GET /acme/skills/tar.gz/{sha} HTTP/1.1")
        );
        assert_eq!(resolution.resolved_revision.as_deref(), Some(sha));
        assert_eq!(resolution.candidates.len(), 1);
        assert_eq!(resolution.candidates[0].name, "review");
        let candidates = th.home.join(format!(
            ".arca/staging/skills/{}/candidates",
            resolution.operation_id
        ));
        assert!(candidates.join("review/SKILL.md").exists());
        assert!(!candidates.join("unrelated").exists());
    }

    #[test]
    fn recorded_github_skill_rejects_path_components_before_network_access() {
        let _th = TestHome::new("recorded-github-path-components");
        let base = Url::parse("http://127.0.0.1:9/").unwrap();
        let error = stage_recorded_skill(
            &SkillSource::Github {
                owner: "..".into(),
                repo: "skills".into(),
                subpath: "catalog/review".into(),
                requested_ref: "main".into(),
                pinned: false,
            },
            Some("2222222222222222222222222222222222222222"),
            "review",
            GithubEndpoints::for_test(base.clone(), base),
        )
        .unwrap_err();

        assert!(matches!(error, SkillError::InvalidSource { .. }));
    }

    fn append_test_file<W: Write>(archive: &mut Builder<W>, path: &str, body: &[u8]) {
        let mut header = Header::new_ustar();
        header.set_entry_type(EntryType::file());
        header.set_mode(0o644);
        header.set_size(body.len() as u64);
        header.set_cksum();
        archive
            .append_data(&mut header, path, Cursor::new(body))
            .unwrap();
    }

    fn append_github_global_pax_comment<W: Write>(archive: &mut Builder<W>, comment: &str) {
        let body_without_length = format!(" comment={comment}\n");
        let mut length = body_without_length.len() + 2;
        loop {
            let record = format!("{length}{body_without_length}");
            if record.len() == length {
                let mut header = Header::new_ustar();
                header.set_entry_type(EntryType::new(b'g'));
                header.set_mode(0o666);
                header.set_size(record.len() as u64);
                header.set_cksum();
                archive
                    .append_data(
                        &mut header,
                        "pax_global_header",
                        Cursor::new(record.into_bytes()),
                    )
                    .unwrap();
                return;
            }
            length = record.len();
        }
    }

    fn append_test_link<W: Write>(archive: &mut Builder<W>, path: &str, target: &str) {
        let mut header = Header::new_ustar();
        header.set_entry_type(EntryType::symlink());
        header.set_mode(0o777);
        header.set_size(0);
        header.set_link_name(target).unwrap();
        header.set_cksum();
        archive
            .append_data(&mut header, path, Cursor::new(Vec::<u8>::new()))
            .unwrap();
    }

    #[cfg(unix)]
    fn walk_symlinks(root: &Path) -> Vec<PathBuf> {
        let mut pending = vec![root.to_path_buf()];
        let mut links = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                let metadata = fs::symlink_metadata(&path).unwrap();
                if metadata.file_type().is_symlink() {
                    links.push(path);
                } else if metadata.is_dir() {
                    pending.push(path);
                }
            }
        }
        links
    }
}
