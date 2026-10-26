use crate::domain::types::{ModelProfile, ModelProtocol};
use crate::paths::backup_timestamp;
use crate::resources::model::adapters::PreparedModelFile;
use crate::resources::model::{
    backup_reviewed_config_bytes, ExternalModelObservedState, ModelApplyResult, ModelObservedState,
    ModelTargetError, ObservedActiveModel,
};
use crate::safe_write::{
    capture_parent_directory, read_path_state_anchored, remove_if_unchanged, write_if_unchanged,
    write_private_if_unchanged, AnchoredPathState, PathIdentity,
};
use crate::settings::{mutate_settings, ModelAgentRuntimeState, Settings};
use jsonc_parser::cst::{CstNode, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

#[cfg(test)]
thread_local! {
    static CONFIG_WRITE_HOOK:
        std::cell::RefCell<Option<Box<dyn FnMut(&Path, bool) -> Result<(), String>>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct ConfigWriteHookGuard;

#[cfg(test)]
impl Drop for ConfigWriteHookGuard {
    fn drop(&mut self) {
        CONFIG_WRITE_HOOK.with(|slot| slot.borrow_mut().take());
    }
}

#[cfg(test)]
pub(crate) fn set_config_write_hook(
    hook: impl FnMut(&Path, bool) -> Result<(), String> + 'static,
) -> ConfigWriteHookGuard {
    CONFIG_WRITE_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    ConfigWriteHookGuard
}

#[cfg(test)]
fn run_config_write_hook(path: &Path, rollback: bool) -> Result<(), String> {
    CONFIG_WRITE_HOOK.with(|slot| {
        slot.borrow_mut()
            .as_mut()
            .map_or(Ok(()), |hook| hook(path, rollback))
    })
}

#[cfg(not(test))]
fn run_config_write_hook(_path: &Path, _rollback: bool) -> Result<(), String> {
    Ok(())
}

pub(crate) const AGENT_ID: &str = "claude-desktop";
pub(crate) const PROFILE_ID: &str = "6d757800-0000-4000-8000-000000000001";
pub(crate) const PROFILE_NAME: &str = "Arca";

pub(crate) struct PreparedClaudeDesktop {
    /// The three ordinary, non-secret targets in display-path order.
    pub files: Vec<PreparedModelFile>,
    pub profile: PreparedClaudeDesktopPrivateFile,
    pub previous_applied_id: Option<String>,
}

pub(crate) struct PreparedClaudeDesktopPrivateFile {
    pub path: PathBuf,
    pub original: Option<Zeroizing<String>>,
    pub content: Option<Zeroizing<String>>,
}

impl PreparedClaudeDesktopPrivateFile {
    fn new(
        path: PathBuf,
        original: Option<Zeroizing<String>>,
        content: Option<Zeroizing<String>>,
    ) -> Self {
        Self {
            path,
            original,
            content,
        }
    }
}

const PRIMARY_CONFIG_INDEX: usize = 0;
const DEPLOYMENT_CONFIG_INDEX: usize = 1;
const META_INDEX: usize = 2;
const PROFILE_INDEX: usize = 3;
const APPLY_WRITE_ORDER: [usize; 4] = [
    PRIMARY_CONFIG_INDEX,
    DEPLOYMENT_CONFIG_INDEX,
    PROFILE_INDEX,
    META_INDEX,
];
const CLEAR_WRITE_ORDER: [usize; 4] = [
    PRIMARY_CONFIG_INDEX,
    DEPLOYMENT_CONFIG_INDEX,
    META_INDEX,
    PROFILE_INDEX,
];

enum PreparedClaudeDesktopFileRef<'a> {
    Ordinary(&'a PreparedModelFile),
    Private(&'a PreparedClaudeDesktopPrivateFile),
}

impl<'a> PreparedClaudeDesktopFileRef<'a> {
    fn path(&self) -> &'a Path {
        match self {
            Self::Ordinary(file) => &file.path,
            Self::Private(file) => &file.path,
        }
    }

    fn original(&self) -> Option<&'a str> {
        match self {
            Self::Ordinary(file) => file.original.as_deref(),
            Self::Private(file) => file.original.as_ref().map(|value| value.as_str()),
        }
    }

    fn content(&self) -> Option<&'a str> {
        match self {
            Self::Ordinary(file) => file.content.as_deref(),
            Self::Private(file) => file.content.as_ref().map(|value| value.as_str()),
        }
    }

    fn private(&self) -> bool {
        matches!(self, Self::Private(_))
    }
}

fn prepared_file(
    prepared: &PreparedClaudeDesktop,
    index: usize,
) -> PreparedClaudeDesktopFileRef<'_> {
    if index == PROFILE_INDEX {
        PreparedClaudeDesktopFileRef::Private(&prepared.profile)
    } else {
        PreparedClaudeDesktopFileRef::Ordinary(&prepared.files[index])
    }
}

pub(crate) fn default_paths() -> Vec<String> {
    vec![
        "~/Library/Application Support/Claude/claude_desktop_config.json".into(),
        "~/Library/Application Support/Claude-3p/claude_desktop_config.json".into(),
        "~/Library/Application Support/Claude-3p/configLibrary/_meta.json".into(),
        format!("~/Library/Application Support/Claude-3p/configLibrary/{PROFILE_ID}.json"),
    ]
}

pub(crate) fn route_is_claude(route: &str) -> bool {
    route
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.to_ascii_lowercase().starts_with("claude-"))
}

fn gateway_base_url(profile: &ModelProfile) -> Result<String, String> {
    const ERROR: &str =
        "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint";
    if profile.protocol != ModelProtocol::AnthropicMessages {
        return Err(ERROR.into());
    }
    super::protocol_client_base_url(&profile.base_url, &profile.protocol, &profile.endpoint_path)
        .map_err(|_| ERROR.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectedInferenceModel<'a> {
    name: &'a str,
    label_override: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectedProfile<'a> {
    inference_provider: &'static str,
    inference_gateway_base_url: String,
    inference_gateway_api_key: &'a str,
    inference_gateway_auth_scheme: &'static str,
    model_discovery_enabled: bool,
    inference_models: [ProjectedInferenceModel<'a>; 1],
    cowork_egress_allowed_hosts: [&'static str; 1],
    disable_deployment_mode_chooser: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    unstable_disable_model_verification: Option<bool>,
}

fn projected_profile_json(
    profile: &ModelProfile,
    credential: &str,
) -> Result<Zeroizing<String>, String> {
    let projected = ProjectedProfile {
        inference_provider: "gateway",
        inference_gateway_base_url: gateway_base_url(profile)?,
        inference_gateway_api_key: credential,
        inference_gateway_auth_scheme: "bearer",
        model_discovery_enabled: false,
        inference_models: [ProjectedInferenceModel {
            name: &profile.model,
            label_override: &profile.name,
        }],
        cowork_egress_allowed_hosts: ["*"],
        disable_deployment_mode_chooser: true,
        unstable_disable_model_verification: (!route_is_claude(&profile.model)).then_some(true),
    };
    let mut content = serde_json::to_string_pretty(&projected)
        .map_err(|error| format!("claude_desktop_profile_serialization_failed: {error}"))?;
    content.push('\n');
    Ok(Zeroizing::new(content))
}

#[cfg(test)]
pub(crate) fn projected_profile(profile: &ModelProfile, credential: &str) -> Result<Value, String> {
    serde_json::from_str(projected_profile_json(profile, credential)?.as_str())
        .map_err(|error| error.to_string())
}

pub(crate) fn validate_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.len() != 4 {
        return Err("claude_desktop_paths_invalid: expected exactly four target paths".into());
    }
    let mut distinct = BTreeSet::new();
    if paths.iter().any(|path| !distinct.insert(path)) {
        return Err(
            "claude_desktop_paths_invalid: all four target roles must use distinct expanded paths"
                .into(),
        );
    }
    Ok(())
}

fn existing_targets_alias(left: &Path, right: &Path) -> Result<bool, String> {
    let inspect = |path: &Path| match fs::metadata(path) {
        Ok(metadata) => {
            let canonical = fs::canonicalize(path).map_err(|error| {
                format!(
                    "claude_desktop_path_identity_failed: {}: {error}",
                    path.display()
                )
            })?;
            Ok(Some((metadata, canonical)))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "claude_desktop_path_identity_failed: {}: {error}",
            path.display()
        )),
    };
    let (Some((left_metadata, left_canonical)), Some((right_metadata, right_canonical))) =
        (inspect(left)?, inspect(right)?)
    else {
        return Ok(false);
    };
    if left_canonical == right_canonical {
        return Ok(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(left_metadata.dev() == right_metadata.dev()
            && left_metadata.ino() == right_metadata.ino());
    }
    #[cfg(not(unix))]
    {
        let _ = (left_metadata, right_metadata);
        Ok(false)
    }
}

fn validate_private_target_aliases(prepared: &PreparedClaudeDesktop) -> Result<(), String> {
    validate_paths(
        &prepared
            .files
            .iter()
            .map(|file| file.path.clone())
            .chain(std::iter::once(prepared.profile.path.clone()))
            .collect::<Vec<_>>(),
    )?;
    for file in &prepared.files {
        if existing_targets_alias(&file.path, &prepared.profile.path)? {
            return Err(format!(
                "claude_desktop_private_target_alias: ordinary target {} aliases the reviewed private target",
                file.path.display()
            ));
        }
    }
    Ok(())
}

fn capture_regular_role(path: &Path) -> Result<AnchoredPathState, String> {
    let parent = capture_parent_directory(path)?;
    match read_path_state_anchored(path, &parent)? {
        state @ (AnchoredPathState::Missing | AnchoredPathState::File { .. }) => Ok(state),
        AnchoredPathState::Symlink { .. }
        | AnchoredPathState::Directory { .. }
        | AnchoredPathState::Other { .. } => Err(format!(
            "claude_desktop_path_unsafe: target must be a regular file or missing: {}",
            path.display()
        )),
    }
}

fn captured_identity(state: &AnchoredPathState) -> Option<PathIdentity> {
    match state {
        AnchoredPathState::File { identity, .. } => Some(*identity),
        AnchoredPathState::Missing
        | AnchoredPathState::Symlink { .. }
        | AnchoredPathState::Directory { .. }
        | AnchoredPathState::Other { .. } => None,
    }
}

fn capture_role_states(paths: &[PathBuf]) -> Result<Vec<AnchoredPathState>, String> {
    validate_paths(paths)?;
    let mut states: [Option<AnchoredPathState>; 4] = std::array::from_fn(|_| None);
    // Hold the reviewed private inode/bytes in a zeroizing state before any
    // ordinary role is converted into a normal String.
    states[PROFILE_INDEX] = Some(capture_regular_role(&paths[PROFILE_INDEX])?);
    for index in [PRIMARY_CONFIG_INDEX, DEPLOYMENT_CONFIG_INDEX, META_INDEX] {
        states[index] = Some(capture_regular_role(&paths[index])?);
    }
    let private = states[PROFILE_INDEX]
        .as_ref()
        .expect("private role was captured");
    for index in [PRIMARY_CONFIG_INDEX, DEPLOYMENT_CONFIG_INDEX, META_INDEX] {
        let ordinary = states[index].as_ref().expect("ordinary role was captured");
        let identity_alias = captured_identity(private).is_some_and(|private_identity| {
            private_identity.is_exact()
                && captured_identity(ordinary).is_some_and(|ordinary_identity| {
                    ordinary_identity.is_exact() && ordinary_identity == private_identity
                })
        });
        let canonical_alias = if matches!(private, AnchoredPathState::Missing)
            || matches!(ordinary, AnchoredPathState::Missing)
        {
            false
        } else {
            fs::canonicalize(&paths[PROFILE_INDEX]).map_err(|error| {
                format!(
                    "claude_desktop_path_identity_failed: {}: {error}",
                    paths[PROFILE_INDEX].display()
                )
            })? == fs::canonicalize(&paths[index]).map_err(|error| {
                format!(
                    "claude_desktop_path_identity_failed: {}: {error}",
                    paths[index].display()
                )
            })?
        };
        if identity_alias || canonical_alias {
            return Err(format!(
                "claude_desktop_private_target_alias: ordinary target {} aliases the reviewed private target",
                paths[index].display()
            ));
        }
    }
    Ok(states
        .into_iter()
        .map(|state| state.expect("all Claude Desktop roles were captured"))
        .collect())
}

fn state_text(path: &Path, state: AnchoredPathState) -> Result<Option<String>, String> {
    match state {
        AnchoredPathState::Missing => Ok(None),
        AnchoredPathState::File { bytes, .. } => {
            String::from_utf8(bytes.to_vec()).map(Some).map_err(|_| {
                format!(
                    "claude_desktop_json_invalid: {}: file is not UTF-8",
                    path.display()
                )
            })
        }
        AnchoredPathState::Symlink { .. }
        | AnchoredPathState::Directory { .. }
        | AnchoredPathState::Other { .. } => {
            unreachable!("capture_role_states rejected unsafe roles")
        }
    }
}

fn secret_state_text(
    path: &Path,
    state: AnchoredPathState,
) -> Result<Option<Zeroizing<String>>, String> {
    match state {
        AnchoredPathState::Missing => Ok(None),
        AnchoredPathState::File { bytes, .. } => String::from_utf8(bytes.to_vec())
            .map(|text| Some(Zeroizing::new(text)))
            .map_err(|_| {
                format!(
                    "claude_desktop_json_invalid: {}: file is not UTF-8",
                    path.display()
                )
            }),
        AnchoredPathState::Symlink { .. }
        | AnchoredPathState::Directory { .. }
        | AnchoredPathState::Other { .. } => {
            unreachable!("capture_role_states rejected unsafe roles")
        }
    }
}

fn parse_json(
    path: &Path,
    original: Option<String>,
    missing: Value,
) -> Result<(Option<String>, Value), String> {
    let value = match original.as_deref() {
        Some(content) => serde_json::from_str(content)
            .map_err(|error| format!("claude_desktop_json_invalid: {}: {error}", path.display()))?,
        None => missing,
    };
    if !value.is_object() {
        return Err(format!(
            "claude_desktop_json_invalid: {}: expected a JSON object",
            path.display()
        ));
    }
    Ok((original, value))
}

fn parse_secret_json(
    path: &Path,
    mut original: Option<Zeroizing<String>>,
) -> Result<Option<Zeroizing<String>>, String> {
    let Some(content) = original.as_deref() else {
        return Ok(None);
    };
    let parsed = CstRootNode::parse(content, &ParseOptions::default());
    let root = match parsed {
        Ok(value) => value,
        Err(error) => {
            original
                .as_mut()
                .expect("present secret Profile content")
                .zeroize();
            return Err(format!(
                "claude_desktop_json_invalid: {}: {error}",
                path.display()
            ));
        }
    };
    if validate_cst_root(&root, path).is_err() {
        original
            .as_mut()
            .expect("present secret Profile content")
            .zeroize();
        return Err(format!(
            "claude_desktop_json_invalid: {}: expected a JSON object",
            path.display()
        ));
    }
    Ok(original)
}

fn parse_cst(
    path: &Path,
    original: Option<String>,
) -> Result<(CstRootNode, Option<String>), String> {
    let root = CstRootNode::parse(
        original.as_deref().unwrap_or_default(),
        &ParseOptions::default(),
    )
    .map_err(|error| format!("claude_desktop_json_invalid: {}: {error}", path.display()))?;
    Ok((root, original))
}

fn cst_root_object(root: &CstRootNode, path: &Path) -> Result<CstObject, String> {
    root.object_value_or_create().ok_or_else(|| {
        format!(
            "claude_desktop_json_invalid: {}: expected a JSON object",
            path.display()
        )
    })
}

fn validate_unique_nested(node: &CstNode, path: &Path, context: &str) -> Result<(), String> {
    if let Some(object) = node.as_object() {
        super::ensure_unique_keys(&object, path, context)?;
        for property in object.properties() {
            let name = property
                .name()
                .and_then(|name| name.decoded_value().ok())
                .unwrap_or_else(|| "<unknown>".into());
            if let Some(value) = property.value() {
                validate_unique_nested(&value, path, &format!("{context}.{name}"))?;
            }
        }
    } else if let Some(array) = node.as_array() {
        for (index, element) in array.elements().into_iter().enumerate() {
            validate_unique_nested(&element, path, &format!("{context}[{index}]"))?;
        }
    }
    Ok(())
}

fn validate_cst_root(root: &CstRootNode, path: &Path) -> Result<CstObject, String> {
    let object = cst_root_object(root, path)?;
    super::ensure_unique_keys(&object, path, "$root")?;
    for property in object.properties() {
        let name = property
            .name()
            .and_then(|name| name.decoded_value().ok())
            .unwrap_or_else(|| "<unknown>".into());
        if let Some(value) = property.value() {
            validate_unique_nested(&value, path, &format!("$root.{name}"))?;
        }
    }
    Ok(object)
}

fn cst_string(node: &CstNode, field: &str, path: &Path) -> Result<Option<String>, String> {
    let Some(object) = node.as_object() else {
        return Err(format!(
            "claude_desktop_json_invalid: {}: entries must contain objects",
            path.display()
        ));
    };
    Ok(object
        .get(field)
        .and_then(|property| property.value())
        .and_then(|value| value.to_serde_value())
        .and_then(|value| value.as_str().map(str::to_string)))
}

fn prepare_deployment_config(
    path: &Path,
    original: Option<String>,
) -> Result<(Option<String>, String), String> {
    let (root, original) = parse_cst(path, original)?;
    let object = validate_cst_root(&root, path)?;
    super::set_json_property(
        &object,
        "deploymentMode",
        Some(Value::String("3p".into())),
        path,
        "$root",
    )?;
    Ok((original, formatted_cst(&root)))
}

fn formatted_cst(root: &CstRootNode) -> String {
    let mut content = root.to_string();
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
}

fn prepare_meta_apply(
    path: &Path,
    original: Option<String>,
) -> Result<(Option<String>, String, Option<String>), String> {
    let (root, original) = parse_cst(path, original)?;
    let object = validate_cst_root(&root, path)?;
    let entries = object.array_value_or_create("entries").ok_or_else(|| {
        format!(
            "claude_desktop_json_invalid: {}: entries must be an array",
            path.display()
        )
    })?;
    let elements = entries.elements();
    if elements.iter().any(|entry| entry.as_object().is_none()) {
        return Err("claude_desktop_json_invalid: entries must contain objects".into());
    }
    let reserved = elements
        .iter()
        .filter(|entry| cst_string(entry, "id", path).ok().flatten().as_deref() == Some(PROFILE_ID))
        .collect::<Vec<_>>();
    if reserved.len() > 1
        || reserved.first().is_some_and(|entry| {
            !matches!(cst_string(entry, "name", path).ok().flatten().as_deref(), Some(PROFILE_NAME) | Some("MUX"))
        })
    {
        return Err(
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
                .into(),
        );
    }
    let applied = object
        .get("appliedId")
        .and_then(|property| property.value())
        .map(|value| {
            value
                .to_serde_value()
                .and_then(|value| value.as_str().map(str::to_string))
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    "claude_desktop_json_invalid: appliedId must be a non-empty string".to_string()
                })
        })
        .transpose()?;
    if let Some(applied) = applied.as_deref() {
        let matches = elements
            .iter()
            .filter(|entry| {
                cst_string(entry, "id", path).ok().flatten().as_deref() == Some(applied)
            })
            .count();
        if matches != 1 {
            return Err(
                "claude_desktop_meta_ambiguous: appliedId must identify exactly one entry".into(),
            );
        }
    }
    if reserved.is_empty() {
        entries.append(super::input_value(
            json!({"id": PROFILE_ID, "name": PROFILE_NAME}),
        ));
    }
    super::set_json_property(
        &object,
        "appliedId",
        Some(Value::String(PROFILE_ID.into())),
        path,
        "$root",
    )?;
    Ok((
        original,
        formatted_cst(&root),
        applied.filter(|value| value != PROFILE_ID),
    ))
}

fn prepare_meta_clear(
    path: &Path,
    original: Option<String>,
    remembered: Option<&str>,
) -> Result<(Option<String>, Option<String>), String> {
    let Some(original_text) = original else {
        return Ok((None, None));
    };
    let (root, original) = parse_cst(path, Some(original_text))?;
    let object = validate_cst_root(&root, path)?;
    let Some(entries_property) = object.get("entries") else {
        return Ok((original.clone(), original));
    };
    let entries = entries_property.array_value().ok_or_else(|| {
        format!(
            "claude_desktop_json_invalid: {}: entries must be an array",
            path.display()
        )
    })?;
    let elements = entries.elements();
    if elements.iter().any(|entry| entry.as_object().is_none()) {
        return Err("claude_desktop_json_invalid: entries must contain objects".into());
    }
    let arca_entries = elements
        .iter()
        .filter(|entry| cst_string(entry, "id", path).ok().flatten().as_deref() == Some(PROFILE_ID))
        .cloned()
        .collect::<Vec<_>>();
    if arca_entries.len() > 1
        || arca_entries.first().is_some_and(|entry| {
            !matches!(cst_string(entry, "name", path).ok().flatten().as_deref(), Some(PROFILE_NAME) | Some("MUX"))
        })
    {
        return Err(
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
                .into(),
        );
    }
    let applied = object
        .get("appliedId")
        .and_then(|property| property.value())
        .and_then(|value| value.to_serde_value())
        .and_then(|value| value.as_str().map(str::to_string));
    let arca_active = applied.as_deref() == Some(PROFILE_ID);
    if arca_active {
        let remembered_matches = remembered
            .filter(|id| !id.trim().is_empty() && *id != PROFILE_ID)
            .map(|id| {
                elements
                    .iter()
                    .filter(|entry| {
                        cst_string(entry, "id", path).ok().flatten().as_deref() == Some(id)
                    })
                    .filter_map(|entry| cst_string(entry, "id", path).ok().flatten())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let defaults = elements
            .iter()
            .filter(|entry| {
                cst_string(entry, "name", path).ok().flatten().as_deref() == Some("Default")
            })
            .filter_map(|entry| cst_string(entry, "id", path).ok().flatten())
            .filter(|id| !id.trim().is_empty() && id != PROFILE_ID)
            .collect::<Vec<_>>();
        let restore =
            match (remembered_matches.as_slice(), defaults.as_slice()) {
                ([remembered], _) => remembered.clone(),
                ([], [default]) => default.clone(),
                _ => return Err(
                    "claude_desktop_restore_target_missing: no previous or Default Profile exists"
                        .into(),
                ),
            };
        super::set_json_property(
            &object,
            "appliedId",
            Some(Value::String(restore)),
            path,
            "$root",
        )?;
    }
    let had_arca_entry = !arca_entries.is_empty();
    for entry in arca_entries {
        entry.remove();
    }
    let changed = arca_active || had_arca_entry;
    Ok((
        original.clone(),
        changed.then(|| formatted_cst(&root)).or(original),
    ))
}

fn reject_profile_collision(entries: &[Value]) -> Result<(), String> {
    let reserved = entries
        .iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) == Some(PROFILE_ID))
        .collect::<Vec<_>>();
    if reserved.len() > 1
        || reserved
            .first()
            .is_some_and(|entry| !matches!(entry.get("name").and_then(Value::as_str), Some(PROFILE_NAME) | Some("MUX")))
    {
        return Err(
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
                .into(),
        );
    }
    Ok(())
}

fn validate_meta_entry_shapes(entries: &[Value]) -> Result<(), String> {
    if entries.iter().any(|entry| !entry.is_object()) {
        return Err("claude_desktop_json_invalid: entries must contain objects".into());
    }
    Ok(())
}

pub(crate) fn prepare_apply(
    paths: &[PathBuf],
    profile: &ModelProfile,
    credential: &str,
) -> Result<PreparedClaudeDesktop, String> {
    let states: [AnchoredPathState; 4] = capture_role_states(paths)?
        .try_into()
        .expect("capture_role_states returns exactly four roles");
    let [first_state, second_state, meta_state, profile_state] = states;
    let (first_original, first_content) =
        prepare_deployment_config(&paths[0], state_text(&paths[0], first_state)?)?;
    let (second_original, second_content) =
        prepare_deployment_config(&paths[1], state_text(&paths[1], second_state)?)?;
    let (meta_original, meta_content, previous_applied_id) =
        prepare_meta_apply(&paths[2], state_text(&paths[2], meta_state)?)?;

    let mut profile_content = projected_profile_json(profile, credential)?;
    let profile_original =
        match parse_secret_json(&paths[3], secret_state_text(&paths[3], profile_state)?) {
            Ok(original) => original,
            Err(error) => {
                profile_content.zeroize();
                return Err(error);
            }
        };
    Ok(PreparedClaudeDesktop {
        files: vec![
            PreparedModelFile {
                path: paths[0].clone(),
                original: first_original,
                content: Some(first_content),
            },
            PreparedModelFile {
                path: paths[1].clone(),
                original: second_original,
                content: Some(second_content),
            },
            PreparedModelFile {
                path: paths[2].clone(),
                original: meta_original,
                content: Some(meta_content),
            },
        ],
        profile: PreparedClaudeDesktopPrivateFile::new(
            paths[3].clone(),
            profile_original,
            Some(profile_content),
        ),
        previous_applied_id,
    })
}

pub(crate) fn prepare_clear(
    paths: &[PathBuf],
    remembered: Option<&str>,
) -> Result<PreparedClaudeDesktop, String> {
    let states: [AnchoredPathState; 4] = capture_role_states(paths)?
        .try_into()
        .expect("capture_role_states returns exactly four roles");
    let [first_state, second_state, meta_state, profile_state] = states;
    let first_original = state_text(&paths[0], first_state)?;
    let second_original = state_text(&paths[1], second_state)?;
    let (meta_original, meta_content) =
        prepare_meta_clear(&paths[2], state_text(&paths[2], meta_state)?, remembered)?;
    let profile_original =
        parse_secret_json(&paths[3], secret_state_text(&paths[3], profile_state)?)?;

    Ok(PreparedClaudeDesktop {
        files: vec![
            PreparedModelFile {
                path: paths[0].clone(),
                original: first_original.clone(),
                content: first_original,
            },
            PreparedModelFile {
                path: paths[1].clone(),
                original: second_original.clone(),
                content: second_original,
            },
            PreparedModelFile {
                path: paths[2].clone(),
                original: meta_original,
                content: meta_content,
            },
        ],
        profile: PreparedClaudeDesktopPrivateFile::new(paths[3].clone(), profile_original, None),
        previous_applied_id: None,
    })
}

fn zeroize_secret_file(prepared: &mut PreparedClaudeDesktop) {
    if let Some(original) = prepared.profile.original.as_mut() {
        original.zeroize();
    }
    if let Some(content) = prepared.profile.content.as_mut() {
        content.zeroize();
    }
}

fn backup_ordinary_files(prepared: &PreparedClaudeDesktop) -> Result<(), String> {
    validate_private_target_aliases(prepared)?;
    let stamp = backup_timestamp();
    for index in [PRIMARY_CONFIG_INDEX, DEPLOYMENT_CONFIG_INDEX, META_INDEX] {
        let file = &prepared.files[index];
        if let Some(original) = file.original.as_deref() {
            backup_reviewed_config_bytes(&file.path, original.as_bytes(), AGENT_ID, &stamp)?;
        }
    }
    Ok(())
}

fn write_prepared_file(file: PreparedClaudeDesktopFileRef<'_>) -> Result<bool, String> {
    let private = file.private();
    match (file.original(), file.content()) {
        (Some(original), Some(content)) if !private && original == content => Ok(false),
        (original, Some(content)) => {
            run_config_write_hook(file.path(), false)?;
            if private {
                write_private_if_unchanged(file.path(), original, content)?;
            } else {
                write_if_unchanged(file.path(), original, content)?;
            }
            Ok(true)
        }
        (Some(original), None) => {
            run_config_write_hook(file.path(), false)?;
            remove_if_unchanged(file.path(), original)?;
            Ok(true)
        }
        (None, None) => Ok(false),
    }
}

fn rollback_prepared_file(file: PreparedClaudeDesktopFileRef<'_>) -> Result<(), String> {
    run_config_write_hook(file.path(), true)?;
    let private = file.private();
    match (file.original(), file.content()) {
        (Some(original), Some(content)) => {
            if private {
                write_private_if_unchanged(file.path(), Some(content), original)
            } else {
                write_if_unchanged(file.path(), Some(content), original)
            }
        }
        (None, Some(content)) => remove_if_unchanged(file.path(), content),
        (Some(original), None) => {
            if private {
                write_private_if_unchanged(file.path(), None, original)
            } else {
                write_if_unchanged(file.path(), None, original)
            }
        }
        (None, None) => Ok(()),
    }
}

fn commit_prepared(
    prepared: &PreparedClaudeDesktop,
    order: &[usize; 4],
) -> Result<(), ModelTargetError> {
    backup_ordinary_files(prepared)?;
    let mut applied = Vec::with_capacity(order.len());
    for &index in order {
        match write_prepared_file(prepared_file(prepared, index)) {
            Ok(true) => applied.push(index),
            Ok(false) => {}
            Err(error) => {
                let mut rollback_errors = Vec::new();
                for previous in applied.into_iter().rev() {
                    if let Err(rollback) = rollback_prepared_file(prepared_file(prepared, previous))
                    {
                        rollback_errors.push(rollback);
                    }
                }
                return if rollback_errors.is_empty() {
                    // The shared safe writer currently exposes only a string
                    // error, so a failed write attempt cannot be proven to
                    // have stopped before publication. Keep outer recovery
                    // evidence even when every earlier target rolled back.
                    Err(ModelTargetError::RecoveryRequired(format!(
                        "Claude Desktop config update failed after a write attempt; earlier targets were rolled back: {error}"
                    )))
                } else {
                    Err(ModelTargetError::RecoveryRequired(format!(
                        "Claude Desktop config update failed ({error}); rollback failed: {}",
                        rollback_errors.join("; ")
                    )))
                };
            }
        }
    }
    Ok(())
}

pub(crate) fn apply(
    paths: &[PathBuf],
    profile: &ModelProfile,
    credential: Zeroizing<Vec<u8>>,
) -> Result<ModelApplyResult, ModelTargetError> {
    let credential_text = std::str::from_utf8(credential.as_slice()).map_err(|_| {
        "claude_desktop_credential_invalid: Claude Desktop API Key must be valid UTF-8".to_string()
    })?;
    let mut credential_string = Zeroizing::new(credential_text.to_owned());
    let mut prepared = prepare_apply(paths, profile, credential_string.as_str())?;
    let previous_applied_id = prepared.previous_applied_id.take();
    let files = prepared
        .files
        .iter()
        .map(|file| file.path.display().to_string())
        .chain(std::iter::once(prepared.profile.path.display().to_string()))
        .collect();
    let committed = commit_prepared(&prepared, &APPLY_WRITE_ORDER);
    zeroize_secret_file(&mut prepared);
    credential_string.zeroize();
    committed?;

    if let Some(previous_applied_profile_id) = previous_applied_id {
        mutate_settings(|settings| {
            settings.set_model_agent_runtime_state(
                AGENT_ID,
                ModelAgentRuntimeState {
                    previous_applied_profile_id: Some(previous_applied_profile_id),
                    ..Default::default()
                },
            );
        })
        .map_err(|error| {
            ModelTargetError::RecoveryRequired(format!(
                "Claude Desktop config was applied, but Arca could not record restore state: {error}"
            ))
        })?;
    }

    Ok(ModelApplyResult {
        agent: AGENT_ID.into(),
        profile: profile.id.clone(),
        files,
        restart_required: true,
        message: "Claude Desktop model routing updated; restart Claude Desktop to use it.".into(),
    })
}

pub(crate) fn clear(paths: &[PathBuf], remembered: Option<&str>) -> Result<(), ModelTargetError> {
    let mut prepared = prepare_clear(paths, remembered)?;
    let committed = commit_prepared(&prepared, &CLEAR_WRITE_ORDER);
    zeroize_secret_file(&mut prepared);
    committed?;
    mutate_settings(|settings| {
        settings.remove_model_agent_runtime_state(AGENT_ID);
    })
    .map_err(|error| {
        ModelTargetError::RecoveryRequired(format!(
            "Claude Desktop config was cleared, but Arca could not clear restore state: {error}"
        ))
    })?;
    Ok(())
}

fn normalized_profile_matches(file: &PreparedClaudeDesktopPrivateFile) -> Result<bool, String> {
    let (Some(original), Some(candidate)) = (&file.original, &file.content) else {
        return Ok(false);
    };
    let mut actual: Value = serde_json::from_str(original).map_err(|_| {
        format!(
            "claude_desktop_json_invalid: {}: invalid Arca Profile JSON",
            file.path.display()
        )
    })?;
    let Some(Value::String(credential)) = actual.get_mut("inferenceGatewayApiKey") else {
        return Ok(false);
    };
    let credential_present = !credential.is_empty();
    credential.zeroize();
    if !credential_present {
        return Ok(false);
    }
    let expected: Value = serde_json::from_str(candidate)
        .expect("prepare_apply always emits valid Claude Desktop Profile JSON");
    Ok(actual == expected)
}

fn private_profile_security(path: &Path) -> Result<Option<bool>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "claude_desktop_read_failed: {}: {error}",
                path.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(Some(false));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return Ok(Some(metadata.permissions().mode() & 0o777 == 0o600));
    }
    #[cfg(not(unix))]
    Ok(Some(true))
}

fn meta_matches_ignoring_active_selection(file: &PreparedModelFile) -> Result<bool, String> {
    let (Some(original), Some(candidate)) = (&file.original, &file.content) else {
        return Ok(false);
    };
    let actual: Value = serde_json::from_str(original).map_err(|_| {
        "claude_desktop_json_invalid: invalid Claude Desktop metadata JSON".to_string()
    })?;
    let mut expected: Value = serde_json::from_str(candidate)
        .expect("prepare_apply always emits valid Claude Desktop metadata JSON");
    expected["appliedId"] = actual.get("appliedId").cloned().unwrap_or(Value::Null);
    Ok(actual == expected)
}

fn observe_prepared(
    mut prepared: PreparedClaudeDesktop,
    ignore_active_selection: bool,
) -> Result<ModelObservedState, String> {
    let observed = (|| {
        let mut missing = false;
        let mut drifted = false;
        for (index, file) in prepared.files[..PROFILE_INDEX].iter().enumerate() {
            match (&file.original, &file.content) {
                (None, Some(_)) => missing = true,
                (Some(_), Some(_))
                    if index == META_INDEX
                        && ignore_active_selection
                        && !meta_matches_ignoring_active_selection(file)? =>
                {
                    drifted = true;
                }
                (Some(_), Some(_)) if index == META_INDEX && ignore_active_selection => {}
                (Some(original), Some(content)) if original != content => drifted = true,
                (Some(_), Some(_)) | (None, None) => {}
                (Some(_), None) => drifted = true,
            }
        }
        if prepared.profile.original.is_none() {
            missing = true;
        } else if !normalized_profile_matches(&prepared.profile)? {
            drifted = true;
        }
        Ok::<_, String>(if missing {
            ModelObservedState::Missing
        } else if drifted {
            ModelObservedState::Drifted
        } else {
            ModelObservedState::Synced
        })
    })();
    zeroize_secret_file(&mut prepared);
    match observed {
        Ok(state) => Ok(state),
        Err(_) => Ok(ModelObservedState::Conflicted),
    }
}

pub(crate) fn observe(
    paths: &[PathBuf],
    profile: &ModelProfile,
) -> Result<ModelObservedState, String> {
    if matches!(
        private_profile_security(&paths[PROFILE_INDEX]),
        Ok(Some(false)) | Err(_)
    ) {
        return Ok(ModelObservedState::Conflicted);
    }
    let prepared = match prepare_apply(paths, profile, "") {
        Ok(prepared) => prepared,
        Err(_) => return Ok(ModelObservedState::Conflicted),
    };
    observe_prepared(prepared, false)
}

pub(crate) fn observe_installed(
    paths: &[PathBuf],
    profile: &ModelProfile,
) -> Result<ModelObservedState, String> {
    if matches!(
        private_profile_security(&paths[PROFILE_INDEX]),
        Ok(Some(false)) | Err(_)
    ) {
        return Ok(ModelObservedState::Conflicted);
    }
    let prepared = match prepare_apply(paths, profile, "") {
        Ok(prepared) => prepared,
        Err(_) => return Ok(ModelObservedState::Conflicted),
    };
    observe_prepared(prepared, true)
}

enum MetaSelection {
    Missing,
    None,
    Arca,
    External,
}

fn meta_selection(path: &Path) -> Result<MetaSelection, String> {
    let original = state_text(path, capture_regular_role(path)?)?;
    let (original, meta) = parse_json(path, original, json!({}))?;
    if original.is_none() {
        return Ok(MetaSelection::Missing);
    }
    let entries: &[Value] = match meta.get("entries") {
        Some(entries) => entries.as_array().ok_or_else(|| {
            format!(
                "claude_desktop_json_invalid: {}: entries must be an array",
                path.display()
            )
        })?,
        None => &[],
    };
    validate_meta_entry_shapes(entries)?;
    reject_profile_collision(entries)?;
    let Some(applied) = meta.get("appliedId") else {
        return Ok(MetaSelection::None);
    };
    let applied = applied
        .as_str()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "claude_desktop_json_invalid: {}: appliedId must be a non-empty string",
                path.display()
            )
        })?;
    let matches = entries
        .iter()
        .filter(|entry| entry.get("id").and_then(Value::as_str) == Some(applied))
        .count();
    if matches != 1 {
        return Err(format!(
            "claude_desktop_meta_ambiguous: {}: appliedId must identify exactly one entry",
            path.display()
        ));
    }
    Ok(if applied == PROFILE_ID {
        MetaSelection::Arca
    } else {
        MetaSelection::External
    })
}

pub(crate) fn observe_active(settings: &Settings, paths: &[PathBuf]) -> ObservedActiveModel {
    if validate_paths(paths).is_err() {
        return ObservedActiveModel::Conflicted;
    }
    match meta_selection(&paths[META_INDEX]) {
        Ok(MetaSelection::Arca) => {
            let Some(profile_id) = settings.model_selection(AGENT_ID).active_profile_id else {
                return ObservedActiveModel::Conflicted;
            };
            if settings
                .model_profiles
                .as_ref()
                .is_some_and(|profiles| profiles.contains_key(&profile_id))
            {
                ObservedActiveModel::Managed(profile_id)
            } else {
                ObservedActiveModel::Conflicted
            }
        }
        Ok(MetaSelection::External) => ObservedActiveModel::External,
        Ok(MetaSelection::Missing | MetaSelection::None) => ObservedActiveModel::None,
        Err(_) => ObservedActiveModel::Conflicted,
    }
}

pub(crate) fn observe_external(paths: &[PathBuf]) -> Result<ExternalModelObservedState, String> {
    validate_paths(paths)?;
    Ok(match meta_selection(&paths[META_INDEX]) {
        Ok(MetaSelection::External) => ExternalModelObservedState::Present,
        Ok(MetaSelection::Missing | MetaSelection::None | MetaSelection::Arca) => {
            ExternalModelObservedState::Absent
        }
        Err(_) => ExternalModelObservedState::Conflicted,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        default_paths, prepare_apply, prepare_clear, projected_profile, set_config_write_hook,
        AGENT_ID, PROFILE_ID, PROFILE_NAME,
    };
    use crate::domain::agents::ModelStorageAuthority;
    use crate::domain::types::{ModelProfile, ModelProtocol};
    use crate::resources::model::{
        apply_profile, clear_profile, credential_snapshot, list_agents, load_settings,
        model_agent_capability, observe_active_model_for_settings, observe_external_model,
        observe_profile, save_profile, set_credential, ExternalModelObservedState,
        ModelObservedState, ObservedActiveModel,
    };
    use crate::safe_write::{begin_transaction_write_tracking, capture_parent_directory};
    use crate::testenv::TestHome;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    const FAKE_CREDENTIAL: &str = "claude-desktop-credential-for-test";

    fn profile(model: &str, endpoint_path: &str) -> ModelProfile {
        ModelProfile {
            id: "profile-for-test".into(),
            name: "Qwen 3.7 Max".into(),
            provider_id: None,
            provider: "custom".into(),
            model_vendor: None,
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: "https://max-ai.amap.com".into(),
            endpoint_path: endpoint_path.into(),
            model: model.into(),
            env_key: None,
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
        }
    }

    fn target_paths(home: &TestHome) -> Vec<PathBuf> {
        vec![
            home.home
                .join("Library/Application Support/Claude/claude_desktop_config.json"),
            home.home
                .join("Library/Application Support/Claude-3p/claude_desktop_config.json"),
            home.home
                .join("Library/Application Support/Claude-3p/configLibrary/_meta.json"),
            home.home.join(format!(
                "Library/Application Support/Claude-3p/configLibrary/{PROFILE_ID}.json"
            )),
        ]
    }

    fn write_json(path: &Path, value: &Value) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn provider_backed_profile(model: &str) -> ModelProfile {
        let mut profile = profile(model, "/anthropic/v1/messages");
        profile.id = "claude-desktop-model".into();
        profile.name = "Claude Desktop Test Model".into();
        profile.provider_id = Some("claude-desktop-provider".into());
        profile.base_url = "https://gateway.example.test".into();
        profile
    }

    fn save_provider_backed_profile(model: &str, credential: Option<&str>) -> ModelProfile {
        let profile = provider_backed_profile(model);
        save_profile(profile.clone(), credential.map(str::to_string)).unwrap();
        profile
    }

    fn seed_external_claude_desktop(paths: &[PathBuf]) {
        write_json(
            &paths[0],
            &json!({
                "deploymentMode": "native",
                "mcpServers": {"keep": {"command": "keep-me"}},
                "future": {"keep": true}
            }),
        );
        write_json(
            &paths[1],
            &json!({
                "mcpServers": {"alsoKeep": {"url": "https://example.test"}},
                "unrelated": [1, 2, 3]
            }),
        );
        write_json(
            &paths[2],
            &json!({
                "appliedId": "cc-switch-id",
                "entries": external_entries(),
                "future": {"keep": true}
            }),
        );
    }

    fn read_value(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn prepared_json(prepared: &super::PreparedClaudeDesktop, index: usize) -> Value {
        let content = if index == super::PROFILE_INDEX {
            prepared
                .profile
                .content
                .as_ref()
                .map(|value| value.as_str())
        } else {
            prepared.files[index].content.as_deref()
        };
        serde_json::from_str(content.expect("prepared target should contain JSON")).unwrap()
    }

    fn preparation_error(result: Result<super::PreparedClaudeDesktop, String>) -> String {
        match result {
            Ok(_) => panic!("preparation unexpectedly succeeded"),
            Err(error) => error,
        }
    }

    fn external_entries() -> Value {
        json!([
            {"id": "default-id", "name": "Default", "futureEntry": 1},
            {"id": "cc-switch-id", "name": "CC Switch", "futureEntry": 2}
        ])
    }

    #[test]
    fn default_paths_cover_all_claude_desktop_targets_in_commit_order() {
        assert_eq!(
            default_paths(),
            vec![
                "~/Library/Application Support/Claude/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/claude_desktop_config.json",
                "~/Library/Application Support/Claude-3p/configLibrary/_meta.json",
                "~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json",
            ]
        );
    }

    #[test]
    fn prepare_apply_preserves_unknown_data_and_prepares_four_targets_losslessly() {
        let home = TestHome::new("claude-apply");
        let paths = target_paths(&home);
        write_json(
            &paths[0],
            &json!({
                "deploymentMode": "native",
                "mcpServers": {"keep": {"command": "keep-me"}},
                "future": {"keep": true}
            }),
        );
        write_json(
            &paths[1],
            &json!({
                "mcpServers": {"alsoKeep": {"url": "https://example.test"}},
                "unrelated": [1, 2, 3]
            }),
        );
        write_json(
            &paths[2],
            &json!({
                "appliedId": "cc-switch-id",
                "entries": external_entries(),
                "future": {"keep": true}
            }),
        );

        let model = profile("qwen3.7-max", "/v1/messages");
        let prepared = prepare_apply(&paths, &model, "credential-for-test").unwrap();

        assert_eq!(
            prepared.previous_applied_id.as_deref(),
            Some("cc-switch-id")
        );
        assert_eq!(
            prepared
                .files
                .iter()
                .map(|file| &file.path)
                .chain(std::iter::once(&prepared.profile.path))
                .collect::<Vec<_>>(),
            paths.iter().collect::<Vec<_>>()
        );
        let first = prepared_json(&prepared, 0);
        assert_eq!(first["deploymentMode"], "3p");
        assert_eq!(first["mcpServers"]["keep"]["command"], "keep-me");
        assert_eq!(first["future"]["keep"], true);
        let second = prepared_json(&prepared, 1);
        assert_eq!(second["deploymentMode"], "3p");
        assert_eq!(
            second["mcpServers"]["alsoKeep"]["url"],
            "https://example.test"
        );
        assert_eq!(second["unrelated"], json!([1, 2, 3]));
        let meta = prepared_json(&prepared, 2);
        assert_eq!(meta["appliedId"], PROFILE_ID);
        assert_eq!(meta["future"]["keep"], true);
        assert_eq!(
            meta["entries"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry["id"] == PROFILE_ID && entry["name"] == PROFILE_NAME)
                .count(),
            1
        );
        assert!(prepared.files.iter().all(|file| {
            file.content
                .as_deref()
                .is_none_or(|content| content.ends_with('\n'))
        }));
        assert!(prepared
            .profile
            .content
            .as_ref()
            .is_some_and(|content| content.ends_with('\n')));
        assert_eq!(
            prepared_json(&prepared, 3),
            projected_profile(&model, "credential-for-test").unwrap()
        );
    }

    #[test]
    fn prepare_clear_restores_remembered_profile_and_removes_only_arca_owned_targets() {
        let home = TestHome::new("claude-clear");
        let paths = target_paths(&home);
        let first = json!({"deploymentMode": "3p", "mcpServers": {"keep": {}}});
        let second = json!({"deploymentMode": "3p", "future": {"keep": true}});
        write_json(&paths[0], &first);
        write_json(&paths[1], &second);
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [
                    {"id": "default-id", "name": "Default"},
                    {"id": "cc-switch-id", "name": "CC Switch", "external": true},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ],
                "future": {"keep": true}
            }),
        );
        write_json(&paths[3], &json!({"owned": "arca"}));

        let prepared = prepare_clear(&paths, Some("cc-switch-id")).unwrap();

        assert_eq!(prepared.previous_applied_id, None);
        assert_eq!(prepared.files.len() + 1, 4);
        assert_eq!(prepared_json(&prepared, 0), first);
        assert_eq!(prepared_json(&prepared, 1), second);
        let meta = prepared_json(&prepared, 2);
        assert_eq!(meta["appliedId"], "cc-switch-id");
        assert_eq!(meta["future"]["keep"], true);
        assert_eq!(meta["entries"].as_array().unwrap().len(), 2);
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["external"] == true));
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["id"] != PROFILE_ID));
        assert!(prepared.files.iter().all(|file| file.content.is_some()));
        assert!(prepared.profile.content.is_none());
    }

    #[test]
    fn prepare_clear_preserves_an_already_active_external_profile() {
        let home = TestHome::new("clear-active");
        let paths = target_paths(&home);
        write_json(&paths[0], &json!({"deploymentMode": "3p"}));
        write_json(&paths[1], &json!({"deploymentMode": "3p"}));
        write_json(
            &paths[2],
            &json!({
                "appliedId": "active-external",
                "entries": [
                    {"id": "remembered", "name": "CC Switch"},
                    {"id": "active-external", "name": "Other"},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ]
            }),
        );

        let prepared = prepare_clear(&paths, Some("remembered")).unwrap();

        assert_eq!(prepared_json(&prepared, 2)["appliedId"], "active-external");
    }

    #[test]
    fn prepare_clear_falls_back_to_default_when_remembered_entry_is_absent() {
        let home = TestHome::new("clear-default");
        let paths = target_paths(&home);
        write_json(&paths[0], &json!({"deploymentMode": "3p"}));
        write_json(&paths[1], &json!({"deploymentMode": "3p"}));
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [
                    {"id": "default-id", "name": "Default"},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ]
            }),
        );

        let prepared = prepare_clear(&paths, Some("missing-id")).unwrap();

        assert_eq!(prepared_json(&prepared, 2)["appliedId"], "default-id");
    }

    #[test]
    fn prepare_clear_never_restores_arca_and_falls_back_to_default() {
        let home = TestHome::new("clear-self-default");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [
                    {"id": "default-id", "name": "Default"},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ]
            }),
        );

        let prepared = prepare_clear(&paths, Some(PROFILE_ID)).unwrap();

        assert_eq!(prepared_json(&prepared, 2)["appliedId"], "default-id");
    }

    #[test]
    fn prepare_clear_rejects_remembered_arca_without_default_restore_target() {
        let home = TestHome::new("clear-self-missing");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [{"id": PROFILE_ID, "name": PROFILE_NAME}]
            }),
        );

        let error = preparation_error(prepare_clear(&paths, Some(PROFILE_ID)));

        assert_eq!(
            error,
            "claude_desktop_restore_target_missing: no previous or Default Profile exists"
        );
    }

    #[test]
    fn prepare_clear_fails_closed_without_a_safe_restore_target() {
        let home = TestHome::new("clear-missing");
        let paths = target_paths(&home);
        write_json(&paths[0], &json!({"deploymentMode": "3p"}));
        write_json(&paths[1], &json!({"deploymentMode": "3p"}));
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [{"id": PROFILE_ID, "name": PROFILE_NAME}]
            }),
        );

        let error = preparation_error(prepare_clear(&paths, Some("missing-id")));

        assert_eq!(
            error,
            "claude_desktop_restore_target_missing: no previous or Default Profile exists"
        );
    }

    #[test]
    fn prepare_apply_rejects_reserved_profile_id_collision() {
        let home = TestHome::new("apply-collision");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "entries": [{"id": PROFILE_ID, "name": "Not Arca"}]
            }),
        );

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert_eq!(
            error,
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
        );
    }

    #[test]
    fn prepare_apply_rejects_duplicate_reserved_profile_entries_as_ambiguous() {
        let home = TestHome::new("apply-duplicate");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "entries": [
                    {"id": PROFILE_ID, "name": PROFILE_NAME},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ]
            }),
        );

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert_eq!(
            error,
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
        );
    }

    #[test]
    fn prepare_apply_rejects_ambiguous_external_applied_selection() {
        let home = TestHome::new("apply-ambiguous-external");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": "external-id",
                "entries": [
                    {"id": "external-id", "name": "First"},
                    {"id": "external-id", "name": "Second"}
                ]
            }),
        );

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert_eq!(
            error,
            "claude_desktop_meta_ambiguous: appliedId must identify exactly one entry"
        );
    }

    #[test]
    fn prepare_apply_rejects_non_string_external_applied_selection() {
        let home = TestHome::new("apply-invalid-external");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({"appliedId": 42, "entries": external_entries()}),
        );

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert_eq!(
            error,
            "claude_desktop_json_invalid: appliedId must be a non-empty string"
        );
    }

    #[test]
    fn prepare_apply_rejects_non_object_metadata_entries() {
        let home = TestHome::new("apply-invalid-entry-shape");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": "default-id",
                "entries": [
                    {"id": "default-id", "name": "Default"},
                    "malformed-entry"
                ]
            }),
        );

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert_eq!(
            error,
            "claude_desktop_json_invalid: entries must contain objects"
        );
    }

    #[test]
    fn prepare_apply_preserves_jsonc_comments_and_rejects_nested_duplicate_keys() {
        let home = TestHome::new("claude-jsonc-lossless");
        let paths = target_paths(&home);
        fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        fs::create_dir_all(paths[2].parent().unwrap()).unwrap();
        fs::write(
            &paths[0],
            "{\n  // keep-primary-comment\n  \"future\": {\"keep\": true}\n}\n",
        )
        .unwrap();
        fs::write(
            &paths[2],
            "{\n  // keep-meta-comment\n  \"appliedId\": \"default-id\",\n  \"entries\": [{\"id\":\"default-id\",\"name\":\"Default\"}]\n}\n",
        )
        .unwrap();

        let prepared = prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            FAKE_CREDENTIAL,
        )
        .unwrap();
        assert!(prepared.files[0]
            .content
            .as_deref()
            .unwrap()
            .contains("keep-primary-comment"));
        assert!(prepared.files[2]
            .content
            .as_deref()
            .unwrap()
            .contains("keep-meta-comment"));

        fs::write(&paths[0], r#"{"future":{"x":1,"x":2}}"#).unwrap();
        let error = prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            FAKE_CREDENTIAL,
        )
        .err()
        .expect("nested duplicate keys must fail closed");
        assert!(error.contains("duplicate JSON key"), "{error}");
    }

    #[test]
    fn prepare_clear_rejects_ambiguous_default_restore_entries() {
        let home = TestHome::new("claude-clear-duplicate-default");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [
                    {"id": PROFILE_ID, "name": PROFILE_NAME},
                    {"id": "default-a", "name": "Default"},
                    {"id": "default-b", "name": "Default"}
                ]
            }),
        );
        let error = prepare_clear(&paths, None)
            .err()
            .expect("ambiguous Default entries must fail closed");
        assert_eq!(
            error,
            "claude_desktop_restore_target_missing: no previous or Default Profile exists"
        );
    }

    #[test]
    fn prepare_clear_rejects_duplicate_reserved_profile_entries_as_ambiguous() {
        let home = TestHome::new("clear-collision");
        let paths = target_paths(&home);
        write_json(
            &paths[2],
            &json!({
                "appliedId": "external-id",
                "entries": [
                    {"id": PROFILE_ID, "name": PROFILE_NAME},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ]
            }),
        );

        let error = preparation_error(prepare_clear(&paths, None));

        assert_eq!(
            error,
            "claude_desktop_profile_collision: reserved Arca Profile ID is owned by another entry"
        );
    }

    #[test]
    fn prepare_clear_keeps_missing_configs_and_metadata_as_true_noops() {
        let home = TestHome::new("clear-missing-files");
        let paths = target_paths(&home);

        let prepared = prepare_clear(&paths, None).unwrap();

        for file in &prepared.files {
            assert!(file.original.is_none());
            assert!(file.content.is_none());
        }
        assert!(prepared.profile.original.is_none());
        assert!(prepared.profile.content.is_none());
    }

    #[test]
    fn prepare_clear_does_not_materialize_entries_in_unchanged_external_metadata() {
        let home = TestHome::new("clear-meta-noop");
        let paths = target_paths(&home);
        let original = r#"{"appliedId":"external-id","future":{"keep":true}}"#;
        fs::create_dir_all(paths[2].parent().unwrap()).unwrap();
        fs::write(&paths[2], original).unwrap();

        let prepared = prepare_clear(&paths, Some("remembered-id")).unwrap();

        assert_eq!(prepared.files[2].original.as_deref(), Some(original));
        assert_eq!(prepared.files[2].content.as_deref(), Some(original));
        assert!(prepared_json(&prepared, 2).get("entries").is_none());
    }

    #[test]
    fn malformed_json_fails_closed_with_path_but_without_document_contents() {
        let home = TestHome::new("apply-malformed");
        let paths = target_paths(&home);
        fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        fs::write(&paths[0], r#"{"secret":"do-not-echo""#).unwrap();

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        ));

        assert!(error.contains(&paths[0].display().to_string()));
        assert!(!error.contains("do-not-echo"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_desktop_ordinary_backup_rejects_a_private_hardlink_alias() {
        let home = TestHome::new("claude-backup-private-alias");
        let paths = target_paths(&home);
        let sentinel = "PRIVATE-ALIAS-BACKUP-SENTINEL";
        write_json(&paths[3], &json!({"inferenceGatewayApiKey": sentinel}));
        fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        fs::hard_link(&paths[3], &paths[0]).unwrap();
        let prepared = super::PreparedClaudeDesktop {
            files: vec![
                super::PreparedModelFile {
                    path: paths[0].clone(),
                    original: Some(r#"{"ordinary":"reviewed"}"#.into()),
                    content: Some(r#"{"ordinary":"desired"}"#.into()),
                },
                super::PreparedModelFile {
                    path: paths[1].clone(),
                    original: None,
                    content: Some("{}".into()),
                },
                super::PreparedModelFile {
                    path: paths[2].clone(),
                    original: None,
                    content: Some("{}".into()),
                },
            ],
            profile: super::PreparedClaudeDesktopPrivateFile::new(
                paths[3].clone(),
                Some(zeroize::Zeroizing::new(
                    r#"{"inferenceGatewayApiKey":"PRIVATE-ALIAS-BACKUP-SENTINEL"}"#.into(),
                )),
                None,
            ),
            previous_applied_id: None,
        };

        let error = super::backup_ordinary_files(&prepared).unwrap_err();

        assert!(
            error.contains("claude_desktop_private_target_alias"),
            "{error}"
        );
        assert!(!home.home.join(".arca/backups").exists());
        assert_eq!(read_value(&paths[3])["inferenceGatewayApiKey"], sentinel);
    }

    #[cfg(unix)]
    #[test]
    fn claude_desktop_prepare_rejects_a_private_hardlink_before_role_parsing() {
        let home = TestHome::new("claude-prepare-private-alias");
        let paths = target_paths(&home);
        write_json(
            &paths[3],
            &json!({"inferenceGatewayApiKey": "PRIVATE-PREPARE-ALIAS-SENTINEL"}),
        );
        fs::create_dir_all(paths[0].parent().unwrap()).unwrap();
        fs::hard_link(&paths[3], &paths[0]).unwrap();

        let error = preparation_error(prepare_apply(
            &paths,
            &profile("claude-sonnet-4-6", "/v1/messages"),
            "new-credential",
        ));

        assert!(
            error.contains("claude_desktop_private_target_alias"),
            "{error}"
        );
        assert!(!home.home.join(".arca/backups").exists());
    }

    #[test]
    fn non_claude_route_disables_verification() {
        let projected = projected_profile(
            &profile("qwen3.7-max", "/v1/messages"),
            "credential-for-test",
        )
        .expect("Anthropic Messages profile should project");

        assert_eq!(projected["inferenceProvider"], "gateway");
        assert_eq!(
            projected["inferenceGatewayBaseUrl"],
            "https://max-ai.amap.com"
        );
        assert_eq!(projected["inferenceGatewayApiKey"], "credential-for-test");
        assert_eq!(projected["inferenceGatewayAuthScheme"], "bearer");
        assert_eq!(projected["modelDiscoveryEnabled"], false);
        assert_eq!(projected["unstableDisableModelVerification"], true);
        assert_eq!(projected["inferenceModels"][0]["name"], "qwen3.7-max");
        assert_eq!(
            projected["inferenceModels"][0]["labelOverride"],
            "Qwen 3.7 Max"
        );
        assert_eq!(
            projected["coworkEgressAllowedHosts"],
            serde_json::json!(["*"])
        );
        assert_eq!(projected["disableDeploymentModeChooser"], true);
    }

    #[test]
    fn claude_route_keeps_verification_enabled() {
        for model in ["claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"] {
            let projected =
                projected_profile(&profile(model, "/v1/messages"), "credential-for-test")
                    .expect("Anthropic Messages profile should project");
            assert!(projected.get("unstableDisableModelVerification").is_none());
        }
    }

    #[test]
    fn non_messages_endpoint_fails_closed() {
        let error = projected_profile(
            &profile("qwen3.7-max", "/chat/completions"),
            "credential-for-test",
        )
        .expect_err("non-Messages endpoint must fail closed");
        assert_eq!(
            error,
            "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint"
        );
    }

    #[test]
    fn prefixed_anthropic_messages_endpoint_projects_prefixed_client_base_url() {
        let mut model = profile("claude-sonnet-4-6", "/anthropic/v1/messages");
        model.base_url = "https://gateway.example".into();

        let projected = projected_profile(&model, "credential-for-test").unwrap();

        assert_eq!(
            projected["inferenceGatewayBaseUrl"],
            "https://gateway.example/anthropic"
        );
    }

    #[test]
    fn non_anthropic_messages_protocol_fails_with_stable_endpoint_error() {
        let mut model = profile("claude-sonnet-4-6", "/v1/messages");
        model.protocol = ModelProtocol::OpenaiResponses;

        let error = projected_profile(&model, "credential-for-test").unwrap_err();

        assert_eq!(
            error,
            "claude_desktop_endpoint_unsupported: expected an Anthropic /v1/messages endpoint"
        );
    }

    #[test]
    fn public_apply_commits_four_targets_and_records_observed_runtime_state() {
        let home = TestHome::new("claude-desktop-public-apply");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));

        let result = apply_profile(AGENT_ID, &profile.id).unwrap();

        assert!(result.restart_required);
        assert_eq!(
            result.files,
            paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
        );
        let first = read_value(&paths[0]);
        assert_eq!(first["deploymentMode"], "3p");
        assert_eq!(first["mcpServers"]["keep"]["command"], "keep-me");
        assert_eq!(first["future"]["keep"], true);
        let second = read_value(&paths[1]);
        assert_eq!(second["deploymentMode"], "3p");
        assert_eq!(
            second["mcpServers"]["alsoKeep"]["url"],
            "https://example.test"
        );
        assert_eq!(second["unrelated"], json!([1, 2, 3]));
        let meta = read_value(&paths[2]);
        assert_eq!(meta["appliedId"], PROFILE_ID);
        assert_eq!(meta["future"]["keep"], true);
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == PROFILE_ID && entry["name"] == PROFILE_NAME));
        let projected = read_value(&paths[3]);
        assert_eq!(
            projected["inferenceGatewayBaseUrl"],
            "https://gateway.example.test/anthropic"
        );
        assert_eq!(projected["inferenceGatewayApiKey"], FAKE_CREDENTIAL);
        assert_eq!(projected["inferenceModels"][0]["name"], "qwen3.7-max");
        assert_eq!(projected["unstableDisableModelVerification"], true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&paths[3]).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let settings = load_settings();
        assert_eq!(
            settings
                .model_agent_runtime_state(AGENT_ID)
                .and_then(|state| state.previous_applied_profile_id.as_deref()),
            Some("cc-switch-id")
        );
        let stored = settings
            .model_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(&profile.id))
            .unwrap();
        assert_eq!(
            observe_profile(AGENT_ID, stored).unwrap(),
            ModelObservedState::Synced
        );
        assert_eq!(
            observe_active_model_for_settings(&settings, AGENT_ID),
            ObservedActiveModel::Managed(profile.id)
        );
    }

    #[test]
    fn public_clear_restores_external_selection_and_keeps_central_assets() {
        let home = TestHome::new("claude-desktop-public-clear");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));
        crate::settings::mutate_settings(|settings| {
            settings.set_model_agent_runtime_state(
                AGENT_ID,
                crate::settings::ModelAgentRuntimeState {
                    previous_applied_profile_id: None,
                    extra: BTreeMap::from([("futureRuntimeField".into(), json!({"keep": true}))]),
                },
            );
        })
        .unwrap();
        apply_profile(AGENT_ID, &profile.id).unwrap();

        clear_profile(AGENT_ID, &profile.id).unwrap();

        assert_eq!(read_value(&paths[0])["deploymentMode"], "3p");
        assert_eq!(read_value(&paths[0])["future"]["keep"], true);
        assert_eq!(read_value(&paths[1])["unrelated"], json!([1, 2, 3]));
        let meta = read_value(&paths[2]);
        assert_eq!(meta["appliedId"], "cc-switch-id");
        assert_eq!(meta["future"]["keep"], true);
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "default-id"));
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["id"] == "cc-switch-id"));
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["id"] != PROFILE_ID));
        assert!(!paths[3].exists());

        let settings = load_settings();
        assert!(settings
            .model_profiles
            .as_ref()
            .is_some_and(|profiles| profiles.contains_key(&profile.id)));
        assert!(settings
            .model_providers
            .as_ref()
            .is_some_and(|providers| providers.contains_key("claude-desktop-provider")));
        assert_eq!(
            credential_snapshot(&profile.id).unwrap().as_deref(),
            Some(FAKE_CREDENTIAL.as_bytes())
        );
        let runtime = settings.model_agent_runtime_state(AGENT_ID).unwrap();
        assert!(runtime.previous_applied_profile_id.is_none());
        assert_eq!(runtime.extra["futureRuntimeField"]["keep"], true);
        assert!(!settings
            .model_assignments
            .unwrap_or_default()
            .contains_key(AGENT_ID));
    }

    #[test]
    fn public_clear_preserves_a_user_selected_external_profile() {
        let home = TestHome::new("claude-desktop-public-clear-external-active");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));
        apply_profile(AGENT_ID, &profile.id).unwrap();
        let mut meta = read_value(&paths[2]);
        meta["appliedId"] = json!("cc-switch-id");
        write_json(&paths[2], &meta);

        clear_profile(AGENT_ID, &profile.id).unwrap();

        let meta = read_value(&paths[2]);
        assert_eq!(meta["appliedId"], "cc-switch-id");
        assert!(meta["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["id"] != PROFILE_ID));
        assert!(!paths[3].exists());
    }

    #[test]
    fn committed_claude_route_omits_unstable_verification_override() {
        let home = TestHome::new("claude-desktop-claude-route");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        let profile = save_provider_backed_profile("claude-sonnet-4-6", Some(FAKE_CREDENTIAL));

        apply_profile(AGENT_ID, &profile.id).unwrap();

        assert!(read_value(&paths[3])
            .get("unstableDisableModelVerification")
            .is_none());
    }

    #[test]
    fn missing_credential_fails_before_writing_any_claude_desktop_target() {
        let home = TestHome::new("claude-desktop-missing-credential");
        let paths = target_paths(&home);
        let profile = save_provider_backed_profile("qwen3.7-max", None);

        let error = apply_profile(AGENT_ID, &profile.id).unwrap_err();

        assert_eq!(
            error,
            "claude_desktop_credential_missing: Claude Desktop requires an API Key in Arca Keychain"
        );
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn non_utf8_credential_returns_a_stable_secret_free_error() {
        let home = TestHome::new("claude-desktop-invalid-credential");
        let paths = target_paths(&home);
        let profile = save_provider_backed_profile("qwen3.7-max", None);
        set_credential(&profile.id, b"invalid-\xff-do-not-echo").unwrap();

        let error = apply_profile(AGENT_ID, &profile.id).unwrap_err();

        assert_eq!(
            error,
            "claude_desktop_credential_invalid: Claude Desktop API Key must be valid UTF-8"
        );
        assert!(!error.contains("do-not-echo"));
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[cfg(unix)]
    #[test]
    fn apply_tightens_an_existing_arca_profile_to_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let home = TestHome::new("claude-desktop-private-profile");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        write_json(&paths[3], &json!({"legacy": "replace-me"}));
        fs::set_permissions(&paths[3], fs::Permissions::from_mode(0o644)).unwrap();
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));

        apply_profile(AGENT_ID, &profile.id).unwrap();

        assert_eq!(
            fs::metadata(&paths[3]).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn observation_rejects_a_world_readable_private_profile() {
        use std::os::unix::fs::PermissionsExt;

        let home = TestHome::new("claude-desktop-observe-private-mode");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));
        apply_profile(AGENT_ID, &profile.id).unwrap();
        fs::set_permissions(&paths[3], fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(
            observe_profile(AGENT_ID, &profile).unwrap(),
            ModelObservedState::Conflicted
        );
    }

    #[cfg(unix)]
    #[test]
    fn stale_late_target_rolls_back_earlier_files_and_secret_profile() {
        let home = TestHome::new("claude-desktop-rollback");
        let paths = target_paths(&home);
        seed_external_claude_desktop(&paths);
        write_json(
            &paths[2],
            &json!({
                "appliedId": PROFILE_ID,
                "entries": [
                    {"id": "default-id", "name": "Default"},
                    {"id": PROFILE_ID, "name": PROFILE_NAME}
                ],
                "reviewed": true
            }),
        );
        let profile = save_provider_backed_profile("qwen3.7-max", Some(FAKE_CREDENTIAL));
        let first_before = fs::read_to_string(&paths[0]).unwrap();
        let second_before = fs::read_to_string(&paths[1]).unwrap();
        let parent_snapshots = paths
            .iter()
            .map(|path| (path.clone(), capture_parent_directory(path).unwrap()))
            .collect::<BTreeMap<_, _>>();
        let tracker = begin_transaction_write_tracking(
            &home.home.join("write-evidence"),
            &paths,
            &parent_snapshots,
        )
        .unwrap();
        let stale_meta = paths[2].clone();
        let _hook = set_config_write_hook(move |path, rollback| {
            if path == stale_meta && !rollback {
                write_json(
                    &stale_meta,
                    &json!({
                        "appliedId": PROFILE_ID,
                        "entries": [
                            {"id": "default-id", "name": "Default"},
                            {"id": PROFILE_ID, "name": PROFILE_NAME}
                        ],
                        "externalEdit": true
                    }),
                );
            }
            Ok(())
        });

        let error = apply_profile(AGENT_ID, &profile.id).unwrap_err();
        drop(tracker);

        assert!(error.contains("rolled back"), "{error}");
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(), first_before);
        assert_eq!(fs::read_to_string(&paths[1]).unwrap(), second_before);
        assert!(!paths[3].exists());
        assert_eq!(read_value(&paths[2])["externalEdit"], true);
    }

    #[test]
    fn capability_registration_declares_exact_claude_desktop_contract() {
        let _home = TestHome::new("claude-desktop-capability");
        let listed = list_agents()
            .into_iter()
            .find(|agent| agent.id == AGENT_ID)
            .expect("Claude Desktop should be listed");
        let capability = model_agent_capability(AGENT_ID).unwrap();

        for agent in [&listed, &capability] {
            assert_eq!(agent.name, "Claude");
            assert_eq!(agent.mode, "managed");
            assert_eq!(agent.storage_authority, ModelStorageAuthority::ArcaMapping);
            assert_eq!(agent.config_paths, default_paths());
            assert!(!agent.supports_multiple);
            assert_eq!(agent.credential_mode, "keychain-export");
            assert_eq!(
                agent.supported_protocols,
                vec![ModelProtocol::AnthropicMessages]
            );
            assert_eq!(agent.docs, "https://support.claude.com/");
            assert!(agent.note.contains("credential"));
            assert!(agent.note.contains("restart"));
        }
    }

    #[test]
    fn non_anthropic_profile_is_rejected_before_any_target_write() {
        let home = TestHome::new("claude-desktop-protocol-gate");
        let paths = target_paths(&home);
        let mut profile = provider_backed_profile("gpt-custom");
        profile.protocol = ModelProtocol::OpenaiResponses;
        profile.endpoint_path = "/v1/responses".into();
        save_profile(profile.clone(), Some(FAKE_CREDENTIAL.into())).unwrap();

        let error = apply_profile(AGENT_ID, &profile.id).unwrap_err();

        assert!(error.contains("does not support the 'openai-responses' profile protocol"));
        assert!(paths.iter().all(|path| !path.exists()));
    }

    #[test]
    fn external_observation_distinguishes_absent_present_and_conflicted_meta() {
        let home = TestHome::new("claude-desktop-external-observation");
        let paths = target_paths(&home);

        assert_eq!(
            observe_external_model(AGENT_ID).unwrap(),
            ExternalModelObservedState::Absent
        );
        write_json(
            &paths[2],
            &json!({
                "appliedId": "external-id",
                "entries": [{"id": "external-id", "name": "External"}]
            }),
        );
        assert_eq!(
            observe_external_model(AGENT_ID).unwrap(),
            ExternalModelObservedState::Present
        );
        write_json(
            &paths[2],
            &json!({
                "appliedId": "external-id",
                "entries": [
                    {"id": "external-id", "name": "First"},
                    {"id": "external-id", "name": "Second"}
                ]
            }),
        );
        assert_eq!(
            observe_external_model(AGENT_ID).unwrap(),
            ExternalModelObservedState::Conflicted
        );
    }
}
