//! Shared Provider connections, reusable Model records, and safe per-Agent
//! configuration writers.
//!
//! Managed Agents are written only through verified user-level configuration
//! surfaces. Multi-model Agents keep Arca provider entries independently from
//! their one active primary pointer; unsupported or insecure surfaces remain
//! guidance-only.

pub(crate) mod adapters;
mod claude_desktop;
pub(crate) mod credential;
mod discovery;
mod open_code_auth;

pub use discovery::{discover_provider_models, ProviderModelSummary};
pub mod curl;
pub(crate) use discovery::{prepare_provider_discovery, execute_provider_discovery};

#[cfg(test)]
pub(crate) use claude_desktop::set_config_write_hook as set_claude_desktop_config_write_hook;

#[cfg(target_os = "macos")]
mod keychain;

use crate::domain::agents::ModelStorageAuthority;
use crate::domain::types::{
    migrate_legacy_provider_endpoints, ApiKeySource, AuthRequirement, ModelProfile, ModelProtocol,
    ModelProviderConfig, ModelProviderProtocolConfig,
};
use crate::paths::{backup_timestamp, backups_dir, settings_file};
use crate::resources::mcp::applier::{backup, backup_bytes};
use crate::resources::mcp::scanner::{collapse_home, expand_tilde};
use crate::safe_write::{
    acquire_settings_lock, remove_if_unchanged, write_if_unchanged, SettingsLock,
};
use crate::settings::{load_settings, mutate_settings};
use jsonc_parser::cst::{CstInputValue, CstNode, CstObject, CstRootNode};
use jsonc_parser::ParseOptions;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(any(test, debug_assertions))]
use std::sync::Mutex;
use std::sync::LazyLock;
use toml_edit::{Array, Document, Item, Table};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

// Persisted credential and native ownership identifiers intentionally remain stable.
// See analysis/18-arca-brand-transition.md; these strings must not be brand-replaced.
const KEYCHAIN_ACCOUNT: &str = "api-key";
const CREDENTIAL_ROLLBACK_PREFIX: &str = "__asset-operation-rollback__";
const PRIVATE_FILE_SNAPSHOT_SERVICE_PREFIX: &str = "com.scoheart.mux.private-file-snapshot";
const PRIVATE_FILE_SNAPSHOT_VERSION: &str = "v1";
const QODER_DOCS: &str = "https://docs.qoder.com/user-guide/chat/custom-models";
const QODER_CLI_DOCS: &str = "https://docs.qoder.com/cli/custom-models";
const QODER_DESKTOP_DOCS: &str = "https://docs.qoder.com/qoder/custom-models";
const GROK_BUILD_MODEL_DOCS: &str = "https://docs.x.ai/build/settings#model-id";
const MINIMAX_CODE_DOCS: &str = "https://agent.minimax.io/download";
const CREDENTIAL_FILE_NATIVE_ID: &str = "__mux_credential_file";
const CREDENTIAL_COMMAND_NATIVE_ID: &str = "__mux_credential_command";
const CREDENTIAL_ARGS_NATIVE_ID: &str = "__mux_credential_args";

fn mutation_lock() -> Result<SettingsLock, String> {
    acquire_settings_lock(&settings_file())
}

#[cfg(any(test, debug_assertions))]
static TEST_CREDENTIALS: LazyLock<Mutex<BTreeMap<String, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[cfg(test)]
thread_local! {
    static TEST_CREDENTIAL_READ_ERROR: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    static TEST_CREDENTIAL_DELETE_ERROR: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct TestCredentialDeleteErrorGuard;

#[cfg(test)]
pub(crate) struct TestCredentialReadErrorGuard;

#[cfg(test)]
impl Drop for TestCredentialReadErrorGuard {
    fn drop(&mut self) {
        TEST_CREDENTIAL_READ_ERROR.with(|slot| slot.borrow_mut().take());
    }
}

#[cfg(test)]
pub(crate) fn set_test_credential_read_error() -> TestCredentialReadErrorGuard {
    TEST_CREDENTIAL_READ_ERROR.with(|slot| {
        *slot.borrow_mut() = Some("credential_read_failed: injected read failure".into());
    });
    TestCredentialReadErrorGuard
}

#[cfg(test)]
impl Drop for TestCredentialDeleteErrorGuard {
    fn drop(&mut self) {
        TEST_CREDENTIAL_DELETE_ERROR.with(|slot| slot.borrow_mut().take());
    }
}

#[cfg(test)]
pub(crate) fn set_test_credential_delete_error(
    error: impl Into<String>,
) -> TestCredentialDeleteErrorGuard {
    TEST_CREDENTIAL_DELETE_ERROR.with(|slot| *slot.borrow_mut() = Some(error.into()));
    TestCredentialDeleteErrorGuard
}

#[cfg(any(test, debug_assertions))]
fn test_credential_key(service: &str) -> String {
    format!(
        "{}::{service}",
        crate::brand::env("ARCA_TEST_PROBE_ROOT").unwrap_or_default()
    )
}

#[cfg(test)]
fn test_credential_delete_error() -> Option<String> {
    TEST_CREDENTIAL_DELETE_ERROR.with(|slot| slot.borrow().clone())
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelProfileView {
    #[serde(flatten)]
    pub profile: ModelProfile,
    pub catalog_key: String,
    pub credential_saved: bool,
}

#[derive(Debug, Clone)]
pub struct ModelProviderView {
    pub id: &'static str,
    pub name: &'static str,
    pub default_base_url: Option<&'static str>,
    pub default_protocol: ModelProtocol,
    pub category: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelProviderInstanceView {
    #[serde(flatten)]
    pub provider: ModelProviderConfig,
    pub portal: Option<ModelProviderPortalView>,
    pub credential_saved: bool,
    pub model_count: usize,
    pub model_discovery_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelProviderEndpointView {
    pub protocol: ModelProtocol,
    pub base_url: &'static str,
}

/// Display-only setup metadata. Account/region placeholders never become a
/// connection default or get persisted in a Provider instance.
#[derive(Debug, Clone, Serialize)]
pub struct ModelProviderSetupView {
    pub base_url_placeholder: &'static str,
    pub hint: &'static str,
}

fn provider_setup(id: &str) -> Option<ModelProviderSetupView> {
    let (base_url_placeholder, hint) = match id {
        "azure-openai" => ("https://<resource>.openai.azure.com/openai/v1", "azure"),
        "amazon-bedrock-mantle" => ("https://bedrock-mantle.<region>.api.aws/v1", "bedrock"),
        "cloudflare-workers-ai" => ("https://api.cloudflare.com/client/v4/accounts/<account-id>/ai/v1", "cloudflare"),
        "tinfoil-proxy" => ("http://127.0.0.1:3301/v1", "tinfoil"),
        _ => return None,
    };
    Some(ModelProviderSetupView { base_url_placeholder, hint })
}

/// Official account access metadata, separate from inference endpoints and
/// stored credentials. A console is not advertised as a direct key page.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderPortalView {
    pub url: String,
    pub kind: ModelProviderPortalKind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProviderPortalKind {
    ApiKey,
    Console,
    Setup,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelProviderLinks {
    docs_url: String,
    portal: ModelProviderPortalView,
}

static PROVIDER_LINKS: LazyLock<BTreeMap<String, ModelProviderLinks>> = LazyLock::new(|| {
    let links: BTreeMap<String, ModelProviderLinks> =
        serde_json::from_str(include_str!("../../../../data/provider-links.json"))
            .expect("provider link catalog must be valid");
    for (id, entry) in &links {
        assert!(MODEL_PROVIDERS.iter().any(|provider| provider.id == id.as_str() && provider.category != "custom"),
            "unknown provider link: {id}");
        for value in [&entry.docs_url, &entry.portal.url] {
            let url = url::Url::parse(value).expect("provider links must be valid URLs");
            assert!(url.scheme() == "https" && url.host_str().is_some()
                && url.username().is_empty() && url.password().is_none(),
                "provider links must be credential-free HTTPS URLs: {id}");
        }
    }
    assert!(MODEL_PROVIDERS.iter().filter(|provider| provider.category != "custom")
        .all(|provider| links.contains_key(provider.id)), "provider link catalog must cover every built-in template");
    links
});

pub fn provider_docs_url(id: &str) -> Option<&'static str> {
    PROVIDER_LINKS.get(id).map(|links| links.docs_url.as_str())
}

pub fn provider_portal(id: &str) -> Option<&'static ModelProviderPortalView> {
    PROVIDER_LINKS.get(id).map(|links| &links.portal)
}

pub fn provider_instance_portal(provider: &ModelProviderConfig) -> Option<ModelProviderPortalView> {
    let template = provider_portal(&provider.provider);
    provider
        .portal_url
        .as_ref()
        .and_then(|url| normalize_provider_portal_url(url).ok())
        .map(|url| ModelProviderPortalView {
            url,
            kind: template.map(|portal| portal.kind.clone()).unwrap_or_else(|| {
                if provider.auth_requirement == crate::domain::types::AuthRequirement::None {
                    ModelProviderPortalKind::Setup
                } else {
                    ModelProviderPortalKind::ApiKey
                }
            }),
        })
        .or_else(|| template.cloned())
}

impl Serialize for ModelProviderView {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let (base_url, protocols) =
            provider_template_connection(self).map_err(serde::ser::Error::custom)?;
        let mut view = serializer.serialize_struct("ModelProviderView", 12)?;
        view.serialize_field("id", self.id)?;
        view.serialize_field("name", self.name)?;
        view.serialize_field("docs_url", &provider_docs_url(self.id))?;
        view.serialize_field("portal", &provider_portal(self.id))?;
        view.serialize_field("default_base_url", &self.default_base_url)?;
        view.serialize_field("default_protocol", &self.default_protocol)?;
        view.serialize_field(
            "additional_endpoints",
            provider_additional_endpoints(self.id),
        )?;
        view.serialize_field("base_url", &base_url)?;
        view.serialize_field("protocols", &protocols)?;
        view.serialize_field("category", self.category)?;
        view.serialize_field("setup", &provider_setup(self.id))?;
        view.serialize_field(
            "model_discovery_supported",
            &discovery::model_discovery_supported(self.id),
        )?;
        view.end()
    }
}

fn provider_template_connection(
    provider: &ModelProviderView,
) -> Result<
    (
        Option<String>,
        BTreeMap<ModelProtocol, ModelProviderProtocolConfig>,
    ),
    String,
> {
    let Some(default_base_url) = provider.default_base_url else {
        return Ok((
            None,
            if provider.id == "azure-openai" {
                [ModelProtocol::OpenaiResponses, ModelProtocol::OpenaiCompletions]
                    .into_iter()
                    .map(|protocol| {
                        let endpoint_path = protocol.default_endpoint_path().into();
                        (protocol, ModelProviderProtocolConfig { endpoint_path })
                    })
                    .collect()
            } else {
                BTreeMap::from([(
                    provider.default_protocol.clone(),
                    ModelProviderProtocolConfig {
                        endpoint_path: provider.default_protocol.default_endpoint_path().into(),
                    },
                )])
            },
        ));
    };
    let mut legacy = BTreeMap::from([(
        provider.default_protocol.clone(),
        default_base_url.to_string(),
    )]);
    legacy.extend(
        provider_additional_endpoints(provider.id)
            .iter()
            .map(|endpoint| (endpoint.protocol.clone(), endpoint.base_url.to_string())),
    );
    let (base_url, protocols) = migrate_legacy_provider_endpoints(provider.name, &legacy)?;
    Ok((Some(base_url), protocols))
}

pub fn provider_additional_endpoints(id: &str) -> &'static [ModelProviderEndpointView] {
    use ModelProtocol::{AnthropicMessages, GeminiGenerateContent, OpenaiCompletions, OpenaiResponses};

    match id {
        "gmi-cloud" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://api.gmi-serving.com/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.gmi-serving.com",
            },
        ],
        "infini-ai" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://cloud.infini-ai.com/maas",
        }],
        "aihubmix" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://aihubmix.com/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://aihubmix.com",
            },
            ModelProviderEndpointView {
                protocol: GeminiGenerateContent,
                base_url: "https://aihubmix.com/v1beta",
            },
        ],
        "opencode-zen" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://opencode.ai/zen/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://opencode.ai/zen",
            },
        ],
        "opencode-go" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://opencode.ai/zen/go/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://opencode.ai/zen/go",
            },
        ],
        "nanogpt" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://api.nano-gpt.com/api/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.nano-gpt.com/api",
            },
        ],
        "synthetic" => &[
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.synthetic.new/anthropic",
            },
        ],
        "venice" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://api.venice.ai/api/v1",
            },
        ],
        "ovhcloud" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://oai.endpoints.kepler.ai.cloud.ovh.net/v1",
            },
        ],
        "upstage" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://api.upstage.ai/v1",
            },
        ],
        "zenmux" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://zenmux.ai/api/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://zenmux.ai/api/anthropic",
            },
        ],
        "byteplus" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://ark.ap-southeast.bytepluses.com/api/v3",
            },
        ],
        "longcat" => &[
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.longcat.chat/anthropic",
            },
        ],
        "iflytek-spark-x2" => &[
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://spark-api-open.xf-yun.com/anthropic/agent",
            },
        ],
        "ollama-cloud" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://ollama.com/v1",
            },
        ],
        "vercel-ai-gateway" => &[ModelProviderEndpointView {
            protocol: OpenaiCompletions,
            base_url: "https://ai-gateway.vercel.sh/v1",
        }],
        "routeway" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://api.routeway.ai/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.routeway.ai",
            },
        ],
        "infron" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://llm.onerouter.pro/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://llm.onerouter.pro",
            },
        ],
        "apinex" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.apinex.bond",
        }],
        "tokenharbor" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://tokenharbor.ai/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://tokenharbor.ai",
            },
        ],
        "iteracompute" => &[ModelProviderEndpointView {
            protocol: OpenaiResponses,
            base_url: "https://api.iteracompute.com/v1",
        }],
        "munito" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://api.munito.ai/inference/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.munito.ai/inference",
            },
        ],
        "ppq" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://api.ppq.ai/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.ppq.ai",
            },
        ],
        "river" => &[ModelProviderEndpointView {
            protocol: OpenaiCompletions,
            base_url: "https://api.river.ai/v1",
        }],
        "trustedrouter" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://api.trustedrouter.com/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.trustedrouter.com",
            },
        ],
        "unorouter" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://api.unorouter.com/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.unorouter.com",
            },
            ModelProviderEndpointView {
                protocol: GeminiGenerateContent,
                base_url: "https://api.unorouter.com/v1beta",
            },
        ],
        "ant-ling" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.ant-ling.com/anthropic",
        }],
        "antdigital-dtmaas" => &[ModelProviderEndpointView {
            protocol: OpenaiResponses,
            base_url: "https://maas-api.antdigital.com/v1",
        }],
        "atria" => &[
            ModelProviderEndpointView {
                protocol: OpenaiCompletions,
                base_url: "https://api.atria-asi.ai/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://api.atria-asi.ai",
            },
        ],
        "unisound" => &[
            ModelProviderEndpointView {
                protocol: OpenaiResponses,
                base_url: "https://maas-api.unisound.com/v1",
            },
            ModelProviderEndpointView {
                protocol: AnthropicMessages,
                base_url: "https://maas-api.unisound.com/anthropic",
            },
        ],
        "minimax" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.minimax.io/anthropic",
        }],
        "minimax-cn" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.minimax.cn/anthropic",
        }],
        "volcengine" => &[ModelProviderEndpointView {
            protocol: OpenaiResponses,
            base_url: "https://ark.cn-beijing.volces.com/api/v3",
        }],
        "volcengine-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://ark.cn-beijing.volces.com/api/coding",
        }],
        "baidu-qianfan-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://qianfan.baidubce.com/anthropic/coding",
        }],
        "google" => &[ModelProviderEndpointView {
            protocol: OpenaiCompletions,
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        }],
        "zai-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.z.ai/api/anthropic",
        }],
        "zhipuai-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://open.bigmodel.cn/api/anthropic",
        }],
        "alibaba-coding-plan-cn" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://coding.dashscope.aliyuncs.com/apps/anthropic",
        }],
        "alibaba-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic",
        }],
        "alibaba-token-plan-cn" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic",
        }],
        "alibaba-token-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic",
        }],
        "xiaomi-token-plan-cn" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://token-plan-cn.xiaomimimo.com/anthropic",
        }],
        "xiaomi-token-plan-sgp" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://token-plan-sgp.xiaomimimo.com/anthropic",
        }],
        "xiaomi-token-plan-ams" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://token-plan-ams.xiaomimimo.com/anthropic",
        }],
        "kimi-for-coding" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.kimi.com/coding",
        }],
        "minimax-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.minimax.io/anthropic",
        }],
        "minimax-cn-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.minimax.cn/anthropic",
        }],
        "stepfun-step-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.stepfun.com/step_plan",
        }],
        "stepfun-ai-step-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.stepfun.ai/step_plan",
        }],
        "tencent-coding-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.lkeap.cloud.tencent.com/coding/anthropic",
        }],
        "tencent-token-plan" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://api.lkeap.cloud.tencent.com/plan/anthropic",
        }],
        "tencent-token-plan-global" => &[ModelProviderEndpointView {
            protocol: AnthropicMessages,
            base_url: "https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic",
        }],
        _ => &[],
    }
}

// Plan-specific endpoints and their official references are audited in
// `core/src/resources/model/PROVIDER_SOURCES.md`.
const MODEL_PROVIDERS: &[ModelProviderView] = &[
    ModelProviderView {
        id: "portkey",
        name: "Portkey",
        default_base_url: Some("https://api.portkey.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "helicone",
        name: "Helicone AI Gateway",
        default_base_url: Some("https://ai-gateway.helicone.ai"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "ppio",
        name: "PPIO",
        default_base_url: Some("https://api.ppio.com/openai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "infini-ai",
        name: "Infini AI GenStudio",
        default_base_url: Some("https://cloud.infini-ai.com/maas/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "aihubmix",
        name: "AIHubMix",
        default_base_url: Some("https://aihubmix.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "302-ai",
        name: "302.AI",
        default_base_url: Some("https://api.302ai.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "opencode-zen",
        name: "OpenCode Zen",
        default_base_url: Some("https://opencode.ai/zen/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "opencode-go",
        name: "OpenCode Go",
        default_base_url: Some("https://opencode.ai/zen/go/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "kilo-gateway",
        name: "Kilo Gateway",
        default_base_url: Some("https://api.kilo.ai/api/gateway"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "nanogpt",
        name: "NanoGPT",
        default_base_url: Some("https://api.nano-gpt.com/api/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "synthetic",
        name: "Synthetic",
        default_base_url: Some("https://api.synthetic.new/openai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "chutes",
        name: "Chutes",
        default_base_url: Some("https://llm.chutes.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "featherless-ai",
        name: "Featherless AI",
        default_base_url: Some("https://api.featherless.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "venice",
        name: "Venice",
        default_base_url: Some("https://api.venice.ai/api/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "friendli",
        name: "FriendliAI",
        default_base_url: Some("https://api.friendli.ai/serverless/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "inference-net",
        name: "Inference.net",
        default_base_url: Some("https://api.inference.net/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "ovhcloud",
        name: "OVHcloud AI Endpoints",
        default_base_url: Some("https://oai.endpoints.kepler.ai.cloud.ovh.net/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "upstage",
        name: "Upstage",
        default_base_url: Some("https://api.upstage.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "ai21",
        name: "AI21 Labs",
        default_base_url: Some("https://api.ai21.com/studio/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "zenmux",
        name: "ZenMux",
        default_base_url: Some("https://zenmux.ai/api/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "byteplus",
        name: "BytePlus ModelArk",
        default_base_url: Some("https://ark.ap-southeast.bytepluses.com/api/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tencent-tokenhub-cn",
        name: "Tencent TokenHub (China)",
        default_base_url: Some("https://tokenhub.tencentmaas.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tencent-tokenhub-global",
        name: "Tencent TokenHub (Global)",
        default_base_url: Some("https://tokenhub-intl.tencentcloudmaas.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "longcat",
        name: "LongCat",
        default_base_url: Some("https://api.longcat.chat/openai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "iflytek-spark-x2",
        name: "iFLYTEK Spark X2",
        default_base_url: Some("https://spark-api-open.xf-yun.com/agent/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "nscale",
        name: "Nscale",
        default_base_url: Some("https://inference.api.nscale.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "io-net",
        name: "io.net",
        default_base_url: Some("https://api.intelligence.io.solutions/api/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "inception",
        name: "Inception",
        default_base_url: Some("https://api.inceptionlabs.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "ollama-cloud",
        name: "Ollama Cloud",
        default_base_url: Some("https://ollama.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tinfoil-proxy",
        name: "Tinfoil Proxy",
        default_base_url: Some("http://127.0.0.1:3301/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "local",
    },
    ModelProviderView {
        id: "vercel-ai-gateway",
        name: "Vercel AI Gateway",
        default_base_url: Some("https://ai-gateway.vercel.sh/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "routeway",
        name: "Routeway",
        default_base_url: Some("https://api.routeway.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "infron",
        name: "Infron",
        default_base_url: Some("https://llm.onerouter.pro/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "apinex",
        name: "APInex",
        default_base_url: Some("https://api.apinex.bond/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "tokenharbor",
        name: "Token Harbor",
        default_base_url: Some("https://tokenharbor.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "flock",
        name: "FLock API Platform",
        default_base_url: Some("https://api.flock.io/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "codecraft",
        name: "CodeCraft API",
        default_base_url: Some("https://codecraftapi.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "beatapi",
        name: "BeatAPI",
        default_base_url: Some("https://api.beatapi.io/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "experiential-labs",
        name: "Experiential Labs",
        default_base_url: Some("https://api.experientiallabs.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "runanywhere",
        name: "RunAnywhere",
        default_base_url: Some("https://inference.runanywhere.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "clean-apis",
        name: "Clean APIs",
        default_base_url: Some("https://cleanapis.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "dolphin",
        name: "Dolphin AI",
        default_base_url: Some("https://lighthouse.dphn.ai/run/text-v/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "iteracompute",
        name: "IteraCompute",
        default_base_url: Some("https://api.iteracompute.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "llmtr",
        name: "LLMTR",
        default_base_url: Some("https://llmtr.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "munito",
        name: "Munito",
        default_base_url: Some("https://api.munito.ai/inference/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "nous-portal",
        name: "Nous Portal",
        default_base_url: Some("https://inference-api.nousresearch.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "ppq",
        name: "PayPerQ",
        default_base_url: Some("https://api.ppq.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "river",
        name: "River AI",
        default_base_url: Some("https://api.river.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "trustedrouter",
        name: "TrustedRouter",
        default_base_url: Some("https://api.trustedrouter.com/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "unorouter",
        name: "UnoRouter",
        default_base_url: Some("https://api.unorouter.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "ant-ling",
        name: "Ant Ling (蚂蚁百灵)",
        default_base_url: Some("https://api.ant-ling.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "antdigital-dtmaas",
        name: "Ant Digital DTMaaS (蚂蚁数科)",
        default_base_url: Some("https://maas-api.antdigital.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "unisound",
        name: "Unisound MaaS (云知声)",
        default_base_url: Some("https://maas-api.unisound.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "azure-openai",
        name: "Azure OpenAI",
        default_base_url: None,
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "official",
    },
    ModelProviderView {
        id: "amazon-bedrock-mantle",
        name: "Amazon Bedrock Mantle",
        default_base_url: None,
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "official",
    },
    ModelProviderView {
        id: "cloudflare-workers-ai",
        name: "Cloudflare Workers AI",
        default_base_url: None,
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "deepinfra",
        name: "DeepInfra",
        default_base_url: Some("https://api.deepinfra.com/v1/openai"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "gmi-cloud",
        name: "GMI Cloud",
        default_base_url: Some("https://api.gmi-serving.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "sambanova",
        name: "SambaNova",
        default_base_url: Some("https://api.sambanova.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "perplexity",
        name: "Perplexity Sonar",
        default_base_url: Some("https://api.perplexity.ai"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "volcengine",
        name: "Volcengine Ark (火山方舟)",
        default_base_url: Some("https://ark.cn-beijing.volces.com/api/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "volcengine-coding-plan",
        name: "Volcengine Coding Plan (火山方舟)",
        default_base_url: Some("https://ark.cn-beijing.volces.com/api/coding/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "baidu-qianfan",
        name: "Baidu Qianfan (百度千帆)",
        default_base_url: Some("https://qianfan.baidubce.com/v2"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "baidu-qianfan-coding-plan",
        name: "Baidu Qianfan Coding Plan (百度千帆)",
        default_base_url: Some("https://qianfan.baidubce.com/v2/coding"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "minimax",
        name: "MiniMax (Global)",
        default_base_url: Some("https://api.minimax.io/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "minimax-cn",
        name: "MiniMax (China)",
        default_base_url: Some("https://api.minimax.cn/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "stepfun",
        name: "StepFun (China)",
        default_base_url: Some("https://api.stepfun.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "stepfun-global",
        name: "StepFun (Global)",
        default_base_url: Some("https://api.stepfun.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "zhipuai",
        name: "Zhipu AI (智谱)",
        default_base_url: Some("https://open.bigmodel.cn/api/paas/v4"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "moonshotai-cn",
        name: "Moonshot AI (China)",
        default_base_url: Some("https://api.moonshot.cn/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba-international",
        name: "Alibaba Cloud (Singapore)",
        default_base_url: Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "siliconflow-cn",
        name: "SiliconFlow (China)",
        default_base_url: Some("https://api.siliconflow.cn/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "openrouter",
        name: "OpenRouter",
        default_base_url: Some("https://openrouter.ai/api/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "anthropic",
        name: "Anthropic",
        default_base_url: Some("https://api.anthropic.com"),
        default_protocol: ModelProtocol::AnthropicMessages,
        category: "official",
    },
    ModelProviderView {
        id: "openai",
        name: "OpenAI",
        default_base_url: Some("https://api.openai.com/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "official",
    },
    ModelProviderView {
        id: "google",
        name: "Google AI Studio",
        default_base_url: Some("https://generativelanguage.googleapis.com/v1beta"),
        default_protocol: ModelProtocol::GeminiGenerateContent,
        category: "official",
    },
    ModelProviderView {
        id: "xai",
        name: "xAI",
        default_base_url: Some("https://api.x.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "official",
    },
    ModelProviderView {
        id: "mistral",
        name: "Mistral AI",
        default_base_url: Some("https://api.mistral.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "sarashina",
        name: "Sarashina",
        default_base_url: Some("https://api.sarashina.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "liquid-ai",
        name: "Liquid AI",
        default_base_url: Some("https://inference.liquid.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "atria",
        name: "Atria",
        default_base_url: Some("https://api.atria-asi.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "official",
    },
    ModelProviderView {
        id: "cohere",
        name: "Cohere",
        default_base_url: Some("https://api.cohere.ai/compatibility/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "deepseek",
        name: "DeepSeek",
        default_base_url: Some("https://api.deepseek.com"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "groq",
        name: "Groq",
        default_base_url: Some("https://api.groq.com/openai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba",
        name: "Alibaba Cloud",
        default_base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba-coding-plan-cn",
        name: "Alibaba Coding Plan (China)",
        default_base_url: Some("https://coding.dashscope.aliyuncs.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba-coding-plan",
        name: "Alibaba Coding Plan (Global)",
        default_base_url: Some("https://coding-intl.dashscope.aliyuncs.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba-token-plan-cn",
        name: "Alibaba Token Plan (China)",
        default_base_url: Some(
            "https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
        ),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "alibaba-token-plan",
        name: "Alibaba Token Plan (Global)",
        default_base_url: Some(
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
        ),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "xiaomi",
        name: "Xiaomi MiMo",
        default_base_url: Some("https://api.xiaomimimo.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "xiaomi-token-plan-cn",
        name: "Xiaomi MiMo Token Plan (China)",
        default_base_url: Some("https://token-plan-cn.xiaomimimo.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "xiaomi-token-plan-sgp",
        name: "Xiaomi MiMo Token Plan (Singapore)",
        default_base_url: Some("https://token-plan-sgp.xiaomimimo.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "xiaomi-token-plan-ams",
        name: "Xiaomi MiMo Token Plan (Europe)",
        default_base_url: Some("https://token-plan-ams.xiaomimimo.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "moonshotai",
        name: "Moonshot AI",
        default_base_url: Some("https://api.moonshot.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "kimi-for-coding",
        name: "Kimi Code Membership",
        default_base_url: Some("https://api.kimi.com/coding/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "zai",
        name: "Z.AI",
        default_base_url: Some("https://api.z.ai/api/paas/v4"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "zai-coding-plan",
        name: "Z.AI Coding Plan",
        default_base_url: Some("https://api.z.ai/api/coding/paas/v4"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "zhipuai-coding-plan",
        name: "Zhipu AI Coding Plan (China)",
        default_base_url: Some("https://open.bigmodel.cn/api/coding/paas/v4"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "minimax-coding-plan",
        name: "MiniMax Token Plan (Global)",
        default_base_url: Some("https://api.minimax.io/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "minimax-cn-coding-plan",
        name: "MiniMax Token Plan (China)",
        default_base_url: Some("https://api.minimax.cn/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "stepfun-step-plan",
        name: "StepFun Step Plan (China)",
        default_base_url: Some("https://api.stepfun.com/step_plan/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "stepfun-ai-step-plan",
        name: "StepFun Step Plan (Global)",
        default_base_url: Some("https://api.stepfun.ai/step_plan/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tencent-coding-plan",
        name: "Tencent Cloud Coding Plan (China)",
        default_base_url: Some("https://api.lkeap.cloud.tencent.com/coding/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tencent-token-plan",
        name: "Tencent Cloud Token Plan (China)",
        default_base_url: Some("https://api.lkeap.cloud.tencent.com/plan/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "tencent-token-plan-global",
        name: "Tencent Cloud Token Plan (Global)",
        default_base_url: Some("https://tokenhub-intl.tencentcloudmaas.com/plan/v3"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "nvidia",
        name: "NVIDIA NIM",
        default_base_url: Some("https://integrate.api.nvidia.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "cerebras",
        name: "Cerebras",
        default_base_url: Some("https://api.cerebras.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "official",
    },
    ModelProviderView {
        id: "siliconflow",
        name: "SiliconFlow",
        default_base_url: Some("https://api.siliconflow.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "together",
        name: "Together AI",
        default_base_url: Some("https://api.together.xyz/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "fireworks",
        name: "Fireworks AI",
        default_base_url: Some("https://api.fireworks.ai/inference/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "poe",
        name: "Poe",
        default_base_url: Some("https://api.poe.com/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "huggingface",
        name: "Hugging Face",
        default_base_url: Some("https://router.huggingface.co/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "novita-ai",
        name: "Novita AI",
        default_base_url: Some("https://api.novita.ai/openai"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "qiniu-ai",
        name: "Qiniu AI",
        default_base_url: Some("https://api.qnaigc.com/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "digitalocean",
        name: "DigitalOcean Gradient AI",
        default_base_url: Some("https://inference.do-ai.run/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "modelscope",
        name: "ModelScope",
        default_base_url: Some("https://api-inference.modelscope.cn/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "scaleway",
        name: "Scaleway Generative APIs",
        default_base_url: Some("https://api.scaleway.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "nebius",
        name: "Nebius Token Factory",
        default_base_url: Some("https://api.tokenfactory.nebius.com/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "requesty",
        name: "Requesty",
        default_base_url: Some("https://router.requesty.ai/v1"),
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "gateway",
    },
    ModelProviderView {
        id: "baseten",
        name: "Baseten",
        default_base_url: Some("https://inference.baseten.co/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "wandb",
        name: "Weights & Biases",
        default_base_url: Some("https://api.inference.wandb.ai/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "gateway",
    },
    ModelProviderView {
        id: "ollama",
        name: "Ollama",
        default_base_url: Some("http://localhost:11434/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "local",
    },
    ModelProviderView {
        id: "lm-studio",
        name: "LM Studio",
        default_base_url: Some("http://localhost:1234/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "local",
    },
    ModelProviderView {
        id: "vllm",
        name: "vLLM",
        default_base_url: Some("http://localhost:8000/v1"),
        default_protocol: ModelProtocol::OpenaiCompletions,
        category: "local",
    },
    ModelProviderView {
        id: "custom",
        name: "Custom Provider",
        default_base_url: None,
        default_protocol: ModelProtocol::OpenaiResponses,
        category: "custom",
    },
];

pub fn list_providers() -> &'static [ModelProviderView] {
    MODEL_PROVIDERS
}

fn provider_display_name(provider: &str) -> String {
    MODEL_PROVIDERS
        .iter()
        .find(|candidate| candidate.id == provider)
        .map(|candidate| candidate.name.to_string())
        .unwrap_or_else(|| {
            provider
                .split(['-', '_'])
                .filter(|part| !part.is_empty())
                .map(|part| {
                    let mut chars = part.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
}

fn unique_provider_name(settings: &crate::settings::Settings, provider: &str) -> String {
    let base_name = provider_display_name(provider);
    let used = settings
        .model_providers
        .iter()
        .flatten()
        .map(|(_, provider)| provider.name.to_lowercase())
        .collect::<BTreeSet<_>>();
    if !used.contains(&base_name.to_lowercase()) {
        return base_name;
    }
    for suffix in 2.. {
        let candidate = format!("{base_name} {suffix}");
        if !used.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}

pub fn list_provider_instances() -> Vec<ModelProviderInstanceView> {
    let settings = load_settings();
    let profiles = settings.model_profiles.unwrap_or_default();
    let mut model_counts = BTreeMap::<&str, usize>::new();
    for profile in profiles.values() {
        if let Some(provider_id) = profile.provider_id.as_deref() {
            *model_counts.entry(provider_id).or_default() += 1;
        }
    }
    let mut providers = settings
        .model_providers
        .unwrap_or_default()
        .into_values()
        .map(|provider| {
            ModelProviderInstanceView {
                credential_saved: provider_credential_present(&provider.id),
                model_count: model_counts.get(provider.id.as_str()).copied().unwrap_or_default(),
                model_discovery_supported: discovery::provider_model_discovery_supported(&provider),
                portal: provider_instance_portal(&provider),
                provider,
            }
        })
        .collect::<Vec<_>>();
    providers.sort_by(|left, right| {
        left.provider
            .name
            .to_lowercase()
            .cmp(&right.provider.name.to_lowercase())
            .then_with(|| left.provider.id.cmp(&right.provider.id))
    });
    providers
}

pub fn catalog_key(profile: &ModelProfile) -> String {
    format!("{}/{}", profile.provider, profile.model)
}

fn normalize_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push((byte as char).to_ascii_lowercase());
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}

pub fn infer_provider(base_url: &str) -> String {
    let normalized = base_url.trim().trim_end_matches('/');
    // Match the complete reviewed endpoint or request URL, not an arbitrary
    // host/path prefix. Shared PAYG/plan URLs deliberately prefer the neutral
    // PAYG template; explicit user-selected plan IDs are never inferred here.
    let matches_endpoint = |endpoint: &str, protocol: &ModelProtocol| {
        endpoint.eq_ignore_ascii_case(normalized)
            || format!("{endpoint}{}", protocol.default_endpoint_path())
                .eq_ignore_ascii_case(normalized)
    };
    if let Some(provider) = MODEL_PROVIDERS.iter().find(|provider| {
        provider
            .default_base_url
            .is_some_and(|endpoint| matches_endpoint(endpoint, &provider.default_protocol))
            || provider_additional_endpoints(provider.id)
                .iter()
                .any(|endpoint| matches_endpoint(endpoint.base_url, &endpoint.protocol))
    }) {
        return provider.id.to_string();
    }

    let parsed = url::Url::parse(normalized).ok();
    let host = parsed.as_ref()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .unwrap_or_default();
    let path = parsed.as_ref().map(|url| url.path()).unwrap_or("");
    let in_path = |prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
    if (host.ends_with(".openai.azure.com") || host.ends_with(".services.ai.azure.com"))
        && in_path("/openai/v1")
    {
        return "azure-openai".into();
    }
    if host.starts_with("bedrock-mantle.") && host.ends_with(".api.aws")
        && in_path("/v1")
    {
        return "amazon-bedrock-mantle".into();
    }
    if host == "api.cloudflare.com" {
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        if parts.len() >= 6 && parts[..3] == ["client", "v4", "accounts"]
            && !parts[3].is_empty() && parts[4..6] == ["ai", "v1"]
        {
            return "cloudflare-workers-ai".into();
        }
    }
    match host.as_str() {
        "openrouter.ai" | "api.openrouter.ai" => "openrouter",
        "api.anthropic.com" => "anthropic",
        "api.openai.com" => "openai",
        "generativelanguage.googleapis.com" => "google",
        "api.x.ai" => "xai",
        "api.mistral.ai" => "mistral",
        "api.sarashina.ai" => "sarashina",
        "inference.liquid.ai" => "liquid-ai",
        "api.atria-asi.ai" => "atria",
        "api.cohere.ai" => "cohere",
        "api.deepseek.com" => "deepseek",
        "api.groq.com" => "groq",
        "api.poe.com" => "poe",
        "api.novita.ai" => "novita-ai",
        "api.qnaigc.com" => "qiniu-ai",
        "integrate.api.nvidia.com" => "nvidia",
        "inference.do-ai.run" => "digitalocean",
        "models.github.ai" => "github-models",
        "router.huggingface.co" => "huggingface",
        "api.moonshot.ai" => "moonshotai",
        "api.moonshot.cn" => "moonshotai-cn",
        "api.minimax.io" => "minimax",
        "api.minimax.cn" => "minimax-cn",
        "api.stepfun.com" => "stepfun",
        "api.stepfun.ai" => "stepfun-global",
        "open.bigmodel.cn" => "zhipuai",
        "api.deepinfra.com" => "deepinfra",
        "ai-gateway.vercel.sh" => "vercel-ai-gateway",
        "api.routeway.ai" => "routeway",
        "llm.onerouter.pro" => "infron",
        "api.apinex.bond" => "apinex",
        "tokenharbor.ai" => "tokenharbor",
        "api.flock.io" => "flock",
        "codecraftapi.com" => "codecraft",
        "api.beatapi.io" => "beatapi",
        "api.experientiallabs.ai" => "experiential-labs",
        "inference.runanywhere.ai" => "runanywhere",
        "cleanapis.com" => "clean-apis",
        "lighthouse.dphn.ai" => "dolphin",
        "api.iteracompute.com" => "iteracompute",
        "llmtr.com" => "llmtr",
        "api.munito.ai" | "api.eu.munito.ai" | "api.us.munito.ai" => "munito",
        "inference-api.nousresearch.com" => "nous-portal",
        "api.ppq.ai" => "ppq",
        "api.river.ai" => "river",
        "api.trustedrouter.com" => "trustedrouter",
        "api.unorouter.com" => "unorouter",
        "api.ant-ling.com" => "ant-ling",
        "maas-api.antdigital.com" => "antdigital-dtmaas",
        "maas-api.unisound.com" => "unisound",
        "api.sambanova.ai" => "sambanova",
        "api.perplexity.ai" => "perplexity",
        "ark.cn-beijing.volces.com" => "volcengine",
        "qianfan.baidubce.com" => "baidu-qianfan",
        "api.z.ai" => "zai",
        "api-inference.modelscope.cn" => "modelscope",
        "api.scaleway.ai" => "scaleway",
        "api.tokenfactory.nebius.com" => "nebius",
        "router.requesty.ai" => "requesty",
        "inference.baseten.co" => "baseten",
        "api.inference.wandb.ai" => "wandb",
        "api.siliconflow.cn" => "siliconflow-cn",
        "api.siliconflow.com" => "siliconflow",
        "api.together.xyz" => "together",
        "api.fireworks.ai" => "fireworks",
        "api.cerebras.ai" => "cerebras",
        "dashscope.aliyuncs.com" => "alibaba",
        "dashscope-intl.aliyuncs.com" => "alibaba-international",
        "api.xiaomimimo.com"
        | "token-plan-cn.xiaomimimo.com"
        | "token-plan-sgp.xiaomimimo.com"
        | "token-plan-ams.xiaomimimo.com" => "xiaomi",
        "localhost" | "127.0.0.1" | "::1" => match url::Url::parse(base_url)
            .ok()
            .and_then(|url| url.port_or_known_default())
        {
            Some(11434) => "ollama",
            Some(1234) => "lm-studio",
            Some(8000) => "vllm",
            _ => "local",
        },
        _ => "custom",
    }
    .to_string()
}

pub fn infer_model_vendor(provider: &str, model: &str) -> Option<String> {
    let candidate = model
        .split_once('/')
        .map(|(vendor, _)| vendor)
        .unwrap_or(provider);
    let candidate = normalize_slug(candidate);
    (!candidate.is_empty() && candidate != "custom" && candidate != "local").then_some(candidate)
}

fn generated_profile_id(provider: &str, model: &str, existing: &BTreeSet<String>) -> String {
    let prefix = normalize_slug(&format!("{provider}-{model}"));
    let prefix = if prefix.is_empty() { "model" } else { &prefix };
    loop {
        let suffix = &Uuid::new_v4().simple().to_string()[..8];
        let max_prefix = 64usize.saturating_sub(suffix.len() + 1);
        let mut compact = prefix.chars().take(max_prefix).collect::<String>();
        while compact.ends_with('-') {
            compact.pop();
        }
        let id = format!("{compact}-{suffix}");
        if !existing.contains(&id) {
            return id;
        }
    }
}

fn migrated_profile_id(
    old_id: &str,
    provider: &str,
    model: &str,
    existing: &BTreeSet<String>,
) -> String {
    let prefix = normalize_slug(&format!("{provider}-{model}"));
    let prefix = if prefix.is_empty() { "model" } else { &prefix };
    for discriminator in 0u32.. {
        let material =
            format!("mux-model-schema-v2\0{old_id}\0{provider}\0{model}\0{discriminator}");
        let digest = hex::encode(Sha256::digest(material.as_bytes()));
        let suffix = &digest[..8];
        let max_prefix = 64usize.saturating_sub(suffix.len() + 1);
        let mut compact = prefix.chars().take(max_prefix).collect::<String>();
        while compact.ends_with('-') {
            compact.pop();
        }
        let id = format!("{compact}-{suffix}");
        if !existing.contains(&id) {
            return id;
        }
    }
    unreachable!()
}

fn generated_provider_id(provider: &str, existing: &BTreeSet<String>) -> String {
    let prefix = normalize_slug(provider);
    let prefix = if prefix.is_empty() {
        "provider"
    } else {
        &prefix
    };
    loop {
        let suffix = &Uuid::new_v4().simple().to_string()[..8];
        let id = format!("{prefix}-{suffix}");
        if !existing.contains(&id) {
            return id;
        }
    }
}

fn normalized_endpoint_origin(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|url| {
            let host = url.host_str()?;
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            Some(format!("{}://{host}{port}", url.scheme()))
        })
        .unwrap_or_else(|| base_url.trim_end_matches('/').to_ascii_lowercase())
}

pub fn normalize_provider_base_url(base_url: &str) -> Result<String, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    let lower = base_url.to_ascii_lowercase();
    if base_url.contains(['<', '>', '{', '}'])
        || ["%3c", "%3e", "%7b", "%7d"].iter().any(|marker| lower.contains(marker))
    {
        return Err("Provider Base URL contains a placeholder; enter your account or region endpoint".into());
    }
    if base_url.is_empty() || base_url.chars().any(char::is_whitespace) {
        return Err("Provider Base URL is required and cannot contain whitespace".into());
    }
    let parsed = url::Url::parse(base_url)
        .map_err(|error| format!("Provider Base URL must be a valid http(s) URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "Provider Base URL must be an http(s) URL without credentials, query, or fragment"
                .into(),
        );
    }
    Ok(base_url.to_string())
}

pub fn normalize_model_catalog_url(model_catalog_url: &str) -> Result<String, String> {
    let model_catalog_url = model_catalog_url.trim();
    if model_catalog_url.is_empty() || model_catalog_url.chars().any(char::is_whitespace) {
        return Err("Models list URL cannot contain whitespace".into());
    }
    let parsed = url::Url::parse(model_catalog_url)
        .map_err(|error| format!("Models list URL must be a valid http(s) URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "Models list URL must be an http(s) URL without credentials or a fragment".into(),
        );
    }
    Ok(model_catalog_url.to_string())
}

pub fn normalize_provider_portal_url(portal_url: &str) -> Result<String, String> {
    let portal_url = portal_url.trim();
    if portal_url.is_empty() || portal_url.chars().any(char::is_whitespace) {
        return Err("Provider portal URL cannot contain whitespace".into());
    }
    let parsed = url::Url::parse(portal_url)
        .map_err(|error| format!("Provider portal URL must be a valid HTTPS URL: {error}"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Provider portal URL must be an HTTPS URL without credentials or a fragment".into());
    }
    if parsed.query_pairs().any(|(name, _)| {
        matches!(name.to_ascii_lowercase().as_str(),
            "key" | "api_key" | "apikey" | "token" | "access_token" | "secret"
            | "password" | "authorization" | "credential" | "sig" | "signature")
    }) {
        return Err("Provider portal URL must not contain credential query parameters".into());
    }
    Ok(portal_url.to_string())
}

pub fn normalize_endpoint_path(endpoint_path: &str) -> Result<String, String> {
    let endpoint_path = endpoint_path.trim();
    if endpoint_path.is_empty() {
        return Err("Endpoint Path is required".into());
    }
    if endpoint_path.chars().any(char::is_whitespace)
        || endpoint_path.contains('#')
        || endpoint_path.contains('?')
        || endpoint_path.contains("://")
        || endpoint_path.starts_with("//")
        || endpoint_path.contains('\\')
    {
        return Err(
            "Endpoint Path must be a relative path without a host, query, fragment, whitespace, or backslash"
                .into(),
        );
    }
    let normalized = format!("/{}", endpoint_path.trim_start_matches('/'));
    for segment in normalized.split('/') {
        let segment = segment.to_ascii_lowercase();
        let decoded_dots = segment.replace("%2e", ".");
        if matches!(decoded_dots.as_str(), "." | "..")
            || segment.contains("%2f")
            || segment.contains("%5c")
        {
            return Err("Endpoint Path cannot contain path traversal segments".into());
        }
    }
    let first = normalized
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or_default();
    if !endpoint_path.starts_with('/') && (first.contains('.') || first.contains(':')) {
        return Err("Endpoint Path cannot contain a domain or URI scheme".into());
    }
    Ok(normalized)
}

pub fn full_request_url(base_url: &str, endpoint_path: &str) -> Result<String, String> {
    Ok(format!(
        "{}{}",
        normalize_provider_base_url(base_url)?,
        normalize_endpoint_path(endpoint_path)?
    ))
}

fn protocol_client_base_url(
    base_url: &str,
    protocol: &ModelProtocol,
    endpoint_path: &str,
) -> Result<String, String> {
    let base_url = normalize_provider_base_url(base_url)?;
    let endpoint_path = normalize_endpoint_path(endpoint_path)?;
    let suffix = protocol.default_endpoint_path();
    let Some(prefix) = endpoint_path.strip_suffix(suffix) else {
        return Err(format!(
            "Endpoint Path '{endpoint_path}' must end with the protocol path '{suffix}'"
        ));
    };
    Ok(format!("{base_url}{prefix}")
        .trim_end_matches('/')
        .to_string())
}

pub(crate) fn profile_endpoint_compatibility_issue(
    agent_id: &str,
    profile: &ModelProfile,
) -> Option<(String, String)> {
    if profile.endpoint_path.is_empty() {
        return None;
    }
    protocol_client_base_url(&profile.base_url, &profile.protocol, &profile.endpoint_path)
        .err()
        .map(|error| {
            (
                "model_endpoint_path_unsupported".into(),
                format!(
                    "{agent_id} cannot express this custom Endpoint Path without changing the request target: {error}"
                ),
            )
        })
}

fn materialize_profile_for_agent(
    agent_id: &str,
    profile: &ModelProfile,
) -> Result<ModelProfile, String> {
    let mut materialized = profile.clone();
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    let source = profile.provider_id.as_deref().and_then(|provider_id| {
        settings
            .model_providers
            .as_ref()
            .and_then(|providers| providers.get(provider_id))
            .and_then(|provider| provider.api_key_source.clone())
    });
    match source {
        Some(ApiKeySource::File { path }) => {
            materialized
                .native_ids
                .insert(CREDENTIAL_FILE_NATIVE_ID.into(), path);
        }
        Some(ApiKeySource::Helper { command, args, .. }) => {
            materialized
                .native_ids
                .insert(CREDENTIAL_COMMAND_NATIVE_ID.into(), command);
            materialized.native_ids.insert(
                CREDENTIAL_ARGS_NATIVE_ID.into(),
                serde_json::to_string(&args).map_err(|error| error.to_string())?,
            );
        }
        _ => {}
    }
    if agent_id == "pi" && !materialized.native_ids.contains_key("pi") {
        materialized
            .native_ids
            .insert("pi".into(), generated_pi_provider_id(&settings, profile));
    }
    if agent_id == "opencode" {
        if let Some(provider) = profile.provider_id.as_ref()
            .and_then(|id| settings.model_providers.as_ref()?.get(id)) {
            materialized.native_ids.insert("__mux_provider_name".into(), provider.name.clone());
        }
        if materialized.native_ids.get("opencode")
            .is_none_or(|id| id == &arca_profile_id(&profile.id)) {
            let id = generated_open_code_provider_id(&settings, profile);
            materialized.native_ids.insert("opencode".into(), id.clone());
            materialized.native_ids.insert("__mux_generated_opencode_provider".into(), id);
        }
    }
    if agent_id != claude_desktop::AGENT_ID && !profile.endpoint_path.is_empty() {
        materialized.base_url =
            protocol_client_base_url(&profile.base_url, &profile.protocol, &profile.endpoint_path)
                .map_err(|error| {
                    format!(
                        "model_endpoint_path_unsupported: {agent_id} cannot express this custom Endpoint Path without changing the request target: {error}"
                    )
                })?;
        materialized.endpoint_path.clear();
    }
    Ok(materialized)
}

fn credential_fingerprint(profile_id: &str) -> Result<Option<String>, String> {
    Ok(read_credential_checked(profile_id)?.map(|credential| {
        Sha256::digest(credential)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }))
}

/// Upgrade v2 Profile-local connection metadata into shared Provider
/// instances. This migration only adds central metadata and references; Agent
/// files and existing per-Profile Keychain items remain byte-for-byte stable.
pub fn migrate_model_providers_v3_if_needed() -> Result<bool, String> {
    let _settings_guard = mutation_lock()?;
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    let version = settings.version.unwrap_or_default();
    if version >= 3 {
        let repaired = repair_missing_provider_references(&settings)?;
        let consolidated = consolidate_provider_credentials()?;
        let upgraded = version < crate::settings::SETTINGS_VERSION;
        if upgraded {
            crate::settings::mutate_settings(|settings| {
                settings.version = Some(crate::settings::SETTINGS_VERSION);
            })
            .map_err(|error| error.to_string())?;
        }
        return Ok(repaired || consolidated || upgraded);
    }
    if version < 2 {
        return Err("model_schema_v2_required: migrate Model Profiles before Providers".into());
    }

    #[derive(Clone, PartialEq, Eq)]
    struct GroupIdentity {
        provider: String,
        origin: String,
        env_key: Option<String>,
        credential: Option<String>,
    }

    let mut profiles = settings.model_profiles.clone().unwrap_or_default();
    let mut providers = BTreeMap::<String, ModelProviderConfig>::new();
    let mut legacy_endpoints = BTreeMap::<String, BTreeMap<ModelProtocol, String>>::new();
    let mut groups = Vec::<(GroupIdentity, String)>::new();
    let mut used_ids = BTreeSet::new();
    let mut used_names = BTreeSet::new();

    for profile in profiles.values_mut() {
        let identity = GroupIdentity {
            provider: profile.provider.clone(),
            origin: normalized_endpoint_origin(&profile.base_url),
            env_key: profile.env_key.clone(),
            credential: credential_fingerprint(&profile.id)?,
        };
        let existing_provider_id = groups.iter().find_map(|(candidate, provider_id)| {
            if candidate != &identity {
                return None;
            }
            legacy_endpoints
                .get(provider_id)
                .and_then(|endpoints| endpoints.get(&profile.protocol))
                .is_none_or(|endpoint| {
                    endpoint.trim_end_matches('/') == profile.base_url.trim_end_matches('/')
                })
                .then_some(provider_id.clone())
        });
        let provider_id = match existing_provider_id {
            Some(provider_id) => provider_id,
            None => {
                let provider_id = generated_provider_id(&profile.provider, &used_ids);
                used_ids.insert(provider_id.clone());
                let base_name = provider_display_name(&profile.provider);
                let mut name = base_name.clone();
                for suffix in 2.. {
                    if !used_names.contains(&name.to_lowercase()) {
                        break;
                    }
                    name = format!("{base_name} {suffix}");
                }
                used_names.insert(name.to_lowercase());
                providers.insert(
                    provider_id.clone(),
                    ModelProviderConfig {
                        id: provider_id.clone(),
                        name,
                        provider: profile.provider.clone(),
                        base_url: String::new(),
                        model_catalog_url: None,
                        portal_url: None,
                        protocols: BTreeMap::new(),
                        auth_requirement: if profile.env_key.is_some() {
                            AuthRequirement::Required
                        } else {
                            AuthRequirement::Optional
                        },
                        api_key_source: profile
                            .env_key
                            .clone()
                            .map(|name| ApiKeySource::Env { name }),
                        env_key: profile.env_key.clone(),
                    },
                );
                groups.push((identity, provider_id.clone()));
                provider_id
            }
        };
        legacy_endpoints
            .entry(provider_id.clone())
            .or_default()
            .insert(profile.protocol.clone(), profile.base_url.clone());
        profile.provider_id = Some(provider_id);
    }
    for (provider_id, endpoints) in legacy_endpoints {
        let provider = providers
            .get_mut(&provider_id)
            .expect("grouped Provider is present");
        let (base_url, protocols) = migrate_legacy_provider_endpoints(&provider.name, &endpoints)?;
        provider.base_url = base_url;
        provider.protocols = protocols;
    }

    crate::settings::mutate_settings_checked(|current| {
        if current.version != settings.version || current.model_profiles != settings.model_profiles
        {
            return Err(std::io::Error::other(
                "asset_operation_stale: Model settings changed during Provider migration",
            ));
        }
        current.model_profiles = (!profiles.is_empty()).then_some(profiles.clone());
        current.model_providers = (!providers.is_empty()).then_some(providers.clone());
        current.version = Some(crate::settings::SETTINGS_VERSION);
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    consolidate_provider_credentials()?;
    Ok(true)
}

fn repair_missing_provider_references(
    settings: &crate::settings::Settings,
) -> Result<bool, String> {
    let providers = settings.model_providers.clone().unwrap_or_default();
    let profiles = settings.model_profiles.clone().unwrap_or_default();
    let repairs = profiles
        .iter()
        .filter(|(_, profile)| {
            profile
                .provider_id
                .as_deref()
                .is_none_or(|provider_id| !providers.contains_key(provider_id))
        })
        .filter_map(|(profile_id, profile)| {
            let candidates = providers
                .values()
                .filter(|provider| {
                    provider.provider == profile.provider
                        && provider.env_key == profile.env_key
                        && provider
                            .protocols
                            .get(&profile.protocol)
                            .is_some_and(|protocol| {
                                protocol_client_base_url(
                                    &provider.base_url,
                                    &profile.protocol,
                                    &protocol.endpoint_path,
                                )
                                .is_ok_and(|endpoint| {
                                    endpoint.trim_end_matches('/')
                                        == profile.base_url.trim_end_matches('/')
                                })
                            })
                })
                .map(|provider| provider.id.clone())
                .collect::<Vec<_>>();
            (candidates.len() == 1).then(|| (profile_id.clone(), candidates[0].clone()))
        })
        .collect::<BTreeMap<_, _>>();
    if repairs.is_empty() {
        return Ok(false);
    }

    crate::settings::mutate_settings_checked(|current| {
        if current.model_profiles != settings.model_profiles
            || current.model_providers != settings.model_providers
        {
            return Err(std::io::Error::other(
                "asset_operation_stale: Model settings changed during Provider repair",
            ));
        }
        let current_profiles = current.model_profiles.get_or_insert_default();
        for (profile_id, provider_id) in &repairs {
            let profile = current_profiles.get_mut(profile_id).ok_or_else(|| {
                std::io::Error::other(
                    "asset_operation_stale: Model Profile disappeared during Provider repair",
                )
            })?;
            profile.provider_id = Some(provider_id.clone());
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    Ok(true)
}

fn consolidate_provider_credentials() -> Result<bool, String> {
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    let profiles = settings.model_profiles.unwrap_or_default();
    let mut changed = false;
    for provider in settings.model_providers.unwrap_or_default().into_values() {
        let profile_ids = profiles
            .iter()
            .filter(|(_, profile)| profile.provider_id.as_deref() == Some(provider.id.as_str()))
            .map(|(profile_id, _)| profile_id)
            .collect::<Vec<_>>();
        let provider_service = provider_keychain_service(&provider.id);
        if !credential_service_exists_checked(&provider_service)? {
            let legacy_credentials = profile_ids
                .iter()
                .map(|profile_id| read_credential_service_checked(&legacy_keychain_service(profile_id)))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter().flatten().collect::<BTreeSet<_>>();
            if legacy_credentials.len() > 1 {
                return Err(format!(
                    "model_provider_credential_drift: Provider '{}' has conflicting legacy API Keys",
                    provider.name
                ));
            }
            if let Some(credential) = legacy_credentials.iter().next() {
                set_credential_service(&provider_service, credential)?;
                changed = true;
            }
        }
        // Keep per-Profile credentials. Existing Agent helpers may still
        // reference them until the user explicitly restores Arca desired state.
    }
    Ok(changed)
}

fn default_profile_name(model: &str) -> String {
    let leaf = model.rsplit('/').next().unwrap_or(model);
    let name = leaf
        .split(['-', '_', ':', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if name.is_empty() {
        "Model".into()
    } else {
        name
    }
}

fn unique_profile_name(
    settings: &crate::settings::Settings,
    provider: &str,
    requested: &str,
    excluding_id: Option<&str>,
) -> String {
    let used = settings
        .model_profiles
        .iter()
        .flatten()
        .filter(|(id, profile)| Some(id.as_str()) != excluding_id && profile.provider == provider)
        .map(|(_, profile)| profile.name.to_lowercase())
        .collect::<BTreeSet<_>>();
    if !used.contains(&requested.to_lowercase()) {
        return requested.to_string();
    }
    for index in 2.. {
        let candidate = format!("{requested} {index}");
        if !used.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}

fn validate_provider_model_identity(
    settings: &crate::settings::Settings,
    profile: &ModelProfile,
    excluding_id: Option<&str>,
) -> Result<(), String> {
    let Some(provider_id) = profile.provider_id.as_deref() else {
        return Ok(());
    };
    let duplicate = settings
        .model_profiles
        .iter()
        .flatten()
        .any(|(id, candidate)| {
            Some(id.as_str()) != excluding_id
                && candidate.provider_id.as_deref() == Some(provider_id)
                && candidate.model == profile.model
        });
    if duplicate {
        return Err(format!(
            "asset_identity_conflict: Model '{}' already exists in this Provider",
            profile.model
        ));
    }
    Ok(())
}

pub(crate) fn prepare_profile_draft(
    settings: &crate::settings::Settings,
    existing_id: Option<&str>,
    mut profile: ModelProfile,
) -> Result<ModelProfile, String> {
    if let Some(provider_id) = profile.provider_id.as_deref() {
        let provider = settings
            .model_providers
            .as_ref()
            .and_then(|providers| providers.get(provider_id))
            .ok_or_else(|| "asset_operation_stale: Model Provider no longer exists".to_string())?;
        profile.provider = provider.provider.clone();
        profile.env_key = provider.env_key.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = provider
            .protocols
            .get(&profile.protocol)
            .ok_or_else(|| {
                format!(
                    "model_provider_endpoint_missing: {} does not support {:?}",
                    provider.name, profile.protocol
                )
            })?
            .endpoint_path
            .clone();
    }
    profile.provider = if profile.provider.trim().is_empty() {
        infer_provider(&profile.base_url)
    } else {
        normalize_slug(&profile.provider)
    };
    if profile.provider.is_empty() {
        profile.provider = "custom".into();
    }
    profile.model_vendor = profile
        .model_vendor
        .as_deref()
        .map(normalize_slug)
        .filter(|vendor| !vendor.is_empty())
        .or_else(|| infer_model_vendor(&profile.provider, &profile.model));
    match existing_id {
        Some(existing_id) => {
            if profile.id.is_empty() {
                profile.id = existing_id.to_string();
            }
            if profile.id != existing_id {
                return Err("asset_identity_change: Model Profile id cannot change".into());
            }
            let existing = settings
                .model_profiles
                .as_ref()
                .and_then(|profiles| profiles.get(existing_id))
                .ok_or_else(|| {
                    "asset_operation_stale: Model Profile no longer exists".to_string()
                })?;
            // Agent-native identities are evidence created only by the explicit
            // adoption flow. Ordinary metadata edits, including switching the
            // Provider relationship, cannot rewrite that evidence.
            profile.native_ids = existing.native_ids.clone();
        }
        None => {
            if !profile.id.is_empty() {
                return Err("invalid_asset: Model Profile id is generated by Arca".into());
            }
            if !profile.native_ids.is_empty() {
                return Err(
                    "invalid_asset: Agent-native identities require explicit adoption".into(),
                );
            }
            let existing = settings
                .model_profiles
                .iter()
                .flatten()
                .map(|(id, _)| id.clone())
                .collect();
            profile.id = generated_profile_id(&profile.provider, &profile.model, &existing);
            if profile.provider_id.is_none() {
                let existing_provider_ids = settings
                    .model_providers
                    .iter()
                    .flatten()
                    .map(|(id, _)| id.clone())
                    .collect();
                profile.provider_id = Some(generated_provider_id(
                    &profile.provider,
                    &existing_provider_ids,
                ));
            }
        }
    }
    if profile.endpoint_path.is_empty() {
        profile.endpoint_path = profile.protocol.default_endpoint_path().into();
    }
    let requested_name = if profile.name.trim().is_empty() {
        default_profile_name(&profile.model)
    } else {
        profile.name.trim().to_string()
    };
    profile.name = unique_profile_name(settings, &profile.provider, &requested_name, existing_id);
    validate_profile(&profile)?;
    validate_provider_model_identity(settings, &profile, existing_id)?;
    Ok(profile)
}

type MigratedProfiles = (BTreeMap<String, String>, BTreeMap<String, ModelProfile>);

pub(crate) fn migrated_profiles_v2(
    settings: &crate::settings::Settings,
) -> Result<MigratedProfiles, String> {
    let old_profiles = settings.model_profiles.clone().unwrap_or_default();
    let mut used_ids = old_profiles.keys().cloned().collect::<BTreeSet<_>>();
    let mut id_map = BTreeMap::new();
    let mut profiles = BTreeMap::new();
    let mut naming = crate::settings::Settings::default();
    for (old_id, mut profile) in old_profiles {
        profile.provider = if profile.provider.trim().is_empty() {
            infer_provider(&profile.base_url)
        } else {
            normalize_slug(&profile.provider)
        };
        if profile.provider.is_empty() {
            profile.provider = "custom".into();
        }
        profile.model_vendor = profile
            .model_vendor
            .as_deref()
            .map(normalize_slug)
            .filter(|vendor| !vendor.is_empty())
            .or_else(|| infer_model_vendor(&profile.provider, &profile.model));
        let new_id = migrated_profile_id(&old_id, &profile.provider, &profile.model, &used_ids);
        used_ids.insert(new_id.clone());
        profile.id = new_id.clone();
        let requested_name = if profile.name.trim().is_empty() {
            default_profile_name(&profile.model)
        } else {
            profile.name.trim().to_string()
        };
        profile.name = unique_profile_name(&naming, &profile.provider, &requested_name, None);
        validate_profile(&profile)?;
        id_map.insert(old_id, new_id.clone());
        profiles.insert(new_id.clone(), profile);
        naming.model_profiles = Some(profiles.clone());
    }
    Ok((id_map, profiles))
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelAgentView {
    pub id: String,
    pub name: String,
    /// `managed` or `guided`.
    pub mode: String,
    pub storage_authority: ModelStorageAuthority,
    pub installed: bool,
    pub config_path: String,
    pub config_paths: Vec<String>,
    pub docs: String,
    pub assigned_profile: Option<String>,
    pub assigned_profiles: Vec<String>,
    pub active_profile: Option<String>,
    pub supports_multiple: bool,
    /// False for clients that select a model separately in each conversation.
    pub supports_global_selection: bool,
    /// `keychain-command`, `keychain-export`, `environment-reference`, or `guided`.
    pub credential_mode: String,
    pub credential_capabilities: credential::AgentCredentialCapabilities,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credential_policies: BTreeMap<String, crate::domain::assets::ModelCredentialPolicy>,
    #[serde(default)]
    pub default_delivery: crate::domain::assets::ApiKeyDelivery,
    #[serde(default)]
    pub available_deliveries: Vec<crate::domain::assets::ApiKeyDelivery>,
    pub supported_protocols: Vec<ModelProtocol>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelApplyResult {
    pub agent: String,
    pub profile: String,
    pub files: Vec<String>,
    pub restart_required: bool,
    pub message: String,
}

#[derive(Debug)]
pub(crate) enum ModelTargetError {
    /// No target write occurred, or every target write was proven restored.
    ConvergenceFailed(String),
    /// A target may remain partially written or lacks proven cleanup.
    RecoveryRequired(String),
}

impl ModelTargetError {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::ConvergenceFailed(message) => message,
            Self::RecoveryRequired(message) => format!("target_recovery_required: {message}"),
        }
    }
}

impl From<String> for ModelTargetError {
    fn from(message: String) -> Self {
        Self::ConvergenceFailed(message)
    }
}

impl From<&str> for ModelTargetError {
    fn from(message: &str) -> Self {
        Self::ConvergenceFailed(message.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelObservedState {
    Synced,
    Missing,
    Drifted,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalModelObservedState {
    Absent,
    Present,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ObservedActiveModel {
    Managed(String),
    External,
    None,
    Conflicted,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn config_path(relative: &str) -> PathBuf {
    home().join(relative)
}

pub fn default_config_paths(agent_id: &str) -> Option<Vec<String>> {
    if agent_id == claude_desktop::AGENT_ID {
        return Some(claude_desktop::default_paths());
    }
    let paths: &[&str] = match agent_id {
        "claude-code" => &["~/.claude/settings.json"],
        "codex" => &["~/.codex/config.toml"],
        "grok-build" => &["~/.grok/config.toml"],
        "pi" => &["~/.pi/agent/models.json", "~/.pi/agent/settings.json"],
        "opencode" | "opencode-desktop" => &["~/.config/opencode/opencode.json"],
        "kilo-code" => &["~/.config/kilo/kilo.jsonc"],
        "kilo-desktop" => &["~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/config/kilo/kilo.jsonc"],
        "qwen-code" => &["~/.qwen/settings.json"],
        "qoder-desktop" | "qoder-cli" => &["~/.qoder/settings.json"],
        "zcode" => &["~/.zcode/v2/config.json"],
        "crush" => &["~/.config/crush/crush.json"],
        "mistral-vibe" => &["~/.vibe/config.toml"],
        "hermes" => &["~/.hermes/config.yaml"],
        "factory-droid" => &["~/.factory/settings.json"],
        "goose" => &["~/Library/Application Support/Block/goose/config/config.yaml"],
        "minimax-code" => &["~/.mavis/config.yaml"],
        "kimi-code" | "kimi-code-desktop" => &["~/.kimi-code/config.toml"],
        _ => return None,
    };
    Some(paths.iter().map(|path| (*path).to_string()).collect())
}

pub fn normalize_config_paths(paths: &[String], expected: usize) -> Result<Vec<String>, String> {
    if paths.len() != expected {
        return Err(format!("Model 配置需要 {expected} 个路径"));
    }
    let normalized = paths
        .iter()
        .map(|path| {
            let path = collapse_home(path.trim());
            if !path.starts_with("~/") {
                return Err("Model 配置路径必须位于用户目录".into());
            }
            if path[2..]
                .split('/')
                .any(|component| component.is_empty() || matches!(component, "." | ".."))
            {
                return Err(format!("Model 配置路径不安全: {path}"));
            }
            Ok(path)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut distinct = BTreeSet::new();
    if normalized
        .iter()
        .map(|path| expand_tilde(path))
        .any(|path| !distinct.insert(path))
    {
        return Err("model_config_paths_not_distinct: Model 配置角色必须使用不同的展开路径".into());
    }
    Ok(normalized)
}

pub(crate) fn configured_path_strings_checked(
    settings: &crate::settings::Settings,
    agent_id: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(defaults) = default_config_paths(agent_id) else {
        return Ok(None);
    };
    let expected = defaults.len();
    let paths = settings
        .agent_config_paths
        .as_ref()
        .and_then(|overrides| overrides.get(agent_id))
        .and_then(|path_override| path_override.model_paths.clone())
        .unwrap_or(defaults);
    let normalized = normalize_config_paths(&paths, expected)?;
    if agent_id == claude_desktop::AGENT_ID {
        let expanded = normalized
            .iter()
            .map(|path| expand_tilde(path))
            .collect::<Vec<_>>();
        claude_desktop::validate_paths(&expanded)?;
    }
    Ok(Some(normalized))
}

fn configured_path_strings(
    settings: &crate::settings::Settings,
    agent_id: &str,
) -> Option<Vec<String>> {
    configured_path_strings_checked(settings, agent_id)
        .ok()
        .flatten()
}

fn configured_paths(agent_id: &str) -> Option<Vec<PathBuf>> {
    configured_path_strings(&load_settings(), agent_id).map(|paths| {
        paths
            .iter()
            .map(|path| expand_tilde(path))
            .collect::<Vec<_>>()
    })
}

fn path_view(settings: &crate::settings::Settings, agent_id: &str) -> (String, Vec<String>) {
    let paths = configured_path_strings(settings, agent_id).unwrap_or_default();
    (paths.join(" + "), paths)
}

fn legacy_keychain_service(profile_id: &str) -> String {
    format!("com.scoheart.mux.model-profile.{profile_id}")
}

fn provider_keychain_service(provider_id: &str) -> String {
    format!("com.scoheart.mux.model-provider.{provider_id}")
}

const PROVIDER_CREDENTIAL_SUBJECT_PREFIX: &str = "__model-provider__:";

pub(crate) fn provider_credential_subject(provider_id: &str) -> String {
    format!("{PROVIDER_CREDENTIAL_SUBJECT_PREFIX}{provider_id}")
}

fn keychain_service(subject_id: &str) -> String {
    if let Some(provider_id) = subject_id.strip_prefix(PROVIDER_CREDENTIAL_SUBJECT_PREFIX) {
        return provider_keychain_service(provider_id);
    }
    load_settings()
        .model_profiles
        .as_ref()
        .and_then(|profiles| profiles.get(subject_id))
        .and_then(|profile| profile.provider_id.as_deref())
        .map(provider_keychain_service)
        .unwrap_or_else(|| legacy_keychain_service(subject_id))
}

fn security_command(profile_id: &str) -> Vec<String> {
    vec![
        "/usr/bin/security".into(),
        "find-generic-password".into(),
        "-w".into(),
        "-s".into(),
        keychain_service(profile_id),
        "-a".into(),
        KEYCHAIN_ACCOUNT.into(),
    ]
}

fn profile_credential_command(profile: &ModelProfile) -> Vec<String> {
    let Some(command) = profile.native_ids.get(CREDENTIAL_COMMAND_NATIVE_ID) else {
        return security_command(&profile.id);
    };
    let args = profile
        .native_ids
        .get(CREDENTIAL_ARGS_NATIVE_ID)
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default();
    std::iter::once(command.clone()).chain(args).collect()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn profile_credential_shell_command(profile: &ModelProfile) -> String {
    profile_credential_command(profile)
        .iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "macos")]
fn read_credential_service_checked(service: &str) -> Result<Option<Vec<u8>>, String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            #[cfg(test)]
            if let Some(error) = TEST_CREDENTIAL_READ_ERROR.with(|slot| slot.borrow().clone()) {
                return Err(error);
            }
            return Ok(TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&test_credential_key(service))
                .cloned());
        }
    }
    let output = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            service,
            "-a",
            KEYCHAIN_ACCOUNT,
        ])
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "credential_read_failed: could not start the Keychain helper".to_string())?;
    if output.status.code() == Some(44) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err("credential_read_failed: Keychain did not authorize a reliable credential read".into());
    }
    let mut value = output.stdout;
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
    {
        value.pop();
    }
    Ok(Some(value))
}

#[cfg(not(target_os = "macos"))]
fn read_credential_service_checked(service: &str) -> Result<Option<Vec<u8>>, String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            #[cfg(test)]
            if let Some(error) = TEST_CREDENTIAL_READ_ERROR.with(|slot| slot.borrow().clone()) {
                return Err(error);
            }
            return Ok(TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(&test_credential_key(service))
                .cloned());
        }
    }
    let _ = service;
    Ok(None)
}

/// Authoritative metadata-only lookup. Bootstrap must not read every stored
/// secret merely to decide whether a Provider needs credential migration.
fn credential_service_exists_checked(service: &str) -> Result<bool, String> {
    // TestHome sets this marker specifically to isolate probes from the real
    // machine. Never consult the user's Keychain from a test process.
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            return Ok(TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .contains_key(&test_credential_key(service)));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                service,
                "-a",
                KEYCHAIN_ACCOUNT,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| "credential_read_failed: could not start the Keychain helper".to_string())?;
        if status.success() { Ok(true) }
        else if status.code() == Some(44) { Ok(false) }
        else { Err("credential_read_failed: Keychain credential presence could not be determined".into()) }
    }
    #[cfg(not(target_os = "macos"))]
    Ok(false)
}

/// Presentation-only presence; mutation and migration use the checked result.
fn credential_service_exists(service: &str) -> bool {
    credential_service_exists_checked(service).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn set_credential_service(service: &str, credential: &[u8]) -> Result<(), String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(test_credential_key(service), credential.to_vec());
            return Ok(());
        }
    }
    if credential.contains(&b'\n') || credential.contains(&b'\r') {
        return Err("API key cannot contain a newline".into());
    }
    keychain::set_password(service, KEYCHAIN_ACCOUNT, credential)
}

#[cfg(not(target_os = "macos"))]
fn set_credential_service(service: &str, credential: &[u8]) -> Result<(), String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(test_credential_key(service), credential.to_vec());
            return Ok(());
        }
    }
    let _ = (service, credential);
    Err("secure model credentials are currently supported on macOS only".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialDeleteOutcome {
    Deleted,
    NotFound,
}

#[cfg(target_os = "macos")]
fn delete_credential_service_authoritative(
    service: &str,
) -> Result<CredentialDeleteOutcome, String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            #[cfg(test)]
            if let Some(error) = test_credential_delete_error() {
                return Err(error);
            }
            let deleted = TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&test_credential_key(service))
                .is_some();
            return Ok(if deleted {
                CredentialDeleteOutcome::Deleted
            } else {
                CredentialDeleteOutcome::NotFound
            });
        }
    }
    let output = Command::new("/usr/bin/security")
        .args([
            "delete-generic-password",
            "-s",
            service,
            "-a",
            KEYCHAIN_ACCOUNT,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("failed to start macOS Keychain helper: {error}"))?;
    if output.status.success() {
        Ok(CredentialDeleteOutcome::Deleted)
    } else if output.status.code() == Some(44) {
        // `security` returns errSecItemNotFound (-25300) modulo the Unix
        // process-status byte. No other helper failure is treated as absence.
        Ok(CredentialDeleteOutcome::NotFound)
    } else {
        Err(format!(
            "failed to remove API key from macOS Keychain: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn delete_credential_service_authoritative(
    service: &str,
) -> Result<CredentialDeleteOutcome, String> {
    #[cfg(any(test, debug_assertions))]
    {
        if crate::brand::env_os("ARCA_TEST_PROBE_ROOT").is_some() {
            #[cfg(test)]
            if let Some(error) = test_credential_delete_error() {
                return Err(error);
            }
            let deleted = TEST_CREDENTIALS
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&test_credential_key(service))
                .is_some();
            return Ok(if deleted {
                CredentialDeleteOutcome::Deleted
            } else {
                CredentialDeleteOutcome::NotFound
            });
        }
    }
    let _ = service;
    Ok(CredentialDeleteOutcome::NotFound)
}

fn delete_credential_service(service: &str) -> Result<(), String> {
    delete_credential_service_authoritative(service).map(|_| ())
}

fn read_credential_checked(profile_id: &str) -> Result<Option<Vec<u8>>, String> {
    let service = keychain_service(profile_id);
    if let Some(value) = read_credential_service_checked(&service)? {
        return Ok(Some(value));
    }
    let legacy = legacy_keychain_service(profile_id);
    if legacy != service { read_credential_service_checked(&legacy) } else { Ok(None) }
}

fn credential_exists(profile_id: &str) -> bool {
    let service = keychain_service(profile_id);
    credential_exists_for_service(profile_id, &service, credential_service_exists)
}

fn credential_exists_for_service(
    profile_id: &str,
    service: &str,
    mut exists: impl FnMut(&str) -> bool,
) -> bool {
    let legacy = legacy_keychain_service(profile_id);
    let keychain_exists = exists(service) || (service != legacy && exists(&legacy));
    #[cfg(any(test, debug_assertions))]
    {
        keychain_exists
            || crate::brand::env("ARCA_TEST_MODEL_CREDENTIAL_PROFILES")
                .ok()
                .is_some_and(|profiles| profiles.split(',').any(|item| item == profile_id))
    }
    #[cfg(not(any(test, debug_assertions)))]
    keychain_exists
}

fn set_credential(profile_id: &str, credential: &[u8]) -> Result<(), String> {
    set_credential_service(&keychain_service(profile_id), credential)
}

fn delete_credential(profile_id: &str) -> Result<(), String> {
    let service = keychain_service(profile_id);
    delete_credential_service(&service)?;
    let legacy = legacy_keychain_service(profile_id);
    if legacy != service {
        delete_credential_service(&legacy)?;
    }
    Ok(())
}

fn validate_profile(profile: &ModelProfile) -> Result<(), String> {
    let valid_id = !profile.id.is_empty()
        && profile.id.len() <= 64
        && profile.id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_' | b'.'))
        });
    if !valid_id {
        return Err("profile id must be 1-64 lowercase letters, digits, '.', '_' or '-'".into());
    }
    if profile.name.trim().is_empty() {
        return Err("profile name is required".into());
    }
    if profile.provider.is_empty() || normalize_slug(&profile.provider) != profile.provider {
        return Err("provider must be a lowercase identifier".into());
    }
    if profile.model.trim().is_empty() {
        return Err("model id is required".into());
    }
    normalize_provider_base_url(&profile.base_url)?;
    if !profile.endpoint_path.is_empty() {
        full_request_url(&profile.base_url, &profile.endpoint_path)?;
    }
    if profile.context_window == Some(0) || profile.max_output_tokens == Some(0) {
        return Err("token limits must be greater than zero".into());
    }
    if let Some(env_key) = profile.env_key.as_deref() {
        let mut bytes = env_key.bytes();
        let valid = bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
        if !valid {
            return Err(
                "environment variable name must start with a letter or '_' and contain only letters, digits, or '_'"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_provider_config(provider: &ModelProviderConfig) -> Result<(), String> {
    let valid_id = !provider.id.is_empty()
        && provider.id.len() <= 64
        && provider.id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'-' | b'_' | b'.'))
        });
    if !valid_id {
        return Err("Provider id must be a stable lowercase identifier".into());
    }
    if provider.name.trim().is_empty() {
        return Err("Provider name is required".into());
    }
    if provider.provider.is_empty() || normalize_slug(&provider.provider) != provider.provider {
        return Err("Provider type must be a lowercase identifier".into());
    }
    normalize_provider_base_url(&provider.base_url)?;
    if let Some(model_catalog_url) = provider.model_catalog_url.as_deref() {
        normalize_model_catalog_url(model_catalog_url)?;
    }
    if let Some(portal_url) = provider.portal_url.as_deref() {
        normalize_provider_portal_url(portal_url)?;
    }
    if provider.protocols.is_empty() {
        return Err("Provider must enable at least one protocol".into());
    }
    for protocol in provider.protocols.values() {
        full_request_url(&provider.base_url, &protocol.endpoint_path)?;
    }
    if let Some(source) = provider.api_key_source.as_ref() {
        credential::validate_source(source)?;
    }
    if provider.auth_requirement == AuthRequirement::None && provider.api_key_source.is_some() {
        return Err("credential_not_required: an unauthenticated Provider cannot retain a credential source".into());
    }
    Ok(())
}

pub(crate) fn provider_profiles(
    settings: &crate::settings::Settings,
    provider: &ModelProviderConfig,
) -> Result<BTreeMap<String, ModelProfile>, String> {
    validate_provider_config(provider)?;
    let linked = settings
        .model_profiles
        .iter()
        .flatten()
        .filter(|(_, profile)| profile.provider_id.as_deref() == Some(provider.id.as_str()))
        .collect::<Vec<_>>();
    let missing = linked
        .iter()
        .filter(|(_, profile)| !provider.protocols.contains_key(&profile.protocol))
        .map(|(_, profile)| profile.name.clone())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "model_provider_protocol_in_use: cannot disable protocols used by Models: {}",
            missing.join(", ")
        ));
    }
    let profiles = linked
        .into_iter()
        .map(|(id, profile)| {
            let protocol = provider
                .protocols
                .get(&profile.protocol)
                .expect("missing protocol was checked");
            let mut profile = profile.clone();
            profile.provider = provider.provider.clone();
            profile.base_url = provider.base_url.clone();
            profile.endpoint_path = protocol.endpoint_path.clone();
            profile.env_key = match provider.api_key_source.as_ref() {
                Some(ApiKeySource::Env { name }) => Some(name.clone()),
                _ => None,
            };
            validate_profile(&profile)?;
            Ok((id.clone(), profile))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    Ok(profiles)
}

pub(crate) fn prepare_provider_draft(
    settings: &crate::settings::Settings,
    existing_id: Option<&str>,
    mut provider: ModelProviderConfig,
) -> Result<ModelProviderConfig, String> {
    provider.name = provider.name.trim().to_string();
    provider.provider = normalize_slug(&provider.provider);
    provider.base_url = normalize_provider_base_url(&provider.base_url)?;
    provider.model_catalog_url = provider
        .model_catalog_url
        .map(|value| normalize_model_catalog_url(&value))
        .transpose()?;
    provider.portal_url = provider.portal_url
        .map(|value| normalize_provider_portal_url(&value))
        .transpose()?;
    provider.protocols = provider
        .protocols
        .into_iter()
        .map(|(protocol, config)| {
            normalize_endpoint_path(&config.endpoint_path)
                .map(|endpoint_path| (protocol, ModelProviderProtocolConfig { endpoint_path }))
        })
        .collect::<Result<_, _>>()?;
    if provider.api_key_source.is_none() {
        provider.api_key_source = provider
            .env_key
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|name| ApiKeySource::Env { name });
    }
    provider.api_key_source = match provider.api_key_source {
        Some(ApiKeySource::ArcaStore) => Some(ApiKeySource::ArcaStore),
        Some(ApiKeySource::Env { name }) => Some(ApiKeySource::Env {
            name: name.trim().to_string(),
        }),
        Some(ApiKeySource::File { path }) => Some(ApiKeySource::File {
            path: path.trim().to_string(),
        }),
        Some(ApiKeySource::Helper {
            command,
            args,
            ttl_ms,
        }) => Some(ApiKeySource::Helper {
            command: command.trim().to_string(),
            args: args
                .into_iter()
                .map(|argument| argument.trim().to_string())
                .filter(|argument| !argument.is_empty())
                .collect(),
            ttl_ms,
        }),
        None => None,
    };
    if provider.auth_requirement == AuthRequirement::None {
        provider.api_key_source = None;
    }
    provider.env_key = match provider.api_key_source.as_ref() {
        Some(ApiKeySource::Env { name }) => Some(name.clone()),
        _ => None,
    };
    match existing_id {
        Some(existing_id) => {
            if provider.id.is_empty() {
                provider.id = existing_id.to_string();
            }
            if provider.id != existing_id {
                return Err("asset_identity_change: Model Provider id cannot change".into());
            }
            let existing = settings
                .model_providers
                .as_ref()
                .and_then(|providers| providers.get(existing_id))
                .ok_or_else(|| {
                    "asset_operation_stale: Model Provider no longer exists".to_string()
                })?;
            if existing.provider != provider.provider {
                return Err("asset_identity_change: Model Provider type cannot change".into());
            }
        }
        None => {
            if !provider.id.is_empty() {
                return Err("invalid_asset: Model Provider id is generated by Arca".into());
            }
            let existing_ids = settings
                .model_providers
                .iter()
                .flatten()
                .map(|(id, _)| id.clone())
                .collect();
            provider.id = generated_provider_id(&provider.provider, &existing_ids);
        }
    }
    if settings
        .model_providers
        .iter()
        .flatten()
        .any(|(id, candidate)| {
            id != &provider.id && candidate.name.eq_ignore_ascii_case(&provider.name)
        })
    {
        return Err("asset_identity_conflict: Model Provider name already exists".into());
    }
    validate_provider_config(&provider)?;
    Ok(provider)
}

pub(crate) fn save_provider_bundle(
    provider: ModelProviderConfig,
    profiles: BTreeMap<String, ModelProfile>,
) -> Result<(), String> {
    validate_provider_config(&provider)?;
    mutate_settings(|settings| {
        settings
            .model_providers
            .get_or_insert_default()
            .insert(provider.id.clone(), provider);
        settings
            .model_profiles
            .get_or_insert_default()
            .extend(profiles);
    })
    .map_err(|error| error.to_string())
}

pub(crate) fn credential_snapshot(profile_id: &str) -> Result<Option<Vec<u8>>, String> {
    read_credential_checked(profile_id)
}

fn credential_rollback_profile_id(operation_id: &str, profile_id: &str) -> String {
    format!("{CREDENTIAL_ROLLBACK_PREFIX}.{operation_id}.{profile_id}")
}

/// Persist the pre-transaction credential in Keychain, never in the operation
/// journal. The first byte distinguishes a missing credential from an empty
/// lookup result; user credentials themselves cannot contain newlines because
/// the normal Keychain writer rejects them.
pub(crate) fn persist_credential_rollback(
    operation_id: &str,
    profile_id: &str,
    credential: Option<&[u8]>,
) -> Result<(), String> {
    let mut payload = Zeroizing::new(Vec::with_capacity(1 + credential.map_or(0, |value| value.len())));
    match credential {
        Some(value) => {
            payload.push(1);
            payload.extend_from_slice(value);
        }
        None => payload.push(0),
    }
    set_credential(
        &credential_rollback_profile_id(operation_id, profile_id),
        &payload,
    )
}

/// Returns `None` when no durable rollback item exists, `Some(None)` when the
/// original Profile had no credential, and `Some(Some(bytes))` otherwise.
pub(crate) fn credential_rollback_snapshot(
    operation_id: &str,
    profile_id: &str,
) -> Result<Option<Option<Vec<u8>>>, String> {
    let Some(payload) = read_credential_checked(&credential_rollback_profile_id(operation_id, profile_id))?
    else {
        return Ok(None);
    };
    let Some((&tag, value)) = payload.split_first() else {
        return Err("credential rollback item is invalid".into());
    };
    match tag {
        0 if value.is_empty() => Ok(Some(None)),
        1 => Ok(Some(Some(value.to_vec()))),
        _ => Err("credential rollback item is invalid".into()),
    }
}

pub(crate) fn clear_credential_rollback(
    operation_id: &str,
    profile_id: &str,
) -> Result<(), String> {
    delete_credential(&credential_rollback_profile_id(operation_id, profile_id))
}

fn private_file_path_hash(path: &Path) -> String {
    hex::encode(Sha256::digest(path.as_os_str().as_encoded_bytes()))
}

pub(crate) fn private_file_snapshot_subject(operation_id: &str, path: &Path) -> String {
    let mut binding = Sha256::new();
    binding.update(operation_id.as_bytes());
    binding.update([0]);
    binding.update(path.as_os_str().as_encoded_bytes());
    format!(
        "{PRIVATE_FILE_SNAPSHOT_SERVICE_PREFIX}.{operation_id}.{}",
        hex::encode(binding.finalize())
    )
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or_default();
        let third = chunk.get(2).copied().unwrap_or_default();
        encoded.push(ALPHABET[(first >> 2) as usize] as char);
        encoded.push(ALPHABET[(((first & 0x03) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(ALPHABET[(((second & 0x0f) << 2) | (third >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(ALPHABET[(third & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

fn decode_base64(encoded: &str) -> Result<Vec<u8>, ()> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = encoded.as_bytes();
    if bytes.len() % 4 != 0 {
        return Err(());
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for (index, chunk) in bytes.chunks_exact(4).enumerate() {
        let last = index + 1 == bytes.len() / 4;
        let first = value(chunk[0]).ok_or(())?;
        let second = value(chunk[1]).ok_or(())?;
        let third = if chunk[2] == b'=' {
            if !last || chunk[3] != b'=' {
                return Err(());
            }
            None
        } else {
            Some(value(chunk[2]).ok_or(())?)
        };
        let fourth = if chunk[3] == b'=' {
            if !last {
                return Err(());
            }
            None
        } else {
            Some(value(chunk[3]).ok_or(())?)
        };
        if third.is_none() && fourth.is_some() {
            return Err(());
        }
        decoded.push((first << 2) | (second >> 4));
        if let Some(third) = third {
            decoded.push((second << 4) | (third >> 2));
            if let Some(fourth) = fourth {
                decoded.push((third << 6) | fourth);
            }
        }
    }
    Ok(decoded)
}

pub(crate) fn persist_private_file_snapshot(
    operation_id: &str,
    path: &Path,
    content: &[u8],
) -> Result<String, String> {
    let subject = private_file_snapshot_subject(operation_id, path);
    let content_hash = hex::encode(Sha256::digest(content));
    let mut encoded_content = Zeroizing::new(encode_base64(content));
    let mut payload = Zeroizing::new(format!(
        "{PRIVATE_FILE_SNAPSHOT_VERSION}|{operation_id}|{}|{content_hash}|{}",
        private_file_path_hash(path),
        encoded_content.as_str()
    ));
    encoded_content.zeroize();
    let result = set_credential_service(&subject, payload.as_bytes());
    payload.zeroize();
    result?;
    Ok(subject)
}

pub(crate) fn load_private_file_snapshot(
    operation_id: &str,
    path: &Path,
    expected_content_hash: &str,
) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let subject = private_file_snapshot_subject(operation_id, path);
    let Some(payload) = read_credential_service_checked(&subject)? else {
        return Ok(None);
    };
    let payload = Zeroizing::new(payload);
    let text = std::str::from_utf8(payload.as_slice()).map_err(|_| {
        "private_file_snapshot_invalid: private rollback payload is not UTF-8".to_string()
    })?;
    let mut parts = text.split('|');
    let version = parts.next();
    let bound_operation = parts.next();
    let bound_path_hash = parts.next();
    let bound_content_hash = parts.next();
    let encoded_content = parts.next();
    if parts.next().is_some()
        || version != Some(PRIVATE_FILE_SNAPSHOT_VERSION)
        || bound_operation != Some(operation_id)
        || bound_path_hash != Some(private_file_path_hash(path).as_str())
        || bound_content_hash != Some(expected_content_hash)
    {
        return Err(
            "private_file_snapshot_invalid: private rollback payload binding is invalid".into(),
        );
    }
    let mut content = Zeroizing::new(decode_base64(encoded_content.unwrap_or_default()).map_err(
        |_| {
            "private_file_snapshot_invalid: private rollback payload encoding is invalid"
                .to_string()
        },
    )?);
    if hex::encode(Sha256::digest(content.as_slice())) != expected_content_hash {
        content.zeroize();
        return Err(
            "private_file_snapshot_invalid: private rollback payload hash is invalid".into(),
        );
    }
    Ok(Some(content))
}

#[cfg(test)]
pub(crate) fn private_file_snapshot_exists(operation_id: &str, path: &Path) -> bool {
    credential_service_exists(&private_file_snapshot_subject(operation_id, path))
}

pub(crate) fn clear_private_file_snapshot(operation_id: &str, path: &Path) -> Result<(), String> {
    delete_credential_service(&private_file_snapshot_subject(operation_id, path))
}

pub(crate) fn restore_credential_snapshot(
    profile_id: &str,
    credential: Option<&[u8]>,
) -> Result<(), String> {
    match credential {
        Some(value) => set_credential(profile_id, value),
        None => delete_credential(profile_id),
    }
}

pub fn list_profiles() -> Vec<ModelProfileView> {
    let profiles = load_settings().model_profiles.unwrap_or_default();
    // Presence only, scoped to this read. Shared Providers need one Keychain
    // probe, while each legacy profile service is still checked independently.
    // Mutation-time credential and snapshot validation remain uncached.
    let mut presence = BTreeMap::<String, bool>::new();
    profiles.values()
        .map(|profile| {
            let service = profile.id.strip_prefix(PROVIDER_CREDENTIAL_SUBJECT_PREFIX)
                .map(provider_keychain_service)
                .or_else(|| profiles.get(&profile.id)
                    .and_then(|stored| stored.provider_id.as_deref())
                    .map(provider_keychain_service))
                .unwrap_or_else(|| legacy_keychain_service(&profile.id));
            let credential_saved = credential_exists_for_service(&profile.id, &service, |service| {
                *presence.entry(service.to_owned())
                    .or_insert_with(|| credential_service_exists(service))
            });
            ModelProfileView {
                catalog_key: catalog_key(profile),
                credential_saved,
                profile: profile.clone(),
            }
        })
        .collect()
}

/// Save metadata and optionally update its Keychain credential. `None` keeps
/// the existing credential; an empty string explicitly removes it.
pub fn save_profile(profile: ModelProfile, credential: Option<String>) -> Result<(), String> {
    let _settings_guard = mutation_lock()?;
    validate_profile(&profile)?;
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    validate_provider_model_identity(&settings, &profile, Some(profile.id.as_str()))?;
    let credential_service = profile
        .provider_id
        .as_deref()
        .map(provider_keychain_service)
        .unwrap_or_else(|| legacy_keychain_service(&profile.id));
    let provider_name = unique_provider_name(&settings, &profile.provider);
    let previous_credential = credential
        .as_ref()
        .map(|_| read_credential_service_checked(&credential_service)).transpose()?;
    if let Some(value) = credential.as_deref() {
        if value.is_empty() {
            delete_credential_service(&credential_service)?;
        } else {
            set_credential_service(&credential_service, value.as_bytes())?;
        }
    }

    let profile_id = profile.id.clone();
    if let Err(error) = mutate_settings(|settings| {
        if let Some(provider_id) = profile.provider_id.as_deref() {
            let providers = settings.model_providers.get_or_insert_default();
            let provider =
                providers
                    .entry(provider_id.to_string())
                    .or_insert_with(|| ModelProviderConfig {
                        id: provider_id.to_string(),
                        name: provider_name.clone(),
                        provider: profile.provider.clone(),
                        base_url: profile.base_url.clone(),
                        model_catalog_url: None,
                        portal_url: None,
                        protocols: BTreeMap::new(),
                        auth_requirement: if profile.env_key.is_some() {
                            AuthRequirement::Required
                        } else {
                            AuthRequirement::Optional
                        },
                        api_key_source: profile
                            .env_key
                            .clone()
                            .map(|name| ApiKeySource::Env { name }),
                        env_key: profile.env_key.clone(),
                    });
            provider
                .protocols
                .entry(profile.protocol.clone())
                .or_insert_with(|| ModelProviderProtocolConfig {
                    endpoint_path: if profile.endpoint_path.is_empty() {
                        profile.protocol.default_endpoint_path().into()
                    } else {
                        profile.endpoint_path.clone()
                    },
                });
        }
        settings
            .model_profiles
            .get_or_insert_with(BTreeMap::new)
            .insert(profile_id.clone(), profile);
    }) {
        if let Some(previous) = previous_credential {
            match previous {
                Some(value) => {
                    let _ = set_credential_service(&credential_service, &value);
                }
                None => {
                    let _ = delete_credential_service(&credential_service);
                }
            }
        }
        return Err(error.to_string());
    }
    Ok(())
}

pub fn delete_profile(profile_id: &str) -> Result<(), String> {
    let _settings_guard = mutation_lock()?;
    validate_profile_id(profile_id)?;
    let previous_credential = read_credential_service_checked(&legacy_keychain_service(profile_id))?;
    delete_credential_service(&legacy_keychain_service(profile_id))?;
    if let Err(error) = delete_profile_metadata(profile_id) {
        if let Some(credential) = previous_credential {
            let _ = set_credential_service(&legacy_keychain_service(profile_id), &credential);
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn delete_profile_metadata(profile_id: &str) -> Result<(), String> {
    validate_profile_id(profile_id)?;
    mutate_settings(|settings| {
        if let Some(profiles) = settings.model_profiles.as_mut() {
            profiles.remove(profile_id);
        }
        if let Some(assignments) = settings.model_assignments.as_mut() {
            assignments.retain(|_, assigned| assigned != profile_id);
        }
        if let Some(consumptions) = settings.model_consumptions.as_mut() {
            for records in consumptions.values_mut() {
                records.remove(profile_id);
            }
            consumptions.retain(|_, records| !records.is_empty());
        }
    })
    .map_err(|error| error.to_string())
}

pub(crate) fn delete_provider(provider_id: &str) -> Result<(), String> {
    let _settings_guard = mutation_lock()?;
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    if !settings
        .model_providers
        .as_ref()
        .is_some_and(|providers| providers.contains_key(provider_id))
    {
        return Err("asset_operation_stale: Model Provider no longer exists".into());
    }
    let referenced = settings
        .model_profiles
        .iter()
        .flatten()
        .filter(|(_, profile)| profile.provider_id.as_deref() == Some(provider_id))
        .map(|(_, profile)| profile.name.clone())
        .collect::<Vec<_>>();
    if !referenced.is_empty() {
        return Err(format!(
            "model_provider_in_use: delete or switch these Models first: {}",
            referenced.join(", ")
        ));
    }
    mutate_settings(|settings| {
        if let Some(providers) = settings.model_providers.as_mut() {
            providers.remove(provider_id);
        }
    })
    .map_err(|error| error.to_string())?;
    delete_credential_service(&provider_keychain_service(provider_id))
}

pub(crate) fn credential_present(profile_id: &str) -> bool {
    credential_exists(profile_id)
}

pub(crate) fn provider_credential_present(provider_id: &str) -> bool {
    credential_exists(&provider_credential_subject(provider_id))
}

/// Read a configured Provider's Keychain value for an explicit reveal action.
///
/// Callers must keep the returned value ephemeral and must not serialize or log it.
pub fn reveal_provider_credential(provider_id: &str) -> Result<String, String> {
    let settings = load_settings();
    if !settings
        .model_providers
        .as_ref()
        .is_some_and(|providers| providers.contains_key(provider_id))
    {
        return Err(format!(
            "model_provider_not_found: Provider '{provider_id}' does not exist"
        ));
    }

    let credential = read_credential_service_checked(&provider_keychain_service(provider_id))?
        .ok_or_else(|| {
            format!(
                "model_provider_credential_missing: Provider '{provider_id}' has no API Key in Keychain"
            )
        })?;
    String::from_utf8(credential).map_err(|_| {
        format!(
            "model_provider_credential_invalid: Provider '{provider_id}' has a non-UTF-8 API Key"
        )
    })
}

/// Return the credential contract that prevents an Agent from consuming a
/// Profile safely. The Agent's current delivery strategy decides whether a
/// Keychain-only Provider is enough, or whether an environment variable name
/// is still required.
pub(crate) fn profile_credential_issue(
    agent_id: &str,
    profile: &ModelProfile,
    has_credential: bool,
    delivery: &crate::domain::assets::ApiKeyDelivery,
) -> Option<(&'static str, &'static str)> {
    let source = profile
        .env_key
        .clone()
        .map(|name| ApiKeySource::Env { name })
        .or_else(|| has_credential.then_some(ApiKeySource::ArcaStore));
    let Some(source) = source else {
        return None;
    };
    let resolved = credential::resolve_delivery(agent_id, delivery);
    match credential::select_delivery(agent_id, &source, &resolved) {
        Ok(credential::PreparedCredentialRoute::NativeEnvReference)
            if profile.env_key.is_none() && !matches!(source, ApiKeySource::Env { .. }) =>
        {
            Some(env_key_required_issue(agent_id).unwrap_or((
                "credential_delivery_unsupported",
                "This Agent cannot deliver the stored API Key using the current credential strategy.",
            )))
        }
        Ok(_) => None,
        Err(_) => Some(env_key_required_issue(agent_id).unwrap_or((
            "credential_delivery_unsupported",
            "This Agent cannot deliver the stored API Key using the current credential strategy.",
        ))),
    }
}

fn env_key_required_issue(agent_id: &str) -> Option<(&'static str, &'static str)> {
    if !matches!(
        agent_id,
        "qoder-cli"
            | "grok-build"
            | "opencode"
            | "opencode-desktop"
            | "kilo-code"
            | "kilo-desktop"
            | "qwen-code"
            | "crush"
            | "mistral-vibe"
            | "hermes"
            | "factory-droid"
            | "goose"
    ) {
        return None;
    }
    Some((
        if agent_id == "grok-build" {
            "grok_build_env_key_required"
        } else {
            "model_env_key_required"
        },
        "This Agent cannot read the API Key stored in Arca Keychain; set an external environment variable name on this Profile before switching.",
    ))
}

pub(crate) fn apply_credential_update(
    profile_id: &str,
    credential: Option<&str>,
) -> Result<(), String> {
    match credential {
        None => Ok(()),
        Some("") => delete_credential(profile_id),
        Some(value) => set_credential(profile_id, value.as_bytes()),
    }
}

fn validate_profile_id(profile_id: &str) -> Result<(), String> {
    let dummy = ModelProfile {
        id: profile_id.into(),
        provider_id: None,
        name: "x".into(),
        provider: "custom".into(),
        model_vendor: None,
        native_ids: Default::default(),
        protocol: ModelProtocol::OpenaiResponses,
        base_url: "http://localhost".into(),
        endpoint_path: String::new(),
        model: "x".into(),
        env_key: None,
        context_window: None,
        max_output_tokens: None,
        reasoning: None,
    };
    validate_profile(&dummy)
}

fn command_exists(names: &[&str]) -> bool {
    let mut directories = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    directories.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home().join(".local/bin"),
        home().join(".bun/bin"),
        home().join(".cargo/bin"),
    ]);
    directories
        .iter()
        .any(|directory| names.iter().any(|name| directory.join(name).is_file()))
}

fn agent_installed(names: &[&str], config_locations: &[&str], app_locations: &[&str]) -> bool {
    command_exists(names)
        || config_locations
            .iter()
            .any(|location| config_path(location).exists())
        || app_locations
            .iter()
            .any(|location| expand_tilde(location).exists())
}

pub fn list_agents() -> Vec<ModelAgentView> {
    let settings = load_settings();
    let assignments = settings.model_assignments.clone().unwrap_or_default();
    let assigned_profiles = |agent_id: &str| {
        settings
            .model_selection(agent_id)
            .profiles
            .into_keys()
            .collect::<Vec<_>>()
    };
    let credential_policies = |agent_id: &str| {
        settings
            .model_selection(agent_id)
            .profiles
            .into_iter()
            .map(|(profile_id, record)| (profile_id, record.credential))
            .collect::<BTreeMap<_, _>>()
    };
    let (claude_config_path, claude_config_paths) = path_view(&settings, "claude-code");
    let (claude_desktop_config_path, claude_desktop_config_paths) =
        path_view(&settings, claude_desktop::AGENT_ID);
    let (codex_config_path, codex_config_paths) = path_view(&settings, "codex");
    let (grok_config_path, grok_config_paths) = path_view(&settings, "grok-build");
    let (pi_config_path, pi_config_paths) = path_view(&settings, "pi");
    let (minimax_config_path, minimax_config_paths) = path_view(&settings, "minimax-code");
    let mut agents = vec![
        ModelAgentView {
            id: "claude-code".into(),
            name: String::new(),
            mode: "managed".into(),
            storage_authority: ModelStorageAuthority::ArcaMapping,
            installed: agent_installed(&["claude"], &[".claude"], &[]),
            config_path: claude_config_path,
            config_paths: claude_config_paths,
            docs: "https://code.claude.com/docs/en/settings".into(),
            assigned_profile: assignments.get("claude-code").cloned(),
            assigned_profiles: assigned_profiles("claude-code"),
            active_profile: assignments.get("claude-code").cloned(),
            supports_multiple: false,
            supports_global_selection: true,
            credential_mode: "keychain-command".into(),
            credential_capabilities: credential::agent_capabilities("claude-code"),
            credential_policies: credential_policies("claude-code"),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![ModelProtocol::AnthropicMessages],
            note: "Anthropic-compatible endpoint; restart the session after applying.".into(),
        },
        ModelAgentView {
            id: claude_desktop::AGENT_ID.into(),
            name: String::new(),
            mode: "managed".into(),
            storage_authority: ModelStorageAuthority::ArcaMapping,
            installed: agent_installed(&[], &[], &["/Applications/Claude.app"]),
            config_path: claude_desktop_config_path,
            config_paths: claude_desktop_config_paths,
            docs: "https://support.claude.com/".into(),
            assigned_profile: assignments.get(claude_desktop::AGENT_ID).cloned(),
            assigned_profiles: assigned_profiles(claude_desktop::AGENT_ID),
            active_profile: assignments.get(claude_desktop::AGENT_ID).cloned(),
            supports_multiple: false,
            supports_global_selection: true,
            credential_mode: "keychain-export".into(),
            credential_capabilities: credential::agent_capabilities(claude_desktop::AGENT_ID),
            credential_policies: credential_policies(claude_desktop::AGENT_ID),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![ModelProtocol::AnthropicMessages],
            note: "Exports the selected Provider credential into Claude's private Profile; restart Claude Desktop after applying.".into(),
        },
        ModelAgentView {
            id: "codex".into(),
            name: String::new(),
            mode: "managed".into(),
            storage_authority: ModelStorageAuthority::ArcaMapping,
            installed: agent_installed(&["codex"], &[".codex"], &["/Applications/Codex.app"]),
            config_path: codex_config_path,
            config_paths: codex_config_paths,
            docs: "https://developers.openai.com/codex/config-advanced".into(),
            assigned_profile: assignments.get("codex").cloned(),
            assigned_profiles: assigned_profiles("codex"),
            active_profile: assignments.get("codex").cloned(),
            supports_multiple: false,
            supports_global_selection: true,
            credential_mode: "keychain-command".into(),
            credential_capabilities: credential::agent_capabilities("codex"),
            credential_policies: credential_policies("codex"),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![ModelProtocol::OpenaiResponses],
            note: "Custom providers currently use the Responses API.".into(),
        },
        ModelAgentView {
            id: "grok-build".into(),
            name: String::new(),
            mode: "managed".into(),
            storage_authority: ModelStorageAuthority::NativeRegistry,
            installed: agent_installed(&["grok"], &[".grok"], &[]),
            config_path: grok_config_path,
            config_paths: grok_config_paths,
            docs: GROK_BUILD_MODEL_DOCS.into(),
            assigned_profile: assignments.get("grok-build").cloned(),
            assigned_profiles: assigned_profiles("grok-build"),
            active_profile: assignments.get("grok-build").cloned(),
            supports_multiple: true,
            supports_global_selection: true,
            credential_mode: "environment-reference".into(),
            credential_capabilities: credential::agent_capabilities("grok-build"),
            credential_policies: credential_policies("grok-build"),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
            ],
            note: "Arca manages custom model entries and [models].default only; fork_secondary_model remains independent. Authenticated Profiles use an external environment variable reference.".into(),
        },
        ModelAgentView {
            id: "pi".into(),
            name: String::new(),
            mode: "managed".into(),
            storage_authority: ModelStorageAuthority::NativeRegistry,
            installed: agent_installed(&["pi"], &[".pi/agent"], &[]),
            config_path: pi_config_path,
            config_paths: pi_config_paths,
            docs: "https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/models.md".into(),
            assigned_profile: assignments.get("pi").cloned(),
            assigned_profiles: assigned_profiles("pi"),
            active_profile: assignments.get("pi").cloned(),
            supports_multiple: true,
            supports_global_selection: true,
            credential_mode: "keychain-command".into(),
            credential_capabilities: credential::agent_capabilities("pi"),
            credential_policies: credential_policies("pi"),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
            ],
            note: "Arca updates the custom provider and default model as one transaction.".into(),
        },
        ModelAgentView {
            id: "minimax-code".into(),
            name: String::new(),
            mode: "guided".into(),
            storage_authority: ModelStorageAuthority::Guided,
            installed: agent_installed(
                &["mavis"],
                &[".mavis"],
                &["/Applications/MiniMax Code.app"],
            ),
            config_path: minimax_config_path,
            config_paths: minimax_config_paths,
            docs: MINIMAX_CODE_DOCS.into(),
            assigned_profile: None,
            assigned_profiles: Vec::new(),
            active_profile: None,
            supports_multiple: false,
            supports_global_selection: true,
            credential_mode: "guided".into(),
            credential_capabilities: credential::agent_capabilities("minimax-code"),
            credential_policies: BTreeMap::new(),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
            ],
            note: "MiniMax Code supports custom providers, but its current config flow stores options.apiKey as plaintext YAML; use its own model configuration flow rather than exporting a Arca Keychain secret.".into(),
        },
        ModelAgentView {
            id: "qoder".into(),
            name: String::new(),
            mode: "guided".into(),
            storage_authority: ModelStorageAuthority::Guided,
            installed: agent_installed(
                &[],
                &["Library/Application Support/Qoder"],
                &["/Applications/Qoder.app"],
            ),
            config_path: String::new(),
            config_paths: Vec::new(),
            docs: QODER_DOCS.into(),
            assigned_profile: None,
            assigned_profiles: Vec::new(),
            active_profile: None,
            supports_multiple: false,
            supports_global_selection: true,
            credential_mode: "guided".into(),
            credential_capabilities: credential::agent_capabilities("qoder"),
            credential_policies: BTreeMap::new(),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: Vec::new(),
            note: "请在 Qoder Settings → Models 中添加和切换自定义模型。Arca 当前提供配置引导，不自动写入模型。".into(),
        },
        {
            let mut view = managed_agent_view(
                &settings, "zcode", &[], &[".zcode/v2"],
                "https://zcode.z.ai/cn/docs/configuration",
                "支持 ZCode 自定义 Chat Completions 多模型。添加后重启 ZCode，在会话中选择模型；API Key 仅按显式明文交付策略写入私有配置。",
            );
            view.installed = agent_installed(&[], &[".zcode/v2"], &["/Applications/ZCode.app"]);
            view.credential_mode = "explicit-plaintext".into();
            view.active_profile = None;
            view.assigned_profile = None;
            view
        },
        {
            let mut view = managed_agent_view(
                &settings, "qoder-desktop", &[], &[],
                QODER_DESKTOP_DOCS,
                "支持 Qoder Desktop 0.1.8 的自定义端点和多模型。添加后重启 Qoder，在会话中选择模型；API Key 按下方策略交付。与 Qoder CLI 共用 settings.json。",
            );
            view.installed = agent_installed(&[], &[], &["/Applications/Qoder.app"]);
            view.supports_global_selection = supports_global_model_selection("qoder-desktop");
            view.active_profile = None;
            view.assigned_profile = None;
            view
        },
        managed_agent_view(
            &settings, "qoder-cli", &["qoder", "qodercli"], &[],
            QODER_CLI_DOCS,
            "支持 Qoder CLI 1.1.50 起的自定义端点、多模型和当前模型切换。重启 CLI 后生效；API Key 使用环境变量引用，启动 CLI 时须提供该变量。与 Desktop 共用 settings.json。",
        ),
        managed_agent_view(
            &settings, "opencode", &["opencode"], &[".config/opencode"],
            "https://opencode.ai/docs/models/",
            "原生 provider/models 与 model；API Key 只写 {env:VAR} 引用。",
        ),
        {
            let mut view = managed_agent_view(
                &settings, "opencode-desktop", &[], &[],
                "https://opencode.ai/docs/models/",
                "与 OpenCode CLI 共用本机全局配置和默认模型；桌面端使用独立的 Arca 模型条目。重启本地服务后生效，远程服务需在对应主机配置。",
            );
            view.installed = crate::paths::system_probe_path("/Applications/OpenCode.app/Contents/Info.plist").is_file()
                || home().join("Applications/OpenCode.app/Contents/Info.plist").is_file();
            view
        },
        managed_agent_view(
            &settings, "kilo-code", &["kilo"], &[".config/kilo"],
            "https://kilo.ai/docs/code-with-ai/agents/custom-models",
            "原生 provider/models 与 model；API Key 只写 {env:VAR} 引用。",
        ),
        {
            let mut view = managed_agent_view(
                &settings, "kilo-desktop", &[], &[],
                "https://kilo.ai/docs/desktop/settings/ai",
                "管理 Kilo Desktop 独立运行配置中的自定义模型；添加后重启 Kilo，并在会话中选择模型。API Key 使用环境变量或文件引用，不向配置导出 Keychain 密钥。",
            );
            view.installed = crate::paths::system_probe_path("/Applications/Kilo.app/Contents/Info.plist").is_file()
                || home().join("Applications/Kilo.app/Contents/Info.plist").is_file();
            view
        },
        managed_agent_view(
            &settings, "qwen-code", &["qwen"], &[".qwen"],
            "https://qwenlm.github.io/qwen-code-docs/en/users/configuration/model-providers/",
            "原生 modelProviders；API Key 由 envKey 指向外部环境变量。",
        ),
        managed_agent_view(
            &settings, "crush", &["crush"], &[".config/crush"],
            "https://github.com/charmbracelet/crush",
            "原生 providers 与 models.large；不会改 small 等辅助槽位。",
        ),
        managed_agent_view(
            &settings, "mistral-vibe", &["vibe"], &[".vibe"],
            "https://docs.mistral.ai/vibe/code/cli/api-keys-profiles",
            "原生 providers/models 与 active_model；API Key 由 api_key_env_var 引用。",
        ),
        managed_agent_view(
            &settings, "hermes", &["hermes"], &[".hermes"],
            "https://hermes-agent.nousresearch.com/docs/user-guide/configuring-models",
            "原生命名 custom provider、model_aliases 与主模型指针；辅助任务模型保持独立。",
        ),
        managed_agent_view(
            &settings, "factory-droid", &["droid"], &[".factory"],
            "https://docs.factory.ai/cli/byok/overview",
            "原生 customModels 与 model；API Key 只写 ${VAR} 引用。",
        ),
        managed_agent_view(
            &settings, "goose", &["goose"], &["Library/Application Support/Block/goose"],
            "https://block.github.io/goose/docs/getting-started/providers",
            "原生 providers/active_provider 与 declarative custom provider；密钥由外部环境变量提供。",
        ),
    ];
    // Both clients share native providers, but their api_key/env fields contain
    // literal credentials. Do not advertise an environment-reference writer.
    for (id, commands, apps, docs, note) in [
        (
            "kimi-code", vec!["kimi"], vec![],
            "https://www.kimi.com/code/docs/en/kimi-code-cli/configuration/providers.html",
            "请在 Kimi /provider 中配置模型。providers.env 保存的是凭据值，不是系统环境变量引用；Arca 不向此文件导出 Keychain 密钥。",
        ),
        (
            "kimi-code-desktop", vec![],
            vec!["/Applications/Kimi Code.app", "~/Applications/Kimi Code.app"],
            "https://www.kimi.com/code/docs/en/kimi-code-desktop/getting-started.html",
            "请在 Kimi Settings → Providers 中配置模型，再从会话选择。与 CLI 共用 config.toml；当前凭据字段要求明文值，Arca 不自动写入。",
        ),
    ] {
        let (config_path, config_paths) = path_view(&settings, id);
        agents.push(ModelAgentView {
            id: id.into(),
            name: String::new(),
            mode: "guided".into(),
            storage_authority: ModelStorageAuthority::Guided,
            installed: agent_installed(&commands, &[], &apps),
            config_path,
            config_paths,
            docs: docs.into(),
            assigned_profile: None,
            assigned_profiles: Vec::new(),
            active_profile: None,
            supports_multiple: true,
            supports_global_selection: false,
            credential_mode: "guided".into(),
            credential_capabilities: credential::agent_capabilities(id),
            credential_policies: BTreeMap::new(),
            default_delivery: Default::default(),
            available_deliveries: Vec::new(),
            supported_protocols: vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
                ModelProtocol::GeminiGenerateContent,
            ],
            note: note.into(),
        });
    }
    // Display identity belongs to the shared Agent catalog, not capability adapters.
    let definitions = crate::agents::builtin_agents();
    for agent in &mut agents {
        agent.name = definitions.get(&agent.id)
            .and_then(|definition| definition.name.clone())
            .unwrap_or_else(|| agent.id.clone());
        let selection = settings.model_selection(&agent.id);
        agent.default_delivery = selection.default_delivery.clone();
        agent.available_deliveries = credential::available_deliveries(&agent.id);
    }
    agents
}

/// Desktop has per-conversation selection; CLI settings.model is not its current model.
pub(crate) fn supports_global_model_selection(agent_id: &str) -> bool {
    !matches!(agent_id, "qoder-desktop" | "zcode" | "kilo-desktop")
}

pub(crate) fn normalize_model_selection(agent_id: &str, selection: &mut crate::domain::assets::ModelAgentSelection) {
    if supports_global_model_selection(agent_id) {
        selection.normalize_active();
    } else {
        selection.active_profile_id = None;
    }
}

fn managed_agent_view(
    settings: &crate::settings::Settings,
    id: &str,
    commands: &[&str],
    config_locations: &[&str],
    docs: &str,
    note: &str,
) -> ModelAgentView {
    let (config_path, config_paths) = path_view(settings, id);
    let selection = settings.model_selection(id);
    let credential_policies = selection
        .profiles
        .iter()
        .map(|(profile_id, record)| (profile_id.clone(), record.credential.clone()))
        .collect();
    ModelAgentView {
        id: id.into(),
        name: String::new(),
        mode: "managed".into(),
        storage_authority: ModelStorageAuthority::NativeRegistry,
        installed: agent_installed(commands, config_locations, &[]),
        config_path,
        config_paths,
        docs: docs.into(),
        assigned_profile: selection.active_profile_id.clone(),
        assigned_profiles: selection.profiles.keys().cloned().collect(),
        active_profile: selection.active_profile_id,
        supports_multiple: true,
        supports_global_selection: supports_global_model_selection(id),
        credential_mode: "environment-reference".into(),
        credential_capabilities: credential::agent_capabilities(id),
        credential_policies,
        default_delivery: Default::default(),
        available_deliveries: Vec::new(),
        supported_protocols: match id {
            "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" => vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
                ModelProtocol::GeminiGenerateContent,
            ],
            "qwen-code" | "crush" | "hermes" | "goose" => vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiCompletions,
            ],
            "mistral-vibe" | "zcode" => vec![ModelProtocol::OpenaiCompletions],
            _ => vec![
                ModelProtocol::AnthropicMessages,
                ModelProtocol::OpenaiResponses,
                ModelProtocol::OpenaiCompletions,
            ],
        },
        note: note.into(),
    }
}

/// Canonical read-only capability lookup used by the shared consumption
/// service. Keeping this projection here prevents another copy of the managed
/// and guided Agent matrix from drifting out of sync with the Model writer.
pub fn model_agent_capability(agent_id: &str) -> Option<ModelAgentView> {
    list_agents().into_iter().find(|agent| agent.id == agent_id)
}

pub(crate) fn observe_active_model_for_settings(
    settings: &crate::settings::Settings,
    agent_id: &str,
) -> ObservedActiveModel {
    let profiles = settings.model_profiles.clone().unwrap_or_default();
    let Some(paths) = configured_path_strings_checked(settings, agent_id)
        .ok()
        .flatten()
        .map(|paths| {
            paths
                .into_iter()
                .map(|path| expand_tilde(&path))
                .collect::<Vec<_>>()
        })
    else {
        return ObservedActiveModel::None;
    };
    if agent_id == claude_desktop::AGENT_ID {
        return claude_desktop::observe_active(settings, &paths);
    }
    let mut selected_pi_model = None;
    let selected = match agent_id {
        "grok-build" => read_toml(&paths[0]).ok().and_then(|(document, _)| {
            document
                .get("models")?
                .as_table()?
                .get("default")?
                .as_str()
                .map(str::to_string)
        }),
        "pi" => {
            let value = read_jsonc(&paths[1]).ok().and_then(|(root, original)| {
                original?;
                root.to_serde_value()
            });
            selected_pi_model = value
                .as_ref()
                .and_then(|value| value.get("defaultModel"))
                .and_then(Value::as_str)
                .map(str::to_string);
            value
                .as_ref()
                .and_then(|value| value.get("defaultProvider"))
                .and_then(Value::as_str)
                .map(str::to_string)
        }
        "claude-code" | "codex" => settings.model_selection(agent_id).active_profile_id,
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => {
            return match adapters::observe_active(agent_id, &paths, &profiles) {
                adapters::ObservedActiveModel::Managed(id) => ObservedActiveModel::Managed(id),
                adapters::ObservedActiveModel::External => ObservedActiveModel::External,
                adapters::ObservedActiveModel::None => ObservedActiveModel::None,
                adapters::ObservedActiveModel::Conflicted => ObservedActiveModel::Conflicted,
            };
        }
        _ => None,
    };
    let Some(selected) = selected else {
        return ObservedActiveModel::None;
    };
    let matches: Vec<_> = profiles
        .values()
        .filter(|profile| match agent_id {
            "grok-build" => selected == grok_model_id(profile),
            "pi" => {
                selected
                    == profile
                        .native_ids
                        .get("pi")
                        .cloned()
                        .unwrap_or_else(|| generated_pi_provider_id(settings, profile))
                    && selected_pi_model.as_deref() == Some(profile.model.as_str())
            }
            _ => selected == profile.id,
        })
        .map(|profile| profile.id.clone())
        .collect();
    match matches.as_slice() {
        [profile_id] => ObservedActiveModel::Managed(profile_id.clone()),
        [] => ObservedActiveModel::External,
        _ => ObservedActiveModel::Conflicted,
    }
}

/// Adopt an Agent-native `/model` switch only when it points at an enabled
/// Arca-managed Profile. External/built-in selections remain read-only and do
/// not overwrite Arca relationships.
pub fn reconcile_active_models() -> Result<(), String> {
    let _settings_guard = mutation_lock()?;
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    let agent_ids: Vec<String> = settings
        .model_consumptions
        .iter()
        .flatten()
        .map(|(agent_id, _)| agent_id.clone())
        .chain(
            settings
                .model_assignments
                .iter()
                .flatten()
                .map(|(id, _)| id.clone()),
        )
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut updates = Vec::new();
    for agent_id in agent_ids {
        let ObservedActiveModel::Managed(profile_id) =
            observe_active_model_for_settings(&settings, &agent_id)
        else {
            continue;
        };
        let selection = settings.model_selection(&agent_id);
        if selection.active_profile_id.as_deref() == Some(profile_id.as_str())
            || !selection
                .profiles
                .get(&profile_id)
                .is_some_and(|record| record.enabled)
        {
            continue;
        }
        let profiles = settings.model_profiles.as_ref();
        let Some(profile) = profiles.and_then(|profiles| profiles.get(&profile_id)) else {
            continue;
        };
        if !matches!(observe_profile_consumption(&agent_id, profile, true), Ok(ModelObservedState::Synced)) {
            continue;
        }
        updates.push((agent_id, profile_id));
    }
    if updates.is_empty() {
        return Ok(());
    }
    mutate_settings(|settings| {
        for (agent_id, profile_id) in &updates {
            let mut selection = settings.model_selection(agent_id);
            selection.active_profile_id = Some(profile_id.clone());
            if let Some(record) = selection.profiles.get_mut(profile_id) {
                record.last_selected_at =
                    Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
            }
            settings.set_model_selection(agent_id, selection);
        }
    })
    .map_err(|error| error.to_string())
}

/// Inspect only the fields owned by the Model adapter. Candidate generation
/// preserves all unrelated bytes, so comparing it with the source identifies
/// owned-field drift without treating comments or unknown settings as drift.
pub fn observe_profile(
    agent_id: &str,
    profile: &ModelProfile,
) -> Result<ModelObservedState, String> {
    let profile = materialize_profile_for_agent(agent_id, profile)?;
    let has_credential = credential_exists(&profile.id);
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    let observed = match agent_id {
        "claude-code" => observe_prepared(prepare_claude(&paths[0], &profile, has_credential)),
        claude_desktop::AGENT_ID => claude_desktop::observe(&paths, &profile),
        "codex" => observe_prepared(prepare_codex(&paths[0], &profile, has_credential)),
        "grok-build" => observe_prepared(prepare_grok_build(&paths[0], &profile)),
        "pi" => {
            let models = observe_prepared(prepare_pi_models_with_api_key(
                &paths[0],
                &profile,
                pi_api_key_value(&profile, has_credential)?,
            ));
            let settings = observe_prepared(prepare_pi_settings(&paths[1], &profile));
            combine_observed(models, settings)
        }
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => Ok(adapters::observe_prepared_files(
            prepare_observed_native_files(agent_id, &paths, &profile, true, has_credential),
        )),
        _ => Ok(ModelObservedState::Conflicted),
    }?;
    // Credentials are optional because local and gateway endpoints may not
    // require authentication. Presence is surfaced on the central Profile; it
    // is not drift unless a future Profile schema explicitly marks it required.
    Ok(observed)
}

/// Observe one installed relationship without requiring it to be the Agent's
/// current/default model. Multi-model Agents keep several provider entries but
/// only one active pointer.
pub fn observe_profile_consumption(
    agent_id: &str,
    profile: &ModelProfile,
    active: bool,
) -> Result<ModelObservedState, String> {
    if active || matches!(agent_id, "claude-code" | claude_desktop::AGENT_ID | "codex") {
        return observe_profile(agent_id, profile);
    }
    let profile = materialize_profile_for_agent(agent_id, profile)?;
    let has_credential = credential_exists(&profile.id);
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    let absent = match agent_id {
        "grok-build" => cleared_toml_profile_absent(prepare_clear_grok_build(&paths[0], &profile)),
        "pi" => cleared_toml_profile_absent(prepare_clear_pi_models(&paths[0], &profile)),
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => {
            adapters::cleared_profile_absent(adapters::prepare_clear(agent_id, &paths, &profile))
        }
        _ => Ok(false),
    };
    match absent {
        Ok(true) => return Ok(ModelObservedState::Missing),
        Ok(false) => {}
        Err(_) => return Ok(ModelObservedState::Conflicted),
    }
    match agent_id {
        "grok-build" => observe_prepared(prepare_grok_model(&paths[0], &profile)),
        "pi" => observe_prepared(prepare_pi_models_with_api_key(
            &paths[0],
            &profile,
            pi_api_key_value(&profile, has_credential)?,
        )),
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => Ok(adapters::observe_prepared_files(
            prepare_observed_native_files(agent_id, &paths, &profile, false, has_credential),
        )),
        _ => observe_profile(agent_id, &profile),
    }
}

fn cleared_toml_profile_absent(
    prepared: Result<(Option<String>, String), String>,
) -> Result<bool, String> {
    let (original, candidate) = prepared?;
    Ok(original
        .as_ref()
        .is_none_or(|original| original == &candidate))
}

/// Detect model-owned fields when no desired Profile exists. This is a
/// deliberately identity-free external projection: it prevents a central
/// selection from silently taking over an Agent configuration without treating
/// the Agent file as a source of central Profile metadata.
pub fn observe_external_model(agent_id: &str) -> Result<ExternalModelObservedState, String> {
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    match agent_id {
        "claude-code" => observe_external_claude(&paths[0]),
        claude_desktop::AGENT_ID => claude_desktop::observe_external(&paths),
        "codex" => observe_external_codex(&paths[0]),
        "grok-build" => observe_external_grok_build(&paths[0]),
        "pi" => observe_external_pi(&paths[0], &paths[1]),
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => Ok(adapters::observe_external(agent_id, &paths[0])),
        _ => Ok(ExternalModelObservedState::Absent),
    }
}

fn observe_external_claude(path: &Path) -> Result<ExternalModelObservedState, String> {
    let (root, original) = match read_jsonc(path) {
        Ok(value) => value,
        Err(_) if path.exists() => return Ok(ExternalModelObservedState::Conflicted),
        Err(error) => return Err(error),
    };
    if original.is_none() {
        return Ok(ExternalModelObservedState::Absent);
    }
    let object = match json_root_object(&root, path) {
        Ok(object) => object,
        Err(_) => return Ok(ExternalModelObservedState::Conflicted),
    };
    if ensure_unique_keys(&object, path, "$root").is_err() {
        return Ok(ExternalModelObservedState::Conflicted);
    }
    let mut present = object.get("model").is_some() || object.get("apiKeyHelper").is_some();
    if object.get("env").is_some() {
        let Some(env) = object.object_value("env") else {
            return Ok(ExternalModelObservedState::Conflicted);
        };
        if ensure_unique_keys(&env, path, "env").is_err() {
            return Ok(ExternalModelObservedState::Conflicted);
        }
        present |= [
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
        ]
        .iter()
        .any(|field| env.get(field).is_some());
    }
    Ok(if present {
        ExternalModelObservedState::Present
    } else {
        ExternalModelObservedState::Absent
    })
}

fn observe_external_codex(path: &Path) -> Result<ExternalModelObservedState, String> {
    let (document, original) = match read_toml(path) {
        Ok(value) => value,
        Err(_) if path.exists() => return Ok(ExternalModelObservedState::Conflicted),
        Err(error) => return Err(error),
    };
    if original.is_none() {
        return Ok(ExternalModelObservedState::Absent);
    }
    Ok(
        if document.as_table().contains_key("model")
            || document.as_table().contains_key("model_provider")
        {
            ExternalModelObservedState::Present
        } else {
            ExternalModelObservedState::Absent
        },
    )
}

fn observe_external_grok_build(path: &Path) -> Result<ExternalModelObservedState, String> {
    let (document, original) = match read_toml(path) {
        Ok(value) => value,
        Err(_) if path.exists() => return Ok(ExternalModelObservedState::Conflicted),
        Err(error) => return Err(error),
    };
    if original.is_none() {
        return Ok(ExternalModelObservedState::Absent);
    }
    let has_default = document
        .get("models")
        .and_then(Item::as_table)
        .is_some_and(|models| models.contains_key("default"));
    let has_custom_models = document
        .get("model")
        .and_then(Item::as_table)
        .is_some_and(|models| !models.is_empty());
    Ok(if has_default || has_custom_models {
        ExternalModelObservedState::Present
    } else {
        ExternalModelObservedState::Absent
    })
}

fn observe_external_pi(
    models_path: &Path,
    settings_path: &Path,
) -> Result<ExternalModelObservedState, String> {
    let models_present = match read_jsonc(models_path) {
        Ok((_, None)) => false,
        Ok((root, Some(_))) => {
            let object = match json_root_object(&root, models_path) {
                Ok(object) => object,
                Err(_) => return Ok(ExternalModelObservedState::Conflicted),
            };
            if ensure_unique_keys(&object, models_path, "$root").is_err() {
                return Ok(ExternalModelObservedState::Conflicted);
            }
            if object.get("providers").is_some() {
                let Some(providers) = object.object_value("providers") else {
                    return Ok(ExternalModelObservedState::Conflicted);
                };
                if ensure_unique_keys(&providers, models_path, "providers").is_err() {
                    return Ok(ExternalModelObservedState::Conflicted);
                }
                !providers.properties().is_empty()
            } else {
                false
            }
        }
        Err(_) if models_path.exists() => return Ok(ExternalModelObservedState::Conflicted),
        Err(error) => return Err(error),
    };
    let settings_present = match read_jsonc(settings_path) {
        Ok((_, None)) => false,
        Ok((root, Some(_))) => {
            let object = match json_root_object(&root, settings_path) {
                Ok(object) => object,
                Err(_) => return Ok(ExternalModelObservedState::Conflicted),
            };
            if ensure_unique_keys(&object, settings_path, "$root").is_err() {
                return Ok(ExternalModelObservedState::Conflicted);
            }
            object.get("defaultProvider").is_some() || object.get("defaultModel").is_some()
        }
        Err(_) if settings_path.exists() => return Ok(ExternalModelObservedState::Conflicted),
        Err(error) => return Err(error),
    };
    Ok(if models_present || settings_present {
        ExternalModelObservedState::Present
    } else {
        ExternalModelObservedState::Absent
    })
}

fn observe_prepared(
    prepared: Result<(Option<String>, String), String>,
) -> Result<ModelObservedState, String> {
    match prepared {
        Ok((None, _)) => Ok(ModelObservedState::Missing),
        Ok((Some(original), candidate)) if original == candidate => Ok(ModelObservedState::Synced),
        Ok((Some(_), _)) => Ok(ModelObservedState::Drifted),
        Err(_) => Ok(ModelObservedState::Conflicted),
    }
}

fn combine_observed(
    left: Result<ModelObservedState, String>,
    right: Result<ModelObservedState, String>,
) -> Result<ModelObservedState, String> {
    use ModelObservedState::*;
    let (left, right) = (left?, right?);
    Ok(if left == Conflicted || right == Conflicted {
        Conflicted
    } else if left == Missing || right == Missing {
        Missing
    } else if left == Drifted || right == Drifted {
        Drifted
    } else {
        Synced
    })
}

fn profile_for_apply(profile_id: &str) -> Result<ModelProfile, String> {
    load_settings()
        .model_profiles
        .and_then(|profiles| profiles.get(profile_id).cloned())
        .ok_or_else(|| format!("unknown model profile: {profile_id}"))
}

fn ensure_supported(agent_id: &str, protocol: &ModelProtocol) -> Result<(), String> {
    let supported = match agent_id {
        "claude-code" => matches!(protocol, ModelProtocol::AnthropicMessages),
        claude_desktop::AGENT_ID => matches!(protocol, ModelProtocol::AnthropicMessages),
        "codex" => matches!(protocol, ModelProtocol::OpenaiResponses),
        "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" => true,
        "grok-build" | "pi" | "factory-droid" => {
            !matches!(protocol, ModelProtocol::GeminiGenerateContent)
        }
        "qwen-code" | "crush" | "hermes" | "goose" => matches!(
            protocol,
            ModelProtocol::AnthropicMessages | ModelProtocol::OpenaiCompletions
        ),
        "mistral-vibe" | "zcode" => matches!(protocol, ModelProtocol::OpenaiCompletions),
        "qoder-desktop" | "qoder-cli" => !matches!(protocol, ModelProtocol::GeminiGenerateContent),
        "qoder" => {
            return Err(format!(
                "Qoder custom models must be configured through Settings → Models; see {QODER_DOCS}"
            ))
        }
        "minimax-code" => {
            return Err(format!(
                "MiniMax Code custom providers currently persist options.apiKey in plaintext YAML and do not expose a Keychain-compatible credential command; see {MINIMAX_CODE_DOCS}"
            ))
        }
        _ => return Err(format!("unsupported model Agent: {agent_id}")),
    };
    if supported {
        Ok(())
    } else {
        Err(format!(
            "{} does not support the '{}' profile protocol in this Arca release",
            agent_id,
            protocol_name(protocol)
        ))
    }
}

fn protocol_name(protocol: &ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::AnthropicMessages => "anthropic-messages",
        ModelProtocol::OpenaiResponses => "openai-responses",
        ModelProtocol::OpenaiCompletions => "openai-completions",
        ModelProtocol::GeminiGenerateContent => "gemini-generate-content",
    }
}

pub fn apply_profile(agent_id: &str, profile_id: &str) -> Result<ModelApplyResult, String> {
    apply_profile_target(agent_id, profile_id).map_err(ModelTargetError::into_message)
}

pub fn set_model_credential_delivery(
    agent_id: &str,
    profile_id: &str,
    delivery: crate::domain::assets::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<ModelApplyResult, String> {
    commit_credential_delivery(agent_id, Some(profile_id), delivery, confirm_plaintext)
}

pub fn set_agent_credential_delivery(
    agent_id: &str,
    delivery: crate::domain::assets::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<ModelApplyResult, String> {
    commit_credential_delivery(agent_id, None, delivery, confirm_plaintext)
}

fn commit_credential_delivery(
    agent_id: &str,
    profile_id: Option<&str>,
    delivery: crate::domain::assets::ApiKeyDelivery,
    confirm_plaintext: bool,
) -> Result<ModelApplyResult, String> {
    let plan = crate::assets::planner::plan_model_credential_delivery(
        agent_id, profile_id, delivery, confirm_plaintext,
    )?;
    let files = plan.target_files.clone();
    let inventory = crate::assets::commit_asset_operation(crate::assets::AssetCommitRequest {
        operation_id: plan.operation_id.clone(),
        candidate_hash: plan.candidate_hash,
    })?;
    if inventory.target_incidents.iter().any(|incident| incident.operation_id == plan.operation_id) {
        return Err("target_convergence_failed: credential policy saved but Agent projection requires recovery".into());
    }
    Ok(ModelApplyResult {
        agent: agent_id.into(),
        profile: profile_id.unwrap_or_default().into(),
        restart_required: !files.is_empty(),
        files,
        message: format!("{agent_id} credential delivery updated."),
    })
}

pub(crate) fn apply_profile_target(
    agent_id: &str,
    profile_id: &str,
) -> Result<ModelApplyResult, ModelTargetError> {
    let _settings_guard = mutation_lock()?;
    apply_profile_consumption_with_credential_presence_target(
        agent_id,
        profile_id,
        credential_exists(profile_id),
        true,
    )
}

pub(crate) fn apply_profile_consumption_target(
    agent_id: &str,
    profile_id: &str,
    active: bool,
) -> Result<ModelApplyResult, ModelTargetError> {
    let _settings_guard = mutation_lock()?;
    apply_profile_consumption_with_credential_presence_target(
        agent_id,
        profile_id,
        credential_exists(profile_id),
        active,
    )
}

pub(crate) fn apply_profile_consumption_with_credential_presence_target(
    agent_id: &str,
    profile_id: &str,
    has_credential: bool,
    active: bool,
) -> Result<ModelApplyResult, ModelTargetError> {
    let active = active && supports_global_model_selection(agent_id);
    let profile = profile_for_apply(profile_id)?;
    ensure_supported(agent_id, &profile.protocol)?;
    let credential_route = credential_route_for(agent_id, &profile, has_credential)?;
    if let Some((source, credential::PreparedCredentialRoute::NativeFileReference)) =
        credential_route.as_ref()
    {
        let _validated = credential::resolve_source(source, None)?;
    }
    let route_has_credential = credential_route.is_some();
    let profile = materialize_profile_for_agent(agent_id, &profile)?;
    if credential_route.is_none() {
        let delivery = crate::settings::load_settings()
            .model_selection(agent_id)
            .delivery_for(&profile.id);
        if let Some((code, message)) =
            profile_credential_issue(agent_id, &profile, has_credential, &delivery)
        {
            return Err(format!("{code}: {message}").into());
        }
    }
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    let result = match agent_id {
        "claude-code" => {
            apply_claude(&paths[0], &profile, route_has_credential).map_err(Into::into)
        }
        claude_desktop::AGENT_ID => {
            let source = credential_route
                .as_ref()
                .map(|(source, _)| source)
                .ok_or_else(|| {
                    ModelTargetError::ConvergenceFailed(
                        "credential_missing: Claude requires a configured API Key source"
                            .to_string(),
                    )
                })?;
            let credential = credential::resolve_source(
                source,
                matches!(source, ApiKeySource::ArcaStore)
                    .then(|| read_credential_checked(&profile.id))
                    .transpose()?.flatten(),
            )?;
            claude_desktop::apply(
                &paths,
                &profile,
                Zeroizing::new(credential.expose_for_delivery().to_vec()),
            )
        }
        "codex" => apply_codex(&paths[0], &profile, route_has_credential).map_err(Into::into),
        "grok-build" if active => apply_grok_build(&paths[0], &profile).map_err(Into::into),
        "grok-build" => apply_grok_model(&paths[0], &profile).map_err(Into::into),
        "pi" => apply_pi(
            &paths[0],
            &paths[1],
            &profile,
            pi_api_key_value(&profile, route_has_credential)?,
            active,
        )
        .map_err(Into::into),
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" => {
            let prepared = adapters::prepare_apply(agent_id, &paths, &profile, active)?;
            match credential_route.as_ref() {
                Some((source, credential::PreparedCredentialRoute::OpenCodeAuthStore))
                    if matches!(agent_id, "opencode" | "opencode-desktop") =>
                {
                    let resolved = credential::resolve_source(
                        source,
                        matches!(source, ApiKeySource::ArcaStore)
                            .then(|| read_credential_checked(&profile.id))
                            .transpose()?.flatten(),
                    )?;
                    let auth_path = home().join(".local/share/opencode/auth.json");
                    let auth = open_code_auth::prepare_auth(
                        &auth_path,
                        &adapters::native_provider_id(agent_id, &profile),
                        resolved.expose_for_delivery(),
                    )?;
                    apply_native_multi_model_with_private_auth(
                        agent_id, &profile, prepared, auth, active,
                    )
                    .map_err(Into::into)
                }
                Some((source, credential::PreparedCredentialRoute::Plaintext)) => {
                    let resolved = credential::resolve_source(
                        source,
                        matches!(source, ApiKeySource::ArcaStore)
                            .then(|| read_credential_checked(&profile.id))
                            .transpose()?.flatten(),
                    )?;
                    let private_model = open_code_auth::PreparedAuthFile::from_model_file(
                        adapters::prepare_apply_plaintext(
                            agent_id,
                            &paths,
                            &profile,
                            active,
                            resolved.expose_for_delivery(),
                        )?,
                    );
                    let files = if matches!(agent_id, "opencode" | "opencode-desktop") {
                        let auth_path = home().join(".local/share/opencode/auth.json");
                        let auth_cleanup = open_code_auth::prepare_remove_auth(
                            &auth_path,
                            &adapters::native_provider_id(agent_id, &profile),
                        )?;
                        vec![private_model, auth_cleanup]
                    } else {
                        vec![private_model]
                    };
                    apply_native_multi_model_private(agent_id, &profile, files, active)
                        .map_err(Into::into)
                }
                _ => apply_native_multi_model(agent_id, &profile, prepared, active)
                    .map_err(Into::into),
            }
        }
        "qwen-code" | "crush" | "mistral-vibe" | "hermes" | "factory-droid" | "goose" => {
            apply_native_multi_model(
                agent_id,
                &profile,
                adapters::prepare_apply(agent_id, &paths, &profile, active)?,
                active,
            )
            .map_err(Into::into)
        }
        _ => unreachable!("ensure_supported filtered unknown agents"),
    }?;

    if active {
        mutate_settings(|settings| {
            settings
                .model_assignments
                .get_or_insert_with(BTreeMap::new)
                .insert(agent_id.into(), profile_id.into());
        })
        .map_err(|error| {
            let message = format!(
                "model config was applied, but Arca could not record the assignment: {error}"
            );
            if agent_id == claude_desktop::AGENT_ID {
                ModelTargetError::RecoveryRequired(message)
            } else {
                ModelTargetError::ConvergenceFailed(message)
            }
        })?;
    }
    Ok(result)
}

fn credential_route_for(
    agent_id: &str,
    profile: &ModelProfile,
    has_arca_credential: bool,
) -> Result<Option<(ApiKeySource, credential::PreparedCredentialRoute)>, String> {
    let settings = load_settings();
    let requested = settings.model_selection(agent_id).delivery_for(&profile.id);
    let delivery = credential::resolve_delivery(agent_id, &requested);
    let provider = profile.provider_id.as_deref().and_then(|provider_id| {
        settings
            .model_providers
            .as_ref()
            .and_then(|providers| providers.get(provider_id))
    });
    let requirement = provider
        .map(|provider| provider.auth_requirement.clone())
        .unwrap_or(AuthRequirement::Optional);
    let source = provider
        .and_then(|provider| provider.api_key_source.clone())
        .or_else(|| {
            profile
                .env_key
                .clone()
                .map(|name| ApiKeySource::Env { name })
        })
        .or_else(|| has_arca_credential.then_some(ApiKeySource::ArcaStore));
    let Some(source) = source else {
        return if requirement == AuthRequirement::Required {
            Err("credential_missing: this Provider requires an API Key before it can be added to the Agent".into())
        } else {
            Ok(None)
        };
    };
    if matches!(source, ApiKeySource::ArcaStore) && !has_arca_credential {
        return if requirement == AuthRequirement::Required {
            Err("credential_missing: Arca secure storage has no API Key for this Provider".into())
        } else {
            Ok(None)
        };
    }
    match credential::select_delivery(agent_id, &source, &delivery) {
        Ok(route) => Ok(Some((source, route))),
        Err(error) => {
            if let Some((code, message)) =
                profile_credential_issue(agent_id, profile, has_arca_credential, &delivery)
            {
                Err(format!("{code}: {message}"))
            } else {
                Err(error)
            }
        }
    }
}

/// Remove only the fields owned by a previously assigned Arca Profile, then
/// clear the desired assignment. The caller must identify the exact Profile so
/// unrelated providers and Agent policy remain untouched.
pub fn clear_profile(agent_id: &str, profile_id: &str) -> Result<(), String> {
    clear_profile_consumption(agent_id, profile_id, true)
}

#[cfg(test)]
pub(crate) fn clear_all_configured_models(agent_id: &str) -> Result<(), String> {
    clear_all_configured_models_for_targets(agent_id, &[])
}

pub(crate) fn clear_all_configured_models_for_targets(
    agent_id: &str,
    reviewed_targets: &[String],
) -> Result<(), String> {
    guard_shared_model_clear(&crate::settings::load_settings_strict().map_err(|error| error.to_string())?, agent_id)?;
    let authority = model_agent_capability(agent_id)
        .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?
        .storage_authority;
    if authority != ModelStorageAuthority::NativeRegistry {
        return Err(format!(
            "model Agent {agent_id} does not use a native registry"
        ));
    }
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    match agent_id {
        "pi" => clear_all_pi(&paths[0], &paths[1]),
        "grok-build" => clear_one_model_file(&paths[0], "grok-build", prepare_clear_all_grok_build),
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => {
            let reviewed = reviewed_targets
                .iter()
                .map(|path| expand_tilde(path))
                .collect::<Vec<_>>();
            let prepared = if reviewed.is_empty() {
                adapters::prepare_clear_all(agent_id, &paths)?
            } else {
                adapters::prepare_clear_all_for_targets(agent_id, &paths, Some(&reviewed))?
            };
            commit_native_model_files(agent_id, prepared)
        }
        _ => Err(format!(
            "unsupported native Model registry Agent: {agent_id}"
        )),
    }
}

pub(crate) fn agent_has_configured_models(agent_id: &str) -> Result<bool, String> {
    let authority = model_agent_capability(agent_id)
        .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?
        .storage_authority;
    if authority != ModelStorageAuthority::NativeRegistry {
        return Ok(false);
    }
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    match agent_id {
        "pi" => {
            let (models_original, models_content) = prepare_clear_all_pi_models(&paths[0])?;
            let (settings_original, settings_content) = prepare_clear_pi_settings(&paths[1])?;
            Ok(models_original
                .as_deref()
                .is_some_and(|value| value != models_content.as_str())
                || settings_original
                    .as_deref()
                    .is_some_and(|value| value != settings_content.as_str()))
        }
        "grok-build" => {
            let (original, content) = prepare_clear_all_grok_build(&paths[0])?;
            Ok(original
                .as_deref()
                .is_some_and(|value| value != content.as_str()))
        }
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => adapters::has_configured_models(agent_id, &paths),
        _ => Ok(false),
    }
}

/// A reviewed clear-all includes external records, so shared model registries
/// cannot be emptied while the sibling Agent still consumes a managed model.
fn guard_shared_model_clear(settings: &crate::settings::Settings, agent_id: &str) -> Result<(), String> {
    let sibling = match agent_id {
        "qoder-cli" => "qoder-desktop",
        "qoder-desktop" => "qoder-cli",
        "opencode" => "opencode-desktop",
        "opencode-desktop" => "opencode",
        "kilo-code" => "kilo-desktop",
        "kilo-desktop" => "kilo-code",
        _ => return Ok(()),
    };
    if settings.model_selection(sibling).profiles.is_empty() { return Ok(()); }
    let targets = configured_path_strings_checked(settings, agent_id)?.unwrap_or_default();
    let sibling_targets = configured_path_strings_checked(settings, sibling)?.unwrap_or_default();
    if targets.iter().any(|path| sibling_targets.iter().any(|other| expand_tilde(path) == expand_tilde(other))) {
        let code = if agent_id.starts_with("qoder-") { "qoder_shared_registry" }
            else if agent_id.starts_with("kilo-") { "kilo_shared_registry" }
            else { "opencode_shared_registry" };
        return Err(format!("{code}: {sibling} also uses this model config; remove individual models instead of clearing the shared registry"));
    }
    Ok(())
}

pub(crate) fn clear_all_configured_model_paths(agent_id: &str) -> Result<Vec<String>, String> {
    let settings = crate::settings::load_settings_strict().map_err(|error| error.to_string())?;
    guard_shared_model_clear(&settings, agent_id)?;
    let paths = configured_path_strings_checked(&settings, agent_id)?
        .ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    if agent_id != "goose" {
        return Ok(paths);
    }
    let expanded = paths
        .iter()
        .map(|path| expand_tilde(path))
        .collect::<Vec<_>>();
    let mut targets = adapters::prepare_clear_all(agent_id, &expanded)?
        .into_iter()
        .map(|file| collapse_home(file.path.to_string_lossy().as_ref()))
        .collect::<Vec<_>>();
    targets.sort();
    targets.dedup();
    Ok(targets)
}

pub(crate) fn clear_profile_consumption(
    agent_id: &str,
    profile_id: &str,
    active: bool,
) -> Result<(), String> {
    clear_profile_consumption_target(agent_id, profile_id, active)
        .map_err(ModelTargetError::into_message)
}

pub(crate) fn clear_profile_consumption_target(
    agent_id: &str,
    profile_id: &str,
    active: bool,
) -> Result<(), ModelTargetError> {
    let _settings_guard = mutation_lock()?;
    let profile = profile_for_apply(profile_id)?;
    ensure_supported(agent_id, &profile.protocol)?;
    let profile = materialize_profile_for_agent(agent_id, &profile)?;
    let paths =
        configured_paths(agent_id).ok_or_else(|| format!("unsupported model Agent: {agent_id}"))?;
    let observed = if agent_id == claude_desktop::AGENT_ID {
        claude_desktop::observe_installed(&paths, &profile)?
    } else {
        observe_profile_consumption(agent_id, &profile, false)?
    };
    match observed {
        ModelObservedState::Synced => {}
        ModelObservedState::Missing => {}
        ModelObservedState::Drifted => {
            return Err(ModelTargetError::ConvergenceFailed(
                "model_owned_fields_drift: review the Agent config before clearing".into(),
            ))
        }
        ModelObservedState::Conflicted => {
            return Err(ModelTargetError::ConvergenceFailed(
                "model_target_conflicted: the Agent config is ambiguous".into(),
            ))
        }
    }
    match agent_id {
        "claude-code" => clear_one_model_file(&paths[0], "claude-code", prepare_clear_claude)
            .map_err(ModelTargetError::from)?,
        claude_desktop::AGENT_ID => {
            let settings = load_settings();
            let remembered = settings
                .model_agent_runtime_state(claude_desktop::AGENT_ID)
                .and_then(|state| state.previous_applied_profile_id.as_deref());
            claude_desktop::clear(&paths, remembered)?;
        }
        "codex" => {
            clear_one_model_file_with_profile(&paths[0], "codex", &profile, prepare_clear_codex)
                .map_err(ModelTargetError::from)?
        }
        "grok-build" => clear_one_model_file_with_profile(
            &paths[0],
            "grok-build",
            &profile,
            prepare_clear_grok_build,
        )
        .map_err(ModelTargetError::from)?,
        "pi" => clear_pi(&paths[0], &paths[1], &profile, active).map_err(ModelTargetError::from)?,
        "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop" | "qwen-code" | "crush" | "mistral-vibe" | "hermes"
        | "factory-droid" | "goose" => {
            commit_native_model_files(
                agent_id,
                adapters::prepare_clear(agent_id, &paths, &profile)?,
            )
            .map_err(ModelTargetError::from)?;
        }
        _ => unreachable!("ensure_supported filtered unsupported model Agent"),
    }
    mutate_settings(|settings| {
        if let Some(assignments) = settings.model_assignments.as_mut() {
            if assignments.get(agent_id).is_some_and(|id| id == profile_id) {
                assignments.remove(agent_id);
            }
        }
    })
    .map_err(|error| {
        let message = error.to_string();
        if agent_id == claude_desktop::AGENT_ID {
            ModelTargetError::RecoveryRequired(message)
        } else {
            ModelTargetError::ConvergenceFailed(message)
        }
    })
}

fn apply_native_multi_model(
    agent_id: &str,
    profile: &ModelProfile,
    files: Vec<adapters::PreparedModelFile>,
    active: bool,
) -> Result<ModelApplyResult, String> {
    let written = files
        .iter()
        .map(|file| file.path.display().to_string())
        .collect();
    commit_native_model_files(agent_id, files)?;
    Ok(ModelApplyResult {
        agent: agent_id.into(),
        profile: profile.id.clone(),
        files: written,
        restart_required: true,
        message: if active {
            format!("{agent_id} Model Profile and primary selection updated.")
        } else {
            format!("{agent_id} backup Model Profile installed without changing the current Model.")
        },
    })
}

fn apply_native_multi_model_with_private_auth(
    agent_id: &str,
    profile: &ModelProfile,
    files: Vec<adapters::PreparedModelFile>,
    auth: open_code_auth::PreparedAuthFile,
    active: bool,
) -> Result<ModelApplyResult, String> {
    let mut written = files
        .iter()
        .map(|file| file.path.display().to_string())
        .collect::<Vec<_>>();
    commit_native_model_files(agent_id, files.clone())?;
    let auth_write = auth
        .content
        .as_deref()
        .ok_or_else(|| {
            "agent_store_conflicted: OpenCode auth candidate is unexpectedly empty".to_string()
        })
        .and_then(|content| {
            write_if_unchanged(
                &auth.path,
                auth.original.as_deref().map(String::as_str),
                content.as_str(),
            )
        });
    if let Err(error) = auth_write {
        let rollback = rollback_native_model_files(&files);
        return Err(if rollback.is_empty() {
            format!("agent_store_conflicted: OpenCode auth write failed and model config was rolled back: {error}")
        } else {
            format!(
                "agent_store_conflicted: OpenCode auth write failed ({error}); model rollback failed: {}",
                rollback.join("; ")
            )
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) = fs::set_permissions(&auth.path, fs::Permissions::from_mode(0o600)) {
            let auth_rollback = match (auth.original.as_deref(), auth.content.as_deref()) {
                (Some(original), Some(content)) => {
                    write_if_unchanged(&auth.path, Some(content.as_str()), original.as_str())
                }
                (None, Some(content)) => remove_if_unchanged(&auth.path, content.as_str()),
                _ => Ok(()),
            };
            let mut rollback = rollback_native_model_files(&files);
            if let Err(error) = auth_rollback {
                rollback.push(error);
            }
            return Err(format!(
                "plaintext_target_insecure: could not enforce 0600 on OpenCode auth.json ({error}); rollback: {}",
                if rollback.is_empty() { "complete".into() } else { rollback.join("; ") }
            ));
        }
    }
    written.push(auth.path.display().to_string());
    Ok(ModelApplyResult {
        agent: agent_id.into(),
        profile: profile.id.clone(),
        files: written,
        restart_required: true,
        message: if active {
            format!(
                "{agent_id} Model Profile, primary selection, and Agent credential store updated."
            )
        } else {
            format!("{agent_id} backup Model Profile and Agent credential store updated.")
        },
    })
}

fn apply_native_multi_model_private(
    agent_id: &str,
    profile: &ModelProfile,
    files: Vec<open_code_auth::PreparedAuthFile>,
    active: bool,
) -> Result<ModelApplyResult, String> {
    commit_private_model_files(&files)?;
    Ok(ModelApplyResult {
        agent: agent_id.into(),
        profile: profile.id.clone(),
        files: files
            .iter()
            .map(|file| file.path.display().to_string())
            .collect(),
        restart_required: true,
        message: if active {
            format!("{agent_id} private plaintext Model configuration updated.")
        } else {
            format!("{agent_id} private plaintext backup Model configuration updated.")
        },
    })
}

fn commit_private_model_files(files: &[open_code_auth::PreparedAuthFile]) -> Result<(), String> {
    let mut applied = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let result = match (file.original.as_deref(), file.content.as_deref()) {
            (original, Some(content)) => {
                write_if_unchanged(&file.path, original.map(String::as_str), content.as_str())
            }
            (Some(original), None) => remove_if_unchanged(&file.path, original.as_str()),
            (None, None) => Ok(()),
        };
        if let Err(error) = result {
            let rollback = rollback_private_model_files(files, &applied);
            return Err(format!(
                "plaintext_target_insecure: private Agent write failed ({error}); rollback: {}",
                if rollback.is_empty() {
                    "complete".into()
                } else {
                    rollback.join("; ")
                }
            ));
        }
        #[cfg(unix)]
        if file.content.is_some() {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) = fs::set_permissions(&file.path, fs::Permissions::from_mode(0o600)) {
                let mut rollback_indexes = applied.clone();
                rollback_indexes.push(index);
                let rollback = rollback_private_model_files(files, &rollback_indexes);
                return Err(format!(
                    "plaintext_target_insecure: could not enforce 0600 ({error}); rollback: {}",
                    if rollback.is_empty() {
                        "complete".into()
                    } else {
                        rollback.join("; ")
                    }
                ));
            }
        }
        applied.push(index);
    }
    Ok(())
}

fn rollback_private_model_files(
    files: &[open_code_auth::PreparedAuthFile],
    applied: &[usize],
) -> Vec<String> {
    let mut errors = Vec::new();
    for index in applied.iter().rev() {
        let file = &files[*index];
        let rollback = match (file.original.as_deref(), file.content.as_deref()) {
            (Some(original), Some(content)) => {
                write_if_unchanged(&file.path, Some(content.as_str()), original.as_str())
            }
            (None, Some(content)) => remove_if_unchanged(&file.path, content.as_str()),
            (Some(original), None) => write_if_unchanged(&file.path, None, original.as_str()),
            (None, None) => Ok(()),
        };
        if let Err(error) = rollback {
            errors.push(error);
        }
    }
    errors
}

fn rollback_native_model_files(files: &[adapters::PreparedModelFile]) -> Vec<String> {
    let mut errors = Vec::new();
    for file in files.iter().rev() {
        let rollback = match (&file.original, &file.content) {
            (Some(original), Some(content)) => {
                write_if_unchanged(&file.path, Some(content), original)
            }
            (None, Some(content)) => remove_if_unchanged(&file.path, content),
            (Some(original), None) => write_if_unchanged(&file.path, None, original),
            (None, None) => Ok(()),
        };
        if let Err(error) = rollback {
            errors.push(error);
        }
    }
    errors
}

fn commit_native_model_files(
    agent_id: &str,
    files: Vec<adapters::PreparedModelFile>,
) -> Result<(), String> {
    if matches!(agent_id, "qoder-desktop" | "qoder-cli" | "zcode" | "opencode" | "opencode-desktop" | "kilo-desktop") {
        return commit_private_model_files(
            &files.into_iter().map(open_code_auth::PreparedAuthFile::from_model_file).collect::<Vec<_>>()
        );
    }
    let stamp = backup_timestamp();
    for file in &files {
        backup_config(&file.path, agent_id, &stamp)?;
    }
    let mut applied: Vec<adapters::PreparedModelFile> = Vec::new();
    for file in &files {
        let result = match (&file.original, &file.content) {
            (original, Some(content)) => {
                write_if_unchanged(&file.path, original.as_deref(), content)
            }
            (Some(original), None) => remove_if_unchanged(&file.path, original),
            (None, None) => Ok(()),
        };
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for previous in applied.into_iter().rev() {
                let rollback = match (&previous.original, &previous.content) {
                    (Some(original), Some(content)) => {
                        write_if_unchanged(&previous.path, Some(content), original)
                    }
                    (None, Some(content)) => remove_if_unchanged(&previous.path, content),
                    (Some(original), None) => write_if_unchanged(&previous.path, None, original),
                    (None, None) => Ok(()),
                };
                if let Err(rollback) = rollback {
                    rollback_errors.push(rollback);
                }
            }
            return if rollback_errors.is_empty() {
                Err(format!(
                    "Model config update failed and was rolled back: {error}"
                ))
            } else {
                Err(format!(
                    "Model config update failed ({error}); rollback failed: {}",
                    rollback_errors.join("; ")
                ))
            };
        }
        applied.push(file.clone());
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("failed to read {}: {}", path.display(), error)),
    }
}

fn input_value(value: Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value),
        Value::Array(values) => CstInputValue::Array(values.into_iter().map(input_value).collect()),
        Value::Object(values) => CstInputValue::Object(
            values
                .into_iter()
                .map(|(name, value)| (name, input_value(value)))
                .collect(),
        ),
    }
}

fn property_count(object: &CstObject, name: &str) -> usize {
    object
        .properties()
        .into_iter()
        .filter(|property| {
            property
                .name()
                .and_then(|name| name.decoded_value().ok())
                .is_some_and(|decoded| decoded == name)
        })
        .count()
}

fn ensure_unique(object: &CstObject, name: &str, path: &Path, context: &str) -> Result<(), String> {
    if property_count(object, name) > 1 {
        return Err(format!(
            "refusing to modify {}: duplicate JSON key '{}.{}' is ambiguous",
            path.display(),
            context,
            name
        ));
    }
    Ok(())
}

fn ensure_unique_keys(object: &CstObject, path: &Path, context: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for property in object.properties() {
        let Some(name) = property.name().and_then(|name| name.decoded_value().ok()) else {
            continue;
        };
        if !seen.insert(name.clone()) {
            return Err(format!(
                "refusing to modify {}: duplicate JSON key '{}.{}' is ambiguous",
                path.display(),
                context,
                name
            ));
        }
    }
    Ok(())
}

fn set_json_property(
    object: &CstObject,
    name: &str,
    value: Option<Value>,
    path: &Path,
    context: &str,
) -> Result<(), String> {
    ensure_unique(object, name, path, context)?;
    match (object.get(name), value) {
        (Some(property), Some(value)) => property.set_value(input_value(value)),
        (None, Some(value)) => {
            object.append(name, input_value(value));
        }
        (Some(property), None) => property.remove(),
        (None, None) => {}
    }
    Ok(())
}

fn read_jsonc(path: &Path) -> Result<(CstRootNode, Option<String>), String> {
    let original = read_optional(path)?;
    let root = CstRootNode::parse(
        original.as_deref().unwrap_or_default(),
        &ParseOptions::default(),
    )
    .map_err(|error| {
        format!(
            "refusing to modify invalid JSON/JSONC at {}: {}",
            path.display(),
            error
        )
    })?;
    Ok((root, original))
}

fn json_root_object(root: &CstRootNode, path: &Path) -> Result<CstObject, String> {
    root.object_value_or_create().ok_or_else(|| {
        format!(
            "refusing to modify {}: JSON root is not an object",
            path.display()
        )
    })
}

fn truthy_json_value(node: Option<CstNode>) -> bool {
    node.and_then(|node| node.to_serde_value())
        .is_some_and(|value| match value {
            Value::Bool(value) => value,
            Value::String(value) => {
                let normalized = value.trim().to_ascii_lowercase();
                !normalized.is_empty() && normalized != "0" && normalized != "false"
            }
            Value::Null => false,
            _ => true,
        })
}

fn prepare_claude(
    path: &Path,
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    ensure_unique(&object, "env", path, "$root")?;

    let env = object.object_value_or_create("env").ok_or_else(|| {
        format!(
            "refusing to modify {}: 'env' is not an object",
            path.display()
        )
    })?;
    ensure_unique_keys(&env, path, "env")?;
    for field in [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ] {
        if truthy_json_value(env.get(field).and_then(|property| property.value())) {
            return Err(format!(
                "refusing to replace active {} routing in {}",
                field,
                path.display()
            ));
        }
    }

    set_json_property(
        &object,
        "model",
        Some(Value::String(profile.model.clone())),
        path,
        "$root",
    )?;
    set_json_property(
        &object,
        "apiKeyHelper",
        has_credential.then(|| Value::String(profile_credential_shell_command(profile))),
        path,
        "$root",
    )?;
    set_json_property(
        &env,
        "ANTHROPIC_BASE_URL",
        Some(Value::String(profile.base_url.clone())),
        path,
        "env",
    )?;
    // These have higher precedence than apiKeyHelper. Applying a Arca profile is
    // an explicit connection takeover, so remove only these conflicting fields.
    set_json_property(&env, "ANTHROPIC_AUTH_TOKEN", None, path, "env")?;
    set_json_property(&env, "ANTHROPIC_API_KEY", None, path, "env")?;
    Ok((original, root.to_string()))
}

fn prepare_clear_claude(path: &Path) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    set_json_property(&object, "model", None, path, "$root")?;
    set_json_property(&object, "apiKeyHelper", None, path, "$root")?;
    if let Some(env) = object.object_value("env") {
        ensure_unique_keys(&env, path, "env")?;
        set_json_property(&env, "ANTHROPIC_BASE_URL", None, path, "env")?;
    }
    Ok((original, root.to_string()))
}

fn read_toml(path: &Path) -> Result<(Document, Option<String>), String> {
    let original = read_optional(path)?;
    let document = original
        .as_deref()
        .unwrap_or_default()
        .parse::<Document>()
        .map_err(|error| {
            format!(
                "refusing to modify invalid TOML at {}: {}",
                path.display(),
                error
            )
        })?;
    Ok((document, original))
}

fn prepare_codex(
    path: &Path,
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    document["model"] = toml_edit::value(&profile.model);
    let provider_id = codex_provider_id(profile);
    document["model_provider"] = toml_edit::value(&provider_id);

    if !document.as_table().contains_key("model_providers") {
        document
            .as_table_mut()
            .insert("model_providers", Item::Table(Table::new()));
    }
    let providers = document
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            format!(
                "refusing to modify {}: 'model_providers' is not a TOML table",
                path.display()
            )
        })?;
    if !providers.contains_key(&provider_id) {
        providers.insert(&provider_id, Item::Table(Table::new()));
    }
    let provider = providers
        .get_mut(&provider_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("Arca provider '{provider_id}' is not a TOML table"))?;
    provider.insert("name", toml_edit::value(&profile.name));
    provider.insert("base_url", toml_edit::value(&profile.base_url));
    provider.insert("wire_api", toml_edit::value("responses"));
    provider.remove("env_key");
    provider.remove("experimental_bearer_token");
    provider.remove("requires_openai_auth");

    if has_credential {
        let mut auth = Table::new();
        let command = profile_credential_command(profile);
        auth.insert("command", toml_edit::value(&command[0]));
        let mut args = Array::new();
        for argument in &command[1..] {
            args.push(argument.as_str());
        }
        auth.insert("args", toml_edit::value(args));
        provider.insert("auth", Item::Table(auth));
    } else {
        provider.remove("auth");
    }
    Ok((original, document.to_string()))
}

fn prepare_clear_codex(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    document.remove("model");
    document.remove("model_provider");
    if let Some(providers) = document
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
    {
        providers.remove(&codex_provider_id(profile));
    }
    Ok((original, document.to_string()))
}

fn native_profile_id(profile: &ModelProfile, agent_id: &str, fallback: String) -> String {
    profile
        .native_ids
        .get(agent_id)
        .cloned()
        .unwrap_or(fallback)
}

fn codex_provider_id(profile: &ModelProfile) -> String {
    native_profile_id(profile, "codex", arca_profile_id(&profile.id))
}

fn arca_profile_id(profile_id: &str) -> String {
    let mut encoded = String::from("mux_");
    for byte in profile_id.bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn grok_model_id(profile: &ModelProfile) -> String {
    native_profile_id(profile, "grok-build", arca_profile_id(&profile.id))
}

fn pi_provider_id(profile: &ModelProfile) -> String {
    native_profile_id(profile, "pi", format!("mux-{}", profile.id))
}

fn pi_provider_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.extend(character.to_lowercase());
            pending_separator = false;
        } else {
            pending_separator = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn pi_provider_config_slug(provider: &ModelProviderConfig) -> String {
    [
        provider.name.as_str(),
        provider.provider.as_str(),
        provider.id.as_str(),
    ]
    .into_iter()
    .map(pi_provider_slug)
    .find(|value| !value.is_empty())
    .unwrap_or_else(|| "provider".into())
}

pub(crate) fn generated_pi_provider_id(
    settings: &crate::settings::Settings,
    profile: &ModelProfile,
) -> String {
    let Some(provider_id) = profile.provider_id.as_deref() else {
        return format!("mux-{}", profile.id);
    };
    let Some(providers) = settings.model_providers.as_ref() else {
        return format!("mux-{}", profile.id);
    };
    let Some(provider) = providers.get(provider_id) else {
        return format!("mux-{}", profile.id);
    };
    let base = pi_provider_config_slug(provider);
    let collides = providers.iter().any(|(candidate_id, candidate)| {
        candidate_id != provider_id && pi_provider_config_slug(candidate) == base
    });
    if !collides {
        return base;
    }
    let suffix = pi_provider_slug(&provider.id);
    if suffix.is_empty() {
        format!("{base}-provider")
    } else {
        format!("{base}-{suffix}")
    }
}

pub(crate) fn generated_open_code_provider_id(
    settings: &crate::settings::Settings,
    profile: &ModelProfile,
) -> String {
    let Some(provider) = profile.provider_id.as_ref()
        .and_then(|id| settings.model_providers.as_ref()?.get(id)) else {
        return arca_profile_id(&profile.id);
    };
    let mut id = generated_pi_provider_id(settings, profile);
    if !id.is_ascii() {
        id = pi_provider_slug(&provider.id);
    }
    // OpenCode has one SDK/base URL per provider. Different protocols cannot
    // share that object even when they originate from the same Arca Provider.
    if settings.model_profiles.as_ref().is_some_and(|profiles| profiles.values().any(|other| {
        other.provider_id == profile.provider_id && other.protocol != profile.protocol
    })) {
        let protocol = match profile.protocol {
            ModelProtocol::AnthropicMessages => "anthropic",
            ModelProtocol::OpenaiResponses => "responses",
            ModelProtocol::OpenaiCompletions => "openai",
            ModelProtocol::GeminiGenerateContent => "gemini",
        };
        id = format!("{id}-{protocol}");
    }
    if id.is_empty() || id.len() > 128 || !id.is_ascii() {
        return arca_profile_id(&profile.id);
    }
    id
}

fn grok_api_backend(protocol: &ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::AnthropicMessages => "messages",
        ModelProtocol::OpenaiResponses => "responses",
        ModelProtocol::OpenaiCompletions => "chat_completions",
        ModelProtocol::GeminiGenerateContent => "gemini",
    }
}

fn insert_optional_toml_integer(
    table: &mut Table,
    name: &str,
    value: Option<u64>,
) -> Result<(), String> {
    match value {
        Some(value) => {
            let value = i64::try_from(value)
                .map_err(|_| format!("{name} exceeds the TOML integer range"))?;
            table.insert(name, toml_edit::value(value));
        }
        None => {
            table.remove(name);
        }
    }
    Ok(())
}

fn prepare_grok_build(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    let model_id = grok_model_id(profile);

    if !document.as_table().contains_key("models") {
        document
            .as_table_mut()
            .insert("models", Item::Table(Table::new()));
    }
    let defaults = document
        .get_mut("models")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            format!(
                "refusing to modify {}: 'models' is not a TOML table",
                path.display()
            )
        })?;
    defaults.insert("default", toml_edit::value(&model_id));

    upsert_grok_model(&mut document, path, profile)?;
    Ok((original, document.to_string()))
}

fn prepare_grok_model(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    upsert_grok_model(&mut document, path, profile)?;
    Ok((original, document.to_string()))
}

fn upsert_grok_model(
    document: &mut Document,
    path: &Path,
    profile: &ModelProfile,
) -> Result<(), String> {
    let model_id = grok_model_id(profile);
    if !document.as_table().contains_key("model") {
        document
            .as_table_mut()
            .insert("model", Item::Table(Table::new()));
    }
    let models = document
        .get_mut("model")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            format!(
                "refusing to modify {}: 'model' is not a TOML table",
                path.display()
            )
        })?;
    if !models.contains_key(&model_id) {
        models.insert(&model_id, Item::Table(Table::new()));
    }
    let model = models
        .get_mut(&model_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("Arca model '{model_id}' is not a TOML table"))?;
    model.insert("model", toml_edit::value(&profile.model));
    model.insert("base_url", toml_edit::value(&profile.base_url));
    model.insert("name", toml_edit::value(&profile.name));
    model.insert(
        "api_backend",
        toml_edit::value(grok_api_backend(&profile.protocol)),
    );
    insert_optional_toml_integer(model, "context_window", profile.context_window)?;
    insert_optional_toml_integer(model, "max_completion_tokens", profile.max_output_tokens)?;
    model.remove("api_key");
    match profile.env_key.as_deref() {
        Some(env_key) => {
            model.insert("env_key", toml_edit::value(env_key));
        }
        None => {
            model.remove("env_key");
        }
    }
    Ok(())
}

fn prepare_clear_grok_build(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    let model_id = grok_model_id(profile);
    if let Some(defaults) = document.get_mut("models").and_then(Item::as_table_mut) {
        if defaults.get("default").and_then(Item::as_str) == Some(model_id.as_str()) {
            defaults.remove("default");
        }
    }
    if let Some(ui) = document.get_mut("ui").and_then(Item::as_table_mut) {
        if ui.get("fork_secondary_model").and_then(Item::as_str) == Some(model_id.as_str()) {
            ui.remove("fork_secondary_model");
        }
    }
    if let Some(models) = document.get_mut("model").and_then(Item::as_table_mut) {
        models.remove(&model_id);
    }
    Ok((original, document.to_string()))
}

fn prepare_clear_all_grok_build(path: &Path) -> Result<(Option<String>, String), String> {
    let (mut document, original) = read_toml(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    if document.get("models").is_some_and(|item| !item.is_table()) {
        return Err(format!(
            "refusing to modify {}: 'models' is not a TOML table",
            path.display()
        ));
    }
    if document.get("model").is_some_and(|item| !item.is_table()) {
        return Err(format!(
            "refusing to modify {}: 'model' is not a TOML table",
            path.display()
        ));
    }
    if let Some(defaults) = document.get_mut("models").and_then(Item::as_table_mut) {
        defaults.remove("default");
    }
    document.remove("model");
    Ok((original, document.to_string()))
}

fn pi_model_value(profile: &ModelProfile, existing: Option<Value>) -> Result<Value, String> {
    let mut model = match existing {
        Some(Value::Object(model)) => model,
        Some(_) => return Err("existing Pi model entry is not an object".into()),
        None => serde_json::Map::new(),
    };
    model.insert("id".into(), Value::String(profile.model.clone()));
    model.insert("name".into(), Value::String(profile.name.clone()));
    model.insert(
        "input".into(),
        Value::Array(vec![Value::String("text".into())]),
    );
    if let Some(value) = profile.reasoning {
        model.insert("reasoning".into(), Value::Bool(value));
    } else {
        model.remove("reasoning");
    }
    if let Some(value) = profile.context_window {
        model.insert("contextWindow".into(), Value::Number(value.into()));
    } else {
        model.remove("contextWindow");
    }
    if let Some(value) = profile.max_output_tokens {
        model.insert("maxTokens".into(), Value::Number(value.into()));
    } else {
        model.remove("maxTokens");
    }
    Ok(Value::Object(model))
}

fn set_pi_model_fields(
    model: &CstObject,
    profile: &ModelProfile,
    path: &Path,
    context: &str,
) -> Result<(), String> {
    ensure_unique_keys(model, path, context)?;
    set_json_property(
        model,
        "id",
        Some(Value::String(profile.model.clone())),
        path,
        context,
    )?;
    set_json_property(
        model,
        "name",
        Some(Value::String(profile.name.clone())),
        path,
        context,
    )?;
    set_json_property(
        model,
        "input",
        Some(Value::Array(vec![Value::String("text".into())])),
        path,
        context,
    )?;
    set_json_property(
        model,
        "reasoning",
        profile.reasoning.map(Value::Bool),
        path,
        context,
    )?;
    set_json_property(
        model,
        "contextWindow",
        profile
            .context_window
            .map(|value| Value::Number(value.into())),
        path,
        context,
    )?;
    set_json_property(
        model,
        "maxTokens",
        profile
            .max_output_tokens
            .map(|value| Value::Number(value.into())),
        path,
        context,
    )
}

fn upsert_pi_provider(
    providers: &CstObject,
    provider_id: &str,
    profile: &ModelProfile,
    api_key: Option<String>,
    path: &Path,
) -> Result<(), String> {
    ensure_unique(providers, provider_id, path, "providers")?;
    if providers.get(provider_id).is_none() {
        providers.append(provider_id, CstInputValue::Object(Vec::new()));
    }
    let provider = providers.object_value(provider_id).ok_or_else(|| {
        format!(
            "refusing to modify {}: Pi provider '{}' is not an object",
            path.display(),
            provider_id
        )
    })?;
    let provider_context = format!("providers.{provider_id}");
    ensure_unique_keys(&provider, path, &provider_context)?;
    ensure_unique(&provider, "models", path, &provider_context)?;
    let models = provider.array_value_or_create("models").ok_or_else(|| {
        format!(
            "refusing to modify {}: '{}.models' is not an array",
            path.display(),
            provider_context
        )
    })?;
    let elements = models.elements();
    let mut matching = Vec::new();
    let mut sibling_count = 0usize;
    for element in &elements {
        if element
            .to_serde_value()
            .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
            .as_deref()
            == Some(profile.model.as_str())
        {
            matching.push(element.clone());
        } else {
            sibling_count += 1;
        }
    }
    if matching.len() > 1 {
        return Err(format!(
            "refusing to modify {}: Pi provider '{}' contains duplicate model id '{}'",
            path.display(),
            provider_id,
            profile.model
        ));
    }
    let expected_api = protocol_name(&profile.protocol);
    if sibling_count > 0
        && (json_property_string(&provider, "baseUrl")
            .is_some_and(|value| value != profile.base_url.as_str())
            || json_property_string(&provider, "api").is_some_and(|value| value != expected_api))
    {
        return Err(format!(
            "refusing to modify {}: Pi provider '{}' has sibling models with a different endpoint or protocol",
            path.display(),
            provider_id
        ));
    }
    set_json_property(
        &provider,
        "baseUrl",
        Some(Value::String(profile.base_url.clone())),
        path,
        &provider_context,
    )?;
    set_json_property(
        &provider,
        "api",
        Some(Value::String(expected_api.into())),
        path,
        &provider_context,
    )?;
    set_json_property(
        &provider,
        "apiKey",
        api_key.map(Value::String),
        path,
        &provider_context,
    )?;
    if let Some(element) = matching.pop() {
        let model = element.as_object().ok_or_else(|| {
            format!(
                "refusing to modify {}: Pi model '{}' is not an object",
                path.display(),
                profile.model
            )
        })?;
        let model_context = format!("{provider_context}.models.{}", profile.model);
        set_pi_model_fields(&model, profile, path, &model_context)?;
    } else {
        models.append(input_value(pi_model_value(profile, None)?));
    }
    Ok(())
}

fn json_property_string(object: &CstObject, name: &str) -> Option<String> {
    object
        .get(name)
        .and_then(|property| property.value())
        .and_then(|value| value.to_serde_value())
        .and_then(|value| value.as_str().map(str::to_string))
}

fn clear_pi_profile_from_provider(
    providers: &CstObject,
    provider_id: &str,
    profile: &ModelProfile,
    path: &Path,
) -> Result<(), String> {
    ensure_unique(providers, provider_id, path, "providers")?;
    let Some(property) = providers.get(provider_id) else {
        return Ok(());
    };
    let provider = property.object_value().ok_or_else(|| {
        format!(
            "refusing to modify {}: Pi provider '{}' is not an object",
            path.display(),
            provider_id
        )
    })?;
    let provider_context = format!("providers.{provider_id}");
    ensure_unique_keys(&provider, path, &provider_context)?;
    ensure_unique(&provider, "models", path, &provider_context)?;
    let Some(models_property) = provider.get("models") else {
        return Ok(());
    };
    let models = models_property.array_value().ok_or_else(|| {
        format!(
            "refusing to modify {}: '{}.models' is not an array",
            path.display(),
            provider_context
        )
    })?;
    let elements = models.elements();
    let matching = elements
        .iter()
        .filter(|element| {
            element
                .to_serde_value()
                .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
                .as_deref()
                == Some(profile.model.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        return Err(format!(
            "refusing to modify {}: Pi provider '{}' contains duplicate model id '{}'",
            path.display(),
            provider_id,
            profile.model
        ));
    }
    let Some(model) = matching.into_iter().next() else {
        return Ok(());
    };
    if elements.len() == 1 {
        property.remove();
    } else {
        model.remove();
    }
    Ok(())
}

fn clear_legacy_pi_profile_provider(
    providers: &CstObject,
    provider_id: &str,
    profile: &ModelProfile,
    path: &Path,
) -> Result<(), String> {
    let legacy_id = format!("mux-{}", profile.id);
    if legacy_id == provider_id {
        return Ok(());
    }
    clear_pi_profile_from_provider(providers, &legacy_id, profile, path)
}

#[cfg(test)]
fn prepare_pi_models(
    path: &Path,
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<(Option<String>, String), String> {
    prepare_pi_models_with_api_key(
        path,
        profile,
        has_credential.then(|| format!("!{}", profile_credential_shell_command(profile))),
    )
}

fn prepare_pi_models_with_api_key(
    path: &Path,
    profile: &ModelProfile,
    api_key: Option<String>,
) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    ensure_unique(&object, "providers", path, "$root")?;
    let providers = object.object_value_or_create("providers").ok_or_else(|| {
        format!(
            "refusing to modify {}: 'providers' is not an object",
            path.display()
        )
    })?;
    ensure_unique_keys(&providers, path, "providers")?;
    let provider_id = pi_provider_id(profile);
    upsert_pi_provider(&providers, &provider_id, profile, api_key, path)?;
    clear_legacy_pi_profile_provider(&providers, &provider_id, profile, path)?;
    Ok((original, root.to_string()))
}

fn pi_api_key_value(
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<Option<String>, String> {
    match credential_route_for("pi", profile, has_credential)? {
        Some((source, credential::PreparedCredentialRoute::Plaintext)) => {
            let resolved = credential::resolve_source(
                &source,
                matches!(source, ApiKeySource::ArcaStore)
                    .then(|| read_credential_checked(&profile.id))
                    .transpose()?.flatten(),
            )?;
            String::from_utf8(resolved.expose_for_delivery().to_vec())
                .map(Some)
                .map_err(|_| "plaintext_target_insecure: API Key must be UTF-8".to_string())
        }
        _ if has_credential => Ok(Some(format!(
            "!{}",
            profile_credential_shell_command(profile)
        ))),
        _ => Ok(None),
    }
}

fn prepare_observed_native_files(
    agent_id: &str,
    paths: &[PathBuf],
    profile: &ModelProfile,
    active: bool,
    has_credential: bool,
) -> Result<Vec<adapters::PreparedModelFile>, String> {
    if matches!(agent_id, "zcode" | "qoder-desktop" | "qoder-cli" | "opencode" | "opencode-desktop" | "kilo-code" | "kilo-desktop") {
        if let Some((source, credential::PreparedCredentialRoute::Plaintext)) =
            credential_route_for(agent_id, profile, has_credential)?
        {
            let resolved = credential::resolve_source(
                &source,
                matches!(source, ApiKeySource::ArcaStore)
                    .then(|| read_credential_checked(&profile.id))
                    .transpose()?.flatten(),
            )?;
            return Ok(vec![adapters::prepare_apply_plaintext(
                agent_id,
                paths,
                profile,
                active,
                resolved.expose_for_delivery(),
            )?]);
        }
    }
    adapters::prepare_apply(agent_id, paths, profile, active)
}

fn prepare_pi_settings(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    set_json_property(
        &object,
        "defaultProvider",
        Some(Value::String(pi_provider_id(profile))),
        path,
        "$root",
    )?;
    set_json_property(
        &object,
        "defaultModel",
        Some(Value::String(profile.model.clone())),
        path,
        "$root",
    )?;
    Ok((original, root.to_string()))
}

fn prepare_clear_pi_models(
    path: &Path,
    profile: &ModelProfile,
) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    if let Some(providers) = object.object_value("providers") {
        ensure_unique_keys(&providers, path, "providers")?;
        let provider_id = pi_provider_id(profile);
        clear_pi_profile_from_provider(&providers, &provider_id, profile, path)?;
        clear_legacy_pi_profile_provider(&providers, &provider_id, profile, path)?;
    }
    Ok((original, root.to_string()))
}

fn prepare_clear_all_pi_models(path: &Path) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    set_json_property(
        &object,
        "providers",
        Some(Value::Object(serde_json::Map::new())),
        path,
        "$root",
    )?;
    Ok((original, root.to_string()))
}

fn prepare_clear_pi_settings(path: &Path) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    if original.is_none() {
        return Ok((None, String::new()));
    }
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    set_json_property(&object, "defaultProvider", None, path, "$root")?;
    set_json_property(&object, "defaultModel", None, path, "$root")?;
    Ok((original, root.to_string()))
}

fn backup_config(path: &Path, agent: &str, stamp: &str) -> Result<(), String> {
    backup(path, &backups_dir(), stamp, agent, "model")
}

pub(crate) fn backup_reviewed_config_bytes(
    path: &Path,
    bytes: &[u8],
    agent: &str,
    stamp: &str,
) -> Result<(), String> {
    backup_bytes(path, bytes, &backups_dir(), stamp, agent, "model")
}

fn apply_claude(
    path: &Path,
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<ModelApplyResult, String> {
    let (original, content) = prepare_claude(path, profile, has_credential)?;
    backup_config(path, "claude-code", &backup_timestamp())?;
    write_if_unchanged(path, original.as_deref(), &content)?;
    Ok(ModelApplyResult {
        agent: "claude-code".into(),
        profile: profile.id.clone(),
        files: vec![path.display().to_string()],
        restart_required: true,
        message: "Claude Code model routing updated; start a new session to use it.".into(),
    })
}

fn apply_codex(
    path: &Path,
    profile: &ModelProfile,
    has_credential: bool,
) -> Result<ModelApplyResult, String> {
    let (original, content) = prepare_codex(path, profile, has_credential)?;
    backup_config(path, "codex", &backup_timestamp())?;
    write_if_unchanged(path, original.as_deref(), &content)?;
    Ok(ModelApplyResult {
        agent: "codex".into(),
        profile: profile.id.clone(),
        files: vec![path.display().to_string()],
        restart_required: true,
        message: "Codex model provider updated; start a new session to use it.".into(),
    })
}

fn apply_grok_build(path: &Path, profile: &ModelProfile) -> Result<ModelApplyResult, String> {
    let (original, content) = prepare_grok_build(path, profile)?;
    backup_config(path, "grok-build", &backup_timestamp())?;
    write_if_unchanged(path, original.as_deref(), &content)?;
    Ok(ModelApplyResult {
        agent: "grok-build".into(),
        profile: profile.id.clone(),
        files: vec![path.display().to_string()],
        restart_required: true,
        message:
            "Grok Build custom model and primary default updated; start a new session to use it."
                .into(),
    })
}

fn apply_grok_model(path: &Path, profile: &ModelProfile) -> Result<ModelApplyResult, String> {
    let (original, content) = prepare_grok_model(path, profile)?;
    backup_config(path, "grok-build", &backup_timestamp())?;
    write_if_unchanged(path, original.as_deref(), &content)?;
    Ok(ModelApplyResult {
        agent: "grok-build".into(),
        profile: profile.id.clone(),
        files: vec![path.display().to_string()],
        restart_required: true,
        message: "Grok Build backup Model Profile installed without changing the primary default."
            .into(),
    })
}

type ClearModelPrepare = fn(&Path) -> Result<(Option<String>, String), String>;
type ClearModelWithProfilePrepare =
    fn(&Path, &ModelProfile) -> Result<(Option<String>, String), String>;

fn clear_one_model_file(
    path: &Path,
    backup_name: &str,
    prepare: ClearModelPrepare,
) -> Result<(), String> {
    let (original, content) = prepare(path)?;
    let Some(original) = original else {
        return Ok(());
    };
    backup_config(path, backup_name, &backup_timestamp())?;
    write_if_unchanged(path, Some(&original), &content)
}

fn clear_one_model_file_with_profile(
    path: &Path,
    backup_name: &str,
    profile: &ModelProfile,
    prepare: ClearModelWithProfilePrepare,
) -> Result<(), String> {
    let (original, content) = prepare(path, profile)?;
    let Some(original) = original else {
        return Ok(());
    };
    backup_config(path, backup_name, &backup_timestamp())?;
    write_if_unchanged(path, Some(&original), &content)
}

fn rollback(path: &Path, original: Option<&str>, written: &str) -> Result<(), String> {
    match original {
        Some(original) => write_if_unchanged(path, Some(written), original),
        None => remove_if_unchanged(path, written),
    }
}

fn write_pi_transaction(
    models_path: &Path,
    models_original: Option<&str>,
    models_content: &str,
    settings_path: &Path,
    settings_original: Option<&str>,
    settings_content: &str,
) -> Result<(), String> {
    write_if_unchanged(models_path, models_original, models_content)?;
    if let Err(error) = write_if_unchanged(settings_path, settings_original, settings_content) {
        return match rollback(models_path, models_original, models_content) {
            Ok(()) => Err(format!(
                "Pi settings update failed and models.json was rolled back: {error}"
            )),
            Err(rollback_error) => Err(format!(
                "Pi settings update failed ({error}); models.json rollback also failed ({rollback_error})"
            )),
        };
    }
    Ok(())
}

fn apply_pi(
    models_path: &Path,
    settings_path: &Path,
    profile: &ModelProfile,
    api_key: Option<String>,
    active: bool,
) -> Result<ModelApplyResult, String> {
    let (models_original, models_content) =
        prepare_pi_models_with_api_key(models_path, profile, api_key)?;
    if !active {
        backup_config(models_path, "pi-models", &backup_timestamp())?;
        write_if_unchanged(models_path, models_original.as_deref(), &models_content)?;
        return Ok(ModelApplyResult {
            agent: "pi".into(),
            profile: profile.id.clone(),
            files: vec![models_path.display().to_string()],
            restart_required: true,
            message: "Pi backup Model Profile installed without changing the default Model.".into(),
        });
    }
    let (settings_original, settings_content) = prepare_pi_settings(settings_path, profile)?;
    let stamp = backup_timestamp();
    backup_config(models_path, "pi-models", &stamp)?;
    backup_config(settings_path, "pi-settings", &stamp)?;

    write_pi_transaction(
        models_path,
        models_original.as_deref(),
        &models_content,
        settings_path,
        settings_original.as_deref(),
        &settings_content,
    )?;

    Ok(ModelApplyResult {
        agent: "pi".into(),
        profile: profile.id.clone(),
        files: vec![
            models_path.display().to_string(),
            settings_path.display().to_string(),
        ],
        restart_required: true,
        message: "Pi custom provider and default model updated.".into(),
    })
}

fn clear_pi(
    models_path: &Path,
    settings_path: &Path,
    profile: &ModelProfile,
    active: bool,
) -> Result<(), String> {
    let (models_original, models_content) = prepare_clear_pi_models(models_path, profile)?;
    if !active {
        let Some(models_before) = models_original.as_deref() else {
            return Ok(());
        };
        backup_config(models_path, "pi-models", &backup_timestamp())?;
        return write_if_unchanged(models_path, Some(models_before), &models_content);
    }
    let (settings_original, settings_content) = prepare_clear_pi_settings(settings_path)?;
    if models_original.is_none() && settings_original.is_none() {
        return Ok(());
    }
    let stamp = backup_timestamp();
    backup_config(models_path, "pi-models", &stamp)?;
    backup_config(settings_path, "pi-settings", &stamp)?;
    match (models_original.as_deref(), settings_original.as_deref()) {
        (Some(models_before), Some(settings_before)) => write_pi_transaction(
            models_path,
            Some(models_before),
            &models_content,
            settings_path,
            Some(settings_before),
            &settings_content,
        ),
        (Some(models_before), None) => {
            write_if_unchanged(models_path, Some(models_before), &models_content)
        }
        (None, Some(settings_before)) => {
            write_if_unchanged(settings_path, Some(settings_before), &settings_content)
        }
        (None, None) => Ok(()),
    }
}

fn clear_all_pi(models_path: &Path, settings_path: &Path) -> Result<(), String> {
    let (models_original, models_content) = prepare_clear_all_pi_models(models_path)?;
    let (settings_original, settings_content) = prepare_clear_pi_settings(settings_path)?;
    if models_original.is_none() && settings_original.is_none() {
        return Ok(());
    }
    let stamp = backup_timestamp();
    if models_original.is_some() {
        backup_config(models_path, "pi-models", &stamp)?;
    }
    if settings_original.is_some() {
        backup_config(settings_path, "pi-settings", &stamp)?;
    }
    match (models_original.as_deref(), settings_original.as_deref()) {
        (Some(models_before), Some(settings_before)) => write_pi_transaction(
            models_path,
            Some(models_before),
            &models_content,
            settings_path,
            Some(settings_before),
            &settings_content,
        ),
        (Some(models_before), None) => {
            write_if_unchanged(models_path, Some(models_before), &models_content)
        }
        (None, Some(settings_before)) => {
            write_if_unchanged(settings_path, Some(settings_before), &settings_content)
        }
        (None, None) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::assets::ApiKeyDelivery;
    use crate::testenv::TestHome;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn protocol(endpoint_path: &str) -> ModelProviderProtocolConfig {
        ModelProviderProtocolConfig {
            endpoint_path: endpoint_path.into(),
        }
    }

    #[test]
    fn provider_base_url_and_endpoint_path_join_without_double_slashes() {
        assert_eq!(
            full_request_url(" https://gateway.example.com/api/v2/ ", "responses").unwrap(),
            "https://gateway.example.com/api/v2/responses"
        );
        assert_eq!(
            full_request_url("https://gateway.example.com/api/v2", "/tenant/v1/messages").unwrap(),
            "https://gateway.example.com/api/v2/tenant/v1/messages"
        );
        assert_eq!(
            full_request_url(
                "http://127.0.0.1:18080/v1beta",
                "/models/{model}:generateContent"
            )
            .unwrap(),
            "http://127.0.0.1:18080/v1beta/models/{model}:generateContent"
        );
    }

    #[test]
    fn endpoint_path_validation_rejects_absolute_and_traversal_inputs() {
        for invalid in [
            "",
            "https://other.example/v1/messages",
            "//other.example/v1/messages",
            "/v1/with space/messages",
            "/v1/messages#fragment",
            "/v1/messages?mode=fast",
            "/v1/../messages",
            "/v1/%2e%2e/messages",
            "/v1/%2e%2e%2fmessages",
            r"\v1\messages",
        ] {
            assert!(
                normalize_endpoint_path(invalid).is_err(),
                "accepted invalid Endpoint Path: {invalid}"
            );
        }
        assert_eq!(
            normalize_endpoint_path("tenant/v1/responses").unwrap(),
            "/tenant/v1/responses"
        );
    }

    #[test]
    fn agent_materialization_preserves_compatible_request_targets_and_blocks_others() {
        let mut profile = responses_profile();
        profile.base_url = "https://gateway.example.test".into();
        profile.endpoint_path = "/tenant/v1/responses".into();

        let materialized = materialize_profile_for_agent("codex", &profile).unwrap();
        assert_eq!(
            materialized.base_url,
            "https://gateway.example.test/tenant/v1"
        );
        assert!(materialized.endpoint_path.is_empty());
        assert_eq!(
            format!(
                "{}{}",
                materialized.base_url,
                profile.protocol.default_endpoint_path()
            ),
            full_request_url(&profile.base_url, &profile.endpoint_path).unwrap()
        );

        profile.endpoint_path = "/tenant/invoke".into();
        let error = materialize_profile_for_agent("codex", &profile).unwrap_err();
        assert!(
            error.starts_with("model_endpoint_path_unsupported:"),
            "{error}"
        );
    }

    #[test]
    fn gemini_native_profile_materializes_for_open_code_without_changing_its_target() {
        let _home = TestHome::new("model-gemini-opencode");
        let profile = ModelProfile {
            id: "local-gemini".into(),
            provider_id: Some("local-gemini-provider".into()),
            name: "Local Gemini".into(),
            provider: "custom".into(),
            model_vendor: Some("google".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::GeminiGenerateContent,
            base_url: "http://127.0.0.1:18080/v1beta".into(),
            endpoint_path: "/models/{model}:generateContent".into(),
            model: "gemini-2.5-pro".into(),
            env_key: Some("GEMINI_API_KEY".into()),
            context_window: None,
            max_output_tokens: None,
            reasoning: Some(true),
        };
        save_profile(profile.clone(), None).unwrap();
        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Auto;
            settings.set_model_selection("opencode", selection);
        })
        .unwrap();

        apply_profile("opencode", &profile.id).unwrap();

        let stored = profile_for_apply(&profile.id).unwrap();
        let materialized = materialize_profile_for_agent("opencode", &stored).unwrap();
        assert_eq!(materialized.base_url, profile.base_url);
        assert!(materialized.endpoint_path.is_empty());
        assert_eq!(
            observe_profile("opencode", &stored).unwrap(),
            ModelObservedState::Synced
        );
        assert!(ensure_supported("opencode", &profile.protocol).is_ok());
        assert!(ensure_supported("kilo-code", &profile.protocol).is_ok());
        assert!(ensure_supported("pi", &profile.protocol).is_err());
    }

    #[test]
    fn provider_cannot_disable_a_protocol_used_by_a_named_model() {
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                protocol("/v1/responses"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let mut profile = anthropic_profile();
        profile.provider_id = Some(provider.id.clone());
        let settings = crate::settings::Settings {
            model_profiles: Some(BTreeMap::from([(profile.id.clone(), profile)])),
            ..Default::default()
        };

        let error = provider_profiles(&settings, &provider).unwrap_err();
        assert_eq!(
            error,
            "model_provider_protocol_in_use: cannot disable protocols used by Models: Team Anthropic"
        );
    }

    #[test]
    fn provider_allows_multiple_protocols_to_share_one_endpoint_path() {
        let provider = ModelProviderConfig {
            id: "shared-path-provider".into(),
            name: "Shared Path Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test/api/v2/".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([
                (
                    ModelProtocol::AnthropicMessages,
                    protocol("/unified/invoke"),
                ),
                (ModelProtocol::OpenaiResponses, protocol("/unified/invoke")),
            ]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };

        validate_provider_config(&provider).unwrap();
        for protocol in provider.protocols.values() {
            assert_eq!(
                full_request_url(&provider.base_url, &protocol.endpoint_path).unwrap(),
                "https://gateway.example.test/api/v2/unified/invoke"
            );
        }
    }

    #[test]
    fn account_templates_never_persist_display_placeholders() {
        for id in ["azure-openai", "amazon-bedrock-mantle", "cloudflare-workers-ai"] {
            let template = list_providers().iter().find(|p| p.id == id).unwrap();
            let value = serde_json::to_value(template).unwrap();
            assert!(value["base_url"].is_null());
            assert!(value["default_base_url"].is_null());
            let placeholder = value["setup"]["base_url_placeholder"].as_str().unwrap();
            assert!(normalize_provider_base_url(placeholder).is_err());
        }
        assert!(normalize_provider_base_url("https://api.cloudflare.com/accounts/%3Caccount%3E/ai/v1").is_err());
        let azure = list_providers().iter().find(|p| p.id == "azure-openai").unwrap();
        let value = serde_json::to_value(azure).unwrap();
        assert_eq!(value["protocols"]["openai-responses"]["endpoint_path"], "/responses");
        assert_eq!(value["protocols"]["openai-completions"]["endpoint_path"], "/chat/completions");
    }

    #[test]
    fn infers_regions_full_requests_and_distinct_coding_plans_without_guessing_auth() {
        for (url, expected) in [
            ("https://resource.openai.azure.com/openai/v1", "azure-openai"),
            ("https://resource.services.ai.azure.com/openai/v1/responses", "azure-openai"),
            ("https://bedrock-mantle.us-east-1.api.aws/v1/responses", "amazon-bedrock-mantle"),
            ("https://api.cloudflare.com/client/v4/accounts/tenant/ai/v1", "cloudflare-workers-ai"),
            ("https://api.cloudflare.com/client/v4/accounts/tenant/ai/v1/chat/completions", "cloudflare-workers-ai"),
            ("https://api.siliconflow.cn/v1/chat/completions", "siliconflow-cn"),
            ("https://api.minimax.io/v1", "minimax"),
            ("https://api.minimax.cn/anthropic/v1/messages", "minimax-cn"),
            ("https://api.routeway.ai/v1/responses", "routeway"),
            ("https://api.routeway.ai/v1/chat/completions", "routeway"),
            ("https://api.routeway.ai/v1/messages", "routeway"),
            ("https://api.gmi-serving.com/v1", "gmi-cloud"),
            ("https://api.gmi-serving.com/v1/chat/completions", "gmi-cloud"),
            ("https://api.gmi-serving.com/v1/responses", "gmi-cloud"),
            ("https://api.gmi-serving.com/v1/messages", "gmi-cloud"),
            ("https://api.gmi-serving.com.evil.test/v1", "custom"),
            ("https://llm.onerouter.pro/v1/responses", "infron"),
            ("https://llm.onerouter.pro/v1/chat/completions", "infron"),
            ("https://llm.onerouter.pro/v1/messages", "infron"),
            ("https://api.apinex.bond/v1/chat/completions", "apinex"),
            ("https://api.apinex.bond/v1/messages", "apinex"),
            ("https://tokenharbor.ai/v1/responses", "tokenharbor"),
            ("https://tokenharbor.ai/v1/chat/completions", "tokenharbor"),
            ("https://tokenharbor.ai/v1/messages", "tokenharbor"),
            ("https://api.flock.io/v1", "flock"),
            ("https://api.flock.io/v1/chat/completions", "flock"),
            ("https://api.flock.io.evil.test/v1", "custom"),
            ("https://codecraftapi.com/v1", "codecraft"),
            ("https://codecraftapi.com/v1/chat/completions", "codecraft"),
            ("https://codecraftapi.com.evil.test/v1", "custom"),
            ("https://api.beatapi.io/v1", "beatapi"),
            ("https://api.beatapi.io/v1/chat/completions", "beatapi"),
            ("https://api.beatapi.io.evil.test/v1", "custom"),
            ("https://api.experientiallabs.ai/v1", "experiential-labs"),
            ("https://api.experientiallabs.ai/v1/chat/completions", "experiential-labs"),
            ("https://api.experientiallabs.ai.evil.test/v1", "custom"),
            ("https://inference.runanywhere.ai/v1", "runanywhere"),
            ("https://inference.runanywhere.ai/v1/chat/completions", "runanywhere"),
            ("https://runanywhere.ai/v1", "custom"),
            ("https://console.runanywhere.ai/v1", "custom"),
            ("https://inference.runanywhere.ai.evil.test/v1", "custom"),
            ("https://api.sarashina.ai/v1", "sarashina"),
            ("https://api.sarashina.ai/v1/chat/completions", "sarashina"),
            ("https://api.sarashina.ai.evil.test/v1", "custom"),
            ("https://inference.liquid.ai/v1", "liquid-ai"),
            ("https://inference.liquid.ai/v1/chat/completions", "liquid-ai"),
            ("https://inference.liquid.ai.evil.test/v1", "custom"),
            ("https://api.atria-asi.ai/v1", "atria"),
            ("https://api.atria-asi.ai/v1/responses", "atria"),
            ("https://api.atria-asi.ai/v1/chat/completions", "atria"),
            ("https://api.atria-asi.ai/v1/messages", "atria"),
            ("https://atria-asi.ai/v1", "custom"),
            ("https://api.atria-asi.ai.evil.test/v1", "custom"),
            ("https://cleanapis.com/v1/chat/completions", "clean-apis"),
            ("https://lighthouse.dphn.ai/run/text-v/v1/chat/completions", "dolphin"),
            ("https://api.iteracompute.com/v1/responses", "iteracompute"),
            ("https://api.iteracompute.com/v1/chat/completions", "iteracompute"),
            ("https://llmtr.com/v1/chat/completions", "llmtr"),
            ("https://api.munito.ai/inference/v1/responses", "munito"),
            ("https://api.munito.ai/inference/v1/chat/completions", "munito"),
            ("https://api.munito.ai/inference/v1/messages", "munito"),
            ("https://api.eu.munito.ai/inference/v1", "munito"),
            ("https://api.us.munito.ai/inference/v1", "munito"),
            ("https://inference-api.nousresearch.com/v1/chat/completions", "nous-portal"),
            ("https://api.ppq.ai/v1/responses", "ppq"),
            ("https://api.ppq.ai/v1/chat/completions", "ppq"),
            ("https://api.ppq.ai/v1/messages", "ppq"),
            ("https://api.river.ai/v1/responses", "river"),
            ("https://api.river.ai/v1/chat/completions", "river"),
            ("https://api.trustedrouter.com/v1/responses", "trustedrouter"),
            ("https://api.trustedrouter.com/v1/chat/completions", "trustedrouter"),
            ("https://api.trustedrouter.com/v1/messages", "trustedrouter"),
            ("https://trustedrouter.com/v1", "custom"),
            ("https://api.unorouter.com/v1/chat/completions", "unorouter"),
            ("https://api.unorouter.com/v1/responses", "unorouter"),
            ("https://api.unorouter.com/v1/messages", "unorouter"),
            ("https://api.unorouter.com/v1beta/models/gemini-4-flash:generateContent", "unorouter"),
            ("https://unorouter.com/v1", "custom"),
            ("https://api.unorouter.com.evil.test/v1", "custom"),
            ("https://api.ant-ling.com/v1/chat/completions", "ant-ling"),
            ("https://api.ant-ling.com/anthropic/v1/messages", "ant-ling"),
            ("https://maas-api.antdigital.com/v1/chat/completions", "antdigital-dtmaas"),
            ("https://maas-api.antdigital.com/v1/responses", "antdigital-dtmaas"),
            ("https://maas-api.unisound.com/v1/chat/completions", "unisound"),
            ("https://maas-api.unisound.com/v1/responses", "unisound"),
            ("https://maas-api.unisound.com/anthropic/v1/messages", "unisound"),
            ("https://maas-api.unisound.com.evil.test/v1", "custom"),
            ("https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions", "volcengine-coding-plan"),
            ("https://qianfan.baidubce.com/anthropic/coding/v1/messages", "baidu-qianfan-coding-plan"),
            ("https://aiplatform.googleapis.com/v1", "custom"),
            ("https://resource.openai.azure.com.evil.test/openai/v1", "custom"),
            ("https://api.cloudflare.com/client/v4/zones/tenant/ai/v1", "custom"),
        ] {
            assert_eq!(infer_provider(url), expected, "{url}");
        }
    }

    #[test]
    fn provider_endpoints_are_valid_and_only_reviewed_plans_share_urls() {
        let mut ids = BTreeSet::new();
        let mut endpoints = BTreeSet::new();
        for provider in list_providers() {
            assert!(
                ids.insert(provider.id),
                "duplicate provider id: {}",
                provider.id
            );
            let Some(default_base_url) = provider.default_base_url else {
                continue;
            };
            assert!(
                endpoints.insert((default_base_url, provider.default_protocol.clone()))
                    || matches!(provider.id, "minimax-coding-plan" | "minimax-cn-coding-plan"),
                "duplicate provider endpoint: {default_base_url}"
            );
            assert!(
                !default_base_url.ends_with('/'),
                "provider default must not end with a slash: {default_base_url}"
            );
            let parsed = url::Url::parse(default_base_url).unwrap_or_else(|error| {
                panic!("invalid provider default {default_base_url}: {error}")
            });
            let expected_scheme = if provider.category == "local" {
                "http"
            } else {
                "https"
            };
            assert_eq!(parsed.scheme(), expected_scheme);
            let inferred = match provider.id {
                "minimax-coding-plan" => "minimax",
                "minimax-cn-coding-plan" => "minimax-cn",
                id => id,
            };
            assert_eq!(infer_provider(default_base_url), inferred);

            for endpoint in provider_additional_endpoints(provider.id) {
                assert_ne!(endpoint.protocol, provider.default_protocol);
                assert!(
                    endpoints.insert((endpoint.base_url, endpoint.protocol.clone()))
                        || matches!(provider.id, "minimax-coding-plan" | "minimax-cn-coding-plan"),
                    "duplicate provider endpoint: {}",
                    endpoint.base_url
                );
                assert!(
                    !endpoint.base_url.ends_with('/'),
                    "provider endpoint must not end with a slash: {}",
                    endpoint.base_url
                );
                let parsed = url::Url::parse(endpoint.base_url).unwrap_or_else(|error| {
                    panic!("invalid provider endpoint {}: {error}", endpoint.base_url)
                });
                assert_eq!(parsed.scheme(), expected_scheme);
                assert_eq!(infer_provider(endpoint.base_url), inferred);
            }
        }

        assert!(list_providers().iter().all(|provider| matches!(
            provider.category,
            "official" | "gateway" | "local" | "custom"
        )));
        assert!(list_providers()
            .iter()
            .all(|provider| provider.id != "local" && provider.name != "Local endpoint"));
        let endpointless = list_providers()
            .iter()
            .filter(|provider| provider.default_base_url.is_none())
            .collect::<Vec<_>>();
        assert_eq!(endpointless.iter().map(|p| p.id).collect::<BTreeSet<_>>(),
            BTreeSet::from(["azure-openai", "amazon-bedrock-mantle", "cloudflare-workers-ai", "custom"]));
        assert_eq!(list_providers().len(), 125);
        assert!(!list_providers().iter().any(|p| p.id == "github-models"));
        let openrouter = list_providers()
            .iter()
            .find(|provider| provider.id == "openrouter")
            .unwrap();
        let serialized = serde_json::to_value(openrouter).unwrap();
        assert_eq!(serialized["base_url"], "https://openrouter.ai/api/v1");
        assert_eq!(
            serialized["protocols"]["openai-responses"]["endpoint_path"],
            "/responses"
        );
        let google = list_providers()
            .iter()
            .find(|provider| provider.id == "google")
            .unwrap();
        let serialized = serde_json::to_value(google).unwrap();
        assert_eq!(
            google.default_protocol,
            ModelProtocol::GeminiGenerateContent
        );
        assert_eq!(
            serialized["base_url"],
            "https://generativelanguage.googleapis.com"
        );
        assert_eq!(
            serialized["protocols"]["gemini-generate-content"]["endpoint_path"],
            "/v1beta/models/{model}:generateContent"
        );
        assert_eq!(
            serialized["protocols"]["openai-completions"]["endpoint_path"],
            "/v1beta/openai/chat/completions"
        );

        let plan_ids = [
            "zai-coding-plan",
            "zhipuai-coding-plan",
            "alibaba-coding-plan-cn",
            "alibaba-coding-plan",
            "alibaba-token-plan-cn",
            "alibaba-token-plan",
            "xiaomi-token-plan-cn",
            "xiaomi-token-plan-sgp",
            "xiaomi-token-plan-ams",
            "kimi-for-coding",
            "minimax-coding-plan",
            "minimax-cn-coding-plan",
            "stepfun-step-plan",
            "stepfun-ai-step-plan",
            "tencent-coding-plan",
            "tencent-token-plan",
            "tencent-token-plan-global",
        ];
        for id in plan_ids {
            assert_eq!(
                provider_additional_endpoints(id).len(),
                1,
                "{id} should include its Anthropic-compatible endpoint"
            );
        }
        let alibaba_plan = list_providers()
            .iter()
            .find(|provider| provider.id == "alibaba-token-plan")
            .unwrap();
        let serialized = serde_json::to_value(alibaba_plan).unwrap();
        assert_eq!(
            serialized["additional_endpoints"][0]["base_url"],
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic"
        );
        assert_eq!(
            serialized["base_url"],
            "https://token-plan.ap-southeast-1.maas.aliyuncs.com"
        );
        assert_eq!(
            serialized["protocols"]["openai-completions"]["endpoint_path"],
            "/compatible-mode/v1/chat/completions"
        );
        assert_eq!(
            serialized["protocols"]["anthropic-messages"]["endpoint_path"],
            "/apps/anthropic/v1/messages"
        );

        for id in ["poe", "huggingface", "nebius", "requesty"] {
            assert_eq!(
                list_providers()
                    .iter()
                    .find(|provider| provider.id == id)
                    .map(|provider| &provider.default_protocol),
                Some(&ModelProtocol::OpenaiResponses),
                "{id} should start with its documented Responses API"
            );
        }
        for id in [
            "novita-ai",
            "qiniu-ai",
            "nvidia",
            "digitalocean",
            "moonshotai",
            "zai",
            "modelscope",
            "scaleway",
            "baseten",
            "wandb",
        ] {
            assert_eq!(
                list_providers()
                    .iter()
                    .find(|provider| provider.id == id)
                    .map(|provider| &provider.default_protocol),
                Some(&ModelProtocol::OpenaiCompletions),
                "{id} should start with its documented Chat Completions API"
            );
        }
    }

    fn anthropic_profile() -> ModelProfile {
        ModelProfile {
            id: "team-anthropic".into(),
            provider_id: None,
            name: "Team Anthropic".into(),
            provider: "custom".into(),
            model_vendor: Some("anthropic".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::AnthropicMessages,
            base_url: "https://gateway.example.test/anthropic".into(),
            endpoint_path: String::new(),
            model: "claude-sonnet-custom".into(),
            env_key: None,
            context_window: Some(200_000),
            max_output_tokens: Some(16_384),
            reasoning: Some(true),
        }
    }

    fn responses_profile() -> ModelProfile {
        ModelProfile {
            id: "team-openai".into(),
            provider_id: None,
            name: "Team OpenAI".into(),
            provider: "custom".into(),
            model_vendor: Some("openai".into()),
            native_ids: Default::default(),
            protocol: ModelProtocol::OpenaiResponses,
            base_url: "https://gateway.example.test/v1".into(),
            endpoint_path: String::new(),
            model: "gpt-custom".into(),
            env_key: None,
            context_window: Some(128_000),
            max_output_tokens: Some(16_000),
            reasoning: Some(true),
        }
    }

    #[test]
    fn existing_provider_migration_never_reads_the_secret_value() {
        let _home = TestHome::new("provider-presence-only");
        let mut profile = responses_profile();
        profile.provider_id = Some("presence-provider".into());
        save_profile(profile, Some("fixture-provider-key".into())).unwrap();
        let service = provider_keychain_service("presence-provider");
        let value_read_failure = set_test_credential_read_error();
        assert!(read_credential_service_checked(&service).is_err());
        assert!(credential_service_exists_checked(&service).unwrap());
        assert!(!consolidate_provider_credentials().unwrap());
        drop(value_read_failure);
        assert_eq!(read_credential_service_checked(&service).unwrap().unwrap(), b"fixture-provider-key");
    }

    fn pi_profile(id: &str, model: &str, name: &str) -> ModelProfile {
        let mut profile = responses_profile();
        profile.id = id.into();
        profile.model = model.into();
        profile.name = name.into();
        profile.provider_id = Some("openrouter-provider".into());
        profile.native_ids.insert("pi".into(), "openrouter".into());
        profile
    }

    fn openrouter_provider(id: &str, name: &str) -> ModelProviderConfig {
        ModelProviderConfig {
            id: id.into(),
            name: name.into(),
            provider: "openrouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(ModelProtocol::OpenaiResponses, protocol("/responses"))]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        }
    }

    #[cfg(unix)]
    #[test]
    fn opencode_arca_store_delivery_updates_model_and_private_agent_store() {
        use std::os::unix::fs::PermissionsExt;

        let _home = TestHome::new("opencode-agent-store");
        let mut provider = openrouter_provider("max-ai", "Max AI");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::ArcaStore);
        let mut profile = responses_profile();
        profile.id = "max-ai-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider.clone(),
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();
        set_credential(
            &provider_credential_subject(&provider.id),
            b"opencode-private-secret",
        )
        .unwrap();
        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Auto;
            settings.set_model_selection("opencode", selection);
        })
        .unwrap();

        apply_profile("opencode", &profile.id).unwrap();

        let model_config =
            fs::read_to_string(home().join(".config/opencode/opencode.json")).unwrap();
        assert!(model_config.contains("mux_"));
        assert!(!model_config.contains("opencode-private-secret"));
        let auth_path = home().join(".local/share/opencode/auth.json");
        let auth: Value = serde_json::from_str(&fs::read_to_string(&auth_path).unwrap()).unwrap();
        let native_id = adapters::native_provider_id("opencode", &profile);
        assert_eq!(auth[&native_id]["type"], "api");
        assert_eq!(auth[&native_id]["key"], "opencode-private-secret");
        assert_eq!(
            fs::metadata(auth_path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let backups = home().join(".arca/backups");
        if backups.exists() {
            for entry in fs::read_dir(backups).unwrap().flatten() {
                if entry.path().is_file() {
                    assert!(!fs::read_to_string(entry.path())
                        .unwrap_or_default()
                        .contains("opencode-private-secret"));
                }
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn provider_owned_credential_ignores_legacy_source_override_but_honors_agent_delivery() {
        use crate::domain::assets::{
            ApiKeyDelivery, ModelAgentSelection, ModelConsumptionRecord, ModelCredentialPolicy,
        };
        use std::os::unix::fs::PermissionsExt;

        let _home = TestHome::new("provider-owned-credential");
        let mut provider = openrouter_provider("provider-owned", "Provider Owned");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::ArcaStore);
        let mut profile = responses_profile();
        profile.id = "provider-owned-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider.clone(),
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();
        set_credential(
            &provider_credential_subject(&provider.id),
            b"provider-owned-secret",
        )
        .unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "opencode",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: profile.id.clone(),
                            enabled: true,
                            credential: ModelCredentialPolicy {
                                source_override: Some(ApiKeySource::Env {
                                    name: "SHOULD_NOT_BE_READ".into(),
                                }),
                                delivery: ApiKeyDelivery::Plaintext,
                            },
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some(profile.id.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        apply_profile("opencode", &profile.id).unwrap();

        let model_path = home().join(".config/opencode/opencode.json");
        let model_config = fs::read_to_string(&model_path).unwrap();
        assert!(model_config.contains("provider-owned-secret"));
        assert!(!model_config.contains("SHOULD_NOT_BE_READ"));
        assert_eq!(
            fs::metadata(model_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!home().join(".local/share/opencode/auth.json").exists());
    }

    #[test]
    fn provider_owned_materialization_ignores_legacy_file_override() {
        use crate::domain::assets::{
            ApiKeyDelivery, ModelAgentSelection, ModelConsumptionRecord, ModelCredentialPolicy,
        };

        let _home = TestHome::new("provider-owned-materialization");
        let mut provider = openrouter_provider("provider-materialized", "Provider Materialized");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::ArcaStore);
        let mut profile = responses_profile();
        profile.id = "provider-materialized-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider.clone(),
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();
        set_credential(
            &provider_credential_subject(&provider.id),
            b"provider-materialized-secret",
        )
        .unwrap();
        mutate_settings(|settings| {
            settings.set_model_selection(
                "opencode",
                ModelAgentSelection {
                    profiles: BTreeMap::from([(
                        profile.id.clone(),
                        ModelConsumptionRecord {
                            profile_id: profile.id.clone(),
                            enabled: true,
                            credential: ModelCredentialPolicy {
                                source_override: Some(ApiKeySource::File {
                                    path: "/legacy/provider.key".into(),
                                }),
                                delivery: ApiKeyDelivery::Auto,
                            },
                            last_selected_at: None,
                        },
                    )]),
                    active_profile_id: Some(profile.id.clone()),
                    ..Default::default()
                },
            );
        })
        .unwrap();

        apply_profile("opencode", &profile.id).unwrap();

        let model_config =
            fs::read_to_string(home().join(".config/opencode/opencode.json")).unwrap();
        assert!(!model_config.contains("/legacy/provider.key"));
        assert!(!model_config.contains("provider-materialized-secret"));
        let auth: Value = serde_json::from_str(
            &fs::read_to_string(home().join(".local/share/opencode/auth.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            auth[adapters::native_provider_id("opencode", &profile)]["key"],
            "provider-materialized-secret"
        );
    }

    #[cfg(unix)]
    #[test]
    fn opencode_file_source_is_validated_before_writing_a_native_reference() {
        use std::os::unix::fs::PermissionsExt;

        let _home = TestHome::new("opencode-file-source");
        let key_path = home().join("provider.key");
        fs::write(&key_path, "file-secret").unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o644)).unwrap();
        let mut provider = openrouter_provider("file-provider", "File Provider");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::File {
            path: key_path.display().to_string(),
        });
        let mut profile = responses_profile();
        profile.id = "file-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider,
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();
        mutate_settings(|settings| {
            let mut selection = settings.model_selection("opencode");
            selection.default_delivery = ApiKeyDelivery::Auto;
            settings.set_model_selection("opencode", selection);
        })
        .unwrap();

        let error = apply_profile("opencode", &profile.id).unwrap_err();
        assert!(error.starts_with("credential_file_insecure:"));
        assert!(!home().join(".config/opencode/opencode.json").exists());

        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
        apply_profile("opencode", &profile.id).unwrap();
        let model_config =
            fs::read_to_string(home().join(".config/opencode/opencode.json")).unwrap();
        assert!(model_config.contains(&format!("{{file:{}}}", key_path.display())));
        assert!(!model_config.contains("file-secret"));
    }

    #[test]
    fn codex_helper_source_preserves_structured_command_and_arguments() {
        let _home = TestHome::new("codex-helper-source");
        let mut provider = openrouter_provider("helper-provider", "Helper Provider");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::Helper {
            command: "/usr/local/bin/read-provider-key".into(),
            args: vec!["--profile".into(), "coding team".into()],
            ttl_ms: Some(300_000),
        });
        let mut profile = responses_profile();
        profile.id = "helper-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider,
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();

        apply_profile("codex", &profile.id).unwrap();

        let config = fs::read_to_string(home().join(".codex/config.toml")).unwrap();
        let document = config.parse::<Document>().unwrap();
        let provider = document["model_providers"][codex_provider_id(&profile)]
            .as_table()
            .unwrap();
        let auth = provider["auth"].as_table().unwrap();
        assert_eq!(
            auth["command"].as_str(),
            Some("/usr/local/bin/read-provider-key")
        );
        assert_eq!(
            auth["args"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|value| value.as_str())
                .collect::<Vec<_>>(),
            vec!["--profile", "coding team"]
        );
    }

    #[test]
    fn required_arca_store_without_a_key_blocks_agent_configuration() {
        let _home = TestHome::new("required-missing-key");
        let mut provider = openrouter_provider("missing-provider", "Missing Provider");
        provider.auth_requirement = AuthRequirement::Required;
        provider.api_key_source = Some(ApiKeySource::ArcaStore);
        let mut profile = responses_profile();
        profile.id = "missing-key-model".into();
        profile.provider_id = Some(provider.id.clone());
        profile.provider = provider.provider.clone();
        profile.base_url = provider.base_url.clone();
        profile.endpoint_path = "/responses".into();
        save_provider_bundle(
            provider,
            BTreeMap::from([(profile.id.clone(), profile.clone())]),
        )
        .unwrap();

        let error = apply_profile("codex", &profile.id).unwrap_err();
        assert!(error.starts_with("credential_missing:"));
        assert!(!home().join(".codex/config.toml").exists());
    }

    #[test]
    fn provider_migration_groups_matching_credentials_and_preserves_request_targets() {
        let _home = TestHome::new("model-provider-v3-migration");
        let anthropic = anthropic_profile();
        let responses = responses_profile();
        set_credential(&anthropic.id, b"shared-secret").unwrap();
        set_credential(&responses.id, b"shared-secret").unwrap();
        crate::settings::save_settings(&crate::settings::Settings {
            version: Some(2),
            model_profiles: Some(BTreeMap::from([
                (anthropic.id.clone(), anthropic.clone()),
                (responses.id.clone(), responses.clone()),
            ])),
            ..Default::default()
        })
        .unwrap();

        assert!(migrate_model_providers_v3_if_needed().unwrap());
        assert!(!migrate_model_providers_v3_if_needed().unwrap());
        let settings = crate::settings::load_settings_strict().unwrap();
        assert_eq!(settings.version, Some(crate::settings::SETTINGS_VERSION));
        let providers = settings.model_providers.unwrap();
        assert_eq!(providers.len(), 1);
        let provider = providers.values().next().unwrap();
        assert_eq!(provider.base_url, "https://gateway.example.test");
        assert_eq!(provider.protocols.len(), 2);
        assert_eq!(
            protocol_client_base_url(
                &provider.base_url,
                &ModelProtocol::AnthropicMessages,
                &provider.protocols[&ModelProtocol::AnthropicMessages].endpoint_path,
            )
            .unwrap(),
            anthropic.base_url,
        );
        assert_eq!(
            protocol_client_base_url(
                &provider.base_url,
                &ModelProtocol::OpenaiResponses,
                &provider.protocols[&ModelProtocol::OpenaiResponses].endpoint_path,
            )
            .unwrap(),
            responses.base_url,
        );
        let profiles = settings.model_profiles.unwrap();
        assert_eq!(
            profiles[&anthropic.id].provider_id,
            profiles[&responses.id].provider_id
        );
        let instances = list_provider_instances();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].model_count, 2);
        assert!(instances[0].credential_saved);
        assert_eq!(
            read_credential_service_checked(&provider_keychain_service(&provider.id)).unwrap().unwrap(),
            b"shared-secret"
        );
        assert_eq!(
            read_credential_service_checked(&legacy_keychain_service(&anthropic.id)).unwrap().unwrap(),
            b"shared-secret"
        );
        assert_eq!(
            read_credential_service_checked(&legacy_keychain_service(&responses.id)).unwrap().unwrap(),
            b"shared-secret"
        );
    }

    #[test]
    fn v4_provider_endpoints_migrate_idempotently_without_changing_request_targets() {
        let _home = TestHome::new("settings-provider-v4-migration");
        let path = settings_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
              "version": 4,
              "model_providers": {
                "team-provider": {
                  "id": "team-provider",
                  "name": "Team Provider",
                  "provider": "custom",
                  "endpoints": {
                    "openai-responses": "https://gateway.example.test/api/v2",
                    "anthropic-messages": "https://gateway.example.test/anthropic"
                  }
                },
                "shared-provider": {
                  "id": "shared-provider",
                  "name": "Shared Provider",
                  "provider": "custom",
                  "endpoints": {
                    "openai-responses": "https://shared.example.test/api/v2",
                    "anthropic-messages": "https://shared.example.test/api/v2"
                  }
                }
              }
            }"#,
        )
        .unwrap();

        assert!(migrate_model_providers_v3_if_needed().unwrap());
        assert!(!migrate_model_providers_v3_if_needed().unwrap());

        let raw_settings: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(raw_settings["version"], crate::settings::SETTINGS_VERSION);
        let raw: Value =
            serde_json::from_str(&fs::read_to_string(crate::paths::model_catalog_file()).unwrap())
                .unwrap();
        let provider = &raw["providers"]["team-provider"];
        assert!(provider.get("endpoints").is_none());
        assert_eq!(provider["base_url"], "https://gateway.example.test");
        assert_eq!(
            provider["protocols"]["openai-responses"]["endpoint_path"],
            "/api/v2/responses"
        );
        assert_eq!(
            provider["protocols"]["anthropic-messages"]["endpoint_path"],
            "/anthropic/v1/messages"
        );
        let shared = &raw["providers"]["shared-provider"];
        assert_eq!(shared["base_url"], "https://shared.example.test/api/v2");
        assert_eq!(
            shared["protocols"]["openai-responses"]["endpoint_path"],
            "/responses"
        );
        assert_eq!(
            shared["protocols"]["anthropic-messages"]["endpoint_path"],
            "/v1/messages"
        );
    }

    #[test]
    fn v4_multi_origin_provider_migration_is_blocked_without_rewriting_settings() {
        let _home = TestHome::new("settings-provider-v4-blocked");
        let path = settings_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{
          "version": 4,
          "model_providers": {
            "mixed-provider": {
              "id": "mixed-provider",
              "name": "Mixed Provider",
              "provider": "custom",
              "endpoints": {
                "openai-responses": "https://first.example.test/v1",
                "anthropic-messages": "https://second.example.test/anthropic"
              }
            }
          }
        }"#;
        fs::write(&path, original).unwrap();

        let error = crate::settings::load_settings_strict()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("model_provider_endpoint_migration_blocked")
                && error.contains("split it into separate Providers"),
            "{error}"
        );
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn v3_repair_restores_only_unique_provider_references() {
        let _home = TestHome::new("model-provider-v3-reference-repair");
        let primary = ModelProviderConfig {
            id: "primary-provider".into(),
            name: "Primary".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([
                (ModelProtocol::OpenaiResponses, protocol("/v1/responses")),
                (
                    ModelProtocol::AnthropicMessages,
                    protocol("/anthropic/v1/messages"),
                ),
            ]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let secure = ModelProviderConfig {
            id: "secure-provider".into(),
            name: "Secure".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::AnthropicMessages,
                protocol("/anthropic/v1/messages"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let responses = responses_profile();
        let anthropic = anthropic_profile();
        crate::settings::save_settings(&crate::settings::Settings {
            version: Some(3),
            model_providers: Some(BTreeMap::from([
                (primary.id.clone(), primary),
                (secure.id.clone(), secure),
            ])),
            model_profiles: Some(BTreeMap::from([
                (responses.id.clone(), responses.clone()),
                (anthropic.id.clone(), anthropic.clone()),
            ])),
            ..Default::default()
        })
        .unwrap();

        assert!(migrate_model_providers_v3_if_needed().unwrap());
        let settings = crate::settings::load_settings_strict().unwrap();
        let profiles = settings.model_profiles.unwrap();
        assert_eq!(
            profiles[&responses.id].provider_id.as_deref(),
            Some("primary-provider")
        );
        assert_eq!(profiles[&anthropic.id].provider_id, None);
        assert!(!migrate_model_providers_v3_if_needed().unwrap());
    }

    #[test]
    fn v3_migration_does_not_merge_distinct_provider_credentials() {
        let _home = TestHome::new("model-provider-v3-distinct-credentials");
        let anthropic = anthropic_profile();
        let responses = responses_profile();
        set_credential(&anthropic.id, b"personal-secret").unwrap();
        set_credential(&responses.id, b"team-secret").unwrap();
        crate::settings::save_settings(&crate::settings::Settings {
            version: Some(2),
            model_profiles: Some(BTreeMap::from([
                (anthropic.id.clone(), anthropic),
                (responses.id.clone(), responses),
            ])),
            ..Default::default()
        })
        .unwrap();

        migrate_model_providers_v3_if_needed().unwrap();
        let settings = crate::settings::load_settings_strict().unwrap();
        assert_eq!(settings.model_providers.unwrap().len(), 2);
    }

    #[test]
    fn v3_credential_consolidation_rejects_conflicting_legacy_keys() {
        let _home = TestHome::new("model-provider-v3-legacy-credential-drift");
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([
                (ModelProtocol::OpenaiResponses, protocol("/v1/responses")),
                (
                    ModelProtocol::AnthropicMessages,
                    protocol("/anthropic/v1/messages"),
                ),
            ]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let mut first = responses_profile();
        first.provider_id = Some(provider.id.clone());
        let mut second = anthropic_profile();
        second.provider_id = Some(provider.id.clone());
        crate::settings::save_settings(&crate::settings::Settings {
            version: Some(3),
            model_providers: Some(BTreeMap::from([(provider.id.clone(), provider)])),
            model_profiles: Some(BTreeMap::from([
                (first.id.clone(), first.clone()),
                (second.id.clone(), second.clone()),
            ])),
            ..Default::default()
        })
        .unwrap();
        set_credential_service(&legacy_keychain_service(&first.id), b"first-secret").unwrap();
        set_credential_service(&legacy_keychain_service(&second.id), b"second-secret").unwrap();

        let error = migrate_model_providers_v3_if_needed().unwrap_err();
        assert!(
            error.starts_with("model_provider_credential_drift:"),
            "{error}"
        );
        assert!(read_credential_service_checked(&provider_keychain_service("team-provider")).unwrap().is_none());
    }

    #[test]
    fn provider_models_share_one_permanent_keychain_item() {
        let _home = TestHome::new("model-provider-one-keychain-item");
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([
                (ModelProtocol::OpenaiResponses, protocol("/v1/responses")),
                (
                    ModelProtocol::AnthropicMessages,
                    protocol("/anthropic/v1/messages"),
                ),
            ]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let mut first = responses_profile();
        first.provider_id = Some(provider.id.clone());
        let mut second = anthropic_profile();
        second.provider_id = Some(provider.id.clone());

        save_profile(first.clone(), Some("shared-secret".into())).unwrap();
        save_profile(second.clone(), None).unwrap();
        let provider_service = provider_keychain_service(&provider.id);
        assert_eq!(
            read_credential_service_checked(&provider_service).unwrap().unwrap(),
            b"shared-secret"
        );
        assert!(read_credential_service_checked(&legacy_keychain_service(&first.id)).unwrap().is_none());
        assert!(read_credential_service_checked(&legacy_keychain_service(&second.id)).unwrap().is_none());
        assert_eq!(security_command(&first.id), security_command(&second.id));

        delete_profile(&first.id).unwrap();
        assert_eq!(credential_snapshot(&second.id).unwrap().unwrap(), b"shared-secret");
        delete_profile(&second.id).unwrap();
        assert_eq!(
            read_credential_service_checked(&provider_service).unwrap().unwrap(),
            b"shared-secret"
        );
        assert!(load_settings()
            .model_providers
            .unwrap()
            .contains_key(&provider.id));
        delete_provider(&provider.id).unwrap();
        assert!(read_credential_service_checked(&provider_service).unwrap().is_none());
    }

    #[test]
    fn provider_credential_reveal_is_scoped_to_configured_providers() {
        let _home = TestHome::new("model-provider-credential-reveal");
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                protocol("/v1/responses"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        crate::settings::save_settings(&crate::settings::Settings {
            model_providers: Some(BTreeMap::from([(provider.id.clone(), provider.clone())])),
            ..Default::default()
        })
        .unwrap();
        set_credential_service(
            &provider_keychain_service(&provider.id),
            b"saved-test-value",
        )
        .unwrap();
        set_credential_service(
            &provider_keychain_service("unmanaged-provider"),
            b"unmanaged-test-value",
        )
        .unwrap();

        assert_eq!(
            reveal_provider_credential(&provider.id).unwrap(),
            "saved-test-value"
        );
        let unmanaged = reveal_provider_credential("unmanaged-provider").unwrap_err();
        assert!(
            unmanaged.starts_with("model_provider_not_found:"),
            "{unmanaged}"
        );

        delete_credential_service(&provider_keychain_service(&provider.id)).unwrap();
        let missing = reveal_provider_credential(&provider.id).unwrap_err();
        assert!(
            missing.starts_with("model_provider_credential_missing:"),
            "{missing}"
        );
        delete_credential_service(&provider_keychain_service("unmanaged-provider")).unwrap();
    }

    #[test]
    fn one_provider_rejects_duplicate_model_ids() {
        let provider = ModelProviderConfig {
            id: "team-provider".into(),
            name: "Team Provider".into(),
            provider: "custom".into(),
            base_url: "https://gateway.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                protocol("/v1/responses"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: None,
        };
        let mut existing = responses_profile();
        existing.provider_id = Some(provider.id.clone());
        let settings = crate::settings::Settings {
            version: Some(3),
            model_providers: Some(BTreeMap::from([(provider.id.clone(), provider)])),
            model_profiles: Some(BTreeMap::from([(existing.id.clone(), existing.clone())])),
            ..Default::default()
        };
        let mut duplicate = existing;
        duplicate.id.clear();
        duplicate.name = "Another Display Name".into();

        let error = prepare_profile_draft(&settings, None, duplicate).unwrap_err();
        assert!(error.starts_with("asset_identity_conflict:"), "{error}");
    }

    #[test]
    fn ordinary_profile_edit_can_switch_provider_relationships() {
        let _home = TestHome::new("model-switch-provider");
        let first_provider = ModelProviderConfig {
            id: "first-provider".into(),
            name: "First Provider".into(),
            provider: "custom".into(),
            base_url: "https://first.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                protocol("/v1/responses"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: Some("FIRST_KEY".into()),
        };
        let second_provider = ModelProviderConfig {
            id: "second-provider".into(),
            name: "Second Provider".into(),
            provider: "openrouter".into(),
            base_url: "https://second.example.test".into(),
            model_catalog_url: None,
            portal_url: None,
            protocols: BTreeMap::from([(
                ModelProtocol::OpenaiResponses,
                protocol("/v1/responses"),
            )]),
            auth_requirement: crate::domain::types::AuthRequirement::Optional,
            api_key_source: None,
            env_key: Some("SECOND_KEY".into()),
        };
        save_provider_bundle(first_provider.clone(), BTreeMap::new()).unwrap();
        save_provider_bundle(second_provider.clone(), BTreeMap::new()).unwrap();
        let mut profile = responses_profile();
        profile.provider_id = Some(first_provider.id.clone());
        save_profile(profile.clone(), None).unwrap();

        profile.provider_id = Some(second_provider.id.clone());
        let profile_id = profile.id.clone();
        let switched = prepare_profile_draft(&load_settings(), Some(&profile_id), profile).unwrap();
        assert_eq!(
            switched.provider_id.as_deref(),
            Some(second_provider.id.as_str())
        );
        assert_eq!(switched.provider, second_provider.provider);
        assert_eq!(switched.base_url, second_provider.base_url);
        assert_eq!(
            switched.endpoint_path,
            second_provider.protocols[&ModelProtocol::OpenaiResponses].endpoint_path
        );
        assert_eq!(switched.env_key, second_provider.env_key);
    }

    #[test]
    fn public_model_writer_waits_for_the_settings_filesystem_lock() {
        let _home = TestHome::new("model-public-writer-lock");
        let settings_guard = acquire_settings_lock(&settings_file()).unwrap();
        let profile = responses_profile();
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();

        let writer = thread::spawn(move || {
            started_tx.send(()).unwrap();
            let result = save_profile(profile, None);
            done_tx.send(result).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            done_rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "public Model writer bypassed the settings filesystem lock"
        );

        drop(settings_guard);
        let result = done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(result.is_ok(), "{result:?}");
        writer.join().unwrap();
        assert!(list_profiles()
            .iter()
            .any(|profile| profile.profile.id == "team-openai"));
    }

    #[test]
    fn profile_metadata_never_serializes_a_credential() {
        let profile = responses_profile();
        let json = serde_json::to_string(&profile).unwrap();
        assert!(!json.contains("credential"));
        assert!(!json.contains("api_key"));
    }

    #[test]
    fn ordinary_profile_creation_cannot_inject_an_agent_native_identity() {
        let settings = crate::settings::Settings::default();
        let mut draft = responses_profile();
        draft.id.clear();
        draft
            .native_ids
            .insert("goose".into(), "../../unexpected".into());

        let error = prepare_profile_draft(&settings, None, draft).unwrap_err();

        assert_eq!(
            error,
            "invalid_asset: Agent-native identities require explicit adoption"
        );
    }

    #[test]
    fn ordinary_profile_edit_preserves_the_adopted_native_identity() {
        let mut settings = crate::settings::Settings::default();
        let mut adopted = responses_profile();
        adopted
            .native_ids
            .insert("codex".into(), "legacy-provider".into());
        settings
            .model_profiles
            .get_or_insert_default()
            .insert(adopted.id.clone(), adopted.clone());
        let mut edited = adopted.clone();
        edited.model = "gpt-custom-new".into();
        edited
            .native_ids
            .insert("codex".into(), "redirected-provider".into());

        let prepared = prepare_profile_draft(&settings, Some(&adopted.id), edited).unwrap();

        assert_eq!(prepared.native_ids, adopted.native_ids);
        assert_eq!(prepared.model, "gpt-custom-new");
    }

    #[test]
    fn detects_agent_from_existing_config_without_shell_path() {
        let th = TestHome::new("model-agent-detect");
        fs::create_dir_all(th.home.join(".pi/agent")).unwrap();
        assert!(agent_installed(&[], &[".pi/agent"], &[]));
    }

    #[test]
    fn editing_profile_metadata_preserves_desired_assignments() {
        let _th = TestHome::new("model-profile-edit");
        let profile = responses_profile();
        save_profile(profile.clone(), None).unwrap();
        mutate_settings(|settings| {
            settings
                .model_assignments
                .get_or_insert_with(BTreeMap::new)
                .insert("codex".into(), profile.id.clone());
        })
        .unwrap();

        let mut changed = profile;
        changed.model = "gpt-custom-new".into();
        save_profile(changed, None).unwrap();

        assert_eq!(
            load_settings()
                .model_assignments
                .unwrap_or_default()
                .get("codex")
                .map(String::as_str),
            Some("team-openai")
        );
    }

    #[test]
    fn clearing_pi_does_not_create_a_missing_counterpart_file() {
        let th = TestHome::new("model-pi-clear-one-file");
        let models_path = th.home.join(".pi/agent/models.json");
        let settings_path = th.home.join(".pi/agent/settings.json");
        fs::create_dir_all(models_path.parent().unwrap()).unwrap();
        fs::write(
            &models_path,
            r#"{"providers":{"arca-team-openai":{"baseUrl":"https://gateway.example.test/v1","api":"openai-responses","models":[{"id":"gpt-custom","name":"Team OpenAI"}]}}}"#,
        )
        .unwrap();

        clear_pi(
            &models_path,
            &settings_path,
            &pi_profile("team-openai", "gpt-custom", "Team OpenAI"),
            true,
        )
        .unwrap();

        assert!(models_path.exists());
        let models: Value =
            serde_json::from_str(&fs::read_to_string(&models_path).unwrap()).unwrap();
        assert!(models["providers"].as_object().unwrap().is_empty());
        assert!(!settings_path.exists());
    }

    #[test]
    fn clearing_all_pi_models_removes_managed_and_external_entries() {
        let th = TestHome::new("model-pi-clear-all");
        let models_path = th.home.join(".pi/agent/models.json");
        let settings_path = th.home.join(".pi/agent/settings.json");
        fs::create_dir_all(models_path.parent().unwrap()).unwrap();
        fs::write(
            &models_path,
            r#"{
  "keep": { "theme": "dark" },
  "providers": {
    "arca-owned": { "models": [{ "id": "owned" }] },
    "manual": { "models": [{ "id": "external" }] }
  }
}"#,
        )
        .unwrap();
        fs::write(
            &settings_path,
            r#"{"theme":"dark","defaultProvider":"manual","defaultModel":"external"}"#,
        )
        .unwrap();

        clear_all_configured_models("pi").unwrap();

        let models: Value =
            serde_json::from_str(&fs::read_to_string(models_path).unwrap()).unwrap();
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(settings_path).unwrap()).unwrap();
        assert_eq!(models["providers"], serde_json::json!({}));
        assert_eq!(models["keep"]["theme"], "dark");
        assert!(settings.get("defaultProvider").is_none());
        assert!(settings.get("defaultModel").is_none());
        assert_eq!(settings["theme"], "dark");
        assert!(!agent_has_configured_models("pi").unwrap());
    }

    #[test]
    fn claude_patch_preserves_jsonc_and_unrelated_settings() {
        let th = TestHome::new("model-claude");
        let path = th.home.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  // keep this comment
  "permissions": { "allow": ["Bash(git:*)"] },
  "model": "old",
  "env": {
    "KEEP_ME": "yes",
    "ANTHROPIC_AUTH_TOKEN": "old-secret"
  }
}"#,
        )
        .unwrap();

        let (_, content) = prepare_claude(&path, &anthropic_profile(), true).unwrap();
        assert!(content.contains("keep this comment"));
        assert!(content.contains("KEEP_ME"));
        assert!(content.contains("permissions"));
        assert!(!content.contains("old-secret"));
        assert!(content.contains("apiKeyHelper"));
        assert!(content.contains("claude-sonnet-custom"));
    }

    #[test]
    fn claude_refuses_to_override_cloud_provider_routing() {
        let th = TestHome::new("model-claude-cloud");
        let path = th.home.join(".claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = r#"{"env":{"CLAUDE_CODE_USE_BEDROCK":"1"}}"#;
        fs::write(&path, original).unwrap();
        assert!(prepare_claude(&path, &anthropic_profile(), true).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn codex_patch_preserves_mcp_and_other_provider_tables() {
        let th = TestHome::new("model-codex");
        let path = th.home.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"# keep this comment
model = "old"

[mcp_servers.github]
url = "https://example.test/mcp"

[model_providers.existing]
name = "Existing"
base_url = "https://existing.test/v1"
wire_api = "responses"
"#,
        )
        .unwrap();

        let (_, content) = prepare_codex(&path, &responses_profile(), true).unwrap();
        assert!(content.contains("keep this comment"));
        assert!(content.contains("mcp_servers.github"));
        assert!(content.contains("model_providers.existing"));
        assert!(content.contains("model_providers.arca_7465616d2d6f70656e6169"));
        assert!(content.contains("find-generic-password"));
    }

    #[test]
    fn pi_patch_accepts_jsonc_and_preserves_other_providers() {
        let th = TestHome::new("model-pi");
        let path = th.home.join(".pi/agent/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  // user provider
  "providers": {
    "local": { "baseUrl": "http://localhost:8080", "models": [], },
  },
}"#,
        )
        .unwrap();

        let profile = pi_profile("team-openai", "gpt-custom", "Team OpenAI");
        let (_, content) = prepare_pi_models(&path, &profile, true).unwrap();
        assert!(content.contains("user provider"));
        assert!(content.contains("\"local\""));
        assert!(content.contains("\"openrouter\""));
        assert!(content.contains("/usr/bin/security"));
    }

    #[test]
    fn pi_generated_provider_id_uses_arca_provider_name_and_disambiguates_collisions() {
        let mut profile = responses_profile();
        profile.provider_id = Some("openrouter-provider".into());
        let settings = crate::settings::Settings {
            model_providers: Some(BTreeMap::from([
                (
                    "openrouter-provider".into(),
                    openrouter_provider("openrouter-provider", "Open Router"),
                ),
                (
                    "other-provider".into(),
                    openrouter_provider("other-provider", "Open-Router"),
                ),
            ])),
            ..Default::default()
        };

        assert_eq!(
            generated_pi_provider_id(&settings, &profile),
            "open-router-openrouter-provider"
        );

        let settings = crate::settings::Settings {
            model_providers: Some(BTreeMap::from([(
                "openrouter-provider".into(),
                openrouter_provider("openrouter-provider", "OpenRouter"),
            )])),
            ..Default::default()
        };
        assert_eq!(generated_pi_provider_id(&settings, &profile), "openrouter");
    }

    #[test]
    fn pi_materialization_preserves_an_explicit_adopted_native_provider_id() {
        let mut profile = responses_profile();
        profile
            .native_ids
            .insert("pi".into(), "my-existing-provider".into());

        let materialized = materialize_profile_for_agent("pi", &profile).unwrap();

        assert_eq!(
            materialized.native_ids.get("pi").map(String::as_str),
            Some("my-existing-provider")
        );
    }

    #[test]
    fn pi_active_model_observation_uses_provider_and_model_identity() {
        let th = TestHome::new("model-pi-active-shared-provider");
        let settings_path = th.home.join(".pi/agent/settings.json");
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(
            &settings_path,
            r#"{"defaultProvider":"openrouter","defaultModel":"second-model"}"#,
        )
        .unwrap();
        let provider = openrouter_provider("openrouter-provider", "OpenRouter");
        let mut first = responses_profile();
        first.id = "first".into();
        first.model = "first-model".into();
        first.provider_id = Some(provider.id.clone());
        let mut second = responses_profile();
        second.id = "second".into();
        second.model = "second-model".into();
        second.provider_id = Some(provider.id.clone());
        let settings = crate::settings::Settings {
            model_providers: Some(BTreeMap::from([(provider.id.clone(), provider)])),
            model_profiles: Some(BTreeMap::from([
                (first.id.clone(), first),
                (second.id.clone(), second),
            ])),
            ..Default::default()
        };

        assert_eq!(
            observe_active_model_for_settings(&settings, "pi"),
            ObservedActiveModel::Managed("second".into())
        );
    }

    #[test]
    fn pi_patch_preserves_comments_and_unknown_fields_inside_a_shared_provider() {
        let th = TestHome::new("model-pi-preserve-shared-provider");
        let path = th.home.join(".pi/agent/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
  "providers": {
    "openrouter": {
      // keep provider comment
      "baseUrl": "https://gateway.example.test/v1",
      "api": "openai-responses",
      "policy": "keep-provider-field",
      "models": [{
        // keep model comment
        "id": "gpt-custom",
        "name": "Old Name",
        "custom": "keep-model-field"
      }]
    }
  }
}"#,
        )
        .unwrap();

        let profile = pi_profile("team-openai", "gpt-custom", "Team OpenAI");
        let (_, content) = prepare_pi_models(&path, &profile, false).unwrap();

        assert!(content.contains("keep provider comment"));
        assert!(content.contains("keep model comment"));
        assert!(content.contains("keep-provider-field"));
        assert!(content.contains("keep-model-field"));
        assert!(content.contains("Team OpenAI"));
    }

    #[test]
    fn pi_provider_keeps_sibling_models_when_upserting_and_clearing_one_profile() {
        let th = TestHome::new("model-pi-shared-provider");
        let path = th.home.join(".pi/agent/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let first = pi_profile("first", "first-model", "First");
        let second = pi_profile("second", "second-model", "Second");

        let (_, first_content) = prepare_pi_models(&path, &first, false).unwrap();
        fs::write(&path, first_content).unwrap();
        let (_, second_content) = prepare_pi_models(&path, &second, false).unwrap();
        fs::write(&path, second_content).unwrap();

        let models: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            models["providers"]["openrouter"]["models"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let (_, cleared) = prepare_clear_pi_models(&path, &first).unwrap();
        let models: Value = serde_json::from_str(&cleared).unwrap();
        assert_eq!(
            models["providers"]["openrouter"]["models"],
            serde_json::json!([{ "id": "second-model", "name": "Second", "input": ["text"], "reasoning": true, "contextWindow": 128000, "maxTokens": 16000 }])
        );
    }

    #[test]
    fn pi_write_migrates_the_matching_legacy_arca_profile_provider() {
        let th = TestHome::new("model-pi-legacy-provider");
        let path = th.home.join(".pi/agent/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"providers":{"arca-team-openai":{"baseUrl":"https://gateway.example.test/v1","models":[{"id":"gpt-custom","name":"Team OpenAI"}]}}}"#,
        )
        .unwrap();

        let profile = pi_profile("team-openai", "gpt-custom", "Team OpenAI");
        let (_, content) = prepare_pi_models(&path, &profile, false).unwrap();
        let models: Value = serde_json::from_str(&content).unwrap();
        assert!(models["providers"].get("arca-team-openai").is_none());
        assert_eq!(
            models["providers"]["openrouter"]["models"][0]["id"],
            "gpt-custom"
        );
    }

    #[test]
    fn pi_backup_profile_does_not_change_external_defaults() {
        let th = TestHome::new("model-pi-backup");
        let models_path = th.home.join(".pi/agent/models.json");
        let settings_path = th.home.join(".pi/agent/settings.json");
        fs::create_dir_all(models_path.parent().unwrap()).unwrap();
        fs::write(
            &settings_path,
            r#"{"defaultProvider":"external","defaultModel":"external-model","keep":true}"#,
        )
        .unwrap();
        let before = fs::read_to_string(&settings_path).unwrap();

        let result = apply_pi(
            &models_path,
            &settings_path,
            &pi_profile("team-openai", "gpt-custom", "Team OpenAI"),
            Some("!/usr/bin/security".into()),
            false,
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&settings_path).unwrap(), before);
        assert!(fs::read_to_string(&models_path)
            .unwrap()
            .contains("\"openrouter\""));
        assert_eq!(result.files, vec![models_path.display().to_string()]);
        assert!(result.message.contains("without changing the default"));
    }

    #[test]
    fn qoder_desktop_and_cli_are_managed_while_ide_keeps_its_guide() {
        let _home = TestHome::new("qoder-model-guides");
        let agents = list_agents();
        for (id, name, docs) in [
            ("qoder", "Qoder", QODER_DOCS),
        ] {
            let agent = agents.iter().find(|agent| agent.id == id).unwrap();
            assert_eq!(agent.name, name);
            assert_eq!(agent.docs, docs);
            assert_eq!(agent.mode, "guided");
            assert!(agent.config_paths.is_empty());
            assert!(agent.credential_capabilities.native_sources.is_empty());
            assert!(!agent.credential_capabilities.plaintext);
            assert!(!agent.credential_capabilities.agent_store);
            for protocol in [ModelProtocol::OpenaiCompletions, ModelProtocol::AnthropicMessages] {
                assert!(ensure_supported(id, &protocol).is_err());
            }
        }
        let cli = agents.iter().find(|agent| agent.id == "qoder-cli").unwrap();
        assert_eq!(cli.mode, "managed");
        assert_eq!(cli.config_paths, vec!["~/.qoder/settings.json"]);
        assert!(cli.supports_multiple && cli.supports_global_selection);
        assert_eq!(cli.credential_capabilities.native_sources, vec!["env"]);
        assert!(!cli.credential_capabilities.plaintext);
        assert!(credential::select_delivery("qoder-cli", &ApiKeySource::ArcaStore, &crate::domain::assets::ApiKeyDelivery::Auto).is_err());
        assert!(credential::select_delivery("qoder-cli", &ApiKeySource::Env { name: "QODER_FIXTURE_KEY".into() }, &crate::domain::assets::ApiKeyDelivery::Auto).is_ok());
        for protocol in [ModelProtocol::OpenaiCompletions, ModelProtocol::OpenaiResponses, ModelProtocol::AnthropicMessages] {
            assert!(ensure_supported("qoder-cli", &protocol).is_ok());
        }
        assert!(ensure_supported("qoder-cli", &ModelProtocol::GeminiGenerateContent).is_err());
        let desktop = agents.iter().find(|agent| agent.id == "qoder-desktop").unwrap();
        assert!(desktop.note.contains("0.1.8"));
        assert_eq!(desktop.mode, "managed");
        assert_eq!(desktop.config_paths, vec!["~/.qoder/settings.json"]);
        assert!(desktop.supports_multiple);
        assert!(!desktop.supports_global_selection);
        assert!(desktop.credential_capabilities.plaintext);
        assert_eq!(desktop.credential_capabilities.native_sources, vec!["env"]);
        for protocol in [ModelProtocol::OpenaiCompletions, ModelProtocol::OpenaiResponses, ModelProtocol::AnthropicMessages] {
            assert!(ensure_supported("qoder-desktop", &protocol).is_ok());
        }
        assert!(ensure_supported("qoder-desktop", &ModelProtocol::GeminiGenerateContent).is_err());
    }

    #[test]
    fn qoder_clear_all_blocks_shared_consumers_but_allows_separate_config_paths() {
        let _home = TestHome::new("qoder-shared-clear");
        let mut settings = crate::settings::Settings::default();
        settings.model_assignments = Some(BTreeMap::from([("qoder-desktop".into(), "fixture".into())]));
        assert!(guard_shared_model_clear(&settings, "qoder-cli").is_err());
        settings.agent_config_paths = Some(BTreeMap::from([("qoder-cli".into(), crate::settings::AgentConfigPathOverride {
            model_paths: Some(vec!["~/.qoder-cli-isolated/settings.json".into()]),
            ..Default::default()
        })]));
        assert!(guard_shared_model_clear(&settings, "qoder-cli").is_ok());
    }

    #[test]
    fn protocol_matrix_rejects_incompatible_assignments() {
        assert!(ensure_supported("claude-code", &ModelProtocol::OpenaiResponses).is_err());
        assert!(ensure_supported("codex", &ModelProtocol::AnthropicMessages).is_err());
        assert!(ensure_supported("pi", &ModelProtocol::AnthropicMessages).is_ok());
        assert!(ensure_supported("qoder", &ModelProtocol::OpenaiResponses).is_err());
        assert!(ensure_supported("grok-build", &ModelProtocol::AnthropicMessages).is_ok());
        assert!(ensure_supported("grok-build", &ModelProtocol::OpenaiResponses).is_ok());
        assert!(ensure_supported("grok-build", &ModelProtocol::OpenaiCompletions).is_ok());
    }

    #[test]
    fn grok_build_is_a_managed_three_protocol_target() {
        let _th = TestHome::new("model-grok-build");
        let agents = list_agents();
        let grok = agents
            .iter()
            .find(|agent| agent.id == "grok-build")
            .expect("Grok Build model target");
        assert_eq!(grok.mode, "managed");
        assert_eq!(grok.config_path, "~/.grok/config.toml");
        assert_eq!(grok.supported_protocols.len(), 3);
        assert!(grok.docs.starts_with("https://docs.x.ai/build/settings"));
    }

    #[test]
    fn grok_build_patch_preserves_unrelated_config_and_uses_env_key() {
        let th = TestHome::new("model-grok-build-patch");
        let path = th.home.join(".grok/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"# keep Grok settings
[models]
default = "private"
web_search = "grok-4.5"

[model.private]
model = "private-model"
base_url = "https://private.invalid/v1"
api_backend = "responses"

[mcp_servers.keep]
command = "keep-mcp"

[permission]
allow = ["Read(**)"]

[ui]
max_thoughts_width = 120
fork_secondary_model = "private-fork"
"#,
        )
        .unwrap();
        let mut profile = responses_profile();
        profile.provider_id = Some("grok-provider".into());
        profile.env_key = Some("TEAM_OPENAI_API_KEY".into());

        let (_, content) = prepare_grok_build(&path, &profile).unwrap();

        assert!(content.contains("keep Grok settings"));
        assert!(content.contains("web_search = \"grok-4.5\""));
        assert!(content.contains("model.private"));
        assert!(content.contains("mcp_servers.keep"));
        assert!(content.contains("permission"));
        assert!(content.contains("default = \"arca_7465616d2d6f70656e6169\""));
        assert!(content.contains("model.arca_7465616d2d6f70656e6169"));
        assert!(content.contains("api_backend = \"responses\""));
        assert!(content.contains("env_key = \"TEAM_OPENAI_API_KEY\""));
        assert!(content.contains("fork_secondary_model = \"private-fork\""));
        assert!(content.contains("max_thoughts_width = 120"));
        assert!(content.contains("context_window = 128000"));
        assert!(content.contains("max_completion_tokens = 16000"));
        assert!(!content.contains("api_key ="));
    }

    #[test]
    fn grok_build_apply_and_clear_manage_only_the_arca_model() {
        let th = TestHome::new("model-grok-build-apply");
        let path = th.home.join(".grok/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[models]\nweb_search = \"grok-4.5\"\n\n[model.private]\nmodel = \"keep\"\n",
        )
        .unwrap();
        let mut profile = responses_profile();
        profile.provider_id = Some("grok-provider".into());
        profile.env_key = Some("TEAM_OPENAI_API_KEY".into());
        save_profile(profile.clone(), None).unwrap();

        apply_profile("grok-build", &profile.id).unwrap();
        assert_eq!(
            load_settings()
                .model_assignments
                .as_ref()
                .and_then(|assignments| assignments.get("grok-build"))
                .map(String::as_str),
            Some(profile.id.as_str())
        );
        assert_eq!(
            observe_profile("grok-build", &profile).unwrap(),
            ModelObservedState::Synced
        );

        clear_profile("grok-build", &profile.id).unwrap();
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("model.private"));
        assert!(content.contains("web_search = \"grok-4.5\""));
        assert!(!content.contains("mux_7465616d2d6f70656e6169"));
        assert!(!content.contains("fork_secondary_model"));
    }

    #[test]
    fn grok_build_apply_rejects_a_keychain_only_credential() {
        let _th = TestHome::new("model-grok-build-keychain-only");
        let profile = responses_profile();
        save_profile(profile.clone(), Some("secret".into())).unwrap();

        let error = apply_profile("grok-build", &profile.id).unwrap_err();

        assert!(error.starts_with("grok_build_env_key_required:"));
        assert!(!config_path(".grok/config.toml").exists());
        assert!(!load_settings()
            .model_assignments
            .unwrap_or_default()
            .contains_key("grok-build"));
    }

    #[test]
    fn grok_build_reports_an_unmanaged_current_model_as_external() {
        let th = TestHome::new("model-grok-build-external");
        let path = th.home.join(".grok/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "[models]\ndefault = \"private\"\n\n[model.private]\nmodel = \"private-model\"\n",
        )
        .unwrap();

        assert_eq!(
            observe_external_model("grok-build").unwrap(),
            ExternalModelObservedState::Present
        );
    }

    #[test]
    fn model_profile_rejects_invalid_environment_variable_name() {
        let mut profile = responses_profile();
        profile.env_key = Some("not-valid-key".into());
        assert!(validate_profile(&profile).is_err());
    }

    #[test]
    fn kimi_clients_expose_native_model_guides_without_credential_writers() {
        let _home = TestHome::new("kimi-model-guides");
        let agents = list_agents();
        for id in ["kimi-code", "kimi-code-desktop"] {
            let agent = agents.iter().find(|agent| agent.id == id).unwrap();
            assert_eq!(agent.mode, "guided");
            assert_eq!(agent.storage_authority, ModelStorageAuthority::Guided);
            assert_eq!(agent.config_paths, vec!["~/.kimi-code/config.toml"]);
            assert!(agent.assigned_profiles.is_empty());
            assert!(agent.credential_capabilities.native_sources.is_empty());
            assert!(!agent.credential_capabilities.plaintext);
            assert!(!agent.credential_capabilities.arca_keychain_helper);
            for protocol in &agent.supported_protocols {
                assert!(ensure_supported(id, protocol).is_err());
            }
        }
    }

    #[test]
    fn minimax_code_is_a_guided_three_protocol_target() {
        let _th = TestHome::new("model-minimax-code");
        let agents = list_agents();
        let minimax = agents
            .iter()
            .find(|agent| agent.id == "minimax-code")
            .expect("MiniMax Code model target");
        assert_eq!(minimax.mode, "guided");
        assert_eq!(minimax.config_path, "~/.mavis/config.yaml");
        assert_eq!(minimax.supported_protocols.len(), 3);
        assert!(minimax.docs.contains("agent.minimax.io/download"));
        assert!(ensure_supported("minimax-code", &ModelProtocol::OpenaiResponses).is_err());
    }

    #[test]
    fn isolated_apply_preserves_codex_mcp_and_records_assignment() {
        let th = TestHome::new("model-codex-apply");
        let path = th.home.join(".codex/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"model = "old"

[mcp_servers.keep]
url = "https://example.test/mcp"
"#,
        )
        .unwrap();
        save_profile(responses_profile(), None).unwrap();

        let result = apply_profile("codex", "team-openai").unwrap();

        assert_eq!(result.files, vec![path.display().to_string()]);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("mcp_servers.keep"));
        assert!(content.contains("model_providers.arca_7465616d2d6f70656e6169"));
        assert!(!content.contains("find-generic-password"));
        assert_eq!(
            load_settings()
                .model_assignments
                .unwrap()
                .get("codex")
                .map(String::as_str),
            Some("team-openai")
        );
        assert!(fs::read_dir(th.home.join(".arca/backups"))
            .unwrap()
            .next()
            .is_some());
    }

    #[test]
    fn pi_transaction_rolls_back_first_file_when_second_changed() {
        let th = TestHome::new("model-pi-rollback");
        let models = th.home.join("models.json");
        let settings = th.home.join("settings.json");
        fs::write(&models, "models-old").unwrap();
        fs::write(&settings, "settings-newer").unwrap();

        let error = write_pi_transaction(
            &models,
            Some("models-old"),
            "models-arca",
            &settings,
            Some("settings-old"),
            "settings-arca",
        )
        .unwrap_err();

        assert!(error.contains("rolled back"));
        assert_eq!(fs::read_to_string(models).unwrap(), "models-old");
        assert_eq!(fs::read_to_string(settings).unwrap(), "settings-newer");
    }

    #[test]
    fn codex_provider_ids_do_not_collapse_profile_punctuation() {
        assert_ne!(arca_profile_id("a-b"), arca_profile_id("a_b"));
        assert_ne!(arca_profile_id("a.b"), arca_profile_id("a_b"));
    }

    #[test]
    fn private_transaction_keychain_snapshot_round_trips_without_plaintext_payload() {
        let home = TestHome::new("private-transaction-keychain-roundtrip");
        let operation_id = "11111111-1111-4111-8111-111111111111";
        let path = home
            .home
            .join("Library/Application Support/Claude-3p/configLibrary/private-profile.json");
        let sentinel = b"{\n  \"inferenceGatewayApiKey\": \"PRIVATE-SNAPSHOT-SENTINEL\"\n}\n";
        let content_hash = hex::encode(Sha256::digest(sentinel));

        let subject = persist_private_file_snapshot(operation_id, &path, sentinel).unwrap();
        let stored = read_credential_service_checked(&subject).unwrap().unwrap();
        assert!(!stored
            .windows(b"PRIVATE-SNAPSHOT-SENTINEL".len())
            .any(|window| window == b"PRIVATE-SNAPSHOT-SENTINEL"));
        assert!(!stored.contains(&b'\n'));

        let restored = load_private_file_snapshot(operation_id, &path, &content_hash)
            .unwrap()
            .unwrap();
        assert_eq!(restored.as_slice(), sentinel);
        clear_private_file_snapshot(operation_id, &path).unwrap();
        assert!(!private_file_snapshot_exists(operation_id, &path));
    }

    #[test]
    fn private_transaction_production_credential_backend_is_compile_time_gated() {
        let source = include_str!("mod.rs");
        let declaration = source
            .find("static TEST_CREDENTIALS")
            .expect("test credential backend declaration");
        let cfg_prefix = &source[declaration.saturating_sub(160)..declaration];

        assert!(
            cfg_prefix.contains("cfg(any(test, debug_assertions))"),
            "the in-memory credential map must be absent from optimized release artifacts"
        );
        assert!(
            !include_str!("../../../Cargo.toml").contains("test-credential-backend"),
            "a production-selectable Cargo feature must not enable the fake credential backend"
        );
        let presence_override = source
            .find("ARCA_TEST_MODEL_CREDENTIAL_PROFILES")
            .expect("test credential-presence override");
        let cfg_prefix = &source[presence_override.saturating_sub(220)..presence_override];
        assert!(
            cfg_prefix.contains("cfg(any(test, debug_assertions))"),
            "the test credential-presence override must be absent from optimized release artifacts"
        );
    }

    #[test]
    fn private_transaction_keychain_snapshot_rejects_corrupt_or_rebound_payload() {
        let home = TestHome::new("private-transaction-keychain-binding");
        let operation_id = "22222222-2222-4222-8222-222222222222";
        let first = home.home.join("first-private.json");
        let second = home.home.join("second-private.json");
        let sentinel = b"PRIVATE-SNAPSHOT-SENTINEL";
        let content_hash = hex::encode(Sha256::digest(sentinel));
        let first_subject = persist_private_file_snapshot(operation_id, &first, sentinel).unwrap();
        let payload = read_credential_service_checked(&first_subject).unwrap().unwrap();
        let second_subject = private_file_snapshot_subject(operation_id, &second);
        set_credential_service(&second_subject, &payload).unwrap();

        let rebound = load_private_file_snapshot(operation_id, &second, &content_hash).unwrap_err();
        assert!(
            rebound.starts_with("private_file_snapshot_invalid:"),
            "{rebound}"
        );
        assert!(!rebound.contains("PRIVATE-SNAPSHOT-SENTINEL"));

        set_credential_service(&first_subject, b"not-a-valid-private-snapshot").unwrap();
        let corrupt = load_private_file_snapshot(operation_id, &first, &content_hash).unwrap_err();
        assert!(
            corrupt.starts_with("private_file_snapshot_invalid:"),
            "{corrupt}"
        );
        assert!(!corrupt.contains("PRIVATE-SNAPSHOT-SENTINEL"));
        clear_private_file_snapshot(operation_id, &first).unwrap();
        clear_private_file_snapshot(operation_id, &second).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "touches the current user's macOS Keychain"]
    fn macos_keychain_helper_round_trip() {
        let profile_id = format!("arca-smoke-{}", std::process::id());
        let _ = delete_credential(&profile_id);
        set_credential(&profile_id, b"arca-smoke-value").unwrap();
        let actual = read_credential_checked(&profile_id).unwrap();
        let helper = std::process::Command::new("/usr/bin/security")
            .args(&security_command(&profile_id)[1..])
            .output()
            .unwrap();
        let cleanup = delete_credential(&profile_id);
        assert_eq!(actual.as_deref(), Some(b"arca-smoke-value".as_slice()));
        assert!(helper.status.success());
        assert_eq!(
            String::from_utf8(helper.stdout).unwrap().trim(),
            "arca-smoke-value"
        );
        cleanup.unwrap();
        assert!(!credential_exists(&profile_id));
    }
    #[test]
    fn opencode_desktop_supports_models_and_guards_the_shared_registry() {
        let _home = TestHome::new("opencode-desktop-capability");
        let agents = list_agents();
        let desktop = agents.iter().find(|agent| agent.id == "opencode-desktop").unwrap();
        assert_eq!(desktop.mode, "managed");
        assert!(desktop.supports_multiple && desktop.supports_global_selection);
        assert_eq!(desktop.config_paths, vec!["~/.config/opencode/opencode.json"]);
        assert_eq!(desktop.supported_protocols.len(), 4);
        assert_eq!(credential::select_delivery("opencode-desktop", &ApiKeySource::ArcaStore,
            &crate::domain::assets::ApiKeyDelivery::Auto).unwrap(), credential::PreparedCredentialRoute::OpenCodeAuthStore);
        for (agent, sibling) in [("opencode", "opencode-desktop"), ("opencode-desktop", "opencode")] {
            let mut settings = crate::settings::Settings::default();
            settings.model_assignments = Some(BTreeMap::from([(sibling.into(), "fixture".into())]));
            assert!(guard_shared_model_clear(&settings, agent).unwrap_err().starts_with("opencode_shared_registry:"));
            settings.agent_config_paths = Some(BTreeMap::from([(agent.into(), crate::settings::AgentConfigPathOverride {
                model_paths: Some(vec!["~/.isolated/opencode.json".into()]),
                ..Default::default()
            })]));
            assert!(guard_shared_model_clear(&settings, agent).is_ok());
        }
    }

    #[test]
    fn kilo_desktop_has_separate_config_and_reference_only_credentials() {
        let _home = TestHome::new("kilo-desktop-capability");
        let agents = list_agents();
        let desktop = agents.iter().find(|agent| agent.id == "kilo-desktop").unwrap();
        assert_eq!(desktop.mode, "managed");
        assert!(desktop.supports_multiple && !desktop.supports_global_selection);
        assert_ne!(desktop.config_paths, default_config_paths("kilo-code").unwrap());
        assert_eq!(desktop.config_path, crate::agents::builtin_agents()["kilo-desktop"].global.clone().unwrap());
        assert!(!desktop.credential_capabilities.plaintext);
        assert_eq!(desktop.credential_capabilities.native_sources, vec!["env", "file"]);
        let mut settings = crate::settings::Settings::default();
        settings.model_assignments = Some(BTreeMap::from([("kilo-code".into(), "fixture".into())]));
        assert!(guard_shared_model_clear(&settings, "kilo-desktop").is_ok());
        settings.agent_config_paths = Some(BTreeMap::from([("kilo-desktop".into(), crate::settings::AgentConfigPathOverride {
            model_paths: default_config_paths("kilo-code"), ..Default::default()
        })]));
        assert!(guard_shared_model_clear(&settings, "kilo-desktop").unwrap_err().starts_with("kilo_shared_registry:"));
    }

}
