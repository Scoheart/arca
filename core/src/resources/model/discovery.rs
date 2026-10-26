use super::full_request_url;
use crate::domain::types::{ApiKeySource, AuthRequirement, ModelProtocol, ModelProviderConfig};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Read;
use std::time::{Duration, Instant};
use url::Url;
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MODELS: usize = 2_000;
const MAX_PAGES: usize = 10;
const CLOUDFLARE_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderModelSummary {
    pub id: String,
    pub name: Option<String>,
    pub context_length: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryAdapter {
    OpenAi,
    Anthropic,
    Gemini,
    Cohere,
    Fireworks,
    Cloudflare,
    DeepInfra,
    Vercel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialPolicy {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiscoverySpec {
    adapter: DiscoveryAdapter,
    credential: CredentialPolicy,
}

#[derive(Debug, PartialEq, Eq)]
struct DecodedPage {
    models: Vec<ProviderModelSummary>,
    next_token: Option<String>,
}

pub(super) fn model_discovery_supported(provider_type: &str) -> bool {
    // Chat API compatibility does not imply a Models API. Azure model names
    // are deployment aliases, not entries in the public model catalog.
    !matches!(provider_type,
        "github-models" | "azure-openai" | "perplexity" | "volcengine" | "volcengine-coding-plan"
        | "baidu-qianfan" | "baidu-qianfan-coding-plan" | "zhipuai"
        | "stepfun" | "stepfun-global"
        | "featherless-ai" | "upstage" | "ai21" | "byteplus"
        | "tencent-tokenhub-cn" | "tencent-tokenhub-global" | "iflytek-spark-x2" | "nscale" | "tinfoil-proxy"
        | "infini-ai" | "302-ai" | "portkey" | "helicone" | "ant-ling" | "antdigital-dtmaas"
        | "inworld")
}

pub(super) fn provider_model_discovery_supported(provider: &ModelProviderConfig) -> bool {
    provider.model_catalog_url.is_some() || model_discovery_supported(&provider.provider)
}

/// Capture central inputs while the application read gate is held. External
/// credential helpers and HTTP run only after that gate has been released.
pub(crate) struct ModelDiscoveryInput {
    provider: ModelProviderConfig,
}

pub(crate) fn prepare_provider_discovery(provider_id: &str) -> Result<ModelDiscoveryInput, String> {
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    let provider = settings.model_providers.as_ref()
        .and_then(|providers| providers.get(provider_id)).cloned()
        .ok_or_else(|| format!("model_provider_not_found: Provider '{provider_id}' does not exist"))?;
    Ok(ModelDiscoveryInput { provider })
}

pub fn discover_provider_models(provider_id: &str) -> Result<Vec<ProviderModelSummary>, String> {
    execute_provider_discovery(prepare_provider_discovery(provider_id)?)
}

pub(crate) fn execute_provider_discovery(input: ModelDiscoveryInput) -> Result<Vec<ProviderModelSummary>, String> {
    let provider = &input.provider;
    let spec = discovery_spec_for_provider(provider)?;
    let stored_credential = if provider.auth_requirement != AuthRequirement::None
        && matches!(provider.api_key_source, None | Some(ApiKeySource::ArcaStore)) {
        super::read_runtime_provider_credential(&provider.id)?.map(Zeroizing::new)
    } else { None };
    let source = provider.api_key_source.clone().or_else(||
        stored_credential.is_some().then_some(ApiKeySource::ArcaStore));
    let credential = if provider.auth_requirement == AuthRequirement::None {
        None
    } else if matches!(source, Some(ApiKeySource::ArcaStore))
        && stored_credential.is_none() && provider.auth_requirement == AuthRequirement::Optional {
        None
    } else if let Some(source) = source.as_ref() {
        let resolved = super::credential::resolve_source(source,
            stored_credential.as_ref().map(|bytes| bytes.to_vec()))?;
        Some(Zeroizing::new(String::from_utf8(resolved.expose_for_delivery().to_vec())
            .map_err(|_| "model_provider_credential_invalid: API Key is not UTF-8".to_string())?))
    } else if provider.auth_requirement == AuthRequirement::Required {
        return Err("model_provider_credential_missing: Configure a credential source before loading the model catalog".into());
    } else { None };

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut url = discovery_url(provider, spec.adapter)?;
    let mut models = Vec::new();
    for page_index in 0..MAX_PAGES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("model_discovery_timeout: model catalog deadline exceeded".into());
        }
        let agent = discovery_agent_with_timeout(remaining.min(Duration::from_secs(15)))?;
        let mut page = fetch_page(
            &agent,
            &url,
            spec.adapter,
            credential.as_ref().map(|value| value.as_str()),
        )?;
        // The Workers AI marketplace endpoint has page/per_page parameters,
        // but no documented continuation envelope. A full page requires the
        // next page; repeated pages fail closed instead of returning a partial
        // catalog when a proxy ignores pagination.
        if spec.adapter == DiscoveryAdapter::Cloudflare
            && page.models.len() == CLOUDFLARE_PAGE_SIZE
        {
            page.next_token = Some((page_index + 2).to_string());
        }
        let prior_count = models.len();
        merge_models(&mut models, page.models)?;
        if spec.adapter == DiscoveryAdapter::Cloudflare
            && page.next_token.is_some() && models.len() == prior_count
        {
            return Err("model_discovery_invalid_response: Provider catalog pagination did not advance".into());
        }
        let Some(next_token) = page.next_token else {
            return Ok(models);
        };
        if page_index + 1 == MAX_PAGES {
            return Err(format!(
                "model_discovery_too_many_pages: Provider catalog exceeded the {MAX_PAGES}-page safety limit"
            ));
        }
        url = next_page_url(url, spec.adapter, &next_token)?;
    }
    Ok(models)
}

fn reviewed_discovery_spec(provider_type: &str) -> Option<DiscoverySpec> {
    let adapter = match provider_type {
        "openrouter"
        | "ppio"
        | "aihubmix"
        | "opencode-zen"
        | "opencode-go"
        | "kilo-gateway"
        | "nanogpt"
        | "synthetic"
        | "chutes"
        | "venice"
        | "friendli"
        | "inference-net"
        | "ovhcloud"
        | "zenmux"
        | "longcat"
        | "io-net"
        | "inception"
        | "ollama-cloud"
        | "openai"
        | "xai"
        | "routeway"
        | "infron"
        | "apinex"
        | "tokenharbor"
        | "flock"
        | "codecraft"
        | "beatapi"
        | "experiential-labs"
        | "runanywhere"
        | "coralbricks"
        | "meta"
        | "maximo-ai"
        | "krafton-raon"
        | "clean-apis"
        | "dolphin"
        | "iteracompute"
        | "llmtr"
        | "munito"
        | "nous-portal"
        | "ppq"
        | "river"
        | "trustedrouter"
        | "unorouter"
        | "orcarouter"
        | "fikra"
        | "constellation-gate"
        | "unisound"
        | "bilibili-index-translate"
        | "sensenova"
        | "akashml"
        | "0g-private-computer"
        | "gonkarouter"
        | "runinfra"
        | "nomask"
        | "anyrouter"
        | "yeta"
        | "scnet"
        | "oxion-ai"
        | "blazerail"
        | "larprouter"
        | "mango-inference"
        | "darkbloom"
        | "scaledown"
        | "amd-token-factory"
        | "subq"
        | "swarms"
        | "mistral"
        | "deepseek"
        | "groq"
        | "gmi-cloud"
        | "alibaba"
        | "alibaba-international"
        | "amazon-bedrock-mantle"
        | "sambanova"
        | "minimax"
        | "minimax-cn"
        | "moonshotai-cn"
        | "siliconflow-cn"
        | "alibaba-coding-plan-cn"
        | "alibaba-coding-plan"
        | "alibaba-token-plan-cn"
        | "alibaba-token-plan"
        | "xiaomi"
        | "xiaomi-token-plan-cn"
        | "xiaomi-token-plan-sgp"
        | "xiaomi-token-plan-ams"
        | "moonshotai"
        | "kimi-for-coding"
        | "zai"
        | "zai-coding-plan"
        | "zhipuai-coding-plan"
        | "minimax-coding-plan"
        | "minimax-cn-coding-plan"
        | "stepfun-step-plan"
        | "stepfun-ai-step-plan"
        | "tencent-coding-plan"
        | "tencent-token-plan"
        | "tencent-token-plan-global"
        | "nvidia"
        | "cerebras"
        | "siliconflow"
        | "together"
        | "poe"
        | "huggingface"
        | "novita-ai"
        | "qiniu-ai"
        | "digitalocean"
        | "modelscope"
        | "scaleway"
        | "nebius"
        | "requesty"
        | "baseten"
        | "ollama"
        | "lm-studio"
        | "vllm" => DiscoveryAdapter::OpenAi,
        "anthropic" => DiscoveryAdapter::Anthropic,
        "google" => DiscoveryAdapter::Gemini,
        "cohere" => DiscoveryAdapter::Cohere,
        "fireworks" => DiscoveryAdapter::Fireworks,
        "cloudflare-workers-ai" => DiscoveryAdapter::Cloudflare,
        "deepinfra" => DiscoveryAdapter::DeepInfra,
        "vercel-ai-gateway" => DiscoveryAdapter::Vercel,
        _ => return None,
    };
    let credential = if matches!(
        provider_type,
        "openrouter"
            | "ppio"
            | "aihubmix"
            | "opencode-zen"
            | "opencode-go"
            | "kilo-gateway"
            | "nanogpt"
            | "synthetic"
            | "chutes"
            | "venice"
            | "friendli"
            | "inference-net"
            | "ovhcloud"
            | "zenmux"
            | "io-net"
            | "inception"
            | "ollama-cloud"
            | "maximo-ai"
            | "iteracompute"
            | "llmtr"
            | "munito"
            | "nous-portal"
            | "ppq"
            | "trustedrouter"
            | "orcarouter"
            | "fikra"
            | "constellation-gate"
            | "0g-private-computer"
            | "bilibili-index-translate"
            | "anyrouter"
            | "blazerail"
            | "subq"
            | "alibaba-coding-plan-cn"
            | "alibaba-coding-plan"
            | "nvidia"
            | "poe"
            | "huggingface"
            | "modelscope"
            | "requesty"
            | "ollama"
            | "lm-studio"
            | "vllm"
    ) {
        CredentialPolicy::Optional
    } else {
        CredentialPolicy::Required
    };
    Some(DiscoverySpec {
        adapter,
        credential,
    })
}

fn discovery_spec_for_provider(provider: &ModelProviderConfig) -> Result<DiscoverySpec, String> {
    if !provider_model_discovery_supported(provider) {
        return Err("model_discovery_unsupported: Enter the model or deployment name manually, or configure a Models list URL".into());
    }
    if provider.provider == "cloudflare-workers-ai" && provider.model_catalog_url.is_some() {
        return Ok(DiscoverySpec { adapter: DiscoveryAdapter::OpenAi, credential: CredentialPolicy::Required });
    }
    if let Some(spec) = reviewed_discovery_spec(&provider.provider) {
        return Ok(spec);
    }
    let adapter = if provider
        .protocols
        .contains_key(&ModelProtocol::OpenaiResponses)
        || provider
            .protocols
            .contains_key(&ModelProtocol::OpenaiCompletions)
    {
        DiscoveryAdapter::OpenAi
    } else if provider
        .protocols
        .contains_key(&ModelProtocol::AnthropicMessages)
    {
        DiscoveryAdapter::Anthropic
    } else if provider
        .protocols
        .contains_key(&ModelProtocol::GeminiGenerateContent)
    {
        DiscoveryAdapter::Gemini
    } else {
        return Err(
            "model_discovery_endpoint_invalid: Provider has no configured protocol endpoint".into(),
        );
    };
    Ok(DiscoverySpec {
        adapter,
        credential: CredentialPolicy::Optional,
    })
}

fn discovery_url(provider: &ModelProviderConfig, adapter: DiscoveryAdapter) -> Result<Url, String> {
    if let Some(model_catalog_url) = provider.model_catalog_url.as_deref() {
        return Url::parse(model_catalog_url).map_err(|_| {
            "model_discovery_endpoint_invalid: Provider Models list URL is invalid".into()
        });
    }
    match adapter {
        DiscoveryAdapter::OpenAi | DiscoveryAdapter::Vercel | DiscoveryAdapter::DeepInfra => [
            // Prefer the Provider's reviewed OpenAI protocol. An additional
            // generic Responses path must not discard a plan/tenant prefix.
            super::list_providers().iter().find(|template| template.id == provider.provider)
                .map(|template| template.default_protocol.clone())
                .filter(|protocol| matches!(protocol, ModelProtocol::OpenaiResponses | ModelProtocol::OpenaiCompletions)),
            Some(ModelProtocol::OpenaiResponses),
            Some(ModelProtocol::OpenaiCompletions),
        ]
        .into_iter()
        .flatten()
        .find_map(|protocol| {
            provider
                .protocols
                .get(&protocol)
                .map(|_| derived_protocol_url(provider, protocol, "/models"))
        })
        .unwrap_or_else(|| {
            Err("model_discovery_endpoint_invalid: Provider has no reviewed OpenAI endpoint".into())
        })
        .map(|mut url| {
            if provider.provider == "siliconflow-cn" {
                url.query_pairs_mut().append_pair("sub_type", "chat");
            }
            url
        }),
        DiscoveryAdapter::Anthropic => {
            let mut url =
                derived_protocol_url(provider, ModelProtocol::AnthropicMessages, "/v1/models")?;
            url.query_pairs_mut().append_pair("limit", "1000");
            Ok(url)
        }
        DiscoveryAdapter::Gemini => {
            let mut url =
                derived_protocol_url(provider, ModelProtocol::GeminiGenerateContent, "/models")?;
            url.query_pairs_mut().append_pair("pageSize", "1000");
            Ok(url)
        }
        DiscoveryAdapter::Cohere => fixed_origin_url(provider, "/v1/models"),
        DiscoveryAdapter::Fireworks => {
            let mut url = fixed_origin_url(provider, "/v1/accounts/fireworks/models")?;
            url.query_pairs_mut()
                .append_pair("filter", "supports_serverless=true")
                .append_pair("pageSize", "200");
            Ok(url)
        }
        DiscoveryAdapter::Cloudflare => {
            let mut url = derived_protocol_url(provider, ModelProtocol::OpenaiCompletions, "/models")?;
            let path = url.path().strip_suffix("/ai/v1/models").ok_or_else(|| {
                "model_discovery_endpoint_invalid: Workers AI requires an account endpoint ending in /ai/v1".to_owned()
            })?;
            let catalog_path = format!("{path}/ai/models/search");
            url.set_path(&catalog_path);
            url.query_pairs_mut()
                .append_pair("format", "openrouter")
                .append_pair("task", "Text Generation")
                .append_pair("per_page", &CLOUDFLARE_PAGE_SIZE.to_string())
                .append_pair("page", "1");
            Ok(url)
        }
    }
}

fn derived_protocol_url(
    provider: &ModelProviderConfig,
    protocol: ModelProtocol,
    replacement: &str,
) -> Result<Url, String> {
    let endpoint = provider.protocols.get(&protocol).ok_or_else(|| {
        "model_discovery_endpoint_invalid: Provider has no reviewed protocol endpoint".to_owned()
    })?;
    let request_url = full_request_url(&provider.base_url, &endpoint.endpoint_path)
        .map_err(|_| "model_discovery_endpoint_invalid: Provider endpoint is invalid".to_owned())?;
    let operation = protocol.default_endpoint_path();
    let prefix = request_url.strip_suffix(operation).ok_or_else(|| {
        "model_discovery_endpoint_invalid: Provider endpoint does not end with the reviewed protocol operation"
            .to_owned()
    })?;
    Url::parse(&format!("{prefix}{replacement}")).map_err(|_| {
        "model_discovery_endpoint_invalid: Provider catalog URL could not be derived".into()
    })
}

fn fixed_origin_url(provider: &ModelProviderConfig, path: &str) -> Result<Url, String> {
    let mut url = Url::parse(&provider.base_url)
        .map_err(|_| "model_discovery_endpoint_invalid: Provider Base URL is invalid".to_owned())?;
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn decode_page(adapter: DiscoveryAdapter, value: Value) -> Result<DecodedPage, String> {
    let next_token = match adapter {
        DiscoveryAdapter::Anthropic
            if value.get("has_more").and_then(Value::as_bool) == Some(true) =>
        {
            required_token(value.get("last_id"))?
        }
        DiscoveryAdapter::Gemini => optional_token(value.get("nextPageToken"))?,
        DiscoveryAdapter::Cohere => optional_token(
            value
                .get("next_page_token")
                .or_else(|| value.get("nextPageToken")),
        )?,
        DiscoveryAdapter::Fireworks => optional_token(value.get("nextPageToken"))?,
        _ => None,
    };
    let entries = match adapter {
        DiscoveryAdapter::OpenAi | DiscoveryAdapter::Vercel | DiscoveryAdapter::DeepInfra | DiscoveryAdapter::Cloudflare => {
            if let Some(entries) = value.as_array() {
                entries
            } else {
                value.get("data").and_then(Value::as_array).ok_or_else(|| {
                    "model_discovery_invalid_response: Provider returned an invalid model catalog"
                        .to_owned()
                })?
            }
        }
        DiscoveryAdapter::Anthropic => {
            value.get("data").and_then(Value::as_array).ok_or_else(|| {
                "model_discovery_invalid_response: Anthropic returned an invalid model catalog"
                    .to_owned()
            })?
        }
        DiscoveryAdapter::Gemini | DiscoveryAdapter::Cohere | DiscoveryAdapter::Fireworks => value
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                "model_discovery_invalid_response: Provider returned an invalid model catalog"
                    .to_owned()
            })?,
    };

    let models: Vec<_> = entries
        .iter()
        .filter(|entry| adapter != DiscoveryAdapter::Vercel
            || entry.get("type").and_then(Value::as_str).is_none_or(|kind| kind == "language"))
        .filter(|entry| {
            adapter != DiscoveryAdapter::DeepInfra
                || entry.pointer("/metadata/tags").and_then(Value::as_array)
                    .is_none_or(|tags| tags.iter().any(|tag| tag.as_str() == Some("chat")))
        })
        .filter(|entry| {
            adapter != DiscoveryAdapter::Gemini
                || entry
                    .get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_some_and(|methods| {
                        methods
                            .iter()
                            .any(|method| method.as_str() == Some("generateContent"))
                    })
        })
        .filter_map(|entry| model_summary(adapter, entry))
        .collect();
    // A malformed Cloudflare ID must not make a full page appear to be the
    // final short page. This adapter requests the documented marketplace shape.
    if adapter == DiscoveryAdapter::Cloudflare && models.len() != entries.len() {
        return Err("model_discovery_invalid_response: Workers AI returned an invalid marketplace model ID".into());
    }
    Ok(DecodedPage { models, next_token })
}

fn model_summary(adapter: DiscoveryAdapter, value: &Value) -> Option<ProviderModelSummary> {
    let id = match adapter {
        DiscoveryAdapter::OpenAi | DiscoveryAdapter::Anthropic
        | DiscoveryAdapter::Cloudflare | DiscoveryAdapter::Vercel | DiscoveryAdapter::DeepInfra => string_field(value, &["id"]),
        DiscoveryAdapter::Gemini => string_field(value, &["baseModelId"]).or_else(|| {
            string_field(value, &["name"]).map(|name| {
                name.strip_prefix("models/")
                    .unwrap_or(name.as_str())
                    .to_owned()
            })
        }),
        DiscoveryAdapter::Cohere | DiscoveryAdapter::Fireworks => string_field(value, &["name"]),
    }?;
    let name = string_field(value, &["display_name", "displayName", "title", "name"]);
    let context_length = integer_field(
        value,
        &[
            "context_length",
            "context_window",
            "context_size",
            "max_context_length",
            "max_input_tokens",
            "inputTokenLimit",
            "contextLength",
        ],
    ).or_else(|| (adapter == DiscoveryAdapter::DeepInfra)
        .then(|| value.get("metadata").and_then(|metadata| integer_field(metadata, &["context_length"])))
        .flatten());
    Some(ProviderModelSummary {
        id,
        name,
        context_length,
    })
}

fn string_field(value: &Value, fields: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        let value = value.get(*field)?.as_str()?.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn integer_field(value: &Value, fields: &[&str]) -> Option<u64> {
    fields.iter().find_map(|field| {
        let value = value.get(*field)?;
        value
            .as_u64()
            .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
    })
}

fn optional_token(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let token = value.as_str().ok_or_else(|| {
        "model_discovery_invalid_response: Provider returned an invalid continuation token"
            .to_owned()
    })?;
    let token = token.trim();
    if token.is_empty() {
        return Ok(None);
    }
    if token.len() > 2_048 || token.chars().any(char::is_control) {
        return Err(
            "model_discovery_invalid_response: Provider returned an invalid continuation token"
                .into(),
        );
    }
    Ok(Some(token.to_owned()))
}

fn required_token(value: Option<&Value>) -> Result<Option<String>, String> {
    optional_token(value)?.map(Some).ok_or_else(|| {
        "model_discovery_invalid_response: Provider omitted its continuation token".into()
    })
}

fn merge_models(
    target: &mut Vec<ProviderModelSummary>,
    incoming: Vec<ProviderModelSummary>,
) -> Result<(), String> {
    let mut seen = target
        .iter()
        .map(|model| model.id.clone())
        .collect::<BTreeSet<_>>();
    for mut model in incoming {
        model.id = model.id.trim().to_owned();
        if model.id.is_empty() || !seen.insert(model.id.clone()) {
            continue;
        }
        if target.len() == MAX_MODELS {
            return Err(format!(
                "model_discovery_too_many_models: Provider catalog exceeded the {MAX_MODELS}-model safety limit"
            ));
        }
        model.name = model
            .name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty());
        target.push(model);
    }
    Ok(())
}

fn read_bounded(mut reader: impl Read, maximum: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let remaining = maximum.saturating_add(1).saturating_sub(total);
        let requested = buffer.len().min(remaining as usize);
        if requested == 0 {
            return Err(format!(
                "model_discovery_response_too_large: Provider response exceeded the {maximum}-byte safety limit"
            ));
        }
        let read = reader.read(&mut buffer[..requested]).map_err(|_| {
            "model_discovery_network: Provider response ended unexpectedly".to_owned()
        })?;
        if read == 0 {
            return Ok(bytes);
        }
        total = total.saturating_add(read as u64);
        if total > maximum {
            return Err(format!(
                "model_discovery_response_too_large: Provider response exceeded the {maximum}-byte safety limit"
            ));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
fn discovery_agent() -> Result<ureq::Agent, String> {
    discovery_agent_with_timeout(Duration::from_secs(15))
}

fn discovery_agent_with_timeout(timeout: Duration) -> Result<ureq::Agent, String> {
    crate::network::build_ureq_agent(
        ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .user_agent("arca-provider-model-discovery"),
    )
}

fn fetch_page(
    agent: &ureq::Agent,
    url: &Url,
    adapter: DiscoveryAdapter,
    credential: Option<&str>,
) -> Result<DecodedPage, String> {
    let authorization = matches!(
        adapter,
        DiscoveryAdapter::OpenAi | DiscoveryAdapter::Cohere | DiscoveryAdapter::Fireworks
        | DiscoveryAdapter::Cloudflare | DiscoveryAdapter::Vercel | DiscoveryAdapter::DeepInfra
    )
    .then(|| credential.map(|credential| Zeroizing::new(format!("Bearer {credential}"))))
    .flatten();
    let mut request = agent.get(url.as_str()).header("Accept", "application/json");
    if let Some(authorization) = authorization.as_ref() {
        request = request.header("Authorization", authorization.as_str());
    }
    if let Some(credential) = credential {
        match adapter {
            DiscoveryAdapter::Anthropic => {
                request = request
                    .header("X-Api-Key", credential)
                    .header("anthropic-version", "2023-06-01");
            }
            DiscoveryAdapter::Gemini => {
                request = request.header("x-goog-api-key", credential);
            }
            _ => {}
        }
    }
    let mut response = request.call().map_err(|_| {
        "model_discovery_network: Unable to reach the Provider model catalog".to_owned()
    })?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!(
            "model_discovery_http: Provider model catalog returned HTTP {status}"
        ));
    }
    let bytes = read_bounded(response.body_mut().as_reader(), MAX_RESPONSE_BYTES)?;
    let value = serde_json::from_slice(&bytes).map_err(|_| {
        "model_discovery_invalid_response: Provider returned invalid JSON".to_owned()
    })?;
    decode_page(adapter, value)
}

fn next_page_url(mut url: Url, adapter: DiscoveryAdapter, token: &str) -> Result<Url, String> {
    let key = match adapter {
        DiscoveryAdapter::Anthropic => "after_id",
        DiscoveryAdapter::Gemini | DiscoveryAdapter::Fireworks => "pageToken",
        DiscoveryAdapter::Cohere => "page_token",
        DiscoveryAdapter::Cloudflare => "page",
        DiscoveryAdapter::OpenAi | DiscoveryAdapter::Vercel | DiscoveryAdapter::DeepInfra => {
            return Err(
                "model_discovery_invalid_response: OpenAI-compatible catalog returned an unsupported continuation token"
                    .into(),
            )
        }
    };
    let existing = url
        .query_pairs()
        .filter(|(name, _)| name != key)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs(existing);
        query.append_pair(key, token);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::{ModelProtocol, ModelProviderProtocolConfig};
    use crate::testenv::TestHome;
    use std::collections::BTreeMap;
    use std::io::{Cursor, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn workers_ai_uses_account_catalog_marketplace_ids_and_bearer_auth() {
        let _home = TestHome::new("cloudflare-model-catalog");
        let mut config = provider("cloudflare-workers-ai", "https://api.cloudflare.com/client/v4/accounts/tenant/ai/v1",
            ModelProtocol::OpenaiCompletions, "/chat/completions");
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::Cloudflare);
        let url = discovery_url(&config, spec.adapter).unwrap();
        assert_eq!(url.path(), "/client/v4/accounts/tenant/ai/models/search");
        let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["format"], "openrouter");
        assert_eq!(query["task"], "Text Generation");
        let next = next_page_url(url.clone(), spec.adapter, "2").unwrap();
        assert_eq!(next.host_str(), url.host_str());
        assert_eq!(next.query_pairs().filter(|(key, _)| key == "page").count(), 1);
        assert!(next.query_pairs().any(|(key, value)| key == "page" && value == "2"));

        let (local, request, server) = serve_once("200 OK", &[],
            r#"{"data":[{"id":"@cf/vendor/model","name":"Model","context_length":32768}]}"#);
        let page = fetch_page(&discovery_agent().unwrap(), &local, spec.adapter, Some("fixture-token")).unwrap();
        assert_eq!(page.models[0].id, "@cf/vendor/model");
        assert_eq!(page.models[0].context_length, Some(32768));
        assert!(request.recv().unwrap().to_lowercase().contains("authorization: bearer fixture-token"));
        server.join().unwrap();
        assert!(decode_page(spec.adapter, serde_json::json!({"data":[{"name":"no-id"}]})).is_err());
        assert!(decode_page(spec.adapter, serde_json::json!({"success":false,"errors":[]})).is_err());

        config.model_catalog_url = Some("https://proxy.example.test/catalog".into());
        let overridden = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(overridden.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(discovery_url(&config, overridden.adapter).unwrap().as_str(), "https://proxy.example.test/catalog");
    }

    #[test]
    fn deepinfra_reads_nested_context_and_omits_non_chat_models() {
        let page = decode_page(DiscoveryAdapter::DeepInfra, serde_json::json!({"data":[
            {"id":"vendor/chat","metadata":{"context_length":131072,"tags":["chat","reasoning"]}},
            {"id":"vendor/image","metadata":{"tags":["text-to-image"]}}
        ]})).unwrap();
        assert_eq!(page.models.len(), 1);
        assert_eq!(page.models[0].id, "vendor/chat");
        assert_eq!(page.models[0].context_length, Some(131072));
    }

    #[test]
    fn vercel_catalog_excludes_non_language_models_and_keeps_context() {
        let page = decode_page(DiscoveryAdapter::Vercel, serde_json::json!({"data":[
            {"id":"vendor/chat","type":"language","context_window":200000},
            {"id":"vendor/video","type":"video"},
            {"id":"vendor/embed","type":"embedding"},
            {"id":"vendor/speech","type":"speech"}
        ]})).unwrap();
        assert_eq!(page.models.len(), 1);
        assert_eq!(page.models[0].id, "vendor/chat");
        assert_eq!(page.models[0].context_length, Some(200000));
    }

    #[test]
    fn siliconflow_china_requests_chat_catalog_without_changing_explicit_urls() {
        let mut config = provider("siliconflow-cn", "https://api.siliconflow.cn/v1",
            ModelProtocol::OpenaiCompletions, "/chat/completions");
        let url = discovery_url(&config, DiscoveryAdapter::OpenAi).unwrap();
        assert_eq!(url.as_str(), "https://api.siliconflow.cn/v1/models?sub_type=chat");
        config.model_catalog_url = Some("https://catalog.example.test/models?scope=personal".into());
        assert_eq!(discovery_url(&config, DiscoveryAdapter::OpenAi).unwrap().as_str(),
            "https://catalog.example.test/models?scope=personal");
    }

    #[test]
    fn stark_relay_has_no_reviewed_catalog_spec_and_falls_back_to_the_openai_models_path() {
        assert!(reviewed_discovery_spec("stark-relay").is_none());
        let config = provider(
            "stark-relay",
            "https://starkrelay.bond",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://starkrelay.bond/v1/models"
        );
    }

    #[test]
    fn inferx_has_no_reviewed_catalog_spec_and_falls_back_to_the_openai_models_path() {
        assert!(reviewed_discovery_spec("inferx").is_none());
        let config = provider(
            "inferx",
            "https://model.inferx.net/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://model.inferx.net/v1/models"
        );
    }

    #[test]
    fn blazerail_discovers_the_public_openai_catalog() {
        let config = provider(
            "blazerail",
            "https://api.blazerail.com/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.blazerail.com/v1/models"
        );
    }

    #[test]
    fn larprouter_discovers_openai_models_on_the_documented_catalog_path() {
        let config = provider(
            "larprouter",
            "https://larprouter.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://larprouter.com/v1/models"
        );
    }

    #[test]
    fn oxion_ai_discovers_openai_models_on_the_documented_catalog_path() {
        let config = provider(
            "oxion-ai",
            "https://oxionai.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://oxionai.com/v1/models"
        );
    }

    #[test]
    fn scnet_discovers_openai_models_on_the_documented_catalog_path() {
        let config = provider(
            "scnet",
            "https://api.scnet.cn",
            ModelProtocol::OpenaiCompletions,
            "/api/llm/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.scnet.cn/api/llm/v1/models"
        );
    }

    #[test]
    fn manual_catalogs_require_an_explicit_override_before_discovery() {
        for id in ["azure-openai", "perplexity", "baidu-qianfan-coding-plan"] {
            let mut config = provider(id, "https://tenant.example.test/openai/v1",
                ModelProtocol::OpenaiResponses, "/responses");
            assert!(discovery_spec_for_provider(&config).unwrap_err().contains("model_discovery_unsupported"));
            config.model_catalog_url = Some("https://tenant.example.test/deployment-models".into());
            assert!(provider_model_discovery_supported(&config));
            let spec = discovery_spec_for_provider(&config).unwrap();
            assert_eq!(discovery_url(&config, spec.adapter).unwrap().path(), "/deployment-models");
        }
    }

    #[test]
    fn step_plan_catalog_uses_the_reviewed_chat_prefix_before_generic_responses() {
        for (id, origin) in [("stepfun-step-plan", "https://api.stepfun.com"),
            ("stepfun-ai-step-plan", "https://api.stepfun.ai")] {
            let mut config = provider(id, origin, ModelProtocol::OpenaiResponses, "/responses");
            config.protocols.insert(ModelProtocol::OpenaiCompletions,
                crate::domain::types::ModelProviderProtocolConfig { endpoint_path: "/step_plan/v1/chat/completions".into() });
            let spec = discovery_spec_for_provider(&config).unwrap();
            assert_eq!(discovery_url(&config, spec.adapter).unwrap().as_str(),
                format!("{origin}/step_plan/v1/models"));
            config.model_catalog_url = Some(format!("{origin}/custom-catalog"));
            assert_eq!(discovery_url(&config, spec.adapter).unwrap().as_str(),
                format!("{origin}/custom-catalog"));
        }
    }

    const EXPECTED_OPENAI_COMPATIBLE: &[&str] = &[
        "openrouter",
        "openai",
        "xai",
        "routeway",
        "infron",
        "apinex",
        "tokenharbor",
        "flock",
        "codecraft",
        "beatapi",
        "experiential-labs",
        "runanywhere",
        "coralbricks",
        "meta",
        "maximo-ai",
        "krafton-raon",
        "clean-apis",
        "dolphin",
        "iteracompute",
        "llmtr",
        "munito",
        "nous-portal",
        "ppq",
        "river",
        "trustedrouter",
        "unorouter",
        "orcarouter",
        "fikra",
        "constellation-gate",
        "unisound",
        "bilibili-index-translate",
        "sensenova",
        "akashml",
        "0g-private-computer",
        "gonkarouter",
        "runinfra",
        "nomask",
        "anyrouter",
        "yeta",
        "scnet",
        "oxion-ai",
        "blazerail",
        "larprouter",
        "mango-inference",
        "darkbloom",
        "scaledown",
        "amd-token-factory",
        "subq",
        "swarms",
        "mistral",
        "deepseek",
        "groq",
        "gmi-cloud",
        "alibaba",
        "alibaba-coding-plan-cn",
        "alibaba-coding-plan",
        "alibaba-token-plan-cn",
        "alibaba-token-plan",
        "xiaomi",
        "xiaomi-token-plan-cn",
        "xiaomi-token-plan-sgp",
        "xiaomi-token-plan-ams",
        "moonshotai",
        "kimi-for-coding",
        "zai",
        "zai-coding-plan",
        "zhipuai-coding-plan",
        "minimax-coding-plan",
        "minimax-cn-coding-plan",
        "stepfun-step-plan",
        "stepfun-ai-step-plan",
        "tencent-coding-plan",
        "tencent-token-plan",
        "tencent-token-plan-global",
        "nvidia",
        "cerebras",
        "siliconflow",
        "together",
        "poe",
        "huggingface",
        "novita-ai",
        "qiniu-ai",
        "digitalocean",
        "modelscope",
        "scaleway",
        "nebius",
        "requesty",
        "baseten",
        "ollama",
        "lm-studio",
        "vllm",
    ];

    fn provider(
        provider_type: &str,
        base_url: &str,
        protocol: ModelProtocol,
        endpoint_path: &str,
    ) -> ModelProviderConfig {
        ModelProviderConfig {
            id: format!("{provider_type}-instance"),
            name: format!("{provider_type} instance"),
            provider: provider_type.into(),
            base_url: base_url.into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                protocol,
                ModelProviderProtocolConfig {
                    endpoint_path: endpoint_path.into(),
                },
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        }
    }

    fn serve_once(
        status: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> (Url, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        let status = status.to_owned();
        let headers = headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect::<Vec<_>>();
        let body = body.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .unwrap();
            let mut response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len(),
            );
            for (name, value) in headers {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(&body);
            stream.write_all(response.as_bytes()).unwrap();
        });
        (
            Url::parse(&format!("http://{address}/models")).unwrap(),
            receiver,
            handle,
        )
    }

    #[test]
    fn distinguishes_manual_catalogs_from_compatible_custom_providers() {
        for id in ["github-models", "azure-openai", "perplexity", "volcengine-coding-plan", "baidu-qianfan",
            "ant-ling", "antdigital-dtmaas", "inworld"] {
            assert!(!model_discovery_supported(id), "{id}");
        }
        for provider_type in ["wandb", "custom", "private-gateway", "deepinfra", "amazon-bedrock-mantle"] {
            assert!(model_discovery_supported(provider_type), "{provider_type}");
        }
    }

    #[test]
    fn assigns_the_expected_adapter_and_credential_policy() {
        for provider_type in EXPECTED_OPENAI_COMPATIBLE {
            assert_eq!(
                reviewed_discovery_spec(provider_type).map(|spec| spec.adapter),
                Some(DiscoveryAdapter::OpenAi),
                "{provider_type}",
            );
        }
        assert_eq!(
            reviewed_discovery_spec("anthropic").map(|spec| spec.adapter),
            Some(DiscoveryAdapter::Anthropic),
        );
        assert_eq!(
            reviewed_discovery_spec("google").map(|spec| spec.adapter),
            Some(DiscoveryAdapter::Gemini),
        );
        assert_eq!(
            reviewed_discovery_spec("cohere").map(|spec| spec.adapter),
            Some(DiscoveryAdapter::Cohere),
        );
        assert_eq!(
            reviewed_discovery_spec("fireworks").map(|spec| spec.adapter),
            Some(DiscoveryAdapter::Fireworks),
        );

        for provider_type in [
            "openrouter",
            "alibaba-coding-plan-cn",
            "alibaba-coding-plan",
            "nvidia",
            "poe",
            "huggingface",
            "modelscope",
            "requesty",
            "ollama",
            "lm-studio",
            "vllm",
            "maximo-ai",
            "iteracompute",
            "llmtr",
            "munito",
            "nous-portal",
            "ppq",
            "trustedrouter",
            "orcarouter",
            "fikra",
            "constellation-gate",
            "0g-private-computer",
            "bilibili-index-translate",
            "anyrouter",
            "blazerail",
            "subq",
        ] {
            assert_eq!(
                reviewed_discovery_spec(provider_type).map(|spec| spec.credential),
                Some(CredentialPolicy::Optional),
                "{provider_type}",
            );
        }
        for provider_type in ["openai", "gmi-cloud", "clean-apis", "dolphin", "river", "unisound", "yeta", "scnet", "oxion-ai", "larprouter", "mango-inference", "darkbloom", "scaledown", "amd-token-factory", "swarms"] {
            assert_eq!(
                reviewed_discovery_spec(provider_type).map(|spec| spec.credential),
                Some(CredentialPolicy::Required),
            );
        }
    }

    #[test]
    fn derives_optional_generic_discovery_from_custom_provider_protocols() {
        let openai = provider(
            "custom",
            "https://gateway.example.test",
            ModelProtocol::OpenaiResponses,
            "/tenant/v1/responses",
        );
        let openai_spec = discovery_spec_for_provider(&openai).unwrap();
        assert_eq!(openai_spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(openai_spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&openai, openai_spec.adapter)
                .unwrap()
                .as_str(),
            "https://gateway.example.test/tenant/v1/models",
        );

        let anthropic = provider(
            "private-anthropic",
            "https://gateway.example.test",
            ModelProtocol::AnthropicMessages,
            "/tenant/v1/messages",
        );
        let anthropic_spec = discovery_spec_for_provider(&anthropic).unwrap();
        assert_eq!(anthropic_spec.adapter, DiscoveryAdapter::Anthropic);
        assert_eq!(anthropic_spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&anthropic, anthropic_spec.adapter)
                .unwrap()
                .as_str(),
            "https://gateway.example.test/tenant/v1/models?limit=1000",
        );

        let gemini = provider(
            "private-gemini",
            "https://gateway.example.test/v1beta",
            ModelProtocol::GeminiGenerateContent,
            "/models/{model}:generateContent",
        );
        let gemini_spec = discovery_spec_for_provider(&gemini).unwrap();
        assert_eq!(gemini_spec.adapter, DiscoveryAdapter::Gemini);
        assert_eq!(gemini_spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&gemini, gemini_spec.adapter)
                .unwrap()
                .as_str(),
            "https://gateway.example.test/v1beta/models?pageSize=1000",
        );
    }

    #[test]
    fn custom_provider_prefers_its_explicit_model_catalog_url() {
        let provider: ModelProviderConfig = serde_json::from_value(serde_json::json!({
            "id": "custom-instance",
            "name": "Custom instance",
            "provider": "custom",
            "base_url": "https://gateway.example.test",
            "model_catalog_url": "https://catalog.example.test/api/models?scope=available",
            "protocols": {
                "openai-responses": { "endpoint_path": "/v1/responses" }
            }
        }))
        .unwrap();
        let spec = discovery_spec_for_provider(&provider).unwrap();

        assert_eq!(
            discovery_url(&provider, spec.adapter).unwrap().as_str(),
            "https://catalog.example.test/api/models?scope=available",
        );
    }

    #[test]
    fn derives_only_reviewed_same_origin_catalog_urls() {
        let openrouter = provider(
            "openrouter",
            "https://openrouter.ai",
            ModelProtocol::OpenaiResponses,
            "/api/v1/responses",
        );
        assert_eq!(
            discovery_url(&openrouter, DiscoveryAdapter::OpenAi)
                .unwrap()
                .as_str(),
            "https://openrouter.ai/api/v1/models",
        );

        let anthropic = provider(
            "anthropic",
            "https://api.anthropic.com",
            ModelProtocol::AnthropicMessages,
            "/v1/messages",
        );
        assert_eq!(
            discovery_url(&anthropic, DiscoveryAdapter::Anthropic)
                .unwrap()
                .as_str(),
            "https://api.anthropic.com/v1/models?limit=1000",
        );

        let gemini = provider(
            "google",
            "https://generativelanguage.googleapis.com/v1beta",
            ModelProtocol::GeminiGenerateContent,
            "/models/{model}:generateContent",
        );
        assert_eq!(
            discovery_url(&gemini, DiscoveryAdapter::Gemini)
                .unwrap()
                .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000",
        );

        let cohere = provider(
            "cohere",
            "https://api.cohere.ai/compatibility/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        assert_eq!(
            discovery_url(&cohere, DiscoveryAdapter::Cohere)
                .unwrap()
                .as_str(),
            "https://api.cohere.ai/v1/models",
        );

        let fireworks = provider(
            "fireworks",
            "https://api.fireworks.ai/inference/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        assert_eq!(
            discovery_url(&fireworks, DiscoveryAdapter::Fireworks)
                .unwrap()
                .as_str(),
            "https://api.fireworks.ai/v1/accounts/fireworks/models?filter=supports_serverless%3Dtrue&pageSize=200",
        );
    }

    #[test]
    fn normalizes_all_response_shapes_without_overwriting_first_seen_models() {
        let mut models = decode_page(
            DiscoveryAdapter::OpenAi,
            serde_json::json!({
                "data": [
                    {"id": "alpha", "name": "Alpha", "context_length": 128000},
                    {"id": "", "name": "Ignored"}
                ]
            }),
        )
        .unwrap()
        .models;
        merge_models(
            &mut models,
            decode_page(
                DiscoveryAdapter::OpenAi,
                serde_json::json!([
                    {"id": "alpha", "name": "Replacement"},
                    {"id": "beta", "display_name": "Beta", "context_window": 64000}
                ]),
            )
            .unwrap()
            .models,
        )
        .unwrap();

        assert_eq!(
            models,
            vec![
                ProviderModelSummary {
                    id: "alpha".into(),
                    name: Some("Alpha".into()),
                    context_length: Some(128000),
                },
                ProviderModelSummary {
                    id: "beta".into(),
                    name: Some("Beta".into()),
                    context_length: Some(64000),
                },
            ],
        );
    }

    #[test]
    fn native_adapters_extract_ids_capabilities_and_continuation_tokens() {
        let anthropic = decode_page(
            DiscoveryAdapter::Anthropic,
            serde_json::json!({
                "data": [{"id": "claude-sonnet", "display_name": "Claude Sonnet", "max_input_tokens": 200000}],
                "has_more": true,
                "last_id": "claude-sonnet"
            }),
        )
        .unwrap();
        assert_eq!(anthropic.next_token.as_deref(), Some("claude-sonnet"));
        assert_eq!(anthropic.models[0].id, "claude-sonnet");

        let gemini = decode_page(
            DiscoveryAdapter::Gemini,
            serde_json::json!({
                "models": [
                    {"name": "models/gemini-2.5-pro", "baseModelId": "gemini-2.5-pro", "displayName": "Gemini 2.5 Pro", "inputTokenLimit": 1048576, "supportedGenerationMethods": ["generateContent"]},
                    {"name": "models/embedding-001", "supportedGenerationMethods": ["embedContent"]}
                ],
                "nextPageToken": "next-gemini"
            }),
        )
        .unwrap();
        assert_eq!(gemini.next_token.as_deref(), Some("next-gemini"));
        assert_eq!(gemini.models.len(), 1);
        assert_eq!(gemini.models[0].id, "gemini-2.5-pro");

        let cohere = decode_page(
            DiscoveryAdapter::Cohere,
            serde_json::json!({"models": [{"name": "command-r-plus", "context_length": 128000}], "next_page_token": "next-cohere"}),
        )
        .unwrap();
        assert_eq!(cohere.next_token.as_deref(), Some("next-cohere"));
        assert_eq!(cohere.models[0].id, "command-r-plus");

        let fireworks = decode_page(
            DiscoveryAdapter::Fireworks,
            serde_json::json!({"models": [{"name": "accounts/fireworks/models/llama-v3", "displayName": "Llama 3", "contextLength": 8192}], "nextPageToken": "next-fireworks"}),
        )
        .unwrap();
        assert_eq!(fireworks.next_token.as_deref(), Some("next-fireworks"));
        assert_eq!(fireworks.models[0].id, "accounts/fireworks/models/llama-v3",);
    }

    #[test]
    fn enforces_response_page_and_model_limits() {
        assert_eq!(
            read_bounded(Cursor::new(vec![b'x'; 16]), 16).unwrap().len(),
            16
        );
        assert!(read_bounded(Cursor::new(vec![b'x'; 17]), 16)
            .unwrap_err()
            .starts_with("model_discovery_response_too_large:"));
        assert_eq!(MAX_RESPONSE_BYTES, 4 * 1024 * 1024);
        assert_eq!(MAX_MODELS, 2_000);
        assert_eq!(MAX_PAGES, 10);

        let mut models = (0..MAX_MODELS)
            .map(|index| ProviderModelSummary {
                id: format!("model-{index}"),
                name: None,
                context_length: None,
            })
            .collect::<Vec<_>>();
        let error = merge_models(
            &mut models,
            vec![ProviderModelSummary {
                id: "one-too-many".into(),
                name: None,
                context_length: None,
            }],
        )
        .unwrap_err();
        assert!(
            error.starts_with("model_discovery_too_many_models:"),
            "{error}"
        );
    }

    #[test]
    fn discovery_uses_file_source_and_honors_explicit_no_auth() {
        let home = TestHome::new("discovery-file-source");
        let path = home.home.join("key");
        std::fs::write(&path, b"fixture-file-key").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        for requirement in [AuthRequirement::Required, AuthRequirement::None] {
            let (url, request, handle) = serve_once("200 OK", &[], r#"{"data": []}"#);
            let mut config = provider("openai", url.as_str(), ModelProtocol::OpenaiResponses, "/responses");
            config.auth_requirement = requirement.clone();
            config.api_key_source = Some(ApiKeySource::File { path: path.to_string_lossy().into_owned() });
            execute_provider_discovery(ModelDiscoveryInput { provider: config }).unwrap();
            let request = request.recv().unwrap().to_ascii_lowercase();
            handle.join().unwrap();
            assert_eq!(request.contains("authorization: bearer fixture-file-key"), requirement == AuthRequirement::Required);
        }
    }

    #[test]
    fn sends_only_the_adapter_specific_authentication_headers() {
        let _home = TestHome::new("model-discovery-auth-headers");
        let agent = discovery_agent().unwrap();

        let (url, request, handle) = serve_once("200 OK", &[], r#"{"data": []}"#);
        fetch_page(
            &agent,
            &url,
            DiscoveryAdapter::OpenAi,
            Some("openai-secret"),
        )
        .unwrap();
        let request = request.recv().unwrap().to_ascii_lowercase();
        handle.join().unwrap();
        assert!(request.contains("authorization: bearer openai-secret\r\n"));

        let (url, request, handle) = serve_once("200 OK", &[], r#"{"data": []}"#);
        fetch_page(
            &agent,
            &url,
            DiscoveryAdapter::Anthropic,
            Some("anthropic-secret"),
        )
        .unwrap();
        let request = request.recv().unwrap().to_ascii_lowercase();
        handle.join().unwrap();
        assert!(request.contains("x-api-key: anthropic-secret\r\n"));
        assert!(request.contains("anthropic-version: 2023-06-01\r\n"));
        assert!(!request.contains("authorization:"));

        let (url, request, handle) = serve_once("200 OK", &[], r#"{"models": []}"#);
        fetch_page(
            &agent,
            &url,
            DiscoveryAdapter::Gemini,
            Some("gemini-secret"),
        )
        .unwrap();
        let request = request.recv().unwrap().to_ascii_lowercase();
        handle.join().unwrap();
        assert!(request.contains("x-goog-api-key: gemini-secret\r\n"));
        assert!(!request.contains("authorization:"));
    }

    #[test]
    fn refuses_redirects_and_never_echoes_provider_bodies_or_credentials() {
        let _home = TestHome::new("model-discovery-safe-errors");
        let agent = discovery_agent().unwrap();
        let (url, _request, handle) = serve_once(
            "302 Found",
            &[("Location", "http://127.0.0.1:9/credential-sink")],
            "redirected openai-secret",
        );
        let redirect = fetch_page(
            &agent,
            &url,
            DiscoveryAdapter::OpenAi,
            Some("openai-secret"),
        )
        .unwrap_err();
        handle.join().unwrap();
        assert!(redirect.starts_with("model_discovery_http:"), "{redirect}");
        assert!(redirect.contains("302"), "{redirect}");
        assert!(!redirect.contains("credential-sink"), "{redirect}");
        assert!(!redirect.contains("openai-secret"), "{redirect}");

        let (url, _request, handle) = serve_once(
            "401 Unauthorized",
            &[],
            "provider echoed openai-secret in its body",
        );
        let unauthorized = fetch_page(
            &agent,
            &url,
            DiscoveryAdapter::OpenAi,
            Some("openai-secret"),
        )
        .unwrap_err();
        handle.join().unwrap();
        assert!(
            unauthorized.starts_with("model_discovery_http:"),
            "{unauthorized}"
        );
        assert!(unauthorized.contains("401"), "{unauthorized}");
        assert!(!unauthorized.contains("provider echoed"), "{unauthorized}");
        assert!(!unauthorized.contains("openai-secret"), "{unauthorized}");
    }
}
