//! One cURL pipeline for Desktop copy and explicit CLI export.
use super::{credential, normalize_endpoint_path, normalize_provider_base_url};
use crate::domain::types::{ApiKeySource, AuthRequirement, ModelProtocol};
use crate::settings::load_settings_strict;
use serde_json::json;
use zeroize::Zeroizing;

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
    let body = match protocol {
        ModelProtocol::AnthropicMessages => {
            lines.push("  --header 'anthropic-version: 2023-06-01'".into());
            json!({"model": model, "max_tokens": 1024, "messages": [{"role": "user", "content": "Hello"}]})
        }
        ModelProtocol::GeminiGenerateContent => json!({"contents": [{"role": "user", "parts": [{"text": "Hello"}]}]}),
        ModelProtocol::GeminiInteractions => json!({"model": model, "input": "Hello", "store": false}),
        ModelProtocol::AwsBedrockConverse => json!({"messages": [{"role": "user", "content": [{"text": "Hello"}]}], "inferenceConfig": {"maxTokens": 1024}}),
        ModelProtocol::OpenaiResponses => json!({"model": model, "input": "Hello"}),
        ModelProtocol::OpenaiCompletions => json!({"model": model, "messages": [{"role": "user", "content": "Hello"}]}),
    };
    lines.push(format!("  --data-raw {}", quote(&serde_json::to_string_pretty(&body).map_err(|_| "curl_serialization_failed")?)));
    Ok(format!("{} | jq -r", lines.join(" \\\n")))
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
