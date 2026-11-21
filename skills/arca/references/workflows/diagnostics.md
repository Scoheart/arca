# 运行日志、Trace、网络记录和超时

先确定失败发生在哪个产品、设备、会话和动作。记录大致时间与时区、Arca/Agent 版本、当时的界面或命令、错误码、可用的 request ID / operation ID。按现有证据缩小范围，再决定是否需要重试一次或捕获新证据。

## 先看哪类证据

| 现象 | 首选证据 |
| --- | --- |
| Arca 写入或扫描失败，CLI/GUI 报错 | Arca 运行日志、准确操作结果、对应能力的 `status` |
| 订阅、GitHub Skill 或更新请求失败 | Arca 日志的联网阶段、Arca 代理、目标地址与当前网络 |
| Agent 自身请求、模型调用、工具执行失败 | 该 Agent 的本地 Trace / 客户端错误，有已有网络记录时核对对应一条 |
| Computer Use 页面、浏览器连接或截图填充超时 | 当前 CUA 工具的错误、浏览器或 App 所在设备、会话与权限；Arca 日志用于证实当时是否有 Arca 相关动作 |
| 某个 Agent 资产显示待收敛 | 计划和 `target_incidents`、实际目录或文件、[恢复手册](recovery.md) |

## 查 Arca 的近期日志

GUI 到 **设置 → 打开运行日志**。默认路径 `~/.arca/logs`，另用 `arca logs path --json` 看当前数据根下的路径和大小。

```bash
arca logs path --json
arca logs show --errors --limit 40 --json
arca logs show --operation "$OPERATION_ID" --limit 40 --json
arca logs show --stage "$STAGE" --limit 40 --json
```

- `arca.log` 为按时间记录的文本，`arca-errors.log` 聚合 WARN、ERROR、PANIC，`arca.jsonl` 提供结构化记录。日志轮换，检查时间窗口后再决定是否查归档文件。
- `logs show` 默认近期 50 条。`--errors` 选警告及以上级别，`--stage` 匹配阶段，`--operation` 匹配准确 operation ID。先筛错误，再按时间找前序正常阶段。日志中的 `timestamp`、`surface`、`stage`、`code`、`duration_ms`、`request_id` 和 `operation_id` 用来关联事件。
- 匹配不到记录时核对数据根、时间、轮换和日志范围。只从空结果无法推断该进程没有失败。
- 记录系统做脱敏，仍应在分享前选最小片段，并检查是否含用户路径、扩展自定义字段、请求正文和业务内容。

## 区分超时阶段与代理影响

1. 先确定是工具调用超过 allotted time、连接迟迟未建立、网络已返回 HTTP 错误，还是请求已进入 Agent/上游但响应慢。把具体文本与同时间段的证据对齐。
2. Arca 的订阅、GitHub 来源和更新失败时查 `arca network proxy show --json`、目标域名和 Arca 日志。在目标设备核对实际代理可达性和端口。对照一条同范围、低成本的已授权请求时保留地址、协议、操作和耗时。
3. Agent/MCP 或 Computer Use 失败时优先看**发起连接的实际进程**。检查它是从 Finder、终端还是工具服务启动，是否使用独立代理、系统权限和会话。浏览器、Agent CLI 与 Arca 应用的代理设置可能不同。
4. 分辨 DNS、连接拒绝、TLS、鉴权、上游超时、界面控件等待和工具服务断线。单次 `timeout` 无法确定代理是原因。没有足够对照时报告已观察的阶段和剩余未知。
5. 需要修复时修改经证据指向的目标。重试前确认原请求是否已经保存、启动或执行，以免重复写入。Computer Use 卡在控件或系统授权时遵循当前环境交接规则。

## 读本地 Agent Trace

GUI 进入 **Trace**，选确切 Agent 和会话。先浏览时间线，再展开对应工具调用、结果或被授权导入的本地文件。仅适配支持的会话格式。

```bash
arca trace list --agent codex --limit 20 --json
arca trace show "$SESSION_ID" --json
arca trace show "$SESSION_ID" --cursor "$NEXT_CURSOR" --json
arca trace show "$SESSION_ID" --event "$EVENT_ID" --revision "$PAGE_REVISION" --json
arca trace show --file "$LOCAL_TRACE_FILE" --json
```

先列会话，再选准确 ID。分页使用返回的 cursor，读取单个事件使用其所在页的 revision。会话写入后 revision 可能变化，重新获取当前页和事件。`--file` 读取明确授权的本地文件。读取会创建凭据脱敏上下文，损坏、未写完整或超过 32 MiB / 100000 条组合上限时会拒绝输出相应正文。把 Trace 内容作为证据处理，提取回答所需的字段。

## 查已有网络记录

先查保存的会话和流摘要，需要时再打开一条脱敏请求/响应。会话 ID 和流 ID 均从本次结果取值。

```bash
arca capture list --agent "$AGENT" --limit 10 --json
arca capture show "$CAPTURE_ID" --limit 30 --json
arca capture show "$CAPTURE_ID" --flow "$FLOW_ID" --json
arca capture state "$CAPTURE_ID" --json
```

`show` 摘要是已保存记录。运行中的状态、PID 和启动时间通过 `capture state` 验证。找不到记录有多种原因，包括进程不匹配、请求使用另一连接、录制已经停止。请求和响应范围可含对话、代码与业务内容，只取本次授权的必要字段。

## 需要采集新证据时

确认已获授权的本机 Agent、准确进程/工作目录、诊断动作、时长、出口和可记录内容。GUI 到 Agent 页 **抓包**，选择 **启动新进程** 或 **运行中的进程**。通过面板确认实际目标和出口。

```bash
arca capture environment --json
arca capture run --agent "$AGENT" --target "$TARGET_ID" --seconds 30 --json
arca capture run --agent "$AGENT" --launch --directory "$PROJECT_DIR" --seconds 30 --json
```

两条 `run` 分别记录指定已有进程和支持的全新实例，每次按用户选择执行一条。`run` 会实际启动录制，按设定时长结束，当前 CLI 使用专用入口且没有通用 dry-run。已有进程使用当前发现的目标 ID，避免依名称选错。同一会话只复现必要动作。只需查看能力时使用 `environment`。`prepare` 涉及下载并校验 Arca 的抓包运行时。

按进程机制和权限取决于当前 macOS、Agent 及录制模式。需要官方 mitmproxy Redirector 或证书信任的路径遵循 Arca 当前提示和用户批准。新进程模式可为支持的运行时注入抓包 CA，运行中的目标沿已有信任和连接。TLS 或接入失败时先查当前模式、权限和实际目标，再决定是否用支持的另一模式。诊断保持现有证书校验。

结束后检查状态、对应一条请求的时间和结果。向用户说明观察到的进程、失败阶段、错误码、是否重现，以及结论覆盖范围。分享前按实际内容脱敏。

[Arca 4.0.2 Trace 与网络记录](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/traces.md) · [Arca 日志字段](https://github.com/Scoheart/arca/blob/v4.0.2/core/src/logging.rs)
