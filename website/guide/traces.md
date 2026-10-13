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

查看已保存记录，可以检查请求、响应与单个 flow。CLI 的 Capture 命令只读取保存内容，不启动抓包，也不把当前 CLI 进程当成桌面抓包状态的权威。实际抓取需按桌面环境说明配置。

## 给其他 Agent 读取

```bash
arca trace list --agent codex --limit 20 --json
arca trace show SESSION_ID --json
arca trace show SESSION_ID --event EVENT_ID --revision PAGE_REVISION --json
arca trace show --file /path/to/session.jsonl --json
arca capture list --agent codex --json
arca capture show SESSION_ID --limit 100 --json
arca capture show SESSION_ID --flow FLOW_ID --json
```

使用页面返回的 cursor 继续读取，事件详情使用对应的 revision。Capture 使用增量缓存避免反复解析未改变的历史内容。

## 内容与脱敏

Trace 会先建立凭据脱敏上下文，再返回内容；不同页与单独读取详情遵守同一规则。上下文和当前读取合计上限为 32 MiB / 100000 条记录，缺失可信上下文时不会继续输出正文。

会话与网络正文仍可能包含代码、业务资料或其他个人信息。分享前检查实际内容；官网演示仅使用公开示例，不使用用户会话。导入文件保留在本地，不因预览而上传。

[CLI 审阅与自动化](/guide/cli#审阅并执行同一份计划)
