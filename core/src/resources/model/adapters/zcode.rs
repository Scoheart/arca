//! ZCode Desktop native custom model registry. See docs/zcode-models.md.
use super::*;

const AGENT: &str = "zcode";

fn load(path: &Path) -> Result<(CstRootNode, Option<String>), String> {
    let (root, original) = read_jsonc(path)
        .map_err(|_| "zcode_config_conflicted: invalid JSON/JSONC".to_string())?;
    fn validate(node: jsonc_parser::cst::CstNode, path: &Path) -> Result<(), String> {
        if let Some(object) = node.as_object() {
            ensure_unique(&object, path, "ZCode config")?;
            for property in object.properties() {
                if let Some(value) = property.value() { validate(value, path)?; }
            }
        } else if let Some(array) = node.as_array() {
            for value in array.elements() { validate(value, path)?; }
        }
        Ok(())
    }
    let top = root_object(&root, path)?;
    ensure_unique(&top, path, "$root")?;
    for property in top.properties() {
        if let Some(value) = property.value() { validate(value, path)?; }
    }
    if let Some(value) = root.to_serde_value() {
        if let Some(providers) = value.get("provider") {
            for (id, provider) in providers.as_object().ok_or("zcode_config_conflicted: provider must be an object")? {
                let provider = provider.as_object().ok_or("zcode_config_conflicted: invalid provider")?;
                for key in ["options", "models"] {
                    if provider.get(key).is_some_and(|value| !value.is_object()) {
                        return Err("zcode_config_conflicted: invalid provider options/models".into());
                    }
                }
                if let Some(models) = provider.get("models").and_then(Value::as_object) {
                    for (id, model) in models {
                        if id.trim().is_empty() || !model.is_object() {
                            return Err("zcode_config_conflicted: invalid model".into());
                        }
                    }
                }
                if !id.starts_with("builtin:") && provider.get("source").and_then(Value::as_str) == Some("custom")
                    && provider.get("kind").and_then(Value::as_str) == Some("openai-compatible") {
                    if provider.get("options").and_then(|options| options.get("baseURL")).and_then(Value::as_str).is_none()
                        || provider.get("models").and_then(Value::as_object).is_none() {
                        return Err("zcode_config_conflicted: custom provider requires baseURL and models".into());
                    }
                }
                if provider.get("enabled").is_some_and(|value| !value.is_boolean()) {
                    return Err("zcode_config_conflicted: invalid enabled flag".into());
                }
            }
        }
    }
    Ok((root, original))
}

pub(crate) fn read_registry(path: &Path) -> Result<Option<Value>, String> {
    let (root, original) = load(path)?;
    Ok(original.and_then(|_| root.to_serde_value()))
}

fn object(parent: &CstObject, key: &str) -> Result<CstObject, String> {
    parent.object_value_or_create(key).ok_or_else(|| "zcode_config_conflicted: expected object".into())
}

pub(super) fn prepare(path: &Path, profile: &ModelProfile, literal_key: Option<&str>) -> Result<PreparedModelFile, String> {
    if profile.protocol != ModelProtocol::OpenaiCompletions {
        return Err("zcode_protocol_unsupported: only Chat Completions is verified".into());
    }
    // External registry entries are never adopted in place, including credentials.
    if profile.native_ids.contains_key(AGENT) {
        return Err("zcode_external_provider_read_only: import into a new Arca provider".into());
    }
    let (root, original) = load(path)?;
    let top = root_object(&root, path)?;
    let providers = object(&top, "provider")?;
    let id = provider_id_for(AGENT, profile);
    if !safe_provider_id(&id) { return Err("zcode_provider_id_invalid".into()); }
    let provider = object(&providers, &id)?;
    let current = provider.to_serde_value().ok_or("zcode_config_conflicted: invalid provider")?;
    if current.get("models").and_then(Value::as_object).is_some_and(|models| models.keys().any(|id| id != &profile.model)) {
        return Err("zcode_shared_provider_read_only: provider contains another model".into());
    }
    for (key, expected) in [("kind", "openai-compatible"), ("source", "custom")] {
        if current.get(key).is_some_and(|value| value != expected) {
            return Err("zcode_provider_conflicted: provider identity changed".into());
        }
        set_json(&provider, key, Some(json!(expected)));
    }
    set_json(&provider, "name", Some(json!(profile.name)));
    set_json(&provider, "enabled", Some(json!(true)));
    let options = object(&provider, "options")?;
    set_json(&options, "baseURL", Some(json!(profile.base_url)));
    // prepare_apply runs before the explicit credential route is resolved. Preserve
    // an existing key here; only prepare_apply_plaintext can replace it.
    if let Some(key) = literal_key { set_json(&options, "apiKey", Some(json!(key))); }
    let models = object(&provider, "models")?;
    let model = object(&models, &profile.model)?;
    if profile.context_window.is_some() || profile.max_output_tokens.is_some() || model.get("limit").is_some() {
        let limit = object(&model, "limit")?;
        set_json(&limit, "context", profile.context_window.map(|value| json!(value)));
        set_json(&limit, "output", profile.max_output_tokens.map(|value| json!(value)));
    }
    if let Some(enabled) = profile.reasoning {
        let reasoning = object(&model, "reasoning")?;
        set_json(&reasoning, "enabled", Some(json!(enabled)));
    }
    Ok(prepared_json(path, original, root))
}

pub(super) fn clear(path: &Path, profile: &ModelProfile) -> Result<PreparedModelFile, String> {
    if profile.native_ids.contains_key(AGENT) { return Err("zcode_external_provider_read_only".into()); }
    let (root, original) = load(path)?;
    if original.is_none() { return Ok(PreparedModelFile { path: path.into(), original, content: None }); }
    let top = root_object(&root, path)?;
    if top.get("provider").is_none() { return Ok(prepared_json(path, original, root)); }
    let providers = object(&top, "provider")?;
    let id = provider_id_for(AGENT, profile);
    if providers.get(&id).is_none() { return Ok(prepared_json(path, original, root)); }
    let provider = object(&providers, &id)?;
    let identity = provider.to_serde_value().ok_or("zcode_config_conflicted")?;
    if identity.get("kind").and_then(Value::as_str) != Some("openai-compatible")
        || identity.get("source").and_then(Value::as_str) != Some("custom") {
        return Err("zcode_provider_conflicted: provider identity changed".into());
    }
    if provider.get("models").is_some() {
        set_json(&object(&provider, "models")?, &profile.model, None);
    }
    // Provider, key, enabled flag and unknown metadata survive removal.
    Ok(prepared_json(path, original, root))
}

pub(super) fn clear_all(path: &Path) -> Result<PreparedModelFile, String> {
    let (root, original) = load(path)?;
    if original.is_none() { return Ok(PreparedModelFile { path: path.into(), original, content: None }); }
    let top = root_object(&root, path)?;
    if top.get("provider").is_some() {
        let providers = object(&top, "provider")?;
        for property in providers.properties() {
            let Some(id) = property.name().and_then(|name| name.decoded_value().ok()) else { return Err("zcode_config_conflicted: invalid provider id".into()); };
            if id.starts_with("builtin:") { continue; }
            let Some(provider) = property.value().and_then(|value| value.as_object()) else { continue; };
            let value = provider.to_serde_value().ok_or("zcode_config_conflicted")?;
            // Same boundary as the observer: custom Chat Completions only.
            if value.get("source").and_then(Value::as_str) != Some("custom")
                || value.get("kind").and_then(Value::as_str) != Some("openai-compatible")
                || value.get("options").and_then(|options| options.get("baseURL")).and_then(Value::as_str).is_none() { continue; }
            if let Some(models) = provider.get("models").and_then(|property| property.object_value()) {
                for property in models.properties() { property.remove(); }
            }
        }
    }
    Ok(prepared_json(path, original, root))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("arca-zcode-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap(); Self(path)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn profile(id: &str) -> ModelProfile {
        serde_json::from_value(json!({"id": id, "name": id, "model": id,
            "protocol": "openai-completions", "base_url": "https://example.invalid/v1", "context_window": 128000})).unwrap()
    }
    #[test]
    fn multiple_models_round_trip_keeps_other_providers_and_credentials() {
        let temp = Temp::new(); let path = temp.0.join("config.json");
        fs::write(&path, "{\n // preserve\n \"provider\": {\"builtin:test\": {\"source\":\"custom\",\"kind\":\"openai-compatible\",\"options\":{\"baseURL\":\"https://example.invalid\"},\"models\":{\"builtin\":{}}}},\"hooks\":{\"keep\":true}}\n").unwrap();
        for id in ["first", "second"] {
            fs::write(&path, prepare(&path, &profile(id), Some("synthetic-fixture-key")).unwrap().content.unwrap()).unwrap();
        }
        let before = fs::read_to_string(&path).unwrap();
        assert!(before.contains("// preserve"));
        assert_eq!(prepare(&path, &profile("first"), Some("synthetic-fixture-key")).unwrap().content.as_deref(), Some(before.as_str()));
        fs::write(&path, clear(&path, &profile("first")).unwrap().content.unwrap()).unwrap();
        let root = read_registry(&path).unwrap().unwrap();
        assert_eq!(root["provider"][provider_id("first")]["options"]["apiKey"], "synthetic-fixture-key");
        assert!(root["provider"][provider_id("first")]["models"].as_object().unwrap().is_empty());
        assert!(root["provider"][provider_id("second")]["models"]["second"].is_object());
        fs::write(&path, clear_all(&path).unwrap().content.unwrap()).unwrap();
        let root = read_registry(&path).unwrap().unwrap();
        assert!(root["provider"]["builtin:test"]["models"]["builtin"].is_object());
        assert_eq!(root["hooks"]["keep"], true);
        assert_eq!(root["provider"][provider_id("second")]["options"]["apiKey"], "synthetic-fixture-key");
    }
    #[test]
    fn corrupt_and_duplicate_registry_is_never_rewritten() {
        let temp = Temp::new(); let path = temp.0.join("config.json");
        for original in ["{", "{\"provider\":{},\"provider\":{}}", "{\"provider\":[]}", "{\"provider\":{\"external\":{\"models\":{\"x\":{},\"x\":{}}}}}"] {
            fs::write(&path, original).unwrap();
            assert!(prepare(&path, &profile("first"), Some("synthetic-fixture-key")).is_err());
            assert!(clear_all(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), original);
        }
    }
}
