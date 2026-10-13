# Arca Agent 与模型供应商覆盖调研

核验日期：2026-09-07。代码基线：Arca v1.8.178。本文记录本次补充及其边界，不以产品目录条数冒充可写能力，也不以供应商模板数量冒充厂商数量。

## 结论与交付范围

Arca 已覆盖 Claude Code、Codex、Gemini CLI、Cursor、Windsurf、Cline/Roo/Kilo、OpenCode、Copilot CLI、Kimi Code、Qwen Code、Qoder 家族、Kiro、Amp、Droid、Goose、Hermes、Pi、OpenHands 等主要开发工具。缺口主要有三类：新 CLI 与 IDE 的身份区分、OpenClaw 这样的个人 Agent，以及需要专属地址或独立套餐的模型服务。

本次增加 **4 个经过配置核验的 Agent 定义、19 个 Provider 模板**，移除已经退役的 GitHub Models 新建模板，并修正模型目录查询与地址推断。新增模板里包含地区和套餐，不是 19 家独立公司。

| 统计口径 | 修改前 | 修改后 |
|---|---:|---:|
| 审核过的 Agent 定义 | 57 | 61 |
| 其中可写全局 MCP | 47 | 50 |
| 其中可分配用户级 Skills | 45 | 46 |
| 发现目录记录 | 201 | 201 |
| 合并去重后的 Agent 身份 | 212 | 216 |
| Provider 模板，含 Custom | 51 | 69 |

没有市场份额数据可支持严格的“主流排名”。此次优先级依据是：开发者工作流相关性、国内外常见服务覆盖、厂商或项目自己的公开契约，以及 Arca 能否安全管理其全局配置。检索发现线索后，再用官方文档、上游项目自己的适配说明和公开只读 API 交叉核验。HTTP 401 单独不足以证明某条 API 路由或响应格式正确。

## Agent：已覆盖、此次补充、尚有缺口

| 产品或产品族 | 基线状态 / 本次处理 | 核验与限制 |
|---|---|---|
| Claude Code / Claude Desktop | 已有独立身份和不同配置适配 | Desktop 的 Connectors、MCP 文件与 Model 配置是不同能力，不能混用 |
| Codex / Gemini CLI / Copilot CLI | 已有 | 本次不重复新增品牌条目 |
| Cursor / Windsurf / VS Code / Zed | 已有 | 全局与项目级配置必须区分；Arca 只写全局 |
| Cline / Roo Code / Kilo Code / Continue | 已有 | 插件宿主、列表布局、协议字段不能照搬一个通用 JSON |
| OpenCode / Amp / Factory Droid / Goose / Hermes / Pi / OpenHands | 已有 | Pi 的 MCP 来自明确标注的社区扩展；目录出现不等于原生支持 |
| Qoder IDE / Qoder CLI / Qoder Desktop / QoderWork | 已有独立身份 | 保持此前独立图标和不同配置文件适配 |
| Kiro | 已覆盖家族的全局 MCP 和 Skills | 当前官方文档统一描述多端配置；不是本轮遗漏的一个全新配置协议。[MCP](https://kiro.dev/docs/mcp/configuration/)、[Skills](https://kiro.dev/docs/skills/) |
| **Antigravity CLI** | **本次增加 MCP** | 与 IDE 共用 `~/.gemini/config/mcp_config.json`；远程字段是 `serverUrl`。[官方 CLI MCP](https://www.antigravity.google/docs/cli/mcp) |
| **Continue CLI (`cn`)** | **本次增加 MCP** | 使用 `~/.continue/config.yaml` 中的 `mcpServers` 列表，与 IDE 共用；`--config` 可改变实际文件，需调整 Arca 目标路径。[CLI 配置](https://docs.continue.dev/cli/configuration) |
| **TRAE CLI 2.0** | **本次增加 stdio MCP** | 默认 `~/.trae/traecli.toml`，`[mcp_servers.<name>]`。不写旧版 YAML，不猜测远程字段或命名 profile。[2.0 配置参考](https://docs.trae.cn/cli_config-file) |
| **OpenClaw** | **本次增加用户级 Skills** | 默认 `~/.openclaw/skills/<name>/SKILL.md`；通过命令或独立配置文件判断安装证据。[Skills](https://docs.openclaw.ai/tools/skills) |
| Aider | 未增加自动 Model writer | 用户级 `.aider.conf.yml` 已找到；还需分别审核 LiteLLM 路由、main/editor/weak model 与凭据交付。不是“没有配置文件”。[配置](https://aider.chat/docs/config/aider_conf.html) |
| CodeBuddy IDE / WorkBuddy | 尚未核实独立全局 writer | 已有 CodeBuddy Code CLI；本次找到的 CLI `.mcp.json` 不能直接证明 IDE/WorkBuddy 使用同一文件。[官方 CLI MCP](https://www.codebuddy.ai/docs/cli/mcp) |
| JetBrains Air / IBM Bob / Blackbox CLI | 发现目录已有相关身份，未提升为可写 | 本次未取得足够的全局文件与保留策略证据，不把目录记录自动升级为 writer |
| Chatbox / Cherry Studio / Open WebUI / LibreChat | 发现目录已有 | 不同产品可能依赖数据库、应用内设置或服务端配置；本次未完成这些存储的独立审计 |
| Devin / Replit / v0 等托管 Agent | 不作为本机文件 writer | 托管端账户配置、组织策略与本机全局文件不是同一资源；本轮不伪造全局路径 |

### 为什么 OpenClaw 只先接 Skills

已经找到其 `~/.openclaw/openclaw.json`，它使用 JSON5，支持 `$include`，MCP 是 `mcp.servers`，Models 又有 Provider、模型列表和 Agent 默认模型指针。凭据还可能是 SecretRef。参考 [配置](https://docs.openclaw.ai/gateway/configuration)、[MCP](https://docs.openclaw.ai/tools/mcp)。

Arca 的 JSON CST 解析器本身能保留注释、单引号和宽松属性名，缺口不是简单加一个 JSON5 库。真正需要解决的是：展开 include 后谁拥有每个字段、写回哪个物理文件、SecretRef 的结构化引用如何保留，以及多文件写入的 CAS、备份和失败边界。直接声明 `key = mcp.servers` 会隐藏这些问题。本轮先接已有完整目录契约的 Skills。

Antigravity CLI 的另一个特殊点是，其文档中的全局 Skills 是平铺 `.md` 斜杠命令，不是 Arca 当前的 `<name>/SKILL.md` 目录布局。不会仅因目录名也叫 `skills` 就启用链接分配。[CLI plugins / skills](https://www.antigravity.google/docs/cli/plugins/)

## 本次新增的 19 个 Provider 模板

以下 Base URL 是协议客户端看到的地址。Arca core 会拆为公共连接根与各协议的请求路径；不用在 UI 手动拼接重复的 `/v1`。

协议缩写：C = OpenAI Chat Completions；R = OpenAI Responses；A = Anthropic Messages。模型目录“手工”表示本轮未启用自动查询，仍可添加模型 ID 或显式配置兼容的 Models 列表 URL。

| Provider ID | 地址 / 必填项 | 协议 | 模型目录 | 主要官方依据 |
|---|---|---|---|---|
| `azure-openai` | `https://<resource>.openai.azure.com/openai/v1` | R、C | 手工填**部署名称** | [Azure v1](https://learn.microsoft.com/en-us/azure/ai-foundry/openai/api-version-lifecycle?view=foundry-classic) |
| `amazon-bedrock-mantle` | `https://bedrock-mantle.<region>.api.aws/v1` | R | `/v1/models` | [AWS Mantle](https://docs.aws.amazon.com/bedrock/latest/userguide/bedrock-mantle.html) |
| `cloudflare-workers-ai` | `https://api.cloudflare.com/client/v4/accounts/<account-id>/ai/v1` | C | 专用 Workers AI 目录 | [OpenAI 兼容](https://developers.cloudflare.com/workers-ai/configuration/open-ai-compatibility/)、[目录 API](https://developers.cloudflare.com/api/resources/ai/subresources/models/methods/list/) |
| `vercel-ai-gateway` | `https://ai-gateway.vercel.sh/v1` | R、C | `/v1/models`，只取语言模型 | [OpenResponses](https://vercel.com/docs/ai-gateway/sdks-and-apis/openresponses)、[模型目录](https://vercel.com/docs/ai-gateway/models-and-providers) |
| `deepinfra` | `https://api.deepinfra.com/v1/openai` | C | `/v1/openai/models`，读取嵌套元数据 | [Chat](https://docs.deepinfra.com/chat/overview) |
| `sambanova` | `https://api.sambanova.ai/v1` | C | `/v1/models` | [SambaNova Models API](https://docs-prod.sambanova.ai/docs/api-reference/endpoints/model-list) |
| `perplexity` | `https://api.perplexity.ai` | C | 手工 | [Sonar OpenAI 兼容](https://docs.perplexity.ai/docs/sonar/openai-compatibility) |
| `volcengine` | `https://ark.cn-beijing.volces.com/api/v3` | C、R | 手工 | [方舟 Responses](https://www.volcengine.com/docs/82379/1795150)、[上游集成契约](https://docs.openclaw.ai/providers/volcengine) |
| `volcengine-coding-plan` | C：`https://ark.cn-beijing.volces.com/api/coding/v3`；A：`https://ark.cn-beijing.volces.com/api/coding` | C、A | 手工 | [快速开始](https://www.volcengine.com/docs/82379/1928261)、[官方社区接入示例](https://developer.volcengine.com/articles/7622875954743083062)、[上游集成](https://docs.openclaw.ai/providers/volcengine) |
| `baidu-qianfan` | `https://qianfan.baidubce.com/v2` | C | 手工 | [OpenAI 兼容](https://cloud.baidu.com/doc/qianfan-docs/s/qm8qxemze) |
| `baidu-qianfan-coding-plan` | C：`https://qianfan.baidubce.com/v2/coding`；A：`https://qianfan.baidubce.com/anthropic/coding` | C、A | 手工 | [千帆 Coding Plan](https://cloud.baidu.com/doc/qianfan/s/imlg0beiu) |
| `minimax` | C：`https://api.minimax.io/v1`；A：`https://api.minimax.io/anthropic` | C、A | `/v1/models` | [OpenAI](https://platform.minimax.io/docs/api-reference/text-openai-api)、[Models](https://platform.minimax.io/docs/api-reference/models/openai/list-models) |
| `minimax-cn` | C：`https://api.minimax.cn/v1`；A：`https://api.minimax.cn/anthropic` | C、A | `/v1/models` | [中国站 OpenAI](https://platform.minimaxi.com/docs/api-reference/text-openai-api)、[Anthropic](https://platform.minimaxi.com/docs/api-reference/text-anthropic-api) |
| `stepfun` | `https://api.stepfun.com/v1` | C | 手工 | [中国站 OpenAI](https://platform.stepfun.com/docs/zh/guides/developer/openai) |
| `stepfun-global` | `https://api.stepfun.ai/v1` | C | 手工 | [国际站 Quickstart](https://platform.stepfun.ai/docs/en/quickstart/overview) |
| `zhipuai` | `https://open.bigmodel.cn/api/paas/v4` | C | 手工 | [智谱 HTTP API](https://docs.bigmodel.cn/cn/guide/develop/http/introduction) |
| `moonshotai-cn` | `https://api.moonshot.cn/v1` | C | `/v1/models` | [Kimi 模型列表](https://platform.kimi.com/docs/api/list-models) |
| `alibaba-international` | `https://dashscope-intl.aliyuncs.com/compatible-mode/v1` | C | 同级 `/models` | [Alibaba OpenAI 兼容](https://www.alibabacloud.com/help/en/model-studio/compatibility-of-openai-with-dashscope) |
| `siliconflow-cn` | `https://api.siliconflow.cn/v1` | C | `/v1/models?sub_type=chat` | [官方 API 文档](https://docs.siliconflow.cn/docs/api/models-get) |

Azure 使用资源 API Key；Bedrock Mantle 使用 Bedrock API Key；Cloudflare 使用有 Workers AI 权限的 API Token。这里没有实现 Entra 登录、AWS SigV4/STS，或替用户创建云资源。模板支持某协议，不保证该账号下每个模型、每个区域都开放它。

火山文档站部分正文在当前读取环境中依赖 JavaScript。本次用它的官方社区接入示例和 OpenClaw 项目自己的 Provider 契约交叉核对；没有把社区文章中的性能、价格、模型排名作为结论。模型型号和价格继续从用户账号与厂商目录确认，不在 Arca 中写死一组容易过期的“最新模型”。

### 模型目录的具体实现

- **Cloudflare**：从已保存的完整 Chat Completions 请求 URL 推导同账号 `/ai/models/search`，请求官方支持的 `format=openrouter`、文本生成任务筛选与分页；读取可用于请求的模型 ID。不会向猜测的 `/ai/v1/models` 发请求。保留总时限、响应大小、页数和条目数限制，拒绝无效 ID 和不前进的分页。
- **DeepInfra**：公开目录当前把上下文长度放在 `metadata.context_length`。新增专用解析，并依据 `metadata.tags` 过滤明确不是 chat 的模型。
- **Vercel**：公开目录包含图像、视频、语音、嵌入等模型；新增适配只将语言模型放入当前 Arca Model 选择器。
- **Azure / Sonar / 部分国内服务**：本轮不声称存在可用的自动模型列表契约；输入框显示手工填写说明，Azure 说明部署名语义。用户显式填写兼容的 Models 列表 URL 后可以启用查询。
- **SiliconFlow 中国站**：使用官方 `sub_type=chat` 筛选，避免把嵌入、图像和音频模型混入；显式 Models 列表 URL 不受这一默认参数影响。
- **Custom**：保留按用户已配置协议推导的目录查询和显式列表 URL；网络错误不会阻止手工添加 Model ID。

公开只读核验：DeepInfra `/v1/openai/models` 返回了 189 个目录条目，Vercel `/v1/models` 返回了 373 个、跨 8 种类型的目录条目。数字仅是 2026-09-07 的响应快照，不用于证明全部模型可调用。这些查询没有使用用户密钥，也没有发起推理。

## 本次修复与清理

1. **发现能力标记没有被前端使用。** 原来只要有 Provider 实例，前端就会自动查询并显示刷新按钮。现在自动请求、刷新按钮与输入框语义都遵守 core 的 `model_discovery_supported`。
2. **按地址错误推断 MiniMax 套餐。** 原目录只有 Token Plan，通用地址会被认成套餐。本次增加中外按量模板，在共享 URL 无法证明套餐时推断普通供应商；显式选定的套餐 ID 保留。中国站新建模板按当前文档更新为 `api.minimax.cn`。
3. **地区与协议归类不精确。** SiliconFlow 中国站不再被归为国际站；补齐 Moonshot 中国站、Alibaba 新加坡等，并识别完整请求 URL。Vertex 的 `aiplatform.googleapis.com` 不再被误认成 Google AI Studio，因为凭据与路径契约不同。
4. **动态 URL 占位符可能被当成有效路径。** 云模板的默认连接值保持空；账号/区域示例只进入显示元数据。Rust 和前端都拒绝未替换的尖括号、花括号及其 URL 编码形式。
5. **缺失目录字段与无关模态混入。** 补齐 DeepInfra 嵌套上下文读取和 Vercel 语言模型筛选。
6. **已退役产品仍可新增。** GitHub 官方确认 Models 已在 2026-07-30 完全关闭。本次移除新建模板；不删除用户已保存的 Provider 或密钥。[官方退役公告](https://github.blog/changelog/2026-07-30-github-models-is-now-retired/)
7. **审核计数与文档漂移。** 修正 Agent 审核数断言和 `agent-catalog.md` 的统计；新增目录条目不扩张未经审核的能力。

## 架构判断与下一步优先级

目前最值得保留的是 core 的中央资产、desired/observed 状态和物理目标写入边界：新 Provider 应只提供连接元数据；MCP writer 继续通过既有 codec、CST/TOML/YAML 编辑和 CAS；React 只展示能力。此次没有在 Desktop 写第二套供应商 URL 拼接或 Agent 配置代码。

| 优先级 | 下一步能力 | 为什么不能只加名称 |
|---|---|---|
| P1 | OpenClaw 原生 MCP / Models | 需要 `$include` 所有权、SecretRef、独立主模型指针和多物理文件写入计划 |
| P1 | Agent 安装证据与配置存在证据分离 | 当前 MCP inventory 主要以文件是否存在判断；共用文件的 CLI/IDE 会共享证据。部分 JSON 顶层 `probes` 也尚未进入 `AgentDefinition` 的正式结构，需要统一发现层，而不是继续添加无效元数据 |
| P1 | 按模型记录支持的协议与模态 | 供应商支持某协议，不等于其全部模型支持；OpenCode Zen 尤其按模型列出不同请求协议。[Zen](https://opencode.ai/docs/zen/) |
| P1 | 云身份与凭据刷新 | Vertex / Google Cloud、完整 AWS Bedrock、Azure Entra 等需要账号、区域、项目、短期 token 和刷新语义，不能把当前 token 永久当 API Key。[Google OpenAI 兼容](https://cloud.google.com/vertex-ai/generative-ai/docs/start/openai) |
| P2 | Aider / Continue 的 Model writer | 需要核实原生多模型角色、配置合并顺序和凭据交付；只写 `model` 字段不等于完整可用 |
| P2 | Databricks / IBM watsonx / 企业推理服务 | Workspace、部署名、项目作用域与 IAM 各不相同；先建立租户字段和适配契约。[Databricks](https://docs.databricks.com/aws/en/machine-learning/model-serving/query-chat-models) |
| P2 | Provider 证据与版本元数据集中 | 目前模板、推断、目录适配和图标分布多处；应逐步收敛到类型化 Catalog，带审核日期、端点、协议与发现方式，减少同步遗漏 |
| P2 | 模型调用兼容性诊断 | 目录读取成功不代表工具调用、流式响应、推理字段、上下文限额可用；需要用户显式发起且能说明用量的连通性诊断 |

TRAE CLI 2.0 的远程 MCP 与 Skills、Antigravity CLI 的平铺 Markdown Skills、CodeBuddy IDE/WorkBuddy 的原生存储也在待补能力内。这里描述的是本轮边界与实现顺序，不是后台已启动的后续任务。

## 代码与验证

- Agent 定义：`data/agents.json`；审核列表：`core/src/agents.rs`。
- Provider 模板、显示元数据和推断：`core/src/resources/model/mod.rs`。
- 模型目录、模态筛选和分页：`core/src/resources/model/discovery.rs`。
- UI：`desktop/src/components/ModelsView.tsx`、`desktop/src/i18n/index.ts`。
- 图标取自官方站点原始资源；来源保存在各 assets 目录的 `SOURCES.md`，共用品牌的 CLI/IDE 使用区分端的角标。
- 新增回归用例覆盖官方文件往返、策略与注释保留、TRAE 远程拒绝、动态 URL、套餐与区域推断、Cloudflare 目录/鉴权、DeepInfra 元数据、Vercel 模态筛选、SiliconFlow 目录参数和 UI 手工部署名输入。
- 按仓库当前极速交付规则，执行生产编译，不执行测试套件、fmt、clippy 或图标检查。测试已补充，但不得描述为已通过。
- 付费推理、用户云账号权限和第三方 Agent 对新配置的端到端消费未实测。正式安装版 UI 与发布资产的核验结果见交付记录。
