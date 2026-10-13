# Provider Catalog sources

## GMI Cloud — 2026-10-09

`gmi-cloud` is a separate gateway Provider with Chat Completions at `https://api.gmi-serving.com/v1/chat/completions`, Responses at `https://api.gmi-serving.com/v1/responses`, and Anthropic Messages at `https://api.gmi-serving.com/v1/messages`. Combining the documented protocol bases gives the normalized Arca Base URL `https://api.gmi-serving.com` and paths `/v1/chat/completions`, `/v1/responses`, and `/v1/messages`.

Official sources: [LLM API](https://docs.gmicloud.ai/inference-engine/api-reference/llm-api-reference), [Codex integration](https://docs.gmicloud.ai/coding-tools/codex), [Claude Code integration](https://docs.gmicloud.ai/coding-tools/claude-code), and [API key guide](https://docs.gmicloud.ai/cluster-engine/user-management/api-keys). The guide links `https://console.gmicloud.ai/user-setting/api-keys`; its unauthenticated redirect preserves the key-page destination behind sign-in.

Model discovery uses the documented authenticated `GET /v1/models`, with the existing OpenAI catalog decoder and required Bearer credential policy. An unauthenticated request returned HTTP 401 (`API key is required`). Users supply their own credential source; no account, key, or saved connection is created by adding this template.

[Qwen's announcement](https://x.com/Alibaba_Qwen/status/2108401307577090259) advertises temporary access to Qwen3.8 Max/Flash and Wan3.0. The published text catalog does not yet establish the two new chat model IDs. Load exact IDs from the account's live catalog or enter a verified ID manually; do not hard-code guessed spellings or permanent free pricing. Wan3.0 uses the separate video API and is not an Agent chat profile. GMI's router API is also separate from these protocol endpoints.

## Official credential portals — 2026-10-04

`data/provider-links.json` is the default link catalog for all 102 built-in Provider templates (103 templates including Custom). Each entry retains `docs_url` and adds a typed `portal` containing a credential-free HTTPS URL and one of `api-key`, `console`, or `setup`. Core validates coverage, known Provider IDs, URL schemes, and absence of embedded credentials before serializing it. Custom and unknown connections have no default vendor link; no website is inferred from the user's Base URL.

The desktop's selected Provider banner, picker selection footer, and editor form use a core-provided portal: **Get API Key**, **Open console**, or **Setup guide**. Key-page entries include API Tokens and Access Tokens; account/region/subscription consoles are explicitly labeled as consoles, not promised as direct key pages. Ollama, LM Studio, and vLLM are local connection templates and retain setup guides rather than unrelated cloud account keys. The editor shows the effective address next to a browser-open action and can restore the template default. URLs open with the native system-browser opener without selecting a template, submitting a form, generating a key, or modifying a saved connection. Opener failures use the existing toast UI.

Each saved Provider connection can persist an optional `portal_url` override. Core accepts only a credential-free HTTPS URL without a fragment and resolves the effective portal from the override or template. CLI Provider list/show and desktop use that same resolved value; templates retain their defaults. `arca model provider docs` and `docs_url` keep their documentation semantics. The portal URL is not exported to Agent configuration, used for model discovery, or treated as an inference endpoint. API credentials remain in Keychain.

Portal sources are the official key pages and linked quickstarts reviewed on this date. Login redirects and JavaScript-only shells confirm an account entry, not successful authenticated key creation. Stable console entries are used when an authenticated resource/team/plan selection prevents verification of a direct key route. Temporary OAuth URLs, guessed team IDs, referral codes, and credentials are never recorded.

Notable evidence and mapping decisions:

| Provider family | Official reference / observed entry | Decision |
|---|---|---|
| OpenRouter | [Account key page](https://openrouter.ai/workspaces/default/keys) | Use the current workspace key route; all models of a Provider use the same account entry. |
| Mistral | [API key guide](https://docs.mistral.ai/admin/identity-access/api-keys) | Follow the guide's `admin.mistral.ai/plateforme/api-keys` link rather than a model documentation page. |
| Anthropic | [Claude Platform keys](https://platform.claude.com/settings/keys) | Use the current Claude Platform account domain. |
| Moonshot | [China keys](https://platform.moonshot.cn/console/api-keys), [Global keys](https://platform.moonshot.ai/console/api-keys) | Store the observed `platform.kimi.com` / `platform.kimi.ai` destinations separately. Kimi Code keeps its membership console. |
| Alibaba | [Coding Plan](https://help.aliyun.com/zh/model-studio/coding-plan), [Token Plan CN](https://help.aliyun.com/zh/model-studio/token-plan-personal-quick-start), [Token Plan Global](https://www.alibabacloud.com/help/en/model-studio/token-plan-team-quickstart) | PAYG keys, Coding Plan, and Token Plan have distinct portals; retain China and international account separation. |
| Xiaomi MiMo | [API integration FAQ](https://mimo.mi.com/docs/zh-CN/quick-start/faq/api-integration) | Official links enter the platform console. PAYG uses API Keys; regional Token Plans use the subscription's key, not the PAYG key. |
| MiniMax | [Global access](https://platform.minimax.io/console/access), [Token Plan](https://platform.minimax.io/docs/token-plan/intro) | PAYG access and plan console remain distinct; preserve CN/Global account domains. |
| StepFun | [Global plan quickstart](https://platform.stepfun.ai/docs/en/step-plan/quick-start), [China keys](https://platform.stepfun.com/interface-key) | The official quickstart links to `interface-key`; preserve each region. |
| Tencent | [Coding Plan FAQ](https://cloud.tencent.com/document/product/1823/130103), [Global Token Plan](https://intl.cloud.tencent.com/document/product/1300/81315) | Use regional TokenHub consoles; Coding and Token Plan keys are separate subscription credentials. |
| Baidu | [Plan entry](https://cloud.baidu.com/product/codingplan) | The official plan link opens `qianfan/resource/token-plan`; ordinary API keys use IAM. |
| DigitalOcean | [Manage model access keys](https://docs.digitalocean.com/products/inference/how-to/manage-model-access-keys/) | Follow the official `model-studio/manage-keys` link, not an Agent endpoint key. |
| Weights & Biases | [Official SDK](https://github.com/wandb/wandb) | The documented account settings entry redirects to `forge.coreweave.com/wandb/settings`. |
| Token Harbor | [Official CLI connection guide](https://tokenharbor.ai/docs/connect) | Add the formerly missing template entry; the guide identifies `/dashboard/api-keys`. |
| Routeway / Infron / APInex | [Routeway dashboard](https://routeway.ai/dashboard), [Infron quickstart](https://infron.ai/docs), [APInex platform](https://apinex.bond/) | Infron documents `/dashboard/apiKeys`; Routeway and APInex use official account consoles where a deeper key route could not be verified. |

## Provider expansion — 2026-10-04

Added 30 reviewed templates: Portkey, Helicone AI Gateway, PPIO, Infini AI GenStudio, AIHubMix, 302.AI, OpenCode Zen/Go, Kilo Gateway, NanoGPT, Synthetic, Chutes, Featherless AI, Venice, FriendliAI, Inference.net, OVHcloud AI Endpoints, Upstage, AI21 Labs, ZenMux, BytePlus ModelArk, Tencent TokenHub China/Global, LongCat, iFLYTEK Spark X2, Nscale, io.net, Inception, Ollama Cloud, and the official Tinfoil local proxy. The [research and endpoint matrix](../../../../docs/provider-gap-research-2026-10-04.md) records protocol references, public catalog observations, manual discovery boundaries, and excluded or retired candidates. Seventeen templates enable reviewed model discovery; the remaining thirteen require a model ID or an explicit catalog URL. No stored user connection or credential is rewritten.

## Documentation references — 2026-09-21

URLs were checked against the official pages on this date, using the existing source matrices and models.dev for discovery only. Regions and Coding/Token Plans retain their specific documentation. Direct requests to some vendors encountered network-policy or anti-bot pages; official page retrieval separately confirmed MiniMax CN, Kimi Code, Requesty, Together AI, Scaleway and StepFun Global. Network interception URLs are not stored in the catalog. MiMo Token Plan uses the current `/tokenplan/Token%20Plan/quick-access` route rather than the outdated `/price/tokenplan/quick-access` route.

Plan-specific Provider templates were audited on 2026-07-28 against
`https://models.dev/api.json` for discovery and the vendors' official
documentation for authority. A Coding/Token Plan is a separate connection:
its API key and endpoint must not be mixed with the vendor's pay-as-you-go API.

| Arca Provider ID | OpenAI-compatible Base URL | Anthropic-compatible Base URL | Official source |
|---|---|---|---|
| `zai-coding-plan` | `https://api.z.ai/api/coding/paas/v4` | `https://api.z.ai/api/anthropic` | <https://docs.z.ai/devpack/quick-start> |
| `zhipuai-coding-plan` | `https://open.bigmodel.cn/api/coding/paas/v4` | `https://open.bigmodel.cn/api/anthropic` | <https://docs.bigmodel.cn/cn/coding-plan/quick-start> |
| `alibaba-coding-plan-cn` | `https://coding.dashscope.aliyuncs.com/v1` | `https://coding.dashscope.aliyuncs.com/apps/anthropic` | <https://help.aliyun.com/zh/model-studio/coding-plan> |
| `alibaba-coding-plan` | `https://coding-intl.dashscope.aliyuncs.com/v1` | `https://coding-intl.dashscope.aliyuncs.com/apps/anthropic` | <https://www.alibabacloud.com/help/en/model-studio/coding-plan> |
| `alibaba-token-plan-cn` | `https://token-plan.cn-beijing.maas.aliyuncs.com/compatible-mode/v1` | `https://token-plan.cn-beijing.maas.aliyuncs.com/apps/anthropic` | <https://help.aliyun.com/zh/model-studio/token-plan-personal-quick-start> |
| `alibaba-token-plan` | `https://token-plan.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1` | `https://token-plan.ap-southeast-1.maas.aliyuncs.com/apps/anthropic` | <https://www.alibabacloud.com/help/en/model-studio/token-plan-quickstart> |
| `xiaomi-token-plan-cn` | `https://token-plan-cn.xiaomimimo.com/v1` | `https://token-plan-cn.xiaomimimo.com/anthropic` | <https://mimo.mi.com/docs/zh-CN/price/tokenplan/quick-access> |
| `xiaomi-token-plan-sgp` | `https://token-plan-sgp.xiaomimimo.com/v1` | `https://token-plan-sgp.xiaomimimo.com/anthropic` | <https://mimo.mi.com/docs/zh-CN/price/tokenplan/quick-access> |
| `xiaomi-token-plan-ams` | `https://token-plan-ams.xiaomimimo.com/v1` | `https://token-plan-ams.xiaomimimo.com/anthropic` | <https://mimo.mi.com/docs/zh-CN/price/tokenplan/quick-access> |
| `kimi-for-coding` | `https://api.kimi.com/coding/v1` | `https://api.kimi.com/coding` | <https://www.kimi.com/code/docs/en/> |
| `minimax-coding-plan` | `https://api.minimax.io/v1` | `https://api.minimax.io/anthropic` | <https://platform.minimax.io/docs/token-plan/quickstart> |
| `minimax-cn-coding-plan` | `https://api.minimax.cn/v1` | `https://api.minimax.cn/anthropic` | <https://platform.minimaxi.com/docs/token-plan/quickstart> |
| `stepfun-step-plan` | `https://api.stepfun.com/step_plan/v1` | `https://api.stepfun.com/step_plan` | <https://platform.stepfun.com/docs/zh/step-plan/quick-start> |
| `stepfun-ai-step-plan` | `https://api.stepfun.ai/step_plan/v1` | `https://api.stepfun.ai/step_plan` | <https://platform.stepfun.ai/docs/en/step-plan/quick-start> |
| `tencent-coding-plan` | `https://api.lkeap.cloud.tencent.com/coding/v3` | `https://api.lkeap.cloud.tencent.com/coding/anthropic` | <https://cloud.tencent.com/document/product/1823/130092> |
| `tencent-token-plan` | `https://api.lkeap.cloud.tencent.com/plan/v3` | `https://api.lkeap.cloud.tencent.com/plan/anthropic` | <https://cloud.tencent.com/document/product/1823/130060> |
| `tencent-token-plan-global` | `https://tokenhub-intl.tencentcloudmaas.com/plan/v3` | `https://tokenhub-intl.tencentcloudmaas.com/plan/anthropic` | <https://intl.cloud.tencent.com/document/product/1300/81315> |

Many plan products are restricted to interactive coding tools and supported
agents. Arca only stores the user-selected connection and credentials locally;
users remain responsible for the vendor's plan eligibility and usage policy.

The China MiniMax template endpoints were reverified on 2026-09-07 against the current official Token Plan and OpenAI/Anthropic SDK guides (`api.minimax.cn`). Existing saved connections are not rewritten.

The September 2026 expansion adds 19 templates and removes the retired GitHub Models new-connection template. See the [full endpoint/source matrix](../../../../docs/agent-provider-research-2026-09-07.md). Shared MiniMax PAYG/plan addresses do not identify billing mode; explicit plan selections are preserved.

## Gateway Provider templates

These gateway templates were reviewed on 2026-09-20. They are selectable
first-class Arca Providers, not aliases for the underlying model vendors. Arca
keeps the OpenAI Responses/Completions and Anthropic Messages paths as separate
protocol entries so one Provider can be reused by Agents with different
protocol requirements.

| Arca Provider ID | OpenAI-compatible Base URL | Anthropic-compatible Base URL | Official source |
|---|---|---|---|
| `routeway` | `https://api.routeway.ai/v1` | `https://api.routeway.ai` | [Codex integration](https://docs.routeway.ai/integrations/agents/codex), [Claude Code integration](https://docs.routeway.ai/integrations/agents/claude-code) |
| `infron` | `https://llm.onerouter.pro/v1` | `https://llm.onerouter.pro` | [API reference](https://infron.ai/models/deepseek/deepseek-v4-flash:free/api-reference) |
| `apinex` | `https://api.apinex.bond/v1` | `https://api.apinex.bond` | [Chat API](https://apinex.bond/developers/models/chat), [Messages API](https://apinex.bond/developers/models/messages) |
| `tokenharbor` | `https://tokenharbor.ai/v1` | `https://tokenharbor.ai` | [Chat and Messages](https://tokenharbor.ai/docs/api/curl), [Codex integration](https://tokenharbor.ai/docs/integrations/codex) |

APInex is currently exposed with Chat Completions and Anthropic Messages only;
its public documentation does not establish a separate Responses endpoint.
Model IDs remain dynamic and are loaded from each Provider's `/models` catalog
when available rather than being hard-coded from social media posts.
