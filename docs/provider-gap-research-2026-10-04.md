# Arca Provider 缺口调研与接入 — 2026-10-04

本次以源码中的 73 个模板为基线，检查开发者常用模型网关、托管推理平台、模型厂商和云平台。新增 30 个模板，合计 103 个（含 Custom）。17 个启用模型列表，13 个先使用手动 Model ID。这是按开发者接入价值和现有协议可实现性筛选的目录补全，不是基于付费流量数据的市场份额排名。

实施先保留在本地开发预览；没有修改版本、提交、推送或发布。

## 接入矩阵

Base URL 是对应 SDK 的客户端地址；Core 将不同协议的地址合并为公共根与各自 Endpoint Path。不能把某一种协议的地址直接复制给另一种协议。模型清单动态读取，不冻结本次观察到的具体模型名称。

| 新增模板 | 默认 SDK Base URL | 已核实协议 | 模型发现 | 依据 |
|---|---|---|---|---|
| Portkey | `https://api.portkey.ai/v1` | Chat Completions | 手动 Model ID | [官方资料](https://portkey.ai/docs/integrations/llms/openai) |
| Helicone AI Gateway | `https://ai-gateway.helicone.ai` | Chat Completions | 手动 Model ID | [官方资料](https://docs.helicone.ai/getting-started/quick-start) |
| PPIO | `https://api.ppio.com/openai/v1` | Chat Completions | 启用 | [官方资料](https://ppio.com/model/ppio-4b) |
| Infini AI GenStudio | `https://cloud.infini-ai.com/maas/v1` | Chat Completions / Messages | 手动 Model ID | [官方资料](https://docs.infini-ai.com/gen-studio/api/get-started/) |
| AIHubMix | `https://aihubmix.com/v1` | Chat Completions / Responses / Messages / Gemini GenerateContent | 启用 | [官方资料](https://aihubmix.com/developers) |
| 302.AI | `https://api.302ai.com/v1` | Chat Completions | 手动 Model ID | [官方资料](https://studio.302.ai/en/docs/features/model-providers/configuration) |
| OpenCode Zen | `https://opencode.ai/zen/v1` | Responses / Chat Completions / Messages | 启用 | [官方资料](https://opencode.ai/docs/zen/) |
| OpenCode Go | `https://opencode.ai/zen/go/v1` | Chat Completions / Responses / Messages | 启用 | [官方资料](https://opencode.ai/docs/go/) |
| Kilo Gateway | `https://api.kilo.ai/api/gateway` | Chat Completions | 启用 | [官方资料](https://kilo.ai/docs/gateway) |
| NanoGPT | `https://api.nano-gpt.com/api/v1` | Chat Completions / Responses / Messages | 启用 | [官方资料](https://docs.nano-gpt.com/) |
| Synthetic | `https://api.synthetic.new/openai/v1` | Chat Completions / Messages | 启用 | [官方资料](https://dev.synthetic.new/docs/api/overview) |
| Chutes | `https://llm.chutes.ai/v1` | Chat Completions | 启用 | [官方资料](https://chutes.ai/docs/guides/starter-guide) |
| Featherless AI | `https://api.featherless.ai/v1` | Chat Completions | 手动 Model ID | [官方资料](https://featherless.ai/docs/quickstart-guide) |
| Venice | `https://api.venice.ai/api/v1` | Chat Completions / Responses | 启用 | [官方资料](https://github.com/veniceai/api-docs/blob/main/api-reference/api-spec.mdx) |
| FriendliAI | `https://api.friendli.ai/serverless/v1` | Chat Completions | 启用 | [官方资料](https://friendli.ai/docs/guides/model-apis/quickstart) |
| Inference.net | `https://api.inference.net/v1` | Chat Completions | 启用 | [官方资料](https://docs.inference.net/api/api-quickstart) |
| OVHcloud AI Endpoints | `https://oai.endpoints.kepler.ai.cloud.ovh.net/v1` | Chat Completions / Responses | 启用 | [官方资料](https://docs.ovhcloud.com/en/guides/public-cloud/ai-machine-learning/ai-endpoints-getting-started) |
| Upstage | `https://api.upstage.ai/v1` | Chat Completions / Responses | 手动 Model ID | [官方资料](https://console.upstage.ai/api/chat) |
| AI21 Labs | `https://api.ai21.com/studio/v1` | Chat Completions | 手动 Model ID | [官方资料](https://docs.ai21.com/reference/jamba-1-6-api-ref) |
| ZenMux | `https://zenmux.ai/api/v1` | Chat Completions / Responses / Messages | 启用 | [官方资料](https://zenmux.ai/docs/guide/quickstart) |
| BytePlus ModelArk | `https://ark.ap-southeast.bytepluses.com/api/v3` | Chat Completions / Responses | 手动 Model ID | [官方资料](https://docs.byteplus.com/en/docs/modelark/quick-start) |
| Tencent TokenHub (China) | `https://tokenhub.tencentmaas.com/v1` | Chat Completions | 手动 Model ID | [官方资料](https://cloud.tencent.com/document/product/1823/131382) |
| Tencent TokenHub (Global) | `https://tokenhub-intl.tencentcloudmaas.com/v1` | Chat Completions | 手动 Model ID | [官方资料](https://intl.cloud.tencent.com/document/product/1300/78939) |
| LongCat | `https://api.longcat.chat/openai/v1` | Chat Completions / Messages | 启用 | [官方资料](https://longcat.chat/platform/docs/APIDocs.html) |
| iFLYTEK Spark X2 | `https://spark-api-open.xf-yun.com/agent/v1` | Chat Completions / Messages | 手动 Model ID | [官方资料](https://www.xfyun.cn/doc/spark/X2-Flash.html) |
| Nscale | `https://inference.api.nscale.com/v1` | Chat Completions | 手动 Model ID | [官方资料](https://docs.nscale.com/api-reference/inference/create-chat-completion) |
| io.net | `https://api.intelligence.io.solutions/api/v1` | Chat Completions | 启用 | [官方资料](https://io.net/p/how-to-deploy-llama-3-on-io-net-step-by-step-guide) |
| Inception | `https://api.inceptionlabs.ai/v1` | Chat Completions | 启用 | [官方资料](https://docs.inceptionlabs.ai/get-started) |
| Ollama Cloud | `https://ollama.com/v1` | Chat Completions / Responses | 启用 | [官方资料](https://docs.ollama.com/cloud) |
| Tinfoil Proxy | `http://127.0.0.1:3301/v1` | Chat Completions | 手动 Model ID | [官方资料](https://tinfoil.sh/coding-agents) |

额外协议的地址由 `provider_additional_endpoints` 统一维护。比如 LongCat 的 Chat 是 `/openai/v1/chat/completions`，Messages 是 `/anthropic/v1/messages`；Synthetic 分别使用 `/openai/v1` 与 `/anthropic/v1`；NanoGPT 使用 API 专用域名 `api.nano-gpt.com`。OpenCode Go 的三个协议和模型列表也核对了[官方服务端路由](https://github.com/anomalyco/opencode/tree/dev/packages/console/app/src/routes/zen/go/v1)。

Venice 的 Responses 当前属于 alpha，保留 Chat Completions 为默认。Ollama Cloud 的 Responses 仅支持无状态调用。协议存在不代表该服务的每个模型都支持所有协议、工具或请求参数，使用时仍应按模型文档选择。

## 模型列表核验

只向官方端点发送不带凭据的 GET 请求，没有创建账号/密钥，也没有发送付费推理请求。下列读取返回 HTTP 200，并核对了 OpenAI 形态的 `data[]`、模型 ID、可选名称和上下文长度：

- OpenCode Zen 86 条、OpenCode Go 43 条；
- Kilo Gateway 401 条，`/models` 同时由[官方 API 文档](https://kilo.ai/docs/gateway/models-and-providers)明确提供；
- Chutes 14 条、NanoGPT 612 条、Synthetic 12 条；
- Venice 128 条、FriendliAI 7 条、Inference.net 65 条；
- OVHcloud 24 条、ZenMux 201 条、io.net 39 条；
- Inception 2 条、Ollama Cloud 17 条。
- PPIO 84 条、AIHubMix 417 条；AIHubMix 的[官方 OpenAPI](https://aihubmix.com/openapi.json)同时确认 Chat、Responses、Messages 与 Gemini 的不同路径和认证头。

这些计数只是 2026-10-04 的时点观察，不进入运行时或模型默认值。LongCat 的列表契约由[官方模型列表文档](https://longcat.chat/platform/docs/zh/api/models.html)确认，匿名请求返回 401，因此使用用户配置的凭据读取。

其余模板默认关闭自动发现，用户仍可手动填写 Model ID，并可显式设置 Models 列表 URL：

- Featherless 的公开列表有 51,234 条，`pagination` 显示 257 页；现有选择器和 Core 的 2,000 条 / 10 页安全上限不适合全量加载。不能将第一页误报为完整目录。后续可单独做按需搜索与分页交互。
- AI21 的匿名列表为空；Upstage、Nscale、BytePlus、TokenHub、Spark、无问芯穹和 302.AI 没有在本次获得完整可复验的列表契约。返回 401 只说明需要鉴权，不能据此宣称模型列表已集成。
- Portkey 的公开列表需鉴权，本次只核实基础请求契约；Helicone 的两个候选列表地址返回 405，因此不假设它提供标准 Models API。
- Tinfoil 使用本机官方代理；未替用户安装或启动第三方代理。

## 当前变化和限制

### Tencent TokenHub

[腾讯官方迁移指南](https://cloud.tencent.com/document/product/1823/131382)要求新接入使用 `https://tokenhub.tencentmaas.com/v1`；[国际版指南](https://intl.cloud.tencent.com/document/product/1300/79733)使用 `https://tokenhub-intl.tencentcloudmaas.com/v1`。旧混元平台正在迁移且停止支持新购服务，所以本次新增当前 TokenHub 的国内/国际按量计费模板。已有 Coding Plan / Token Plan 与用户保存的连接不自动改写。

### Ollama Cloud 与 Tinfoil

Ollama Cloud 和原先的本机 Ollama 是独立连接，前者需要云端 API Key。官方[OpenAI 兼容说明](https://docs.ollama.com/api/openai-compatibility)确认 Chat 与 Responses。其 [Messages 接口](https://docs.ollama.com/api/anthropic-compatibility)只接受 Bearer，不能将其泛化为适用于所有 Agent 的 x-api-key 接口，因此本次先接入 OpenAI 两种协议。

Tinfoil 的[官方编程工具指南](https://tinfoil.sh/coding-agents)明确要求本地代理，在连接时验证远端证明。本次模板使用 `http://127.0.0.1:3301/v1`，保留 API Key 鉴权并说明先启动官方代理。没有将普通远端 HTTP 调用包装成“已验证机密推理”。

### OpenCode Go

[官方使用要求](https://opencode.ai/docs/go/#where-can-i-use-it)要求典型编程 Agent 流量、真实客户端 User-Agent 与稳定会话信息。Arca 只管理连接和凭据，不替 Agent 伪造会话标识；客户端能否正确发送这些请求信息还取决于它自己的版本。

### Portkey 与 Helicone

Portkey 的[现行 OpenAI SDK 示例](https://portkey.ai/docs/integrations/llms/openai)只需要网关 API Key 和 Base URL，并用 `@供应商标识/模型` 选择用户事先配置的供应商；与旧版额外路由头教程不同，本次接入这个已核实的基础路径。Helicone 使用[托管 AI Gateway](https://docs.helicone.ai/getting-started/quick-start)的根地址，不混用旧教程或自托管实例的 `/ai` 路由。两者的 BYOK 和管理配置继续在各自控制台中完成。

## 调查后暂未新增

| 候选 | 当前判断 |
|---|---|
| GitHub Models | [官方公告](https://github.blog/changelog/2026-07-01-github-models-is-being-fully-retired-on-july-30-2026/)确认 2026-07-30 已完全退役，继续不提供新连接模板。 |
| Hyperbolic、Lambda Inference | 当前[Hyperbolic 文档](https://docs.hyperbolic.xyz/)和[Lambda 文档](https://docs.lambda.ai/)以 GPU 云及自部署为主；未获得可核实的现行共享推理入口，不能复用旧名单里的地址。自部署端点可使用 Custom。 |
| Google Vertex AI | [官方兼容接口说明](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/multimodal/call-vertex-using-openai-library)将认证与资源选择作为独立接入步骤；本次没有补齐其项目/区域、OAuth/ADC 或 Express Mode 的完整配置流程，因此暂不作为固定静态 API Key 模板。 |
| AWS Bedrock 原生 API | 原生调用与认证契约不等同于当前四种协议；已有 Mantle 模板覆盖其已核验的简化接口。 |
| 网关的高级路由与双凭据模式 | Portkey/Helicone 的基础入口已接入；需要额外头、独立供应商密钥或自托管配置的模式尚未作为通用模板开放。 |
| Nous Portal | [官方集成](https://hermes-agent.nousresearch.com/docs/integrations/nous-portal)包含 OAuth/认证存储和订阅工具网关；应独立做登录与凭据生命周期，不能只把订阅 token 当成永久 API Key。 |
| Baichuan、01.AI、SenseNova、Ling 的直接服务 | 本次未取得足够明确的现行通用协议/认证资料。它们的模型可通过已接入网关使用，暂不猜测独立平台端点。 |
| RunPod、Modal、Databricks 自定义部署 | 地址与用户的部署或工作区相关；适合后续增加资源型配置向导，当前可用 Custom。 |

## 实现位置与验收范围

- Core：统一模板、协议地址、URL 识别与模型发现能力；Desktop 与 CLI 自动消费同一目录。
- `data/provider-links.json`：补齐官方说明和密钥/控制台入口。无法确认独立密钥路径时使用有明确语义的控制台或配置指南，不编造深层链接。
- Desktop：补齐本地品牌资源与 alias；新连接仍通过原有中央 Provider 生命周期保存，凭据仍在 Keychain。
- 本地编译检查和开发预览验收；完整付费模型请求需用户配置相应服务凭据后才能验证。本次未将公开列表读取等同于付费推理成功。
