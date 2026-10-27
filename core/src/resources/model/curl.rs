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
    reviewed_on: String,
    templates: BTreeMap<ModelProtocol, CurlTemplate>,
}

#[derive(Deserialize)]
struct CurlTemplate {
    sources: Vec<String>,
    notes: Vec<String>,
    request: serde_json::Value,
}

static TEMPLATES: LazyLock<CurlTemplates> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../data/model-curl-templates.json"))
        .expect("embedded model cURL templates must be valid")
});

fn request_template(protocol: &ModelProtocol, model: &str) -> Result<serde_json::Value, String> {
    let mut request = TEMPLATES.templates.get(protocol)
        .ok_or("curl_template_missing: protocol has no request template")?.request.clone();
    let body = request.as_object_mut().ok_or("curl_template_invalid: request must be an object")?;
    if body.contains_key("model") { body.insert("model".into(), json!(model)); }
    if *protocol == ModelProtocol::AwsBedrockConverse && model.starts_with("arn:") && model.contains(":prompt/") {
        for field in ["system", "inferenceConfig", "toolConfig", "additionalModelRequestFields"] {
            body.insert(field.into(), serde_json::Value::Null);
        }
    }
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

pub fn render(protocol: &ModelProtocol, url: &str, model: &str, auth: CurlAuth<'_>) -> Result<String, String> {
    if model.contains('\0') || url.contains('\0') {
        return Err("curl_invalid_input: NUL bytes are unsupported".into());
    }
    let template = TEMPLATES.templates.get(protocol).ok_or("curl_template_missing: protocol has no request template")?;
    let body = serde_json::to_string_pretty(&request_template(protocol, model)?)
        .map_err(|_| "curl_serialization_failed")?;
    let mut preamble = format!("# Arca complete top-level request template (reviewed {}).\n", TEMPLATES.reviewed_on);
    preamble.push_str("# null = unset: omitted before sending; false, 0, empty arrays/objects and nested nulls are preserved.\n");
    preamble.push_str("# 1024 is an editable Arca example output-token limit, not an API default. Requires curl and jq.\n");
    preamble.push_str(match protocol {
        ModelProtocol::GeminiGenerateContent => "# Non-streaming generateContent. Streaming uses streamGenerateContent?alt=sse instead; do not add a stream body field.\n",
        ModelProtocol::AwsBedrockConverse => "# Non-streaming Converse. ConverseStream uses a separate /converse-stream endpoint and AWS event-stream, not JSON/SSE.\n",
        _ => "# stream defaults to false for JSON output. For SSE, set stream=true and replace the final jq response formatter.\n",
    });
    for source in &template.sources { preamble.push_str(&format!("# Reference: {source}\n")); }
    for note in &template.notes { preamble.push_str(&format!("# {note}\n")); }
    // A quoted here-document keeps model names and example text literal. Scope
    // the transient body to a subshell and stop before curl if JSON is invalid.
    preamble.push_str("(\n  ARCA_REQUEST_BODY=$(\n    jq -ce 'with_entries(select(.value != null))' <<'ARCA_REQUEST_JSON'\n");
    preamble.push_str(&body);
    preamble.push_str("\nARCA_REQUEST_JSON\n  ) || exit\n");
    let mut lines = vec![format!("  curl --request POST {}", quote(&url.replace("{model}", &url_component(model)))),
        "    --header 'Content-Type: application/json'".into()];
    let prefix = match protocol {
        ModelProtocol::AnthropicMessages => "x-api-key: ",
        ModelProtocol::GeminiGenerateContent | ModelProtocol::GeminiInteractions => "x-goog-api-key: ",
        _ => "Authorization: Bearer ",
    };
    match auth {
        CurlAuth::None => {},
        CurlAuth::Placeholder => lines.push(format!("    --header \"{prefix}${{ARCA_API_KEY}}\"")),
        CurlAuth::Key(key) => {
            if key.is_empty() || key.contains(['\0', '\r', '\n']) {
                return Err("credential_invalid: API Key must be one non-empty line".into());
            }
            lines.push(format!("    --header {}", quote(&format!("{prefix}{key}"))));
        }
    }
    if *protocol == ModelProtocol::AnthropicMessages {
        lines.push("    --header 'anthropic-version: 2023-06-01'".into());
    }
    lines.push("    --data-raw \"$ARCA_REQUEST_BODY\"".into());
    Ok(format!("{preamble}{} | jq -r\n)", lines.join(" \\\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactions_uses_google_auth_and_stateless_input() {
        let command = render(&ModelProtocol::GeminiInteractions,
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
        let command = render(&ModelProtocol::AwsBedrockConverse,
            "https://example.invalid/model/{model}/converse", model, CurlAuth::Placeholder).unwrap();
        assert!(command.contains("/model/arn%3Aaws%3Abedrock%3Aus-east-1%3A123456789012%3Ainference-profile%2Fa%2Fb/converse"));
        assert!(command.contains("Authorization: Bearer ${ARCA_API_KEY}"));
        assert!(command.contains("\"text\": \"Hello\""));
        assert!(!command.contains("\"model\":"));
        assert!(!command.contains("{model}"));
        assert!(render(&ModelProtocol::AwsBedrockConverse, "https://example.invalid/model/{model}/converse",
            "model", CurlAuth::Key("fixture\r\nInjected: yes")).is_err());
    }

    #[test]
    fn prompt_management_arns_do_not_override_the_managed_prompt() {
        let body = request_template(&ModelProtocol::AwsBedrockConverse,
            "arn:aws:bedrock:us-east-1:123456789012:prompt/ABCDEFGHIJ:1").unwrap();
        for key in ["system", "inferenceConfig", "toolConfig", "additionalModelRequestFields"] {
            assert!(body[key].is_null(), "{key} cannot override a managed prompt");
        }
        assert_eq!(body["messages"][0]["role"], "user");
        let ordinary = request_template(&ModelProtocol::AwsBedrockConverse, "anthropic.fixture").unwrap();
        assert!(ordinary["system"][0]["text"].is_string());
        assert_eq!(ordinary["inferenceConfig"]["maxTokens"], 1024);
    }

    #[test]
    fn model_text_cannot_escape_the_quoted_request_document() {
        let model = "fixture'$(touch unwanted)\nARCA_REQUEST_JSON\nexit 7";
        let command = render(&ModelProtocol::OpenaiResponses,
            "https://example.invalid/responses", model, CurlAuth::None).unwrap();
        assert_eq!(command.lines().filter(|line| *line == "ARCA_REQUEST_JSON").count(), 1);
        let start = command.find("\n{\n").unwrap() + 1;
        let end = command.find("\nARCA_REQUEST_JSON\n").unwrap();
        let body: serde_json::Value = serde_json::from_str(&command[start..end]).unwrap();
        assert_eq!(body["model"], model);
        assert_eq!(body["input"], "Hello");
        assert!(body["instructions"].is_string());
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
        return render(&profile.protocol, &url, &profile.model, CurlAuth::None);
    }
    let configured_source = if let Some(provider) = provider { provider.api_key_source.clone() }
        else { profile.env_key.as_ref().map(|name| ApiKeySource::Env { name: name.clone() }) };
    if !include_api_key {
        let auth = if *auth_requirement == AuthRequirement::Optional && configured_source.is_none() {
            CurlAuth::None
        } else { CurlAuth::Placeholder };
        return render(&profile.protocol, &url, &profile.model, auth);
    }
    let stored = if configured_source.as_ref().is_none_or(|source| matches!(source, ApiKeySource::ArcaStore)) {
        if let Some(provider) = provider {
            super::read_credential_service_checked(&super::provider_keychain_service(&provider.id))?
        } else { super::read_credential_checked(profile_id)? }
    } else { None };
    if configured_source.as_ref().is_none_or(|source| matches!(source, ApiKeySource::ArcaStore))
        && stored.is_none() && *auth_requirement == AuthRequirement::Optional {
        return render(&profile.protocol, &url, &profile.model, CurlAuth::None);
    }
    let resolved = credential::resolve_source(&configured_source.unwrap_or(ApiKeySource::ArcaStore), stored)?;
    let key = Zeroizing::new(String::from_utf8(resolved.expose_for_delivery().to_vec())
        .map_err(|_| "credential_invalid: API Key must be UTF-8")?);
    render(&profile.protocol, &url, &profile.model, CurlAuth::Key(&key))
}
