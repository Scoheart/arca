# Desktop / CLI 对齐与弹窗简化

目标：两端管理相同的中央资产与 Agent 配置，复用 core 的校验、计划、提交和错误语义。弹窗只默认展示完成当前任务必需的信息；保持 Arca 原有配色与简洁布局。

## 能力审查

| 能力 | 审查结果与处理 |
| --- | --- |
| MCP 中央资产增删改、来源订阅/导入/导出 | 已共用 core，保留 CLI 输出脱敏与显式导出边界 |
| Model / Provider 增删改、模型发现、凭据更新与清除 | 已共用 core；UI 空值保存与 CLI 显式清除均映射同一删除语义 |
| Skill 安装/导入/更新/修复/删除 | 已共用 core，保留风险确认与候选哈希 |
| Agent 资产分配/解除、启停、当前模型、漂移收敛 | 已共用 core；补齐 CLI MCP 全部启停 |
| Agent 自定义定义和配置路径 | 已共用 core |
| Agent 启动与启动配置、默认目录、参数/环境变量 | 补齐 CLI，复用 `application::agent_launch` |
| 默认终端、置顶 Agent、界面语言 | 补齐 CLI 共享偏好入口，复用 core，保留图形外观/编辑器等宿主交互 |
| Provider 官方文档 | Desktop 链接与 CLI 同一份官方 URL 元数据 |
| Model cURL | 将协议格式生成收敛至 core，Desktop 与 CLI 共用；显式导出密钥，普通 CLI 查询仍脱敏 |
| MCP 自定义图标 | 提供 CLI 同源设置入口；预览、文件选择由 Desktop 宿主负责 |
| 状态/快照、网络代理、更新 | 保留共同 core；CLI 安装升级与 Desktop 签名 App 更新属于各自分发方式 |

## 弹窗设计

- 所有普通表单、选择器、检查器与确认窗口复用同一套尺寸、间距、可滚动内容区和固定操作栏。
- Provider 默认只展示名称、服务地址、API Key；模型列表地址和协议端点放入“高级设置”。
- Model 默认展示 Provider、模型和协议；请求预览、上下文和输出限制按需展开。
- Agent 启动默认展示启动方式、应用/命令和工作目录；参数、环境变量和新实例开关按需展开。
- MCP 默认展示名称、命令/URL和必要参数；环境变量、请求头、主页等次要项按需展开。
- Skill / 资产确认先展示操作对象、影响和异常，详细文件与扫描证据按需查看。真实风险不可隐藏或跳过确认。
- 修复隐藏页签、关闭的折叠区和禁用字段参与焦点循环的问题；风险确认不再叠两层弹窗。

## 交付

用户本次已明确选择 Remote PR。与当前未发布的性能、Provider 文档、凭据编辑和模型卡片操作一起交付；保持原 checkout HEAD，官方流程自动生成版本与签名资产。发布后独立复验正式 App、CLI、Updater 与 DMG，再验收正式安装版。

## CLI 用法

```sh
arca agent launch show opencode
arca agent launch configure opencode --file launch.json --default-directory ~/Code --yes
arca agent run opencode --directory ~/Code --yes
arca agent run opencode --dry-run --json
arca agent launch reset opencode --yes
arca settings show --json
arca settings terminal ghostty --yes
arca settings locale zh-CN --yes
arca settings pins codex opencode --yes
arca settings pins --clear --yes
arca mcp disable-all --agent opencode --yes
arca mcp enable-all --agent opencode --yes
arca mcp icon set 'filesystem::stdio' files --yes
arca mcp icon import 'filesystem::stdio' --file ./icon.png --yes
arca mcp icon reset 'filesystem::stdio' --yes
arca model provider docs openai
arca model curl PROFILE_ID
arca model curl PROFILE_ID --include-api-key
```

`launch.json` 是与 Desktop 相同的 `LaunchTarget`，例如：

```json
{
  "kind": "app",
  "path": "/Applications/ZCode.app",
  "args": [],
  "new_instance": true,
  "env": { "NODE_USE_SYSTEM_CA": "1" }
}
```

`--default-directory ''` 清除固定目录；省略参数保留当前目录设置。配置参数和环境变量通过 JSON 文件或 `--file -` 输入，普通查询仅输出参数数量和环境变量名称。CLI cURL 默认引用 `$ARCA_API_KEY`，`--include-api-key` 是明确的凭据导出；Desktop 的复制按钮直接复制含实际凭据的命令。无需认证的模型不强加 Authorization。两端均不自动执行该请求。

GUI 外观、拖拽、文件选择和剪贴板属于宿主表现，不要求 CLI 模拟窗口。业务状态、写入规则及共享启动偏好均走同一 core。CLI 改动共享设置后，Desktop 通过文件观察刷新显示。

## 审查修复与验证

- 修复 Desktop 编辑 MCP 丢失 `cwd`、裁剪参数空格以及无修改时丢失空参数的问题。
- 修复隐藏页签、禁用字段、收起区域进入弹窗焦点循环的问题；初始焦点也排除隐藏控件。
- 修复资源选择器重复提交、异步拒绝未捕获，以及已禁用/消失选项继续提交的问题。
- 修复环境变量名 `__proto__` 被普通对象赋值吞掉的问题。
- MCP 图标目录提取为两端共享数据，CLI 可以列出可用图标；core 拒绝不存在的图标 ID。
- Skill 风险确认现在替换原确认页，保持一个弹窗，仍要求绑定原计划的风险指纹和显式勾选。
- Provider 被清除的凭据不会在 cURL 导出时回退到遗留 Profile 凭据。
- 可选布局/主题/提示偏好的浏览器存储不可用或写满时，页面与更新检查继续工作，当前会话保留用户选择。
- 显示缓存不用于事务决策；更改继续经过 core 的 plan、CAS、权限和回滚规则。

已完成 16 条弹窗定向回归、5 条凭据编辑回归和 2 条可选存储容错回归，以及隔离的 CLI/core 能力验收。后者覆盖共享偏好、启动设置/重置/仅预览、MCP 批量开关和图标、文档 URL、无密钥模型、四种请求协议与 shell 转义。未执行全量测试套件，未启动真实 Agent 或发送真实模型请求。

复现命令：

```sh
cargo build -p arca-cli
cargo run -p arca-core --example verify_desktop_cli -- /absolute/path/to/target/debug/arca
cd desktop
npm exec -- vitest run src/components/DesktopAlignment.test.tsx src/components/DialogShell.test.tsx
```
