# 支持的 Agent

下表来自 Arca 发布版的核验目录与 Core 能力输出，支持按名称、准确 ID 和能力搜索。不同产品形态保留独立身份；名称相同的条目通过 CLI、Desktop、IDE 或 Plugin 区分。

<AgentReference />

## 能力口径

MCP 路径、Skills 目录和 Models 能力分别核验，不能从其中一项推断另一项。没有已确认路径与原生格式的入口不提供写入。发现目录只是后续核验的候选，不代表所有产品都能自动配置。

- MCP 列展示已确认的用户级全局配置。
- Skills 列展示主要用户级目录；兼容读取目录与共享目标在实际操作计划中列出。
- Models 显示自动配置或官方引导。协议、当前模型与凭据限制见 [Models](/guide/models)。

## 共享配置与不同入口

**Codex CLI 与桌面入口**共享 Codex 配置与用户级 Skills，Model writer 由 CLI 身份管理。**Cursor IDE 与 CLI**共享 MCP / Skills；启动入口不同，实际文件影响合并展示。

**OpenCode CLI 与 Desktop**共享原生配置，均可管理 Models。**Qoder IDE**使用独立 MCP 文件；**Qoder Desktop 与 CLI**共享 settings，Models 的选择规则不同。**QoderWork**有独立用户配置。不要仅按显示名称判断目标。

共享文件或目录的修改会展示全部受影响 Agent，不会把同一份文件当成互不相关的目标。

## 安装与版本

运行时检测与“配置文件已经存在”分开显示。macOS 应用从包信息读取版本；已核验的默认 CLI 通过限时版本查询检测，自定义 CLI 不会被自动执行。

插件宿主可以启动，不代表插件已经安装。未确认的信息保持未知。CLI 可用 `arca agent launch show <agent-id> --json` 查看同一套启动与版本信息。

## Skills 能力

Skills 仅分配到明确核验的用户级目录。只有当前机器实际可用的目标参与操作；多个 Agent 共用一个目录时，会作为同一影响组处理。Codex 的主要目录是 `~/.agents/skills`，不能把其他产品文档列出的兼容目录直接当成 Codex 的写入契约。

## 只读与官方引导

没有稳定用户级配置文件契约的产品保持只读或提供原生设置指导。Pi 的原生 MCP 契约适用于 0.99.0+；更早版本需其对应扩展。Claude 桌面端的本地 MCP 文件只接收 stdio，远程连接由客户端的 Connectors 管理。

核验方法与来源见仓库 [Agent Catalog](https://github.com/Scoheart/mux/blob/main/docs/agent-catalog.md)。
