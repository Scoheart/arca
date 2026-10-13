# Arca Target-scoped 收敛、故障隔离与恢复设计

## 状态

- 日期：2026-08-04
- 状态：已实现
- 优先级：P0
- 替代：跨 Agent 全有或全无的 Asset transaction、任意 rollback manifest 触发全局 `ReadOnly`

## 决策

Arca 对 MCP、Model、Skill 采用中央 Desired State 与 Agent Observed State 分离的最终一致模型。

一次用户操作可以修改一个中央资产并影响多个 Agent，但只有中央写入属于中央提交；每个物理 Agent target 都是独立的收敛单元。四个 target 成功、一个 target 失败时，四个成功结果必须保留，中央 Desired State 必须保留，失败 target 进入局部 incident 并可单独重试。

```text
central desired commit
        │
        ├── target A converge ── synced
        ├── target B converge ── synced
        ├── target C converge ── incident → retry/adopt/detach
        └── target D converge ── synced
```

## 最小安全写入单元

故障域按以下键确定：

```text
capability + physical target identity
```

Agent 是展示和路由维度，不一定是物理隔离边界。多个 Agent 共用同一个 Skill 目录时，它们共享一个 Skill target incident；Qoder 的全部 MCP 共用一个 `mcp.json` 时，它们共享一个 `Qoder × MCP` incident。不同 target、不同 capability 永不因该 incident 被阻止。

每个操作必须公开 secret-free write set。当前 wire contract 由 `target_files`、`affected_agent_ids` 与 domain capability 共同表达，至少包含：

- capability；
- target identity 与展示路径；
- 受影响 Agent IDs；
- 中央或 Agent target 类型；
- operation ID。

只有 active write set 相交时才返回可重试 `mutation_busy`。未完成历史写入不是全局锁；它投影为 target incident。`target_files` 是物理写入边界，不是 UI 按资产复制告警的依据。

## 提交语义

1. 计划绑定当前 settings、中央资产版本和每个 target observation revision。
2. 提交中央 Desired State；中央提交失败时不开始新的 target 收敛。
3. 按物理 target 去重并独立执行安全写入。
4. 每个 target 单独校验父目录身份、候选 hash、现场 CAS 和后置状态。
5. 成功 target 清除同 scope incident；失败 target 记录稳定诊断码并继续其他 target。
6. 返回新的 inventory，明确区分 desired、observed、convergence incident。

中央更新成功而 target 未同步不是事务失败。命令返回的新 inventory 通过 `target_incidents` 表达部分收敛，但不能回滚其他 target，也不能把部分收敛转换为全局 `recovery_required`。

## Incident 模型

```text
TargetIncident
  id
  operation_id
  capability
  target_id
  target_path
  affected_agent_ids[]
  code
  retryable
```

Incident 不保存凭据、配置内容、回滚字节或底层错误路径。原始恢复证据继续保存在私有 Arca 目录，只用于安全判断。

Incident 的动作与普通外部漂移一致：

- `restore-desired`：基于最新 observation 重新计划并覆盖 Arca-owned 字段；
- `adopt-observed`：采用当前现场并使旧 evidence 失效；
- `detach`：释放 desired ownership，保留现场。

所有动作都必须重新扫描。`observation_stale` 自动刷新并重新准备一次；再次变化才作为当前项目的局部可重试错误返回。

## 启动恢复

启动扫描必须遍历所有 operation，不得在第一笔失败时停止：

- 已有 `commit-complete`：验证并清理；
- 可以证明安全完成：完成并清理；
- 可以证明安全回滚：仅回滚该 target；
- target 已被外部修改：保留现场，形成 target incident，继续扫描；
- 无法读取中央 settings：进入全局 `ReadOnly`；
- 单个中央 MCP/Model/Skill 记录不可用：仅该 capability/target incident。

`rollback/manifest.json` 的存在本身不再是 Gate。应用 Gate 只处理：启动尚未完成、中央 settings 不可读、全局安全写 journal 无法恢复，以及短时跨进程互斥。

## UI 与 CLI

- 顶部全局恢复 banner 仅用于中央 settings/global write journal 故障。
- Target incident 在对应 Agent capability 区域显示一次，不按该文件中的资产数量复制。
- 普通 `external-added/changed/removed` 继续显示在资产卡片上，但不称为锁。
- 多 target 操作显示 `4/5 已收敛，1 个待处理`，失败 target 可直接重试。
- CLI inventory/status 输出结构化 incidents；退出失败只用于中央提交失败或用户请求的唯一 target 未收敛，多 target 部分成功使用稳定的 partial result。

## 删除的旧语义

- 任意 Asset rollback manifest → 全局 `ReadOnly`；
- Asset 错误字符串包含 `recovery_required` → 永久 latch 全局状态；
- settings 与全部 Agent 文件组成一笔跨 target rollback；
- 一个 target 失败后回滚其他已成功 target；
- 恢复扫描遇到第一笔失败即终止；
- UI 从 `ConsumptionInventory.recovery_error` 推导全局锁。

不提供旧 wire field、旧 Gate 或旧恢复路径的兼容分支。

## 验收矩阵

1. 5 个 Agent 同时消费一个资产，4 个成功、1 个失败：中央 Desired 和 4 个 target 保持成功。
2. `Qoder × MCP` incident 不影响 Qoder Model/Skill、其他 Agent MCP 或 UI 偏好。
3. 同一物理 target 上的多个资产只显示一个 incident。
4. 启动存在多个未完成 operation 时全部扫描，健康域可立即读写。
5. 外部修改后的旧回滚证据永不覆盖现场。
6. restore/adopt/detach 只处理选中 scope，并清除已解决 incident。
7. settings 损坏仍会全局 fail closed；单能力中央数据异常只隔离该能力。
8. CLI、Tauri、Desktop 使用同一 incident/write-set contract，不解析错误字符串决定权限。
9. 完整 Rust、CLI、Tauri、Desktop 和发布门禁通过。
