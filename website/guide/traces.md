# Agent Trace 与网络记录

Trace 展示支持的 Agent 本机会话、消息和工具事件。网络记录展示保存的请求与响应；两者都是排查与审阅入口。

## Agent Trace

1. 点击顶部 **Trace**，选择 Agent 和本机会话。
2. 阅读消息时间线，展开工具调用与结果。
3. 查看事件详情；也可以导入支持的本地会话文件。

会话按页读取，详情绑定当前页的 revision。文件在读取后变化时，需要重新读取，避免把旧事件位置套到新内容。文件损坏、未写完整或超过读取限制会返回错误。

Arca 支持的原生会话格式以当前客户端适配器为准；有启动入口不等于有 Trace 适配器。导入不会把第三方内容当成执行指令。

## 网络记录与代理

**网络**菜单提供代理设置与记录入口。代理配置用于 Arca 的远程来源、GitHub Skills、更新检查等联网操作，不会自动改写所有 Agent 的系统代理。

在 Agent 页面打开 **抓包**，工作台直接沿用当前 Agent，无需再次选择；进程和历史会话也限定在该 Agent 下。可以选择 **启动新进程** 或 **运行中的进程**。前者为支持的运行时追加抓包证书，再绑定新实例；后者记录指定的已有进程。出口可选择直连、Arca 代理或自定义 HTTP 代理。系统证书型客户端仍需系统信任抓包 CA。

Desktop 与 CLI 共用 Core 的发现、启动、转发、脱敏和停止流程。`capture list/show` 读取已保存记录，`capture environment` 检查环境，`capture state` 通过进程身份核验会话状态。`capture run` 在前台记录指定时长并停止；关闭其所属进程也会结束抓包。

## 给其他 Agent 读取

```bash
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca trace show --file /path/to/session.jsonl --json
arca capture list --agent codex --json
arca capture show SESSION_ID --limit 100 --json
arca capture show SESSION_ID --flow FLOW_ID --json
arca capture environment --json
arca capture run --agent pi --target TARGET_ID --seconds 60 --json
arca capture run --agent pi --launch --directory /path/to/project --seconds 60 --json
arca capture state SESSION_ID --json
```

使用页面返回的 cursor 继续读取，事件详情使用对应的 revision。Capture 使用增量缓存避免反复解析未改变的历史内容。

首次开始记录会由官方 mitmproxy 安装并注册 Mitmproxy Redirector，无需终端命令；注册前可能不会出现在系统设置中。通知条提供 **打开授权设置** 按钮。按 macOS 提示批准安装与启用后，Arca 自动检测并继续记录，无需手动刷新，最多等待 3 分钟。

已有进程不能在运行中追加 CA，代理设置或已经建立的连接也可能使请求绕过记录。TLS 失败时优先使用新进程；启动器无响应时会在 10 秒后明确报错，已完成扩展初始化的抓包引擎未就绪时会在 12 秒后停止并保存脱敏原因。按进程抓包目前仅支持 macOS，需要官方 mitmproxy；系统权限仍需用户批准。

## 内容与脱敏

Trace 会先建立凭据脱敏上下文，再返回内容；不同页与单独读取详情遵守同一规则。上下文和当前读取合计上限为 32 MiB / 100000 条记录，缺失可信上下文时不会继续输出正文。

会话与网络正文仍可能包含代码、业务资料或其他个人信息。分享前检查实际内容；官网演示仅使用公开示例，不使用用户会话。导入文件保留在本地，不因预览而上传。

[CLI 审阅与自动化](/guide/cli#审阅并执行同一份计划)
