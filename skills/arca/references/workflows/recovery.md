# 状态、外部改动和同步恢复

Arca 将中央资产、期望关系和实际文件或目录分别记录。错误报告也按能力和物理目标区分。恢复前先确定这是单个资产、共享物理目标、整个能力，还是工作区公共事务。

## 取得当前范围的证据

```bash
arca agent mcp status --agent "$AGENT" --json
arca agent model status --agent "$AGENT" --json
arca agent skill status --agent "$AGENT" --json
arca status --agent codex --agent codex-desktop --capability skill --json
```

只需查看单域时执行对应一条。在 GUI 看准确 Agent 和资产详情，区分中央、实际副本、当前状态和可用动作。需要跨域诊断时查看 `arca workspace --json`。输出包含关系、现场及当前计划允许的动作。

| 字段 | 如何判断 |
| --- | --- |
| `ownership` | `managed` 表示受管关系，`external` 表示客户端现场 |
| `desired` / `observed` | 期望存在与实际观测到。缺失时看对应状态和原因 |
| `enabled` / `observed_enabled` | 期望启停与观察到的启停，具体字段按能力提供 |
| `desired_active` / `active` | 对 Model 区分期望 current 与现场 current |
| `observation_id` / `available_actions` | 这次现场观测和允许动作，重新扫描后取当前值 |
| `affected_agent_ids` / `target.target_id` | 共享影响以及物理目标，操作前核对全部实际消费者 |

## 状态字典

| `status` | 含义和下一步 |
| --- | --- |
| `synced` | 期望与观测一致。按用户目标核对启停、current 或实际加载 |
| `external-added` | 客户端新出现的外部内容。用户要求时检查显式采用或导入 |
| `external-changed` | 已管理内容或原生启停/current 漂移。先对比任务意图与当前可用动作 |
| `external-removed` | 期望关系保留，现场缺失。选择恢复还是让 Arca 采用现状 |
| `unparseable` | 当前文件或目标无法解析。保留原件，定位文件与格式并检查原生客户端 |
| `ambiguous` | 存在身份或值歧义。确认准确副本、路径和冲突原因，按该能力可用的方式消歧 |
| `unsupported` | 原生内容暂不能安全采用或表达。查目标能力、格式与官方引导 |

重新扫描只刷新现场与可用证据。外部漂移仍受保护，普通分配、启用和选当前不会自动覆盖现场。范围查询的 revision 标记该次读取，实际写入计划另行绑定完整 inventory、候选与快照。

## 按意图选择收敛动作

| 意图 | UI 或 CLI | 作用 |
| --- | --- | --- |
| 保留并接管现场 | **采用外部** / `adopt`，观测动作 `adopt-observed` | 依据当前现场更新中央或期望关系，保留现场原内容 |
| 按 Arca 期望恢复 | **恢复 Arca** / `restore`，观测动作 `restore-desired` | 只恢复指定可安全写入的关系或目标 |
| 交回客户端自行管理 | **解除管理** / `detach` | 去掉管理归属，保留现场内容并按外部观测展示 |

```bash
arca agent mcp converge docs::http --agent codex adopt --dry-run --json
arca agent model converge "$ASSET_ID" --agent "$AGENT" restore --dry-run --json
arca agent skill converge "$SKILL_NAME" --agent "$AGENT" detach --dry-run --json
```

三条是不同情况的独立例子。选择目标域和当前允许的动作，Model 有时用状态返回的准确外部候选 ID。遇到多物理目标以当前帮助、状态和计划确认实际作用范围。用户要复制一份中央资产时，Skill/Model 也有单独的显式导入流程。`detach` 适合放开管理并留现场，`unassign` 则是移除分配关系。

交互终端可以用 `arca agent mcp converge KEY --agent AGENT detach` 查看本次计划后决定，按 `N` 或默认 Enter 取消。自动化交接用具名审阅会话。

MCP 的 `mcp_enabled_state_drift` 表示实际启停和期望不同。4.0.2 的 Pi 原生开关实测中，外部修改只有 `enabled`，提交 `detach` 会删原生条目。此时即使可用动作包含 `detach`，如果用户要求保留现场也应停在预览。对于其他使用原生启停字段的客户端，也先核对当前版本的实际行为。

收到 `observation_stale` 或审阅目标改变时，重读状态与现场，再生成计划。Skill 内容变动需复查候选和风险。用户审阅内容或影响变化较大时，把新的准确范围交回审阅。[提交或取消同一份收敛计划](../technical/cli-protocol.md#收敛审阅示例)

## 部分目标同步失败

`pending_convergence` 的退出码是非零，但可能已经保存中央变更。读取 `error.details.changed`、`operation_id` 和 `target_incidents`。

1. 重查命令针对的能力与 Agent。区分已存在的中央资产/关系、成功的物理目标和失败目标。
2. 用 incident 的 `capability`、`target_id`、`affected_agent_ids`、`code` 和 `retryable` 定位。目标共享时按同一物理文件或目录处理。需要更多原因时用 `arca logs show --operation "$OPERATION_ID" --json`。
3. 修正权限、路径、进程环境或明确冲突等已知原因。用当前状态所提供的有效动作来预览和提交最小范围恢复。Skill 修复只用于 [确认为可修的目标](skills.md#修复一个失败目标)。
4. 重查该目标和受影响的关系。报告中央已保存部分、完成的物理目标、仍待处理项与失败原因。运行时加载如在任务范围内也一并复查。

举例，中央 Skill 分配计划需要同步两个实际目录，其中一个已成功，另一个返回可重试的缺链。核对失败关系仍启用、中央副本完好与该目标链接可安全修复，再审阅对应 target 的 repair。成功目标无需再次提交。

## 能力故障和共享恢复

- `capability_errors` 或后端 `capability_unavailable` 表示某个能力的读写受影响。定位具体能力，其他能力可以按其当前状态继续。
- `recovery_error`、后端只读或 `recovery_required` 指向共享设置或事务恢复。保留现场，核对当前数据根、运行日志中的恢复阶段与受影响的操作，使用该版本提供的明确恢复途径。凭据后端或目录不可达时先恢复真实依赖。
- 原生文件损坏、歧义或并发变化时，先通过支持的方式处理原文件问题。保留原件与备份，按原生应用加载规则复验。Arca 的 `journals/`、`staging/` 和备份与事务相关，恢复时保留这些证据。

[Arca 4.0.2 状态与收敛](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#外部变更与收敛) · [Core 数据与恢复概念](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/concepts.md)
