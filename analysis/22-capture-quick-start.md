# 抓包低配置启动：分析与实现

日期：2026-10-18。基线：2.4.6 / 73482b0。目标是让用户选择 Agent 后直接开始，只有不可代办的系统授权需要用户处理。

## 原有负担及根因

| 用户要理解的事情 | 原因 | 本次处理 |
|---|---|---|
| 安装 mitmproxy、配置 PATH | Core 仅查找外部 mitmdump，UI 只有禁止开始的文字 | 下载固定官方组件、校验哈希和签名、安装到 Arca 私有目录 |
| 为什么 CLI 也要批准网络扩展 | 所有目标统一使用 local capture | Codex CLI / Pi 新实例优先使用进程代理，其余路径保留扩展 |
| 启动 Agent 后为什么漏请求 | 原来先启动 Agent，再启动抓包引擎 | 代理先监听随机本机端口，再启动 Agent |
| 证书在哪里、需要做什么 | 系统 CA 前置校验与自动生成之间存在阻塞；没有可操作入口 | 一键生成并打开公开证书，SSL 授权由用户在系统 UI 完成 |
| 是否要选出口、进程、工作目录 | 每次进入抓包都弹出完整设置表 | 默认开始直接执行；可选项放在明确的“记录选项”入口 |
| 已有代理为什么仍不能联网 | 默认直连或仅跟随 Arca 配置 | Core 自动检查 Arca → 环境 → 系统 HTTP 代理，失败不悄悄绕过 |
| 停止抓包后新启动 CLI 会不会断网 | 进程代理地址不能在进程启动后普遍撤销 | 停止保存并释放记录锁，保留仅转发助手至目标退出 |

## 实现边界

Core 继续拥有策略、安装、CA、出口、进程绑定和生命周期。Desktop 与 CLI 都调用相同用例，不在 React 中复制网络发现或启动命令。

- `install.rs`：固定 mitmproxy 12.2.1，官方下载域名，Apple Silicon / Intel 分别固定 SHA-256；最大下载 256 MiB、总超时 180 秒，下载校验后才解包并核验代码签名，使用同目录 staging 和最终 rename。既有可用组件可复用，失败清理本次临时目录，不要求 Homebrew 或 Python。
- `routing.rs`：自动出口与公开证书准备；不修改系统代理、不自动修改 Keychain 信任、不关闭 TLS 校验。
- `capture-agent-profiles.json`：仅 Codex CLI / Pi 标记 `explicit_proxy`，匹配到 CLI launcher 时才启用。两者已有独立显式代理实验依据；其他产品不能凭“是 CLI”推断兼容。
- 代理启动：监听 `127.0.0.1:0`，从引擎就绪状态取得实际端口，随后在新 Agent 进程环境中设置代理及额外 CA。HTTP/HTTPS 请求依赖客户端遵循代理，不能宣称覆盖绕过代理的 SDK、UDP 或全部网络调用。
- 会话记录仍为私有目录、脱敏正文、容量上限和增量索引。原始命令行、凭据和未脱敏正文不会新增到安装日志或会话元数据。
- 停止/所有者退出：先收尾记录，再关闭记录入口；辅助进程保持原出口与 TLS 校验，仅转发。通过继承 FD 的设备号/inode 核验释放该会话的记录租约；不删除锁文件，不停止其他 Agent 或代理。目标 PID 与启动时间继续约束生命周期。

## 验证证据

本地证据保存在父仓 Git 忽略目录 `workspace/local/research/arca-capture-usability-2026-10-18/`。

- Rust Core / CLI 实际构建完成；TypeScript / Vite 生产编译完成。
- `capture prepare` 在隔离 HOME / ARCA_HOME 中完成真实官方下载、哈希校验、macOS 签名校验及私有目录安装，返回 ready。
- 现有 12.2.1 引擎与本次下载的引擎分别执行真实网络实验。独立本机 HTTP、带验证的 HTTPS、SSE、认证 canary 脱敏通过；在 PID 绑定前发送的启动请求被记录。
- 正常停止后请求继续成功且会话文件哈希不再变化，另一个记录租约能立即获取。
- 下载的引擎在记录所有者退出时自动停止保存，后续 HTTPS 仍成功；目标退出后引擎退出。整个实验仅使用临时 HOME、合成数据和本机服务，没有调用用户模型账户或信任真实系统 CA。
- 组件交互与两种窗口尺寸检查使用明确标识的合成数据；不能替代逐 Agent 的原生客户端验收。
- 按项目极速交付规则，不运行 cargo/npm 测试套件、fmt、clippy 或发布 preflight。保留真实数据的本地 Tauri 开发预览。

## 仍需后续验证或改进

1. 系统型桌面客户端仍需一次 CA 信任和网络扩展批准。证书固定、应用自有信任库和第三方网络扩展无法靠统一环境变量解决，也不能以绕过 TLS 校验代替。
2. 继续逐产品验证显式代理能力，只有证据成立后再扩展注册表。当前改动不能宣称所有 Agent 已完成真实模型调用验收。
3. SOCKS、带认证代理、PAC 仍需要独立适配；自动检测遇到它们会给出明确说明，不静默直连。现有 ProxyBridge 等规则不自动修改。
4. 官方组件采用固定版本而非每次追新。后续应维护兼容版本与签名/哈希升级策略，避免内部 LocalRedirector 接口变化导致隐蔽失败。
5. 后续可增加持续的“无流量 / TLS 失败 / 出口失败”诊断状态，把当前底部说明进一步变成可执行动作；这不应演变成新的前置设置表。

## 来源

- [mitmproxy 官方代理模式](https://docs.mitmproxy.org/stable/concepts/modes/)：对支持 HTTP 代理的客户端推荐 regular proxy；local capture 使用操作系统拦截能力。
- [mitmproxy 官方证书说明](https://docs.mitmproxy.org/stable/concepts/certificates/)：HTTPS 解密要求客户端信任对应 CA，各运行时信任机制不同。
- [固定 12.2.1 官方包与校验值的 Homebrew 审核记录](https://github.com/Homebrew/homebrew-cask/blob/e0021c91dcc56d71c47967ca9429ac9e0b864ab3/Casks/m/mitmproxy.rb)。
- [12.2.1 proxyserver 源码](https://github.com/mitmproxy/mitmproxy/blob/v12.2.1/mitmproxy/addons/proxyserver.py)：从实际监听地址取得随机端口。
- [既有逐 Agent 抓包验收](16-agent-capture-workbench.md) 与 [记录租约修复](21-trace-conversation-and-capture-lease.md)。
