//! Qoder Desktop 0.1.8 and CLI 1.1.50 native custom-endpoint registry.
//! See docs/qoder-desktop-models.md and docs/qoder-cli-models.md for the audits.

use super::*;

#[cfg(test)]
const AGENT: &str = "qoder-desktop";

pub(crate) fn read_registry(path: &Path) -> Result<Option<Value>, String> {
    let (root, original) = read_jsonc(path)
        .map_err(|_| "qoder_config_conflicted: invalid JSON/JSONC".to_string())?;
    if original.is_none() { return Ok(None); }
    fn validate(node: jsonc_parser::cst::CstNode, path: &Path) -> Result<(), String> {
        if let Some(object) = node.as_object() {
            ensure_unique(&object, path, "Qoder config")?;
            for property in object.properties() {
                if let Some(value) = property.value() { validate(value, path)?; }
            }
        } else if let Some(array) = node.as_array() {
            for value in array.elements() { validate(value, path)?; }
        }
        Ok(())
    }
    let top = root.object_value().ok_or("qoder_config_conflicted: root must be an object")?;
    for property in top.properties() {
        if let Some(value) = property.value() { validate(value, path)?; }
    }
    ensure_unique(&top, path, "$root")?;
    Ok(root.to_serde_value())
}

fn object(parent: &CstObject, key: &str, path: &Path) -> Result<CstObject, String> {
    let value = parent.object_value_or_create(key).ok_or_else(|| {
        format!("qoder_config_conflicted: {}: {key} must be an object", path.display())
    })?;
    ensure_unique(&value, path, key)?;
    Ok(value)
}

fn validate_string(value: &str) -> Result<(), String> {
    // Qoder expands variables recursively, including literal credentials.
    if value.as_bytes().windows(2).any(|pair| {
        pair[0] == b'$' && (pair[1].is_ascii_alphanumeric() || matches!(pair[1], b'_' | b'{'))
    }) {
        return Err("qoder_interpolation_unsupported: literal values containing environment substitutions cannot be preserved; use an environment credential reference".into());
    }
    Ok(())
}

pub(super) fn prepare(
    agent_id: &str,
    path: &Path,
    profile: &ModelProfile,
    active: bool,
    literal_key: Option<&str>,
) -> Result<PreparedModelFile, String> {
    for value in [&profile.base_url, &profile.model, &profile.name] {
        validate_string(value)?;
    }
    if let Some(key) = literal_key {
        validate_string(key)?;
    }
    read_registry(path)?;
    let (root, original) = read_jsonc(path).map_err(|_| "qoder_config_conflicted: configuration must be valid JSON/JSONC".to_string())?;
    let top = root_object(&root, path)?;
    ensure_unique(&top, path, "$root")?;
    let providers = object(&top, "providers", path)?;
    let id = provider_id_for(agent_id, profile);
    if !safe_provider_id(&id) || !id.as_bytes()[0].is_ascii_alphanumeric() || id == "qoder" {
        return Err("qoder_provider_id_invalid: unsupported provider identity".into());
    }
    let provider = object(&providers, &id, path)?;
    if provider.get("models").and_then(|value| value.array_value()).is_some_and(|models| {
        models.elements().iter().any(|entry| {
            entry.to_serde_value().and_then(|value| value.get("model").cloned())
                .is_some_and(|id| id != json!(profile.model))
        })
    }) {
        return Err("qoder_shared_provider_read_only: provider connection is shared with another model".into());
    }
    let protocol = match profile.protocol {
        ModelProtocol::OpenaiCompletions => "openai",
        ModelProtocol::OpenaiResponses => "openai-responses",
        ModelProtocol::AnthropicMessages => "anthropic",
        _ => return Err("qoder_protocol_unsupported".into()),
    };
    let api_key = literal_key.map(str::to_string)
        .or_else(|| profile.env_key.as_ref().map(|key| format!("${{{key}}}")))
        .unwrap_or_default();
    for (key, value) in [
        ("type", "openai-compatible"),
        ("protocol", protocol),
        ("authType", if protocol == "anthropic" { "api-key" } else { "bearer" }),
        ("baseUrl", profile.base_url.as_str()),
        ("apiKey", api_key.as_str()),
    ] {
        set_json(&provider, key, Some(json!(value)));
    }
    // Each Arca-created provider owns one model; preserve provider metadata.
    if provider.get("displayName").is_none() {
        set_json(&provider, "displayName", Some(json!(profile.name)));
    }
    if provider.get("model").is_none() {
        set_json(&provider, "model", Some(json!(profile.model)));
    }
    let models = provider.array_value_or_create("models").ok_or_else(|| {
        "qoder_config_conflicted: provider models must be an array".to_string()
    })?;
    let mut ids = BTreeSet::new();
    let mut existing = None;
    for element in models.elements() {
        let entry = element.as_object().ok_or("qoder_config_conflicted: model must be an object")?;
        ensure_unique(&entry, path, "models[]")?;
        let model_id = entry.get("model").and_then(|p| p.value()).and_then(|v| v.to_serde_value())
            .and_then(|v| v.as_str().map(str::to_string)).ok_or("qoder_config_conflicted: model id missing")?;
        if !ids.insert(model_id.clone()) {
            return Err("qoder_config_conflicted: duplicate model id".into());
        }
        if model_id == profile.model { existing = Some(entry); }
    }
    let entry = match existing {
        Some(entry) => entry,
        None => {
            models.append(input_value(json!({"model": profile.model})));
            models.elements().last().and_then(|node| node.as_object())
                .ok_or("qoder_config_conflicted: cannot create model")?
        }
    };
    set_json(&entry, "displayName", Some(json!(profile.name)));
    set_json(&entry, "contextWindow", profile.context_window.map(|value| json!(value)));
    set_json(&entry, "maxOutputTokens", profile.max_output_tokens.map(|value| json!(value)));
    if let Some(reasoning) = profile.reasoning {
        let capabilities = object(&entry, "capabilities", path)?;
        let thinking = object(&capabilities, "thinking", path)?;
        set_json(&thinking, "modes", Some(if reasoning { json!(["enabled"]) } else { json!([]) }));
    }
    if agent_id == "qoder-cli" && active {
        let selection = object(&top, "model", path)?;
        set_json(&selection, "name", Some(json!(format!("{id}/{}", profile.model))));
    }
    Ok(prepared_json(path, original, root))
}

pub(super) fn clear(agent_id: &str, path: &Path, profile: &ModelProfile) -> Result<PreparedModelFile, String> {
    read_registry(path)?;
    let (root, original) = read_jsonc(path).map_err(|_| "qoder_config_conflicted: configuration must be valid JSON/JSONC".to_string())?;
    if original.is_none() { return Ok(PreparedModelFile { path: path.into(), original, content: None }); }
    let top = root_object(&root, path)?;
    ensure_unique(&top, path, "$root")?;
    if top.get("providers").is_none() { return Ok(prepared_json(path, original, root)); }
    let providers = object(&top, "providers", path)?;
    let id = provider_id_for(agent_id, profile);
    if providers.get(&id).is_none() { return Ok(prepared_json(path, original, root)); }
    let provider = object(&providers, &id, path)?;
    let models = provider.get("models").and_then(|p| p.array_value())
        .ok_or("qoder_config_conflicted: provider models must be an array")?;
    let mut remaining = Vec::new();
    let mut removed = false;
    let mut ids = BTreeSet::new();
    for element in models.elements() {
        let value = element.to_serde_value().ok_or("qoder_config_conflicted: invalid model")?;
        let id = value.get("model").and_then(Value::as_str).ok_or("qoder_config_conflicted: model id missing")?;
        if !ids.insert(id.to_string()) { return Err("qoder_config_conflicted: duplicate model id".into()); }
        if id == profile.model { element.remove(); removed = true; } else { remaining.push(id.to_string()); }
    }
    if !removed { return Ok(prepared_json(path, original, root)); }
    clear_selection(&top, path, &BTreeSet::from([format!("{id}/{}", profile.model)]))?;
    if remaining.is_empty() {
        set_json(&providers, &id, None);
    } else if provider.get("model").and_then(|p| p.value()).and_then(|v| v.to_serde_value()).as_ref() == Some(&json!(profile.model)) {
        set_json(&provider, "model", Some(json!(remaining[0])));
    }
    Ok(prepared_json(path, original, root))
}

fn clear_selection(top: &CstObject, path: &Path, removed: &BTreeSet<String>) -> Result<(), String> {
    if top.get("model").is_none() { return Ok(()); }
    let selection = object(top, "model", path)?;
    if selection.get("name").and_then(|p| p.value()).and_then(|v| v.to_serde_value())
        .and_then(|v| v.as_str().map(str::to_string)).is_some_and(|name| removed.contains(&name)) {
        set_json(&selection, "name", None);
    }
    Ok(())
}

pub(super) fn clear_all(path: &Path) -> Result<PreparedModelFile, String> {
    // Discovery and deletion must agree on which entries were reviewed.
    read_registry(path)?;
    let (root, original) = read_jsonc(path).map_err(|_| "qoder_config_conflicted: configuration must be valid JSON/JSONC".to_string())?;
    if original.is_none() { return Ok(PreparedModelFile { path: path.into(), original, content: None }); }
    let top = root_object(&root, path)?;
    ensure_unique(&top, path, "$root")?;
    let mut removed = BTreeSet::new();
    if top.get("providers").is_some() {
        let providers = object(&top, "providers", path)?;
        // Delete only records represented by the custom-endpoint observer.
        for property in providers.properties() {
            let Some(value) = property.value().and_then(|v| v.to_serde_value()) else { continue; };
            if value.get("baseUrl").and_then(Value::as_str).is_none() { continue; }
            let Some(models) = value.get("models").and_then(Value::as_array) else { continue; };
            let mut ids = BTreeSet::new();
            for model in models {
                let id = model.get("model").and_then(Value::as_str)
                    .ok_or("qoder_config_conflicted: invalid model id")?;
                if !ids.insert(id) { return Err("qoder_config_conflicted: duplicate model id".into()); }
            }
            if !matches!(value.get("protocol").and_then(Value::as_str).unwrap_or("openai"), "openai" | "openai-responses" | "anthropic") {
                return Err("qoder_config_conflicted: unsupported provider protocol".into());
            }
            let provider_id = property.name().and_then(|name| name.decoded_value().ok())
                .ok_or("qoder_config_conflicted: invalid provider identity")?;
            for id in ids { removed.insert(format!("{provider_id}/{id}")); }
            if !models.is_empty() { property.remove(); }
        }
    }
    clear_selection(&top, path, &removed)?;
    Ok(prepared_json(path, original, root))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("arca-qoder-models-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path { &self.0 }
    }
    impl Drop for TempDir {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }

    fn profile() -> ModelProfile {
        serde_json::from_value(json!({
            "id": "qoder-fixture", "name": "Fixture model", "model": "fixture-model",
            "protocol": "openai-completions", "base_url": "https://example.invalid/v1",
            "env_key": "ARCA_QODER_FIXTURE_KEY", "context_window": 128000
        })).unwrap()
    }

    #[test]
    fn cli_selection_and_removal_preserve_desktop_and_other_cli_models() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{\n// preserve\n\"mcpServers\":{},\"model\":{\"preferences\":{\"keep\":true}}}").unwrap();
        let first = profile();
        let mut second = profile();
        second.id = "second".into();
        second.model = "vendor/second".into();
        for (agent, profile, active) in [(AGENT, &first, false), ("qoder-cli", &first, true), ("qoder-cli", &second, false)] {
            fs::write(&path, prepare(agent, &path, profile, active, None).unwrap().content.unwrap()).unwrap();
        }
        let profiles = std::collections::BTreeMap::from([(first.id.clone(), first.clone()), (second.id.clone(), second.clone())]);
        assert_eq!(observe_active("qoder-cli", &[path.clone()], &profiles), ObservedActiveModel::Managed(first.id.clone()));
        assert_ne!(provider_id_for(AGENT, &first), provider_id_for("qoder-cli", &first));
        fs::write(&path, prepare("qoder-cli", &path, &second, true, None).unwrap().content.unwrap()).unwrap();
        fs::write(&path, clear("qoder-cli", &path, &first).unwrap().content.unwrap()).unwrap();
        assert_eq!(observe_active("qoder-cli", &[path.clone()], &profiles), ObservedActiveModel::Managed(second.id.clone()));
        let root = read_registry(&path).unwrap().unwrap();
        assert!(root["providers"].get(provider_id_for(AGENT, &first)).is_some());
        assert_eq!(root["model"]["preferences"]["keep"], true);
        fs::write(&path, clear("qoder-cli", &path, &second).unwrap().content.unwrap()).unwrap();
        assert_eq!(observe_active("qoder-cli", &[path.clone()], &profiles), ObservedActiveModel::None);
        assert!(fs::read_to_string(path).unwrap().contains("// preserve"));
    }

    #[test]
    fn clear_all_removes_only_deleted_model_selection_and_preserves_preferences() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        for (selected, cleared) in [("external/model", true), ("qoder/builtin", false)] {
            fs::write(&path, json!({"providers":{"external":{"baseUrl":"https://example.invalid", "models":[{"model":"model"}]}}, "model":{"name":selected,"preferences":{"keep":true}}, "mcpServers":{}}).to_string()).unwrap();
            fs::write(&path, clear_all(&path).unwrap().content.unwrap()).unwrap();
            let root = read_registry(&path).unwrap().unwrap();
            assert_eq!(root["model"].get("name").is_none(), cleared);
            assert_eq!(root["model"]["preferences"]["keep"], true);
            assert!(root.get("mcpServers").is_some());
        }
    }

    #[test]
    fn cli_rejects_ambiguous_nested_config_without_writing() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        let original = r#"{"model":{"name":"one","name":"two"}}"#;
        fs::write(&path, original).unwrap();
        assert!(prepare("qoder-cli", &path, &profile(), true, None).is_err());
        assert_eq!(observe_active("qoder-cli", &[path.clone()], &Default::default()), ObservedActiveModel::Conflicted);
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn round_trip_preserves_mcp_comments_and_cli_selection_for_each_protocol() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        let original = "{\n  // keep this MCP and CLI selection\n  \"mcpServers\": {\"keep\": {\"command\": \"fixture\"}},\n  \"model\": {\"name\": \"cli-selection\"},\n  \"hooks\": {}\n}\n";
        for (protocol, wire) in [
            (ModelProtocol::OpenaiCompletions, "openai"),
            (ModelProtocol::OpenaiResponses, "openai-responses"),
            (ModelProtocol::AnthropicMessages, "anthropic"),
        ] {
            fs::write(&path, original).unwrap();
            let mut profile = profile();
            profile.protocol = protocol;
            let prepared = prepare(AGENT, &path, &profile, false, None).unwrap();
            let content = prepared.content.unwrap();
            assert!(content.contains("// keep this MCP and CLI selection"));
            fs::write(&path, &content).unwrap();
            let root = read_jsonc(&path).unwrap().0.to_serde_value().unwrap();
            let provider = &root["providers"][provider_id_for(AGENT, &profile)];
            assert_eq!(provider["protocol"], wire);
            assert_eq!(provider["apiKey"], "${ARCA_QODER_FIXTURE_KEY}");
            assert_eq!(root["model"]["name"], "cli-selection");
            assert_eq!(root["mcpServers"]["keep"]["command"], "fixture");
            assert_eq!(prepare(AGENT, &path, &profile, false, None).unwrap().content.as_deref(), Some(content.as_str()));
            let cleared = clear(AGENT, &path, &profile).unwrap().content.unwrap();
            fs::write(&path, cleared).unwrap();
            let root = read_jsonc(&path).unwrap().0.to_serde_value().unwrap();
            assert!(root["providers"].as_object().unwrap().is_empty());
            assert_eq!(root["model"]["name"], "cli-selection");
            assert_eq!(root["mcpServers"]["keep"]["command"], "fixture");
        }
    }

    #[test]
    fn ambiguous_configs_and_interpolated_literal_keys_fail_closed() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        let original = "{\"providers\": {}, \"providers\": {}}";
        fs::write(&path, original).unwrap();
        assert!(prepare(AGENT, &path, &profile(), false, None).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::write(&path, "{}").unwrap();
        assert!(prepare(AGENT, &path, &profile(), false, Some("fixture-$SUBSTITUTION")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
    }

    #[test]
    fn clear_preserves_external_sibling_models() {
        let dir = TempDir::new();
        let path = dir.path().join("settings.json");
        let mut profile = profile();
        profile.native_ids.insert(AGENT.into(), "external".into());
        fs::write(&path, r#"{"providers":{"external":{"baseUrl":"https://example.invalid/v1","apiKey":"${FIXTURE_KEY}","model":"fixture-model","models":[{"model":"fixture-model"},{"model":"sibling","custom":true}]}},"mcpServers":{}}"#).unwrap();
        let content = clear(AGENT, &path, &profile).unwrap().content.unwrap();
        fs::write(&path, content).unwrap();
        let root = read_jsonc(&path).unwrap().0.to_serde_value().unwrap();
        assert_eq!(root["providers"]["external"]["model"], "sibling");
        assert_eq!(root["providers"]["external"]["models"][0]["custom"], true);
        assert_eq!(root["providers"]["external"]["apiKey"], "${FIXTURE_KEY}");
    }
}
