# Arca 中央资产与 Agent 消费模型设计

## 状态

- 日期：2026-07-18
- 状态：已实现，等待完整回归与用户验收
- 范围：Rust core、Tauri commands、macOS Desktop
- 不包含：CLI/TUI 交互迁移、项目级配置、稳定版发布

## 背景与问题证据

Arca 已经把 MCPs、Models、Skills 放在并列的顶层资源视图中，但三类资源的产品逻辑并未真正统一。

- MCP Registry 基本属于中央资产，Agent 页面会从 Registry 中选择 MCP 并写入 Agent 配置。
- Model Profile 保存在 `~/.arca/settings.json`，Agent 会选择一个兼容 Profile；但编辑 Profile 时现有实现会清除 assignment，而不是将变更传播给消费者。
- Skills core 已经分别保存 `managed_skills` 中央副本和 `skill_assignments`，但 UI 将“中央入库”和“分配给 Agent”耦合在同一个安装向导中。
- Agent 页的“添加 Skill”会跳转到来源解析、候选选择和安装流程，而不是先列出中央已有 Skills，因此用户看起来必须为每个 Agent 重新添加同一资产。
- MCP 当前主要从 Agent 文件和 disabled snapshot 反推使用状态，没有明确的 desired relationship；扫描结果、中央资产和期望状态的权威边界不够清楚。
- MCP 当前的 `refreshAll` 会调用 `importDiscovered`，将新扫描内容直接写入 managed discovered source；这与“外部资产必须显式导入”的目标规则冲突。

问题的根因不是单个按钮或文案，而是 UI 和部分 core 数据流仍把 Agent 配置文件当作资源管理入口。目标产品模型应当是：

```text
中央资产入库与维护 → Agent 选择消费 → Arca 对账实际状态
```

## 已确认的产品决策

| 决策点 | 结论 |
| --- | --- |
| 入库与消费 | 严格拆成两个阶段；顶层只管理中央资产，Agent 页面不安装来源 |
| 外部资产 | 只读发现；用户明确导入后才由中央资产库管理 |
| 中央更新 | 一次操作传播到全部消费者；遇到漂移或冲突时暂停并审阅 |
| 关系入口 | Agent 页面和中央资产详情都可管理，但修改同一份关系 |
| MCP / Skills 基数 | 一个 Agent 可消费多个 |
| Model 基数 | 一个 Agent 同时只消费一个当前 Profile |
| 中央删除 | 审阅完整影响后，原子解除全部关系、清理 Agent 配置并删除资产 |
| 外部漂移 | 中央关系仍是 desired state；扫描只报告，不后台覆盖或反向写回 |
| Agent UI | 只显示正在使用的资产，通过中央选择器管理 |
| 推荐架构 | 统一消费契约和事务，不强行统一三类资产的数据模型 |

## 目标

1. 让 MCP、Model、Skill 都成为先在 Arca 中统一配置的中央资产。
2. 让 Agent 只消费中央资产，不在 Agent 上下文中重新创建或安装来源。
3. 为三类资源建立相同的 desired、observed 和 reconciled 状态语义。
4. 让中央资产编辑、更新和删除对所有消费者产生可审阅、可恢复的影响计划。
5. 保留 MCP codec、Model Keychain、Skill target/link 和风险审阅等领域安全边界。
6. 在升级过程中不自动接管、删除或覆盖现有 Agent 配置。

## 非目标

- 不建立一个用任意 JSON payload 表示所有领域的万能 Asset 表。
- 不让 React 复制兼容性、漂移判断、写入计划或恢复逻辑。
- 不把 Agent 实际配置自动反向同步为中央 desired state。
- 不重新引入项目级配置写入。
- 不让中央资产页或 Agent 页显示 Keychain 明文、token、secret 或未脱敏配置。
- 不在本设计中增加市场、搜索服务、私有仓库支持或云同步。
- 不在本设计中发布 Stable Release 或替换 `/Applications/Arca.app`。

## 术语与权威边界

### 中央资产

中央资产定义“资产是什么”，只能从顶层 MCPs、Models、Skills 工作区创建、导入、编辑、更新和删除。

- MCP Asset：`name + transport + config + source`。
- Model Profile：protocol、model、Base URL、token limits、reasoning 和 Keychain credential 引用。
- Managed Skill：中央内容、source、resolved revision、content hash、risk 和 update state。

### 消费关系

消费关系定义“哪个 Agent 应该使用哪个中央资产”。它是 Arca 持久化的 desired state，不由文件扫描临时推断。

- MCP：每个 Agent 为 `0..N`，允许领域专属的 enabled state 和 override。
- Model：本历史设计当时限定为 `0..1`；当前实现已由多模型语义取代，支持的 Agent 为 `0..N` assigned、最多一个 current，单模型 Agent 仍由兼容性门禁限制为一个。
- Skill：每个 Agent 为 `0..N`；写入时仍归一化为真实物理 target。

### 实际状态

Agent 配置文件、disabled snapshot、Skill link 和目标目录只提供 observed state 证据。扫描不得自动创建中央资产或消费关系。

### 对账状态

统一对外状态为：

- `synced`：desired 与 observed 一致；
- `pending`：desired 已选择但尚未提交或需要重新同步；
- `drifted`：目标仍可识别，但内容被外部修改；
- `conflicted`：目标被不兼容内容占用或状态歧义；
- `unsupported`：Agent capability 不支持该资产；
- `external`：仅在 Agent 中发现，尚未由中央资产库管理。

状态原因必须包含可操作的领域证据；React 不从字符串或文件差异自行推导状态。

## 架构

```text
┌─────────────────────────────────────────────────────────┐
│ Central Assets                                          │
│ MCP Assets        Model Profiles        Managed Skills  │
└──────────────────────────┬──────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────┐
│ core::consumption                                       │
│ desired relationships + observed inventory + reconcile  │
│ compatibility + plan + transaction + migration          │
└───────────────┬────────────────┬────────────────┬────────┘
                ▼                ▼                ▼
        MCP adapter       Model adapter      Skill adapter
        codecs/write      protocol/keychain  targets/links/risk
                └────────────────┬────────────────┘
                                 ▼
                       Agent user-level config
```

Rust core 是资产、关系、兼容性、计划、写入和恢复的唯一权威。Tauri command 只转换 tagged request/response，React 只展示 inventory、收集用户意图并提交 plan confirmation。

统一的是生命周期和消费契约，不是领域内容。MCP transport、Model protocol、Skill physical target 不进入共享业务枚举。

## 持久化模型

### 保留的中央数据

- MCP 继续以 source 聚合后的 effective Registry 作为中央资产集合。
- Model 继续使用 `model_profiles`，凭据只存在系统 Keychain。
- Skill 继续使用 `managed_skills` 和 `~/.arca/skills/<name>/` 中央副本。

### 消费关系

保留并规范现有：

- `model_assignments: AgentId -> ProfileId`
- `skill_assignments: SkillName -> PhysicalTargetIds`

新增明确类型的 MCP desired records，而不是继续只从 Agent 文件反推：

```rust
pub struct McpConsumptionRecord {
    pub asset_key: String,      // stable name::transport key
    pub enabled: bool,
    pub overrides: OverridePatch,
}

pub struct Settings {
    // existing fields omitted
    pub mcp_consumptions:
        Option<BTreeMap<AgentId, BTreeMap<String, McpConsumptionRecord>>>,
}
```

`OverridePatch` 只能包含 MCP adapter 已拥有的字段，不得吸收 Agent 文件中的未知内容。disabled snapshot 仍是 MCP 恢复机制，但不再替代 desired relationship。

共享服务通过 Rust tagged enum 投影消费状态：

```rust
pub enum AssetRef {
    Mcp { key: String },
    Model { profile_id: String },
    Skill { name: String },
}

pub struct ConsumptionView {
    pub agent_id: String,
    pub asset: AssetRef,
    pub desired: bool,
    pub observed: bool,
    pub status: ConsumptionStatus,
    pub reason: Option<String>,
    pub affected_agent_ids: Vec<String>,
}
```

这是读取投影，不要求三类关系在 JSON 中使用相同存储形态。settings 继续透传未知字段，并通过 strict read-modify-write 和原子替换更新。

## 生命周期与数据流

### 中央入库

1. 用户在顶层资产工作区创建、选择来源或发起导入。
2. 领域 adapter 验证 schema、路径、risk、credential boundary 和身份冲突。
3. core 生成只涉及中央资产的计划。
4. 用户确认后保存中央资产。
5. 不选择 Agent、不写 Agent 配置、不创建消费关系。

Skills 安装向导因此移除 Agent 选择；`PlanInstallRequest` 的中央入库路径不再接收 `agent_ids`。从 Agent 发起的 Skills navigation 不再打开安装向导。

### 建立或变更消费

1. 用户从 Agent 页打开中央选择器，或从资产 Inspector 打开消费者管理。
2. core 根据 Agent capability 计算兼容资产和不兼容原因。
3. 用户提交目标选择；MCP/Skills 是集合，Model 是单值。
4. planner 读取 settings 和全部目标快照，生成关系变化、Agent 文件变化、备份和真实共享影响。
5. review 确认后，transaction 同时写入 Agent 和关系。
6. commit 后重新扫描；只有验证一致才返回 `synced`。

两个入口不得调用两套写入 API；它们只产生同一种 desired selection request。

### 中央更新传播

1. 编辑先形成未落盘 draft。
2. planner 枚举全部消费者并读取 observed state。
3. 计划同时包含中央差异和每个消费者差异。
4. 所有目标干净时，一次确认后原子更新中央资产和消费者。
5. 任一目标漂移、冲突或并发变化时，不做部分写入。
6. 用户只能取消、审阅并明确覆盖允许覆盖的目标，或先解除冲突 Agent 的消费后重新计划。

“自动传播”指一次中央更新操作覆盖全部消费者，不代表后台无确认写入。

### 外部资产导入

1. 扫描将 Agent 独有内容标记为 `external`。
2. 外部内容只读展示，并提供“导入资产库”。
3. 导入复制、验证并保存中央资产，但不建立消费关系。
4. 原 Agent 是否改为消费中央资产，必须通过下一次独立的“管理资源”操作确认。

### 中央删除

1. planner 查出全部 desired consumers 和 observed targets。
2. review 展示将解除的关系、修改的 Agent 文件、Skill 共享影响、备份和不可用目标。
3. unresolved drift/conflict 阻止 commit。
4. commit 原子清理 Agent 配置、关系和中央资产。
5. 不允许删除中央记录后留下隐式、未声明的 Agent 副本。

## UI 信息架构

### 顶层中央资产库

顶层顺序保持 `MCPs -> Models -> Skills`。每个工作区只负责中央资产生命周期，并以单独状态展示扫描到但尚未托管的外部项。

- MCPs：来源、创建、导入、编辑、更新和删除。
- Models：Profile、协议、凭据 presence、编辑和删除。
- Skills：来源、中央副本、更新、风险、恢复和删除。

卡片和 Inspector 的 impact 区域显示正在使用该资产的 Agent。Inspector 提供“管理 Agent”，可批量修改同一份消费关系。

Skills 顶层主操作使用“添加到资产库”；来源 URL、本地目录选择和候选安装只在这里出现。

### Agent 页面

Agent 页面保留 MCPs、Model、Skills Tabs。主列表只展示当前 Agent 已建立 desired relationship 的中央资产；即使 observed target 缺失或冲突，该项仍留在列表并显示异常状态，不能因为当前未生效而消失。

扫描到的外部项不混入“正在使用”主列表。Tab 显示紧凑的“发现 N 个外部资源”提示，展开后只读展示来源和实际位置，并提供“导入资产库”入口；导入完成后仍需单独建立消费关系。

- 标题：`正在使用`；
- 操作：`管理 MCPs`、`切换 Model`、`管理 Skills`；
- 状态：`已同步`、`待同步`、`有漂移`、`有冲突`；
- 解除动作：`解除使用`，在需要时进入 review；
- 详情动作：跳转对应中央资产 Inspector。

管理 Dialog 使用已确认的“已使用列表 + 中央选择器”布局：

- 只列兼容中央资产；
- 支持搜索、状态和来源信息；
- 不兼容项提供数量和原因入口，不无声隐藏；
- 若中央资产不存在，提供跳转资产库的链接；
- 不嵌套创建表单、GitHub 来源、本地目录选择或 Skill 安装向导。

架构和代码使用 `consumption`，用户界面使用“资产库、正在使用、管理资源、解除使用”，不把内部术语暴露为主要文案。

## 领域适配规则

### MCP

- identity 是 `name::transport`，同名不同 transport 是不同资产。
- compatibility 由 Agent supported transports 决定。
- desired relationship 显式保存 enabled state 和 Agent-specific override。
- observed scan 区分 active、Arca disabled snapshot、exact central match、customized drift 和 external。
- `refreshAll` 不再调用会写 Registry 的 `importDiscovered`；新扫描结果进入只读 external inventory，只有显式导入才写中央资产。
- 旧版本已经持久化到 managed discovered source 的 MCP 继续作为中央资产保留，避免升级时丢失用户数据；本版本之后的新发现不再自动入库。
- 中央 MCP 编辑替代现有独立的自动 resync 路径，统一经过影响计划。
- 未知字段、注释、非目标策略和手工定制必须继续保留或触发 fail-closed。

### Model

- 一个受管 Agent 最多有一个 desired Profile。
- compatibility 由 protocol capability 决定。
- observed reader 只比较该 adapter 拥有的 Model 字段与中央 Profile；匹配时为 synced，差异为 drifted，无法对应中央 Profile 的配置为 external。
- Guided/read-only Agent 不建立虚假的 managed relationship，只显示官方配置建议。
- 编辑 Profile 不再清除引用它的 `model_assignments`；改为生成传播计划并更新全部消费者。
- Keychain 明文不进入 settings、plan、journal、日志、fixture 或 DOM。

### Skill

- 中央内容仍只有一份，Agent target 中只保存受管 link。
- Agent 选择由 planner 归一化为最少的物理 targets。
- 一个 target 同时被多个 Agent 读取时，计划和 UI 必须显示真实共享影响。
- 不能对同一物理 target 创建互相矛盾的 Agent 选择。
- install/import、assignment、update、remove、repair 的风险、候选 hash、operation id 和 recovery 约束继续有效。

## MCP 升级迁移

Model 和 Skill 已有显式 assignment，可直接进入新读取投影。MCP 不具备可靠的所有权证据，迁移不得自动认领。

首次运行新模型时：

1. 只读扫描全部用户级 Agent MCP 配置和 disabled snapshots。
2. 与 effective central MCP 完全一致的 observed entry 标记为 `adoptable`。
3. 有 Agent-specific 内容差异的 entry 标记为 `drifted` 或 `external`，并展示差异原因。
4. UI 提供一次迁移审阅，用户确认后才创建 `mcp_consumptions`。
5. 建立关系时不重写完全一致的 Agent 文件；只记录 desired state 并重新验证。
6. 未选择的 observed entry 保持外部可见，不删除、不覆盖、不隐藏。

迁移失败不得修改 settings version 或已存在的 Agent 配置。迁移不是启动阻塞器；用户可稍后处理，但未接管项不显示为中央“正在使用”。

## Core 模块与 API 边界

新增：

```text
core/src/consumption.rs
core/src/consumption/
├── inventory.rs
├── compatibility.rs
├── planner.rs
├── transaction.rs
├── migration.rs
└── types.rs
```

- `inventory`：读取中央资产、desired records 和 observed targets，生成投影。
- `compatibility`：调用 Agent catalog 和 domain capability，返回兼容结果与原因。
- `planner`：接受 tagged intent，委托领域 adapter 产生 typed mutations。
- `transaction`：snapshot、hash、backup、journal、commit、rollback 和 post-verify。
- `migration`：只生成 MCP adoption candidates 和确认计划。

建议的 Tauri command 表面：

```text
list_consumption_inventory
plan_set_agent_consumption
plan_set_asset_consumers
plan_update_central_asset
plan_delete_central_asset
plan_adopt_mcp_consumptions
commit_asset_operation
cancel_asset_operation
recover_asset_operation
```

`plan_set_agent_consumption` 与 `plan_set_asset_consumers` 只是在不同方向表达同一 desired diff；planner 必须把它们归一化为相同关系变化。

现有领域 commands 在过渡期可委托新服务，但 Desktop 完成迁移后不得继续绕过统一事务直接写 Agent。

## React 组件边界

新增共享层：

- `useConsumptionState`：app-level 关系 inventory、pending operation 和 refresh；
- `AgentConsumptionPanel`：Agent 当前关系的统一 shell；
- `ConsumptionPickerDialog`：中央资产搜索、兼容性和目标 selection；
- `AssetConsumerDialog`：从资产方向管理 Agent；
- `AssetOperationReviewDialog`：统一 plan、diff、warnings、targets 和 confirmation；
- `ConsumptionStatus`：只渲染 core 返回的状态和原因。

领域视图保留：

- `RegistryView` 提供 MCP identity、source 和配置展示；
- `ModelsView` 提供 Profile、protocol 和 credential presence；
- `SkillsView` 提供 source、revision、risk、update 和 recovery；
- `AgentView` 只做 selected Agent 与三类 domain adapter 的组合。

必须删除或替换的错误路径：

- `AgentSkillsSection` 不再产生 `kind: "install"` navigation；
- `SkillsView` 不再因 Agent navigation 打开 `SkillInstallDialog`；
- `SkillInstallDialog` 不再维护 `selectedAgentIds`；
- Agent MCP、Model、Skill 不再分别维护三套关系写入 UI；
- Model Profile 保存不再隐式清除 assignment。

## 事务、并发与恢复

所有会影响消费关系的操作遵循：

```text
read snapshots
→ validate compatibility
→ calculate desired / observed diff
→ persist immutable plan
→ user review
→ verify settings and target hashes unchanged
→ create backups
→ commit files and settings
→ rescan and post-verify
```

事务不变量：

- 不允许关系已保存但 Agent 配置未生效；
- 不允许中央资产已更新但只有部分消费者更新；
- 不允许中央资产已删除但消费者仍被 Arca 声称为 managed；
- 不允许 cancel 后残留 staging 或可提交旧 plan；
- 任一并发 hash 变化使原 plan 失效，必须重新计划。

失败处理：

- 写入前失败：取消并清理 staging；
- 部分写入失败：按 journal 回滚已写 targets；
- 回滚失败：进入 `recovery_required`，相关资产写操作变为只读；
- 恢复可继续到同一目标状态，或回滚整个 operation；
- Keychain 不持久化秘密到 journal；若崩溃恢复需要凭据，要求重新输入并重新计划。

统一错误码至少包括：

- `unsupported`
- `external_unmanaged`
- `drifted`
- `conflicted`
- `concurrent_change`
- `credential_required`
- `recovery_required`

错误必须附带用户可执行的下一步，不能只返回底层文件或 serde 文本。

## 实施分段

1. 建立 consumption types、inventory、compatibility 和 MCP adoption migration，不改变写路径。
2. 将 Skills 中央安装与 assignment 拆开，先修正最明显的 Agent 重装逻辑。
3. 引入共享 Agent consumption UI 和双向消费者 Dialog。
4. 将 MCP 与 Model 写入迁到统一 plan/transaction，并修正 Profile 更新传播。
5. 接入中央更新、级联删除、漂移修复和 recovery UI。
6. 删除 Desktop 绕过统一契约的旧写入路径，完成正式安装版验收。

每段都必须保持可测试、可回滚，并在合并时不留下同时可用的新旧写入口。

## 测试与验收

### Core

- 中央创建或导入不修改任何 Agent target。
- MCP/Skills 多选、Model 单选基数正确。
- compatibility 对 transport、protocol、guided Agent 和 Skill target 正确分类。
- desired/observed 对账覆盖 synced、pending、drifted、conflicted、unsupported 和 external。
- 中央更新对所有消费者生成完整计划并保持关系。
- 任一消费者漂移阻止整个更新，不产生部分写入。
- 中央删除级联清理全部关系和目标。
- Skill target 归一化展示共享影响并拒绝矛盾选择。
- MCP migration 只生成 adoption candidates，未确认不写 settings 或 Agent。
- settings round-trip 保留未知字段和 CLI-owned sections。
- 在备份前、每个 target 写入后、settings 写入前后和 post-verify 注入失败，验证回滚或 recovery。

### Desktop

- Agent Skills 页面没有来源 URL、本地目录选择或“安装 Skill”。
- Agent 三个 Tab 只显示已消费中央资产。
- 管理 Dialog 只提供兼容中央资产，并能解释不兼容项。
- 从 Agent 和资产两端修改关系得到相同 inventory 结果。
- 外部资产导入后不会自动建立消费关系。
- review 显示消费者、文件、共享影响、备份、漂移和冲突。
- pending operation 期间禁止重复提交和关闭不可逆 commit。
- recovery state 禁止新的相关写操作并提供恢复入口。

### 端到端场景

对 MCP、Model、Skill 分别验证：

1. 添加中央资产；
2. 确认 Agent 未变化；
3. 从 Agent Picker 建立消费；
4. 从资产 Inspector 增减消费者；
5. 编辑中央资产并传播；
6. 制造外部漂移并确认更新被阻止；
7. 审阅后修复；
8. 删除中央资产并级联清理。

正式安装版在 `1200x820` 与 `900x600`、浅色和深色模式下验收，无横向滚动、Dialog clipping、焦点丢失或敏感值进入 DOM、截图和日志。

## 完成标准

- 用户只需在顶层配置一次 MCP、Model 或 Skill。
- Agent 页面不会要求重新解析、安装或创建同一资产。
- 所有 Agent 关系都有持久化 desired state 和可解释 observed state。
- 中央更新、删除和关系变更都通过同一 plan/review/commit 契约。
- 三类领域继续保留各自类型安全和安全写入行为。
- 升级不会自动接管或破坏已有 Agent 配置。
- 所有聚焦测试、Desktop build、Tauri tests、Rust workspace tests 与正式安装版 UI 验收通过。
