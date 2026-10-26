# Arca Agent 覆盖与接入审计

调研日期：2026-10-04。该调研完成时对应本地开发预览，尚未提交、推送、升版或发布；后续交付由单独的发布请求决定。

## 结论与范围

原先的 Arca 并非缺少 Claude Code、Codex CLI、Cursor、OpenCode、Pi、Gemini、Qoder 等主流名称，主要缺口是同品牌的独立产品形态，以及新出现的本地 Agent。原始已核验定义为 75 个，其中 61 个有 MCP 文件契约、57 个有用户级 Skills 目录。本轮新增 13 个已核验入口，达到 **88 个定义、73 个 MCP 目标、68 个 Skills 消费者**。这三个数字按契约声明计数，包含共用物理文件的产品，不代表本机安装数或独立配置文件数。

更新后的发现目录为 **246 项**，与已核验定义去重合并后 **271 个身份**。目录刷新补充了官方产品、Glama 客户端和 ACP Registry 的新增项目；ACP 的启动身份不能证明 MCP、Models 或 Skills 的文件写入契约。

本次新增入口实际开放 **12 个 MCP 与 11 个 Skills 能力**。没有为新身份自动复制 Model writer：共享后端、模型凭据获取和环境注入需要分别核验，不能仅凭 JSON 看起来相似就宣称三个能力全部支持。

“知名/流行”采用官方持续维护、实际发布的产品形态、较大的开源关注度以及多种主流客户端接入作为筛选信号；没有可比的活跃用户量，不能把 GitHub Star 当市场份额。调研覆盖 CLI、桌面、IDE 插件和桌面编排工具，不把仅有 SDK 的框架当成可直接安装的 Agent。

## 已落地的能力矩阵

| 名称 | 稳定 ID | 形态 | 用户级 MCP 目标 | 用户级 Skills | MCP 传输 |
|---|---|---|---|---|---|
| Cline CLI | `cline-cli` | cli | `~/.cline/data/settings/cline_mcp_settings.json` | `~/.cline/skills` | stdio/http |
| Kiro CLI | `kiro-cli` | cli | `~/.kiro/settings/mcp.json` | `~/.kiro/skills` | stdio/http |
| Junie CLI | `junie-cli` | cli | `~/.junie/mcp/mcp.json` | `~/.junie/skills` | stdio/http |
| Goose | `goose-desktop` | desktop | `~/Library/Application Support/Block/goose/config/config.yaml` | `~/.agents/skills` | stdio/http |
| Kilo Code | `kilo-vscode` | plugin | `~/.config/kilo/kilo.jsonc` | `~/.kilo/skills` | stdio/http |
| Codex | `codex-ide` | plugin | `~/.codex/config.toml` | `~/.agents/skills` | stdio/http |
| MiMoCode | `mimo-code` | cli | `~/.config/mimocode/mimocode.jsonc` | `~/.config/mimocode/skills` | stdio/http |
| DeepSeek Harness | `deepseek-harness` | cli | `—` | `~/.dsh/skills` | — |
| jcode | `jcode` | cli | `~/.jcode/mcp.json` | `~/.jcode/skills` | stdio |
| Jan | `jan-desktop` | desktop | `~/Library/Application Support/Jan/data/mcp_config.json` | `~/.jan/skills` | stdio/http |
| Jan Agent | `jan-cli` | cli | `~/Library/Application Support/Jan/data/mcp_config.json` | `~/.jan/skills` | stdio/http |
| AnythingLLM | `anythingllm` | desktop | `~/Library/Application Support/anythingllm-desktop/storage/plugins/anythingllm_mcp_servers.json` | `—` | stdio/http |
| IBM Bob | `ibm-bob` | ide | `~/.bob/settings/mcp.json` | `—` | stdio/http |

上述 macOS 默认路径均可以通过 Arca 已有配置入口调整。Jan 用户若移动过 data folder，需要选择实际 `mcp_config.json`；自定义路径不被默认值覆盖。

## 核验发现与实现选择

### CLI / IDE 的共享后端

- **Cline CLI**：官方文档仍写 `~/.cline/mcp.json`，但当前源码的 `resolveMcpSettingsPath()` 返回 `~/.cline/data/settings/cline_mcp_settings.json`。本轮按源码接入并复用嵌套 transport codec，不新增旧路径回退、迁移或兼容读取。来源：[解析路径源码](https://github.com/cline/cline/blob/39ff2359f7e08231281539696e48a166ce49270c/sdk/packages/shared/src/storage/paths.ts)、[SDK MCP Schema](https://github.com/cline/cline/blob/39ff2359f7e08231281539696e48a166ce49270c/sdk/packages/core/src/extensions/mcp/config-loader.ts)、[官方文档问题记录](https://github.com/cline/cline/issues/11671)。
- **Kiro CLI**：当前官方配置表确认与 IDE 共用 `~/.kiro/settings/mcp.json` 和 `~/.kiro/skills`。没有再接入已被 Kiro CLI 替代的 Amazon Q CLI 老路径。来源：[配置范围](https://kiro.dev/docs/cli/chat/configuration/)、[Q CLI 迁移说明](https://kiro.dev/docs/cli/migrating-from-q/)。
- **Junie CLI**：当前官方文档确认与 Junie IDE 插件共用用户级 JSON MCP。用户 Skills 位于 `~/.junie/skills`，也读取跨 Agent 的 `~/.agents/skills`。旧版本曾使用不同多态格式；本轮只支持当前文档契约。来源：[MCP](https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html)、[Skills](https://junie.jetbrains.com/docs/agent-skills.html)。
- **Kilo Code VS Code**：当前官方插件已经采用 CLI 后端，全局配置入口指向 XDG `kilo/kilo.jsonc`；不是历史 Roo 分支的 globalStorage 文件。来源：[当前插件配置路径](https://github.com/Kilo-Org/kilocode/blob/76bcfd40be616a72f4697b3041565f322245b462/packages/kilo-vscode/src/kilo-provider/config-file.ts)。
- **Codex IDE**：官方明确 CLI 与 IDE 扩展共享配置层。新增 IDE 身份用于形态区分、启动宿主和 MCP/Skills 消费；没有建立第二套 Models 选择逻辑。来源：[共享配置层](https://learn.chatgpt.com/docs/config-file/config-basic)。

### 新 CLI

- **MiMoCode**：官方仓库约 1.36 万 Star（本次 GitHub API 快照），是 OpenCode 分支但有独立命名空间和配置。新增 JSONC writer，使用 `mcp`、`local/remote`、command 数组、environment；不写不支持的 cwd，不把 SSE 冒充支持的远程协议，保留模型、permission、sampling、oauth 与 timeout。原生停用条目保持只读，需先在 Agent 中启用后再由 Arca 更新。Skills 使用官方用户目录和共享 `.agents` 目录。来源：[配置源码](https://github.com/XiaomiMiMo/MiMo-Code/blob/6babeb0b98f9b4818bddf04a4331edfee04dbf85/packages/cli/src/config/config.ts)、[MCP Schema](https://github.com/XiaomiMiMo/MiMo-Code/blob/6babeb0b98f9b4818bddf04a4331edfee04dbf85/packages/cli/src/config/mcp.ts)、[Skills](https://github.com/XiaomiMiMo/MiMo-Code/blob/6babeb0b98f9b4818bddf04a4331edfee04dbf85/packages/cli/src/skill/index.ts)。
- **DeepSeek Harness**：官方仓库约 24.3 万 Star，仍是 developer preview。MCP 连接挂在插件树中，不能用通用 `mcpServers` 映射安全写入；当前只开放 Skills 分配，运行配置必须启用 filesystem skill provider。来源：[产品](https://www.deepseek.com/en/harness/)、[Skill provider](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/skill/skill-filesystem/README.md)、[MCP 插件](https://github.com/deepseek-ai/deepseek-harness/blob/5badb15009ae1756c3afe0ae0cef1faafc290ccc/packages/mcp/mcp-client/README.md)。
- **jcode**：从官方网站反查到 `1jehuang/jcode`，排除了多个同名仓库；官方仓库约 2.03 万 Star。源码序列化键是 `servers`，`mcpServers` 是上游接收的另一个名称，当前仅运行 STDIO。Arca 使用原生序列化键；若已有文件使用不同键，拒绝新增第二个映射，提示设置实际键，不自动迁移。保留 shared、timeout_secs 与停用策略；HTTP、SSE 和 cwd 会被拒绝。来源：[官网](https://jcode.sh/)、[MCP 格式](https://github.com/1jehuang/jcode/blob/77f4f9d50bb41ae849da9ffaab7ea25e7815e096/crates/jcode-base/src/mcp/protocol.rs)、[Skills](https://github.com/1jehuang/jcode/blob/77f4f9d50bb41ae849da9ffaab7ea25e7815e096/crates/jcode-base/src/skill.rs)。

### 桌面与 IDE

- **Goose Desktop**：与 CLI 共享后端，但保留独立启动入口。官方说明页给出的 Linux 风格路径不适合直接套到 macOS；当前源码仍使用 etcetera 的 `Block/goose` 平台路径，所以保留已有正确 macOS 目标。Models 仍从现有 Goose 能力入口/客户端设置管理，本轮不新增桌面凭据投影。来源：[配置路径源码](https://github.com/aaif-goose/goose/blob/591edd47cf2cfea4957d720c607cf2a4def8673d/crates/goose/src/config/paths.rs)、[配置文档](https://goose-docs.ai/docs/guides/config-file/)。
- **Jan Desktop / Jan Agent**：官方当前 CLI 和桌面共享 MCP store，用户 Skills 位于 `~/.jan/skills`；CLI 仍是 preview。新增原生 `active` 开关的读取、更新和并发快照校验，保留 official、timeout、工具权限和独立 OAuth 存储。只新增 STDIO 与 HTTP 写入；源码将旧 SSE 按 Streamable HTTP 连接，因此不新增 SSE 配置。来源：[Jan Agent](https://www.jan.ai/docs/agent)、[MCP parser](https://github.com/janhq/jan/blob/14a720628f9592c8e60f7e081ef733076db8a6a9/src-tauri/src/core/mcp/models.rs)、[共享 data folder](https://github.com/janhq/jan/blob/14a720628f9592c8e60f7e081ef733076db8a6a9/src-tauri/src/core/app/commands.rs)、[Skills](https://github.com/janhq/jan/blob/14a720628f9592c8e60f7e081ef733076db8a6a9/src-tauri/src/core/agent/skills.rs)。
- **AnythingLLM**：官方 Desktop 有稳定 MCP 文件，支持 STDIO、SSE 和 Streamable HTTP。专用 codec 写 `streamable` 并在读取时归一化；保留嵌套 autoStart 和 suppressedTools。autoStart=false 的既有条目拒绝静默更新。它的内置 Agent Skills 使用另一种插件格式，不伪装成 SKILL.md 分配。来源：[Desktop MCP](https://docs.anythingllm.com/mcp-compatibility/desktop)、[macOS storage](https://docs.anythingllm.com/installation-desktop/storage)、[MCP 源码](https://github.com/Mintplex-Labs/anything-llm/blob/feb04ca0a57cda6d0b3a69c62578f0af44d388fc/server/utils/MCP/hypervisor/index.js)。
- **IBM Bob**：IDE 已有官方全局 MCP 路径，因此从目录项升级为可管理目标；IDE 远程写 `url` + `streamable-http`。Bob Shell 使用同一个路径却使用 `httpURL`，不能混写。发现任一 Shell 远程条目后，整个文件的 IDE 写操作拒绝执行；Shell 暂不开放 writer。来源：[IDE](https://bob.ibm.com/docs/ide/configuration/mcp/mcp-in-bob)、[Shell](https://bob.ibm.com/docs/shell/configuration/mcp/mcp-bobshell)。

共享文件不等于复制配置。CLI/Desktop/IDE 保留各自的稳定身份和启动方式，MCP/Skills 按现有 Core 物理目标机制处理影响范围；没有在 React 中维护第二套写入规则。

## 其他知名候选：目前为何只保留发现

| 产品/形态 | Arca 调研前状态 | 本轮结果与原因 | 官方来源 |
|---|---|---|---|
| GitHub Copilot app / Desktop | 缺少独立桌面身份 | 新增发现项；官方有 MCP/Skills UI，但没有确认可直接写入的全局文件，不推定与 CLI 共用 | [定制](https://docs.github.com/en/copilot/how-tos/github-copilot-app/customize-github-copilot-app) |
| Xiaomi MiMo Desktop | 缺少桌面身份 | 新增发现项；官方 README 确认 Desktop beta 使用 MiMoCode 核心，但不能据此假定配置路径一致 | [官方 beta](https://github.com/XiaomiMiMo/MiMo-Code#xiaomi-mimo-desktop-beta) |
| Manus Studio | 缺少桌面身份 | 新增发现项；官方桌面端存在，未核验到稳定本地 MCP/Skill writer | [桌面产品](https://manus.im/desktop) |
| Aider / CLI | 不在目录 | 新增发现项；有用户模型配置，但没有据此新增 MCP 或明文模型凭据 writer | [官方配置](https://aider.chat/docs/config.html) |
| CodeRabbit CLI | 不在目录 | 新增发现项；主要是代码审阅工具和其他 Agent 的技能/插件集成，不把它当通用 MCP 消费者 | [CLI](https://docs.coderabbit.ai/cli/index) |
| Refact.ai / IDE 插件 | 不在目录 | 新增发现项；需进一步核验当前个人配置与 agent 模式格式 | [官方仓库](https://github.com/smallcloudai/refact) |
| Tabby / IDE 插件 | 不在目录 | 新增发现项；官方 tabby-agent 主要是 LSP，不能等同为 MCP 客户端 | [官方仓库](https://github.com/TabbyML/tabby) |
| Conductor / Desktop | 不在目录 | 新增发现项；官方明确继承 Claude/Codex/Cursor 的各自用户配置，不新增统一 Conductor writer | [MCP 参考](https://www.conductor.build/docs/reference/mcp) |
| Superset / Desktop | 不在目录 | 新增发现项；是多 Agent 工作区，内置 MCP 示例属于项目配置 | [官方产品](https://superset.sh/) |
| Emdash / Desktop | 不在目录 | 新增发现项；多 Agent 编排和隔离工作区，未核验统一用户级写入契约 | [官方仓库](https://github.com/generalaction/emdash) |
| T3 Code / Desktop | 不在目录 | 新增发现项；有多后端和会话级 MCP，配置/凭据因后端与账号而异 | [官方仓库](https://github.com/pingdotgg/t3code) |
| JetBrains Air / Desktop、IDE | 已有只读目录，分类过粗 | 保留发现；官方 Skills/MCP 文档聚焦仓库配置或被托管 Agent，不能写项目 `.air/` | [Skills](https://www.jetbrains.com/help/air/skills.html) |
| BLACKBOX CLI | 已有目录 | 保留发现；官方命令可确认产品，但尚未确认当前全局客户端文件和连接 schema | [命令参考](https://docs.blackbox.ai/features/blackbox-cli/commands-reference) |
| GitHub Copilot for Xcode | 已有目录 | 保留发现；已确认 MCP 功能，尚未核验当前稳定用户级存储契约 | [官方 MCP](https://docs.github.com/en/copilot/customizing-copilot/extending-copilot-chat-with-mcp) |
| Cherry Studio | 已有目录 | 保留发现；官方 UI 配置存在，不凭 UI 导出 JSON 推定它监听某个全局文件 | [官方配置](https://docs.cherry-ai.com/cherry-studio-wen-dang/en-us/advanced-basic/mcp/config) |
| Chatbox、ChatWise、Msty Studio、5ire | 已有目录 | 没有把发现记录升级成未经审计的写入能力；后续应逐个核验实际设置存储 | [现有目录来源](https://glama.ai/mcp/clients) |
| Qodo、Replit Agent、Lovable、Devin | 已有目录 | 产品已记录；主要有云端/组织/项目范围，不能伪造本机全局 writer | [Qodo](https://docs.qodo.ai/)、[Replit](https://docs.replit.com/replitai/integrations/mcp)、[Lovable](https://docs.lovable.dev/integrations/mcp-servers)、[Devin](https://docs.devin.ai/work-with-devin/mcp) |
| Vibe Kanban | 未纳入重点管理 | 官方已宣布 sunsetting、转社区维护，本轮不列为优先新增写入目标 | [官方状态](https://www.vibekanban.com/) |

仅发现项可通过 CLI 的 Agent inventory 查询；桌面选择器沿用已核验图标的可见性过滤，也可以显示有图标的只读目标；本轮未恢复独立的只读目录 Tab，未补图标的发现项继续隐藏。新增 13 个管理入口都已补齐图标/别名与形态标记。

## 实现与验证边界

- `data/agents.json` 继续是核验身份/路径/Skills 的权威；`agent-launchers`、安装/帮助链接仅提供入口。新增原生格式集中在 Rust Core 的 MCP codec 和已有安全 JSON adapter，CLI/Tauri/React 不复制配置写入逻辑。
- 复用原地写入、CAS、备份、未知字段保留及 shared-target 机制；没有新增旧中央目录兼容代码，没有导入或改动真实 Agent 配置。
- 新增官方格式的脱敏 fixtures、共享形态 round-trip、策略保留、原生开关、无效文件/混用格式拒写用例；同步修正旧测试中滞后的目录数量断言。
- Rust workspace 生产编译检查、CLI 构建与前端生产构建通过；隔离 HOME/ARCA_HOME 的 CLI inventory 查询返回 271 个身份，新增 13 个入口的 MCP/Skill 能力与声明一致。
- 按当前极速模式，本轮没有执行测试套件、fmt/clippy、图标检查脚本或发布验证。新增格式用例已经写入，尚未运行；没有安装全部外部 Agent 做真实端到端连接或发送付费模型请求。
- 完成后重启 `npm run tauri dev`，使用真实 HOME 和 `~/.arca` 做本地预览。开发预览不等同于已安装 Stable，也不会替换 `/Applications/Arca.app`。

## 可重复来源与后续优先级

目录由 `scripts/update-agent-catalog.mjs` 重建，2026-10-04 收集 Glama 与 ACP Registry，ACP 固定提交为 `5a70ccd34483b957b56497d54fee0b59821d5d73`。上述可写契约按各官方仓库的不可变提交审计，避免移动 main/master 的文档变更无法追溯。

目录文件 SHA-256：`176bd436aa8d93354c3f008340a436aef17cf6daa70c7e9ce94aaa3c65f3baec`。

后续最有价值的工作是独立核验 GitHub Copilot app、MiMo Desktop 和 JetBrains Air 的全局契约，以及为 MiMoCode/Jan/jcode 增加安全的原生 Models writer。这需要实际存储、认证和当前模型选择的证据；不会用默认 JSON 字段或复制旧 Agent 规则充数。
