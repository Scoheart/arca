# DeepSeek Harness 配置接入

本次依据官方 `0.2.2-rc.1` 源码，核验提交为
[`fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159`](https://github.com/deepseek-ai/deepseek-harness/tree/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159)。
DeepSeek Harness 仍在 developer preview；这里描述的是本次已核验的文件契约，
不把目录记录、插件安装或静态配置当作实际会话的运行结果。
另对照官方 [`dsh-v0.2.1-rc.3`](https://github.com/deepseek-ai/deepseek-harness/tree/1f81c047fc48fa190a683e469daa699857e9afe8)
核验了用户当前 CLI 版本：该版尚无 `mcp-manager`，不能直接应用新版 MCP manager
格式；模型自定义字段、home 解析和用户 Skills 目录契约保持兼容。

## 两个客户端与配置层次

| Arca Agent ID | 原生入口 | MCP / Models 用户配置 |
|---|---|---|
| `deepseek-harness` | `dsh web` | `$DSH_HOME/profiles/web/cordis.patch.yml` |
| `deepseek-harness-desktop` | DeepSeek Harness 桌面应用 | `$DSH_HOME/profiles/desktop/cordis.patch.yml` |

`DSH_HOME` 未设置或为空白时为 `~/.dsh`。Desktop 与 CLI 共用产品数据和用户级
Skills，但有各自的 Profile、依赖及激活状态。改变 Arca 的配置路径只改变写入目标，
不会替 Harness 切换 Profile、初始化安装、修改 bundle 或迁移文件。

原生启动链是：Profile 选择的 bundle patch → Profile patch → home
`cordis.patch.yml` → 启动时额外 `--patch`。一条 Cordis patch 的 `config` 替换
该插件条目的完整配置。Arca 因此逐叶保留现有配置，并检查与本次操作有关的 home
覆盖；不能把普通 `mcpServers` 或一个模型根对象直接塞进此文件。

新建 override 前需要一个已由 Harness 初始化的 Profile，以及可证明为空的官方
默认继承结构。遇到不明的自定义 bundle、依赖覆盖、禁用插件、重复身份、动态受控字段
或更高层冲突时，Arca 拒绝写入并给出原因。既有完整 override 可按已核验字段编辑。
Arca 不执行 YAML 的 `!!js`，也不推断某次启动额外的 `--patch`。

首次为 `mcp-manager` 建立 config override 时，还需从该客户端自己的安装载体
核验其 manager 契约；CLI 和 Desktop 不相互借用安装证明，也不会跳过 PATH 前面的
旧版去选择后面的新版。旧版或无法核验的载体拒绝首次写入。既有完整 manager
配置仍按用户已有声明编辑，不把它当作运行时已挂载的证明。
当前安装证明覆盖标准 npm 的 CLI 链接和正式 Desktop 的 `app.asar`，绑定准确
`0.2.2-rc.1` 包身份、依赖与五份官方 Web patch 摘要。自定义 wrapper、开发安装、
其他版本和被修改的 bundle 保持未核验，不执行程序来补证据。

官方依据：[Profile 合成](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/boot/app-boot/README.md)、
[启动层次](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/cli/src/profile-boot.ts)、
[Desktop Profile](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/apps/desktop/src/paths.ts)。

## Models

自定义模型使用 `llm-pi-ai` 条目的 `config.providers`，新会话默认模型由
`agent-default-model.config.provider` 与 `model` 决定。原生允许多个自定义模型。
Arca 为每个已添加 Model 保留稳定的 Provider route，添加备用模型不会替换当前默认。
已开始请求的会话保留其会话模型；“默认模型”不代表强制改变所有正在运行的会话。

本次 writer 仅开放原生自定义 API 明确支持的三种协议：

- OpenAI Chat Completions：`openai-completions`。
- OpenAI Responses：`openai-responses`。
- Anthropic Messages：`anthropic-messages`。

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

Web/Desktop 通过在 `cordis.patch.yml` 顶层 `- insert:` 列表中插入 `@deepseek-ai/dsh-mcp-client`
插件项持久化 MCP 连接，每个条目分配独立的 `id: mcp-<serverName>` 与 `config.serverName`。
Arca 写入 stdio 或 Streamable HTTP 原生格式，并保留超时、重连与未知策略。名字须符合原生
`[A-Za-z0-9_-]{1,32}`；SSE 和无法表达的跨 transport 参数明确拒绝，不静默丢弃。

原生通过插件条目级的 `disabled: true` 暂停挂载连接，缺省或 `disabled: false` 为启用。
Arca 的观察、快照切换和删除均以完整插件条目为边界；清空 MCP 时清理所有 managed 客户端项，
保留兄弟插件与模型的原有配置。MCP 和 Models 可以共用同一个 Profile 文件，但只修改各自拥有的行与字段。

未接管的配置声明可以从 MCP 详情查看单项脱敏 JSON 和源路径。查看不要求密钥解锁，
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
任意深度的 Skill 树。插件/内置 Skill 的来源观察与中央 Skill 托管是不同状态。

Instructions 沿用原文件编辑：DSH home 与 agents home 下的用户级 `AGENTS.md`。
不建立中央正文副本，不自动修改原生加载策略，也不新建或迁移其他指令文件。

官方依据：[Skills roots 与优先级](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/skill/skill-filesystem/README.md)、
[原生 home 解析](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/util/home-paths/src/index.ts)、
[用户 Instructions](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/context/agent-instructions/src/files.ts)。

## 其他原生扩展

Harness 的扩展是 Cordis bundle、插件条目与 agent preset。`package.json` 的
`dsh.profile.bundles` 是选择声明；安装元数据、配置禁用、运行时激活和会话使用分别
表示，不能把 npm 依赖或缓存目录直接当成已启用插件。
Arca 观察 Profile 内明确选择的 bundle 和 dependency，只在绑定到该 Profile
`node_modules` 内的真实包元数据后显示“已安装”；无法解析的包保留“已声明”，
实际激活状态保持未知。跨出该依赖树的别名不用于推断组件。

插件 Skill 仅对已核验的官方 `@deepseek-ai/dsh-agent-preset` 的 `0.2.1-rc.3` 和
`0.2.2-rc.1` 发布契约识别，读取包元数据和 `SKILL.md` 的存在性，在 Agent 的 Skills 页显示
名称和插件来源，不读取正文。其他包不能仅凭一个同名 `skills/` 目录获得该能力。
bundle patch 中的 MCP 插件条目只显示静态引用，不伪造运行时 MCP 服务或连接状态。

Agent preset 是 `dsh-agent-preset` 声明的子插件树。Hook 是 Cordis 事件扩展点，
不是一套已经核验的用户级 `agents/*.md` 或 `hooks.json` 文件规范；Arca 不伪造这些
目录的 writer。只读插件来源、内置组件和已核验 Skill 声明不提供删除能力。

官方依据：[插件管理](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/boot/plugin-manager/README.md)、
[Agent presets](https://github.com/deepseek-ai/deepseek-harness/blob/fcc0ad462172522aaf5c93a7b2e01f6b9f0b0159/packages/preset/agent-preset/README.md)。

## 本次验证范围

生产 CLI 编译通过。隔离 HOME 下通过真实 Arca CLI 检查三种模型 API、多模型
默认切换与移除、MCP 启停、两个 Profile 的独立写入、Models/MCP 共用文件的
注释与 inode 保留、更高层覆盖拒写、共享 Skills 生命周期和自定义 home 解析。
插件观察检查了声明/安装区分、官方 preset Skill 来源及静态 MCP 引用。
后续版本边界检查覆盖九个真实 Arca CLI 隔离场景，包括旧版/未知/修改后 bundle
拒写、首个旧 CLI 不跳到后方新版、Desktop 不借用 CLI、已核验 npm/ASAR 首次
写入，以及既有完整 manager 配置的保留。旧版 preset Skill 的识别另有单独检查。

按当前项目极速模式保留回归测试代码，没有运行测试套件；没有以这些文件检查
冒充 Harness 会话验收，也没有进行真实模型推理、MCP 连接或账户登录验证。
