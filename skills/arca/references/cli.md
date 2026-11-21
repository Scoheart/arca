# Arca CLI

适用于精确查询、批量操作、可审阅写入和结构化验收。命令以 Arca 4.0.2 为基准。先确认终端里的二进制、登录用户和数据根，示例中的 ID、URL 及路径均用任务的实际值替换。

```bash
type -a arca
arca --version
arca --help
arca agent mcp assign --help
```

macOS 正式 App 内置 CLI，通常从 `~/.local/bin/arca` 链到 `/Applications/Arca.app/Contents/MacOS/arca`。有多个候选时用准确二进制的绝对路径验证。`ARCA_NO_TUI=1` 让无参数调用显示帮助，`ARCA_NO_UPDATE_CHECK=1` 关闭当前进程普通命令后的每日更新检查。分配和查看状态时应保持同一用户和同一数据根。[安装、环境与数据根](workflows/settings.md)

## 先查哪一层

| 问题 | 命令 |
| --- | --- |
| 本机有哪些 Agent、身份和能力 | `arca agent list --json` |
| 单个 Agent 的 MCP、Model 和 Skill 状态 | `arca agent status codex --json` |
| 跨 Agent 对照一个能力 | `arca status --agent codex --agent opencode --capability mcp --json` |
| 一个 Agent 的精确关系和外部候选 | `arca agent mcp status --agent codex --json`，或将 `mcp` 换成 `model` / `skill` |
| 中央资产清单 | `arca mcp list --json`、`arca model list --json`、`arca skill list --json` |
| 一个中央资产的脱敏详情 | `arca mcp show KEY --json`、`arca model show PROFILE --json`、`arca skill show NAME --json` |
| Agent 原生配置中的外部发现 | `arca discover mcp --json`、`arca discover model --json`、`arca discover skill --json` |
| 全局资产、关系及后端状态概览 | `arca workspace --json` |

`status` 看期望关系和现场。`list/show` 看中央记录。`discover` 只观察，有些候选的身份、路径或值仍需核对。查一个关系时用 Agent 和能力缩小范围。`AGENT` 用 `agent list` 返回的 ID，外部候选和共享物理目标须取自对应能力的当前输出。[同步状态字典](workflows/recovery.md)

## 命令的共同写法

| 意图 | CLI 形状 |
| --- | --- |
| 中央资产 | `arca mcp …`、`arca model …`、`arca skill …` |
| 指定 Agent 的关系 | `arca agent <mcp\|model\|skill> ACTION ID --agent AGENT` |
| 预览单次写入 | `--dry-run --json`，检查计划和影响 |
| 执行已授权写入 | `--yes --json`，核对本次结果和状态。交互命令也可以走终端确认 |
| 审阅并提交同一份计划 | `arca operation review --file request.json --json` |

`assign` 建立关系，`enable/disable` 改启停，`unassign` 移除关系，`converge` 对准当前现场处理漂移。`--yes` 与 `--dry-run` 互斥。`--yes` 表达执行确认，风险、明文交付或覆盖冲突还可能需要当前计划要求的独立选择。`--dry-run` 仍可能解析下载源、访问凭据后端或生成暂存数据，按具体命令判断其输入和联网影响。

```bash
arca agent mcp status --agent codex --json
arca agent mcp assign notes::stdio --agent codex --dry-run --json
# 在确定目标和影响后，执行用户已授权的变更
arca agent mcp assign notes::stdio --agent codex --yes --json
arca agent mcp status --agent codex --json
```

`assign/unassign` 支持一次给同一个 Agent 传多个准确 ID。状态查询可跨 Agent，普通关系写入每次只传一个 `--agent`。执行多个目标时按实际配置文件或目录检查共享范围，避免并发修改同一目标。代理、终端、文件路径等选项也应按本机帮助核对。

## 自动化和最小结果

- 普通 `--json` 成功结果在 stdout，包含 `schema_version`、`ok`、`command`、`changed`、`data` 和可选 `backend`。失败用退出码及 stderr 的 `error.code` 与脱敏详情处理。输出形状按命令选择，例如 `operation review` 是 NDJSON。
- 在回复中提取准确 ID、动作、范围、状态和失败原因。提交结果为 `changed=false` 时，结合重读判断是否原本已满足。资源预览的 `data.would_change` 表示预计变化，外层 `changed` 仍为 `false`。
- 在程序里分别读取 stdout/stderr，检查退出状态再解析。TTY 提示与表格适合人读。`--no-color` 可以用于人读的日志，结构化脚本优先 `--json`。
- 核对具名文件的路径和作用。`--file -` 供明确支持的命令从 stdin 读取输入。Provider 的 `--credential-stdin` 将 stdin 留给密钥，审阅会话将 stdin 留给决定。
- `pending_convergence` 可能已经持久化中央状态。重读目标域再决定恢复，不把失败状态直接当成可重复整个批次。详见 [同步与恢复](workflows/recovery.md)。

## 按任务继续

| 要做什么 | 下一步 |
| --- | --- |
| 新建 STDIO / HTTP、维护来源、审阅出口配置、分配 | [MCP 命令与例子](workflows/mcp.md) |
| 保存 Provider、用凭据输入、创建 Profile、设 current | [Provider 与模型](workflows/models.md) |
| 检查指定 Skill 候选、导入、更新、修复 | [Skills 命令与例子](workflows/skills.md) |
| Agent launch、原生路径或运行时 | [Agent 手册](workflows/agents.md) |
| 更换代理、设置终端、语言和独立 CLI 更新 | [安装与设置](workflows/settings.md) |
| 读取 Trace 分页、查看保存的 capture、排查连接 | [诊断手册](workflows/diagnostics.md) |
| 自动化消费 JSON、审阅同一次计划、处理重试字段 | [CLI 输出和审阅协议](technical/cli-protocol.md) |

准确参数随版本启用，查询对应子命令 `--help`。版本对照可查发布版文档和源码仓的 CLI Skill。[Arca 4.0.2 CLI 文档](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md) · [Arca 4.0.2 命令参考](https://github.com/Scoheart/arca/blob/v4.0.2/skills/arca-cli/references/commands.md)
