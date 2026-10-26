//! Model adoption into the central asset model.
//!
//! Read-only discovery and explicit adoption of Agent-native Model configs.
//!
//! Candidate DTOs never contain credential values. Planning re-reads every
//! source byte and keeps an optional literal credential only in the existing
//! in-memory pending-payload store used by Model Profile edits.

use super::lifecycle::store_pending_model_profile_secret;
use super::planner::{finalize_plan_with, CredentialAction, LifecycleBinding};
use super::types::{
    AssetOperationKind, AssetOperationPlan, AssetRef, CentralAssetAction, CentralAssetChange,
    DomainPlan, ModelConsumptionRecord,
};
use crate::domain::types::{ModelProfile, ModelProtocol};
use crate::resources::mcp::scanner::expand_tilde;
use crate::resources::model::{
    configured_path_strings_checked, infer_model_vendor, infer_provider, list_agents,
    prepare_profile_draft,
};
use crate::settings::{load_settings_strict, Settings};
use jsonc_parser::cst::CstRootNode;
use jsonc_parser::ParseOptions;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelAdoptionStatus {
    Adoptable,
    NeedsCredential,
    Unsupported,
    Conflicted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelCredentialKind {
    None,
    EnvironmentReference,
    Literal,
    ExternalCommand,
}

/// Secret-free evidence for one Agent-native provider/model entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelAdoptionCandidate {
    pub candidate_id: String,
    pub agent_id: String,
    pub native_id: String,
    /// Existing Arca Profile whose native slot produced this observation.
    /// Present only when the Agent changed fields of a managed Profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub managed_profile_id: Option<String>,
    pub name: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_vendor: Option<String>,
    pub protocol: ModelProtocol,
    pub base_url: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub active: bool,
    pub credential_kind: ModelCredentialKind,
    pub status: ModelAdoptionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub fingerprint: String,
    pub settings_hash: String,
    pub target_hash: String,
    pub candidate_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanModelAdoptionRequest {
    pub candidate_fingerprints: BTreeMap<String, String>,
}

#[derive(Clone)]
enum ExtractedCredential {
    None,
    Env(String),
    Literal(Zeroizing<String>),
    Command,
    Conflict,
    Invalid(String),
}

#[derive(Clone)]
struct ExtractedModel {
    agent_id: String,
    native_id: String,
    name: String,
    protocol: ModelProtocol,
    base_url: String,
    model: String,
    env_key: Option<String>,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    reasoning: Option<bool>,
    enabled: Option<bool>,
    active: bool,
    credential: ExtractedCredential,
    target_paths: Vec<PathBuf>,
}

impl ExtractedModel {
    fn credential_kind(&self) -> ModelCredentialKind {
        match self.credential {
            ExtractedCredential::None | ExtractedCredential::Invalid(_) => {
                ModelCredentialKind::None
            }
            ExtractedCredential::Env(_) => ModelCredentialKind::EnvironmentReference,
            ExtractedCredential::Literal(_) | ExtractedCredential::Conflict => {
                ModelCredentialKind::Literal
            }
            ExtractedCredential::Command => ModelCredentialKind::ExternalCommand,
        }
    }

    fn credential_identity(&self) -> String {
        match &self.credential {
            ExtractedCredential::None => "none".into(),
            ExtractedCredential::Env(key) => format!("env:{key}"),
            ExtractedCredential::Literal(value) => format!("literal:{}", hash(value.as_bytes())),
            ExtractedCredential::Command => "external-command".into(),
            ExtractedCredential::Conflict => "credential-conflict".into(),
            ExtractedCredential::Invalid(reason) => format!("invalid:{}", hash(reason.as_bytes())),
        }
    }

    fn status(&self) -> (ModelAdoptionStatus, Option<String>) {
        if self.agent_id == "qoder-desktop" && self.native_id.starts_with("mux_cli_") {
            return (ModelAdoptionStatus::Unsupported, Some("此条目由 Qoder CLI 管理，请在 CLI 页面操作".into()));
        }
        if matches!(self.credential, ExtractedCredential::Conflict) {
            return (
                ModelAdoptionStatus::Conflicted,
                Some("检测到多个不同的明文 credential，不能安全合并".into()),
            );
        }
        if let ExtractedCredential::Invalid(reason) = &self.credential {
            return (ModelAdoptionStatus::Conflicted, Some(reason.clone()));
        }
        let keychain_capable = matches!(self.agent_id.as_str(), "claude-code" | "codex" | "pi");
        match (&self.credential, keychain_capable) {
            (ExtractedCredential::Env(_) | ExtractedCredential::Literal(_), _) if matches!(self.agent_id.as_str(), "qoder-desktop" | "zcode") => (ModelAdoptionStatus::Adoptable, None),
            (ExtractedCredential::Command, _) => (
                ModelAdoptionStatus::NeedsCredential,
                Some(
                    "外部 credential command 不会被执行；请先改为明文一次性导入或安全环境变量"
                        .into(),
                ),
            ),
            (ExtractedCredential::Env(_), true) => (
                ModelAdoptionStatus::NeedsCredential,
                Some("该 Agent 的 Arca writer 使用 Keychain，不能无损接管外部环境变量引用".into()),
            ),
            (ExtractedCredential::Literal(_), false) => (
                ModelAdoptionStatus::NeedsCredential,
                Some("该 Agent 仅支持环境变量引用；请先把明文 Key 改为环境变量".into()),
            ),
            _ => (ModelAdoptionStatus::Adoptable, None),
        }
    }

    fn profile(&self) -> ModelProfile {
        let provider = infer_provider(&self.base_url);
        ModelProfile {
            id: String::new(),
            provider_id: None,
            name: self.name.clone(),
            model_vendor: infer_model_vendor(&provider, &self.model),
            provider,
            native_ids: BTreeMap::from([(self.agent_id.clone(), self.native_id.clone())]),
            protocol: self.protocol.clone(),
            base_url: self.base_url.clone(),
            endpoint_path: String::new(),
            model: self.model.clone(),
            env_key: self.env_key.clone(),
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
            reasoning: self.reasoning,
        }
    }

    fn fingerprint(&self) -> String {
        let profile = self.profile();
        hash_fields(&[
            profile.provider.as_bytes(),
            protocol_name(&profile.protocol).as_bytes(),
            normalized_url(&profile.base_url).as_bytes(),
            profile.model.as_bytes(),
            match profile.reasoning {
                None => b"reasoning:auto".as_slice(),
                Some(true) => b"reasoning:on".as_slice(),
                Some(false) => b"reasoning:off".as_slice(),
            },
            match self.enabled {
                None => b"enabled:unknown".as_slice(),
                Some(true) => b"enabled:true".as_slice(),
                Some(false) => b"enabled:false".as_slice(),
            },
            self.credential_identity().as_bytes(),
        ])
    }

    fn candidate_id(&self) -> String {
        hash_fields(&[
            self.agent_id.as_bytes(),
            self.native_id.as_bytes(),
            self.model.as_bytes(),
            self.base_url.as_bytes(),
        ])[..24]
            .to_string()
    }
}

pub fn list_model_adoption_candidates() -> Result<Vec<ModelAdoptionCandidate>, String> {
    list_model_adoption_candidates_scoped(&BTreeSet::new())
}

pub(super) fn list_model_adoption_candidates_scoped(agent_ids: &BTreeSet<String>) -> Result<Vec<ModelAdoptionCandidate>, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let settings_hash = hash_optional(fs::read(crate::paths::settings_file()).ok().as_deref());
    let mut candidates = Vec::new();
    for extracted in extract_models_scoped(&settings, agent_ids)? {
        let managed_profile_id = managed_profile_id(&settings, &extracted);
        if let Some(profile_id) = managed_profile_id.as_ref() {
            if managed_profile_matches(&settings, profile_id, &extracted)? { continue; }
        }
        let (status, reason) = extracted.status();
        let target_hash = hash_target_paths(&extracted.target_paths);
        let fingerprint = extracted.fingerprint();
        let candidate_id = extracted.candidate_id();
        let credential_kind = extracted.credential_kind();
        let candidate_hash = hash_fields(&[
            candidate_id.as_bytes(),
            fingerprint.as_bytes(),
            settings_hash.as_bytes(),
            target_hash.as_bytes(),
        ]);
        let profile = extracted.profile();
        candidates.push(ModelAdoptionCandidate {
            candidate_id,
            agent_id: extracted.agent_id,
            native_id: extracted.native_id,
            managed_profile_id,
            name: profile.name,
            provider: profile.provider,
            model_vendor: profile.model_vendor,
            protocol: profile.protocol,
            base_url: profile.base_url,
            model: profile.model,
            env_key: profile.env_key,
            enabled: extracted.enabled,
            active: extracted.active,
            credential_kind,
            status,
            reason,
            fingerprint,
            settings_hash: settings_hash.clone(),
            target_hash,
            candidate_hash,
        });
    }
    candidates.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| left.model.cmp(&right.model))
            .then_with(|| left.agent_id.cmp(&right.agent_id))
    });
    Ok(candidates)
}

/// Exact Agent-native observations that already match an assigned Arca Profile.
///
/// Model writers intentionally own only one model entry, not the entire native
/// provider container. Comparing extracted model identity avoids classifying
/// unrelated sibling models or formatting differences as drift.
pub(super) fn exact_managed_model_observations_scoped(
    settings: &Settings,
    agent_ids: &BTreeSet<String>,
) -> Result<BTreeSet<(String, String)>, String> {
    let mut exact = BTreeSet::new();
    for candidate in extract_models_scoped(settings, agent_ids)? {
        let Some(profile_id) = managed_profile_id(settings, &candidate) else {
            continue;
        };
        if managed_profile_matches(settings, &profile_id, &candidate)? {
            exact.insert((candidate.agent_id, profile_id));
        }
    }
    Ok(exact)
}

pub fn plan_model_adoption(
    request: PlanModelAdoptionRequest,
) -> Result<AssetOperationPlan, String> {
    if request.candidate_fingerprints.is_empty() {
        return Err("invalid_migration_selection: select at least one Model".into());
    }
    let public = list_model_adoption_candidates()?;
    let by_id: BTreeMap<_, _> = public
        .iter()
        .map(|candidate| (candidate.candidate_id.as_str(), candidate))
        .collect();
    let mut selected_public = Vec::new();
    for (candidate_id, expected_fingerprint) in &request.candidate_fingerprints {
        let candidate = by_id.get(candidate_id.as_str()).ok_or_else(|| {
            "migration_selection_stale: a Model candidate is no longer available".to_string()
        })?;
        if &candidate.fingerprint != expected_fingerprint {
            return Err("migration_selection_stale: a Model candidate changed after review".into());
        }
        if candidate.status != ModelAdoptionStatus::Adoptable {
            return Err(
                "migration_credential_required: selected Model cannot be safely adopted".into(),
            );
        }
        selected_public.push(*candidate);
    }
    let fingerprints: BTreeSet<_> = selected_public
        .iter()
        .map(|candidate| candidate.fingerprint.as_str())
        .collect();
    if fingerprints.len() != 1 {
        return Err("migration_conflict: selected Model entries are not identical".into());
    }
    let selected_agents: BTreeSet<_> = selected_public
        .iter()
        .map(|candidate| candidate.agent_id.as_str())
        .collect();
    if selected_agents.len() != selected_public.len() {
        return Err(
            "migration_conflict: one Agent cannot adopt multiple models from the same native provider in one Profile"
                .into(),
        );
    }
    let managed_profile_ids = selected_public
        .iter()
        .filter_map(|candidate| candidate.managed_profile_id.as_deref())
        .collect::<BTreeSet<_>>();
    if managed_profile_ids.len() > 1
        || (!managed_profile_ids.is_empty()
            && selected_public
                .iter()
                .any(|candidate| candidate.managed_profile_id.is_none()))
    {
        return Err(
            "observation_ambiguous: managed and external Model observations cannot be adopted together"
                .into(),
        );
    }
    let managed_profile_id = managed_profile_ids.into_iter().next().map(str::to_string);

    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let extracted_by_id: BTreeMap<_, _> = extract_models(&settings)?
        .into_iter()
        .map(|candidate| (candidate.candidate_id(), candidate))
        .collect();
    let mut selected = Vec::new();
    for candidate in &selected_public {
        let extracted = extracted_by_id
            .get(&candidate.candidate_id)
            .ok_or_else(|| {
                "migration_selection_stale: a Model source changed during planning".to_string()
            })?;
        if extracted.fingerprint() != candidate.fingerprint {
            return Err("migration_selection_stale: Model connection identity changed".into());
        }
        selected.push(extracted.clone());
    }

    let mut draft = selected[0].profile();
    draft.native_ids.clear();
    for candidate in &selected {
        // Imports into these shared registries get a new Arca-owned provider, preserving the external
        // provider and every sibling model instead of adopting its identity.
        if agent_uses_native_id(&candidate.agent_id) && !matches!(candidate.agent_id.as_str(), "zcode" | "qoder-cli" | "opencode-desktop" | "kilo-desktop") {
            draft
                .native_ids
                .insert(candidate.agent_id.clone(), candidate.native_id.clone());
        }
    }
    let native_ids = std::mem::take(&mut draft.native_ids);
    let mut profile = prepare_profile_draft(&settings, managed_profile_id.as_deref(), draft)?;
    profile.native_ids = native_ids;
    let credential = selected
        .iter()
        .find_map(|candidate| match &candidate.credential {
            ExtractedCredential::Literal(value) => Some(value.clone()),
            _ => None,
        });
    let credential_action = if credential.is_some() {
        CredentialAction::Set
    } else {
        CredentialAction::Keep
    };
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for candidate in &selected {
        let current = settings.model_selection(&candidate.agent_id);
        if matches!(candidate.agent_id.as_str(), "zcode" | "qoder-cli" | "opencode-desktop" | "kilo-desktop") {
            // ModelAdopt intentionally never writes native configuration. Import
            // only the central asset; an explicit Add later creates its Arca slot.
            before.insert(candidate.agent_id.clone(), current.clone());
            after.insert(candidate.agent_id.clone(), current);
            continue;
        }
        let mut desired = current.clone();
        desired.profiles.insert(
            profile.id.clone(),
            ModelConsumptionRecord {
                profile_id: profile.id.clone(),
                enabled: true,
                credential: if candidate.agent_id == "qoder-desktop" && matches!(candidate.credential, ExtractedCredential::Env(_)) {
                    crate::domain::assets::ModelCredentialPolicy {
                        source_override: None,
                        delivery: crate::domain::assets::ApiKeyDelivery::Env,
                    }
                } else { Default::default() },
                last_selected_at: candidate
                    .active
                    .then(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            },
        );
        if candidate.active || desired.active_profile_id.is_none() {
            desired.active_profile_id = Some(profile.id.clone());
        }
        crate::resources::model::normalize_model_selection(&candidate.agent_id, &mut desired);
        before.insert(candidate.agent_id.clone(), current);
        after.insert(candidate.agent_id.clone(), desired);
    }
    let domain_plan = DomainPlan::Model { before, after };
    let draft_hash = hash_serializable(&profile)?;
    let mut target_files = selected
        .iter()
        .flat_map(|candidate| candidate.target_paths.iter())
        .map(|path| crate::resources::mcp::scanner::collapse_home(&path.to_string_lossy()))
        .collect::<Vec<_>>();
    target_files.sort();
    target_files.dedup();
    let mut summary = vec![
        if managed_profile_id.is_some() {
            "采用 Agent 当前模型配置并更新中央 Model Profile".into()
        } else {
            "从 Agent 当前配置创建中央 Model Profile".into()
        },
        format!("使用 {} / {}", profile.provider, profile.model),
    ];
    if credential_action == CredentialAction::Set {
        summary.push("将 API Key 保存到钥匙串".into());
    }
    if managed_profile_id.is_none() && selected.iter().any(|candidate| candidate.agent_id == "zcode") {
        summary.push("仅导入中央模型库，保留 ZCode 原 Provider 及其模型；请到 ZCode 页面选择中央模型并添加".into());
    }
    if managed_profile_id.is_none() && selected.iter().any(|candidate| candidate.agent_id == "qoder-cli") {
        summary.push("仅导入中央模型库，保留共享的 Qoder 原配置；请到 Qoder CLI 页面添加中央模型".into());
    }
    if managed_profile_id.is_none() && selected.iter().any(|candidate| candidate.agent_id == "opencode-desktop") {
        summary.push("仅导入中央模型库，保留共享的 OpenCode 原配置；请到 OpenCode Desktop 页面添加中央模型".into());
    }
    if managed_profile_id.is_none() && selected.iter().any(|candidate| candidate.agent_id == "kilo-desktop") {
        summary.push("仅导入中央模型库，保留 Kilo Desktop 原配置；请到 Kilo 页面添加中央模型".into());
    }
    let linked_agents = selected.iter().filter(|candidate| !matches!(candidate.agent_id.as_str(), "zcode" | "qoder-cli" | "opencode-desktop" | "kilo-desktop")).count();
    if linked_agents > 0 {
        summary.push(format!("关联 {} 个 Agent 中的现有模型配置", linked_agents));
    }
    let plan = finalize_plan_with(
        AssetOperationKind::Adopt,
        domain_plan,
        vec![CentralAssetChange {
            asset: AssetRef::Model {
                profile_id: profile.id.clone(),
            },
            action: if managed_profile_id.is_some() {
                CentralAssetAction::Update
            } else {
                CentralAssetAction::Create
            },
            summary,
        }],
        target_files,
        Some(LifecycleBinding::ModelAdopt {
            profile_id: profile.id.clone(),
            draft_hash,
            credential_action,
        }),
    )?;
    store_pending_model_profile_secret(&plan.operation_id, profile, credential);
    Ok(plan)
}

fn extract_models(settings: &Settings) -> Result<Vec<ExtractedModel>, String> {
    extract_models_scoped(settings, &BTreeSet::new())
}

fn extract_models_scoped(settings: &Settings, agent_ids: &BTreeSet<String>) -> Result<Vec<ExtractedModel>, String> {
    let managed: BTreeSet<_> = list_agents()
        .into_iter()
        .filter(|agent| agent.mode == "managed" && (agent_ids.is_empty() || agent_ids.contains(&agent.id)))
        .map(|agent| agent.id)
        .collect();
    let mut extracted = Vec::new();
    for agent_id in managed {
        let paths = match configured_path_strings_checked(settings, &agent_id) {
            Ok(Some(paths)) => paths,
            Ok(None) => continue,
            Err(error) => {
                extracted.push(invalid_candidate(&agent_id, error, Vec::new()));
                continue;
            }
        };
        let paths = paths
            .iter()
            .map(|path| expand_tilde(path))
            .collect::<Vec<_>>();
        let parsed = match agent_id.as_str() {
            "claude-code" => extract_claude(&paths[0]),
            "codex" => extract_codex(&paths[0]),
            "grok-build" => extract_grok(&paths[0]),
            "pi" => extract_pi(&paths[0], &paths[1]),
            "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" => extract_open_code(&agent_id, &paths[0]),
            "qoder-desktop" | "qoder-cli" => extract_qoder(&agent_id, &paths[0]),
            "zcode" => extract_zcode(&paths[0]),
            "qwen-code" => extract_qwen(&paths[0]),
            "crush" => extract_crush(&paths[0]),
            "mistral-vibe" => extract_vibe(&paths[0]),
            "hermes" => extract_hermes(&paths[0]),
            "factory-droid" => extract_factory(&paths[0]),
            "goose" => extract_goose(&paths[0]),
            _ => Ok(Vec::new()),
        };
        let mut rows = match parsed {
            Ok(rows) => rows,
            Err(error) => vec![invalid_candidate(&agent_id, error, paths.clone())],
        };
        extracted.append(&mut rows);
    }
    mark_unsafe_native_identities(&mut extracted);
    mark_shared_native_provider_models(&mut extracted);
    Ok(extracted)
}

fn agent_uses_native_id(agent_id: &str) -> bool {
    matches!(
        agent_id,
        "codex"
            | "grok-build"
            | "pi"
            | "opencode"
            | "opencode-desktop"
            | "kilo-code"
            | "kilo-desktop"
            | "qoder-desktop"
            | "qoder-cli"
            | "zcode"
            | "crush"
            | "mistral-vibe"
            | "hermes"
            | "goose"
    )
}

fn safe_native_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn mark_unsafe_native_identities(extracted: &mut [ExtractedModel]) {
    for candidate in extracted {
        if agent_uses_native_id(&candidate.agent_id) && !safe_native_id(&candidate.native_id) {
            candidate.credential = ExtractedCredential::Invalid(
                "Agent-native provider identity 含有不安全字符，Arca 不会把它用于配置键或文件名"
                    .into(),
            );
        }
    }
}

fn mark_shared_native_provider_models(extracted: &mut [ExtractedModel]) {
    let mut groups = BTreeMap::<(String, String), Vec<usize>>::new();
    for (index, candidate) in extracted.iter().enumerate() {
        if candidate.agent_id == "zcode"
            || matches!(candidate.credential, ExtractedCredential::Invalid(_))
        {
            continue;
        }
        groups
            .entry((candidate.agent_id.clone(), candidate.native_id.clone()))
            .or_default()
            .push(index);
    }
    for indices in groups.into_values().filter(|indices| indices.len() > 1) {
        let keep = indices
            .iter()
            .copied()
            .find(|index| extracted[*index].active)
            .unwrap_or(indices[0]);
        for index in indices.into_iter().filter(|index| *index != keep) {
            extracted[index].credential = ExtractedCredential::Invalid(
                "多个 Model 共用同一个 Agent-native provider；请先在 Agent 中拆分 provider identity，Arca 不会覆盖兄弟模型"
                    .into(),
            );
        }
    }
}

fn invalid_candidate(agent_id: &str, reason: String, target_paths: Vec<PathBuf>) -> ExtractedModel {
    ExtractedModel {
        agent_id: agent_id.into(),
        native_id: "invalid-config".into(),
        name: format!("{agent_id} 配置无法解析"),
        protocol: ModelProtocol::OpenaiCompletions,
        base_url: "https://invalid.local".into(),
        model: "invalid-config".into(),
        env_key: None,
        context_window: None,
        max_output_tokens: None,
        reasoning: None,
        enabled: None,
        active: false,
        credential: ExtractedCredential::Invalid(reason),
        target_paths,
    }
}

fn extract_claude(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(path)? else {
        return Ok(Vec::new());
    };
    let Some(base_url) = root
        .pointer("/env/ANTHROPIC_BASE_URL")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        // A stock Anthropic login is not a reusable custom endpoint.
        return Ok(Vec::new());
    };
    let Some(model) = string_at(&root, &["model"]) else {
        return Ok(Vec::new());
    };
    let auth = string_at(&root, &["env", "ANTHROPIC_AUTH_TOKEN"]);
    let key = string_at(&root, &["env", "ANTHROPIC_API_KEY"]);
    let credential = merge_literals(auth, key, string_at(&root, &["apiKeyHelper"]).is_some());
    Ok(vec![ExtractedModel {
        agent_id: "claude-code".into(),
        native_id: "claude-settings".into(),
        name: model.clone(),
        protocol: ModelProtocol::AnthropicMessages,
        base_url,
        model,
        env_key: None,
        context_window: None,
        max_output_tokens: None,
        reasoning: None,
        enabled: None,
        active: true,
        credential,
        target_paths: vec![path.into()],
    }])
}

fn extract_codex(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_toml_value(path)? else {
        return Ok(Vec::new());
    };
    let Some(native_id) = toml_string(&root, &["model_provider"]) else {
        return Ok(Vec::new());
    };
    let Some(model) = toml_string(&root, &["model"]) else {
        return Ok(Vec::new());
    };
    let Some(provider) = root
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .and_then(|providers| providers.get(&native_id))
    else {
        return Ok(Vec::new());
    };
    let Some(base_url) = provider
        .get("base_url")
        .and_then(toml::Value::as_str)
        .map(str::to_string)
    else {
        return Ok(Vec::new());
    };
    let wire_api = provider
        .get("wire_api")
        .and_then(toml::Value::as_str)
        .unwrap_or("responses");
    let protocol = if wire_api == "responses" {
        ModelProtocol::OpenaiResponses
    } else {
        ModelProtocol::OpenaiCompletions
    };
    let env_key = provider
        .get("env_key")
        .and_then(toml::Value::as_str)
        .map(str::to_string);
    let credential = if let Some(value) = provider
        .get("experimental_bearer_token")
        .and_then(toml::Value::as_str)
    {
        ExtractedCredential::Literal(Zeroizing::new(value.to_string()))
    } else if provider.get("auth").is_some() {
        ExtractedCredential::Command
    } else if let Some(key) = &env_key {
        ExtractedCredential::Env(key.clone())
    } else {
        ExtractedCredential::None
    };
    Ok(vec![ExtractedModel {
        agent_id: "codex".into(),
        native_id,
        name: provider
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or(&model)
            .to_string(),
        protocol,
        base_url,
        model,
        env_key,
        context_window: None,
        max_output_tokens: None,
        reasoning: None,
        enabled: None,
        active: true,
        credential,
        target_paths: vec![path.into()],
    }])
}

fn extract_grok(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_toml_value(path)? else {
        return Ok(Vec::new());
    };
    let active = toml_string(&root, &["models", "default"]);
    let Some(models) = root.get("model").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, value) in models {
        let Some(table) = value.as_table() else {
            continue;
        };
        let Some(model) = table.get("model").and_then(toml::Value::as_str) else {
            continue;
        };
        let Some(base_url) = table.get("base_url").and_then(toml::Value::as_str) else {
            continue;
        };
        let protocol = protocol_from_backend(
            table
                .get("api_backend")
                .and_then(toml::Value::as_str)
                .unwrap_or("chat_completions"),
        );
        let env_key = table
            .get("env_key")
            .and_then(toml::Value::as_str)
            .map(str::to_string);
        let credential = if let Some(value) = table.get("api_key").and_then(toml::Value::as_str) {
            ExtractedCredential::Literal(Zeroizing::new(value.to_string()))
        } else if let Some(key) = &env_key {
            ExtractedCredential::Env(key.clone())
        } else {
            ExtractedCredential::None
        };
        rows.push(ExtractedModel {
            agent_id: "grok-build".into(),
            native_id: native_id.clone(),
            name: table
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or(model)
                .to_string(),
            protocol,
            base_url: base_url.into(),
            model: model.into(),
            env_key,
            context_window: table
                .get("context_window")
                .and_then(toml::Value::as_integer)
                .and_then(|v| u64::try_from(v).ok()),
            max_output_tokens: table
                .get("max_completion_tokens")
                .and_then(toml::Value::as_integer)
                .and_then(|v| u64::try_from(v).ok()),
            reasoning: None,
            enabled: None,
            active: active.as_deref() == Some(native_id.as_str()),
            credential,
            target_paths: vec![path.into()],
        });
    }
    Ok(rows)
}

fn extract_pi(models_path: &Path, settings_path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(models_path)? else {
        return Ok(Vec::new());
    };
    let settings = read_jsonc(settings_path)?.unwrap_or(Value::Null);
    let active_provider = string_at(&settings, &["defaultProvider"]);
    let active_model = string_at(&settings, &["defaultModel"]);
    let Some(providers) = root.get("providers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        let Some(base_url) = provider.get("baseUrl").and_then(Value::as_str) else {
            continue;
        };
        let protocol = protocol_from_name(
            provider
                .get("api")
                .and_then(Value::as_str)
                .unwrap_or("openai-completions"),
        );
        let credential = provider
            .get("apiKey")
            .and_then(Value::as_str)
            .map(pi_credential)
            .unwrap_or(ExtractedCredential::None);
        let Some(models) = provider.get("models").and_then(Value::as_array) else {
            continue;
        };
        for model_value in models {
            let Some(model) = model_value.get("id").and_then(Value::as_str) else {
                continue;
            };
            rows.push(ExtractedModel {
                agent_id: "pi".into(),
                native_id: native_id.clone(),
                name: model_value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(model)
                    .into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                env_key: None,
                context_window: model_value.get("contextWindow").and_then(Value::as_u64),
                max_output_tokens: model_value.get("maxTokens").and_then(Value::as_u64),
                reasoning: model_value.get("reasoning").and_then(Value::as_bool),
                enabled: None,
                active: active_provider.as_deref() == Some(native_id.as_str())
                    && active_model.as_deref().is_none_or(|active| active == model),
                credential: credential.clone(),
                target_paths: vec![models_path.into(), settings_path.into()],
            });
        }
    }
    Ok(rows)
}

fn extract_open_code(agent_id: &str, path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(path)? else {
        return Ok(Vec::new());
    };
    let active = string_at(&root, &["model"]);
    let Some(providers) = root.get("provider").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        let Some(base_url) = provider.pointer("/options/baseURL").and_then(Value::as_str) else {
            continue;
        };
        let protocol = match provider
            .get("npm")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            value if value.contains("anthropic") => ModelProtocol::AnthropicMessages,
            value if value.contains("google") => ModelProtocol::GeminiGenerateContent,
            value if value.contains("openai-compatible") => ModelProtocol::OpenaiCompletions,
            _ => ModelProtocol::OpenaiResponses,
        };
        let (env_key, credential) = provider
            .pointer("/options/apiKey")
            .and_then(Value::as_str)
            .map(env_or_literal)
            .unwrap_or((None, ExtractedCredential::None));
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model_key, model_value) in models {
            let model = model_value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(model_key);
            rows.push(ExtractedModel {
                agent_id: agent_id.into(),
                native_id: native_id.clone(),
                name: model_value
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| provider.get("name").and_then(Value::as_str))
                    .unwrap_or(model)
                    .into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                env_key: env_key.clone(),
                context_window: model_value
                    .pointer("/limit/context")
                    .and_then(Value::as_u64),
                max_output_tokens: model_value.pointer("/limit/output").and_then(Value::as_u64),
                reasoning: model_value.get("reasoning").and_then(Value::as_bool),
                enabled: None,
                active: active.as_deref() == Some(format!("{native_id}/{model}").as_str()),
                credential: credential.clone(),
                target_paths: vec![path.into()],
            });
        }
    }
    Ok(rows)
}

fn extract_qoder(agent_id: &str, path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = crate::resources::model::adapters::read_qoder_registry(path)? else { return Ok(Vec::new()); };
    let Some(providers_value) = root.get("providers") else { return Ok(Vec::new()); };
    let providers = providers_value.as_object().ok_or("Qoder providers must be an object")?;
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        let Some(base_url) = provider.get("baseUrl").and_then(Value::as_str) else { continue; };
        let Some(models) = provider.get("models").and_then(Value::as_array) else { continue; };
        let protocol = match provider.get("protocol").and_then(Value::as_str).unwrap_or("openai") {
            "openai" => ModelProtocol::OpenaiCompletions,
            "openai-responses" => ModelProtocol::OpenaiResponses,
            "anthropic" => ModelProtocol::AnthropicMessages,
            _ => return Err("Qoder provider protocol is unsupported".into()),
        };
        let (env_key, credential) = match provider.get("apiKey").and_then(Value::as_str) {
            Some("") => (None, ExtractedCredential::None),
            Some(key) => env_or_literal(key),
            None => (None, ExtractedCredential::Invalid("Qoder apiKey is missing".into())),
        };
        let mut ids = BTreeSet::new();
        for entry in models {
            let model = entry.get("model").and_then(Value::as_str).ok_or("Qoder model id missing")?;
            if !ids.insert(model) { return Err("Qoder contains duplicate model ids".into()); }
            rows.push(ExtractedModel {
                agent_id: agent_id.into(),
                native_id: native_id.clone(),
                name: entry.get("displayName").and_then(Value::as_str).unwrap_or(model).into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                env_key: env_key.clone(),
                context_window: entry.get("contextWindow").and_then(Value::as_u64),
                max_output_tokens: entry.get("maxOutputTokens").and_then(Value::as_u64),
                reasoning: entry.pointer("/capabilities/thinking/modes").and_then(Value::as_array)
                    .map(|modes| modes.iter().any(|mode| mode.as_str() == Some("enabled"))),
                enabled: None,
                active: agent_id == "qoder-cli" && root.pointer("/model/name").and_then(Value::as_str)
                    == Some(format!("{native_id}/{model}").as_str()),
                credential: if models.len() > 1 {
                    ExtractedCredential::Invalid("此 Qoder provider 共用多个模型；保留只读，避免接管时改动其他模型的连接与凭据".into())
                } else { credential.clone() },
                target_paths: vec![path.into()],
            });
        }
    }
    Ok(rows)
}

fn extract_zcode(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = crate::resources::model::adapters::read_zcode_registry(path)? else {
        return Ok(Vec::new());
    };
    let Some(providers_value) = root.get("provider") else { return Ok(Vec::new()); };
    let providers = providers_value.as_object().ok_or("ZCode provider must be an object")?;
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        if native_id.starts_with("builtin:")
            || provider.get("source").and_then(Value::as_str) != Some("custom")
        {
            continue;
        }
        // Only the verified desktop Chat Completions contract is importable.
        if provider.get("kind").and_then(Value::as_str) != Some("openai-compatible") {
            continue;
        }
        let base_url = provider.pointer("/options/baseURL").and_then(Value::as_str)
            .ok_or("ZCode custom provider baseURL is missing")?;
        let models = provider.get("models").and_then(Value::as_object)
            .ok_or("ZCode custom provider models must be an object")?;
        let credential = match provider.pointer("/options/apiKey").and_then(Value::as_str) {
            Some(key) if !key.is_empty() => ExtractedCredential::Literal(Zeroizing::new(key.into())),
            _ if provider.pointer("/options/apiKeyRequired").and_then(Value::as_bool) == Some(false) => {
                ExtractedCredential::None
            }
            _ => ExtractedCredential::Invalid("ZCode apiKey is missing".into()),
        };
        for (model, entry) in models {
            if model.is_empty() || !entry.is_object() {
                return Err("ZCode model entry is invalid".into());
            }
            rows.push(ExtractedModel {
                agent_id: "zcode".into(),
                native_id: native_id.clone(),
                name: entry.get("name").and_then(Value::as_str).unwrap_or(model).into(),
                protocol: ModelProtocol::OpenaiCompletions,
                base_url: base_url.into(),
                model: model.clone(),
                env_key: None,
                context_window: entry.pointer("/limit/context").and_then(Value::as_u64),
                max_output_tokens: entry.pointer("/limit/output").and_then(Value::as_u64),
                reasoning: entry.pointer("/reasoning/enabled").and_then(Value::as_bool),
                // Provider enablement is not a global/session model selection.
                enabled: Some(provider.get("enabled").and_then(Value::as_bool).unwrap_or(true)),
                active: false,
                credential: credential.clone(),
                target_paths: vec![path.into()],
            });
        }
    }
    Ok(rows)
}

fn extract_qwen(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(path)? else {
        return Ok(Vec::new());
    };
    let active_model = string_at(&root, &["model", "name"]);
    let active_auth = string_at(&root, &["security", "auth", "selectedType"]);
    let Some(providers) = root.get("modelProviders").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (auth, provider) in providers {
        let protocol = if auth == "anthropic" {
            ModelProtocol::AnthropicMessages
        } else {
            ModelProtocol::OpenaiCompletions
        };
        let models = qwen_provider_models_for_discovery(provider, auth)?;
        for value in models {
            let Some(model) = value.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(base_url) = value.get("baseUrl").and_then(Value::as_str) else {
                continue;
            };
            let env_key = value
                .get("envKey")
                .and_then(Value::as_str)
                .map(str::to_string);
            rows.push(ExtractedModel {
                agent_id: "qwen-code".into(),
                native_id: format!("{auth}:{model}:{base_url}"),
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(model)
                    .into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                credential: env_key
                    .as_ref()
                    .map(|key| ExtractedCredential::Env(key.clone()))
                    .unwrap_or(ExtractedCredential::None),
                env_key,
                context_window: value
                    .pointer("/generationConfig/contextWindowSize")
                    .and_then(Value::as_u64),
                max_output_tokens: None,
                reasoning: None,
                enabled: None,
                active: active_auth.as_deref() == Some(auth.as_str())
                    && active_model.as_deref() == Some(model),
                target_paths: vec![path.into()],
            });
        }
    }
    Ok(rows)
}

fn qwen_provider_models_for_discovery<'a>(
    provider: &'a Value,
    auth: &str,
) -> Result<&'a [Value], String> {
    // Keep discovery aligned with the writer: Qwen Code 0.20.0 consumes the
    // direct ModelConfig[] shape. Older Arca builds wrote the exact
    // `{ protocol, models }` wrapper, which remains safe to discover, but an
    // extended or malformed wrapper is Agent-owned and must fail closed.
    match provider {
        Value::Array(models) => Ok(models),
        Value::Object(legacy)
            if legacy.len() == 2
                && legacy.get("protocol").and_then(Value::as_str) == Some(auth) =>
        {
            legacy
                .get("models")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .ok_or_else(|| {
                    format!(
                        "Qwen modelProviders.{auth} legacy wrapper models must be an array"
                    )
                })
        }
        Value::Object(_) => Err(format!(
            "Qwen modelProviders.{auth} is neither the stable array shape nor the exact legacy Arca wrapper"
        )),
        _ => Err(format!(
            "Qwen modelProviders.{auth} must use the stable array shape"
        )),
    }
}

fn extract_crush(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(path)? else {
        return Ok(Vec::new());
    };
    let active_provider = string_at(&root, &["models", "large", "provider"]);
    let active_model = string_at(&root, &["models", "large", "model"]);
    let Some(providers) = root.get("providers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        let Some(base_url) = provider.get("base_url").and_then(Value::as_str) else {
            continue;
        };
        let protocol = if provider.get("type").and_then(Value::as_str) == Some("anthropic") {
            ModelProtocol::AnthropicMessages
        } else {
            ModelProtocol::OpenaiCompletions
        };
        let (env_key, credential) = provider
            .get("api_key")
            .and_then(Value::as_str)
            .map(env_or_literal)
            .unwrap_or((None, ExtractedCredential::None));
        let Some(models) = provider.get("models").and_then(Value::as_array) else {
            continue;
        };
        for value in models {
            let Some(model) = value.get("id").and_then(Value::as_str) else {
                continue;
            };
            rows.push(ExtractedModel {
                agent_id: "crush".into(),
                native_id: native_id.clone(),
                name: value
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| provider.get("name").and_then(Value::as_str))
                    .unwrap_or(model)
                    .into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                env_key: env_key.clone(),
                context_window: None,
                max_output_tokens: None,
                reasoning: None,
                enabled: None,
                active: active_provider.as_deref() == Some(native_id.as_str())
                    && active_model.as_deref() == Some(model),
                credential: credential.clone(),
                target_paths: vec![path.into()],
            });
        }
    }
    Ok(rows)
}

fn extract_vibe(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_toml_value(path)? else {
        return Ok(Vec::new());
    };
    let active = toml_string(&root, &["active_model"]);
    let providers = root
        .get("providers")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let provider_map: BTreeMap<String, toml::Value> = providers
        .into_iter()
        .filter_map(|value| {
            let name = value.get("name")?.as_str()?.to_string();
            Some((name, value))
        })
        .collect();
    let models = root
        .get("models")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for value in models {
        let Some(alias) = value.get("alias").and_then(toml::Value::as_str) else {
            continue;
        };
        let provider_id = value
            .get("provider")
            .and_then(toml::Value::as_str)
            .unwrap_or(alias);
        let Some(provider) = provider_map.get(provider_id) else {
            continue;
        };
        let Some(model) = value.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        let Some(base_url) = provider.get("api_base").and_then(toml::Value::as_str) else {
            continue;
        };
        let env_key = provider
            .get("api_key_env_var")
            .and_then(toml::Value::as_str)
            .map(str::to_string);
        rows.push(ExtractedModel {
            agent_id: "mistral-vibe".into(),
            native_id: provider_id.into(),
            name: alias.into(),
            protocol: ModelProtocol::OpenaiCompletions,
            base_url: base_url.into(),
            model: model.into(),
            env_key: env_key.clone(),
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
            enabled: None,
            active: active.as_deref() == Some(alias),
            credential: env_key
                .map(ExtractedCredential::Env)
                .unwrap_or(ExtractedCredential::None),
            target_paths: vec![path.into()],
        });
    }
    Ok(rows)
}

fn extract_hermes(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_yaml_value(path)? else {
        return Ok(Vec::new());
    };
    let active_provider = string_at(&root, &["model", "provider"])
        .and_then(|value| value.strip_prefix("custom:").map(str::to_string));
    let active_model = string_at(&root, &["model", "default"]);
    let Some(providers) = root.get("providers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, provider) in providers {
        let Some(base_url) = provider.get("api").and_then(Value::as_str) else {
            continue;
        };
        let Some(model) = provider.get("default_model").and_then(Value::as_str) else {
            continue;
        };
        let protocol =
            if provider.get("transport").and_then(Value::as_str) == Some("anthropic_messages") {
                ModelProtocol::AnthropicMessages
            } else {
                ModelProtocol::OpenaiCompletions
            };
        let env_key = provider
            .get("key_env")
            .and_then(Value::as_str)
            .map(str::to_string);
        rows.push(ExtractedModel {
            agent_id: "hermes".into(),
            native_id: native_id.clone(),
            name: provider
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(model)
                .into(),
            protocol,
            base_url: base_url.into(),
            model: model.into(),
            env_key: env_key.clone(),
            context_window: None,
            max_output_tokens: None,
            reasoning: None,
            enabled: None,
            active: active_provider.as_deref() == Some(native_id.as_str())
                && active_model.as_deref().is_none_or(|active| active == model),
            credential: env_key
                .map(ExtractedCredential::Env)
                .unwrap_or(ExtractedCredential::None),
            target_paths: vec![path.into()],
        });
    }
    Ok(rows)
}

fn extract_factory(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_jsonc(path)? else {
        return Ok(Vec::new());
    };
    let active = string_at(&root, &["model"]);
    let models = root
        .get("customModels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut rows = Vec::new();
    for value in models {
        let Some(model) = value.get("model").and_then(Value::as_str) else {
            continue;
        };
        let Some(base_url) = value.get("baseUrl").and_then(Value::as_str) else {
            continue;
        };
        let protocol = match value
            .get("provider")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "anthropic" => ModelProtocol::AnthropicMessages,
            "openai" => ModelProtocol::OpenaiResponses,
            _ => ModelProtocol::OpenaiCompletions,
        };
        let (env_key, credential) = value
            .get("apiKey")
            .and_then(Value::as_str)
            .map(env_or_literal)
            .unwrap_or((None, ExtractedCredential::None));
        rows.push(ExtractedModel {
            agent_id: "factory-droid".into(),
            native_id: format!("{model}:{base_url}"),
            name: value
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(model)
                .into(),
            protocol,
            base_url: base_url.into(),
            model: model.into(),
            env_key,
            context_window: None,
            max_output_tokens: value.get("maxOutputTokens").and_then(Value::as_u64),
            reasoning: None,
            enabled: None,
            active: active.as_deref() == Some(model),
            credential,
            target_paths: vec![path.into()],
        });
    }
    Ok(rows)
}

fn extract_goose(path: &Path) -> Result<Vec<ExtractedModel>, String> {
    let Some(root) = read_yaml_value(path)? else {
        return Ok(Vec::new());
    };
    let active = string_at(&root, &["active_provider"]);
    let Some(providers) = root.get("providers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::new();
    for (native_id, state) in providers {
        if state.get("configured").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        if !safe_native_id(native_id) {
            rows.push(ExtractedModel {
                agent_id: "goose".into(),
                native_id: native_id.clone(),
                name: "Goose provider identity 不安全".into(),
                protocol: ModelProtocol::OpenaiCompletions,
                base_url: "https://invalid.local".into(),
                model: "invalid-config".into(),
                env_key: None,
                context_window: None,
                max_output_tokens: None,
                reasoning: None,
                enabled: None,
                active: active.as_deref() == Some(native_id.as_str()),
                credential: ExtractedCredential::Invalid(
                    "Goose provider identity 不能安全映射到 custom_providers 文件名".into(),
                ),
                target_paths: vec![path.into()],
            });
            continue;
        }
        let provider_path = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("custom_providers")
            .join(format!("{native_id}.json"));
        let Some(provider) = read_plain_json(&provider_path)? else {
            continue;
        };
        let Some(base_url) = provider.get("base_url").and_then(Value::as_str) else {
            continue;
        };
        let protocol = if provider.get("engine").and_then(Value::as_str) == Some("anthropic") {
            ModelProtocol::AnthropicMessages
        } else {
            ModelProtocol::OpenaiCompletions
        };
        let env_key = provider
            .get("api_key_env")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let models = provider
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for value in models {
            let Some(model) = value.get("name").and_then(Value::as_str) else {
                continue;
            };
            rows.push(ExtractedModel {
                agent_id: "goose".into(),
                native_id: native_id.clone(),
                name: provider
                    .get("display_name")
                    .and_then(Value::as_str)
                    .unwrap_or(model)
                    .into(),
                protocol: protocol.clone(),
                base_url: base_url.into(),
                model: model.into(),
                env_key: env_key.clone(),
                context_window: value.get("context_limit").and_then(Value::as_u64),
                max_output_tokens: None,
                reasoning: provider.get("preserves_thinking").and_then(Value::as_bool),
                enabled: None,
                active: active.as_deref() == Some(native_id.as_str())
                    && state
                        .get("model")
                        .and_then(Value::as_str)
                        .is_none_or(|active| active == model),
                credential: env_key
                    .clone()
                    .map(ExtractedCredential::Env)
                    .unwrap_or(ExtractedCredential::None),
                target_paths: vec![path.into(), provider_path.clone()],
            });
        }
    }
    Ok(rows)
}

fn managed_profile_id(settings: &Settings, candidate: &ExtractedModel) -> Option<String> {
    let selection = settings.model_selection(&candidate.agent_id);
    selection.profiles.keys().find_map(|profile_id| {
        settings
            .model_profiles
            .as_ref()
            .and_then(|profiles| profiles.get(profile_id))
            .and_then(|profile| {
                profile_owns_candidate(settings, profile, candidate).then(|| profile_id.clone())
            })
    })
}

fn profile_owns_candidate(
    settings: &Settings,
    profile: &ModelProfile,
    candidate: &ExtractedModel,
) -> bool {
    let explicit_native_id = profile.native_ids.get(&candidate.agent_id);
    let expected_native_id = explicit_native_id
        .cloned()
        .unwrap_or_else(|| generated_native_id(settings, &candidate.agent_id, profile));
    if expected_native_id != candidate.native_id {
        return false;
    }
    // Pi Provider names are shared by every Arca Model under the same Provider,
    // while adopted provider containers may also hold several models. In both
    // cases the provider id alone is not asset identity; the model id completes it.
    if matches!(candidate.agent_id.as_str(), "pi" | "zcode") {
        return profile.model == candidate.model;
    }
    explicit_native_id.is_none()
        || !matches!(
            candidate.agent_id.as_str(),
            "pi" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qoder-desktop" | "qoder-cli" | "zcode" | "crush" | "goose"
        )
        || profile.model == candidate.model
}

fn generated_native_id(settings: &Settings, agent_id: &str, profile: &ModelProfile) -> String {
    match agent_id {
        "claude-code" => "claude-settings".into(),
        "qoder-cli" | "opencode-desktop" | "kilo-desktop" => crate::resources::model::adapters::native_provider_id(agent_id, profile),
        "pi" => crate::resources::model::generated_pi_provider_id(settings, profile),
        "qwen-code" => format!(
            "{}:{}:{}",
            if profile.protocol == ModelProtocol::AnthropicMessages {
                "anthropic"
            } else {
                "openai"
            },
            profile.model,
            profile.base_url
        ),
        "factory-droid" => format!("{}:{}", profile.model, profile.base_url),
        _ => {
            let mut id = String::from("mux_");
            for byte in profile.id.bytes() {
                id.push_str(&format!("{byte:02x}"));
            }
            id
        }
    }
}

fn managed_profile_matches(
    settings: &Settings,
    profile_id: &str,
    candidate: &ExtractedModel,
) -> Result<bool, String> {
    let Some(profile) = settings.model_profiles.as_ref().and_then(|profiles| profiles.get(profile_id)) else {
        return Ok(false);
    };
    if normalized_url(&profile.base_url) != normalized_url(&candidate.base_url)
        || profile.model != candidate.model || profile.protocol != candidate.protocol {
        return Ok(false);
    }
    Ok(managed_credential_identity(&candidate.agent_id, profile)? == candidate.credential_identity())
}

fn managed_credential_identity(agent_id: &str, profile: &ModelProfile) -> Result<String, String> {
    if matches!(agent_id, "claude-code" | "codex" | "pi") {
        return Ok(crate::resources::model::credential_snapshot(&profile.id)?
            .map(|value| format!("literal:{}", hash(&value)))
            .unwrap_or_else(|| "none".into()));
    }
    Ok(profile.env_key.as_ref().map(|key| format!("env:{key}")).unwrap_or_else(|| "none".into()))
}

fn merge_literals(
    left: Option<String>,
    right: Option<String>,
    has_command: bool,
) -> ExtractedCredential {
    match (left, right) {
        (Some(left), Some(right)) if left != right => ExtractedCredential::Conflict,
        (Some(value), _) | (_, Some(value)) => ExtractedCredential::Literal(Zeroizing::new(value)),
        (None, None) if has_command => ExtractedCredential::Command,
        _ => ExtractedCredential::None,
    }
}

fn env_or_literal(value: &str) -> (Option<String>, ExtractedCredential) {
    let env = value
        .strip_prefix("{env:")
        .and_then(|value| value.strip_suffix('}'))
        .or_else(|| {
            value
                .strip_prefix("${")
                .and_then(|value| value.strip_suffix('}'))
        })
        .or_else(|| {
            value
                .strip_prefix('$')
                .filter(|value| !value.contains(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
        });
    match env {
        Some(key) if !key.is_empty() => (
            Some(key.to_string()),
            ExtractedCredential::Env(key.to_string()),
        ),
        _ => (
            None,
            ExtractedCredential::Literal(Zeroizing::new(value.to_string())),
        ),
    }
}

fn pi_credential(value: &str) -> ExtractedCredential {
    if value.starts_with('!') {
        ExtractedCredential::Command
    } else {
        let (_, credential) = env_or_literal(value);
        credential
    }
}

fn read_jsonc(path: &Path) -> Result<Option<Value>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", display_path(path))),
    };
    let root = CstRootNode::parse(&content, &ParseOptions::default())
        .map_err(|error| format!("invalid JSON/JSONC at {}: {error}", display_path(path)))?;
    root.to_serde_value()
        .ok_or_else(|| format!("invalid JSON root at {}", display_path(path)))
        .map(Some)
}

fn read_plain_json(path: &Path) -> Result<Option<Value>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", display_path(path))),
    };
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| format!("invalid JSON at {}: {error}", display_path(path)))
}

fn read_toml_value(path: &Path) -> Result<Option<toml::Value>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", display_path(path))),
    };
    toml::from_str(&content)
        .map(Some)
        .map_err(|error| format!("invalid TOML at {}: {error}", display_path(path)))
}

fn read_yaml_value(path: &Path) -> Result<Option<Value>, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("failed to read {}: {error}", display_path(path))),
    };
    serde_yaml::from_str(&content)
        .map(Some)
        .map_err(|error| format!("invalid YAML at {}: {error}", display_path(path)))
}

fn display_path(path: &Path) -> String {
    crate::resources::mcp::scanner::collapse_home(&path.to_string_lossy())
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn toml_string(value: &toml::Value, keys: &[&str]) -> Option<String> {
    let mut current = value;
    for key in keys {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn protocol_from_name(value: &str) -> ModelProtocol {
    match value {
        "anthropic-messages" | "anthropic" | "messages" => ModelProtocol::AnthropicMessages,
        "openai-responses" | "responses" => ModelProtocol::OpenaiResponses,
        "gemini-generate-content" | "gemini" | "google" | "generate-content" => {
            ModelProtocol::GeminiGenerateContent
        }
        _ => ModelProtocol::OpenaiCompletions,
    }
}

fn protocol_from_backend(value: &str) -> ModelProtocol {
    protocol_from_name(value)
}

fn protocol_name(protocol: &ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::AnthropicMessages => "anthropic-messages",
        ModelProtocol::OpenaiResponses => "openai-responses",
        ModelProtocol::OpenaiCompletions => "openai-completions",
        ModelProtocol::GeminiGenerateContent => "gemini-generate-content",
    }
}

fn normalized_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_ascii_lowercase()
}

fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hash_fields(fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        digest.update((field.len() as u64).to_be_bytes());
        digest.update(field);
    }
    hex::encode(digest.finalize())
}

fn hash_optional(bytes: Option<&[u8]>) -> String {
    match bytes {
        Some(bytes) => hash_fields(&[b"present", bytes]),
        None => hash_fields(&[b"absent"]),
    }
}

fn hash_target_paths(paths: &[PathBuf]) -> String {
    let mut material = Vec::new();
    let mut paths = paths.to_vec();
    paths.sort();
    paths.dedup();
    for path in paths {
        material.extend_from_slice(path.to_string_lossy().as_bytes());
        material.push(0);
        material.extend_from_slice(hash_optional(fs::read(path).ok().as_deref()).as_bytes());
        material.push(0xff);
    }
    hash(&material)
}

fn hash_serializable<T: Serialize>(value: &T) -> Result<String, String> {
    super::payload_hash::hash(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{commit_asset_operation, AssetCommitRequest};
    use crate::testenv::TestHome;

    #[test]
    fn qoder_cli_observes_selection_and_matches_its_distinct_native_identity() {
        let home = TestHome::new("qoder-cli-discovery");
        let path = home.home.join("settings.json");
        fs::write(&path, r#"{"providers":{"mux_cli_66697874757265":{"baseUrl":"https://example.invalid/v1","apiKey":"${QODER_FIXTURE_KEY}","protocol":"openai-responses","models":[{"model":"vendor/model"}]}},"model":{"name":"mux_cli_66697874757265/vendor/model"}}"#).unwrap();
        let rows = extract_qoder("qoder-cli", &path).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].active);
        assert_eq!(rows[0].status().0, ModelAdoptionStatus::Adoptable);
        let mut profile = rows[0].profile();
        profile.id = "fixture".into();
        profile.native_ids.clear();
        assert!(profile_owns_candidate(&Settings::default(), &profile, &rows[0]));
        let desktop = extract_qoder("qoder-desktop", &path).unwrap();
        assert!(!desktop[0].active);
        assert_eq!(desktop[0].status().0, ModelAdoptionStatus::Unsupported);
        assert!(!profile_owns_candidate(&Settings::default(), &profile, &desktop[0]));
    }

    #[test]
    fn qoder_cli_import_preserves_shared_native_config_and_waits_for_explicit_add() {
        let home = TestHome::new("qoder-cli-import");
        let path = home.home.join(".qoder/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"providers":{"external":{"baseUrl":"https://example.invalid/v1","apiKey":"${QODER_FIXTURE_KEY}","models":[{"model":"model"}]}},"model":{"name":"external/model"}}"#;
        fs::write(&path, original).unwrap();
        let candidate = list_model_adoption_candidates().unwrap().into_iter()
            .find(|candidate| candidate.agent_id == "qoder-cli").unwrap();
        let plan = plan_model_adoption(PlanModelAdoptionRequest {
            candidate_fingerprints: BTreeMap::from([(candidate.candidate_id, candidate.fingerprint)]),
        }).unwrap();
        commit_asset_operation(AssetCommitRequest { operation_id: plan.operation_id, candidate_hash: plan.candidate_hash }).unwrap();
        let settings = load_settings_strict().unwrap();
        assert!(settings.model_selection("qoder-cli").profiles.is_empty());
        assert!(!settings.model_profiles.unwrap().values().next().unwrap().native_ids.contains_key("qoder-cli"));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    fn write_grok(home: &TestHome, credential_line: &str) {
        let path = home.home.join(".grok/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                r#"[models]
default = "legacy"

[model.legacy]
name = "OpenRouter HY3"
model = "tencent/hy3:free"
base_url = "https://openrouter.ai/api/v1"
api_backend = "chat_completions"
{credential_line}
"#,
            ),
        )
        .unwrap();
    }

    #[test]
    fn zcode_shared_provider_models_are_importable_and_secret_free() {
        let home = TestHome::new("model-adoption-zcode-shared");
        let path = home.home.join(".zcode/v2/config.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&serde_json::json!({
            "provider": {
                "external": {
                    "name": "External provider",
                    "source": "custom",
                    "kind": "openai-compatible",
                    "enabled": false,
                    "options": {"baseURL": "https://example.invalid/v1", "apiKey": "synthetic-zcode-test-key"},
                    "models": {
                        "first": {"limit": {"context": 100000, "output": 8000}, "reasoning": {"enabled": true}},
                        "second": {}
                    }
                },
                "builtin:example": {
                    "source": "custom", "kind": "openai-compatible", "models": {}
                },
                "unsupported": {"source": "custom", "kind": "anthropic", "models": {}}
            }
        })).unwrap()).unwrap();
        let mut extracted = extract_zcode(&path).unwrap();
        mark_shared_native_provider_models(&mut extracted);
        assert_eq!(extracted.len(), 2);
        assert!(extracted.iter().all(|row| !row.active));
        assert!(extracted.iter().all(|row| row.enabled == Some(false)));
        let mut enabled_copy = extracted[0].clone();
        enabled_copy.enabled = Some(true);
        assert_ne!(extracted[0].fingerprint(), enabled_copy.fingerprint());
        assert!(extracted.iter().all(|row| row.status().0 == ModelAdoptionStatus::Adoptable));
        let first = extracted.iter().find(|row| row.model == "first").unwrap();
        assert_eq!(first.context_window, Some(100000));
        assert_eq!(first.max_output_tokens, Some(8000));
        assert_eq!(first.reasoning, Some(true));
        let candidates = list_model_adoption_candidates().unwrap();
        assert_eq!(candidates.iter().filter(|row| row.agent_id == "zcode").count(), 2);
        assert!(candidates.iter().filter(|row| row.agent_id == "zcode").all(|row| row.enabled == Some(false)));
        assert!(!serde_json::to_string(&candidates).unwrap().contains("synthetic-zcode-test-key"));
        let candidate = candidates.iter().find(|row| row.agent_id == "zcode").unwrap();
        let original = fs::read(&path).unwrap();
        let plan = plan_model_adoption(PlanModelAdoptionRequest {
            candidate_fingerprints: BTreeMap::from([(
                candidate.candidate_id.clone(), candidate.fingerprint.clone(),
            )]),
        }).unwrap();
        let DomainPlan::Model { before, after } = &plan.domain_plan else {
            panic!("expected model plan");
        };
        assert_eq!(before, after);
        assert!(after.get("zcode").unwrap().profiles.is_empty());
        assert!(plan.central_changes[0].summary.iter().any(|line| line.contains("仅导入中央模型库")));
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn discovery_is_secret_free_and_prioritizes_active_candidates() {
        let home = TestHome::new("model-adoption-discovery");
        write_grok(&home, "api_key = \"do-not-serialize-this\"");

        let candidates = list_model_adoption_candidates().unwrap();
        let grok = candidates
            .iter()
            .find(|candidate| candidate.agent_id == "grok-build")
            .unwrap();
        assert!(grok.active);
        assert_eq!(grok.provider, "openrouter");
        assert_eq!(grok.status, ModelAdoptionStatus::NeedsCredential);
        let serialized = serde_json::to_string(&candidates).unwrap();
        assert!(!serialized.contains("do-not-serialize-this"));
    }

    #[test]
    fn environment_reference_model_is_adopted_with_native_identity() {
        let home = TestHome::new("model-adoption-commit");
        write_grok(&home, "env_key = \"OPENROUTER_API_KEY\"");
        let candidate = list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.agent_id == "grok-build")
            .unwrap();
        assert_eq!(candidate.status, ModelAdoptionStatus::Adoptable);
        let plan = plan_model_adoption(PlanModelAdoptionRequest {
            candidate_fingerprints: BTreeMap::from([(
                candidate.candidate_id.clone(),
                candidate.fingerprint,
            )]),
        })
        .unwrap();
        assert_eq!(plan.kind, AssetOperationKind::Adopt);
        assert_eq!(plan.model_state_changes.len(), 1);
        assert_eq!(
            plan.central_changes[0].summary,
            vec![
                "从 Agent 当前配置创建中央 Model Profile",
                "使用 openrouter / tencent/hy3:free",
                "关联 1 个 Agent 中的现有模型配置",
            ]
        );
        commit_asset_operation(AssetCommitRequest {
            operation_id: plan.operation_id,
            candidate_hash: plan.candidate_hash,
        })
        .unwrap();

        let settings = load_settings_strict().unwrap();
        let profile = settings
            .model_profiles
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap();
        assert_eq!(
            profile.native_ids.get("grok-build").map(String::as_str),
            Some("legacy")
        );
        assert_eq!(
            settings
                .model_selection("grok-build")
                .active_profile_id
                .as_deref(),
            Some(profile.id.as_str())
        );
        let written = fs::read_to_string(home.home.join(".grok/config.toml")).unwrap();
        assert!(written.contains("[model.legacy]"));
        assert!(written.contains("env_key = \"OPENROUTER_API_KEY\""));
        assert!(list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .all(|candidate| candidate.agent_id != "grok-build"));
    }

    #[test]
    fn shared_native_provider_keeps_only_the_active_model_adoptable() {
        let home = TestHome::new("model-adoption-shared-provider");
        let path = home.home.join(".config/opencode/opencode.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            r#"{
  "model": "legacy/active",
  "provider": {
    "legacy": {
      "npm": "@ai-sdk/openai-compatible",
      "options": {"baseURL": "https://openrouter.ai/api/v1", "apiKey": "{env:OPENROUTER_API_KEY}"},
      "models": {"active": {"name": "Active"}, "inactive": {"name": "Inactive"}}
    }
  }
}"#,
        )
        .unwrap();

        let rows = list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.agent_id == "opencode")
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .filter(|candidate| candidate.status == ModelAdoptionStatus::Adoptable)
                .count(),
            1
        );
        assert!(rows
            .iter()
            .find(|candidate| candidate.model == "active")
            .is_some_and(|candidate| candidate.status == ModelAdoptionStatus::Adoptable));
    }

    #[test]
    fn qwen_discovery_accepts_stable_arrays_and_only_the_exact_legacy_wrapper() {
        let home = TestHome::new("model-adoption-qwen-stable");
        let path = home.home.join(".qwen/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "modelProviders": {
    "openai": [
      {"id":"stable-model","name":"Stable","baseUrl":"https://gateway.example/v1","envKey":"GATEWAY_KEY"}
    ]
  },
  "model": {"name":"stable-model"},
  "security": {"auth":{"selectedType":"openai"}}
}"#,
        )
        .unwrap();

        let candidate = list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.agent_id == "qwen-code")
            .unwrap();
        assert_eq!(candidate.model, "stable-model");
        assert!(candidate.active);
        assert_eq!(candidate.status, ModelAdoptionStatus::Adoptable);

        let exact_legacy = serde_json::json!({
            "protocol": "openai",
            "models": [{"id": "legacy"}]
        });
        assert_eq!(
            qwen_provider_models_for_discovery(&exact_legacy, "openai")
                .unwrap()
                .len(),
            1
        );
        let extended_legacy = serde_json::json!({
            "protocol": "openai",
            "models": [],
            "future": true
        });
        assert!(qwen_provider_models_for_discovery(&extended_legacy, "openai").is_err());
    }

    #[test]
    fn every_managed_model_importer_has_an_adoptable_fixture() {
        let home = TestHome::new("model-adoption-all-managed-agents");
        let root = home.home.join("fixtures");
        let write = |path: &Path, content: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        };

        let claude = root.join("claude.json");
        write(
            &claude,
            r#"{"model":"claude-test","env":{"ANTHROPIC_BASE_URL":"https://gateway.example/anthropic","ANTHROPIC_AUTH_TOKEN":"test-secret"}}"#,
        );
        let codex = root.join("codex.toml");
        write(
            &codex,
            r#"model = "gpt-test"
model_provider = "legacy"
[model_providers.legacy]
name = "Legacy"
base_url = "https://gateway.example/v1"
wire_api = "responses"
experimental_bearer_token = "test-secret"
"#,
        );
        let grok = root.join("grok.toml");
        write(
            &grok,
            r#"[models]
default = "legacy"
[model.legacy]
model = "vendor/grok-test"
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "GATEWAY_KEY"
"#,
        );
        let pi_models = root.join("pi-models.json");
        let pi_settings = root.join("pi-settings.json");
        write(
            &pi_models,
            r#"{"providers":{"legacy":{"baseUrl":"https://gateway.example/v1","api":"openai-completions","apiKey":"test-secret","models":[{"id":"pi-test","name":"Pi Test"}]}}}"#,
        );
        write(
            &pi_settings,
            r#"{"defaultProvider":"legacy","defaultModel":"pi-test"}"#,
        );
        let open_code = root.join("opencode.json");
        write(
            &open_code,
            r#"{"model":"legacy/open-test","provider":{"legacy":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://gateway.example/v1","apiKey":"{env:GATEWAY_KEY}"},"models":{"open-test":{"name":"Open Test"}}}}}"#,
        );
        let kilo = root.join("kilo.jsonc");
        write(
            &kilo,
            r#"{"model":"legacy/kilo-test","provider":{"legacy":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://gateway.example/v1","apiKey":"{env:GATEWAY_KEY}"},"models":{"kilo-test":{"name":"Kilo Test"}}}}}"#,
        );
        let qwen = root.join("qwen.json");
        write(
            &qwen,
            r#"{"modelProviders":{"openai":[{"id":"qwen-test","name":"Qwen Test","baseUrl":"https://gateway.example/v1","envKey":"GATEWAY_KEY"}]},"model":{"name":"qwen-test"},"security":{"auth":{"selectedType":"openai"}}}"#,
        );
        let crush = root.join("crush.json");
        write(
            &crush,
            r#"{"models":{"large":{"provider":"legacy","model":"crush-test"}},"providers":{"legacy":{"base_url":"https://gateway.example/v1","api_key":"$GATEWAY_KEY","models":[{"id":"crush-test","name":"Crush Test"}]}}}"#,
        );
        let vibe = root.join("vibe.toml");
        write(
            &vibe,
            r#"active_model = "legacy"
[[providers]]
name = "legacy"
api_base = "https://gateway.example/v1"
api_key_env_var = "GATEWAY_KEY"
[[models]]
name = "vibe-test"
alias = "legacy"
provider = "legacy"
"#,
        );
        let hermes = root.join("hermes.yaml");
        write(
            &hermes,
            r#"model:
  provider: custom:legacy
  default: hermes-test
providers:
  legacy:
    name: Legacy
    api: https://gateway.example/v1
    key_env: GATEWAY_KEY
    default_model: hermes-test
"#,
        );
        let factory = root.join("factory.json");
        write(
            &factory,
            r#"{"model":"factory-test","customModels":[{"model":"factory-test","displayName":"Factory Test","baseUrl":"https://gateway.example/v1","provider":"openai-compatible","apiKey":"${GATEWAY_KEY}"}]}"#,
        );
        let goose = root.join("goose/config.yaml");
        let goose_provider = root.join("goose/custom_providers/legacy.json");
        write(
            &goose,
            r#"active_provider: legacy
providers:
  legacy:
    configured: true
    model: goose-test
"#,
        );
        write(
            &goose_provider,
            r#"{"display_name":"Legacy","engine":"openai","base_url":"https://gateway.example/v1","api_key_env":"GATEWAY_KEY","models":[{"name":"goose-test","context_limit":128000}]}"#,
        );

        let fixtures = vec![
            ("claude-code", extract_claude(&claude).unwrap()),
            ("codex", extract_codex(&codex).unwrap()),
            ("grok-build", extract_grok(&grok).unwrap()),
            ("pi", extract_pi(&pi_models, &pi_settings).unwrap()),
            (
                "opencode",
                extract_open_code("opencode", &open_code).unwrap(),
            ),
            ("kilo-code", extract_open_code("kilo-code", &kilo).unwrap()),
            ("qwen-code", extract_qwen(&qwen).unwrap()),
            ("crush", extract_crush(&crush).unwrap()),
            ("mistral-vibe", extract_vibe(&vibe).unwrap()),
            ("hermes", extract_hermes(&hermes).unwrap()),
            ("factory-droid", extract_factory(&factory).unwrap()),
            ("goose", extract_goose(&goose).unwrap()),
        ];
        for (agent_id, rows) in fixtures {
            assert!(
                rows.iter()
                    .any(|row| row.status().0 == ModelAdoptionStatus::Adoptable),
                "{agent_id} lacks an adoptable fixture"
            );
        }
    }

    #[test]
    fn discovery_errors_do_not_expose_the_private_home_path() {
        let home = TestHome::new("model-adoption-path-free-errors");
        let path = home.home.join(".qwen/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{").unwrap();

        let candidate = list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.agent_id == "qwen-code")
            .unwrap();
        let reason = candidate.reason.unwrap();
        assert!(reason.contains("~/.qwen/settings.json"));
        assert!(!reason.contains(home.home.to_string_lossy().as_ref()));
    }

    #[test]
    fn sequential_batch_adoption_tolerates_settings_changes_from_prior_items() {
        let home = TestHome::new("model-adoption-sequential");
        let path = home.home.join(".grok/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"[models]
default = "first"

[model.first]
model = "vendor/first"
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "GATEWAY_KEY"

[model.second]
model = "vendor/second"
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "GATEWAY_KEY"
"#,
        )
        .unwrap();
        let original = list_model_adoption_candidates()
            .unwrap()
            .into_iter()
            .filter(|candidate| candidate.agent_id == "grok-build")
            .collect::<Vec<_>>();
        assert_eq!(original.len(), 2);
        for candidate in original {
            let plan = plan_model_adoption(PlanModelAdoptionRequest {
                candidate_fingerprints: BTreeMap::from([(
                    candidate.candidate_id,
                    candidate.fingerprint,
                )]),
            })
            .unwrap();
            commit_asset_operation(AssetCommitRequest {
                operation_id: plan.operation_id,
                candidate_hash: plan.candidate_hash,
            })
            .unwrap();
        }
        let settings = load_settings_strict().unwrap();
        assert_eq!(settings.model_profiles.as_ref().unwrap().len(), 2);
        assert_eq!(settings.model_selection("grok-build").profiles.len(), 2);
    }
    #[test]
    fn opencode_desktop_import_does_not_adopt_a_shared_cli_provider() {
        let home = TestHome::new("opencode-desktop-import");
        let path = home.home.join(".config/opencode/opencode.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"provider":{"external":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://example.invalid/v1","apiKey":"{env:OPENCODE_FIXTURE_KEY}"},"models":{"model":{"name":"Example"}}}},"model":"external/model"}"#;
        fs::write(&path, original).unwrap();
        let candidate = list_model_adoption_candidates().unwrap().into_iter()
            .find(|candidate| candidate.agent_id == "opencode-desktop").unwrap();
        let plan = plan_model_adoption(PlanModelAdoptionRequest {
            candidate_fingerprints: BTreeMap::from([(candidate.candidate_id, candidate.fingerprint)]),
        }).unwrap();
        commit_asset_operation(AssetCommitRequest { operation_id: plan.operation_id, candidate_hash: plan.candidate_hash }).unwrap();
        let settings = load_settings_strict().unwrap();
        assert!(settings.model_selection("opencode-desktop").profiles.is_empty());
        assert!(!settings.model_profiles.unwrap().values().next().unwrap().native_ids.contains_key("opencode-desktop"));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn kilo_desktop_import_keeps_native_providers_and_does_not_assign_a_consumer() {
        let _home = TestHome::new("kilo-desktop-import");
        let path = expand_tilde(&crate::resources::model::default_config_paths("kilo-desktop").unwrap()[0]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"provider":{"external":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"https://example.invalid/v1","apiKey":"{env:KILO_FIXTURE_KEY}"},"models":{"model":{"name":"Example"}}}}}"#;
        fs::write(&path, original).unwrap();
        let candidate = list_model_adoption_candidates().unwrap().into_iter()
            .find(|candidate| candidate.agent_id == "kilo-desktop").unwrap();
        let plan = plan_model_adoption(PlanModelAdoptionRequest {
            candidate_fingerprints: BTreeMap::from([(candidate.candidate_id, candidate.fingerprint)]),
        }).unwrap();
        commit_asset_operation(AssetCommitRequest { operation_id: plan.operation_id, candidate_hash: plan.candidate_hash }).unwrap();
        let settings = load_settings_strict().unwrap();
        assert!(settings.model_selection("kilo-desktop").profiles.is_empty());
        assert!(!settings.model_profiles.unwrap().values().next().unwrap().native_ids.contains_key("kilo-desktop"));
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

}
