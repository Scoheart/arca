# Arca 是什么

Arca 是一款 Agent 资源与配置管理工具。把 **Models、MCPs、Skills** 集中保存，再选择让哪些 CLI、桌面应用或 IDE 使用它们。桌面 App 和原生 `arca` CLI 共用 `~/.arca/` 与同一个管理核心。


## 从一份资产开始

| 你要管理的内容 | 在哪里创建 | 如何交给 Agent |
|---|---|---|
| 模型连接与模型 | Models：添加 Provider，再添加 Model | 在 Agent 的 Models 页选择兼容模型 |
| 工具服务 | MCPs：添加、粘贴、订阅或导入配置 | 在 Agent 的 MCPs 页选择中央条目 |
| 可重复使用的指令 | Skills：从 GitHub、本地文件夹或压缩包下载 / 导入 | 在 Agent 的 Skills 页选择中央副本 |

创建中央资产与让 Agent 使用它是两个步骤。一个模型、MCP 或 Skill 可以供多个兼容 Agent 使用；各 Agent 的格式、配置文件和共享目录由 Arca 适配。

## 日常使用

1. 在顶部选择 Models、MCPs 或 Skills，整理中央资产。
2. 打开 Agent 选择器，选择你使用的客户端。
3. 在对应标签页添加中央资产；需要时启用、停用或切换当前模型。
4. 出现外部改动时，查看差异，再选择采用外部内容、恢复 Arca 配置或解除管理。

普通操作直接执行并反馈结果。删除、覆盖本地内容和高风险 Skill 等需要审阅时，会集中显示一次确认。

## 桌面与自动化

桌面端提供资源卡片、来源导航、Agent 启动与版本信息，以及本地会话 Trace。CLI 可以管理同样的中央资产和关系，也可以批量读取 Agent 状态，把一份操作计划交给人或其他 Agent 审阅，再在同一进程执行。

无参数的 TUI 聚焦 MCP 管理；它与完整 CLI 的覆盖范围不同。详见 [CLI / TUI](/guide/cli)。

## 配置与同步

Arca 保留无关设置、注释和策略字段。扫描到的外部配置先保持只读，不会自动纳管。中央变化先保存，再按实际配置文件或目录同步；某个目标失败时，会保留已经成功的其他目标，并记录待处理关系。

中央 API Key 保存到系统 Keychain；Agent 使用环境变量引用、读取命令或经明确审阅的原生交付方式。具体限制见 [Models](/guide/models)。

[安装 Arca](/guide/install) · [观看演示](/guide/demo) · [查看支持的 Agent](/guide/agents)
