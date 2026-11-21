# CLI 输出、写入审阅和可恢复错误

在需要写自动化、把计划交给人审阅，或需要按错误码分支时读此页。普通终端使用见 [CLI 指南](../cli.md)。

## 结构化输出

普通成功结果是 stdout 上的一份 JSON 文档。`command` 标识实际结果语义。不同命令的 `data` 形状不同，兼容别名也可能共享一个 `command`。

```json
{
  "schema_version": 1,
  "ok": true,
  "command": "agent.mcp.assign",
  "changed": false,
  "data": {
    "dry_run": true,
    "would_change": true,
    "plan": {}
  }
}
```

上面省略了计划字段和可选 `backend`。`data.would_change` 表示预期效果，`changed` 表示这次调用实际改变了状态。无变更、取消、只读和 dry-run 通常为 `changed=false`。`backend.status` 提供启动与恢复边界，注意当前能力及可用诊断。

失败时检查退出码，并从 stderr 解析 `schema_version`、`ok:false`、`error.code`、`error.message` 和可用的 `error.details`。程序用稳定代码和当前 schema 处理分支，提示文案用于展示。命令帮助或 `--version` 仍按本机帮助输出处理。POSIX 程序应保留退出状态并分开 stdout/stderr。例如：

```bash
if arca agent mcp status --agent "$AGENT" --json >"$RESULT_PATH" 2>"$ERROR_PATH"; then
  jq '{command, changed, backend}' "$RESULT_PATH"
else
  jq '{code: .error.code, details: .error.details}' "$ERROR_PATH"
fi
```

结果文件应位于当前任务的受保护位置。完整 MCP export、显式凭据输出、Trace 和流详情在用途上与普通脱敏状态有差异。应用应按命令限制保存与分享范围。

## 选择写入接口

| 需要 | 接口 | 核对点 |
| --- | --- | --- |
| 一次性资源预览 | 具体命令加 `--dry-run --json` | 读取 `data.plan`、`data.would_change`。预览返回前会取消计划 |
| 常规直达写入 | 具体命令加 `--yes --json`，或终端交互确认 | 当前资产、影响范围、风险及其他选项。写入调用会重新规划 |
| 程序提交一个已知 Core 请求 | `arca operation apply --file request.json --yes --json` | 类型必须是当前版本的 `PlanOperationRequest`，沿相同的风险检查 |
| 先审阅再提交**原计划** | `arca operation review --file request.json --json` | 保持同一进程，用程序返回的决定模板提交 |

来源偏好、部分设置和独立 CLI updater 走专用接口，它们的 dry-run 可能只展示意图。`operation review` 处理有类型的 Core 资源计划。对新 operation 从现有源码定义确认 schema，再构造请求。

## 保持同一份计划的审阅会话

例如，按用户意图启用一个已存在且已停用的 MCP 关系。先从 `status` 确认 identity，再准备具名 `request.json`。

```json
{
  "operation": "set_mcp_enabled",
  "request": {
    "agent_id": "opencode",
    "asset_key": "docs::stdio",
    "enabled": true
  }
}
```

```bash
arca operation review --file request.json --json
```

1. 让子进程的 stdin 保持可写，读 stdout 的第一行。如果是 `phase: "review"`，读取 `data.plan`、受影响目标、警告与 findings。外层 `changed:false`。如果无变更，可能直接收到 `phase:"complete"` 与 `data.would_change:false`。
2. 用户或获授权的审阅者确认**该计划的实际内容**后，将同一行里的 `data.commit_decision` 对象原样序列化为一行 JSON，写回同一进程的 stdin 并换行。模板含 operation ID、候选哈希及需要的 findings 确认。范围不符合时写入程序给出的 cancel 决定或 `{"action":"cancel"}`。
3. EOF 也会取消。请求文件保留为具名路径，stdin 只供决定。该会话与 `--file -`、`--yes`、`--dry-run`、`--accept-risk` 冲突。决定行上限 16 KiB。
4. 读取最终 `phase:"complete"` 的 NDJSON 行与退出码。取消会带 `data.cancelled:true`。提交时核对 `changed` 和 `data.result`，再重读目标关系。如果发生错误，检查 stderr 的 JSON 文档，即使已经读到早期审阅行。

stdin 关闭或进程退出后，这次会话不能再确认。模板和计划绑定当前进程与现场，后续修改可能返回 `review_mismatch` 或 `observation_stale`。不要保存含秘密的确认草稿，重新审阅时取得本次模板。已有 Skill 计划还受产品的过期规则约束。

### 返回字段的语义

| 字段 | 使用方法 |
| --- | --- |
| `data.plan.domain`、变更和警告 | 描述作用域和待提交的变化。Skill 生命周期计划另有 `kind` |
| `data.plan.affected_agents`、`target_files` / `targets` | 合并共享文件或目录的影响。以实际写入目的地及具体变更为准 |
| `data.commit_decision` | 审阅成功后发送回同一进程，按敏感操作标识处理 |
| `data.cancel_decision` | 审阅后决定停止时回传 |
| `backend.status` | 区分就绪、能力局部故障、共享事务恢复与只读 |

审阅确认对象由程序生成。确认“同一份计划”时，使用此进程往返，比重新执行一个普通带 `--yes` 的命令多保留了一层绑定。带真实 Key 的 Provider 操作走专用凭据输入，具名 review 请求文件保持无秘密正文。

### 收敛审阅示例

用户希望查看解除管理的计划，同时保留已修改的 MCP 现场。先查看 `arca agent mcp status --agent "$AGENT" --json`，确认目标键、外部改动和可用 `detach`。再用无过滤的 `arca status --json` 取得 `data.revision`，并核对该次完整观测仍包含目标与动作。`observed_revision` 必须使用完整 inventory revision。对应 Agent 或单能力查询的 revision 和 `observation_id` 属于各自的范围标识。

```json
{
  "operation": "converge_consumption",
  "request": {
    "agent_id": "codex",
    "asset": {"domain": "mcp", "key": "docs::http"},
    "action": "detach",
    "observed_revision": "PASTE_FULL_INVENTORY_REVISION"
  }
}
```

把准确值写入具名请求文件，运行 `arca operation review --file request.json --json`。展示目标、关系变化和共享影响后，在同一进程回传 `data.cancel_decision` 即可取消。验收 `phase:"complete"`、`changed:false`、`data.cancelled:true`，并重读原关系确认仍受管。用户确实批准解除时也要核对当前版本可以保留实际条目，特别是 [原生启停漂移的已验证边界](../workflows/recovery.md#按意图选择收敛动作)。完成后复查管理关系与现场内容。

同一类型表达 `adopt-observed` 与 `restore-desired`。Model 资产写 `{"domain":"model","profile_id":"…"}`，Skill 资产写 `{"domain":"skill","name":"…"}`，值以当前查询为准。现场变化后重新读取并审阅。[4.0.2 收敛计划定义](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/domain/assets.rs) · [Core 收敛校验](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/application/convergence.rs)

## 错误与复验

| 错误或状态 | 应对方式 |
| --- | --- |
| `invalid_arguments` / `option_conflict` | 按该子命令帮助核对参数及 stdin 归属 |
| `confirmation_required` | 带预览，或按已授权的实际计划确认执行 |
| `operation_blocked` | 读取阻断原因，检查目标能力、安全边界和现有现场 |
| `review_mismatch` / `observation_stale` | 重读目标并生成新的计划，重新审阅实际影响 |
| `pending_convergence` | 中央可能已持久化。读取 `changed`、`operation_id`、`target_incidents`，只对失败范围恢复 |
| `capability_errors` / `recovery_error` | 分别检查故障能力和共享事务状态。详见 [同步与恢复](../workflows/recovery.md) |

可恢复错误有时返回 `retry_at` 或风险确认上下文，按当前结果和授权处理。恢复确认不用于普通日志、共享文档或后续其他操作。完成前用对应 Agent 和能力的查询校验实际状态。

[4.0.2 CLI 审阅说明](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#审阅并执行同一份计划) · [Core 请求类型](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/application/operations.rs) · [CLI 会话实现](https://github.com/Scoheart/arca/blob/v4.0.2/cli/src/operation_session.rs)
