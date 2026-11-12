# DeepSeek Harness 配置接入

在 macOS 上实际验证了 CLI `0.2.1-rc.3`、npm `0.2.2-rc.1` 和签名 DMG 内的
Desktop Host `0.2.2-rc.1`。GitHub 的 `0.2.2-rc.1` 合并提交含有
`mcp-manager`，同期 npm 包和签名 Desktop 包实际携带的是 `dsh-mcp-client`。
Arca 的写入行为以发布包里的运行契约为准。
[GitHub 源码](https://github.com/deepseek-ai/deepseek-harness/tree/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159) ·
[官方安装页](https://www.deepseek.com/en/harness/)

## 客户端、Profile 与配置层次

| Arca Agent ID | 原生入口 | MCP / Models 用户配置 |
|---|---|---|
| `deepseek-harness` | `dsh --profile <name>` | `$DSH_HOME/profiles/<name>/cordis.patch.yml`，默认 `web` |
| `deepseek-harness-desktop` | DeepSeek Harness 桌面应用 | `$DSH_HOME/profiles/desktop/cordis.patch.yml` |

`DSH_HOME` 未设置或为空白时为 `~/.dsh`。在 Arca 的 DeepSeek Harness CLI 条目打开
「编辑 → 配置文件 → Harness Profile」，可选已经初始化的 Profile。Models 与 MCPs 共用
选中的配置文件，Plugins 页也读取这个 Profile。启动时使用对应的 `dsh --profile <name>`。
已有 MCP 或 Model 托管关系时，Arca 会拒绝把写入目标切到另一个 Profile。
Desktop 固定使用 `desktop`。用户级 Skills 和 Instructions 由各 Profile 共享。

原生启动链是：Profile 选择的 bundle patch → Profile patch → home
`cordis.patch.yml` → 启动时额外 `--patch`。一条 Cordis patch 的 `config` 替换
该插件条目的完整配置。Arca 因此逐叶保留现有配置，并检查与本次操作有关的 home
覆盖；不能把普通 `mcpServers` 或一个模型根对象直接塞进此文件。

新建 override 前需要一个已由 Harness 初始化的 Profile，以及可核验的继承结构。
Arca 识别官方 `web`、`acp`、`headless`、`sdk` 的标准组合，以及从这些组合创建的命名 Profile。
遇到不明的自定义 bundle、依赖覆盖、禁用插件、重复身份、动态受控字段
或更高层冲突时，Arca 拒绝写入并给出原因。既有完整 override 可按已核验字段编辑。
Arca 不执行 YAML 的 `!!js`，也不推断某次启动额外的 `--patch`。

官方依据：[Profile 合成](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/boot/app-boot/README.md)、
[启动层次](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/cli/src/profile-boot.ts)、
[Desktop Profile](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/desktop/src/paths.ts)。

## Models

自定义模型使用 `llm-pi-ai` 条目的 `config.providers`。原生允许多个自定义模型。
Arca 为每个已添加 Model 保留稳定的 Provider route，添加备用模型不会替换当前默认。

| Profile | 新会话模型选择 |
|---|---|
| Web、Desktop、Headless | `agent-default-model.config.provider` 和 `model` |
| ACP | `acp.config.provider` 和 `model` |
| SDK | SDK 客户端在 `initialize` 中指定 `provider` 和 `model`，Arca 注册可用模型 |

SDK 客户端使用 `llm-pi-ai.config.providers` 中的对应键作为 `provider`。
Agent 的 Models 页显示显式配置的原生默认和来源。HOME 层覆盖的自定义模型以
外部配置显示，账号登录与凭据仍由 Harness 管理。已运行的会话使用自己的模型选择。
[ACP 原生配置](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/acp/acp/README.md) ·
[SDK 初始化协议](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/sdk/protocol/src/types.ts)

已发布运行包的自定义模型支持三种协议。

- OpenAI Chat Completions 使用 `openai-completions`。
- OpenAI Responses 使用 `openai-responses`。
- Anthropic Messages 使用 `anthropic-messages`。

`0.2.2-rc.2` 标签中的模型源码新增 Gemini generateContent，对应 `google-generative-ai`。
该标签的发布页尚无运行包，本次验证使用已发布的 CLI 和 Desktop 版本。
[rc.2 发布页](https://github.com/deepseek-ai/deepseek-harness/releases/tag/dsh-v0.2.2-rc.2) ·
[rc.2 协议实现](https://github.com/deepseek-ai/deepseek-harness/blob/ddac96760b6edf8fa0fe17cbdcf0b512f4c12b2c/packages/llm/llm-pi-ai/src/provider.ts)

Provider 的 `baseURL`、`apiKeyEnv`、显示名，以及模型的 ID、名称和容量分别写入
对应原生字段。未知 Provider/model 策略和其他条目保留。多个原生模型共用同一
Provider 时保持只读，避免接管一个模型时修改兄弟模型的端点和凭据。

`apiKeyEnv` 是 Harness credential 引用；原生先查自己的 credential store，再按其
环境层次回退。Arca 只写引用名，不读取或修改 `$DSH_HOME/.credentials.yaml`，也不
把 Arca Keychain 的值展开到此文件。使用 Arca 的环境变量交付配置相应引用名，并让
运行 Harness 的环境或其原生 credential store 提供值。
原生账户登录、隐式认证和仅由安装目录解析的模型 catalog 仍由 Harness 管理。

删除最后一个当前模型时必须移除默认模型的 config override，恢复 bundle 的原生
默认；留下 `config: {}` 会缺少必需的 provider/model 字段。Arca 不会通过删空字段
制造这样的启动错误，也不会清理原生账户凭据。

官方依据：[模型用户指南](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/docs/user/guide/providers.md)、
[自定义协议列表](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/llm/llm-pi-ai/src/provider.ts)、
[默认模型服务](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/core/agent-default-model/src/index.ts)。

## MCP

经验证的 Profile 通过在 `cordis.patch.yml` 顶层 `- insert:` 列表中插入 `@deepseek-ai/dsh-mcp-client`
插件项持久化 MCP 连接，每个条目分配独立的 `id: mcp-<serverName>` 与 `config.serverName`。
Arca 写入 stdio 或 Streamable HTTP 原生格式，并保留超时、重连与未知策略。名字须符合原生
`[A-Za-z0-9_-]{1,32}`；SSE 和无法表达的跨 transport 参数明确拒绝，不静默丢弃。

原生通过插件条目级的 `disabled: true` 暂停挂载连接，缺省或 `disabled: false` 为启用。
观察、启停和移除以单个客户端条目为边界。批量清理前同样检查 home 覆盖。
MCP 和 Models 共用 Profile 文件时，各自只修改对应的行和字段。

旧版 Arca 写入的 `mcp-manager.config.servers` 条目仍可查看。分配一个可转换的
MCP 或对已有关系执行“恢复期望配置”时，Arca 把所选条目移到单独的
`dsh-mcp-client`，保留策略、
注释、同文件中的其他 server 和模型配置。已有的 Arca 托管关系沿用原来的启停选择，
首次分配按 Arca 的默认规则启用。包含 manager 凭据模板或动态值的条目
保持原样，并在分配时报告原因。读取配置时不会迁移任何条目。

Profile 层和 `$DSH_HOME/cordis.patch.yml` 的声明分别显示。插件 bundle 中带静态名称和
传输类型的 MCP 也显示其插件来源。只读声明可以从 MCP 详情查看单项脱敏 JSON 和源路径。
查看不要求密钥解锁，
不会启动 MCP、执行 `!!js` 或写入配置。
静态声明的查看也不证明原生会话已加载或已连通。

官方依据：[MCP client 与接入方法](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/mcp/mcp-client/README.md)。

## Skills 与 Instructions

用户级 Skills 目录是 `$DSH_HOME/skills`，兼容目录是 `$DSH_AGENTS_HOME/skills`；
后者默认 `~/.agents/skills`。两种客户端共享同一物理目标，Arca 复用 Core 的中央副本、
链接、消费关系与恢复机制。环境覆盖改变目标时，不把它误算作其他 Agent 的默认
`~/.agents/skills` 目录。
共享目录中的分配或解除会同时影响两个客户端；操作计划列出所有受影响客户端，
重复添加不会制造第二份副本。解除分配保留中央 Skill。
两种 home 都使用上游的 `~`、`~/`、`~\` 和相对路径解析；同一目录去重，
解析到已有共享目录时复用该目录的物理目标身份。

原生 filesystem provider 按项目目录、custom 目录、DSH 用户目录、共享用户目录和
bundled 目录的顺序合并。Arca 只分配用户级目录，不写项目配置。用户在 Harness 禁用
provider 或默认 roots 后，链接存在仍不能证明 Skill 会进入会话；此类原生策略不会被
Arca 自动开启。原生支持直接子目录的 `SKILL.md` 与根目录的扁平 `.md`，不递归发现
任意深度的 Skill 树。两个用户级根目录的独立 `.md` 文件在 Agent 的 Skills 页按
frontmatter 中的名称显示。详情提供来源和模型、用户调用权限，只读展示。

Instructions 沿用原文件编辑：DSH home 与 agents home 下的用户级 `AGENTS.md`。
不建立中央正文副本，不自动修改原生加载策略，也不新建或迁移其他指令文件。

官方依据：[Skills roots 与优先级](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/skill/skill-filesystem/README.md)、
[原生 home 解析](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/util/home-paths/src/index.ts)、
[用户 Instructions](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/files.ts)。

## 其他原生扩展

Harness 的扩展是 Cordis bundle、插件条目与 agent preset。`package.json` 的
`dsh.profile.bundles` 是选择声明；安装元数据、配置禁用、运行时激活和会话使用分别
表示，不能把 npm 依赖或缓存目录直接当成已启用插件。
Arca 优先读取 Profile 的 `node_modules`。对于默认 bundle，Arca 还会沿选中的
`dsh` 启动文件定位其 npm 包，或读取 macOS Desktop 的 `app.asar` 元数据。
Desktop 使用 Bundle ID `com.deepseek.dsh`，并核对 ASAR 头部与包清单完整性。
无法解析的包仍显示“已声明”，实际激活状态由 Harness 管理。

已验证的 `@deepseek-ai/dsh-agent-preset` 有 4 个随包分发的 Skill。
签名桌面包还在 `Contents/Resources/runtime/office-skills` 携带
`@deepseek-ai/dsh-skill-office` 的 7 个 Skill。Agent 的 Skills 页把这些条目标为内置，
显示插件来源与源路径，并保持只读。扫描读取包元数据和 `SKILL.md` 的文件位置。
当前会话能加载哪些 Skill 取决于原生 preset、provider 和项目配置。
bundle patch 中的 MCP 插件条目显示静态引用。

Agent preset 是 `dsh-agent-preset` 声明的子插件树。Hook 是 Cordis 事件扩展点，
不是一套已经核验的用户级 `agents/*.md` 或 `hooks.json` 文件规范；Arca 不伪造这些
目录的 writer。只读插件来源、内置组件和已核验 Skill 声明不提供删除能力。

官方依据：[插件管理](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/boot/plugin-manager/README.md)、
[Agent presets](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/preset/agent-preset/README.md)。

## 本次验证范围

2026 年 11 月 12 日，在隔离 HOME 中用 Arca CLI 分配模型、MCP 和 Skill。
实际会话覆盖已安装 CLI、npm 发布包的 Web、Headless、ACP、SDK 和一个从 Headless
初始化的命名 Profile，以及签名 DMG 内的 Desktop Host。
三种模型协议通过本机模拟端点验证请求和流式回复，stdio 和 Streamable HTTP 均完成
真实 MCP 握手和工具调用。会话读取了受管 Skill、独立 Markdown Skill 和 `AGENTS.md`。
验证还覆盖旧 MCP 迁移、注释与 inode 保留、Web 配置隔离和同一进程内的 MCP 启停。
账号登录和云模型推理未纳入该验证。
