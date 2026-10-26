# Models 与 Providers

Provider 定义一套 API 连接，Model 引用它并指定协议与 Model ID。一份 Provider 可供多个模型使用，中央 Model 又可分配给多个兼容 Agent。

## 添加连接与模型

1. 在 **Models → 添加 Provider** 中选择模板或自定义连接。
2. 填写名称、唯一 Base URL 与凭据方式。没有鉴权的本地服务可留空。
3. **服务商入口 URL** 可修改；右侧按钮打开密钥页、控制台或配置指南。不同地区与 Coding Plan 使用对应入口。
4. 根据需要填写 **Models 列表 URL**，启用协议并编辑 **Endpoint Path**。这些字段直接展示；完整请求 URL 随输入更新。
5. 保存 Provider，再点击 **添加 Models**，选 Provider、协议与准确的 Model ID。上下文和最大输出等设置按实际模型填写。
6. 进入 Agent 的 Models 页，添加中央模型并选择当前模型；需要时重启客户端或新建会话。


Provider 使用一个 Base URL。不同协议可以有不同的相对 Endpoint Path；不同域名或账号应创建独立 Provider。例如 `https://gateway.example.com/v1` 与 `/responses` 对应 `https://gateway.example.com/v1/responses`。

仍有 Model 使用某协议时，Arca 会阻止停用该协议。若客户端无法无损表达自定义请求路径，Arca 会拒绝该关系，不会忽略你的 Path。

## 支持的协议

| 协议 | 常见默认路径 |
|---|---|
| Anthropic Messages | `/v1/messages`，具体以 Provider 模板为准 |
| OpenAI Responses | `/responses` |
| OpenAI Chat Completions | `/chat/completions` |
| Gemini GenerateContent | `/models/{model}:generateContent` |

协议兼容并不代表任意模型都能运行。准确的 Model ID、模型能力、账号权限和客户端要求仍由服务商与 Agent 决定。

## Agent 支持与当前模型

下表由发布版 Core 能力导出。名称与身份取自 Arca 核验目录，CLI 与 Desktop 形态分别列出。

<AgentReference models-only />

**已添加、已启用、当前模型**是不同状态。原生多模型 Agent 可以保留多个模型；单模型 Agent 最多一个。停用或移除当前模型时，计划会展示影响和可用的后续选择。

- **Codex CLI 与桌面入口**共用用户配置；Model writer 在 `codex` 上管理同一配置。
- **OpenCode CLI 与 Desktop**共用原生配置，可从对应入口管理 Models。
- **Qoder IDE、Qoder Desktop、Qoder CLI**分别保留身份。IDE 的 Models 是官方引导；Desktop 0.1.8+ 与 CLI 1.1.50+ 可以自动配置自定义端点。
- **Qoder Desktop**在会话里选用模型；**Qoder CLI**支持全局当前模型。它们共用 settings 文件，但有独立的 Arca Provider 身份；共享文件仍有其他消费关系时，不能清空整个库。

## 凭据

中央密钥正文存系统 Keychain。Agent 的原生能力决定交付方式：Keychain 读取命令、环境变量引用，或在明确审阅后导出到客户端的私有配置。Claude 桌面端、ZCode 和可用的 Qoder 原生交付方式有各自限制，界面会展示实际支持的选项。

使用环境变量引用时，启动 Agent 的环境必须提供该变量。仅把 Key 保存到 Arca Keychain，并不自动把它变成任意客户端可读取的环境变量。不要把真实密钥放进 CLI 参数、示例文件或共享截图。

Keychain 读取失败会阻止相关操作，不会当成“没有密钥”继续覆盖。编辑 Provider 会包含全部受影响的消费者；不相关的 MCP 和 Skill 操作保持可用。

## 外部配置与删除

扫描到的外部模型不会自动变成中央资产。采用或导入准确的候选后，再按需要分配；普通分配不会覆盖外部漂移。

删除中央 Model 会先展示消费者，保留其他 Model 和 Provider。删除 Provider 是另一项操作，需处理仍在引用它的 Model。Agent 页的“移除全部”只清理选定 Agent 的模型范围；中央资产和凭据保留，共享存储冲突由 Core 拒绝。

[Provider 模板目录](/guide/providers) · [CLI 创建与审阅](/guide/cli#model)
