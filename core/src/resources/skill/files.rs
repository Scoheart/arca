#[cfg(unix)]
use super::anchored::validate_supported_links;
#[cfg(test)]
use super::anchored::verify_anchored_identity;
use super::anchored::{
    consume_bounded_and_hash, verify_anchored_digest, AnchoredFileKind, AnchoredIdentity,
    AnchoredRoot,
};
use super::staging::StagingDirectory;
use super::{
    io_error, normalized_error_path, parse_manifest, FileChangeKind, SkillContentKind, SkillError,
    SkillFile, SkillFileChange, SkillFileKind, ValidatedSkill,
};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

pub const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_SKILL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_SINGLE_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: u64 = 10_000;
pub const MAX_DIFF_INPUT_BYTES: u64 = 1024 * 1024;
pub const MAX_DIFF_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_PLAN_DIFF_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Directory,
    File,
    Symlink,
}

#[derive(Debug, Clone)]
struct TreeNode {
    path: String,
    full_path: PathBuf,
    kind: NodeKind,
    size: u64,
    executable: bool,
    link_target: Option<String>,
    sha256: Option<String>,
    mode: u32,
    identity: AnchoredIdentity,
}

impl TreeNode {
    fn as_skill_file(&self) -> Option<SkillFile> {
        let kind = match self.kind {
            NodeKind::Directory => return None,
            NodeKind::File => SkillFileKind::File,
            NodeKind::Symlink => SkillFileKind::Symlink,
        };
        Some(SkillFile {
            path: self.path.clone(),
            kind,
            size: self.size,
            executable: self.executable,
            link_target: self.link_target.clone(),
            sha256: self.sha256.clone().unwrap_or_default(),
        })
    }
}

struct TreeSnapshot {
    root: AnchoredRoot,
    nodes: Vec<TreeNode>,
    total_bytes: u64,
}

impl TreeSnapshot {
    fn files(&self) -> Vec<SkillFile> {
        self.nodes
            .iter()
            .filter_map(TreeNode::as_skill_file)
            .collect()
    }

    fn file_nodes(&self) -> impl Iterator<Item = &TreeNode> {
        self.nodes
            .iter()
            .filter(|node| node.kind != NodeKind::Directory)
    }
}

struct AggregateLimit<'a> {
    current: &'a mut u64,
    allowed: u64,
    limit: &'static str,
}

struct WalkState<'a> {
    nodes: Vec<TreeNode>,
    entries: u64,
    total_bytes: u64,
    aggregate: Option<AggregateLimit<'a>>,
    shared_entries: Option<AggregateLimit<'a>>,
}

pub fn inspect_tree(root: &Path) -> Result<Vec<SkillFile>, SkillError> {
    Ok(load_tree(root)?.files())
}

pub(super) fn inspect_tree_anchored(root: &AnchoredRoot) -> Result<Vec<SkillFile>, SkillError> {
    Ok(load_tree_anchored(root, None, None)?.files())
}

pub fn hash_tree(root: &Path) -> Result<String, SkillError> {
    let snapshot = load_tree(root)?;
    hash_snapshot(&snapshot)
}

pub(super) fn hash_tree_anchored(root: &AnchoredRoot) -> Result<String, SkillError> {
    let snapshot = load_tree_anchored(root, None, None)?;
    hash_snapshot(&snapshot)
}

pub fn copy_tree_secure(source: &Path, destination: &Path) -> Result<(), SkillError> {
    copy_tree_secure_with_permissions(source, destination, CopyPermissions::Preserve)
}

pub(super) fn copy_tree_secure_private_to_staging(
    source: &Path,
    destination: &StagingDirectory,
) -> Result<(), SkillError> {
    let snapshot = load_tree(source)?;
    copy_snapshot_to_staging(snapshot, destination)
}

pub(super) fn copy_tree_anchored_private_to_staging(
    source: &AnchoredRoot,
    destination: &StagingDirectory,
) -> Result<(), SkillError> {
    let snapshot = load_tree_anchored(source, None, None)?;
    copy_snapshot_to_staging(snapshot, destination)
}

pub(super) fn validate_staging_candidate(
    directory: &StagingDirectory,
) -> Result<ValidatedSkill, SkillError> {
    let root = directory.anchored_root()?;
    validate_candidate_anchored_private(&root)
}

pub(super) fn validate_candidate_anchored_private(
    root: &AnchoredRoot,
) -> Result<ValidatedSkill, SkillError> {
    let mut aggregate = 0;
    let mut entries = 0;
    validate_candidate_anchored(
        root,
        &mut aggregate,
        MAX_SKILL_BYTES,
        "skill_content",
        &mut entries,
        MAX_ARCHIVE_ENTRIES,
        "entries",
    )
}

fn copy_snapshot_to_staging(
    snapshot: TreeSnapshot,
    destination: &StagingDirectory,
) -> Result<(), SkillError> {
    if !destination.is_empty()? {
        return Err(SkillError::Conflict {
            message: "copy destination already exists".into(),
            path: normalized_error_path(destination.path()),
        });
    }
    if destination
        .path()
        .starts_with(snapshot.root.canonical_path())
    {
        return Err(SkillError::UnsafePath {
            message: "copy destination must not be inside the source tree".into(),
            path: normalized_error_path(destination.path()),
        });
    }
    for node in &snapshot.nodes {
        match node.kind {
            NodeKind::Directory => {
                destination.ensure_directory(&node.path)?;
            }
            NodeKind::File => {
                let source = snapshot.root.open_regular_relative(
                    &node.path,
                    &node.identity,
                    &node.full_path,
                )?;
                let mut target = destination.create_file(&node.path, node.mode & 0o111)?;
                let consumed = consume_bounded_and_hash(
                    source,
                    &mut target,
                    node.size,
                    MAX_SINGLE_FILE_BYTES,
                    &node.full_path,
                    "single_file",
                )?;
                target
                    .flush()
                    .map_err(|error| io_error(destination.path(), error))?;
                verify_anchored_digest(
                    node.sha256.as_deref().unwrap_or_default(),
                    &consumed.sha256,
                    &node.full_path,
                )?;
            }
            NodeKind::Symlink => {
                let target = node
                    .link_target
                    .as_deref()
                    .ok_or_else(|| SkillError::UnsafePath {
                        message: "validated symlink is missing its relative target".into(),
                        path: normalized_error_path(&node.full_path),
                    })?;
                destination.create_symlink(&node.path, target)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyPermissions {
    Preserve,
}

fn copy_tree_secure_with_permissions(
    source: &Path,
    destination: &Path,
    permissions: CopyPermissions,
) -> Result<(), SkillError> {
    let snapshot = load_tree(source)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => {
            return Err(SkillError::Conflict {
                message: "copy destination already exists".into(),
                path: normalized_error_path(destination),
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(destination, error)),
    }

    let source_root = snapshot.root.canonical_path();
    let destination_absolute = resolve_destination(destination)?;
    if destination_absolute.starts_with(source_root) {
        return Err(SkillError::UnsafePath {
            message: "copy destination must not be inside the source tree".into(),
            path: normalized_error_path(destination),
        });
    }

    if let Some(parent) = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    create_copy_directory(destination)?;

    let copy_result = (|| {
        for node in &snapshot.nodes {
            let target = destination.join(path_from_normalized(&node.path));
            match node.kind {
                NodeKind::Directory => {
                    create_copy_directory(&target)?;
                }
                NodeKind::File => copy_regular_file(&snapshot, node, &target, permissions)?,
                NodeKind::Symlink => create_relative_symlink(node, &target)?,
            }
        }
        Ok(())
    })();
    finish_copy_with_cleanup(copy_result, destination, |path| fs::remove_dir_all(path))
}

fn finish_copy_with_cleanup<F>(
    copy_result: Result<(), SkillError>,
    destination: &Path,
    cleanup: F,
) -> Result<(), SkillError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let Err(original) = copy_result else {
        return Ok(());
    };
    cleanup(destination).map_err(|_| SkillError::RecoveryRequired {
        message: "partial skill copy could not be removed; manual recovery is required".into(),
    })?;
    Err(original)
}

pub fn diff_trees(before: Option<&Path>, after: &Path) -> Result<Vec<SkillFileChange>, SkillError> {
    let before_snapshot = before.map(load_tree).transpose()?;
    let after_snapshot = load_tree(after)?;
    let before_nodes = before_snapshot
        .as_ref()
        .map(file_node_map)
        .unwrap_or_default();
    let after_nodes = file_node_map(&after_snapshot);
    let paths: BTreeSet<&str> = before_nodes
        .keys()
        .chain(after_nodes.keys())
        .copied()
        .collect();
    let mut remaining_diff_bytes = MAX_PLAN_DIFF_BYTES;
    let mut changes = Vec::new();

    for path in paths {
        let before_node = before_nodes.get(path).copied();
        let after_node = after_nodes.get(path).copied();
        match (before_node, after_node) {
            (None, Some(new)) => changes.push(change_without_diff(
                path,
                FileChangeKind::Added,
                None,
                Some(new),
            )),
            (Some(old), None) => changes.push(change_without_diff(
                path,
                FileChangeKind::Removed,
                Some(old),
                None,
            )),
            (Some(old), Some(new))
                if old.kind != new.kind
                    || (old.kind == NodeKind::Symlink && old.sha256 != new.sha256) =>
            {
                changes.push(change_without_diff(
                    path,
                    FileChangeKind::LinkChanged,
                    Some(old),
                    Some(new),
                ));
            }
            (Some(old), Some(new)) => {
                if old.sha256 != new.sha256 {
                    let (unified_diff, diff_truncated) = build_text_diff(
                        path,
                        before_snapshot
                            .as_ref()
                            .expect("modified paths have a before snapshot"),
                        old,
                        &after_snapshot,
                        new,
                        &mut remaining_diff_bytes,
                    )?;
                    changes.push(SkillFileChange {
                        path: path.to_string(),
                        kind: FileChangeKind::Modified,
                        before_hash: old.sha256.clone(),
                        after_hash: new.sha256.clone(),
                        unified_diff,
                        diff_truncated,
                    });
                }
                if old.executable != new.executable {
                    changes.push(change_without_diff(
                        path,
                        FileChangeKind::ModeChanged,
                        Some(old),
                        Some(new),
                    ));
                }
            }
            (None, None) => {}
        }
    }
    Ok(changes)
}

fn change_without_diff(
    path: &str,
    kind: FileChangeKind,
    before: Option<&TreeNode>,
    after: Option<&TreeNode>,
) -> SkillFileChange {
    SkillFileChange {
        path: path.to_string(),
        kind,
        before_hash: before.and_then(|node| node.sha256.clone()),
        after_hash: after.and_then(|node| node.sha256.clone()),
        unified_diff: None,
        diff_truncated: false,
    }
}

pub fn validate_candidate(root: &Path) -> Result<ValidatedSkill, SkillError> {
    let snapshot = load_tree(root)?;
    validated_from_snapshot(snapshot)
}

pub(super) fn validate_candidate_anchored(
    root: &AnchoredRoot,
    aggregate: &mut u64,
    aggregate_allowed: u64,
    aggregate_limit: &'static str,
    shared_entries: &mut u64,
    shared_entries_allowed: u64,
    shared_entries_limit: &'static str,
) -> Result<ValidatedSkill, SkillError> {
    let snapshot = load_tree_anchored(
        root,
        Some(AggregateLimit {
            current: aggregate,
            allowed: aggregate_allowed,
            limit: aggregate_limit,
        }),
        Some(AggregateLimit {
            current: shared_entries,
            allowed: shared_entries_allowed,
            limit: shared_entries_limit,
        }),
    )?;
    validated_from_snapshot(snapshot)
}

fn validated_from_snapshot(snapshot: TreeSnapshot) -> Result<ValidatedSkill, SkillError> {
    let root = snapshot.root.canonical_path();
    let manifest_node = snapshot
        .file_nodes()
        .find(|node| node.path == "SKILL.md" && node.kind == NodeKind::File)
        .ok_or_else(|| SkillError::InvalidManifest {
            message: "candidate must contain a regular SKILL.md file".into(),
            path: normalized_error_path(root),
        })?;
    let manifest_bytes = read_node_bounded(
        &snapshot,
        manifest_node,
        MAX_SINGLE_FILE_BYTES,
        "single_file",
    )?;
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|_| SkillError::InvalidManifest {
            message: "SKILL.md must be valid UTF-8".into(),
            path: normalized_error_path(root),
        })?;
    let manifest = parse_manifest(root, manifest_text)?;
    let files = snapshot.files();
    let content_kind = classify_content(&files);
    let content_hash = hash_snapshot(&snapshot)?;
    Ok(ValidatedSkill {
        manifest,
        content_kind,
        files,
        content_hash,
        total_bytes: snapshot.total_bytes,
    })
}

fn load_tree(root: &Path) -> Result<TreeSnapshot, SkillError> {
    let anchored_root = AnchoredRoot::open(root)?;
    load_tree_owned(anchored_root, None, None)
}

fn load_tree_anchored(
    root: &AnchoredRoot,
    aggregate: Option<AggregateLimit<'_>>,
    shared_entries: Option<AggregateLimit<'_>>,
) -> Result<TreeSnapshot, SkillError> {
    load_tree_owned(root.try_clone()?, aggregate, shared_entries)
}

fn load_tree_owned(
    anchored_root: AnchoredRoot,
    aggregate: Option<AggregateLimit<'_>>,
    shared_entries: Option<AggregateLimit<'_>>,
) -> Result<TreeSnapshot, SkillError> {
    #[cfg(not(unix))]
    {
        let _ = anchored_root;
        return Err(super::anchored::unsupported_platform());
    }
    #[cfg(unix)]
    let root_directory = anchored_root.root_directory()?;
    let mut state = WalkState {
        nodes: Vec::new(),
        entries: 0,
        total_bytes: 0,
        aggregate,
        shared_entries,
    };
    #[cfg(unix)]
    walk_directory(
        anchored_root.canonical_path(),
        &anchored_root,
        &root_directory,
        "",
        &mut state,
    )?;
    state
        .nodes
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(TreeSnapshot {
        root: anchored_root,
        nodes: state.nodes,
        total_bytes: state.total_bytes,
    })
}

#[cfg(unix)]
fn walk_directory(
    root: &Path,
    anchored_root: &AnchoredRoot,
    directory: &File,
    relative_directory: &str,
    state: &mut WalkState<'_>,
) -> Result<(), SkillError> {
    let directory_path = if relative_directory.is_empty() {
        root.to_path_buf()
    } else {
        root.join(path_from_normalized(relative_directory))
    };
    for name in anchored_root.read_directory(directory, &directory_path)? {
        let name_text =
            std::str::from_utf8(name.as_bytes()).map_err(|_| SkillError::UnsafePath {
                message: "Skill entry paths must be valid UTF-8".into(),
                path: normalized_error_path(&directory_path),
            })?;
        let relative = if relative_directory.is_empty() {
            name_text.to_string()
        } else {
            format!("{relative_directory}/{name_text}")
        };
        let path = root.join(path_from_normalized(&relative));
        charge_tree_entry(state)?;
        let identity = anchored_root.stat_entry(directory, &name, &path)?;
        if identity.kind == AnchoredFileKind::Directory {
            let child = anchored_root.open_directory_entry(directory, &name, &identity, &path)?;
            state.nodes.push(TreeNode {
                path: relative.clone(),
                full_path: path.clone(),
                kind: NodeKind::Directory,
                size: 0,
                executable: false,
                link_target: None,
                sha256: None,
                mode: identity.mode,
                identity,
            });
            walk_directory(root, anchored_root, &child, &relative, state)?;
        } else if identity.kind == AnchoredFileKind::Regular {
            validate_supported_links(&identity, &path)?;
            enforce_single_file_limit(identity.size)?;
            charge_tree_content(state, identity.size)?;
            let file = anchored_root.open_regular_entry(directory, &name, &identity, &path)?;
            let consumed = consume_bounded_and_hash(
                file,
                &mut std::io::sink(),
                identity.size,
                MAX_SINGLE_FILE_BYTES,
                &path,
                "single_file",
            )?;
            state.nodes.push(TreeNode {
                path: relative,
                full_path: path,
                kind: NodeKind::File,
                size: consumed.size,
                executable: identity.mode & 0o111 != 0,
                link_target: None,
                sha256: Some(consumed.sha256),
                mode: identity.mode,
                identity,
            });
        } else if identity.kind == AnchoredFileKind::Symlink {
            validate_supported_links(&identity, &path)?;
            let target = anchored_root.read_link_entry(directory, &name, &identity, &path)?;
            let target_text = std::str::from_utf8(&target).map_err(|_| SkillError::UnsafePath {
                message: "Skill symlink targets must be valid UTF-8".into(),
                path: normalized_error_path(&path),
            })?;
            anchored_root.validate_symlink_target(&relative, target_text, &path)?;
            let target_bytes = target_text.as_bytes();
            charge_tree_content(state, target_bytes.len() as u64)?;
            state.nodes.push(TreeNode {
                path: relative,
                full_path: path,
                kind: NodeKind::Symlink,
                size: target_bytes.len() as u64,
                executable: false,
                link_target: Some(target_text.to_string()),
                sha256: Some(hex::encode(Sha256::digest(target_bytes))),
                mode: 0,
                identity,
            });
        } else {
            return Err(SkillError::UnsafePath {
                message: "special files are not allowed in Skill trees".into(),
                path: normalized_error_path(&path),
            });
        }
    }
    Ok(())
}

fn charge_tree_entry(state: &mut WalkState<'_>) -> Result<(), SkillError> {
    if let Some(shared) = state.shared_entries.as_mut() {
        let actual = shared.current.saturating_add(1);
        enforce_named_limit(shared.limit, actual, shared.allowed)?;
        *shared.current = actual;
    }
    state.entries = state.entries.saturating_add(1);
    enforce_entry_limit(state.entries)
}

fn charge_tree_content(state: &mut WalkState<'_>, added: u64) -> Result<(), SkillError> {
    enforce_total_limit(state.total_bytes, added)?;
    if let Some(aggregate) = state.aggregate.as_mut() {
        let actual = aggregate.current.saturating_add(added);
        enforce_named_limit(aggregate.limit, actual, aggregate.allowed)?;
        *aggregate.current = actual;
    }
    state.total_bytes = state.total_bytes.saturating_add(added);
    Ok(())
}

fn hash_snapshot(snapshot: &TreeSnapshot) -> Result<String, SkillError> {
    let mut tree_hash = Sha256::new();
    for node in snapshot.file_nodes() {
        tree_hash.update(match node.kind {
            NodeKind::File => b"file".as_slice(),
            NodeKind::Symlink => b"symlink".as_slice(),
            NodeKind::Directory => unreachable!(),
        });
        tree_hash.update([u8::from(node.executable)]);
        tree_hash.update((node.path.len() as u64).to_be_bytes());
        tree_hash.update(node.path.as_bytes());
        tree_hash.update(node.size.to_be_bytes());
        match node.kind {
            NodeKind::File => {
                let file = snapshot.root.open_regular_relative(
                    &node.path,
                    &node.identity,
                    &node.full_path,
                )?;
                let consumed = consume_bounded_and_hash(
                    file,
                    &mut HashWriter(&mut tree_hash),
                    node.size,
                    MAX_SINGLE_FILE_BYTES,
                    &node.full_path,
                    "single_file",
                )?;
                verify_anchored_digest(
                    node.sha256.as_deref().unwrap_or_default(),
                    &consumed.sha256,
                    &node.full_path,
                )?;
            }
            NodeKind::Symlink => {
                tree_hash.update(node.link_target.as_deref().unwrap_or_default().as_bytes())
            }
            NodeKind::Directory => unreachable!(),
        }
    }
    Ok(hex::encode(tree_hash.finalize()))
}

struct HashWriter<'a>(&'a mut Sha256);

impl Write for HashWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn enforce_total_limit(current: u64, added: u64) -> Result<(), SkillError> {
    let actual = current.saturating_add(added);
    enforce_skill_limit(actual)
}

fn enforce_named_limit(limit: &'static str, actual: u64, allowed: u64) -> Result<(), SkillError> {
    if actual > allowed {
        return Err(SkillError::LimitExceeded {
            limit: limit.into(),
            actual,
            allowed,
        });
    }
    Ok(())
}

fn enforce_entry_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("entries", actual, MAX_ARCHIVE_ENTRIES)
}

fn enforce_skill_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("skill", actual, MAX_SKILL_BYTES)
}

fn enforce_single_file_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("single_file", actual, MAX_SINGLE_FILE_BYTES)
}

fn enforce_diff_input_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("diff_input", actual, MAX_DIFF_INPUT_BYTES)
}

fn enforce_diff_output_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("diff_output", actual, MAX_DIFF_OUTPUT_BYTES as u64)
}

fn enforce_plan_diff_limit(actual: u64) -> Result<(), SkillError> {
    enforce_named_limit("plan_diff", actual, MAX_PLAN_DIFF_BYTES as u64)
}

fn copy_regular_file(
    snapshot: &TreeSnapshot,
    node: &TreeNode,
    destination: &Path,
    permissions: CopyPermissions,
) -> Result<(), SkillError> {
    let source =
        snapshot
            .root
            .open_regular_relative(&node.path, &node.identity, &node.full_path)?;
    let mut target = create_copy_file(destination, node.mode & 0o111)?;
    let consumed = consume_bounded_and_hash(
        source,
        &mut target,
        node.size,
        MAX_SINGLE_FILE_BYTES,
        &node.full_path,
        "single_file",
    )?;
    target
        .flush()
        .map_err(|error| io_error(destination, error))?;
    verify_anchored_digest(
        node.sha256.as_deref().unwrap_or_default(),
        &consumed.sha256,
        &node.full_path,
    )?;
    #[cfg(unix)]
    if permissions == CopyPermissions::Preserve {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(destination, fs::Permissions::from_mode(node.mode & 0o777))
            .map_err(|error| io_error(destination, error))?;
    }
    Ok(())
}

#[cfg(unix)]
fn create_relative_symlink(node: &TreeNode, destination: &Path) -> Result<(), SkillError> {
    let target = node
        .link_target
        .as_deref()
        .ok_or_else(|| SkillError::UnsafePath {
            message: "validated symlink is missing its relative target".into(),
            path: normalized_error_path(&node.full_path),
        })?;
    std::os::unix::fs::symlink(target, destination).map_err(|error| io_error(destination, error))
}

#[cfg(windows)]
fn create_relative_symlink(node: &TreeNode, destination: &Path) -> Result<(), SkillError> {
    let target = node
        .link_target
        .as_deref()
        .ok_or_else(|| SkillError::UnsafePath {
            message: "validated symlink is missing its relative target".into(),
            path: normalized_error_path(&node.full_path),
        })?;
    let resolved = node
        .full_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(target);
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
    .map_err(|error| io_error(destination, error))
}

#[cfg(all(not(unix), not(windows)))]
fn create_relative_symlink(_node: &TreeNode, _destination: &Path) -> Result<(), SkillError> {
    Err(super::anchored::unsupported_platform())
}

#[cfg(unix)]
fn create_copy_directory(path: &Path) -> Result<(), SkillError> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder.create(path).map_err(|error| io_error(path, error))
}

#[cfg(not(unix))]
fn create_copy_directory(path: &Path) -> Result<(), SkillError> {
    fs::create_dir(path).map_err(|error| io_error(path, error))
}

#[cfg(unix)]
fn create_copy_file(path: &Path, executable_bits: u32) -> Result<File, SkillError> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600 | (executable_bits & 0o111));
    options.open(path).map_err(|error| io_error(path, error))
}

#[cfg(not(unix))]
fn create_copy_file(path: &Path, _executable_bits: u32) -> Result<File, SkillError> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| io_error(path, error))
}

fn file_node_map(snapshot: &TreeSnapshot) -> BTreeMap<&str, &TreeNode> {
    snapshot
        .file_nodes()
        .map(|node| (node.path.as_str(), node))
        .collect()
}

fn build_text_diff(
    path: &str,
    before_snapshot: &TreeSnapshot,
    before: &TreeNode,
    after_snapshot: &TreeSnapshot,
    after: &TreeNode,
    remaining_plan_bytes: &mut usize,
) -> Result<(Option<String>, bool), SkillError> {
    if before.kind != NodeKind::File || after.kind != NodeKind::File {
        return Ok((None, false));
    }
    if enforce_diff_input_limit(before.size).is_err()
        || enforce_diff_input_limit(after.size).is_err()
    {
        return Ok((None, true));
    }
    let before_bytes =
        read_node_bounded(before_snapshot, before, MAX_DIFF_INPUT_BYTES, "diff_input")?;
    let after_bytes = read_node_bounded(after_snapshot, after, MAX_DIFF_INPUT_BYTES, "diff_input")?;
    let Ok(before_text) = std::str::from_utf8(&before_bytes) else {
        return Ok((None, false));
    };
    let Ok(after_text) = std::str::from_utf8(&after_bytes) else {
        return Ok((None, false));
    };
    let before_header = format!("a/{path}");
    let after_header = format!("b/{path}");
    let mut output = TextDiff::from_lines(before_text, after_text)
        .unified_diff()
        .header(&before_header, &after_header)
        .to_string();
    let mut truncated = if enforce_diff_output_limit(output.len() as u64).is_err() {
        truncate_utf8(&mut output, MAX_DIFF_OUTPUT_BYTES)
    } else {
        false
    };
    let used = MAX_PLAN_DIFF_BYTES.saturating_sub(*remaining_plan_bytes);
    if enforce_plan_diff_limit(used.saturating_add(output.len()) as u64).is_err() {
        truncated |= truncate_utf8(&mut output, *remaining_plan_bytes);
    }
    *remaining_plan_bytes = remaining_plan_bytes.saturating_sub(output.len());
    Ok((Some(output), truncated))
}

fn truncate_utf8(value: &mut String, maximum: usize) -> bool {
    if value.len() <= maximum {
        return false;
    }
    let mut boundary = maximum;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    true
}

fn read_node_bounded(
    snapshot: &TreeSnapshot,
    node: &TreeNode,
    maximum: u64,
    limit: &'static str,
) -> Result<Vec<u8>, SkillError> {
    let file = snapshot
        .root
        .open_regular_relative(&node.path, &node.identity, &node.full_path)?;
    let mut bytes = Vec::new();
    let consumed =
        consume_bounded_and_hash(file, &mut bytes, node.size, maximum, &node.full_path, limit)?;
    verify_anchored_digest(
        node.sha256.as_deref().unwrap_or_default(),
        &consumed.sha256,
        &node.full_path,
    )?;
    Ok(bytes)
}

pub(super) fn classify_content(files: &[SkillFile]) -> SkillContentKind {
    if files
        .iter()
        .any(|file| file.executable || file.path.starts_with("scripts/"))
    {
        SkillContentKind::Automation
    } else if files.iter().any(|file| file.path.starts_with("assets/")) {
        SkillContentKind::Assets
    } else if files
        .iter()
        .any(|file| file.path.starts_with("references/"))
    {
        SkillContentKind::Reference
    } else {
        SkillContentKind::Instructions
    }
}

fn path_from_normalized(path: &str) -> PathBuf {
    path.split('/').collect()
}

fn lexical_absolute(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn resolve_destination(path: &Path) -> Result<PathBuf, SkillError> {
    let absolute = lexical_absolute(path);
    let mut missing = Vec::new();
    let mut cursor = absolute.as_path();
    loop {
        match fs::canonicalize(cursor) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor.file_name().ok_or_else(|| io_error(cursor, error))?;
                missing.push(name.to_os_string());
                cursor = cursor.parent().ok_or_else(|| SkillError::UnsafePath {
                    message: "copy destination has no resolvable parent".into(),
                    path: normalized_error_path(path),
                })?;
            }
            Err(error) => return Err(io_error(cursor, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::skill::SkillsPaths;
    use crate::testenv::TestHome;
    use std::io::Cursor;

    #[cfg(unix)]
    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/skills")
            .join(name)
    }

    #[cfg(unix)]
    fn copy_safe_fixture(destination: &Path) {
        fs::create_dir_all(destination.join("references")).unwrap();
        fs::copy(
            fixture("safe").join("SKILL.md"),
            destination.join("SKILL.md"),
        )
        .unwrap();
        fs::copy(
            fixture("safe").join("references/guide.md"),
            destination.join("references/guide.md"),
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_escape_symlinks_and_limit_overflow() {
        let th = TestHome::new("skill-file-safety");
        let root = th.home.join("escape");
        fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink("../../outside", root.join("escape")).unwrap();
        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::UnsafePath { .. })
        ));

        fs::remove_file(root.join("escape")).unwrap();
        fs::write(
            root.join("large.bin"),
            vec![0_u8; MAX_SINGLE_FILE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::LimitExceeded {
                limit: ref name,
                ..
            }) if name == "single_file"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn normalized_tree_hash_is_stable_and_content_sensitive() {
        let first = fixture("safe");
        let th = TestHome::new("skill-tree-hash");
        let copy = th.home.join("safe");
        copy_safe_fixture(&copy);
        assert_eq!(hash_tree(&first).unwrap(), hash_tree(&copy).unwrap());
        fs::write(copy.join("references/guide.md"), "changed").unwrap();
        assert_ne!(hash_tree(&first).unwrap(), hash_tree(&copy).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn arca_owned_skill_roots_are_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let _th = TestHome::new("skill-root-permissions");
        let paths = SkillsPaths::from_env().unwrap();
        for path in [
            paths.skills_dir(),
            paths.staging_skills_dir(),
            paths.backups_skills_dir(),
            paths.journals_skills_dir(),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn secure_copy_preserves_executables_and_safe_relative_symlinks() {
        use std::os::unix::fs::PermissionsExt;

        let th = TestHome::new("skill-secure-copy");
        let source = th.home.join("source");
        let destination = th.home.join("destination");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::write(source.join("SKILL.md"), "instructions").unwrap();
        fs::write(source.join("scripts/run.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(
            source.join("scripts/run.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink("scripts/run.sh", source.join("run")).unwrap();

        copy_tree_secure(&source, &destination).unwrap();

        assert_eq!(
            hash_tree(&source).unwrap(),
            hash_tree(&destination).unwrap()
        );
        assert_eq!(
            fs::read_link(destination.join("run")).unwrap(),
            Path::new("scripts/run.sh")
        );
        assert_ne!(
            fs::metadata(destination.join("scripts/run.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111,
            0
        );
        let files = inspect_tree(&destination).unwrap();
        assert!(files.iter().any(|entry| {
            entry.path == "run"
                && entry.kind == SkillFileKind::Symlink
                && entry.link_target.as_deref() == Some("scripts/run.sh")
        }));
    }

    #[test]
    fn copy_cleanup_failures_require_recovery_without_leaking_paths() {
        let th = TestHome::new("skill-copy-cleanup-failure");
        let destination = th.home.join("private-destination");
        let original = SkillError::Io {
            message: "copy failed".into(),
            path: None,
        };

        assert_eq!(
            finish_copy_with_cleanup(Err(original.clone()), &destination, |_| Ok(())).unwrap_err(),
            original
        );

        let error = finish_copy_with_cleanup(Err(original), &destination, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cleanup denied",
            ))
        })
        .unwrap_err();
        let SkillError::RecoveryRequired { message } = error else {
            panic!("expected recovery-required cleanup error");
        };
        assert_eq!(
            message,
            "partial skill copy could not be removed; manual recovery is required"
        );
        assert!(!message.contains(destination.to_string_lossy().as_ref()));
    }

    #[cfg(unix)]
    #[test]
    fn secure_copy_rejects_a_destination_inside_source_through_a_symlinked_parent() {
        let th = TestHome::new("skill-copy-alias-destination");
        let real_parent = th.home.join("real");
        let source = real_parent.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "instructions").unwrap();
        std::os::unix::fs::symlink(&real_parent, th.home.join("alias")).unwrap();
        let aliased_source = th.home.join("alias/source");
        let destination = aliased_source.join("nested-copy");

        assert!(matches!(
            copy_tree_secure(&aliased_source, &destination),
            Err(SkillError::UnsafePath { .. })
        ));
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_hard_links_special_files_and_absolute_symlinks() {
        use std::os::unix::net::UnixListener;

        let th = TestHome::new("skill-rejected-files");
        let root = th.home.join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("original"), "same inode").unwrap();
        fs::hard_link(root.join("original"), root.join("linked")).unwrap();
        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::UnsafePath { .. })
        ));

        fs::remove_file(root.join("linked")).unwrap();
        fs::remove_file(root.join("original")).unwrap();
        let listener = UnixListener::bind(root.join("socket")).unwrap();
        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::UnsafePath { .. })
        ));
        drop(listener);
        fs::remove_file(root.join("socket")).unwrap();

        fs::write(root.join("target"), "target").unwrap();
        std::os::unix::fs::symlink(root.join("target"), root.join("absolute")).unwrap();
        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::UnsafePath { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_hard_linked_symlinks() {
        use std::os::unix::fs::MetadataExt;

        let th = TestHome::new("skill-hard-linked-symlink");
        let root = th.home.join("root");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("target"), "target").unwrap();
        std::os::unix::fs::symlink("target", root.join("link")).unwrap();
        fs::hard_link(root.join("link"), root.join("link-alias")).unwrap();
        assert_eq!(fs::symlink_metadata(root.join("link")).unwrap().nlink(), 2);

        assert!(matches!(
            inspect_tree(&root),
            Err(SkillError::UnsafePath { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn diff_classifies_content_mode_link_and_presence_changes() {
        use std::os::unix::fs::PermissionsExt;

        let th = TestHome::new("skill-tree-diff");
        let before = th.home.join("before");
        let after = th.home.join("after");
        fs::create_dir_all(&before).unwrap();
        fs::create_dir_all(&after).unwrap();
        fs::write(before.join("modified.txt"), "before\n").unwrap();
        fs::write(after.join("modified.txt"), "after\n").unwrap();
        fs::write(before.join("mode.txt"), "same\n").unwrap();
        fs::write(after.join("mode.txt"), "same\n").unwrap();
        fs::set_permissions(after.join("mode.txt"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(before.join("first-target"), "one").unwrap();
        fs::write(after.join("first-target"), "one").unwrap();
        fs::write(before.join("second-target"), "two").unwrap();
        fs::write(after.join("second-target"), "two").unwrap();
        std::os::unix::fs::symlink("first-target", before.join("link")).unwrap();
        std::os::unix::fs::symlink("second-target", after.join("link")).unwrap();
        fs::write(before.join("removed.txt"), "removed\n").unwrap();
        fs::write(after.join("added.txt"), "added\n").unwrap();

        let changes = diff_trees(Some(&before), &after).unwrap();
        let kind = |path: &str| {
            changes
                .iter()
                .find(|change| change.path == path)
                .map(|change| change.kind.clone())
                .unwrap()
        };
        assert_eq!(kind("added.txt"), FileChangeKind::Added);
        assert_eq!(kind("link"), FileChangeKind::LinkChanged);
        assert_eq!(kind("mode.txt"), FileChangeKind::ModeChanged);
        assert_eq!(kind("modified.txt"), FileChangeKind::Modified);
        assert_eq!(kind("removed.txt"), FileChangeKind::Removed);
        let modified = changes
            .iter()
            .find(|change| change.path == "modified.txt")
            .unwrap();
        assert!(modified.before_hash.is_some());
        assert!(modified.after_hash.is_some());
        assert!(modified
            .unified_diff
            .as_deref()
            .unwrap()
            .contains("-before"));
        assert!(modified.unified_diff.as_deref().unwrap().contains("+after"));
        assert!(!modified.diff_truncated);
    }

    #[cfg(unix)]
    #[test]
    fn diff_reports_content_and_mode_changes_as_two_deterministic_rows() {
        use std::os::unix::fs::PermissionsExt;

        let th = TestHome::new("skill-tree-content-and-mode-diff");
        let before = th.home.join("before");
        let after = th.home.join("after");
        fs::create_dir_all(&before).unwrap();
        fs::create_dir_all(&after).unwrap();
        fs::write(before.join("both.txt"), "before\n").unwrap();
        fs::write(after.join("both.txt"), "after\n").unwrap();
        fs::set_permissions(after.join("both.txt"), fs::Permissions::from_mode(0o755)).unwrap();

        let changes = diff_trees(Some(&before), &after).unwrap();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "both.txt");
        assert_eq!(changes[0].kind, FileChangeKind::Modified);
        assert!(changes[0].unified_diff.is_some());
        assert_eq!(changes[1].path, "both.txt");
        assert_eq!(changes[1].kind, FileChangeKind::ModeChanged);
        assert!(changes[1].unified_diff.is_none());
        assert_eq!(changes[0].before_hash, changes[1].before_hash);
        assert_eq!(changes[0].after_hash, changes[1].after_hash);
    }

    #[cfg(unix)]
    #[test]
    fn text_diffs_are_utf8_safely_bounded() {
        let th = TestHome::new("skill-diff-bound");
        let before = th.home.join("before");
        let after = th.home.join("after");
        fs::create_dir_all(&before).unwrap();
        fs::create_dir_all(&after).unwrap();
        fs::write(
            before.join("large.txt"),
            format!("{}\n", "旧".repeat(120_000)),
        )
        .unwrap();
        fs::write(
            after.join("large.txt"),
            format!("{}\n", "新".repeat(120_000)),
        )
        .unwrap();

        let changes = diff_trees(Some(&before), &after).unwrap();
        let change = &changes[0];
        let diff = change.unified_diff.as_ref().unwrap();
        assert!(diff.len() <= MAX_DIFF_OUTPUT_BYTES);
        assert!(change.diff_truncated);
        assert!(std::str::from_utf8(diff.as_bytes()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn validates_content_kind_priority() {
        let th = TestHome::new("skill-content-kind");
        let root = th.home.join("content-kind");
        fs::create_dir_all(root.join("assets")).unwrap();
        fs::create_dir_all(root.join("references")).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: content-kind\ndescription: Content kind\n---\nbody\n",
        )
        .unwrap();
        fs::write(root.join("assets/icon.bin"), [0_u8, 159, 146, 150]).unwrap();
        fs::write(root.join("references/guide.md"), "guide").unwrap();
        assert_eq!(
            validate_candidate(&root).unwrap().content_kind,
            SkillContentKind::Assets
        );

        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("scripts/run.txt"), "run").unwrap();
        assert_eq!(
            validate_candidate(&root).unwrap().content_kind,
            SkillContentKind::Automation
        );
    }

    #[test]
    fn rejects_a_relative_arca_root() {
        let _th = TestHome::new("skill-relative-arca-root");
        std::env::set_var("ARCA_HOME", "relative-arca-root");
        assert!(matches!(
            SkillsPaths::from_env(),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn anchored_reopen_rejects_type_identity_link_and_size_mismatches() {
        let path = Path::new("/private/arca-test/skill/file.txt");
        let expected = AnchoredIdentity {
            kind: AnchoredFileKind::Regular,
            device: 7,
            inode: 11,
            links: 1,
            size: 4,
            mode: 0o100644,
        };
        let mismatches = [
            AnchoredIdentity {
                kind: AnchoredFileKind::Symlink,
                ..expected
            },
            AnchoredIdentity {
                device: 8,
                ..expected
            },
            AnchoredIdentity {
                inode: 12,
                ..expected
            },
            AnchoredIdentity {
                links: 2,
                ..expected
            },
            AnchoredIdentity {
                size: 5,
                ..expected
            },
        ];

        for actual in mismatches {
            assert!(verify_anchored_identity(&expected, &actual, path).is_err());
        }
        assert!(verify_anchored_identity(&expected, &expected, path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn anchored_reader_bounds_bytes_and_rejects_digest_changes() {
        let path = Path::new("/private/arca-test/skill/file.txt");
        let mut output = Vec::new();
        let error = consume_bounded_and_hash(
            Cursor::new(b"12345"),
            &mut output,
            4,
            4,
            path,
            "single_file",
        )
        .unwrap_err();
        assert_eq!(
            error,
            SkillError::LimitExceeded {
                limit: "single_file".into(),
                actual: 5,
                allowed: 4,
            }
        );

        let mut output = Vec::new();
        let consumed =
            consume_bounded_and_hash(Cursor::new(b"safe"), &mut output, 4, 4, path, "single_file")
                .unwrap();
        assert_eq!(output, b"safe");
        assert!(verify_anchored_digest("wrong", &consumed.sha256, path).is_err());
        assert!(verify_anchored_digest(&consumed.sha256, &consumed.sha256, path).is_ok());
    }

    #[test]
    fn exact_limits_accept_boundary_and_report_plus_one_values() {
        let cases = [
            (
                enforce_entry_limit as fn(u64) -> Result<(), SkillError>,
                "entries",
                MAX_ARCHIVE_ENTRIES,
            ),
            (enforce_skill_limit, "skill", MAX_SKILL_BYTES),
            (
                enforce_single_file_limit,
                "single_file",
                MAX_SINGLE_FILE_BYTES,
            ),
            (enforce_diff_input_limit, "diff_input", MAX_DIFF_INPUT_BYTES),
            (
                enforce_diff_output_limit,
                "diff_output",
                MAX_DIFF_OUTPUT_BYTES as u64,
            ),
            (
                enforce_plan_diff_limit,
                "plan_diff",
                MAX_PLAN_DIFF_BYTES as u64,
            ),
        ];

        for (enforce, name, allowed) in cases {
            assert!(enforce(allowed).is_ok(), "{name} rejected its boundary");
            assert_eq!(
                enforce(allowed + 1),
                Err(SkillError::LimitExceeded {
                    limit: name.into(),
                    actual: allowed + 1,
                    allowed,
                })
            );
        }

        let allowed = 512 * 1024 * 1024;
        let mut aggregate = allowed;
        let mut state = WalkState {
            nodes: Vec::new(),
            entries: 0,
            total_bytes: 0,
            aggregate: Some(AggregateLimit {
                current: &mut aggregate,
                allowed,
                limit: "inventory_managed_content",
            }),
            shared_entries: None,
        };
        assert!(charge_tree_content(&mut state, 0).is_ok());
        assert_eq!(
            charge_tree_content(&mut state, 1),
            Err(SkillError::LimitExceeded {
                limit: "inventory_managed_content".into(),
                actual: allowed + 1,
                allowed,
            })
        );

        let entry_allowed = 10_000;
        let mut shared_entries = entry_allowed - 1;
        let mut state = WalkState {
            nodes: Vec::new(),
            entries: 0,
            total_bytes: 0,
            aggregate: None,
            shared_entries: Some(AggregateLimit {
                current: &mut shared_entries,
                allowed: entry_allowed,
                limit: "inventory_entries",
            }),
        };
        assert!(charge_tree_entry(&mut state).is_ok());
        drop(state);
        assert_eq!(shared_entries, entry_allowed);
        let mut state = WalkState {
            nodes: Vec::new(),
            entries: 1,
            total_bytes: 0,
            aggregate: None,
            shared_entries: Some(AggregateLimit {
                current: &mut shared_entries,
                allowed: entry_allowed,
                limit: "inventory_entries",
            }),
        };
        assert_eq!(
            charge_tree_entry(&mut state),
            Err(SkillError::LimitExceeded {
                limit: "inventory_entries".into(),
                actual: entry_allowed + 1,
                allowed: entry_allowed,
            })
        );
    }
}
