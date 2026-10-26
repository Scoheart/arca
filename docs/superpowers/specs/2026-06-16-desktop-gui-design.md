# Arca 桌面图形化配置 — 设计文档

- 日期：2026-06-16
- 状态：已确认，待生成实现计划
- 作者：scoheart（与 Claude 共同 brainstorm）

## 1. 背景与目标

Arca 目前是一个基于 Ink/React 的终端 TUI（`@scoheart/arca`），用于跨多个 AI
编码工具统一管理 MCP 服务器配置。本设计将其能力扩展为一个**桌面图形化配置应用**。

用户的核心诉求（原话）：

> 有一个 MCP 记录仓库，能够帮我安装到项目或全局，且能支持各种各样的 agent
> 的不同的配置。

由此提炼出产品本质：**一个规范化的 MCP 服务器仓库，可一键安装到「N 个 agent ×
2 种 scope」，并能为每个 agent 单独覆写配置。**

### 已确认的关键决策

| 决策点 | 结论 |
|--------|------|
| GUI 与 CLI 关系 | **并存**。CLI/TUI 保持不变继续发布，桌面 App 是新增产物 |
| 桌面框架 | **Tauri + React**（体积小、原生体验） |
| 核心逻辑复用 | **Rust 重写 core**；CLI 仍用现有 TS core；两边共享数据 |
| agent 配置差异 | **方案 B：自动翻译 + 逐 agent 覆写**（同一服务器可对不同 agent 用不同 args/env/url） |
| 主界面信息架构 | **仓库为主视图（App Store 式）+ 矩阵总览为切换视图** |
| 代码位置 | 本仓库 `desktop/` 子目录 |
| 状态存储 | 与 CLI 共用 `~/.arca/`，数据互通 |

## 2. 整体架构

```
┌─────────────────────────────────────────────────────────┐
│  Tauri 桌面 App (desktop/)                               │
│  ┌────────────────────┐      ┌────────────────────────┐  │
│  │ React 前端 (WebView)│ IPC  │ Rust 后端 (core)        │  │
│  │ - 仓库浏览/搜索      │◄────►│ - registry              │  │
│  │ - 安装弹窗/覆写编辑  │invoke│ - scanner               │  │
│  │ - 矩阵总览          │      │ - applier (+backup)     │  │
│  │ - 项目管理 / 扫描导入│      │ - differ                │  │
│  └────────────────────┘      │ - adapters (json/toml)  │  │
│                              └────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
            ▲                              ▲
            │ 都读同一份                    │
   ┌────────┴─────────┐          ┌─────────┴──────────┐
   │ 共享数据文件       │          │ 现有 npm 包 (CLI)   │
   │ registry.json     │          │ TS core 不变        │
   │ agents.json       │          │ arca mcp assign/status… │
   └───────────────────┘          └────────────────────┘
```

### 模块边界

- **新建 `desktop/` Tauri 项目**：前端 React + TS，后端 Rust。
- **Rust core**（`desktop/src-tauri/src/core/`）：等价重写现有 TS core。
  - `adapters`：`Adapter` trait + `JsonAdapter`（带 `key`）+ `TomlAdapter`，
    对应现有 `read/write/remove` 三方法。
  - `scanner`：扫描各 agent 的 global/project 配置文件，返回已存在的 MCP。
  - `differ`：对比「期望状态」与「实际扫描」，产出 add/remove/change 差异。
  - `applier`：按差异写入目标文件，写前自动备份到 `~/.arca/backups/`。
  - `registry`：读取内置 + 用户自定义服务器，合并（用户同名覆盖内置）。
- **共享数据文件**（抽离自现有代码，TS 与 Rust 共读）：
  - `registry.json` ← 抽离自 `src/builtin-registry.ts`（40+ 内置服务器）。
  - `agents.json` ← 抽离自 `src/constants.ts` 的 `DEFAULT_AGENTS`（18 个 agent）。
  - 抽离后，现有 TS core 改为读取该 JSON（保持 CLI 行为不变）。
- **CLI 不动**：`@scoheart/arca` 的 TS core / TUI / 命令保持原样继续发布。
- **状态存储**：App 与 CLI 共用 `~/.arca/`（`registry/` 自定义条目、`state.json`
  安装状态、`backups/`、新增 `overrides.json`、`projects.json`）。

## 3. 数据模型

所有实体持久化在 `~/.arca/`，CLI 与 GUI 共用。

```
RegistryServer  规范化服务器定义（仓库的一条记录）
  name, description, tags[], builtin
  transport: "stdio" | "http"
  stdio?: { command, args[], env{} }
  http?:  { type:"http"|"sse", url, headers{} }

Agent           目标工具定义（18 个，可增删/启用）
  id, displayName, globalPath?, projectPath?
  format: "json" | "toml"
  key                      # mcpServers / servers / context_servers / mcp_servers / mcp
  supportsTransport[]      # 有些 agent 不支持 http（设计预留）

Project         桌面端特有：用户添加的项目文件夹
  id, name, path

Installation    安装记录（"谁装在哪"）—— 矩阵与状态的数据源
  serverName, agentId
  scope: "global" | "project"
  projectId?               # scope=project 时指向某个 Project
  status: applied | pending | drifted

Override        逐 (server, agent, scope, project) 的可选覆写
  serverName, agentId, scope, projectId?
  patch: { args?, env?, url?, headers? }   # 只存与 canonical 的差异
```

### 设计决策

- **覆写存"差异 patch"**，不存整份副本。落地时计算
  `effective = canonical ⊕ override.patch`；改动 canonical 默认值时，未被覆写
  的字段自动跟随更新。
- **drift 检测**：打开 App 或刷新时，scanner 比对各 agent 实际文件 vs 我们的
  `Installation` 记录。不一致标记 `drifted`，让用户选择「以 App 为准覆盖」或
  「导入实际值」。
- **密钥/令牌**（env、headers 中的 token）：v1 沿用现有做法——明文存于
  `~/.arca/` 并写入各 agent 配置（与 CLI 当前行为一致）。OS keychain 加密留待
  v2，数据模型预留接口。
- **桌面端 project 概念**：CLI 用 cwd 作 project；桌面 App 无 cwd 语境，故引入
  显式 `Project`（用户添加文件夹）。安装到 project scope 时需先选定一个
  Project。

## 4. 界面设计

### 主视图 · 仓库（App Store 式）

- 左侧导航：仓库 / 总览矩阵 / 项目 / 扫描导入 / Agents；下方分类筛选。
- 主区：搜索框 + 服务器卡片墙。每张卡显示名称、描述、已安装处数。
- 点卡片 → 详情侧栏 → 「安装」按钮打开安装弹窗；`+ 自定义` 新增本地服务器。

### 安装弹窗

- 选 Scope：全局 / 项目（项目需从下拉选一个已添加的 Project）。
- 多选目标 Agents（按 `supportsTransport` 过滤不兼容项）。
- 可选「逐 agent 覆写」：展开后对某个 agent 编辑 args/env/url，存为 `patch`。
- 「预览改动 & 应用」：先用 `differ` 展示将写入/删除的内容，确认后 `applier`
  执行（自动备份）。

### 总览矩阵（切换视图，B 方案）

- 行 = 服务器，列 = agent；顶部切换 scope（及 project）。
- 单元格：`●`已装 / `○`未装 / `◐`有覆写 / `⚠`drift。点格直接开关，双击编辑覆写。
- 用于批量跨 agent 管理与「谁装在哪」的全局视角。

### 其它界面

- **项目**：列出/添加/移除项目文件夹。
- **扫描导入**：扫描现有 agent 配置，列出发现的服务器，选择导入到仓库 +
  登记为 Installation（对应现有 `import` 命令的可视化）。
- **Agents**：启用/禁用 agent，查看/编辑其路径、格式、key。

## 5. 错误处理

- **写入失败/权限不足**：applier 单条目标失败不影响其它目标；汇总报告失败项。
- **备份**：任何写入前对目标文件备份到 `~/.arca/backups/`，文件名带时间戳。
- **drift 冲突**：检测到外部改动时不静默覆盖，弹出二选一（覆盖 / 导入）。
- **格式解析失败**：目标文件已损坏无法解析时，跳过并提示用户手动检查，不破坏
  原文件。
- **无效路径**：agent 的 global/project 为 null 时该目标不可选（UI 置灰）。

## 6. 测试策略

- **Rust core 单元测试**：adapters（json/toml 读写删幂等性）、differ（各
  add/remove/change 场景）、applier（备份生成、覆写合并 `canonical ⊕ patch`）、
  scanner（drift 判定）。以现有 `tests/` 的 TS 用例为对照基线，保证 Rust 行为
  与 TS core 等价。
- **共享 JSON 一致性测试**：校验 `registry.json` / `agents.json` 与原 TS 常量
  内容一致（抽离不丢数据）。
- **前端**：安装弹窗 → 预览 → 应用的关键交互做组件/集成测试。
- **端到端冒烟**：在临时目录构造假 agent 配置，走「装→矩阵显示→卸→drift」全
  流程。

## 7. v1 范围与分期

**v1（本设计实现目标）**：

1. 抽离 `registry.json` / `agents.json`，TS core 改读共享 JSON（不破坏 CLI）。
2. Tauri 项目骨架 + Rust core（adapters/scanner/differ/applier/registry）。
3. 仓库主视图 + 搜索 + 服务器详情 + 自定义服务器增删改。
4. 安装弹窗（scope × 多 agent + 逐 agent 覆写）+ 预览 + 应用 + 备份。
5. 总览矩阵视图（开关 / 覆写标记 / drift 标记）。
6. 项目管理、扫描导入、Agents 管理。

**v2（预留，不在本次实现）**：OS keychain 加密密钥；服务器仓库的云端/Git 共享；
团队同步。

## 8. 未决/默认事项

- 桌面 App 与 CLI 共用 `~/.arca/`：若 schema 需要演进，以向后兼容方式增量添加
  字段（`overrides.json`、`projects.json` 为新增文件，不影响 CLI 既有读取）。
- 打包分发（macOS/Windows/Linux 三平台、签名公证）在实现计划阶段细化。
