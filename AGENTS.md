# AGENTS.md — Arca

> 作用域：Arca 独立 Git 仓库。用户用法写 README；完整操作规则按任务读取父仓 `llm-wiki/wiki/knowledge/`。

## 架构边界

- 跨 Agent 行为使用共享 Core 机制；原生配置路径、字段和运行时能力通过声明或适配器接入。用户代理地址和端口来自运行上下文，验证覆盖多个客户端及可变参数，具体故障实例只作为复现场景。
- Rust `core/` 是 Agent 发现、codec、MCP/Model/Skills 中央资产、消费关系与写入行为的唯一权威；CLI、TUI、Tauri command 和 React 只做薄适配。
- `data/` 是 Agent 与精选资源的 source of truth。新增可写 Agent 时同步 codec、发现、fixture、round-trip、图标 alias 与完整性检查。
- 当前只管理全局 Agent 配置和用户级 Skills；不得重新暴露项目级写入或在多个前端复制 core 编排。
- 顶层 `MCPs`、`Models`、`Skills` 是中央资产生命周期入口；Agent 页面只能选择和解除消费关系，不能创建、导入、编辑或安装资产。外部扫描结果保持只读，只有显式导入才进入中央资产库；唯一例外是 `native-registry` Model Agent 的专用 reviewed `clear_agent_models`，它可删除当前 Agent observer 已列出的全部原生 Model（含 external），但不得删除中央资产、Provider 或凭据，单个 external 卡片仍不可直接写入。
- 用户明确授权的 `Instructions` 是 Agent 原生全局文件查看/编辑能力，不属于中央资产。只保存用户对原文件的修改；完整检查确认没有原生入口后才允许用户主动新建一个文件。不得增加删除、迁移、分配、托管标记、中央正文副本、自动补文件或规则集合 CRUD。
- `Instructions` 目录通过 Agent「编辑 → 配置文件」配置，由 Core 按稳定原生 source ID 保存和解析；registry 路径只是默认值。更改路径只更新 Arca 偏好，不迁移原文件、不变更 Agent 原生加载设置，也不触发中央资产投影。

## 安全不变量

- 配置修改必须保留未知字段、注释、格式和非目标策略；损坏、歧义或并发变化时 fail closed，并经过备份、权限收紧和 CAS。已存在且被 Agent 监听的配置文件必须原地改写并保留路径与 inode；仅 Arca 私有文件和新建目标使用同目录临时文件与原子发布。
- MCP 与 model writer 只能修改各自拥有的字段。API key/token 只存系统 Keychain，不进入配置、日志、fixture、截图或仓库。ZCode 原生 Models 为用户明确授权的例外：仅在显式选择 plaintext delivery 时向其私有配置导出凭据，中央密钥仍存 Keychain；必须使用权限收紧、CAS 和加密回滚快照，禁止明文备份或日志。
- Skills 只保留 `~/.arca/assets/skills/items/` 中央副本并通过已核验用户级目录链接分配；不保留旧中央目录回退、迁移或兼容链接。生命周期写操作必须由 core 先 plan，再以原 operation id、候选哈希和风险确认 commit。
- 中央 desired state 与 Agent 投影必须分离提交：中央变更先持久化，每个 `Agent × capability × physical target` 再独立收敛。跨 target 不得回滚已经成功的目标；失败只形成与最小物理 write set 绑定的 incident，并允许无关 Agent、无关能力和无关偏好继续写入。单个 target 内仍必须 fail closed、CAS、备份，并按上一条的目标类型选择原地改写或原子发布。MCP/Skills 每个 Agent 为 `0..N`；原生支持多模型的 Agent 可安装 `0..N` 个 Model Profile 且最多一个为当前模型，单模型 Agent 仍为 `0..1`。
- 测试必须隔离 `HOME`/`ARCA_HOME`，不得访问真实用户配置、Skills 或 Keychain。

## 产品与验证

- Desktop 的添加 Skills 入口采用直接导入：输入 GitHub 来源并提交，或选择文件夹/压缩包后，自动安装该来源的全部候选。该提交就是安装意图；绑定 Core 原计划的 findings hash，不再弹出安装风险确认。同名中央内容先备份再替换，保留 Core 的候选重验、路径校验、CAS 与恢复流程；Agent 分配仍独立执行。

- Agent 显示名使用核验过的实际 App 名称；CLI / 插件使用其正式产品名，不额外添加地区、版本或形态说明。同名产品保留独立 ID，用现有图标形态标记和类别区分；不得为消歧改名。名称权威在 `data/agents.json`（目录项在 `data/agent-catalog.json`），各能力页面与前端不得维护第二份名称表。

- 资产操作不得以内嵌确认面板替换页面列表。普通添加/移除直接执行，失败使用全局通知；必要确认统一通过 `DialogShell` / `Modal` 挂到 `document.body`，覆盖整个应用，禁止局部弹窗或重复弹窗外壳。
- 成功、失败、校验、恢复、启动同步和更新反馈统一使用 `ToastProvider` / `ToastNotice`：右上角通知条，从右向左进入，并位于所有 dialog 之上。禁止另建页面内提示框或底部横幅；持续进度使用稳定 noticeKey 更新同一条通知，普通操作反馈可打断后台进度。资源与请求自身的状态、操作计划和风险内容仍作为数据展示。
- 顶层为 `MCPs`、`Models`、`Skills` 中央资产库；Agent 页面统一显示已添加资产与中央选择器，多模型 Agent 还需区分“已添加 / 已启用 / 当前模型”，三类状态由 core 的 desired/observed inventory 提供。UI 保持不透明、克制，并覆盖 `1200x820` 与 `900x600`。
- 完成功能修改后先启动 `npm run tauri dev` 做本地开发预览，默认使用真实 `HOME` 与 `~/.arca`，从而验证同一套 MCP、Model、Skill 和 Agent 配置；不得用空白临时数据冒充本地启动。只有自动化测试继续隔离 `HOME`/`ARCA_HOME`，或用户明确要求隔离环境时才覆盖这些路径。
- 当前使用极速交付模式：除非用户在当前任务中明确要求，不运行 `cargo test`、`npm test`、fmt、clippy、图标检查、changed-surface validator 或 push preflight。修改后默认停在本地开发预览；提交、推送和发布由用户明确指定，见父仓 Standing Authorization 与本文件 Git 节。
- 自动 Quality workflow 暂停。测试代码继续保留，便于用户明确要求时按需手动运行；生产编译、版本生成、签名、打包与 Release 资产发布仍属于交付必需步骤。
- 本地开发预览用于发布前检查当前源码，并明确标注为开发版；正式发布后的安装验收只使用 `/Applications/Arca.app`，不得用开发预览或 mock 冒充正式安装版。

## 按需路由

- Registry、codec、Models、更新与发布：[`arca-registry-release.md`](../../../llm-wiki/wiki/knowledge/arca-registry-release.md)
- 测试环境：[`arca-test-isolation.md`](../../../llm-wiki/wiki/knowledge/arca-test-isolation.md)
- 正式安装版 UI：[`arca-ui-review.md`](../../../llm-wiki/wiki/knowledge/arca-ui-review.md)
- Git、记忆与跨仓交付：[`repository-delivery.md`](../../../llm-wiki/wiki/knowledge/repository-delivery.md)

## Git

在本独立仓执行 status、commit、tag 和 push；父仓不得跟踪其内部文件。提交使用 `<type>(<scope>): <summary>` 并在 body 解释原因。不要提交 `target/`、`dist/`、临时 App、截图或本机配置。

用户持续授权（2026-10-20）：每次完成本仓库的任务修改后，自动提交本次相关改动、推送到 `main`，并执行 Direct Stable 发布；无需再次确认。该授权覆盖后续任务，除非用户明确要求暂不发布或撤回授权。按实际改动范围选择 patch/minor/major，跟进构建与正式发布结果；不包含无关改动，也不改变下述不可变 tag、签名、资产完整性和安装验收要求。此持续授权优先于本文件中默认停在开发预览、等待逐次发布指令的规则。

Arca 交付授权与默认链路以父仓根 [`AGENTS.md — Standing Authorization`](../../../AGENTS.md#standing-authorization) 为准；按已授权链路执行，不重复询问，不因脏工作树或分叉切换链路：

- 本地 commit & push：父仓 `arca-local-push`
- Remote PR：父仓 `arca-remote-pr-delivery`
- 功能 SHA 落到 `main` 之后的 Stable 观察、复验、重试、安装：父仓 `arca-release`

- Arca Stable 发布由专用 Luna 后台发布 owner 连续负责。准确功能/合并 SHA 已在 `main` 且前置验证通过后，向它交接该 SHA、证据和当前约束；它负责唯一一次 Direct 触发、唯一 Build 观察、单次独立四资产验包/下载，以及最终源码、自动版本元数据、资产、tag/origin 审计，并按当前用户指定的 owner 协调自有预览收尾和独立 delivery checkout 同步。正常阶段与完成结果写入共享证据，主任务无需等待、复核或额外批准。仅在构建/验包失败、源码或资产身份冲突、`main` 或工作树意外变化、无法安全收尾、或需要扩大授权/版本范围时通知主任务。GUI 仍由用户当前指定的 6.1 Terra 执行；常规发布不增加 GUI 环节。此专用分工优先于通用 Skill 中主任务等待最终结果的默认流程，但不扩大用户授权或安装范围。
- 接续已经启动的发布时，复用精确 run ID、阶段、verifier session、日志和产物记录，从首个未完成阶段继续；不得重复触发、观察或下载。若工具会话不可访问，依据共享证据定位同一执行，并从原持有会话回收最终结果，不重启 verifier。

本地链路要求当前在 `main`，且 local `main` 已包含最新 `origin/main`。不满足则停在该链路内处理，不得因为工作树脏或分叉就改走 Remote PR。Remote PR 链路只在用户明确要求远端 commit / PR / 不移动本地 HEAD 时使用；脏或分叉只约束「不要改用户 checkout」。

- 永久使用 Direct Stable：只有用户明确授权发布后，才从 `main` 显式派发 `direct-stable-release.yml` 并选择 `release_type=patch|minor|major`。普通 commit / push 不触发发布；workflow 只处理仍为当前 main head 的普通提交，由所选范围生成 release metadata、Draft 和不可变 Stable tag，拒绝从自动 release commit 再次升版。
- 自动 Quality 暂停。Direct Stable 在 tag/Draft 落地后从 `main` 显式派发唯一一次 macOS build，使发布构建连续复用 default-branch Rust cache。发布仍必须完成版本、签名、App/DMG、Updater、CLI、完整资产集合和 latest 语义版本顺序检查。
- 用户明确要求暂不发布时，本地链路停止 push。交付授权不覆盖无关改动或 Stable tag 人工操作；本机安装授权按父仓根 `AGENTS.md` 判断。
- 功能提交不直接修改 `version.txt`、`CHANGELOG.md` 或 lockfile 版本；这些字段由自动 release commit 统一更新。npm lockfile 只能由 `release-version.mjs` 在无项目 `node_modules` 的临时目录更新；portable dependency closure 失败时不能绕过或手工补 JSON。
- 版本按一次已授权发布所包含的改动范围选择，而不是按每个 commit 自动递增 patch：兼容的修复与细节优化用 patch，兼容的新能力或成组功能用 minor，破坏兼容的接口/配置契约变更用 major。先说明版本选择的理由，再通过 Direct Stable 的显式 `release_type` 执行发布。
- 不手工创建、移动或覆盖 Stable tag，不直接发布 Draft，不以 `--clobber` 修复正式资产。发布缺陷使用新的 main commit 生成下一 patch。
- 发布凭据与 Tauri 签名材料只存在于 GitHub Secrets，不进入日志、fixture、文档或仓库。
