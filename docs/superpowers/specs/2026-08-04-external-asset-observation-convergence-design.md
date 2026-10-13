# Arca 三类资产外部变更观测与统一收敛设计

## 状态

- 日期：2026-08-04
- 状态：已实现
- 优先级：P0
- 范围：MCP、Model、Skill 的中央资产、Agent 消费关系、外部观测、迁移与恢复

## 结论

Agent 配置文件和用户级 Skill 目录是合法输入源。用户、Agent 或其他工具对它们的修改属于正常状态变化，不是 Arca 事务冲突，也不能成为整个产品的启动阻塞条件。

Arca 保存 `desired state`，实时扫描 `observed state`，并按单个 `Agent × asset` 投影差异。差异只在用户明确选择 `采用外部`、`恢复 Arca` 或 `解除管理` 时收敛。无法解析、身份歧义和不支持的内容只隔离对应观测；只有共享中央存储或未完成事务本身损坏时，才允许进入全局只读恢复。

## 目标语义

```text
central asset + desired relationship
                 │
                 ├── Agent 原生配置 / Skill target（外部可修改）
                 │                         │
                 └──────── scan ──────────┘
                              │
                     revisioned inventory
                              │
             adopt observed / restore desired / detach
                              │
                    plan → verify → commit
```

Arca 不在后台自动覆盖外部变更，也不把外部变更自动写回中央资产。观测与所有权是两条独立维度：一个被外部修改的关系仍然是 `managed`，直到用户显式解除或采用现场状态；一个外部新增项仍然是 `external`，直到采用成功。

## 统一状态机

| 状态 | 含义 | 是否异常 | 可用动作 |
|---|---|---:|---|
| `synced` | desired 与 observed 一致 | 否 | 无 |
| `external-added` | Agent 中存在、Arca 没有关系 | 否 | `adopt-observed` |
| `external-changed` | Arca 管理的字段、启停或 current 被外部修改 | 否 | `adopt-observed`（可安全时）、`restore-desired`、`detach` |
| `external-removed` | desired 仍存在，但 Agent 目标已删除 | 否 | `restore-desired`、`detach` |
| `unparseable` | 单个目标无法解析 | 局部异常 | `detach`（若已有关系） |
| `ambiguous` | 单个目标存在多重身份或互相冲突的值 | 局部异常 | `detach`（若已有关系） |
| `unsupported` | 现场状态无法无损表达或接管 | 局部限制 | `detach`（若已有关系） |

`available_actions` 由 core 投影，前端不得根据 `reason` 字符串自行推导写权限。`reason` 只提供稳定诊断码。

## 三个收敛动作

### 采用外部 `adopt-observed`

- MCP：将当前精确配置创建或更新为中央资产，保留来源 Agent、启停状态和原文件字节。
- Model：按准确 candidate identity 接管；同一 Agent 的多个外部模型分别投影，不折叠成“当前或第一个”。托管 Model 的 current 漂移可采用为新的 desired current。
- Skill：外部目录通过统一 Skill import 计划进入中央副本；托管中央 Skill 内容被修改时更新基线。
- 接管前后都重新扫描，candidate 已变化则返回 `observation_stale`。

### 恢复 Arca `restore-desired`

- 只重写选中的准确关系，不借 `assign`、`enable`、`use` 或中央资产编辑隐式修复其他漂移。
- 使用现有安全 writer、备份、同目录临时文件、原子替换和提交后验证。
- Skill 只恢复可证明安全的中央链接；外部目录、普通文件或异向链接不会被覆盖。

### 解除管理 `detach`

- 删除 desired relationship，不删除中央资产。
- 如果现场仍是准确的 Arca 内容，可执行正常清理。
- 如果现场已漂移、缺失或变成外部内容，只释放 Arca 所有权并保留现场字节/目录。
- 解除后仍存在的现场状态立即重新投影为 `external-added`。

## 观测与并发边界

- inventory 带内容哈希 `revision` 和展示时间 `observed_at`。
- MCP revision 包含准确配置指纹；Model revision 包含全部配置文件字节、文件类型和软链接目标；Skill 使用内容/目标 identity。
- `plan_converge_consumption` 必须携带用户看到的 revision。
- core 在生成计划后再次扫描；计划准备期间发生变化时取消已暂存计划。
- commit 继续校验 operation id、candidate hash、目标快照和事务后置条件。
- 已删除历史 `conflict_confirmation` 协议；明确的 convergence action 就是用户意图，第二套确认令牌只会制造分叉语义。

## 实时刷新

Desktop 使用文件系统 watcher 监听：

- `~/.arca/settings.json` 与中央 Skill 目录；
- 所有已配置 MCP 文件；
- 所有受管 Model 文件；
- 已核验用户级 Skill target。

目标尚不存在时监听最近的现存父目录；目录创建后自动把监听点移动到更精确的位置。Skill 目录使用递归监听。Agent capability 路径变化后重新计算完整 target 集合。事件在 Rust 和前端两侧去抖；窗口重新获得焦点或恢复可见时也会刷新。

任一不可访问路径只丢失该路径的实时事件，不会关闭其他 watcher。用户仍可通过刷新、重新聚焦或下一次 CLI 查询得到新状态。

## 故障隔离

| 故障位置 | 影响范围 |
|---|---|
| 单个 MCP/Model 文件解析失败 | 对应 observation 为 `unparseable`；其他 Agent、其他资产和其他域继续工作 |
| 单个 Skill target 异常 | 对应 target/关系异常；MCP 和 Model 继续工作 |
| 单个能力 schema 迁移失败 | 该能力写入只读；其他能力继续工作 |
| 单个 convergence 计划过期 | 只取消该 operation，刷新后重试 |
| 共享 settings 损坏、事务 journal 无法安全恢复 | 全局只读恢复；禁止猜测性部分写入 |

普通的 `model_active_state_drift`、`model_external_current`、文件删除和外部启停不属于 hard blocker。

`ConsumptionInventory.recovery_error` 只表示共享事务恢复边界；能力域读取失败进入
`capability_errors[]`，携带 `mcp` / `model` / `skill` 和稳定诊断码。Desktop 与 CLI
原位展示能力告警，但不得把它提升为全局恢复。即使 Skill inventory 整体不可读，MCP
与 Model 的 inventory、计划和提交仍然继续工作。

## Schema 迁移

Model v2/v3/v4 迁移只升级 `~/.arca/` 中央数据与安全凭据，不重写 Agent 现场配置。升级完成后，Agent 文件按当前字节重新扫描并投影为正常 observed state。旧 credential 只有在未迁移的 Agent 现场仍需要时才保留。

启动期不再维护“迁移审核对话框”或 `migration review/resolve` 命令。中央数据迁移失败按 Model 能力隔离；未完成共享事务才进入全局恢复。

## 前端与 CLI 契约

三个域共享：

```text
list · show · status · assign · unassign · enable · disable · converge
```

收敛命令统一为：

```bash
arca mcp converge <name::transport> --agent <id> <adopt|restore|detach>
arca model converge <profile-or-external-id> --agent <id> <adopt|restore|detach>
arca skill converge <name> --agent <id> <adopt|restore|detach>
```

`discover` 是只读的全域观察入口。旧 `adopt`、`reapply`、`migration review/resolve` 路由不再存在，也不接受旧 wire 字段。

Desktop Agent 页只消费 core inventory，不再从旧 MCP 安装列表和 Model assignment 拼装 fallback 状态。每个异常关系原位展示状态与 core 提供的动作；一个按钮失败只保留该项错误，不隐藏或锁住其他域。

## 验收条件

1. 外部新增、删除、字段修改、启停和 Model current 切换能在同一进程中刷新显示。
2. 三个域均通过同一个 revision-bound convergence 请求收敛。
3. 外部变更不会触发全局 migration hard block。
4. detach 不破坏外部现场；restore 不扩大到未选择的关系；adopt 不猜 candidate。
5. 单能力错误不阻止无关能力 mutation。
6. CLI、Desktop、Tauri 和 core 不再暴露旧迁移审核、reapply 路由或 conflict-confirmation wire 字段。
7. 完整 Rust、Tauri、Desktop 测试和构建通过。
