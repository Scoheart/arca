# Agent 资产配置审计 · 2026-11-05

本次核对 MCP、Models、Skills 三类主资产及原生插件组件的可见性，覆盖当前注册表的 **98 个已审计 Agent**，对 73 个 MCP writer 执行隔离的原生格式读写、快照和恢复矩阵，并核对全部 98 项的 Core 公共能力投影。MCP 连接解码、配置可见性和写入所有权是三个独立结果：OAuth、未知 transport 或宿主管理的声明，不应因无法接管而从查看入口消失。

这是源码、官方格式和隔离 fixture 的检查；不是在本机安装并执行 98 个厂商产品的端到端推理验收。发现目录中的产品也不因目录记录而升级为可写能力。各 Agent 原生契约原先核验的日期仍以注册表为准，没有统一改成今天。没有执行第三方 Agent、模型请求或读取私有原生配置/数据库/凭据来代替这些检查。

## 覆盖与缺口

| 能力 | 当前结果 | 含义 |
|---|---:|---|
| MCP 写入入口 | 73 / 98 | 原生 JSON/JSONC、TOML map/list、YAML map/list；独立连接 codec 与保留策略 |
| MCP 配置查看 | 上述入口及已观测原生插件来源 | 按身份按需读取单项，展示脱敏 JSON 和源路径；不要求先导入 |
| Skills 用户级目录 | 72 / 98 | Core 声明的路径和物理 target；插件内置 Skills 的只读观察不意味着可写目录 |
| Models 托管 | 18 / 98 | 有已核验的原生写入/凭据交付契约 |
| Models 原生指引 | 5 / 98 | 保留客户端自己的设置流程，未宣称 Arca 可自动写入 |
| 原生插件观察器 | 57 / 98 | 版本、路径和宿主范围受观察器证据约束；缓存、安装、启用、会话生效分别表示 |

以下 25 项目前没有 MCP writer；部分可查看插件声明或管理 Skills，不能根据产品名猜测配置路径：

`capy`、`cline-desktop`、`codebuddy-cn-ide`、`codebuddy-ide`、`cortex-code`、`cortex-code-desktop`、`deepseek-harness`、`dirac`、`docker-agent`、`freebuff`、`minion-code`、`openclaw`、`openclaw-desktop`、`poolside`、`qoder-cli-cn`、`qoder-cn`、`qoderwake`、`qoderwake-cn`、`qoderwork-cn`、`qwenwork-cn`、`raycast`、`theiaai-theiaide`、`trae-cn`、`trae-ide`、`zencoder`

仅提供 Models 原生指引：`kimi-code`、`kimi-code-desktop`、`minimax-code`、`qoder`、`warp`。

支持 Models 托管：`claude-code`、`claude-desktop`、`codex`、`crush`、`factory-droid`、`goose`、`grok-build`、`hermes`、`kilo-code`、`kilo-desktop`、`mistral-vibe`、`opencode`、`opencode-desktop`、`pi`、`qoder-cli`、`qoder-desktop`、`qwen-code`、`zcode`。

其他 75 项尚无已核验 Models 接管入口。订阅登录、宿主账号路由、服务端托管、项目或组织设置，不能直接等同为可导出的用户级 API Provider。完整按项状态见后表；`—` 表示没有该能力声明，不表示厂商产品本身不支持。

## 已修复的问题

- Goose/Continue 嵌套 YAML 节点不能按脱离父级的文本重新解析，否则缩进损坏导致有效 MCP 漏扫。改为遍历 lossless YAML 结构，保留标量类型，拒绝歧义和不支持的别名语义。
- 现存原生私有 Model/auth 文件曾在受跟踪写入后单独 chmod，导致第二个 Profile 的 CAS 与回滚证据失配。权限收紧和原地写入现在是同一个受跟踪操作，保留原 inode，失败恢复原权限。
- Tauri 的 Hook/Subagent 命令经显式 application 用例调用 Core，Model 返回类型也从 application 暴露，避免前端依赖底层资源引擎；没有复制写入逻辑或更改这些引擎的生命周期语义。
- 并发写入拒绝发生在修改之前时，不再留下本操作的假恢复状态；父目录已替换时，在触及原文件前拒绝。
- macOS HOME 尾随斜杠会削弱插件目录的 NOFOLLOW 检查；规范化基目录并检查 HOME 边界，保持插件配置读取约束。
- Models 安装探测现在遵守测试系统目录隔离，空白 TestHome 不再继承真实系统的 App/二进制安装状态；生产路径保持原义。
- Skills 消费关系移除绑定精确物理目录差分，保留通过其他兼容目录仍可读取的 Agent；外部目录只参与不可变快照/CAS，不授予目录写入权限。中央目标已缺失的精确链接经过受核验父链与链接 inode CAS 才清理，外部、逃逸和循环链接保持原样。
- 中央 Skills 详情页恢复了原先丢失的生命周期回调与副本身份选择；只读、忙碌和缺失 owner 时不取得写入权限，插件/内置 Skills 仍使用独立只读卡片。
- Goose stdio 的 `env` alias 正确读取并保留原拼写；HTTP 更新不再删除原生 `envs`、`env_keys`。同名 `env`/`envs` 或畸形字段拒写；跨 transport 存在环境上下文时拒绝静默移动/删除这些值。
- Agent 的添加入口按 Core 的实际 MCP/Model/Skills 写入能力禁用，Model 协议和 MCP transport 不兼容项不可选。外部资源和插件 Skills 不再产生虚假的可写入口。

- Goose / Goose Desktop 按原生 `enabled` 读取停用状态，新建连接显式写入 `enabled: true`；已有 `false` 不被覆盖。原生切换绑定完整条目快照并保留注释、策略和 inode。清空只移除可管理的 MCP，保留 builtin、platform、未知扩展和引用；针对这些非 MCP 项的修改和快照删除明确拒绝。

## MCP 查看链路

Desktop Agent MCP 名称/详情 → `inspect_mcp_configuration` → Core `inspect_configuration` → 已核验 Agent 用户级目标或重新观测的插件身份 → 有界文件读取 → 原生 adapter 的 `inspect_text` → Core 脱敏 → 只读对话框。

配置声明名的枚举独立于 transport 和 credential writer policy；普通消费卡片已表示的条目会去重，剩余条目标为“配置声明 · 仅查看”，不推测其归属，也不提供收录、切换或删除。配置损坏、不可读或歧义时明确报告，不能用空对象冒充成功。源码生成或归档内没有独立文件的 MCP 会显示来源及不可读取原因，不执行宿主代码。

环境值、请求头、OAuth/常见凭据、带引号的命令凭据和 URL 参数在离开 Core 前隐藏。未知字段、策略和非敏感连接结构保留用于理解配置；这不是完整 Agent 文件导出。查看不触发接管、凭据解锁或写入。

声明可查看不等于已连接；宿主多层配置、会话覆盖和运行时扩展启用仍由原生客户端决定，查看入口不推断最终生效层。

CLI 同样可用：`arca mcp show 'docs::stdio' --agent cursor --json`。没有 `--agent` 时仍查看中央资产。

## 原生格式来源复核

- [Goose 扩展配置](https://github.com/aaif-goose/goose/blob/bfe8ccd23de1d2c47a4cb2970d522f093ae7f2ba/crates/goose/src/config/extensions.rs)：`ExtensionEntry.enabled` 必需，原生会过滤停用项；`extensions` 同时容纳内置和 MCP 扩展，不能整节删除。证据 commit 为 2026-11-04 main。
- [VT Code MCP 指南](https://github.com/vinhnx/VTCode/blob/4c2a8f65d3fd0a8d3db1eff75d0f654ab562bd87/docs/guides/mcp-integration.md)与[配置类型](https://github.com/vinhnx/VTCode/blob/4c2a8f65d3fd0a8d3db1eff75d0f654ab562bd87/crates/codegen/vtcode-config/src/mcp.rs)：原生为 `[[mcp.providers]]`，Arca 已正确走 dotted-path list adapter；无需改成 TOML map。总开关和 provider 开关继续是独立边界。
- 其他契约沿用 [逐 Agent 官方证据](../analysis/agent-capability-audit/verified-agents-evidence.md)、[格式与路径指南](../website/guide/agents.md)、[Models 研究](agent-provider-research-2026-09-07.md)。既有核验不等同于今天重新验证每个厂商的最新发布。

## 测试维护

本轮全量检查发现快交付期间未运行的旧用例：旧 IPC 包装、已更换的导航/Inspector 流程、暂停前 Quality workflow 和旧资产目录断言。按现行契约修正 fixture 和交互断言，不恢复旧流程。另退休 7 个仅匹配旧 CSS 颜色/像素/源码字符串的历史套件（共 29 个用例），以及 Models 中 11 个同类历史镜像用例，不把它们计作通过；导航、对话框、Skills 和 Agent 操作继续通过组件交互验证。新增只读 MCP 对话框回归覆盖 StrictMode 单次读取、失败重试、跨 Agent 延迟响应、身份错误响应和无独立声明说明。原生视觉验收仍需真实窗口证据，源码正则和 jsdom 都不能代替。

## 验证边界

Desktop 最终全套为 74 个文件、452 项用例通过，TypeScript/Vite 生产构建通过；CLI 94 项用例、发布契约 16 项用例及隔离发布器模拟通过。Core 首次完整测试暴露的 29 项失败已全部闭环。最终覆盖 1,658 项通过、2 项按原规则跳过；最后一轮并行测试中，限时模拟 CLI 版本探测曾返回一次 None，源码未改，随后完整重跑同序单元套件为 1,300/1,300 通过，其余 358 项集成测试在完整运行中全部通过。这个单次波动的具体原因尚未确认，没有调大生产时限或删掉断言。

自动化用例使用 TestHome/独立 ARCA_HOME；没有启用会触及真实 Keychain 的 ignored 测试。开发预览使用真实 HOME 和现有 Arca 数据，并与正式安装版分开。REA 已确认最终开发 App 的可见原生窗口（1200×821），并在凭据保持锁定时实际打开 Cursor 未接管 MCP 的单项 JSON/来源路径；内置 Computer Use 详情没有删除、切换或收录操作，并明确说明由宿主代码生成配置。该真实配置样本没有敏感字段，脱敏保证来自 Core 的敏感字段 fixture 回归；900×600、精确双尺寸及长 JSON 滚动仍未验，组件测试和构建不冒充这些结果。启动初期曾观察到 0×0 窗口，约四分钟后复查已上屏，确切上屏时刻及原因未确认。发布由独立后台 owner 按准确功能 SHA、不可变 tag 与四资产验包完成，结果属于发布记录，不预写为成功。

## 逐 Agent 能力快照

来源：本轮通过的 `agent_capability_matrix` 公共 Core 投影；base SHA `7ed1802ef163af191b8d673c1d422c513b7fcf00`，`data/agents.json` SHA-256 `dd1243c66fdc3e5e9ec5b29eab2c9cbea782fdbc73d37719abefcdcb9ec1f139`。安装状态来自隔离 fixture，未列入此表。路径/身份以注册表和 Core 为权威，此表仅为本日审计快照。

| Agent / ID | MCP 格式与配置 | Models | Skills 目录 | 插件观察器 |
|---|---|---|---|---|
| Agentkube / `agentkube` | `json` `~/.agentkube/mcp.json`<br>key: `mcpServers` | — | — | — |
| Amazon Q Developer IDE / `amazon-q` | `json` `~/.aws/amazonq/default.json`<br>key: `mcpServers` | — | — | — |
| Amp / `amp` | `json` `~/.config/amp/settings.json`<br>key: `amp.mcpServers` | — | `~/.config/agents/skills` | 有 |
| Antigravity / `antigravity` | `json` `~/.gemini/config/mcp_config.json`<br>key: `mcpServers` | — | `~/.gemini/config/skills` | 有 |
| Google Antigravity CLI / `antigravity-cli` | `json` `~/.gemini/config/mcp_config.json`<br>key: `mcpServers` | — | — | — |
| AnythingLLM / `anythingllm` | `json` `~/Library/Application Support/anythingllm-desktop/storage/plugins/anythingllm_mcp_servers.json`<br>key: `mcpServers` | — | — | — |
| Augment Code / `augment` | `json` `~/.augment/settings.json`<br>key: `mcpServers` | — | `~/.augment/skills` | 有 |
| BoltAI / `boltai` | `json` `~/.boltai/mcp.json`<br>key: `mcpServers` | — | — | — |
| Capy / `capy` | — | — | — | — |
| ChatMCP / `chatmcp` | `json` `~/Library/Application Support/ChatMcp/mcp_server.json`<br>key: `mcpServers` | — | — | — |
| Claude Code / `claude-code` | `json` `~/.claude.json`<br>key: `mcpServers` | 托管 | `~/.claude/skills` | 有 |
| Claude / `claude-desktop` | `json` `~/Library/Application Support/Claude/claude_desktop_config.json`<br>key: `mcpServers` | 托管 | — | 有 |
| Cline / `cline` | `json` `~/.cline/data/settings/cline_mcp_settings.json`<br>key: `mcpServers` | — | `~/.cline/skills` | — |
| Cline CLI / `cline-cli` | `json` `~/.cline/data/settings/cline_mcp_settings.json`<br>key: `mcpServers` | — | `~/.cline/skills` | 有 |
| Cline / `cline-desktop` | — | — | — | 有 |
| CodeBuddy CN / `codebuddy-cn-ide` | — | — | `~/.codebuddy/skills` | 有 |
| CodeBuddy CLI / `codebuddy-code` | `json` `~/.codebuddy/.mcp.json`<br>key: `mcpServers` | — | `~/.codebuddy/skills` | 有 |
| CodeBuddy / `codebuddy-ide` | — | — | `~/.codebuddy/skills` | 有 |
| CodeWhale / `codewhale` | `json` `~/.codewhale/mcp.json`<br>key: `servers` | — | `~/.codewhale/skills` | — |
| Codex CLI / `codex` | `toml` `~/.codex/config.toml`<br>key: `mcp_servers` | 托管 | `~/.agents/skills` | 有 |
| ChatGPT / `codex-desktop` | `toml` `~/.codex/config.toml`<br>key: `mcp_servers` | — | `~/.agents/skills` | 有 |
| Codex / `codex-ide` | `toml` `~/.codex/config.toml`<br>key: `mcp_servers` | — | `~/.agents/skills` | 有 |
| Continue / `continue` | `yaml` `~/.continue/config.yaml`<br>key: `mcpServers` | — | — | — |
| Continue CLI / `continue-cli` | `yaml` `~/.continue/config.yaml`<br>key: `mcpServers` | — | — | — |
| GitHub Copilot CLI / `copilot-cli` | `json` `~/.copilot/mcp-config.json`<br>key: `mcpServers` | — | `~/.copilot/skills` | 有 |
| CoCo CLI / `cortex-code` | — | — | `~/.snowflake/cortex/skills` | 有 |
| Cortex Code / `cortex-code-desktop` | — | — | — | 有 |
| Crush / `crush` | `json` `~/.config/crush/crush.json`<br>key: `mcp` | 托管 | `~/.config/crush/skills` | — |
| Cursor / `cursor` | `json` `~/.cursor/mcp.json`<br>key: `mcpServers` | — | `~/.cursor/skills` | 有 |
| Cursor CLI / `cursor-cli` | `json` `~/.cursor/mcp.json`<br>key: `mcpServers` | — | `~/.cursor/skills` | 有 |
| DeepSeek Harness / `deepseek-harness` | — | — | `~/.dsh/skills` | — |
| Dirac / `dirac` | — | — | `~/.agents/skills` | — |
| Docker Agent / `docker-agent` | — | — | `~/.agents/skills` | — |
| Factory / `factory-desktop` | `json` `~/.factory/mcp.json`<br>key: `mcpServers` | — | `~/.factory/skills` | 有 |
| Factory Droid / `factory-droid` | `json` `~/.factory/mcp.json`<br>key: `mcpServers` | 托管 | `~/.factory/skills` | 有 |
| Firebender / `firebender` | `json` `~/.firebender/firebender.json`<br>key: `mcpServers` | — | `~/.firebender/skills` | — |
| Freebuff / `freebuff` | — | — | — | — |
| Gemini CLI / `gemini` | `json` `~/.gemini/settings.json`<br>key: `mcpServers` | — | `~/.gemini/skills` | 有 |
| Goose / `goose` | `yaml` `~/Library/Application Support/Block/goose/config/config.yaml`<br>key: `extensions` | 托管 | `~/.agents/skills` | 有 |
| Goose / `goose-desktop` | `yaml` `~/Library/Application Support/Block/goose/config/config.yaml`<br>key: `extensions` | — | `~/.agents/skills` | 有 |
| Grok Build / `grok-build` | `toml` `~/.grok/config.toml`<br>key: `mcp_servers` | 托管 | `~/.grok/skills` | — |
| Hermes Agent / `hermes` | `yaml` `~/.hermes/config.yaml`<br>key: `mcp_servers` | 托管 | `~/.hermes/skills` | 有 |
| IBM Bob / `ibm-bob` | `json` `~/.bob/settings/mcp.json`<br>key: `mcpServers` | — | — | — |
| Jan Agent / `jan-cli` | `json` `~/Library/Application Support/Jan/data/mcp_config.json`<br>key: `mcpServers` | — | `~/.jan/skills` | — |
| Jan / `jan-desktop` | `json` `~/Library/Application Support/Jan/data/mcp_config.json`<br>key: `mcpServers` | — | `~/.jan/skills` | — |
| jcode / `jcode` | `json` `~/.jcode/mcp.json`<br>key: `servers` | — | `~/.jcode/skills` | — |
| JetBrains Junie / `junie` | `json` `~/.junie/mcp/mcp.json`<br>key: `mcpServers` | — | — | — |
| Junie CLI / `junie-cli` | `json` `~/.junie/mcp/mcp.json`<br>key: `mcpServers` | — | `~/.junie/skills` | 有 |
| Kilo Code CLI / `kilo-code` | `json` `~/.config/kilo/kilo.jsonc`<br>key: `mcp` | 托管 | `~/.kilo/skills` | 有 |
| Kilo / `kilo-desktop` | `json` `~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/config/kilo/kilo.jsonc`<br>key: `mcp` | 托管 | `~/.kilo/skills` | 有 |
| Kilo Code / `kilo-vscode` | `json` `~/.config/kilo/kilo.jsonc`<br>key: `mcp` | — | `~/.kilo/skills` | — |
| Kimi Code CLI / `kimi-code` | `json` `~/.kimi-code/mcp.json`<br>key: `mcpServers` | 原生指引 | `~/.kimi-code/skills` | 有 |
| Kimi Code / `kimi-code-desktop` | `json` `~/.kimi-code/mcp.json`<br>key: `mcpServers` | 原生指引 | `~/.kimi-code/skills` | 有 |
| Kiro / `kiro` | `json` `~/.kiro/settings/mcp.json`<br>key: `mcpServers` | — | `~/.kiro/skills` | 有 |
| Kiro CLI / `kiro-cli` | `json` `~/.kiro/settings/mcp.json`<br>key: `mcpServers` | — | `~/.kiro/skills` | — |
| LM Studio / `lmstudio` | `json` `~/.lmstudio/mcp.json`<br>key: `mcpServers` | — | — | — |
| MiMoCode / `mimo-code` | `json` `~/.config/mimocode/mimocode.jsonc`<br>key: `mcp` | — | `~/.config/mimocode/skills` | — |
| MiniMax Code / `minimax-code` | `json` `~/.mavis/mcp.json`<br>key: `mcpServers` | 原生指引 | — | — |
| Minion Code / `minion-code` | — | — | `~/.minion/skills` | — |
| Mistral Vibe / `mistral-vibe` | `toml` `~/.vibe/config.toml`<br>key: `mcp_servers` | 托管 | `~/.vibe/skills` | 有 |
| OpenClaw / `openclaw` | — | — | `~/.openclaw/skills` | 有 |
| OpenClaw / `openclaw-desktop` | — | — | — | 有 |
| OpenCode / `opencode` | `json` `~/.config/opencode/opencode.json`<br>key: `mcp` | 托管 | `~/.config/opencode/skills` | 有 |
| OpenCode / `opencode-desktop` | `json` `~/.config/opencode/opencode.json`<br>key: `mcp` | 托管 | `~/.config/opencode/skills` | 有 |
| OpenHands CLI / `openhands` | `json` `~/.openhands/mcp.json`<br>key: `mcpServers` | — | `~/.openhands/skills` | — |
| Pi Coding Agent / `pi` | `json` `~/.pi/agent/mcp.json`<br>key: `mcpServers` | 托管 | `~/.pi/agent/skills` | 有 |
| Poolside / `poolside` | — | — | `~/.agents/skills` | — |
| Qoder / `qoder` | `json` `~/.qoder/mcp.json`<br>key: `mcpServers` | 原生指引 | `~/.qoder/skills` | 有 |
| Qoder CLI / `qoder-cli` | `json` `~/.qoder/settings.json`<br>key: `mcpServers` | 托管 | `~/.qoder/skills` | 有 |
| Qoder CLI CN / `qoder-cli-cn` | — | — | — | 有 |
| Qoder CN / `qoder-cn` | — | — | — | 有 |
| Qoder / `qoder-desktop` | `json` `~/.qoder/settings.json`<br>key: `mcpServers` | 托管 | `~/.qoder/skills` | 有 |
| QoderWake / `qoderwake` | — | — | — | 有 |
| QoderWake CN / `qoderwake-cn` | — | — | — | 有 |
| QoderWork / `qoderwork` | `json` `~/.qoderwork/mcp.json`<br>key: `mcpServers` | — | `~/.qoderwork/skills` | 有 |
| QoderWork CN / `qoderwork-cn` | — | — | — | 有 |
| Qwen Code / `qwen-code` | `json` `~/.qwen/settings.json`<br>key: `mcpServers` | 托管 | `~/.qwen/skills` | 有 |
| QwenWork / `qwenwork` | `json` `~/.qwenwork/mcp.json`<br>key: `mcpServers` | — | `~/.qwenwork/skills` | 有 |
| QwenWorkCN / `qwenwork-cn` | — | — | `~/.qwenwork-cn/skills` | 有 |
| Raycast / `raycast` | — | — | `~/.config/raycast/skills` | — |
| Roo Code / `roo-code` | `json` `~/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json`<br>key: `mcpServers` | — | `~/.roo/skills` | — |
| Atlassian Rovo Dev CLI / `rovo-dev` | `json` `~/.rovodev/mcp.json`<br>key: `mcpServers` | — | `~/.rovodev/skills` | — |
| Stakpak / `stakpak` | `toml` `~/.stakpak/mcp.toml`<br>key: `mcpServers` | — | `~/.stakpak/skills` | — |
| Step Code / `step-code` | `toml` `~/.stepcode/config.toml`<br>key: `mcp_servers` | — | `~/.stepcode/agent/skills` | — |
| Tabnine / `tabnine` | `json` `~/.tabnine/mcp_servers.json`<br>key: `mcpServers` | — | — | — |
| TheiaIDE / `theiaai-theiaide` | — | — | `~/.agents/skills` | — |
| TRAE CLI / `trae-cli` | `toml` `~/.trae/traecli.toml`<br>key: `mcp_servers` | — | — | — |
| Trae CN / `trae-cn` | — | — | — | 有 |
| Trae / `trae-ide` | — | — | `~/.trae/skills` | 有 |
| Visual Studio Code / `vscode` | `json` `~/Library/Application Support/Code/User/mcp.json`<br>key: `servers` | — | `~/.copilot/skills` | 有 |
| VT Code / `vt-code` | `toml` `~/.vtcode/vtcode.toml`<br>key: `mcp.providers` | — | `~/.agents/skills` | — |
| Warp / `warp` | `json` `~/.warp/.mcp.json`<br>key: `mcpServers` | 原生指引 | `~/.agents/skills` | 有 |
| Devin / `windsurf` | `json` `~/.config/devin/mcp_config.json`<br>key: `mcpServers` | — | `~/.config/devin/skills` | 有 |
| WorkBuddy AI / `workbuddy` | `json` `~/.workbuddy-ai/mcp.json`<br>key: `mcpServers` | — | `~/.workbuddy-ai/skills` | 有 |
| WorkBuddy / `workbuddy-cn` | `json` `~/.workbuddy/mcp.json`<br>key: `mcpServers` | — | `~/.workbuddy/skills` | 有 |
| ZCode / `zcode` | `json` `~/.zcode/cli/config.json`<br>key: `mcp.servers` | 托管 | `~/.zcode/skills` | 有 |
| Zed / `zed` | `json` `~/.config/zed/settings.json`<br>key: `context_servers` | — | `~/.agents/skills` | 有 |
| Zenflow / `zencoder` | — | — | `~/.agents/skills` | — |
