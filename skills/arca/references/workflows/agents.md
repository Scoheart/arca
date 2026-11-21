# Agent 身份、能力、启动和原生资源

识别客户端、修改配置和启动进程是三个操作。确定用户想打开哪个 App 或 CLI、处理哪个资源能力，以及是设置以后启动的默认值还是只启动一次。

## 识别客户端

```bash
arca agent list --json
arca agent status "$AGENT" --json
arca agent launch show "$AGENT" --json
```

- 从输出选准确 ID。GUI 用 **选择 Agent** 或置顶入口进入工作区，再核对 CLI、Desktop、IDE、Plugin 形态。同名客户端可有不同 ID 和权限。
- `runtime_detected` 表示运行时检测结果，`null` 表示未知。`config_detected` 表示检测到原生配置。插件宿主版本、插件是否安装与可写能力分别核对。
- MCP 配置路径和传输、Models 协议和选择策略、Skills 用户级目录是独立能力。无可写能力时使用当前 Agent 给出的原生设置引导。
- 同名关系有时共享文件。例如 Codex CLI 与 Desktop 读取共享配置及用户级 Skills，Model writer 由 `codex` 管理。OpenCode CLI/桌面也有共享原生目标。按当前计划中的真实路径和受影响身份判断。

## 启动一次，或配置默认启动方式

GUI 的具体 Agent 页面上方有运行时版本、启动和编辑入口。CLI 可以选择工作目录及默认终端，桌面 App 通常通过系统启动器打开。Web 打开目标浏览器。使用当前窗体可选的启动配置与目录，核对自定义参数、环境和新实例选项。需要一次性目录时选择本次使用的目录。

```bash
arca agent launch show "$AGENT" --json
arca agent run "$AGENT" --directory "$PROJECT_DIR" --dry-run --json
arca agent run "$AGENT" --directory "$PROJECT_DIR" --yes --json
```

`run` 按客户端支持的启动类型使用参数。预览用来核对解析到的运行时、工作目录和终端。提交后通过目标应用窗口、终端进程或浏览器页确认实际启动。已发送启动请求和完成启动是两个观察点。

只更新默认目录时使用局部参数，当前启动目标由 Arca 保留。传空字符串清除默认目录；省略参数则保留。

```bash
arca agent launch configure "$AGENT" --default-directory "$PROJECT_DIR" --dry-run --json
arca agent launch configure "$AGENT" --default-directory '' --dry-run --json
arca agent launch reset "$AGENT" --dry-run --json
```

`launch reset` 恢复自动检测前先确认用户要重置启动设置。CLI 与桌面共享启动设置。`--file launch.json` 配置整份 `LaunchTarget`，编辑前从 GUI 或用户的受保护配置取得完整目标。

| `kind` | 输入字段 |
| --- | --- |
| `cli` | `command`，可选 `args` 字符串数组和 `env` 键值对象 |
| `app` | `path`，可选 `args`、`env`、`new_instance` 布尔值 |
| `web` | `url` |

CLI 启动草稿示例。使用实际已安装的命令及目标客户端文档支持的参数。

```json
{"kind":"cli","command":"/absolute/path/to/agent","args":[],"env":{}}
```

```bash
arca agent launch configure "$AGENT" --file launch.json --dry-run --json
```

`launch show` 提供脱敏的参数数量和环境变量名。修改前确保保留任务之外的参数和环境。需要代理时核对目标客户端支持的配置与启动进程实际环境。用于 API 的 Key 优先通过 [Provider 交付方式](../technical/model-connections.md#agent-凭据交付) 处理。

## 调整配置位置或添加自定义 Agent

1. GUI 在目标 Agent **编辑配置** 中核对 MCP 文件、Model 路径或用户级 Skill 目录。使用确认为该产品原生支持的格式和全局路径。`配置` 展示当前能力路径、文档和打开方式。
2. CLI 对现有 Agent 用 `agent configure AGENT --file PATCH.json --dry-run --json`。`AgentConfigurationPatch` 按能力提供字段。只改变 Model 时可用 `{"model":{"paths":["~/.codex/config.toml"]}}`。MCP 局部形状为 `{"mcp":{"path":"…","key":"…"}}`，Skills 为 `{"skill":{"global_dir":"…","alias_dirs":[]}}`。准确值由当前配置与客户端契约确定。
3. 自定义 Agent 从 **选择 Agent** 的添加入口建立，或将完整已核验 `AgentDefinition` 交给 `arca agent save ID --file agent.json --dry-run --json`。自定义 MCP 定义需要明确用户级路径、格式和 MCP 键。有已有 ID 且有意替换定义时使用 `--replace`。
4. 路径变更前审阅已有消费者、共享目标与新路径当前内容。提交后检查实际路径和关系。原生配置由 Arca 支持的适配器负责写入与保留非目标内容。

Agent 在 Arca 中的启停是 Arca 管理设置。想停用某个资源，应走该 Agent 的具体关系，而全局启停前应核对其管理范围。

## 管理原生资源

这组资源在桌面中按实际客户端能力显示。先判定当前是在定义库、具体客户端工作区还是本地文件编辑器。

| 页面 | 当前任务的入口与核对点 |
| --- | --- |
| 顶栏 **Agents** | 建立或查看启动用 Agent/角色定义。在目标客户端的对应标签查看可用的启动参数、原生投影或默认能力 |
| **Subagents** | 从中央定义库找准确项，在具体 Agent 的 Subagents 标签添加或查看实际原生定义 |
| **Hooks** | 查定义、触发事件、命令与实际客户端支持，再按授权分配或移除。应用后复查挂载是否出现在所选 Agent |
| Agent 页 **Instructions** | 查看当前原生全局文件、共享读者、可编辑状态和加载提示。选择明确文件后预览、编辑与保存 |
| Agent 页 **Plugins** | 查看原生插件信息和当前能力，安装或配置沿产品提供的对应入口 |

Instructions 保存用户对原生文件的修改。页面只在检查完整且有明确创建入口时允许主动新建。写入前核对路径和其他读者，遇到外部改动保留草稿，重新加载并核对差异后再保存。变更 **编辑 → 配置文件** 中的 Instructions 路径只修改 Arca 读取位置，已有原生文件按原位置保留。保存后依据界面给出的重载提示验证。

## 常见卡点

- 启动器存在但配置能力为只读时，按该能力的官方引导。启用 Arca 的 Agent 记录不会获得缺失的原生写入契约。
- shell 中能运行、Finder 启动失败时，核对 Arca 解析的运行时路径、目标环境、默认终端与目录权限。自定义 CLI 的版本通常不会被自动执行探测。
- 启动了同名但错误的 App 或 CLI 时，按 GUI 徽标与 `launch show` 对照默认目标和覆盖设置。只修正准确 Agent 的启动配置。
- 共享文件已有其他资源时，以当前可用关系动作和作用范围为准。[同步与恢复](recovery.md)

[Arca 4.0.2 Agent 说明](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/agents.md) · [CLI 启动规则](https://github.com/Scoheart/arca/blob/v4.0.2/website/guide/cli.md#启动-agent-与共享设置)
