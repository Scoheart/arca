# Provider 模型发现调研

日期：2026-08-21

## 调研口径

本调研以 Arca 当前内置的 51 个 Provider 类型为全集，先查 Provider 官方 API 文档，再对 Arca 默认连接的候选列表地址做一次无 API Key 的只读 `GET` 路由探测。探测不携带用户凭据、不调用推理、不修改任何 Provider 侧状态。

HTTP `200` 可确认公开目录；`401` 可确认路由存在且要求鉴权。单独的 `403` 不作为充分证据，因为当前网络出口也会拦截部分域名；这类 Provider 必须同时有官方文档、OpenAI-compatible 契约或同产品已验证路由作为依据。

## 结论

- 首版可对 48/51 个内置 Provider 类型启用模型发现。
- 其中 44 个使用同源 OpenAI-compatible `GET .../models`，但 URL 必须从 Core 已保存的完整协议请求地址严格推导，不能由 React 拼接。
- Anthropic、Google Gemini、Cohere、Fireworks 使用 4 个显式原生适配器。
- `github-models`、`wandb`、`custom` 本期不启用，原因见下文。
- 所有 Provider 都继续允许手工填写 Model ID；“支持发现”不等于推理调用一定可用。

## 首版支持矩阵

### OpenAI-compatible 同级 `/models`（44 个）

以下 Provider 从其已配置的 OpenAI `.../responses` 或 `.../chat/completions` 请求地址精确移除末尾操作段，再追加 `/models`：

```text
openrouter
openai
xai
mistral
deepseek
groq
alibaba
alibaba-coding-plan-cn
alibaba-coding-plan
alibaba-token-plan-cn
alibaba-token-plan
xiaomi
xiaomi-token-plan-cn
xiaomi-token-plan-sgp
xiaomi-token-plan-ams
moonshotai
kimi-for-coding
zai
zai-coding-plan
zhipuai-coding-plan
minimax-coding-plan
minimax-cn-coding-plan
stepfun-step-plan
stepfun-ai-step-plan
tencent-coding-plan
tencent-token-plan
tencent-token-plan-global
nvidia
cerebras
siliconflow
together
poe
huggingface
novita-ai
qiniu-ai
digitalocean
modelscope
scaleway
nebius
requesty
baseten
ollama
lm-studio
vllm
```

响应兼容以下已观察到的目录形态：

- OpenAI/OpenRouter/NVIDIA/Poe/Hugging Face/ModelScope 等：`data[]`。
- Together/Mistral 文档形态：顶层数组。
- 每个元素至少提取 `id`；展示名兼容 `name`、`display_name`、`displayName`、`title`；上下文兼容 `context_length`、`context_window`、`context_size`、`max_context_length`。

无密钥实测中，OpenRouter、Alibaba Coding Plan（中国/国际）、NVIDIA、Poe、Hugging Face、ModelScope 返回了可解析的公开目录；Anthropic/OpenAI/xAI/Mistral/Cohere/DeepSeek/Groq/Kimi/Moonshot/MiniMax/Tencent/DigitalOcean/Baseten 等返回鉴权挑战，说明列表路由存在。

### 原生适配器（4 个）

| Provider | 列表请求 | 鉴权 | 主要字段 |
|---|---|---|---|
| `anthropic` | 同源 `GET /v1/models?limit=1000` | `X-Api-Key` + `anthropic-version: 2023-06-01` | `data[].id/display_name/max_input_tokens` |
| `google` | 同源 `GET /v1beta/models?pageSize=1000` | `x-goog-api-key` | `models[].baseModelId/displayName/inputTokenLimit`，只保留支持 `generateContent` 的条目 |
| `cohere` | 同源 `GET /v1/models` | Bearer | `models[].name/context_length/endpoints` |
| `fireworks` | 同源 `GET /v1/accounts/fireworks/models?filter=supports_serverless%3Dtrue&pageSize=200` | Bearer | `models[].name/displayName/contextLength` |

### 凭据策略

以下列表目录可公开访问或本地默认不鉴权；若 Provider 已保存 API Key，Core 仍会附带它，以获得该 Key 的真实可见范围：

```text
openrouter
alibaba-coding-plan-cn
alibaba-coding-plan
nvidia
poe
huggingface
modelscope
requesty
ollama
lm-studio
vllm
```

其余支持项在未保存 Provider API Key 时应在发网前给出明确的“先保存 API Key”错误。凭据只从 Keychain 临时读取，不进入 Desktop 参数、日志或设置文件。

## 本期明确不启用（3 个）

| Provider | 原因 |
|---|---|
| `github-models` | GitHub 已宣布并于 2026-07-30 完全关闭 GitHub Models；模型目录和推理 API 均不再可用。当前目录请求返回 retirement 错误。保留现有 Provider 记录属于另一个兼容性清理任务，本功能不扩大到删除它。 |
| `wandb` | 官方 `/v1/models` 除 Bearer Key 外还要求 `OpenAI-Project: <team>/<project>`。Arca 当前 Provider schema 没有该非敏感租户字段；在没有可靠参数来源时不应发送一个注定不完整的请求。 |
| `custom` | 任意 Custom Provider 是否实现 `/models` 无法从现有 schema 确认。为了避免携带 Key 对任意地址盲探测，本期只保留手工 Model ID；后续可增加用户显式配置的 discovery path。 |

## 代表性官方依据

- OpenRouter Models API: https://openrouter.ai/docs/api/api-reference/models/list-all-models-and-their-properties
- OpenAI Models API: https://platform.openai.com/docs/api-reference/models
- Anthropic Models API: https://platform.claude.com/docs/en/api/models/list
- Google Gemini Models API: https://ai.google.dev/api/models
- xAI Models API: https://docs.x.ai/developers/rest-api-reference/inference/models
- Cohere Models API: https://docs.cohere.com/reference/list-models
- DeepSeek Models API: https://api-docs.deepseek.com/api/list-models
- Groq API Reference: https://console.groq.com/docs/api-reference
- Alibaba Model Studio Models API: https://help.aliyun.com/en/model-studio/list-models
- MiniMax Models API: https://platform.minimax.io/docs/api-reference/models/openai/list-models
- Cerebras Models API: https://inference-docs.cerebras.ai/api-reference/models/list-models
- SiliconFlow Models API: https://docs.siliconflow.com/en/api-reference/models/get-model-list
- Together Models API: https://docs.together.ai/reference/models
- Fireworks Models API: https://docs.fireworks.ai/api-reference/list-models
- Poe Models API: https://creator.poe.com/api-reference/listModels
- Hugging Face Inference Providers: https://huggingface.co/docs/inference-providers/en/hub-api
- Novita Models API: https://novita.ai/docs/api-reference/model-apis-llm-list-models
- DigitalOcean model listing example: https://docs.digitalocean.com/products/marketplace/catalog/codex-cli/
- Scaleway Models API: https://www.scaleway.com/en/docs/generative-apis/api-cli/using-models-api/
- Nebius Models API: https://docs.tokenfactory.nebius.com/api-reference/models/list-models
- Requesty Models API: https://docs.requesty.ai/api-reference/endpoint/models-list
- W&B Inference model listing: https://docs.wandb.ai/weave/guides/integrations/inference
- Ollama OpenAI compatibility: https://docs.ollama.com/api/openai-compatibility
- LM Studio Models API: https://lmstudio.ai/docs/developer/openai-compat/models
- GitHub Models retirement: https://github.blog/changelog/2026-07-01-github-models-is-being-fully-retired-on-july-30-2026/

## 实现边界

- 支持表是 Core 内部权威，不复制到 React。
- URL 只允许从已保存 Provider instance 的同源连接推导；不接受前端传 URL 或 Header。
- 禁止跨源重定向；首版直接关闭自动重定向。
- 目录结果只驻留当前新增/编辑弹窗，不写入 `settings.json`。
- 列表失败不影响手工输入和保存。
