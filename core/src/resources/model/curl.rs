//! One cURL pipeline for Desktop copy and explicit CLI export.
use super::{credential, normalize_endpoint_path, normalize_provider_base_url};
use crate::domain::types::{ApiKeySource, AuthRequirement, ModelProtocol};
use crate::settings::load_settings_strict;
use serde_json::json;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};
use zeroize::Zeroizing;

#[derive(Deserialize)]
struct CurlTemplates {
    provider_overrides: BTreeMap<String, BTreeMap<ModelProtocol, BTreeMap<String, serde_json::Value>>>,
    templates: BTreeMap<ModelProtocol, CurlTemplate>,
}

#[derive(Deserialize)]
struct CurlTemplate {
    request: serde_json::Value,
}

static TEMPLATES: LazyLock<CurlTemplates> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../data/model-curl-templates.json"))
        .expect("embedded model cURL templates must be valid")
});

fn request_template(provider_type: &str, protocol: &ModelProtocol, model: &str) -> Result<serde_json::Value, String> {
    let mut request = TEMPLATES.templates.get(protocol)
        .ok_or("curl_template_missing: protocol has no request template")?.request.clone();
    let body = request.as_object_mut().ok_or("curl_template_invalid: request must be an object")?;
    if let Some(overrides) = TEMPLATES.provider_overrides.get(provider_type)
        .and_then(|templates| templates.get(protocol)) {
        body.extend(overrides.clone());
    }
    if body.contains_key("model") { body.insert("model".into(), json!(model)); }
    if *protocol == ModelProtocol::AwsBedrockConverse && model.starts_with("arn:") && model.contains(":prompt/") {
        for field in ["system", "inferenceConfig", "toolConfig", "additionalModelRequestFields"] {
            body.insert(field.into(), serde_json::Value::Null);
        }
    }
    // Filter unset options in Core so the copied text is just one cURL command.
    // Do not remove false, zero, empty collections or nested null values.
    body.retain(|_, value| !value.is_null());
    Ok(request)
}

fn quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn url_component(value: &str) -> String {
    value.bytes().map(|byte| {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            (byte as char).to_string()
        } else { format!("%{byte:02X}") }
    }).collect()
}

pub enum CurlAuth<'a> { None, Placeholder, Key(&'a str) }

pub fn render(provider_type: &str, protocol: &ModelProtocol, url: &str, model: &str, auth: CurlAuth<'_>) -> Result<String, String> {
    if model.contains('\0') || url.contains('\0') {
        return Err("curl_invalid_input: NUL bytes are unsupported".into());
    }
    let body = serde_json::to_string_pretty(&request_template(provider_type, protocol, model)?)
        .map_err(|_| "curl_serialization_failed")?;
    let mut lines = vec![format!("curl --request POST {}", quote(&url.replace("{model}", &url_component(model)))),
        "  --header 'Content-Type: application/json'".into()];
    let prefix = match protocol {
        ModelProtocol::AnthropicMessages => "x-api-key: ",
        ModelProtocol::GeminiGenerateContent | ModelProtocol::GeminiInteractions => "x-goog-api-key: ",
        _ => "Authorization: Bearer ",
    };
    match auth {
        CurlAuth::None => {},
        CurlAuth::Placeholder => lines.push(format!("  --header \"{prefix}${{ARCA_API_KEY}}\"")),
        CurlAuth::Key(key) => {
            if key.is_empty() || key.contains(['\0', '\r', '\n']) {
                return Err("credential_invalid: API Key must be one non-empty line".into());
            }
            lines.push(format!("  --header {}", quote(&format!("{prefix}{key}"))));
        }
    }
    if *protocol == ModelProtocol::AnthropicMessages {
        lines.push("  --header 'anthropic-version: 2023-06-01'".into());
    }
    lines.push(format!("  --data-raw {}", quote(&body)));
    Ok(lines.join(" \\\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responses_exports_follow_reviewed_provider_storage_contracts() {
        for (provider, store) in [("decuff", false), ("openai", true), ("custom", true), ("decuff-other", true)] {
            let command = render(provider, &ModelProtocol::OpenaiResponses,
                "https://example.invalid/v1/responses", "fixture-model", CurlAuth::Placeholder).unwrap();
            let body = command.split_once("--data-raw '").unwrap().1.strip_suffix('\'').unwrap();
            let body: serde_json::Value = serde_json::from_str(body).unwrap();
            assert_eq!(body["store"], store, "{provider}");
            assert_eq!(body["model"], "fixture-model");
            assert_eq!(body["input"], "Hello");
            assert!(body.get("previous_response_id").is_none());
            assert!(body.get("conversation").is_none());
        }
    }

    #[test]
    fn responses_storage_overrides_do_not_leak_to_other_protocols() {
        let body = request_template("decuff", &ModelProtocol::AnthropicMessages, "fixture-model").unwrap();
        assert!(body.get("store").is_none());
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["model"], "fixture-model");
    }

    #[test]
    fn interactions_uses_google_auth_and_stateless_input() {
        let command = render("custom", &ModelProtocol::GeminiInteractions,
            "https://example.invalid/v1beta/interactions", "gemini-fixture", CurlAuth::Placeholder).unwrap();
        assert!(command.contains("x-goog-api-key: ${ARCA_API_KEY}"));
        assert!(command.contains("\"input\": \"Hello\""));
        assert!(command.contains("\"store\": false"));
        assert!(!command.contains("\"contents\""));
        assert!(!command.contains("Authorization: Bearer"));
    }

    #[test]
    fn converse_escapes_model_path_and_uses_content_blocks() {
        let model = "arn:aws:bedrock:us-east-1:123456789012:inference-profile/a/b";
        let command = render("custom", &ModelProtocol::AwsBedrockConverse,
            "https://example.invalid/model/{model}/converse", model, CurlAuth::Placeholder).unwrap();
        assert!(command.contains("/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A123456789012%3Ainference-profile%2Fa%2Fb/converse"));
        assert!(command.contains("Authorization: Bearer ${ARCA_API_KEY}"));
        assert!(command.contains("\"text\": \"Hello\""));
        assert!(!command.contains("\"model\":"));
        assert!(!command.contains("{model}"));
        assert!(render("custom", &ModelProtocol::AwsBedrockConverse, "https://example.invalid/model/{model}/converse",
            "model", CurlAuth::Key("fixture\r\nInjected: yes")).is_err());
    }

    #[test]
    fn prompt_management_arns_do_not_override_the_managed_prompt() {
        let body = request_template("custom", &ModelProtocol::AwsBedrockConverse,
            "arn:aws:bedrock:us-east-1:123456789012:prompt/ABCDEFGHIJ:1").unwrap();
        for key in ["system", "inferenceConfig", "toolConfig", "additionalModelRequestFields"] {
            assert!(body.get(key).is_none(), "{key} cannot override a managed prompt");
        }
        assert_eq!(body["messages"][0]["role"], "user");
        let ordinary = request_template("custom", &ModelProtocol::AwsBedrockConverse, "anthropic.fixture").unwrap();
        assert!(ordinary["system"][0]["text"].is_string());
        assert_eq!(ordinary["inferenceConfig"]["maxTokens"], 1024);
    }

    #[test]
    fn export_is_one_curl_command_with_a_literal_json_argument() {
        let model = "fixture'$(touch unwanted)\nexit 7";
        let command = render("custom", &ModelProtocol::OpenaiResponses,
            "https://example.invalid/responses", model, CurlAuth::None).unwrap();
        assert!(command.starts_with("curl --request POST "));
        assert!(!command.contains("ARCA_REQUEST_BODY"));
        assert!(!command.contains("jq "));
        assert!(!command.contains("<<"));
        let argument = command.split_once("--data-raw ").unwrap().1;
        let json = argument.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap()
            .replace("'\\''", "'");
        let body: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(body["model"], model);
        assert_eq!(body["input"], "Hello");
        assert!(body["instructions"].is_string());
        assert!(body.get("temperature").is_none());
        assert_eq!(body["stream"], false);
    }
}

/// The returned command may contain a secret only on explicit export. Never log,
/// cache or persist it. Missing required credentials fail instead of copying a
/// command advertised as ready to run.
pub fn for_profile(profile_id: &str, include_api_key: bool) -> Result<String, String> {
    let settings = load_settings_strict().map_err(|error| error.to_string())?;
    let profile = settings.model_profiles.as_ref().and_then(|profiles| profiles.get(profile_id))
        .ok_or("model_profile_not_found: no Model Profile has that ID")?;
    let provider = profile.provider_id.as_ref().and_then(|id| settings.model_providers.as_ref()?.get(id));
    if profile.provider_id.is_some() && provider.is_none() {
        return Err("model_provider_not_found: this Profile references a missing Provider".into());
    }
    let provider_type = provider.map(|provider| provider.provider.as_str()).unwrap_or(profile.provider.as_str());
    let (base, endpoint) = if let Some(provider) = provider {
        let route = provider.protocols.get(&profile.protocol)
            .ok_or("model_protocol_unavailable: Provider no longer supports this protocol")?;
        (provider.base_url.as_str(), route.endpoint_path.as_str())
    } else {
        (profile.base_url.as_str(), if profile.endpoint_path.is_empty() { profile.protocol.default_endpoint_path() } else { &profile.endpoint_path })
    };
    let url = format!("{}{}", normalize_provider_base_url(base)?, normalize_endpoint_path(endpoint)?);
    let auth_requirement = provider.map(|provider| &provider.auth_requirement).unwrap_or(&AuthRequirement::Required);
    if *auth_requirement == AuthRequirement::None {
        return render(provider_type, &profile.protocol, &url, &profile.model, CurlAuth::None);
    }
    let configured_source = if let Some(provider) = provider { provider.api_key_source.clone() }
        else { profile.env_key.as_ref().map(|name| ApiKeySource::Env { name: name.clone() }) };
    if !include_api_key {
        let auth = if *auth_requirement == AuthRequirement::Optional && configured_source.is_none() {
            CurlAuth::None
        } else { CurlAuth::Placeholder };
        return render(provider_type, &profile.protocol, &url, &profile.model, auth);
    }
    let stored = if configured_source.as_ref().is_none_or(|source| matches!(source, ApiKeySource::ArcaStore)) {
        if let Some(provider) = provider {
            super::read_credential_service_checked(&super::provider_keychain_service(&provider.id))?
        } else { super::read_credential_checked(profile_id)? }
    } else { None };
    if configured_source.as_ref().is_none_or(|source| matches!(source, ApiKeySource::ArcaStore))
        && stored.is_none() && *auth_requirement == AuthRequirement::Optional {
        return render(provider_type, &profile.protocol, &url, &profile.model, CurlAuth::None);
    }
    let resolved = credential::resolve_source(&configured_source.unwrap_or(ApiKeySource::ArcaStore), stored)?;
    let key = Zeroizing::new(String::from_utf8(resolved.expose_for_delivery().to_vec())
        .map_err(|_| "credential_invalid: API Key must be UTF-8")?);
    render(provider_type, &profile.protocol, &url, &profile.model, CurlAuth::Key(&key))
}
