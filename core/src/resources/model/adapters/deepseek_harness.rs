//! DeepSeek Harness 0.2.2-rc.1 profile-scoped model overrides.
//!
//! Cordis replaces an entry's complete config. The shared patch editor and
//! inheritance guard therefore own structure; this adapter changes only one
//! provider route and the native default-model pointer. Credentials remain
//! references handled by Harness, never copies of its credential document.

use super::*;
use crate::resources::deepseek_harness::{guard_profile, PatchFile};

const PROVIDERS: &str = "llm-pi-ai";
const DEFAULT_MODEL: &str = "agent-default-model";

fn guarded(path: &Path) -> Result<PatchFile, String> {
    guard_profile(path, &[PROVIDERS, DEFAULT_MODEL])?;
    PatchFile::read(path)
}

fn config_or_empty(patch: &PatchFile, id: &str) -> Result<Value, String> {
    Ok(patch.config(id)?.unwrap_or_else(|| json!({})))
}

/// Read only the two model-owned configuration entries. Never open the
/// Harness credential store or evaluate Cordis expressions.
pub(crate) fn read_registry(path: &Path) -> Result<Option<Value>, String> {
    let patch = PatchFile::read(path)?;
    if patch.original().is_none() { return Ok(None); }
    let routes = config_or_empty(&patch, PROVIDERS)?;
    let default = config_or_empty(&patch, DEFAULT_MODEL)?;
    let providers = match routes.get("providers") {
        None => json!({}),
        Some(value) if value.is_object() => value.clone(),
        Some(_) => return Err("deepseek_harness_model_conflicted: providers must be a mapping".into()),
    };
    Ok(Some(json!({"providers": providers, "default": default})))
}

fn protocol(profile: &ModelProfile) -> Result<&'static str, String> {
    match profile.protocol {
        ModelProtocol::OpenaiCompletions => Ok("openai-completions"),
        ModelProtocol::OpenaiResponses => Ok("openai-responses"),
        ModelProtocol::AnthropicMessages => Ok("anthropic-messages"),
        _ => Err("deepseek_harness_protocol_unsupported: custom routes support Chat Completions, Responses and Anthropic Messages".into()),
    }
}

fn provider_identity(agent_id: &str, profile: &ModelProfile) -> Result<String, String> {
    let id = provider_id_for(agent_id, profile);
    // Preserve adopted native map identities literally. Generated identities
    // use a bounded Arca namespace, never a built-in catalog/account route.
    if !safe_provider_id(&id) {
        return Err("deepseek_harness_provider_id_invalid: unsafe native provider identity".into());
    }
    Ok(id)
}

fn models_for_route(value: Option<&Value>) -> Result<Vec<Value>, String> {
    let Some(value) = value else { return Ok(Vec::new()); };
    let models = value.as_array().ok_or("deepseek_harness_model_conflicted: models must be an array")?;
    let mut ids = BTreeSet::new();
    for model in models {
        let id = model.get("id").and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("deepseek_harness_model_conflicted: model id is missing")?;
        if !model.is_object() || !ids.insert(id) {
            return Err("deepseek_harness_model_conflicted: duplicate or invalid model entry".into());
        }
    }
    Ok(models.clone())
}

fn prepared(path: &Path, patch: PatchFile) -> Result<PreparedModelFile, String> {
    let original = patch.original().map(str::to_string);
    let content = patch.render()?;
    Ok(PreparedModelFile { path: path.into(), original, content: Some(content) })
}

pub(super) fn prepare(
    agent_id: &str, path: &Path, profile: &ModelProfile, active: bool,
) -> Result<PreparedModelFile, String> {
    let api = protocol(profile)?;
    let route = provider_identity(agent_id, profile)?;
    let env = profile.env_key.as_deref().filter(|key| !key.is_empty())
        .ok_or("deepseek_harness_env_reference_required: configure a Harness credential/environment reference; Arca does not export Keychain secrets to .credentials.yaml")?;
    let mut patch = guarded(path)?;
    let config = config_or_empty(&patch, PROVIDERS)?;
    let providers = config.get("providers");
    if providers.is_some_and(|value| !value.is_object()) {
        return Err("deepseek_harness_model_conflicted: providers must be a mapping".into());
    }
    let existing = providers.and_then(|value| value.get(&route));
    if existing.is_some_and(|value| !value.is_object()) {
        return Err("deepseek_harness_model_conflicted: provider must be a mapping".into());
    }
    let models = models_for_route(existing.and_then(|value| value.get("models")))?;
    if models.len() > 1 {
        return Err("deepseek_harness_shared_provider_read_only: this provider serves sibling models; split it in Harness before adoption".into());
    }
    if profile.native_ids.contains_key(agent_id)
        && models.first().is_some_and(|model| model.get("id") != Some(&json!(profile.model))) {
        return Err("deepseek_harness_model_identity_changed: adopted provider belongs to another model".into());
    }
    if let Some(existing) = existing {
        let has_compat = |value: Option<&Value>| value.is_some_and(|value|
            value.as_object().is_none_or(|fields| !fields.is_empty()));
        if existing.get("api").and_then(Value::as_str).is_some_and(|previous| previous != api)
            && (has_compat(existing.get("compat")) || has_compat(models.first().and_then(|model| model.get("compat")))) {
            return Err("deepseek_harness_native_policy_conflicted: changing API protocol would invalidate unowned native compatibility policy".into());
        }
        if profile.reasoning == Some(false)
            && existing.get("reasoning").and_then(Value::as_str).is_some_and(|effort| effort != "off") {
            return Err("deepseek_harness_native_policy_conflicted: native provider reasoning conflicts with this non-reasoning model".into());
        }
    }
    for (field, value) in [
        ("api", json!(api)),
        ("baseURL", json!(profile.base_url)),
        ("apiKeyEnv", json!(env)),
        ("displayName", json!(provider_display_name(profile))),
    ] {
        patch.set_config_path(PROVIDERS, &["providers", &route, field], &value)?;
    }
    if models.is_empty() {
        patch.set_config_path(PROVIDERS, &["providers", &route, "models"], &json!([{}]))?;
    }
    for (field, value) in [("id", json!(profile.model)), ("name", json!(profile.name))] {
        patch.set_config_path(PROVIDERS, &["providers", &route, "models", "0", field], &value)?;
    }
    for (field, value) in [("contextWindow", profile.context_window), ("maxTokens", profile.max_output_tokens)] {
        let keys = ["providers", &route, "models", "0", field];
        if let Some(value) = value {
            patch.set_config_path(PROVIDERS, &keys, &json!(value))?;
        } else {
            patch.remove_config_path(PROVIDERS, &keys)?;
        }
    }
    if let Some(reasoning) = profile.reasoning {
        let keys = ["providers", &route, "models", "0", "reasoningEfforts"];
        if !reasoning {
            patch.set_config_path(PROVIDERS, &keys, &Value::Bool(false))?;
        } else if !models.first().and_then(|value| value.get("reasoningEfforts")).is_some_and(Value::is_object) {
            // The central boolean declares reasoning capability. Preserve a
            // native explicit wire map; otherwise expose one standard level.
            patch.set_config_path(PROVIDERS, &keys, &json!({"off": null, "high": "high"}))?;
        }
    }
    if active {
        patch.set_config_path(DEFAULT_MODEL, &["provider"], &json!(route))?;
        patch.set_config_path(DEFAULT_MODEL, &["model"], &json!(profile.model))?;
        patch.remove_config_path(DEFAULT_MODEL, &["reasoningEffort"])?;
    }
    prepared(path, patch)
}

fn clear_default_if(
    patch: &mut PatchFile, removed: &BTreeSet<String>, model: Option<&str>,
) -> Result<(), String> {
    let Some(config) = patch.config(DEFAULT_MODEL)? else { return Ok(()); };
    let current = config.get("provider").and_then(Value::as_str);
    if !current.is_some_and(|id| removed.contains(id))
        || model.is_some_and(|model| config.get("model").and_then(Value::as_str) != Some(model)) {
        return Ok(());
    }
    if config.as_object().is_none_or(|object| object.keys().any(|key|
        !matches!(key.as_str(), "provider" | "model" | "reasoningEffort"))) {
        return Err("deepseek_harness_default_policy_conflicted: removing this default would discard unowned policy; choose another native default first".into());
    }
    // Removing both required fields but retaining config:{} would break boot.
    // Removing the override restores the selected bundle's native default.
    patch.remove_config_path(DEFAULT_MODEL, &[])
}

pub(super) fn clear(
    agent_id: &str, path: &Path, profile: &ModelProfile,
) -> Result<PreparedModelFile, String> {
    if PatchFile::read(path)?.original().is_none() {
        return Ok(PreparedModelFile { path: path.into(), original: None, content: None });
    }
    let mut patch = guarded(path)?;
    let route = provider_identity(agent_id, profile)?;
    let config = config_or_empty(&patch, PROVIDERS)?;
    let provider = config.get("providers").and_then(|providers| providers.get(&route));
    if let Some(provider) = provider {
        let models = models_for_route(provider.get("models"))?;
        if models.len() > 1 || models.first().is_some_and(|model| model.get("id") != Some(&json!(profile.model))) {
            return Err("deepseek_harness_shared_provider_read_only: clear cannot remove a sibling model".into());
        }
        patch.remove_config_path(PROVIDERS, &["providers", &route])?;
    }
    clear_default_if(&mut patch, &BTreeSet::from([route]), Some(&profile.model))?;
    prepared(path, patch)
}

pub(super) fn clear_all(path: &Path) -> Result<PreparedModelFile, String> {
    if PatchFile::read(path)?.original().is_none() {
        return Ok(PreparedModelFile { path: path.into(), original: None, content: None });
    }
    let mut patch = guarded(path)?;
    let config = config_or_empty(&patch, PROVIDERS)?;
    let Some(providers) = config.get("providers") else { return prepared(path, patch); };
    let providers = providers.as_object().ok_or("deepseek_harness_model_conflicted: providers must be a mapping")?;
    for provider in providers.values() {
        if !provider.is_object() || models_for_route(provider.get("models"))?.is_empty()
            || !matches!(provider.get("api").and_then(Value::as_str),
                Some("openai-completions" | "openai-responses" | "anthropic-messages")) {
            return Err("deepseek_harness_catalog_read_only: cannot clear an unobserved inherited catalog or unsupported provider; use Harness for this route".into());
        }
    }
    let ids = providers.keys().cloned().collect::<BTreeSet<_>>();
    if ids.is_empty() { return prepared(path, patch); }
    // Keep the complete providers override empty. Removing it could restore
    // a bundle provider which the reviewed operation intended to clear.
    for id in &ids { patch.remove_config_path(PROVIDERS, &["providers", id])?; }
    clear_default_if(&mut patch, &ids, None)?;
    prepared(path, patch)
}

pub(super) fn observe_external(path: &Path) -> crate::resources::model::ExternalModelObservedState {
    use crate::resources::model::ExternalModelObservedState::{Absent, Conflicted, Present};
    if matches!(read_registry(path), Ok(None)) { return Absent; }
    if guard_profile(path, &[PROVIDERS, DEFAULT_MODEL]).is_err() { return Conflicted; }
    match read_registry(path) {
        Err(_) => Conflicted,
        Ok(None) => Absent,
        Ok(Some(root)) => if root["providers"].as_object().is_some_and(|providers| !providers.is_empty())
            || root["default"].get("provider").is_some() { Present } else { Absent },
    }
}

pub(super) fn observe_active(
    agent_id: &str, path: &Path, profiles: &std::collections::BTreeMap<String, ModelProfile>,
) -> ObservedActiveModel {
    let root = match read_registry(path) {
        Err(_) => return ObservedActiveModel::Conflicted,
        Ok(None) => return ObservedActiveModel::None,
        Ok(Some(root)) => root,
    };
    if guard_profile(path, &[PROVIDERS, DEFAULT_MODEL]).is_err() { return ObservedActiveModel::Conflicted; }
    let (Some(provider), Some(model)) = (root["default"].get("provider").and_then(Value::as_str), root["default"].get("model").and_then(Value::as_str)) else {
        return ObservedActiveModel::None;
    };
    let matches = profiles.values().filter(|profile|
        provider_id_for(agent_id, profile) == provider && profile.model == model)
        .map(|profile| profile.id.clone()).collect::<Vec<_>>();
    match matches.as_slice() {
        [id] => ObservedActiveModel::Managed(id.clone()),
        [] => ObservedActiveModel::External,
        _ => ObservedActiveModel::Conflicted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testenv::TestHome;

    fn profile() -> ModelProfile {
        ModelProfile { id: "work".into(), provider_id: None, name: "Work model".into(),
            provider: "custom".into(), model_vendor: None, native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses, base_url: "https://gateway.example/v1".into(),
            endpoint_path: String::new(), model: "reasoner".into(), env_key: Some("WORK_MODEL_KEY".into()),
            context_window: Some(100_000), max_output_tokens: Some(4096), reasoning: Some(false) }
    }

    fn initialized(home: &TestHome) -> PathBuf {
        let directory = home.home.join(".dsh/profiles/web");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("package.json"), r#"{"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app"]}}}"#).unwrap();
        directory.join("cordis.patch.yml")
    }

    #[test]
    fn multi_models_preserve_mcp_policy_and_clear_restores_inheritance() {
        let home = TestHome::new("dsh-model-native");
        let path = initialized(&home);
        let original = "# user layer\n- id: mcp-manager\n  config:\n    servers: {} # keep MCP policy\n- id: unrelated\n  disabled: !!js process.env.SOMETHING\n";
        fs::write(&path, original).unwrap();
        let first = profile();
        let file = prepare("deepseek-harness", &path, &first, true).unwrap();
        let content = file.content.as_ref().unwrap();
        assert!(content.contains("# keep MCP policy"));
        assert!(content.contains("!!js process.env.SOMETHING"));
        assert!(content.contains("WORK_MODEL_KEY"));
        fs::write(&path, content).unwrap();
        let mut second = first.clone(); second.id = "backup".into(); second.model = "backup-model".into();
        let content = prepare("deepseek-harness", &path, &second, false).unwrap().content.unwrap();
        fs::write(&path, content).unwrap();
        let root = read_registry(&path).unwrap().unwrap();
        assert_eq!(root["providers"].as_object().unwrap().len(), 2);
        assert_eq!(root["default"]["model"], first.model);
        let unchanged = prepare("deepseek-harness", &path, &first, true).unwrap();
        assert_eq!(unchanged.original, unchanged.content);
        let content = clear("deepseek-harness", &path, &first).unwrap().content.unwrap();
        fs::write(&path, content).unwrap();
        let root = read_registry(&path).unwrap().unwrap();
        assert_eq!(root["providers"].as_object().unwrap().len(), 1);
        assert!(root["default"].get("provider").is_none());
        assert!(root["default"].get("model").is_none());
        assert!(fs::read_to_string(&path).unwrap().contains("# keep MCP policy"));
        assert!(!home.home.join(".dsh/.credentials.yaml").exists());
    }

    #[test]
    fn shared_routes_and_keychain_only_profiles_are_rejected_without_writes() {
        let home = TestHome::new("dsh-model-guards");
        let path = initialized(&home);
        let mut managed = profile();
        managed.native_ids.insert("deepseek-harness".into(), "external".into());
        let original = "- id: llm-pi-ai\n  config:\n    providers:\n      external:\n        api: openai-responses\n        models: [{id: reasoner}, {id: sibling}]\n";
        fs::write(&path, original).unwrap();
        assert!(prepare("deepseek-harness", &path, &managed, true).is_err());
        assert!(clear("deepseek-harness", &path, &managed).is_err());
        managed.env_key = None;
        assert!(prepare("deepseek-harness", &path, &managed, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
}
