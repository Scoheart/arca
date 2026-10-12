# Agent 抓包与 Charles 工作台

目标：Cursor、Grok Build、Qoder IDE/Desktop、Codex CLI/Desktop、Pi、Qoder CLI、OpenCode CLI/Desktop、QwenWork、QwenWorkCN、QoderWork 的按 Agent 实时请求/响应抓包。保留原上游出口，不把代理启动、注册记录或历史截图当作真实流量验证。

## 实现前核验（本地基线 1.12.0）

- 1.12.0 只提供 mitmproxy macOS local 模式，依靠进程名/Skills probe 识别客户端；Node/Python CLI 被排除。
- addon 只在 response/error 时保存；长 SSE 在结束前没有请求/响应记录，进程更新也会被长请求阻塞。
- 本机 mitmdump 12.2.1、Mitmproxy Redirector 网络扩展可用；CA 文件存在但系统验证未通过。ProxyBridge、Fiddler、Tailscale 扩展亦活动，需要按目标观察，不能全局停用。
- 已安装 Cursor、Qoder IDE、ChatGPT；发现 Grok、Codex、Pi、OpenCode CLI。后续发现 Qoder CLI 1.1.68 的本地 npm 缓存与启动包装器；Qoder Desktop、OpenCode Desktop、QwenWork/CN、QoderWork App 仍未发现。
- REA 官方 Skill 的方法用于区分静态制品证据和运行行为；不修改厂商应用、不禁用证书校验、不自动注册 REA MCP 或安装原生反编译器。
- Charles 官方文档确认 Structure（host/path 树）与 Sequence（时序列表），选择请求后显示 Request/Response、Headers、Body 详情。

## 完成所需证据

1. Agent 发现：根据实际 launcher、可执行文件和 CLI argv 匹配，Node/Bun 不扩大为所有 Node 进程；PID 实例和后代明确。
2. HTTPS：有可用的 CA 信任方式；运行中附加与带 CA 启动分开表达，不把系统 CA 等同于所有 SDK 信任。
3. 实时记录：请求发出时出现，响应 Headers 和 SSE 事件增量更新，选中详情随更新刷新；长流不会导致新子进程永久遗漏。
4. 出口：直连、MUX、自定义代理明确，错误不回退直连；本机内部通信不被误路由，环回 API 与内部 TLS 的覆盖区分。
5. 安全与生命周期：认证信息在进入磁盘前脱敏；限制正文/流量大小；停止、退出、失败和过期状态如实记录。
6. UI：Structure/Sequence、紧凑多列请求表、Overview/Request/Response/Timing、搜索/过滤、流式状态、可调整面板与两个窗口尺寸实际检查。
7. 逐 Agent 验证：真实进程、请求、响应、SSE 与停止证据。缺少安装/账号的产品保留明确未验证项，不能声称所有 Agent 已实测。

## 本地实现与验证（2026-10-12）

- 启动器与实际可执行文件绑定；macOS 使用 `proc_pidpath`，CLI 使用 PID + 启动时间。Pi 的已发布入口会重写 `process.title`；Qoder CLI 的包装器经 npx 启动实际 npm 入口，两者均有专门识别，未扩大为所有 Node。原生通用解释器缺少固定参数时不能绑定全部实例。
- 匹配表达式每个启动器只编译一次，可执行文件只规范化一次。本机 `capture environment` 单次查询从早期约 9.9 秒降至 1.8 秒；这是不同时间的运行观察，不是严格性能基准。
- 临时公开 CA 集合仅用于新进程，保留 TLS 校验，不写 Agent 配置。Codex 支持额外的 `CODEX_CA_CERTIFICATE`；Chromium 参数仅在实际发现 Electron/CEF 框架时传递。默认 Cursor、Qoder IDE、Codex Desktop 启动器增加已验证的 Bundle ID 约束，避免重名 App 混淆。
- 请求、Headers、SSE 在结束前保存。分片 UTF-8、gzip/deflate、多行 SSE data、结构化认证字段、数值 token 用量分别处理。原始传输字节不被修改；保存容量同时计算完整记录和索引摘要。
- 进程更新直接修改 mitmproxy 12.2.1 已存在的 LocalRedirector，不重建 mode 实例。首次实验只得到父进程请求；改为原地更新后，长 gzip SSE 保持完整，同时记录新辅助进程请求。此适配依赖该版本的 Python 内部接口，接口缺失时停止捕获，不回退广泛拦截。
- 多实例使用同一中央目录中的文件租约；引擎继承锁，所有者崩溃后仍保留至引擎退出。所有者和引擎均以 PID + 启动时间核验。跨进程实时状态、拒绝重复启动、所有者崩溃后的中断行和自动收尾均已验证。
- CLI 增加 `capture environment/run/state`，与 Desktop 共用 core。捕获不再长期占用中央资产写入 gate。非本实例的停止只发送私有停止标记，不杀死其他 MUX 或 Agent 进程。
- Charles 工作台：Structure/Sequence、可调整面板、紧凑搜索、请求表、Overview/Request/Response/Timing、Headers/Query/Pretty/Raw、复制与导出、增量刷新选中详情。使用真实 HOME 的原生预览检查 1200×820 与 900×600；截图展示历史公开请求，不冒充当前实时模型流量。
- 桌面端补充应用组和独立顶层 PID 实例两种选择。安装版 Cursor、Qoder 的 Electron 在隔离 HOME 中以受支持的 RunAsNode 执行本机 HTTPS gzip SSE 探针，两次均仅匹配该 PID、捕获 1 条请求，状态依次出现 pending / streaming / complete，正文从 0 增至 899 字节，认证字段脱敏通过。这是安装版网络运行时验证，不是 GUI 内的模型推理。
- 安装制品中的 Electron fuse：Cursor / Qoder 的 RunAsNode、NodeOptions 与 NodeCliInspectArguments 开启；Codex 桌面端三者关闭。Codex 的 Electron 框架实际名为 Codex Framework.framework，main bundle 使用 net.fetch；NodeOptions 关闭后，NODE_EXTRA_CA_CERTS 不会生效。新增类型化的进程 / 混合 / 系统 CA 策略与明确提示；系统型客户端在系统 CA 未信任时拒绝误导性的环境变量启动。已检测的 Chromium 框架包括 Codex 的改名框架，启动时可禁用 QUIC，仍保留证书校验。
- 系统型客户端的开始记录也在 core 中校验系统 CA，UI 显示禁用原因，防止只拦启动按钮却仍截断运行中的 HTTPS 请求。隔离 HOME 的 Codex Desktop CLI 实验返回 capture_system_ca_required，未启动任何抓包引擎；原生真实数据预览也显示禁用状态。
- 官方 OpenCode Desktop 1.19.0 的签名/公证包已核验。其 NodeOptions fuse 开启，发布源码使用 utilityProcess.fork 启动本地服务并继承环境；调整为混合 CA 策略。在官方 GUI 中选择本机 gpt-6-luna 测试接口并发送提示，桌面界面收到 CAPTURE STREAM VERIFIED，捕获两条 200 HTTPS Chat Completions（对话与标题）和增量 gzip SSE；记录出现 pending / streaming / complete，正文 0 → 899 字节，脱敏与停止通过。总共 21 条启动/模型记录，进程范围动态纳入辅助进程；这不代表所有浏览器请求已被解密。
- 桌面实验的额外隔离：仅覆盖 HOME 无法覆盖本机 Electron 的 home/appData；packaged App 也不接受 NODE_OPTIONS 的 --require。首轮仅作启动观察、未通过目录隔离验收。最终使用厂商已开启的主进程 inspector，在厂商 main 执行前明确设置 home/appData/userData，验证三者均位于实验目录后才继续；未修改厂商包、登录数据或证书校验。
- 官方 QoderWork 0.9.18 签名/公证包的 RunAsNode、NodeOptions 和 NodeCliInspectArguments fuse 均开启。隔离 Node HTTPS gzip SSE 探针仅绑定自己的 PID，捕获 1 条请求，状态/正文增长/脱敏/停止通过；据此调整为混合 CA 策略，GUI 模型流量仍未验证。
- 默认桌面启动器补齐 Qoder Desktop、QoderWork、OpenCode、QwenWork/CN 的已核验 Bundle ID。Qoder Desktop 同时接受 Qoder Desktop.app / Qoder.app，必须匹配 com.qoder.app，避免把旧名称的 Qoder IDE 绑定为新桌面产品。
- Qoder Desktop 0.4.4 官方安装器中的内嵌 Qoder.app 已离线解包；载荷 SHA-256 与签名清单一致，签名/公证与 com.qoder.app 核验通过。NodeOptions / NodeCliInspectArguments 均关闭，保持系统 CA 策略；不运行安装器、不修改已有 Qoder IDE 或厂商包，不在未信任系统 CA 时伪称环境变量足够。
- QwenWorkCN 1.2.5 官方包、签名/公证与 cn.qwenwork.desktop.mac 核验通过，NodeOptions 开启，源码显式处理模型网关的额外 CA。隔离 Node HTTPS / gzip SSE 探针通过，调整为混合 CA 策略。国际版的官方 CDN 域名由已签名客户端来源确认，取得包后还需独立检查其身份和运行行为；不把 CN 包当作国际版。
- 国际版 QwenWork 1.0.4 的包已通过镜像完整性、签名/公证和 com.qwenwork.desktop.mac 核验，NodeOptions 开启。独立隔离 Node HTTPS / gzip SSE 探针捕获 1 条请求，pending / streaming / complete 和 0 → 899 字节增长可见，脱敏与停止通过，调整为混合 CA 策略。两个版本保留独立文件名、Bundle ID、目录和验证结果；未运行其 GUI 登录或模型推理。
- WebSocket 不再只保存握手：按方向保存完整应用消息的序号/时间/文本，JSON 与已知凭据在落盘前脱敏；最多 512 条 / 2 MiB，二进制只保存长度与省略标记。150 ms 合并刷新，结束/停止取消未执行的刷新，避免完成状态被覆盖；原始帧与转发行为不修改，清理引擎内的原始消息列表以限制额外内存。
- 官方 QwenWorkCN Node 运行时的 WSS 实验使用应用组选择、重写进程标题、分片文本、认证字段回显和二进制帧。服务端消息数量实时从 0 增至 4，握手 101，streaming / complete 可见，总时长 5157 ms；客户端收到原样二进制，保存文件不含认证 canary，二进制正文被省略。加入 150 ms 刷新节流后再次验证通过。Python 抓包器的后续进程范围刷新也改用 proc_pidpath，与 core 的实际可执行路径识别一致。

| 客户端 | 已有证据 | 仍未验证 |
|---|---|---|
| Pi | 安装版 CLI 的 HTTP、HTTPS、gzip、多行 SSE、原样传输、脱敏、长流中的新子进程、停止/崩溃与跨实例锁 | 真实厂商模型推理；本次使用本机确定性 `gpt-6-luna` 接口，无付费调用 |
| Codex CLI | 安装版 CLI 的 HTTP/HTTPS Responses 请求与增量 SSE，模拟回复正常返回；以额外公开 CA 保留客户端/上游校验，脱敏通过 | 真实厂商模型请求与桌面启动按钮的端到端流程 |
| Grok Build | 安装版 CLI 的 HTTP/HTTPS Responses 请求与增量 SSE；补齐模拟响应的 usage 与 sequence_number 后回复正常返回，额外 CA 与脱敏验证通过 | 真实厂商模型推理 |
| OpenCode CLI | 安装版 CLI 的 HTTP/HTTPS Chat Completions 请求与增量 SSE，模拟回复正常返回，额外 CA 与脱敏验证通过 | 真实厂商模型推理 |
| Qoder CLI | 真实 npm 入口/PID 识别和 9 条启动 HTTP 请求，脱敏通过 | 隔离账号未登录，未进入自定义模型请求；不能算模型/SSE 验收通过 |
| Cursor / Qoder IDE | 已安装 App、实际进程、启动器与 Bundle ID 绑定；独立 PID 的安装版 Electron Node HTTPS / gzip SSE / 脱敏通过 | GUI 内模型 Body、Chromium 请求、完整推理；系统 CA 当前未信任 |
| Codex Desktop | 已安装 App、实际进程、启动器与 Bundle ID 绑定；确认关闭 NodeOptions / RunAsNode fuse，系统 CA 条件在 core / UI 中提示 | HTTPS 模型 Body 与完整推理；系统 CA 当前未信任 |
| OpenCode Desktop | 官方 1.19.0 包隔离运行；GUI → 实际辅助进程 → 本机 HTTPS gzip SSE → GUI 回复全链路通过，目录隔离/脱敏/停止通过 | 厂商模型推理与全部 Chromium 请求；系统 CA 当前未信任，未安装到 Applications |
| QoderWork | 官方 0.9.18 包、签名/fuse、Node HTTPS gzip SSE 与独立 PID/脱敏/停止通过 | GUI 模型请求和完整推理；系统 CA 当前未信任，未安装到 Applications |
| Qoder Desktop | 官方 0.4.4 内嵌包/清单哈希、签名/公证、独立 Bundle ID 与关闭 NodeOptions / 主进程 inspector 的证据 | 系统 CA 未信任；GUI 模型请求和完整推理，未安装到 Applications |
| QwenWorkCN | 官方 1.2.5 签名包、Node HTTPS gzip SSE、WSS 分片/双向消息/二进制原样转发/脱敏/停止、标题变化后的应用组跟踪通过 | GUI 登录/模型请求与原生 CLI/VM 网络栈，未安装到 Applications |
| QwenWork | 官方 1.0.4 签名/公证、独立 Bundle ID、Node HTTPS gzip SSE、独立 PID/脱敏/停止通过 | GUI 登录/模型请求与原生 CLI/VM 网络栈，未安装到 Applications |

交付前验证：Rust CLI/core、Tauri 开发构建、TypeScript/Vite 和 Python 语法通过；按项目极速模式没有运行 cargo/npm 测试套件、fmt、clippy。以上是本地实现阶段的证据，正式交付的提交、版本和签名包验证以 Git 历史、Release 与发布工作流为准；本次发布不代表下列未完成的逐客户端验收已通过。

参考：[REA Skill](https://github.com/morluto/rea/blob/main/.agents/skills/reverse-engineer-anything/SKILL.md)、[mitmproxy local 模式](https://docs.mitmproxy.org/stable/concepts/modes/)、[12.2.1 LocalRedirector 源码](https://github.com/mitmproxy/mitmproxy/blob/v12.2.1/mitmproxy/proxy/mode_servers.py)、[Charles 请求/响应视图](https://www.charlesproxy.com/documentation/using-charles/requests-responses/)、[Electron fuses](https://www.electronjs.org/docs/latest/tutorial/fuses)、[Electron net](https://www.electronjs.org/docs/latest/api/net)、[OpenCode Desktop 1.19.0 服务与环境](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/desktop/src/main/server.ts)、[QoderWork 官方下载](https://qoder.com/zh/download)、[Qoder CLI BYOK 账号条件](https://docs.qoder.com/cli/custom-models)。

逐客户端验收状态：改造已实现，完整范围的运行验收仍未完成。最终复核时系统 CA 仍未信任，Qoder CLI 的隔离环境仍未登录，全部实验引擎已退出，本地开发服务仍在运行。证书信任与独立登录条件连续多轮未变化；可独立完成的实现、包核验和隔离传输验证已结束，目标标记为受阻，保留完整范围。不能把网络探针、适配注册或未登录的启动流量当作全部 GUI 模型链路成功。
