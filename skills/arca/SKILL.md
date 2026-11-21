---
name: arca
description: 使用 Arca 桌面 GUI、arca CLI 或 MCP TUI。适用于管理 Provider、模型和凭据存储，订阅与分配 MCP，导入与分配 Skills，配置或启动 Agent，检查更新，以及排查同步、代理、Trace 和网络记录。
---

# 使用 Arca

按用户指定的设备、工作区和界面开展操作。CLI、MCP TUI 和桌面 GUI 在同一用户与数据根下共用 Arca Core 和资源。示例以正式版 4.0.2 为基准。遇到选项或界面变化时，以本机 `arca --version`、目标子命令的 `--help` 和当前窗口为准。

## 选择本次要读的指南

知道具体任务时，直接打开对应任务手册。需要选界面、定位按钮或掌握按键时，先读一份界面指南。具体参数、输入文档和恢复步骤由相关手册继续链接，按卡点读取。

| 界面 | 何时使用 |
| --- | --- |
| [CLI](references/cli.md) | 查准确 ID、批量状态、写入预览、脚本化验收和交接审阅 |
| [MCP TUI](references/tui.md) | 在交互终端浏览中央 MCP、管理来源和操作 Agent 的 MCP 关系 |
| [桌面 GUI](references/gui.md) | 通过表单、选择器和详情管理资源，或验收窗口中的真实结果 |

| 任务 | 打开 |
| --- | --- |
| 添加、导入、订阅或分配 MCP；查看来源与传输差异 | [MCP 手册](references/workflows/mcp.md) |
| 连接服务商、保存 Model、设当前模型、试验台对比 | [Provider 与模型手册](references/workflows/models.md) |
| 从文件夹、压缩包、GitHub 或现有 Agent 导入 Skill；分配、更新、修复 | [Skills 手册](references/workflows/skills.md) |
| 识别 Agent 能力、启动或配置客户端、管理 Instructions 等桌面资产 | [Agent 手册](references/workflows/agents.md) |
| 安装或更新、找数据根、设置语言/终端/代理 | [安装与设置手册](references/workflows/settings.md) |
| 定位超时、查运行日志、本地 Trace 或已有网络记录 | [诊断手册](references/workflows/diagnostics.md) |
| 外部改动、实际状态和中央关系不一致、某个目标写入失败 | [同步与恢复手册](references/workflows/recovery.md) |

## 确认范围并验收

1. 先区分中央资产、Agent 关系和客户端现场。Provider 是共享连接，Model Profile 引用 Provider。添加、启停、当前模型、解除关系及删除中央资产各自改变不同状态。
2. 用 `arca agent list --json` 或 GUI 的身份徽标定位准确 Agent。MCP 使用 `name::transport`，Model 使用 Profile ID，Skill 使用中央名称。来源 ID、外部候选和物理目标从当前结果取值。共享目录或文件可能影响多个 Agent。
3. 按用户请求执行目标动作。变更前核对准确对象、现有关系和影响，必要时预览写入计划。调用付费 API、启用网络记录和处理密钥等动作按各自的实际用途授权。
4. 操作后重查准确关系和实际目标。用当前可用动作处理外部漂移。`pending_convergence` 可能已经保存中央状态，先核对成功与失败目标。报告足以证明结果的版本、范围和状态，敏感内容用最小范围和脱敏视图。
