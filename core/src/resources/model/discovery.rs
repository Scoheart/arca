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
        "amazon-bedrock" | "github-models" | "azure-openai" | "perplexity" | "volcengine" | "volcengine-coding-plan"
        | "baidu-qianfan" | "baidu-qianfan-coding-plan" | "zhipuai"
        | "stepfun" | "stepfun-global"
        | "featherless-ai" | "upstage" | "ai21" | "byteplus"
        | "tencent-tokenhub-cn" | "tencent-tokenhub-global" | "iflytek-spark-x2" | "nscale" | "tinfoil-proxy"
        | "infini-ai" | "302-ai" | "portkey" | "helicone" | "ant-ling" | "antdigital-dtmaas"
        | "inworld")
}

pub(super) fn provider_model_discovery_supported(provider: &ModelProviderConfig) -> bool {
    provider.model_catalog_url.is_some() || (model_discovery_supported(&provider.provider)
        && provider.protocols.keys().any(|protocol| *protocol != ModelProtocol::AwsBedrockConverse))
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
    // A reviewed static list is the catalog when the provider has no Models
    // API. An explicit Models list URL still uses the generic decoder below.
    if provider.model_catalog_url.is_none() {
        if let Some(models) = static_seed_models(&provider.provider) {
            return Ok(models);
        }
    }
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
    } else if provider.auth_requirement == AuthRequirement::Required
        && (provider.model_catalog_url.is_some() || reviewed_static_catalog(&provider.provider).is_none()) {
        return Err("model_provider_credential_missing: Configure a credential source before loading the model catalog".into());
    } else { None };

    // Anonymous GET /v1/models is not a public catalog for these providers.
    // A saved credential still uses the reviewed live decoder below.
    if credential.is_none() && provider.model_catalog_url.is_none() {
        if let Some(models) = reviewed_static_catalog(&provider.provider) {
            return Ok(models);
        }
    }

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

/// Documented model ids used when the provider's catalog is not public.
/// Ids are the current published directory, plus docs aliases that directory omits.
const NARAROUTER_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("agnes-3-flash", Some("Agnes 3 Flash"), Some(1_000_000)),
    ("claude-fable-5.1", Some("Claude Fable 5.1"), Some(1_000_000)),
    ("claude-haiku-5.5", Some("Claude Haiku 5.5"), Some(1_000_000)),
    ("claude-opus-4.8", Some("Claude Opus 4.8"), Some(1_000_000)),
    ("claude-opus-5", Some("Claude Opus 5"), Some(1_000_000)),
    ("claude-opus-5.5", Some("Claude Opus 5.5"), Some(1_000_000)),
    ("claude-sonnet-5", Some("Claude Sonnet 5"), Some(1_000_000)),
    ("claude-sonnet-5.5", Some("Claude Sonnet 5.5"), Some(1_000_000)),
    ("deepseek-v4.1-flash-abliterated", Some("DeepSeek v4.1 Flash Abliterated"), Some(1_000_000)),
    ("deepseek-v4.1-flash-fast", Some("DeepSeek v4.1 Flash Fast"), Some(1_000_000)),
    ("deepseek-v4.1-flash", Some("Deepseek v4.1 Flash"), Some(1_000_000)),
    ("glm-5.3", Some("GLM 5.3"), Some(1_000_000)),
    ("glm-5.3-flash", Some("GLM 5.3 Flash"), Some(1_000_000)),
    ("gpt-5.6-luna", Some("GPT 5.6 Luna"), Some(1_000_000)),
    ("gpt-5.6-sol", Some("GPT 5.6 Sol"), Some(1_000_000)),
    ("gpt-5.6-terra", Some("GPT 5.6 Terra"), Some(1_000_000)),
    ("gpt-6-astra", Some("GPT 6 Astra"), Some(1_000_000)),
    ("gpt-6.1-terra", Some("GPT 6.1 Terra"), Some(1_000_000)),
    ("grok-4.6", Some("Grok 4.6"), Some(500_000)),
    // Catalog JSON says 262000; pricing says 1000000. Do not pick one.
    ("kimi-k2.7-code", Some("Kimi K2.7-code"), None),
    ("kimi-k3", Some("Kimi K3"), Some(1_000_000)),
    ("kimi-k3.1-pro", Some("Kimi K3.1 Pro"), Some(1_000_000)),
    ("laguna-s-2.1", Some("Laguna S 2.1"), Some(262_000)),
    ("longcat-2.0-free", Some("LongCat 2.0"), Some(1_000_000)),
    ("minimax-m3", Some("MiniMax M3"), Some(1_000_000)),
    ("muse-spark-1.2", Some("Muse Spark 1.2"), Some(1_000_000)),
    ("muse-spark-1.2-contributor", Some("Muse Spark 1.2 Contributor"), Some(1_000_000)),
    ("muse-spark-1.3", Some("Muse Spark 1.3"), Some(1_000_000)),
    ("muse-spark-1.3-contributor", Some("Muse Spark 1.3 Contributor"), Some(1_000_000)),
    ("qwen3.7-plus", Some("Qwen 3.7 Plus"), Some(1_000_000)),
    ("qwen3.8-27b", Some("Qwen 3.8 27B"), Some(1_000_000)),
    ("qwen3.8-flash", Some("Qwen 3.8 Flash"), Some(1_000_000)),
    ("qwen3.8-max", Some("Qwen3.8-Max"), Some(1_000_000)),
    ("stepfun-3.7-flash", Some("Stepfun 3.7 Flash"), Some(262_000)),
    ("deepseek-v4-flash", Some("DeepSeek v4 Flash"), Some(1_000_000)),
    ("deepseek-v4-pro", Some("DeepSeek v4 Pro"), Some(1_000_000)),
    ("step-5-preview", Some("Step 5 Preview"), Some(1_000_000)),
    ("auto/bynara", None, None),
];

/// Documented chat model used when Nodecode's catalog is not public.
/// The image and classifier ids are real and are not session chat models.
const NODECODE_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("nodecode-cyber-v1", Some("Cyber v1"), Some(1_048_576)),
];

/// Documented model ids used when the provider's catalog is not public.
/// Ids are the current models page, not aliases or retired routes.
const ZRO_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("deepseek-v4.1-flash", Some("DeepSeek V4.1 Flash"), Some(1_048_576)),
    ("glm-5.3", Some("GLM-5.3"), Some(1_048_576)),
    ("glm-5.3-flash", Some("GLM-5.3 Flash"), Some(1_048_576)),
    ("dolly1-security", Some("Dolly 1 Security"), Some(1_048_576)),
    ("dolly2-security", Some("Dolly 2 Security"), Some(1_048_576)),
    ("abyss1", Some("Abyss 1"), Some(1_048_576)),
    ("auto", None, None),
];

/// Documented chat model used when anonymous GET /v1/models is not a catalog.
/// The id, display name, and context length are the K-EXAONE 2.5 API page.
const EXAONE_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("k-exaone_v2.5", Some("K-EXAONE 2.5"), Some(524_288)),
];

/// The models page lists one id. Context is that page's context window.
/// `GET /v1/models` does not return a name or a context length.
const FORTYTWO_ENGINE_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("qwen/qwen3.8-27b", Some("Qwen3.8-27B"), Some(262_144)),
];

/// Documented free-tier id. Anonymous `GET /v1/models` is not a catalog, and
/// this id is not the full AnyAPI list. A saved key uses the OpenAI decoder.
const ANYAPI_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("qwen/qwen3.8-27b:free", None, None),
];

/// Time-limited promo id through 2026-11-17 00:00 UTC. It is not Itzi's full
/// catalog. A saved key uses the OpenAI decoder.
const ITZI_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("special-free/deepseek-v4.1-flash", None, None),
];

/// Documented Hyperbolic Inference chat ids. Anonymous `GET /models` is not a
/// catalog. A saved key uses the OpenAI decoder. The overview states context
/// windows; this seed stores ids only.
const HYPERBOLIC_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("glm-5.3-flash", None, None),
    ("glm-5.3", None, None),
    ("kimi-k3", None, None),
    ("minimax-m3", None, None),
    ("deepseek-v4.1-flash", None, None),
    ("deepseek-v4-flash-0731", None, None),
    ("nemotron-3-ultra-550b-a55b", None, None),
    ("step-5-preview", None, None),
    ("mimo-v2.6-flash", None, None),
    ("hy4-preview", None, None),
    ("qwen3.8-27b", None, None),
];

/// Public Coarena chat ids from anonymous `GET /api/v1/models` on 2026-11-10.
/// That body was the whole catalog (`has_more` false). A saved key uses the
/// OpenAI decoder. The response includes display names and context lengths;
/// this seed stores ids only.
const COARENA_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("claude-fable-5", None, None),
    ("claude-fable-5-1", None, None),
    ("claude-fable-5-2", None, None),
    ("claude-haiku-5-5", None, None),
    ("claude-opus-5-5", None, None),
    ("claude-sonnet-5-5", None, None),
    ("deepseek-v4-1-flash-1017", None, None),
    ("gemini-3-8-flash", None, None),
    ("gemini-4-argon", None, None),
    ("gemini-4-carbon", None, None),
    ("glm-5-3-flash", None, None),
    ("gpt-6-1-sol", None, None),
    ("gpt-6-1-terra", None, None),
    ("gpt-6-astra", None, None),
    ("gpt-6-luna", None, None),
    ("grok-4-7", None, None),
    ("hy4", None, None),
    ("kimi-k3", None, None),
    ("kimi-k3-1-pro", None, None),
    ("mimo-v2-6-pro", None, None),
    ("minimax-m3-1-flash", None, None),
    ("qwen3-8-2-4t-a95b", None, None),
];

/// Public cheapllm.shop chat id from anonymous `GET /v1/models` on 2026-11-11.
/// That body was the whole catalog. A saved key uses the OpenAI decoder.
/// The response has no display name and no context length.
const CHEAPLLM_SHOP_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("deepseek-v4.1-flash-1017", None, None),
];

/// Reviewed chat ids from anonymous `GET https://api.modelverse.cn/v1/models`
/// on 2026-11-11. That body listed 129 rows and mixed chat with audio, image,
/// video, embedding, rerank, and tool ids. A saved key uses the OpenAI decoder.
/// This seed stores ids only, in the reviewed subset order.
const UCLOUD_MODELVERSE_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("deepseek-v4.1-flash-1017", None, None),
    ("deepseek-v4-pro-0813", None, None),
    ("glm-5.3-flash", None, None),
    ("glm-5.3", None, None),
    ("qwen3.7-flash", None, None),
    ("kimi-k3.1-pro", None, None),
];

/// Documented model ids used when anonymous GET /v1/models is not a catalog.
/// Ids and names are the current models page. Context values are the decimal
/// token counts of the labels that page publishes (1M, 256K, 262K, 16K).
const SAILRESEARCH_STATIC_MODELS: &[(&str, Option<&str>, Option<u64>)] = &[
    ("moonshotai/Kimi-K3.1-Pro", Some("Kimi K3.1 Pro"), Some(1_000_000)),
    ("moonshotai/Kimi-K3", Some("Kimi K3"), Some(1_000_000)),
    ("zai-org/GLM-5.3", Some("GLM-5.3"), Some(1_000_000)),
    ("zai-org/GLM-5.3-Flash", Some("GLM-5.3-Flash"), Some(1_000_000)),
    (
        "deepseek-ai/DeepSeek-V4.1-Flash-1017",
        Some("DeepSeek V4.1 Flash 1017"),
        Some(1_000_000),
    ),
    (
        "deepseek-ai/DeepSeek-V4-Pro-0813",
        Some("DeepSeek V4 Pro 0813"),
        Some(1_000_000),
    ),
    (
        "deepseek-ai/DeepSeek-V4-Flash-0731",
        Some("DeepSeek V4 Flash 0731"),
        Some(1_000_000),
    ),
    ("google/gemma-4-31B-it", Some("Gemma 4 31B IT"), Some(256_000)),
    (
        "nvidia/Gemma-4-31B-IT-NVFP4",
        Some("Gemma 4 31B IT (NVFP4)"),
        Some(262_000),
    ),
    ("google/gemma-4-12B-it", Some("Gemma 4 12B IT"), Some(16_000)),
    ("Qwen/Qwen3.6-35B-A3B", Some("Qwen3.6 35B A3B"), Some(262_000)),
];

fn reviewed_static_catalog(provider_type: &str) -> Option<Vec<ProviderModelSummary>> {
    let entries = match provider_type {
        "nodecode" => NODECODE_STATIC_MODELS,
        "zro" => ZRO_STATIC_MODELS,
        "nararouter" => NARAROUTER_STATIC_MODELS,
        "exaone" => EXAONE_STATIC_MODELS,
        "sailresearch" => SAILRESEARCH_STATIC_MODELS,
        "fortytwo-engine" => FORTYTWO_ENGINE_STATIC_MODELS,
        "anyapi" => ANYAPI_STATIC_MODELS,
        "itzi" => ITZI_STATIC_MODELS,
        "hyperbolic" => HYPERBOLIC_STATIC_MODELS,
        "coarena" => COARENA_STATIC_MODELS,
        "cheapllm-shop" => CHEAPLLM_SHOP_STATIC_MODELS,
        "ucloud-modelverse" => UCLOUD_MODELVERSE_STATIC_MODELS,
        _ => return None,
    };
    Some(entries.iter().map(|(id, name, context_length)| ProviderModelSummary {
        id: (*id).to_string(),
        name: name.map(str::to_string),
        context_length: *context_length,
    }).collect())
}

/// Built-in model IDs copied from a provider's official catalog when that
/// provider does not publish a Models API. Names and context lengths stay
/// empty unless the same page states them.
fn static_seed_models(provider_type: &str) -> Option<Vec<ProviderModelSummary>> {
    let ids: &[&str] = match provider_type {
        // https://pages.edgeone.ai/document/models-vendors-overview
        "edgeone-makers" => &[
            "@makers/hy3",
            "@makers/hy3-preview",
            "@makers/deepseek-v4-pro",
            "@makers/deepseek-v4-flash",
            "@makers/deepseek-v4.1-flash",
            "@makers/minimax-m3",
            "@makers/minimax-m2.7",
            "@makers/kimi-k2.6",
            "@makers/zhuque-text",
        ],
        // https://app.vllm-sr.ai/docs
        // GET /v1/models also lists System One decision models. Only this id is chat.
        "vllm-semantic-router" => &["vllm-sr/auto"],
        // https://inferbase.ai/docs/api
        // Public /v1/models mixes image, embedding, rerank, and speech rows.
        "inferbase" => &["auto"],
        // https://docs.opentyphoon.ai/en/models/
        // Public /v1/models is a bare JSON array and includes OCR/ASR ids.
        "opentyphoon" => &["typhoon-v2.5-30b-a3b-instruct"],
        // https://regolo.ai/pricing/
        // Public /v1/models mixes OCR, speech, image, embedding, and rerank rows.
        "regolo" => &[
            "qwen3.8-27b",
            "gemma4-31b",
            "glm5.2",
            "gpt-oss-120b",
        ],
        // https://proxy.gonkabroker.com/v1/models
        // The public list also includes the embedding model BAAI/bge-m3.
        "gonka-broker" => &[
            "MiniMaxAI/MiniMax-M2.7",
            "zai-org/GLM-5.3-Flash",
            "deepseek-ai/DeepSeek-V4-Flash-0731",
        ],
        // https://bazaarlink.ai/free
        // Public /v1/models mixes paid chat with image rows.
        "bazaarlink" => &[
            "auto:free",
            "qwen/qwen3.7-flash",
            "nvidia/nemotron-3.5-lightning-free",
            "stealth/glyph-cluster",
            "deepseek/deepseek-v4-flash-0731free",
        ],
        // https://aiapi-pro.com/docs
        // Public /v1/models includes image, video, audio, and embedding rows.
        "novai" => &[
            "glm-4.7-flash",
            "glm-4.6v-flash",
            "glm-4.1v-thinking-flash",
        ],
        // https://inference.dahl.global/docs/tokens/
        // The public list is these three chat ids. Embedding rows are not seeded.
        "dahl-inference" => &[
            "MiniMaxAI/MiniMax-M2.7",
            "zai-org/GLM-5.3-Flash",
            "deepseek-ai/DeepSeek-V4-Flash-0731",
        ],
        // https://docs.meganova.ai/tiers/tier-1
        // Public /v1/models includes whisper, embeddings, rerankers, and image rows.
        "meganova" => &[
            "meganova-ai/manta-mini-1.0",
            "meganova-ai/manta-flash-1.0",
            "Sao10K/L3-8B-Stheno-v3.2",
            "Sao10K/L3-70B-Euryale-v2.1",
            "Steelskull/L3.3-MS-Nevoria-70b",
            "BruhzWater/Sapphira-L3.3-70b-0.1",
            "FallenMerick/MN-Violet-Lotus-12B",
            "mistralai/Mistral-Small-3.2-24B-Instruct-2506",
        ],
        // https://www.runcrate.ai/inference/free
        // Public /v1/models includes audio, image, and video rows.
        "runcrate" => &[
            "deepseek-ai/DeepSeek-V4.1-Flash",
            "zai-org/GLM-5.3-Flash",
            "zai-org/GLM-5.3",
            "openai/gpt-oss-120b",
            "Qwen/Qwen3.8-Flash",
        ],
        // https://api.moark.com/v1/models
        // The public list mixes chat with OCR, image, and embedding rows.
        "moark" => &[
            "MiniMax-M3.1-Flash",
            "kimi-k3.1-pro",
            "DeepSeek-V4.1-Flash-1017",
            "deepseek-v4.1-flash",
        ],
        // https://docs.llm.kiwi/quickstart
        "llmkiwi" => &[
            "auto",
            "minimax-m3",
            "nemotron-3-ultra",
            "nemotron-3-super",
            "minimax-m2.5",
        ],
        // https://docs.sea-lion.ai/guides/inferencing/api
        // Anonymous GET /v1/models is not a catalog. Embeddings and SEA-Guard are not chat.
        "sea-lion" => &[
            "aisingapore/Qwen-SEA-LION-v4.5-27B-IT",
            "aisingapore/Llama-SEA-LION-v3.5-70B-R",
        ],
        // https://www.aionlabs.ai/docs/quickstart/
        "aion-labs" => &[
            "aion-labs/aion-3.5",
            "aion-labs/aion-3.5-mini",
            "aion-labs/aion-3.0",
            "aion-labs/aion-3.0-mini",
            "aion-labs/aion-2.0",
            "aion-labs/aion-rp-llama-3.1-8b",
        ],
        // https://platform.publicai.co/docs
        // Anonymous GET /v1/models is not a catalog.
        "publicai" => &["swiss-ai/apertus-v1.5-8b"],
        // https://kiosapi.id/docs
        // Public /v1/models includes image, video, embedding, and rerank rows.
        "kiosapi" => &[
            "meta/llama-3.1-8b",
            "qwen/qwen3-30b",
            "zai/glm-4.7-flash",
            "google/gemma-4-26b",
            "aisingapore/sea-lion-27b",
        ],
        // https://simplellm.eu/docs/models.html
        // GET /v1/models is not a public catalog. The priced gemma-4-E4B id is not free.
        "simplellm" => &["free-gemma-4-E4B", "free-Llama-3.1-8B-Instruct"],
        // https://helyxai.space/docs
        // GET /v1/models is HTTP 404. The quickstart example claude-opus-5 is not in the catalog.
        "helyx-ai" => &[
            "agnes-3.0-flash",
            "deepseek-v4.1-flash",
            "gemini-3.1-flash-lite",
            "gemma-4-31B-it",
            "glm-5.3-flash",
            "grok-4.3",
            "grok-build-0.1",
            "muse-spark-1.1",
        ],
        // https://docs.vectoree.ai/models
        // GET /api/v1/models needs a key and includes speech, image, video, and embeddings.
        "vectoree" => &["vectoree/auto"],
        // https://api.onysoft.com/docs
        // GET /v1/models needs a key. The quickstart example google/gemini-3.8-flash is not in this promo seed.
        "onysoft" => &[
            "tencent/hy4:tencent",
            "tencent/hy4-preview:tencent",
            "tencent/hy3:tencent",
            "deepseek/deepseek-v4.1-flash:tencent",
            "deepseek/deepseek-v4-flash-202605:tencent",
            "deepseek/deepseek-v4-pro-202606:tencent",
            "deepseek/deepseek-v4-pro-0813:tencent",
            "z-ai/glm-5.3:tencent",
            "z-ai/glm-5.3-flash:tencent",
            "z-ai/glm-5.3-flashx:tencent",
            "z-ai/glm-5.2:tencent",
            "moonshotai/kimi-k3:tencent",
            "moonshotai/kimi-k2.7-code:tencent",
            "moonshotai/kimi-k2.7-code-highspeed:tencent",
            "minimax/minimax-m3:tencent",
            "minimax/minimax-m3.1-flash:tencent",
        ],
        // https://twigpine.com/opengateway/docs
        // Public /v1/models includes paid chat rows. Only the router and the free ids are seeded.
        "twigpine-opengateway" => &[
            "auto",
            "qwen/qwen3.8-max",
            "qwen/qwen3.8-flash",
            "nvidia/nemotron-3.5-super-vl:free",
        ],
        // https://hiapi.ai/docs/text-api/chat-completions/
        // GET /v1/models needs a key. The curl example deepseek-v4-flash is not this seed.
        "hiapi" => &["deepseek-v4.1-flash"],
        // https://docs.maritaca.ai/en/s5-quick-start
        // GET /api/models needs a key. Previous-generation ids such as sabia-4 are not this seed.
        "maritaca" => &["S5-free", "S5", "S5-br-sp", "S5-nano"],
        // https://crowllm.com/docs
        // Public /v1/models mixes chat with embeddings, images, and other non-chat rows.
        "crowllm" => &[
            "qwen-3.8-27b:free",
            "deepseek-v4.1-flash:free",
            "longcat-2.5:free",
            "gemma-4-31b-it:free",
        ],
        // https://www.respan.ai/docs/ai-gateway/quickstart
        // GET https://gateway.respan.ai/v1/models is HTTP 404. Unsuffixed paid ids are not this seed.
        "respan" => &[
            "google_gemini_ai/gemini-4-carbon:free",
            "meta/muse-ember:free",
        ],
        // https://community.hetzner.com/tutorials/opencode-with-hetzner-inference-api-systemd-sandbox/
        // GET /api/v1/models requires auth. Launch ids other than this FP8 id are not this seed.
        "hetzner-experiments" => &["Qwen/Qwen3.6-35B-A3B-FP8"],
        // https://docs.powertokens.ai/en/guides/api-key-and-model-call
        // GET /v1/models requires auth and mixes image, speech, and video rows.
        // glm-4.7-flash is not supported on Responses. The quickstart example MiniMax-M3 is not this seed.
        "powertokens" => &["glm-4.7-flash"],
        // https://free.ai/api/
        // Public GET /v1/models is {models,total,caller_country} and mixes image, speech, and video rows.
        // Several self-hosted ids are labels for one Qwen model. qwen3-coder and the other aliases are not this seed.
        "free-ai" => &["qwen7b", "qwen-coder"],
        // https://www.suanli.cn/docs/large-model-cloud-service/quickstart/cqsrwzrbsim4idkkjbxcdux4nfg/
        // GET /v1/models requires a key. Unauthenticated returns a custom 400, not an OpenAI list.
        // The product page also lists kimi-k2.6, glm-5, glm-5.1, and tencent/hy3-preview. Those are not this seed.
        "suanli-maas" => &[
            "deepseek/deepseek-v4-flash",
            "deepseek/deepseek-v4-pro",
            "moonshotai/kimi-k3",
            "z-ai/glm-5.2",
            "qwen/qwen3.7-max",
        ],
        // https://botscanner.ai/faq
        // GET /v1/models requires a key. Unauthenticated returns HTTP 401, not an OpenAI list.
        // Playground ids such as qwen/qwen3.7-flash are not router knobs and are not this seed.
        "botscanner" => &[
            "autobench/beta-cost",
            "autobench/beta-balanced",
            "autobench/beta-quality",
        ],
        // https://www.cucloud.cn/product/tokenplan.html
        // The console issues the Base URL. /v1/models is not public.
        // HappyHorse1.1 is a video model on the Double 11 page and is not this seed.
        // https://www.cucloud.cn/bwnnov.html
        "cucloud" => &[
            "DeepSeek-V4.1-Flash-1017",
            "DeepSeek-V4-Pro",
            "Qwen3.8-Max",
            "Qwen3.8-Flash",
            "glm-5.3",
        ],
        // https://developer.puter.com/tutorials/use-openai-sdk-with-puter/
        // https://developer.puter.com/tutorials/use-anthropic-sdk-with-puter/
        // GET /puterai/openai/v1/models is HTTP 404. The catalog data-id
        // openai/gpt-5.4-nano is not the tutorial string and is not this seed.
        "puter" => &[
            "meta/muse-wonder",
            "gpt-5.4-nano",
            "claude-sonnet-5-5",
        ],
        // https://platform.mosi.cn/docs/models/nex
        // GET /v1/models requires a key. Unauthenticated returns HTTP 403, not an OpenAI list.
        // The snapshot nex-n2.6-2026-11-05 is not this seed.
        "moss" => &["nex-n2.6"],
        // https://minekuai.cn/llm
        // GET /v1/models requires a key. Unauthenticated returns HTTP 401, not an OpenAI list.
        // The public price-table id deepseek-v4.1-flash-1017 is not this seed.
        "minekuai" => &["deepseek-flash", "deepseek-v4-pro"],
        _ => return None,
    };
    Some(
        ids.iter()
            .map(|id| ProviderModelSummary {
                id: (*id).to_string(),
                name: None,
                context_length: None,
            })
            .collect(),
    )
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
        | "helmholtz-blablador"

        | "token360"
        | "edenai"
        | "nararouter"
        | "zro"
        | "xkiro"
        | "nodecode"
        | "freeinference"
        | "optracloud"
        | "ifm"
        | "exaone"
        | "sailresearch"
        | "llm7"
        | "fortytwo-engine"
        | "anyapi"
        | "itzi"
        | "hyperbolic"
        | "coarena"
        | "cheapllm-shop"
        | "ucloud-modelverse"
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
            | "token360"
            | "edenai"
            | "xkiro"
            | "freeinference"
            | "optracloud"
            | "ifm"
            | "llm7"
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
            | "coarena"
            | "cheapllm-shop"
            | "ucloud-modelverse"
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
    if provider.model_catalog_url.is_some() && provider.protocols.keys().all(|protocol| *protocol == ModelProtocol::AwsBedrockConverse) {
        return Ok(DiscoverySpec { adapter: DiscoveryAdapter::OpenAi, credential: CredentialPolicy::Required });
    }
    if let Some(spec) = reviewed_discovery_spec(&provider.provider) {
        return Ok(spec);
    }
    if provider.model_catalog_url.is_none() && static_seed_models(&provider.provider).is_some() {
        return Err("model_discovery_static_seed: This provider publishes a reviewed model list and no Models API".into());
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
        || provider.protocols.contains_key(&ModelProtocol::GeminiInteractions)
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
            let protocol = if provider.protocols.contains_key(&ModelProtocol::GeminiGenerateContent) {
                ModelProtocol::GeminiGenerateContent
            } else { ModelProtocol::GeminiInteractions };
            let mut url = derived_protocol_url(provider, protocol, "/models")?;
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
        .filter(|entry| !is_non_chat_ifm_catalog_entry(entry))
        .filter_map(|entry| model_summary(adapter, entry))
        .collect();
    // A malformed Cloudflare ID must not make a full page appear to be the
    // final short page. This adapter requests the documented marketplace shape.
    if adapter == DiscoveryAdapter::Cloudflare && models.len() != entries.len() {
        return Err("model_discovery_invalid_response: Workers AI returned an invalid marketplace model ID".into());
    }
    Ok(DecodedPage { models, next_token })
}

/// IFM publishes speech-only rows in the same OpenAI list as chat models.
/// Drop a row only when it is an IFM model and `supported_endpoints` lists
/// neither Chat Completions nor Responses. Other catalogs are unchanged.
fn is_non_chat_ifm_catalog_entry(entry: &Value) -> bool {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
    if entry.get("owned_by").and_then(Value::as_str) != Some("MBZUAI") || !id.starts_with("IFM/") {
        return false;
    }
    let Some(endpoints) = entry.get("supported_endpoints").and_then(Value::as_array) else {
        return false;
    };
    !endpoints.iter().any(|endpoint| {
        matches!(
            endpoint.as_str(),
            Some("/v1/chat/completions") | Some("/v1/responses")
        )
    })
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

    #[test]
    fn interactions_reuses_google_catalog_without_a_converse_models_probe() {
        let google = provider("custom", "https://example.invalid", ModelProtocol::GeminiInteractions, "/tenant/v1beta/interactions");
        let spec = discovery_spec_for_provider(&google).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::Gemini);
        assert_eq!(discovery_url(&google, spec.adapter).unwrap().path(), "/tenant/v1beta/models");
        let mut bedrock = provider("amazon-bedrock", "https://bedrock-runtime.eu-west-1.amazonaws.com",
            ModelProtocol::AwsBedrockConverse, "/model/{model}/converse");
        assert!(!provider_model_discovery_supported(&bedrock));
        assert!(discovery_spec_for_provider(&bedrock).unwrap_err().contains("manually"));
        bedrock.provider = "custom".into();
        assert!(!provider_model_discovery_supported(&bedrock));
    }
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
    fn decuff_has_no_reviewed_catalog_spec_and_falls_back_to_the_openai_models_path() {
        assert!(reviewed_discovery_spec("decuff").is_none());
        assert!(model_discovery_supported("decuff"));
        let config = provider(
            "decuff",
            "https://api.decuff.ai",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.decuff.ai/v1/models"
        );
    }

    #[test]
    fn asi_cloud_has_no_reviewed_catalog_spec_and_falls_back_to_the_openai_models_path() {
        assert!(reviewed_discovery_spec("asi-cloud").is_none());
        assert!(model_discovery_supported("asi-cloud"));
        let config = provider(
            "asi-cloud",
            "https://inference.asicloud.cudos.org/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://inference.asicloud.cudos.org/v1/models"
        );
    }

    #[test]
    fn edgeone_makers_serves_the_documented_builtin_seed_and_does_not_call_models() {
        assert!(reviewed_discovery_spec("edgeone-makers").is_none());
        assert!(model_discovery_supported("edgeone-makers"));
        let mut config = provider(
            "edgeone-makers",
            "https://ai-gateway.edgeone.link",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        config.auth_requirement = AuthRequirement::Required;
        assert!(discovery_spec_for_provider(&config)
            .unwrap_err()
            .contains("model_discovery_static_seed"));
        let models = execute_provider_discovery(ModelDiscoveryInput { provider: config.clone() }).unwrap();
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            [
                "@makers/hy3",
                "@makers/hy3-preview",
                "@makers/deepseek-v4-pro",
                "@makers/deepseek-v4-flash",
                "@makers/deepseek-v4.1-flash",
                "@makers/minimax-m3",
                "@makers/minimax-m2.7",
                "@makers/kimi-k2.6",
                "@makers/zhuque-text",
            ]
        );
        assert!(models.iter().all(|model| model.name.is_none() && model.context_length.is_none()));

        config.model_catalog_url = Some("https://catalog.example.test/models".into());
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://catalog.example.test/models"
        );
    }

    #[test]
    fn vllm_semantic_router_seeds_vllm_sr_auto_and_does_not_call_models() {
        assert!(reviewed_discovery_spec("vllm-semantic-router").is_none());
        assert!(model_discovery_supported("vllm-semantic-router"));
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"vllm-semantic-router"));
        let mut config = provider(
            "vllm-semantic-router",
            "https://app.vllm-sr.ai/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        config.auth_requirement = AuthRequirement::Required;
        assert!(discovery_spec_for_provider(&config)
            .unwrap_err()
            .contains("model_discovery_static_seed"));
        let models = execute_provider_discovery(ModelDiscoveryInput {
            provider: config.clone(),
        })
        .unwrap();
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            ["vllm-sr/auto"]
        );
        assert!(models
            .iter()
            .all(|model| model.name.is_none() && model.context_length.is_none()));
        for omitted in [
            "vllm-sr/d3-ultra",
            "vllm-sr/d3-turbo",
            "vllm-sr/d3",
            "vllm-sr/d3-flash",
            "vllm-sr/d3-mini",
            "vllm-sr/d3-nano",
            "vllm-sr/d3-lite",
            "vllm-sr/d3-edge",
            "vllm-sr/vela3-turbo",
            "vllm-sr/vela3-flash",
            "zai/glm-5.3-flash",
            "qwen/qwen3.8-flash-next",
            "qwen/qwen3.8-27b",
        ] {
            assert!(!models.iter().any(|model| model.id == omitted), "{omitted}");
        }

        config.model_catalog_url = Some("https://catalog.example.test/models".into());
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://catalog.example.test/models"
        );
    }

    fn assert_static_chat_seed(
        provider_type: &str,
        base_url: &str,
        endpoint_path: &str,
        expected: &[&str],
        omitted: &[&str],
    ) {
        assert!(reviewed_discovery_spec(provider_type).is_none(), "{provider_type}");
        assert!(reviewed_static_catalog(provider_type).is_none(), "{provider_type}");
        assert!(model_discovery_supported(provider_type), "{provider_type}");
        assert!(
            !EXPECTED_OPENAI_COMPATIBLE.contains(&provider_type),
            "{provider_type}"
        );
        let mut config = provider(
            provider_type,
            base_url,
            ModelProtocol::OpenaiCompletions,
            endpoint_path,
        );
        config.auth_requirement = AuthRequirement::Required;
        assert!(discovery_spec_for_provider(&config)
            .unwrap_err()
            .contains("model_discovery_static_seed"));
        let models = execute_provider_discovery(ModelDiscoveryInput {
            provider: config.clone(),
        })
        .unwrap();
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            expected,
            "{provider_type}"
        );
        assert!(models
            .iter()
            .all(|model| model.name.is_none() && model.context_length.is_none()));
        for id in omitted {
            assert!(!models.iter().any(|model| model.id == *id), "{provider_type} {id}");
        }
        config.model_catalog_url = Some("https://catalog.example.test/models".into());
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://catalog.example.test/models"
        );
    }

    #[test]
    fn inferbase_seeds_auto_and_does_not_decode_the_mixed_public_catalog() {
        assert_static_chat_seed(
            "inferbase",
            "https://api.inferbase.ai/v1",
            "/chat/completions",
            &["auto"],
            &[],
        );
    }

    #[test]
    fn opentyphoon_seeds_the_documented_chat_model_and_skips_the_bare_models_array() {
        assert_static_chat_seed(
            "opentyphoon",
            "https://api.opentyphoon.ai/v1",
            "/chat/completions",
            &["typhoon-v2.5-30b-a3b-instruct"],
            &["typhoon-v2.1-12b-instruct"],
        );
    }

    #[test]
    fn regolo_seeds_core_chat_models_and_skips_mixed_modalities() {
        assert_static_chat_seed(
            "regolo",
            "https://api.regolo.ai/v1",
            "/chat/completions",
            &["qwen3.8-27b", "gemma4-31b", "glm5.2", "gpt-oss-120b"],
            &[
                "deepseek-ocr-2",
                "faster-whisper-large-v3",
                "Qwen-Image",
                "Qwen3-Embedding-8B",
                "Qwen3-Reranker-4B",
                "Llama-3.3-70B-Instruct",
            ],
        );
    }

    #[test]
    fn gonka_broker_seeds_chat_models_and_skips_the_embedding_row() {
        assert_static_chat_seed(
            "gonka-broker",
            "https://proxy.gonkabroker.com/v1",
            "/chat/completions",
            &[
                "MiniMaxAI/MiniMax-M2.7",
                "zai-org/GLM-5.3-Flash",
                "deepseek-ai/DeepSeek-V4-Flash-0731",
            ],
            &["BAAI/bge-m3"],
        );
    }

    #[test]
    fn fortytwo_engine_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("fortytwo-engine").expect("fortytwo seed");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "qwen/qwen3.8-27b");
        assert_eq!(models[0].name.as_deref(), Some("Qwen3.8-27B"));
        assert_eq!(models[0].context_length, Some(262_144));
        assert!(static_seed_models("fortytwo-engine").is_none());
        let config = provider(
            "fortytwo-engine",
            "https://api.fortytwo.network/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.fortytwo.network/v1/models"
        );
        let _home = TestHome::new("fortytwo-engine-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "fortytwo-engine-instance".into(),
                name: "Fortytwo Engine".into(),
                provider: "fortytwo-engine".into(),
                base_url: "https://api.fortytwo.network/v1".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "qwen/qwen3.8-27b");
        assert_eq!(seeded[0].context_length, Some(262_144));
    }

    #[test]
    fn anyapi_seeds_the_documented_free_model_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("anyapi").expect("anyapi seed");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "qwen/qwen3.8-27b:free");
        assert!(models[0].name.is_none());
        assert!(models[0].context_length.is_none());
        assert!(static_seed_models("anyapi").is_none());
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"anyapi"));
        let config = provider(
            "anyapi",
            "https://api.anyapi.ai/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.anyapi.ai/v1/models"
        );
        let _home = TestHome::new("anyapi-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "anyapi-instance".into(),
                name: "AnyAPI".into(),
                provider: "anyapi".into(),
                base_url: "https://api.anyapi.ai/v1".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "qwen/qwen3.8-27b:free");
    }

    #[test]
    fn itzi_seeds_the_time_limited_promo_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("itzi").expect("itzi seed");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "special-free/deepseek-v4.1-flash");
        assert!(models[0].name.is_none());
        assert!(models[0].context_length.is_none());
        assert!(static_seed_models("itzi").is_none());
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"itzi"));
        let config = provider(
            "itzi",
            "https://itzi.app/v1",
            ModelProtocol::OpenaiResponses,
            "/responses",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://itzi.app/v1/models"
        );
        let _home = TestHome::new("itzi-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "itzi-instance".into(),
                name: "Itzi".into(),
                provider: "itzi".into(),
                base_url: "https://itzi.app/v1".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiResponses,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/responses".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "special-free/deepseek-v4.1-flash");
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
    fn token360_discovers_the_public_openai_catalog() {
        let config = provider(
            "token360",
            "https://api.token360.ai",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.token360.ai/v1/models"
        );
    }

    #[test]
    fn llm7_discovers_the_public_openai_catalog() {
        let config = provider(
            "llm7",
            "https://api.llm7.io/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.llm7.io/v1/models"
        );
    }

    #[test]
    fn zro_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("zro").expect("zro seed");
        let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "deepseek-v4.1-flash",
                "glm-5.3",
                "glm-5.3-flash",
                "dolly1-security",
                "dolly2-security",
                "abyss1",
                "auto",
            ]
        );
        assert_eq!(
            models[0].name.as_deref(),
            Some("DeepSeek V4.1 Flash")
        );
        assert_eq!(models[0].context_length, Some(1_048_576));
        assert_eq!(models[5].name.as_deref(), Some("Abyss 1"));
        assert!(models[6].name.is_none());
        assert!(models[6].context_length.is_none());
        for retired in [
            "glm-5.2",
            "minimax-m3",
            "kimi-k3",
            "kimi-k2.7-code",
            "deepseek-v4-flash-0731",
            "laguna-2.1",
            "deepseek-v4-flash-vision",
            "minimax-m3.1-flash",
            "zro/deepseek-v4.1-flash",
        ] {
            assert!(!ids.contains(&retired), "{retired}");
        }
        assert!(reviewed_static_catalog("decuff").is_none());
        assert!(reviewed_static_catalog("token360").is_none());

        let config = provider(
            "zro",
            "https://zro.moonmath.ai",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://zro.moonmath.ai/v1/models"
        );

        let _home = TestHome::new("zro-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "zro-instance".into(),
                name: "Zro".into(),
                provider: "zro".into(),
                base_url: "https://zro.moonmath.ai".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), ids.len());
        assert_eq!(seeded[0].id, "deepseek-v4.1-flash");
    }

    #[test]
    fn exaone_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("exaone").expect("exaone seed");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "k-exaone_v2.5");
        assert_eq!(models[0].name.as_deref(), Some("K-EXAONE 2.5"));
        assert_eq!(models[0].context_length, Some(524_288));
        assert!(static_seed_models("exaone").is_none());

        let config = provider(
            "exaone",
            "https://api.exaone.ai/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.exaone.ai/v1/models"
        );

        let _home = TestHome::new("exaone-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "exaone-instance".into(),
                name: "EXAONE API Platform".into(),
                provider: "exaone".into(),
                base_url: "https://api.exaone.ai/v1".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "k-exaone_v2.5");
        assert_eq!(seeded[0].context_length, Some(524_288));
    }

    #[test]
    fn sailresearch_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("sailresearch").expect("sailresearch seed");
        let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "moonshotai/Kimi-K3.1-Pro",
                "moonshotai/Kimi-K3",
                "zai-org/GLM-5.3",
                "zai-org/GLM-5.3-Flash",
                "deepseek-ai/DeepSeek-V4.1-Flash-1017",
                "deepseek-ai/DeepSeek-V4-Pro-0813",
                "deepseek-ai/DeepSeek-V4-Flash-0731",
                "google/gemma-4-31B-it",
                "nvidia/Gemma-4-31B-IT-NVFP4",
                "google/gemma-4-12B-it",
                "Qwen/Qwen3.6-35B-A3B",
            ]
        );
        assert_eq!(models[0].name.as_deref(), Some("Kimi K3.1 Pro"));
        assert_eq!(models[0].context_length, Some(1_000_000));
        assert_eq!(models[4].name.as_deref(), Some("DeepSeek V4.1 Flash 1017"));
        assert_eq!(models[4].context_length, Some(1_000_000));
        assert_eq!(models[7].context_length, Some(256_000));
        assert_eq!(models[8].context_length, Some(262_000));
        assert_eq!(models[9].context_length, Some(16_000));
        assert_eq!(models[10].name.as_deref(), Some("Qwen3.6 35B A3B"));
        assert_eq!(models[10].context_length, Some(262_000));
        assert!(static_seed_models("sailresearch").is_none());
        assert!(reviewed_static_catalog("decuff").is_none());

        let config = provider(
            "sailresearch",
            "https://api.sailresearch.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.sailresearch.com/v1/models"
        );

        let _home = TestHome::new("sailresearch-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "sailresearch-instance".into(),
                name: "Sail Research".into(),
                provider: "sailresearch".into(),
                base_url: "https://api.sailresearch.com".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), ids.len());
        assert_eq!(seeded[0].id, "moonshotai/Kimi-K3.1-Pro");
        assert_eq!(seeded[3].id, "zai-org/GLM-5.3-Flash");
        assert_eq!(seeded[10].context_length, Some(262_000));
    }

    #[test]
    fn nararouter_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("nararouter").expect("nararouter seed");
        let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "agnes-3-flash",
                "claude-fable-5.1",
                "claude-haiku-5.5",
                "claude-opus-4.8",
                "claude-opus-5",
                "claude-opus-5.5",
                "claude-sonnet-5",
                "claude-sonnet-5.5",
                "deepseek-v4.1-flash-abliterated",
                "deepseek-v4.1-flash-fast",
                "deepseek-v4.1-flash",
                "glm-5.3",
                "glm-5.3-flash",
                "gpt-5.6-luna",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-6-astra",
                "gpt-6.1-terra",
                "grok-4.6",
                "kimi-k2.7-code",
                "kimi-k3",
                "kimi-k3.1-pro",
                "laguna-s-2.1",
                "longcat-2.0-free",
                "minimax-m3",
                "muse-spark-1.2",
                "muse-spark-1.2-contributor",
                "muse-spark-1.3",
                "muse-spark-1.3-contributor",
                "qwen3.7-plus",
                "qwen3.8-27b",
                "qwen3.8-flash",
                "qwen3.8-max",
                "stepfun-3.7-flash",
                "deepseek-v4-flash",
                "deepseek-v4-pro",
                "step-5-preview",
                "auto/bynara",
            ]
        );
        assert_eq!(models[8].name.as_deref(), Some("DeepSeek v4.1 Flash Abliterated"));
        assert_eq!(models[8].context_length, Some(1_000_000));
        assert_eq!(models[10].name.as_deref(), Some("Deepseek v4.1 Flash"));
        assert_eq!(models[10].context_length, Some(1_000_000));
        assert_eq!(models[18].context_length, Some(500_000));
        assert_eq!(models[19].name.as_deref(), Some("Kimi K2.7-code"));
        assert!(models[19].context_length.is_none());
        assert_eq!(models[22].name.as_deref(), Some("Laguna S 2.1"));
        assert_eq!(models[22].context_length, Some(262_000));
        assert_eq!(models[36].name.as_deref(), Some("Step 5 Preview"));
        assert_eq!(models[36].context_length, Some(1_000_000));
        assert!(models[37].name.is_none());
        assert!(models[37].context_length.is_none());
        for omitted in [
            "kimi-k2.5",
            "gemini-3.1-pro",
            "text-embedding-3-small",
            "rerank-v3",
            "jev",
            "jev-1.14.0",
            "combo/free-pack",
            "deepseek-v4-flash:online",
            "gpt-image-2",
            "crouton-alpha",
        ] {
            assert!(!ids.contains(&omitted), "{omitted}");
        }
        assert!(reviewed_static_catalog("decuff").is_none());
        assert!(reviewed_static_catalog("token360").is_none());

        let config = provider(
            "nararouter",
            "https://router.bynara.id",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://router.bynara.id/v1/models"
        );

        let _home = TestHome::new("nararouter-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "nararouter-instance".into(),
                name: "NaraRouter".into(),
                provider: "nararouter".into(),
                base_url: "https://router.bynara.id".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: crate::domain::types::AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), ids.len());
        assert_eq!(seeded[0].id, "agnes-3-flash");
        assert_eq!(seeded[8].id, "deepseek-v4.1-flash-abliterated");
        assert_eq!(seeded[37].id, "auto/bynara");
    }

    #[test]
    fn xkiro_discovers_the_public_openai_catalog() {
        assert!(reviewed_static_catalog("xkiro").is_none());
        let config = provider(
            "xkiro",
            "https://api.xkiro.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.xkiro.com/v1/models"
        );
    }

    #[test]
    fn nodecode_seeds_the_documented_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("nodecode").expect("nodecode seed");
        let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, ["nodecode-cyber-v1"]);
        assert_eq!(models[0].name.as_deref(), Some("Cyber v1"));
        assert_eq!(models[0].context_length, Some(1_048_576));
        for omitted in [
            "nodecode-image-v1",
            "nodecode-decide-v1",
            "nodecode-free/nodecode-cyber-v1",
            "nodecode-free",
        ] {
            assert!(!ids.contains(&omitted), "{omitted}");
        }

        let config = provider(
            "nodecode",
            "https://api.nodecode.ai",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.nodecode.ai/v1/models"
        );

        let _home = TestHome::new("nodecode-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "nodecode-instance".into(),
                name: "Nodecode".into(),
                provider: "nodecode".into(),
                base_url: "https://api.nodecode.ai".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "nodecode-cyber-v1");
        assert_eq!(seeded[0].context_length, Some(1_048_576));
    }

    #[test]
    fn freeinference_discovers_the_public_openai_catalog() {
        assert!(reviewed_static_catalog("freeinference").is_none());
        let config = provider(
            "freeinference",
            "https://freeinference.org",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://freeinference.org/v1/models"
        );
    }

    #[test]
    fn optracloud_discovers_the_public_openai_catalog() {
        assert!(reviewed_static_catalog("optracloud").is_none());
        let config = provider(
            "optracloud",
            "https://api.optracloud.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.optracloud.com/v1/models"
        );
    }

    #[test]
    fn ifm_discovers_the_public_openai_catalog() {
        assert!(reviewed_static_catalog("ifm").is_none());
        let config = provider(
            "ifm",
            "https://api.ifm.ai/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.ifm.ai/v1/models"
        );
    }

    #[test]
    fn edenai_discovers_the_public_openai_catalog() {
        let config = provider(
            "edenai",
            "https://api.edenai.run/v3",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.edenai.run/v3/models"
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
            "https://lutheos.com",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://lutheos.com/v1/models"
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
    fn helmholtz_blablador_discovers_openai_models_on_the_documented_catalog_path() {
        let config = provider(
            "helmholtz-blablador",
            "https://api.blablador.fz-juelich.de/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.blablador.fz-juelich.de/v1/models"
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
        "helmholtz-blablador",

        "token360",
        "edenai",
        "nararouter",
        "zro",
        "xkiro",
        "nodecode",
        "freeinference",
        "optracloud",
        "ifm",
        "exaone",
        "sailresearch",
        "llm7",
        "fortytwo-engine",
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
        "coarena",
        "cheapllm-shop",
        "ucloud-modelverse",
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
            "token360",
            "edenai",
            "xkiro",
            "freeinference",
            "optracloud",
            "ifm",
            "llm7",
            "coarena",
            "cheapllm-shop",
            "ucloud-modelverse",
        ] {
            assert_eq!(
                reviewed_discovery_spec(provider_type).map(|spec| spec.credential),
                Some(CredentialPolicy::Optional),
                "{provider_type}",
            );
        }
        for provider_type in ["openai", "gmi-cloud", "clean-apis", "dolphin", "river", "unisound", "yeta", "scnet", "oxion-ai", "larprouter", "mango-inference", "darkbloom", "scaledown", "amd-token-factory", "swarms", "helmholtz-blablador", "zro", "nararouter", "nodecode", "exaone", "sailresearch", "fortytwo-engine", "anyapi", "itzi", "hyperbolic"] {
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
    fn ifm_public_catalog_drops_speech_only_models() {
        let page = decode_page(
            DiscoveryAdapter::OpenAi,
            serde_json::json!({
                "object": "list",
                "data": [
                    {
                        "id": "IFM/K2-Horizon-375B-A23B",
                        "object": "model",
                        "owned_by": "MBZUAI",
                        "display_name": "K2 Horizon 375B A23B",
                        "max_model_len": 524288,
                        "context_length": 524288,
                        "supported_endpoints": ["/v1/chat/completions", "/v1/responses"]
                    },
                    {
                        "id": "IFM/Jais-3-MoVA-36B-A4B",
                        "object": "model",
                        "owned_by": "MBZUAI",
                        "display_name": "Jais 3 MoVA 36B A4B",
                        "supported_endpoints": ["/v1/chat/completions"]
                    },
                    {
                        "id": "IFM/Jais-ASR",
                        "object": "model",
                        "owned_by": "MBZUAI",
                        "display_name": "Jais ASR",
                        "supported_endpoints": ["/v1/audio/transcriptions"]
                    },
                    {
                        "id": "vendor/embed",
                        "owned_by": "other",
                        "supported_endpoints": ["/v1/embeddings"]
                    }
                ]
            }),
        )
        .unwrap();
        assert_eq!(page.models.len(), 3);
        assert_eq!(page.models[0].id, "IFM/K2-Horizon-375B-A23B");
        assert_eq!(page.models[0].name.as_deref(), Some("K2 Horizon 375B A23B"));
        assert_eq!(page.models[0].context_length, Some(524288));
        assert_eq!(page.models[1].id, "IFM/Jais-3-MoVA-36B-A4B");
        assert_eq!(page.models[1].name.as_deref(), Some("Jais 3 MoVA 36B A4B"));
        assert!(page.models[1].context_length.is_none());
        assert_eq!(page.models[2].id, "vendor/embed");
        assert!(page.models.iter().all(|model| model.id != "IFM/Jais-ASR"));
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

    #[test]
    fn thirteen_provider_templates_seed_only_reviewed_chat_ids() {
        assert_static_chat_seed(
            "bazaarlink",
            "https://api.bazaarlink.ai/v1",
            "/chat/completions",
            &[
                "auto:free",
                "qwen/qwen3.7-flash",
                "nvidia/nemotron-3.5-lightning-free",
                "stealth/glyph-cluster",
                "deepseek/deepseek-v4-flash-0731free",
            ],
            &[
                "qwen3.7-flash",
                "qwen/qwen3.7-flash:free",
                "deepseek-v4-flash-0731free",
                "gemini-3.1-flash-image",
            ],
        );
        assert_static_chat_seed(
            "novai",
            "https://aiapi-pro.com/v1",
            "/chat/completions",
            &[
                "glm-4.7-flash",
                "glm-4.6v-flash",
                "glm-4.1v-thinking-flash",
            ],
            &[
                "cogview-3-flash",
                "cogvideox-flash",
                "qwen3-tts-flash",
                "qwen3.7-text-embedding",
            ],
        );
        assert_static_chat_seed(
            "dahl-inference",
            "https://inference.dahl.global/v1",
            "/chat/completions",
            &[
                "MiniMaxAI/MiniMax-M2.7",
                "zai-org/GLM-5.3-Flash",
                "deepseek-ai/DeepSeek-V4-Flash-0731",
            ],
            &["BAAI/bge-m3"],
        );
        assert_static_chat_seed(
            "meganova",
            "https://api.meganova.ai/v1",
            "/chat/completions",
            &[
                "meganova-ai/manta-mini-1.0",
                "meganova-ai/manta-flash-1.0",
                "Sao10K/L3-8B-Stheno-v3.2",
                "Sao10K/L3-70B-Euryale-v2.1",
                "Steelskull/L3.3-MS-Nevoria-70b",
                "BruhzWater/Sapphira-L3.3-70b-0.1",
                "FallenMerick/MN-Violet-Lotus-12B",
                "mistralai/Mistral-Small-3.2-24B-Instruct-2506",
            ],
            &[
                "meganova-ai/manta-pro-1.0",
                "zai-org/GLM-4.7-Flash",
                "Systran/faster-whisper-large-v3",
                "Qwen/Qwen3-Embedding-8B",
                "BAAI/bge-reranker-v2-m3",
                "Bytedance/seedream-4-5-251128",
            ],
        );
        assert_static_chat_seed(
            "runcrate",
            "https://api.runcrate.ai/v1",
            "/chat/completions",
            &[
                "deepseek-ai/DeepSeek-V4.1-Flash",
                "zai-org/GLM-5.3-Flash",
                "zai-org/GLM-5.3",
                "openai/gpt-oss-120b",
                "Qwen/Qwen3.8-Flash",
            ],
            &[
                "openai/whisper-large-v3-turbo",
                "Qwen/Qwen3-Embedding-8B",
                "stabilityai/stable-diffusion-3-5-large",
                "kuaishou/kling-v3-video",
                "Qwen/Qwen3-TTS",
            ],
        );
        assert_static_chat_seed(
            "moark",
            "https://api.moark.com/v1",
            "/chat/completions",
            &[
                "MiniMax-M3.1-Flash",
                "kimi-k3.1-pro",
                "DeepSeek-V4.1-Flash-1017",
                "deepseek-v4.1-flash",
            ],
            &[
                "Unlimited-OCR-2",
                "qwen-image-2.1-viggle-turbo",
                "WeMM-Embedding-9B",
            ],
        );
        assert_static_chat_seed(
            "llmkiwi",
            "https://api.llm.kiwi/v1",
            "/chat/completions",
            &[
                "auto",
                "minimax-m3",
                "nemotron-3-ultra",
                "nemotron-3-super",
                "minimax-m2.5",
            ],
            &["hrllm"],
        );
        assert_static_chat_seed(
            "sea-lion",
            "https://api.sea-lion.ai/v1",
            "/chat/completions",
            &[
                "aisingapore/Qwen-SEA-LION-v4.5-27B-IT",
                "aisingapore/Llama-SEA-LION-v3.5-70B-R",
            ],
            &["aisingapore/SEA-Guard"],
        );
        assert_static_chat_seed(
            "aion-labs",
            "https://api.aionlabs.ai/v1",
            "/chat/completions",
            &[
                "aion-labs/aion-3.5",
                "aion-labs/aion-3.5-mini",
                "aion-labs/aion-3.0",
                "aion-labs/aion-3.0-mini",
                "aion-labs/aion-2.0",
                "aion-labs/aion-rp-llama-3.1-8b",
            ],
            &[],
        );
        assert_static_chat_seed(
            "publicai",
            "https://api.publicai.co/v1",
            "/chat/completions",
            &["swiss-ai/apertus-v1.5-8b"],
            &[],
        );
        assert_static_chat_seed(
            "kiosapi",
            "https://api.kiosapi.id/v1",
            "/chat/completions",
            &[
                "meta/llama-3.1-8b",
                "qwen/qwen3-30b",
                "zai/glm-4.7-flash",
                "google/gemma-4-26b",
                "aisingapore/sea-lion-27b",
            ],
            &[
                "openai/text-embedding-3-small",
                "baai/bge-reranker-base",
                "deepseek/search",
            ],
        );
        assert_static_chat_seed(
            "simplellm",
            "https://api.simplellm.eu/v1",
            "/chat/completions",
            &["free-gemma-4-E4B", "free-Llama-3.1-8B-Instruct"],
            &["gemma-4-E4B"],
        );
    }

    #[test]
    fn bytez_has_no_seed_and_is_not_an_openai_compatible_catalog_provider() {
        assert!(static_seed_models("bytez").is_none());
        assert!(reviewed_static_catalog("bytez").is_none());
        assert!(reviewed_discovery_spec("bytez").is_none());
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"bytez"));
        assert!(model_discovery_supported("bytez"));
        let config = provider(
            "bytez",
            "https://api.bytez.com/models/v2/openai/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.bytez.com/models/v2/openai/v1/models"
        );
    }

    #[test]
    fn helyx_and_vectoree_seed_only_reviewed_chat_ids() {
        assert_static_chat_seed(
            "helyx-ai",
            "https://helyxai.space/v1",
            "/chat/completions",
            &[
                "agnes-3.0-flash",
                "deepseek-v4.1-flash",
                "gemini-3.1-flash-lite",
                "gemma-4-31B-it",
                "glm-5.3-flash",
                "grok-4.3",
                "grok-build-0.1",
                "muse-spark-1.1",
            ],
            &["claude-opus-5"],
        );
        assert_static_chat_seed(
            "vectoree",
            "https://vectoree.ai/api/v1",
            "/chat/completions",
            &["vectoree/auto"],
            &["alibaba/wan-3.0"],
        );
    }

    #[test]
    fn hyperbolic_seeds_the_documented_chat_models_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("hyperbolic").expect("hyperbolic seed");
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            [
                "glm-5.3-flash",
                "glm-5.3",
                "kimi-k3",
                "minimax-m3",
                "deepseek-v4.1-flash",
                "deepseek-v4-flash-0731",
                "nemotron-3-ultra-550b-a55b",
                "step-5-preview",
                "mimo-v2.6-flash",
                "hy4-preview",
                "qwen3.8-27b",
            ]
        );
        assert!(models.iter().all(|model| model.name.is_none() && model.context_length.is_none()));
        assert!(static_seed_models("hyperbolic").is_none());
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"hyperbolic"));
        let config = provider(
            "hyperbolic",
            "https://api.hyperbolic.ai/inference/v1",
            ModelProtocol::OpenaiCompletions,
            "/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Required);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.hyperbolic.ai/inference/v1/models"
        );
        let _home = TestHome::new("hyperbolic-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "hyperbolic-instance".into(),
                name: "Hyperbolic".into(),
                provider: "hyperbolic".into(),
                base_url: "https://api.hyperbolic.ai/inference/v1".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    crate::domain::types::ModelProviderProtocolConfig {
                        endpoint_path: "/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 11);
        assert_eq!(seeded[0].id, "glm-5.3-flash");
    }

    #[test]
    fn onysoft_twigpine_and_hiapi_seed_only_reviewed_chat_ids() {
        assert_static_chat_seed(
            "onysoft",
            "https://api.onysoft.com/v1",
            "/chat/completions",
            &[
                "tencent/hy4:tencent",
                "tencent/hy4-preview:tencent",
                "tencent/hy3:tencent",
                "deepseek/deepseek-v4.1-flash:tencent",
                "deepseek/deepseek-v4-flash-202605:tencent",
                "deepseek/deepseek-v4-pro-202606:tencent",
                "deepseek/deepseek-v4-pro-0813:tencent",
                "z-ai/glm-5.3:tencent",
                "z-ai/glm-5.3-flash:tencent",
                "z-ai/glm-5.3-flashx:tencent",
                "z-ai/glm-5.2:tencent",
                "moonshotai/kimi-k3:tencent",
                "moonshotai/kimi-k2.7-code:tencent",
                "moonshotai/kimi-k2.7-code-highspeed:tencent",
                "minimax/minimax-m3:tencent",
                "minimax/minimax-m3.1-flash:tencent",
            ],
            &["google/gemini-3.8-flash"],
        );
        assert_static_chat_seed(
            "twigpine-opengateway",
            "https://api.twigpine.com/v1",
            "/chat/completions",
            &[
                "auto",
                "qwen/qwen3.8-max",
                "qwen/qwen3.8-flash",
                "nvidia/nemotron-3.5-super-vl:free",
            ],
            &[
                "nvidia/nemotron-3.5-super-vl",
                "openai/gpt-6-luna",
                "qwen/qwen3.7-max",
            ],
        );
        assert_static_chat_seed(
            "hiapi",
            "https://api.hiapi.ai/v1",
            "/chat/completions",
            &["deepseek-v4.1-flash"],
            &["deepseek-v4-flash"],
        );
    }

    #[test]
    fn maritaca_and_crowllm_seed_only_reviewed_chat_ids() {
        assert_static_chat_seed(
            "maritaca",
            "https://chat.maritaca.ai/api",
            "/chat/completions",
            &["S5-free", "S5", "S5-br-sp", "S5-nano"],
            &["sabia-4", "s5-free"],
        );
        assert_static_chat_seed(
            "crowllm",
            "https://crowllm.com/v1",
            "/chat/completions",
            &[
                "qwen-3.8-27b:free",
                "deepseek-v4.1-flash:free",
                "longcat-2.5:free",
                "gemma-4-31b-it:free",
            ],
            &["agnes-2.5-flash:free", "qwen3.8-max"],
        );
    }

    #[test]
    fn respan_seeds_only_reviewed_free_chat_ids() {
        assert_static_chat_seed(
            "respan",
            "https://gateway.respan.ai/v1",
            "/chat/completions",
            &[
                "google_gemini_ai/gemini-4-carbon:free",
                "meta/muse-ember:free",
            ],
            &[
                "google_gemini_ai/gemini-4-carbon",
                "meta/muse-ember",
                "gpt-5.5",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"respan"));
    }

    #[test]
    fn hetzner_experiments_seeds_only_the_documented_chat_id() {
        assert_static_chat_seed(
            "hetzner-experiments",
            "https://inference.hetzner.com/api/v1",
            "/chat/completions",
            &["Qwen/Qwen3.6-35B-A3B-FP8"],
            &[
                "Qwen/Qwen3.6-35B-A3B",
                "Kimi-K2.7-Code",
                "GLM-5.2",
                "DeepSeek-V4-Flash-0731",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"hetzner-experiments"));
    }

    #[test]
    fn powertokens_seeds_only_the_free_chat_id() {
        assert_static_chat_seed(
            "powertokens",
            "https://api.powertokens.ai/v1",
            "/chat/completions",
            &["glm-4.7-flash"],
            &["MiniMax-M3", "glm-4.7", "glm-5.2"],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"powertokens"));
    }

    #[test]
    fn free_ai_seeds_only_the_reviewed_self_hosted_chat_ids() {
        assert_static_chat_seed(
            "free-ai",
            "https://api.free.ai/v1",
            "/chat/completions",
            &["qwen7b", "qwen-coder"],
            &[
                "qwen3-coder",
                "deepseek-r1",
                "mistral",
                "qwen3-8b",
                "deepseek-r1-7b",
                "qwen-vl",
                "qwen25-vl",
                "Qwen/Qwen3-30B-A3B-Instruct",
                "sdxl",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"free-ai"));
    }

    #[test]
    fn suanli_maas_seeds_only_the_reviewed_chat_ids() {
        assert_static_chat_seed(
            "suanli-maas",
            "https://api.suanli.cn/v1",
            "/chat/completions",
            &[
                "deepseek/deepseek-v4-flash",
                "deepseek/deepseek-v4-pro",
                "moonshotai/kimi-k3",
                "z-ai/glm-5.2",
                "qwen/qwen3.7-max",
            ],
            &[
                "moonshotai/kimi-k2.6",
                "z-ai/glm-5",
                "z-ai/glm-5.1",
                "tencent/hy3-preview",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"suanli-maas"));
    }

    #[test]
    fn botscanner_seeds_only_the_reviewed_router_knobs() {
        assert_static_chat_seed(
            "botscanner",
            "https://api.autobench.org/v1",
            "/chat/completions",
            &[
                "autobench/beta-cost",
                "autobench/beta-balanced",
                "autobench/beta-quality",
            ],
            &[
                "qwen/qwen3.7-flash",
                "openai/gpt-6-luna",
                "anthropic/claude-opus-5.5",
                "google/gemini-4-argon",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"botscanner"));
    }

    #[test]
    fn cucloud_seeds_only_the_reviewed_console_model_names() {
        assert_static_chat_seed(
            "cucloud",
            "https://console-issued.example/v1",
            "/chat/completions",
            &[
                "DeepSeek-V4.1-Flash-1017",
                "DeepSeek-V4-Pro",
                "Qwen3.8-Max",
                "Qwen3.8-Flash",
                "glm-5.3",
            ],
            &["GLM-5.3", "HappyHorse1.1"],
        );
    }

    #[test]
    fn puter_seeds_only_the_reviewed_chat_ids() {
        assert_static_chat_seed(
            "puter",
            "https://api.puter.com/puterai/openai/v1",
            "/chat/completions",
            &["meta/muse-wonder", "gpt-5.4-nano", "claude-sonnet-5-5"],
            &[
                "openai/gpt-5.4-nano",
                "anthropic/claude-sonnet-5-5",
                "gemini-3.5-flash-lite",
                "grok-4.7",
                "claude-opus-5-5",
                "claude-haiku-5-5",
                "meta/muse-ember",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"puter"));
    }

    #[test]
    fn moss_seeds_only_the_reviewed_chat_id() {
        assert_static_chat_seed(
            "moss",
            "https://api.mosi.cn/v1",
            "/chat/completions",
            &["nex-n2.6"],
            &[
                "nex-n2.6-2026-11-05",
                "moss-tts-1.5-flash",
                "moss-transcribe-diarize-pro",
                "moss-vl-1.0",
            ],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"moss"));
    }

    #[test]
    fn minekuai_seeds_only_the_reviewed_chat_ids() {
        assert_static_chat_seed(
            "minekuai",
            "https://minekuai.cn/v1",
            "/chat/completions",
            &["deepseek-flash", "deepseek-v4-pro"],
            &["deepseek-v4.1-flash-1017"],
        );
        assert!(!EXPECTED_OPENAI_COMPATIBLE.contains(&"minekuai"));
    }

    #[test]
    fn cheapllm_shop_seeds_the_public_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("cheapllm-shop").expect("cheapllm seed");
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            ["deepseek-v4.1-flash-1017"]
        );
        assert!(models
            .iter()
            .all(|model| model.name.is_none() && model.context_length.is_none()));
        assert!(models.iter().all(|model| model.id != "deepseek-v4.1-flash"));
        assert!(static_seed_models("cheapllm-shop").is_none());
        assert!(EXPECTED_OPENAI_COMPATIBLE.contains(&"cheapllm-shop"));
        let config = provider(
            "cheapllm-shop",
            "https://cheapllm.shop",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://cheapllm.shop/v1/models"
        );
        let _home = TestHome::new("cheapllm-shop-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "cheapllm-shop-instance".into(),
                name: "cheapllm.shop".into(),
                provider: "cheapllm-shop".into(),
                base_url: "https://cheapllm.shop".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    crate::domain::types::ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].id, "deepseek-v4.1-flash-1017");
    }

    #[test]
    fn coarena_seeds_the_public_catalog_without_a_key_and_discovers_with_one() {
        let models = reviewed_static_catalog("coarena").expect("coarena seed");
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            [
                "claude-fable-5",
                "claude-fable-5-1",
                "claude-fable-5-2",
                "claude-haiku-5-5",
                "claude-opus-5-5",
                "claude-sonnet-5-5",
                "deepseek-v4-1-flash-1017",
                "gemini-3-8-flash",
                "gemini-4-argon",
                "gemini-4-carbon",
                "glm-5-3-flash",
                "gpt-6-1-sol",
                "gpt-6-1-terra",
                "gpt-6-astra",
                "gpt-6-luna",
                "grok-4-7",
                "hy4",
                "kimi-k3",
                "kimi-k3-1-pro",
                "mimo-v2-6-pro",
                "minimax-m3-1-flash",
                "qwen3-8-2-4t-a95b",
            ]
        );
        assert!(models.iter().all(|model| model.name.is_none() && model.context_length.is_none()));
        assert!(models.iter().all(|model| model.id != "anthropic/claude-fable-5"));
        assert!(static_seed_models("coarena").is_none());
        assert!(EXPECTED_OPENAI_COMPATIBLE.contains(&"coarena"));
        let config = provider(
            "coarena",
            "https://coarena.ai",
            ModelProtocol::OpenaiCompletions,
            "/api/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://coarena.ai/api/v1/models"
        );
        let _home = TestHome::new("coarena-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "coarena-instance".into(),
                name: "Coarena".into(),
                provider: "coarena".into(),
                base_url: "https://coarena.ai".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    crate::domain::types::ModelProviderProtocolConfig {
                        endpoint_path: "/api/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 22);
        assert_eq!(seeded[0].id, "claude-fable-5");
        assert_eq!(seeded[21].id, "qwen3-8-2-4t-a95b");
    }

    #[test]
    fn ucloud_modelverse_seeds_the_reviewed_chat_ids_and_discovers_with_a_key() {
        let models = reviewed_static_catalog("ucloud-modelverse").expect("modelverse seed");
        assert_eq!(
            models.iter().map(|model| model.id.as_str()).collect::<Vec<_>>(),
            [
                "deepseek-v4.1-flash-1017",
                "deepseek-v4-pro-0813",
                "glm-5.3-flash",
                "glm-5.3",
                "qwen3.7-flash",
                "kimi-k3.1-pro",
            ]
        );
        assert!(models
            .iter()
            .all(|model| model.name.is_none() && model.context_length.is_none()));
        for omitted in [
            "deepseek-v4.1-flash",
            "qwen3.8-max",
            "kimi-k3",
            "auto",
            "qwen-audio-3.1-tts-plus",
            "BAAI/bge-m3",
            "wan3.0-video",
            "happyhorse-1.1-t2v",
        ] {
            assert!(!models.iter().any(|model| model.id == omitted), "{omitted}");
        }
        assert!(static_seed_models("ucloud-modelverse").is_none());
        assert!(EXPECTED_OPENAI_COMPATIBLE.contains(&"ucloud-modelverse"));
        let config = provider(
            "ucloud-modelverse",
            "https://api.modelverse.cn",
            ModelProtocol::OpenaiCompletions,
            "/v1/chat/completions",
        );
        let spec = discovery_spec_for_provider(&config).unwrap();
        assert_eq!(spec.adapter, DiscoveryAdapter::OpenAi);
        assert_eq!(spec.credential, CredentialPolicy::Optional);
        assert_eq!(
            discovery_url(&config, spec.adapter).unwrap().as_str(),
            "https://api.modelverse.cn/v1/models"
        );
        let _home = TestHome::new("ucloud-modelverse-static-catalog");
        let seeded = execute_provider_discovery(ModelDiscoveryInput {
            provider: ModelProviderConfig {
                id: "ucloud-modelverse-instance".into(),
                name: "UCloud 星图 AstraFlow（ModelVerse）".into(),
                provider: "ucloud-modelverse".into(),
                base_url: "https://api.modelverse.cn".into(),
                model_catalog_url: None,
                portal_url: None,
                protocols: BTreeMap::from([(
                    ModelProtocol::OpenaiCompletions,
                    crate::domain::types::ModelProviderProtocolConfig {
                        endpoint_path: "/v1/chat/completions".into(),
                    },
                )]),
                auth_requirement: AuthRequirement::Required,
                api_key_source: None,
                env_key: None,
            },
        })
        .unwrap();
        assert_eq!(seeded.len(), 6);
        assert_eq!(seeded[0].id, "deepseek-v4.1-flash-1017");
        assert_eq!(seeded[5].id, "kimi-k3.1-pro");
    }
}
