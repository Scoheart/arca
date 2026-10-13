# Agent 配置位置与 Skills 筛选精简设计

日期：2026-07-17
状态：用户已批准

## 1. 目标

本轮调整解决两个相邻的信息架构问题：

1. Skills 工作区删除 Arca 自行推断、且对外部或断链 Skill 不可靠的“内容类型”导航。
2. Agent 页把含糊的两块“Agent 配置文件 / MCP 配置文件”重构为 Model、MCP、Skills 三个同级配置位置。

结果应让用户看到真实、可核验的路径语义，同时保持 v1.2.16 已发布的 Skills 持久化与恢复数据可兼容。

## 2. 当前问题

### 2.1 “内容类型”不是可靠的用户分类

当前 Arca 根据 Skill 文件树推断 `automation`、`assets`、`reference` 或 `instructions`。外部列表扫描为避免遍历完整目录，会先把类型统一兜底为 `instructions`；断链条目无法在详情加载时重新分类。因此侧栏计数会把未知内容显示成“说明型”，形成错误暗示。

该字段同时已经进入 v1.2.16 的 managed record、source resolution、operation plan、settings snapshot 和 recovery journal。直接删除 wire/persistence 字段会使旧计划、旧恢复日志和版本降级不兼容。

### 2.2 “Agent 配置文件”是错误的前端别名

当前 Agent 页没有独立的 Agent 配置 capability。界面使用：

```text
agentConfigPath = modelAgent.config_path ?? agent.global
```

有 Model capability 时它实际表示 Model 配置；没有时则复制 MCP 路径。这会造成重复卡片，也掩盖了 Model、MCP 和 Skills 使用三套独立能力声明的事实。

Skills inventory 只包含安装探针命中的 Agent，不能单独用来判断某个 catalog Agent 是否具有已核验的 Skills 目录。已核验能力与本机安装状态必须分开表达。

## 3. 范围

### 3.1 本轮包含

- 删除 Skills 页面“内容类型”侧栏、状态和过滤谓词。
- 保留来源、状态、搜索及现有生命周期操作。
- 将 Agent 顶部区域改名为“配置位置”，固定呈现 Model、MCP、Skills 三块。
- 为 Agent 的只读 UI 投影增加可选 `skills_global_dir`，来源为受信任的 `AgentDefinition.skills.global_dir`。
- 使用 Skills inventory 补充本机检测、可分配和共享影响状态，但不从 inventory 反推 capability。
- 覆盖桌面、`900×600` 和防御性窄屏布局。

### 3.2 本轮不包含

- 不删除 Rust/IPC/persistence 中的 `SkillContentKind` 或 `content_kind`。
- 不迁移或重写 `~/.arca/settings.json`、staging、plan 或 journal。
- 不改变 Skills 安装探针、分配目标或共享目录语义。
- 不新增 Model 或 Skills 编辑入口；各自仍由下方现有管理区负责。
- 不修正 Qoder Desktop 与 Qoder CLI 的既有产品映射；本轮只用现有 capability。

## 4. Skills 工作区

左侧只保留“来源”：

- 全部来源
- GitHub
- 本地

顶部状态 tabs 继续提供“全部 / 有更新 / 需处理 / 外部”，搜索继续匹配名称和描述。`filterSkills` 只组合状态、来源和搜索，不再接收 `contentKind`。

后端 `content_kind` 作为兼容字段继续生成、序列化和校验，但 Desktop 不展示、不计数、不筛选。将来若要彻底移除，必须单独设计带版本的 settings、plan、resolution 和 journal 迁移。

## 5. Agent 配置位置

### 5.1 数据来源

三块必须使用彼此独立的权威来源：

| 块 | 路径来源 | 不可用状态 |
|---|---|---|
| Model | `ModelAgentView.config_path` | `尚未接入 Models` |
| MCP | `AgentInfo.global` | 只读参考页不渲染该区域 |
| Skills | `AgentInfo.skills_global_dir` | `尚未核验 Skills 目录` |

`AgentInfo.skills_global_dir` 是 catalog capability 的只读投影，不改变自定义 Agent 的安全边界；自定义 Agent 继续没有受信任的 Skills capability。Skills inventory 中同 ID 的 `SkillAgentView` 只用于补充“已检测 / 可分配”和共享影响信息。

Model 缺失时不得回退到 MCP 路径。Skills 缺失时不得从 MCP 路径、alias 或文件系统猜测目录。

### 5.2 视觉结构

区域标题由“配置文件”改为“配置位置”，说明文字为“Model、MCP 与 Skills 使用的用户级配置入口。”

```text
配置位置                         [Model / MCP 共用]

┌ Model            │ MCP              │ Skills              ┐
│ 模型配置文件     │ MCP 配置文件     │ 用户级目录          │
│ ~/.codex/...     │ ~/.codex/...  ✎  │ ~/.agents/skills    │
└──────────────────┴──────────────────┴─────────────────────┘
```

- 三块位于同一个扁平边框容器中，不做卡片套卡片。
- Model 使用 `LayersIcon`，MCP 使用 `PackageIcon`，Skills 使用 `SparklesIcon`。
- MCP 保留现有路径编辑按钮；Model 与 Skills 不在摘要块中新增操作。
- 整块不可点击，避免把信息摘要伪装成导航。
- Model 路径存在时才比较 Model/MCP：相同显示“Model / MCP 共用”，不同显示“Model / MCP 分离”；Model 不可用时不显示关系 badge。
- `guided` Model 继续显示真实路径，并用次要文案标明“官方引导”。
- Pi 的多文件路径按后端现有 `config_path` 原样显示，不伪装成单文件。

### 5.3 响应式

- 常规桌面和 `900×600`：三列等宽；长路径允许换行，不产生水平滚动。
- `≤820px`：三块改为单列，分隔线由左边框改为上边框。
- 路径使用等宽字体并保留完整 `title`；不可用状态使用可读文字而不是空字符串或错误路径。
- 编辑按钮保留键盘焦点样式和准确的 accessible name。

## 6. 页面其余结构

配置位置只是路径总览。下方现有管理区继续按以下顺序呈现：

1. Model
2. Skills
3. MCP

每个管理区继续负责自己的状态与操作；本轮不把操作复制到顶部摘要。

`has_global = false` 的参考页保持现状，不渲染三块配置位置，也不暴露未经核验的可写目标。

## 7. 错误与加载状态

- Model 列表尚在加载时，Model 块显示稳定的加载文案，不回退到 MCP。
- Skills inventory 尚在加载或失败时，只影响运行时状态说明；已核验的 `skills_global_dir` 仍可显示。
- capability 缺失与加载失败必须使用不同文案，避免把“尚未接入”误报为请求失败。
- MCP 路径编辑继续走现有 `AddAgentDialog`，本轮不改变写入行为。

## 8. 测试策略

实现遵循测试先行。

### 8.1 Desktop

- Skills 工作区不再渲染“内容类型”，仍可组合状态、来源和搜索。
- `filterSkills` 的公开输入不再包含 `contentKind`。
- Agent 页按 Model、MCP、Skills 顺序渲染三块。
- Codex 显示 Model/MCP 共用；独立路径显示分离。
- 无 Model capability 时显示不可用状态，且不会复制 MCP 路径。
- 已核验但本机未检测到的 Skills capability 仍显示 catalog 路径。
- 无 Skills capability 的 Agent 显示明确不可用状态。
- CSS 断言覆盖三列、长路径换行和 `≤820px` 单列。

### 8.2 Core

- `AgentInfo` 正确投影 builtin catalog 的首选 `skills_global_dir`。
- 无 Skills capability 和自定义 Agent 返回 `None`。
- 不改变 Skills inventory 只列出安装探针命中 Agent 的既有契约。

### 8.3 验收

- 运行 Desktop 单元测试、Rust 相关测试、类型检查和生产构建。
- 使用正式安装的 `/Applications/Arca.app` 验收，不使用 Preview bundle 或浏览器 mock。
- 截图检查 Skills 页已移除分类，以及 Codex Agent 页三块配置位置在常规窗口和 `900×600` 下无横向溢出。

## 9. 兼容与交付

- 本轮新增的 `AgentInfo.skills_global_dir` 只影响运行时返回值，不写入用户配置。
- `content_kind` 的持久化 schema 保持不变，因此升级、未完成事务恢复和降级不受本轮 UI 精简影响。
- 现有 Skills 管理设计中关于“内容类型”侧栏的条款由本文取代；历史 implementation plan 保留为实施记录，不回写历史步骤。
- 完成实现与真实 App 验收后再决定版本号和发布，不在本设计中隐式发布。
