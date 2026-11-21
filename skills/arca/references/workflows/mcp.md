# MCP 目录、来源与 Agent 分配

中央目录决定可选条目。Agent 的关系决定该客户端的配置与启停。实际连接由目标 Agent 发起。准备好 MCP 的准确名称、传输、命令或 URL，以及要改变的 Agent ID。发现客户端原有条目时先看现场和可用动作。

## 选择输入与操作

| 已有信息 | 桌面入口 | CLI 入口 |
| --- | --- | --- |
| 一条服务的命令或 URL | **MCPs → 添加 MCPs → 手动配置** | `arca mcp add KEY` |
| 一份待粘贴配置 | **MCPs → 添加 MCPs → 粘贴配置** | 单条结构化草稿用 `arca mcp save --file FILE` |
| 可重复刷新的文件 | 添加 MCP 时选 **导入配置** | `arca mcp source add-local --path FILE` |
| 远程目录订阅或官方精选 | **添加订阅** 或来源页 | `arca mcp source subscribe` 或 `add-builtin` |
| 已存在于 Agent 文件 | 目标 Agent 的 MCP 详情与可用动作 | `arca discover mcp`、`arca agent mcp status` 与可用的 `converge` |
| 加入、停用或解除 Agent 关系 | **选择 Agent → MCPs** | `arca agent mcp … --agent AGENT` |

终端交互的按钮和表单见 [TUI 指南](../tui.md)。同名 `docs::stdio` 和 `docs::http` 可同时存在。HTTP 标识的实际类型可以是 `http`、`streamable-http` 或 `sse`。先按服务与 Agent 的兼容性选择具体类型。

## 新建：STDIO 或 HTTP

下面是两条独立的预览。STDIO 示例的可执行文件和路径需换成用户指定的真实值。每个 `--arg` 传一个参数，`--cwd` 仅供 STDIO 使用。HTTP 则需实际服务的完整端点。

```bash
arca mcp add notes::stdio --command /absolute/path/to/notes-mcp \
  --arg=--root --arg /path/to/notes --tag work --dry-run --json
arca mcp add docs::http --url https://mcp.example.test/mcp \
  --http-type streamable-http --dry-run --json
```

在 GUI 手动配置里先定传输，再核对 STDIO 的命令、参数、环境项和工作目录，或 HTTP 的类型、URL、请求头。确认来源提供的命令可信且 Agent 启动环境可以定位它。创建中央条目时服务还未被 Agent 加载。

需要保存完整单条定义时，`mcp save` 接收一个 `RegistryEntry`。例如将以下草稿保存为 `notes-mcp.json`。`name`、`description`、`tags`、`config` 属于条目，`args`、`env`、`cwd` 属于 `config.stdio`。HTTP 形式用 `config.http`，其中为 `type`、`url` 和可选 `headers`。编辑已有条目时保留准确连接字段并提供原键。

```json
{
  "name": "notes",
  "description": "Notes MCP",
  "tags": ["work"],
  "config": {
    "stdio": {
      "command": "/absolute/path/to/notes-mcp",
      "args": ["--root", "/path/to/notes"],
      "cwd": "/path/to/notes"
    }
  }
}
```

```bash
arca mcp save --file notes-mcp.json --dry-run --json
arca mcp save --file notes-mcp.json --key notes::stdio --dry-run --json
arca mcp show notes::stdio --json
```

第一条用于创建，第二条用于编辑已存在的 `notes::stdio`。两条是按当前意图选用的替代路径。一旦用户授权且预览符合范围，用 `--yes` 执行选定动作，再重读准确条目。带秘密的 `env` 或 `headers` 只通过当前任务授权的受保护输入处理。`show` 提供脱敏详情，重建完整定义时以用户提供的原配置或当前表单为准。

## 添加或刷新来源

```bash
arca mcp source list --json
arca mcp source subscribe --url https://catalog.example.test/mcps.json \
  --name "Work catalog" --dry-run --json
arca mcp source add-local --path "$MCP_CATALOG_FILE" --name "Local catalog" --dry-run --json
arca mcp source add-builtin --dry-run --json
arca mcp source refresh "$SOURCE_ID" --dry-run --json
arca mcp source disable "$SOURCE_ID" --dry-run --json
arca mcp source enable "$SOURCE_ID" --dry-run --json
arca mcp source remove "$SOURCE_ID" --dry-run --json
```

- 来源 dry-run 展示将提交给 Core 的意图。联网与完整目录解析在实际刷新或订阅时发生。订阅地址应是目录文件，HTTP MCP 地址应是服务端点。
- 本地来源示例输入是一份支持的配置文件，例如 `{"mcpServers":{"docs":{"url":"https://mcp.example.test/mcp","type":"streamable-http"}}}`。以后可按来源 ID 刷新。需要批量导入前先检查识别的服务和有效范围。
- `manual`、`discovered` 是自动管理来源。刷新或移除受保护来源时按 Core 拒绝原因和实际任务处理。新的客户端外部发现只是观测。
- 同键来自多个来源时，手动来源优先。GUI 保留来源和被覆盖副本。切换或删除来源前看有效条目与既有消费者，之后重查来源的 `enabled`、`server_count`、`has_error` 和生效条目。

## 分配、启停和清理

```bash
arca agent list --json
arca agent mcp status --agent codex --json
arca agent mcp assign docs::http --agent codex --dry-run --json
arca agent mcp enable docs::http --agent codex --dry-run --json
arca agent mcp disable docs::http --agent codex --dry-run --json
arca agent mcp unassign docs::http --agent codex --dry-run --json
```

依任务选一个动作。先看已存在的关系和可用动作。分配只增加指定关系，停用会保留它，解除使用移除该 Agent 的关系。重复普通分配或启用仍可能保留外部漂移。要接管、恢复或解除管理，使用 [同步与恢复](recovery.md) 中针对观测的动作。

批量添加可把多个准确键一起传给同一个 `assign`。启停全部对应 `enable-all` / `disable-all`。`unassign --all` 可能包括外部项和停用快照，只有要清空该 Agent 的整个 MCP 范围才审阅此计划。用 `arca mcp delete KEY --dry-run --json` 审阅删除中央条目及全部消费者。

### 验收

1. 中央写入后，用 `mcp show KEY` 核对身份、传输和来源。
2. Agent 写入后，用 `agent mcp status --agent AGENT` 核对该关系、启停、现场与共享影响。GUI 则核对目标 Agent 标签内的传输和状态。
3. 用户要求验证真实连接时，在目标客户端查看实际加载情况或相关错误。STDIO 问题先看可执行文件、参数和客户端进程的工作目录或环境。HTTP 问题先区分端点、传输、认证及目标客户端的网络路径。需要重载时按客户端自己的规则操作。[诊断](diagnostics.md)

## 导出当前有效目录

普通巡检用 `mcp list/show`。`mcp export` 包含完整连接值，stdout 方式同样适用这一边界。用户明确要保存完整配置时，使用受保护目录中尚不存在的新路径。`--out` 只创建权限 `0600` 的新文件，已有目标会被拒绝。

```bash
arca mcp export --out "$NEW_PRIVATE_CONFIG_PATH" --dry-run --json
arca mcp export --out "$NEW_PRIVATE_CONFIG_PATH" --yes --json
```

验收时核对输出位置、权限和范围，汇报文件位置与条目统计。日志、聊天和提交中保留脱敏信息。

[发布版 MCP 命令](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#mcp) · [来源与优先级](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/concepts.md)
