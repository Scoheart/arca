<img src="desktop/src-tauri/icons/icon.png" width="104" align="right" alt="Arca icon" />

# Arca — Agent Asset Manager

**Configure MCP servers, reusable model endpoints, and user-level Agent Skills once, then let each Agent consume those central assets.**

Arca is a central asset and Agent configuration manager for Claude Code, Claude
Desktop, Codex, Cursor, QoderWork, OpenCode CLI/Desktop, and many other AI agents. MCPs, Model Profiles, and
Skills are created and maintained in their top-level libraries; each Agent then
selects which compatible assets it should consume. Arca adapts that desired state
to the Agent's native format while preserving unrelated settings.

Model credentials managed by Arca stay in macOS Keychain. Agents without a safe
Keychain command receive only a non-secret environment-variable reference; Arca
never edits shell startup files. Primary model selection is managed independently
from fork, small, auxiliary, and other secondary model slots. Skills are downloaded
from GitHub or imported from local folders and archives directly into one managed copy. Arca
still validates paths, archive structure, hashes, and concurrent changes in the
background. Assigning that copy to verified Agent directories remains a
separate operation, so an Agent page never asks you to reinstall the same Skill.

In the desktop Agent page, use the editor menu beside the theme and language controls to choose
VS Code, Cursor, Zed, Sublime Text, TextEdit, or another installed macOS app.
Arca remembers the choice for configuration files across Agents and app restarts.
Folder links still use Finder. Selecting **系统默认** restores the system file association;
if a selected editor cannot launch, Arca reports the error without silently switching apps.

To launch an Agent on macOS, use **▶ 运行** in the upper-right corner of its
workspace, or right-click
a pinned Agent icon. Desktop applications open through the system launcher; CLI
Agents open in Terminal and ask for a working directory on first use. Arca remembers
that directory per Agent. The run dropdown lets CLI Agents use another directory;
**安装文档** is a separate toolbar button. Configure the application, executable
with arguments, or website through **编辑配置**. Extension entries name the host editor they open. Missing
built-in runtimes offer their official installation page. On the first CLI launch,
macOS may ask for permission to let Arca control Terminal.
Installed Agent pages show a detected version beside the launch controls. Arca
reads macOS app bundle metadata or checks an audited default CLI with a bounded
`--version` call; custom CLI commands are not executed for version detection.
`arca agent launch show <agent-id>` exposes the same version in the CLI.

Agent details use one workspace with a centered identity, compact launch and
credential controls, and a single MCPs / Models / Skills tab bar. **配置** expands
the selected capability's paths and documentation inline. MCP resources use compact
rows with controls on the right. Skill cards separate the title, three-line
description, and footer so shared-state badges and controls stay readable. Their
**…** menu exposes details and removal when supported. Narrow windows reduce the
number of columns.

**编辑配置** includes application / CLI / website launch settings alongside the
MCP, Model and Skills paths. Changing only the launcher saves directly; path edits
retain the impact review. Cancelling that review leaves launch preferences
unchanged. Configurable Agents keep launch settings in this editor instead of
duplicating the settings entry in the run dropdown.

Arca ships as **two front-ends that share the same data** (`~/.arca/`):

- 🖥️ a **macOS desktop app** (Tauri + React) — a visual manager, and
- ⌨️ a **CLI + TUI** (`arca`, a native Rust binary) — an interactive terminal UI
  plus scriptable subcommands.

Both frontends enter through the same revisioned workspace, capability graph,
startup recovery, and plan/commit/cancel application boundary. External MCP,
Model, and Skill changes use the same explicit per-item contract: `arca discover`
is read-only, while each domain's `converge` command adopts, restores, or detaches
one exact observation. Desktop and CLI share central asset lifecycles, Agent
launch configuration, default terminals, and credential rules. The no-argument
TUI remains an MCP-focused terminal workspace.

---


## Upgrade from MUX

Arca is the new name of MUX. Install **Arca.app** and use **`arca`** for new
installations. Quit the previous app before upgrading or opening Arca.

On first successful startup, Core recovers pending writes in the existing
`~/.mux` directory, moves that physical directory to `~/.arca`, and retains
`~/.mux` as a compatibility symlink. Existing Skills, links, settings and backups
remain available. If both directories already contain independent data, startup
refuses to merge them; set `ARCA_HOME` to the intended real directory. An explicit
`ARCA_HOME` takes priority; the former `MUX_HOME` override remains accepted and
its chosen directory is not moved automatically.

Keychain service identifiers and native Agent ownership IDs remain stable.
The rename does not copy secrets, rewrite Agent documents, or claim their
external settings. Legacy `mux-store` metadata is accepted, while new API
responses use `arca-store`.

Existing desktop updaters keep the same signature trust key and follow the
GitHub repository redirect. A legacy CLI upgrade archive remains solely for
older standalone clients; the executable it contains is Arca. The installer is
named Arca.app; an in-place update of an old MUX.app may retain that old folder
name, so use the Arca installer to replace the application name in Applications.
Do not run the old and new applications side by side against the same assets.

The documentation continues at the existing hostname until a separate domain
change is deployed. Old guide routes redirect to the Arca guide.

## Central sources and external observations

Arca doesn't bundle a fixed server list. Its central MCP catalog is assembled from sources you control, while Agent files remain an independent observed-state input:

| Source | What it is |
|--------|------------|
| **订阅 (Subscribe)** | A **URL** to an MCP config file. Arca fetches + caches it; refresh re-pulls upstream. |
| **本地 (Local)** | A config file **imported from disk** — copied into Arca; refresh re-reads the original. |
| **手动添加 (Manual)** | Servers you create by hand or **paste** in — stored as a managed local source. |
| **外部发现 (External)** | Servers already present in Agent files, scanned as read-only observed state. Arca detects them automatically and offers explicit per-item management. |

A one-click **Arca 精选 (curated collection)** subscribes you to a curated source. Managed sources can be toggled on/off; the Registry shows their effective union, while external Agent observations remain read-only until the user reviews and manages one exact item. Arca never bulk-adopts detected Agent configurations.

## Features

- **Aggregated catalog** with search, source filtering, and an explicit view of copies shadowed by precedence.
- **Central assets, explicit consumers** — create or import assets once, then manage each Agent's desired MCP/Skill/Model sets from that Agent's page. Asset Inspectors keep lifecycle and impact read-only.
- **Transport-aware** — `stdio` / `http` / `sse`, plus a **custom `type`** (e.g. `streamable-http`). Same-named stdio and http variants are tracked separately.
- **Paste a config** — drop a `{"mcpServers": {…}}` block and Arca recognizes the servers and adds them.
- **Desired vs. observed state** — Agent files and Skill links are scanned for `synced`, `external-added`, `external-changed`, `external-removed`, `unparseable`, `ambiguous`, and `unsupported`; scans never silently create ownership.
- **Normal external change tracking** — newly added, removed, toggled, or edited MCPs, Models, and Skills refresh in place. Every changed relationship exposes only the safe core-projected actions: adopt observed state, restore Arca desired state, or detach ownership.
- **Reviewed propagation** — editing or deleting a central MCP or Model plans the central change together with every consumer. Ordinary central edits never overwrite drift; an exact convergence operation must resolve the affected relationship first.
- **Safe, local writes** — Arca reads and edits only fields it owns. Existing files are backed up, prepared, and verified as one recoverable transaction; unrelated keys, comments, formatting, policy fields, permissions, and symlinks are preserved.
- **Unified Agent consumption center** — each Agent page shows only desired central assets under MCPs, Model, and Skills, with a central picker for relationship changes and a separate read-only external section.
- **Reusable model connections (preview)** — define one Provider Base URL, shared credential, and an editable Endpoint Path for every enabled protocol; Models then reference that connection with only their model ID and optional token limits. Native multi-model Agents can keep several Profiles installed, enable or disable each one, and choose exactly one current primary model; Claude, Claude Code, and Codex are managed single-Profile Model Agents. Claude accepts only Anthropic Messages-compatible Providers. Applying selects or creates one Arca-owned third-party inference Profile; the operation review discloses the selected Provider key export and that the key is written only to Claude's private `0600` Profile, and non-Claude route names automatically disable Claude model verification. Restart Claude after apply or clear. This is Model configuration, separate from Claude MCP support and Connectors.
- **User-level Skills in Desktop** — enter a public GitHub source and press Enter, or select a local folder / archive, to import all discovered Skills directly without a second install confirmation. Same-name central copies are backed up before replacement. No Git, Node.js, or `npx` is needed; Agent assignment remains a separate step.
- **One proxy for Arca networking** — configure HTTP, SOCKS4/SOCKS4A, or SOCKS5 once for GitHub Skills, remote sources, CLI updates, and signed Desktop update checks; credentials are never stored in `settings.json`.
- **CLI ⇄ Desktop on one application core** — both use the same bootstrap, Agent capability graph, revisioned MCP/Model/Skill snapshot, typed errors, and recoverable operation coordinator.
- **Dark mode** and a compact, consistent resource workspace for MCPs, Models, and Skills, with shared cards, right-side Inspectors, and review dialogs only for consequential existing-asset changes.

## Screenshots

[Watch the historical desktop walkthrough (before the rename)](https://mux.scoheart.com/en/guide/demo) — Models, MCPs, Skills, Provider editing, and the Agent picker, with Chinese / English captions.

![Arca central Models library](website/public/media/arca-1.10.0-models.jpg)

![Arca Skills source navigation and cards](website/public/media/arca-1.10.0-skills.jpg)

![Arca editable Provider portal and protocol paths](website/public/media/arca-1.10.0-provider.jpg)

See the [desktop app guide](website/guide/desktop.md) for the current workspace and source navigation. Demo assets are public examples and contain no live credentials.

## Supported agents

Arca keeps audited Agent contracts in `data/agents.json` and discovery-only records in `data/agent-catalog.json`. Only verified user-level MCP paths and native schemas are writable; other entries expose verified Skills or native setup guidance. Arca never guesses a path or writes a generic schema into discovery-only records.

Audited targets include Claude Code/Desktop, Codex, Cursor, VS Code, Zed, Windsurf, Gemini CLI, Google Antigravity, Amazon Q, OpenCode, Grok Build, MiniMax Code, Copilot CLI, Cline, Cline Desktop, Freebuff, Continue, Goose, Hermes, Kimi Code CLI/Desktop, Qwen Code, Qoder, Qoder Desktop, Qoder CLI, QoderWork, QwenWork (international and CN), Step Code, Mistral Vibe, Rovo Dev, Tabnine, LM Studio, and others. For MCP, Claude and BoltAI local files accept stdio only. Pi 0.99.0+ reads `~/.pi/agent/mcp.json` natively; older Pi builds need an MCP extension. Devin, Cline Desktop, and Freebuff remain audited read-only records because no stable user-level global config file is documented.

Managed Model Profile configuration is available
for Claude, Claude Code, Codex, Grok Build, Pi, OpenCode, Kilo Code CLI, Qwen Code,
Crush, Mistral Vibe, Hermes Agent, Factory Droid, Goose, ZCode, Qoder Desktop 0.1.8, and Qoder CLI 1.1.50+.
Qoder Desktop uses native custom providers in `~/.qoder/settings.json`; select the
installed model in a Qoder conversation after restarting the app. Qoder CLI shares
that registry and supports a global current model, with environment credential
references. MiniMax Code, Qoder, and Kimi Code CLI/Desktop remain guided targets.

Models use a shared Provider architecture: one Provider owns its name, type,
single Base URL, enabled protocol Endpoint Paths, environment reference, and
single Keychain credential, while any number of Models reference it by stable
`providerId`. Editing a Provider reapplies every affected Agent transactionally.
Arca refuses to disable a protocol while a named Model still uses it. Existing
same-origin protocol endpoints migrate to Base URL + Endpoint Path without
changing request targets; a legacy Provider with multiple origins is left
untouched and must be split explicitly. If an Agent's native schema can only
represent a client Base URL, Arca also blocks custom paths it cannot preserve
exactly instead of silently ignoring them.

See the [complete audited matrix](website/guide/agents.md) and [catalog methodology](docs/agent-catalog.md). Every writable target's global path remains editable; paths inside the home directory are normalized to the portable `~/…` form.

Skill consumption supports the audited user-level capabilities declared in `data/agents.json`, across CLI, IDE, and desktop products. `arca agent list --json` exposes the current capability matrix. Only capabilities detected on the current machine appear, and Agents sharing one physical compatibility directory are selected and reviewed as an inseparable impact group. Managed links expose one live central copy, so consumer-side edits are detected as central drift rather than isolated copies. See the [Skills guide](website/guide/skills.md).

---

## Desktop app

Grab the **Desktop installer · Apple Silicon** asset from the latest stable [**Release**](../../releases/latest). The app checks that stable channel automatically and also exposes a manual **Check for updates** action. Installing the app makes its bundled `arca` CLI available through `~/.local/bin/arca` when that directory is on `PATH`.

Arca uses a direct Stable flow started by an explicit `workflow_dispatch` request on the current ordinary `main` commit. Select `patch` for fixes, `minor` for compatible new capabilities, or `major` for breaking changes. Ordinary commits and pushes do not publish a release. Direct Stable creates the release metadata commit, Draft, and immutable version tag, then dispatches exactly one macOS Stable build from the resulting `main` commit; automated Quality is currently paused and is run manually when requested. Stable publication still requires release provenance, version consistency, signing, App/DMG inspection, updater and CLI packaging, complete-asset validation, and semantic-version latest-channel ordering.

Build from source:

```bash
cd desktop
npm ci
npm run tauri build      # or: npm run tauri dev
```

## CLI

The `arca` CLI is a native Rust binary built on the same `arca-core` as the desktop app. It is bundled with the desktop app, can be downloaded separately from Releases, or built from source:

```bash
cargo install --path cli    # installs the `arca` binary onto your PATH
# or just build it:
cargo build --release -p arca-cli   # → target/release/arca
```

Everything runs against `~/.arca/`, shared with the desktop app.

Run `arca` with **no arguments** for the **interactive TUI** — an MCP-focused,
keyboard-driven terminal workspace with three screens (Registry /
Sources / Agents). Browse and search the MCP catalog, install to Agents, enable,
disable, or delete entries, and manage MCP sources and Agent targets. Press `?`
for the keymap and `q` to quit. Set `ARCA_NO_TUI=1` to print help instead when a
script invokes `arca` without arguments.

Or drive it non-interactively with subcommands:

```text
arca mcp {list,show,status,assign,unassign,enable,disable,enable-all,disable-all,converge,add,save,delete,export,source,icon}
arca mcp icon {list,set,import,reset}
arca mcp source {list,subscribe,add-local,add-builtin,refresh,enable,disable,remove}
arca model {list,show,status,save,delete,import,assign,unassign,enable,disable,converge,use,delivery,provider,curl}
arca model provider {list,show,templates,models,save,delete,docs}
arca skill {list,show,status,inspect-source,install,import,update,remove,repair,check-updates,assign,unassign,enable,disable,converge}
arca agent {list,save,configure,enable,disable,run,launch}
arca agent launch {show,configure,reset}
arca settings {show,terminal,locale,pins}
arca network proxy {show,set,clear}
arca discover [mcp|model|skill]
arca status --agent <id> [--agent <id>] [--capability mcp|model|skill]
arca operation {apply,review} --file request.json
arca trace {list,show}
arca capture {environment,run,state,list,show}
arca workspace
arca upgrade
```

All Agent-relationship writes use exact stable asset IDs and exactly one explicit
`--agent <id>`. `assign` adds only the named relationships; `unassign` removes
only those relationships; `enable` and `disable` preserve the relationship.
`arca mcp unassign --all --agent <id>` performs a reviewed,
target-scoped clear of managed, disabled, and external MCP entries while
leaving the central catalog and every other Agent unchanged.
`arca model unassign --all --agent <id>` uses the same Model clear operation as
Desktop, including external Models on native-registry Agents. Core protects
shared physical stores and retains central Profiles, Providers and credentials.
`arca model use <profile-id> --agent <id>` selects the current Model independently
of assignment. `converge <asset-id> --agent <id> <adopt|restore|detach>` is the
shared explicit reconciliation verb; repeating `assign`, `enable`, or `use`
never repairs or overwrites external changes as a side effect.
Skill plans report every installed Agent affected by a shared
physical Skills directory. MCP IDs always include transport, such as
`github::stdio`; a same-named HTTP variant is a separate asset and is never
selected implicitly. Every convergence plan binds the inventory revision and
re-scans before commit, so a changed observation returns `observation_stale`.

The global `--json`, `--yes`, `--dry-run`, and `--no-color` options support
machine output, reviewed automation, plan-only runs, and deterministic terminal
output. Any command that writes—including `arca mcp export --out`—requires
interactive confirmation or an explicit `--yes` / `--dry-run`. See the
[complete CLI guide](website/guide/cli.md). High-risk Skill plans additionally require
`--accept-risk`; ordinary `--yes` does not grant a risk override. A partially
converged commit exits with `pending_convergence`, retaining the operation ID,
`changed: true` and the affected target incidents for recovery.

Central Model, Provider, MCP and Agent edits accept typed JSON documents via
`--file` (or `--file -` for stdin). Provider credentials can be supplied through
`--credential-stdin` and are stored by Core in Keychain; they never enter argv
or ordinary JSON output. Skill source inspection and installation support the
same GitHub, local directory and archive resolver as Desktop.

Model schema upgrades migrate central Arca data and credentials without rewriting
the Agent's current files. Those files are scanned afterward as ordinary observed
state. A capability-local migration or parser failure isolates that capability;
only damaged shared settings or an incomplete transaction that cannot be safely
recovered make the whole workspace read-only.

---

## Data layout

Everything lives under `~/.arca/`:

```
~/.arca/
├── settings.json           # agents · consumption relationships · UI/network state
├── assets/
│   ├── mcps/
│   │   ├── catalog.json    # MCP registry and source definitions
│   │   └── sources/{remote,local}/  # cached and managed source payloads
│   ├── models/catalog.json # Provider and Model Profile assets
│   └── skills/
│       ├── catalog.json    # managed Skill metadata
│       └── items/<name>/   # one managed central copy per Skill
├── skills -> assets/skills/items  # compatibility alias for links made by older releases
├── staging/skills/         # resolved Skill candidates and internal Skill operations
├── staging/consumption/    # reviewed cross-domain plans and durable rollback snapshots
├── backups/                # timestamped backups made before managed writes
│   └── skills/             # reversible Skill replacements, imports, and removals
└── journals/skills/        # crash-recovery progress for committed Skill operations
```

Skills-specific runtime paths:

```text
~/.arca/assets/skills/items/     managed Skill contents
~/.arca/staging/skills/          staged candidates and internal operation plans
~/.arca/backups/skills/          reversible replacements/removals
~/.arca/journals/skills/         crash recovery journals
```

Model API keys are not stored under `~/.arca/`; they remain in macOS Keychain.

The three resource types share one typed state contract in Core and one central
asset root, while retaining domain-specific catalogs and payload formats. Asset
operations persist semantic revisions only for
the exact central assets, their reverse consumer sets, Agent relationships,
target graph, and credential presence they reviewed. Unrelated UI, network,
Agent, or other state outside those semantic subjects does not invalidate that
review. MCP source precedence, the single managed Skill tree, and Keychain
credentials remain in macOS Keychain.

## How it works

1. **Build the central libraries** — subscribe or import MCP sources, create Model Profiles, and directly download or import Skills. No Agent target is changed during central intake.
2. **Choose consumers** — from an Agent page or the CLI, select compatible assets. MCPs and Skills are sets; supported multi-model Agents also keep a Model Profile set plus one independent current pointer.
3. **Review only meaningful risk** — routine reversible Agent relationship changes sync directly with inline progress; conflicts, removals from shared Skill directories, and disruptive Model changes still show their exact impact before commit.
4. **Commit and verify** — settings, Agent targets, and central lifecycle changes are applied as a recoverable transaction and return their verified inventory before reporting success.
5. **Converge external state explicitly, one item at a time** — Arca detects unmanaged and externally changed MCPs, Model Profiles, and user-level Skills without changing ownership. Adopt, restore, or detach one exact revision-bound observation through a recoverable transaction.
6. **Propagate central lifecycle changes** — updates reach every desired consumer; deletion clears all managed targets and relationships instead of leaving implicit orphan copies.

Skills in this version are user-level only. Project-level Skills, private repositories, and Skill editing are not supported. The CLI can query and manage Agent consumption for MCPs, Models, and Skills, and explicitly converge one detected change at a time. CLI commands also cover central Model/Provider/MCP authoring, Skill lifecycle, MCP sources, Agent configuration and proxy settings. Desktop retains file dialogs, icon/locale/pinning preferences, launch integration and credential reveal/validation UI; the no-argument TUI is MCP-focused. See the [source-backed parity audit](analysis/11-cli-desktop-parity-and-bug-audit-2026-09-12.md) for remaining differences.

## Development

A Cargo workspace plus the Tauri desktop app:

```
core/           # arca-core — domain contracts, application facade, resource engines, transactions
  src/domain/       # IO-free Agent/resource/relationship/error contracts
  src/application/  # bootstrap, workspace snapshot, capability graph, plan/commit/cancel
  src/resources/    # MCP/Model/Skill-specific engines (MCP codecs no longer occupy root)
  src/assets/       # cross-domain desired/observed relationships and coordinator
cli/            # arca-cli  — the clap-based `arca` binary, built on arca-core
desktop/        # Tauri v2 (Rust, depends on arca-core) + React 19 + Vite + Tailwind v4
data/           # audited agent definitions + discovery catalog + curated MCP collection
```

The desktop app is a separate build (`exclude`d from the workspace) so its Tauri bundle output path stays put.

```bash
cargo test                            # arca-core + arca-cli
cd desktop/src-tauri && cargo test    # Rust core + integration tests (desktop)
cd desktop && npm run build           # desktop frontend (tsc + vite)
node scripts/update-agent-catalog.mjs # refresh the public client discovery catalog
```

## License

[MIT](LICENSE) © Scoheart

Agent and Provider coverage, supported endpoints, and remaining integration gaps: [September 2026 review](docs/agent-provider-research-2026-09-07.md).

The Agent page’s title and icon open its documentation home (maintained in
`data/agent-docs-home.json`). Capability-specific documentation remains separate.

In configuration cards, click the MCPs, Models, or Skills title/icon to open
that capability’s documentation. File and folder paths remain separate local
open actions; a title without a documentation URL is not clickable.

Agent pages include an installation status and an official download or installation
guide link. Detection checks declared executable/application probes, not configuration
directories; unavailable probes show an unknown state. Refocus Arca after installation to refresh detection. Arca opens the vendor page and does not run installers.

CodeBuddy CLI (`codebuddy-code`), CodeBuddy, and the two WorkBuddy editions
are separate Agents. **WorkBuddy AI** (`workbuddy`) uses
`~/.workbuddy-ai/mcp.json` and `~/.workbuddy-ai/skills`, launching `WorkBuddy AI.app`.
**WorkBuddy** (`workbuddy-cn`) uses `~/.workbuddy/mcp.json` and
`~/.workbuddy/skills`, launching `WorkBuddy.app`. Both support stdio, HTTP, SSE,
native MCP pause, and central Skill links. Their resources are assigned independently;
existing overseas assignments stay intact. Models use each edition's native guide.
CodeBuddy remains guided-only. See [WorkBuddy integration](docs/workbuddy.md).

ZCode 自定义模型接入与凭据交付说明见 [ZCode Models](docs/zcode-models.md)。

默认打开方式：顶部「设置」集中管理文件编辑器、Agent CLI 默认终端、外观、语言、网络及更新。默认终端支持已安装的 Terminal、iTerm2、Ghostty（1.3+）和 Warp，设置对所有 CLI Agent 生效。

Kimi Code is a separate Agent (`kimi-code-desktop`) with its official app icon and launcher. It shares user-level MCP and Skills with Kimi Code CLI; Models currently open the native setup guide because provider credentials are literal config values. See [integration evidence and boundaries](docs/kimi-code-desktop.md).

Kilo Desktop is available as `Kilo` (`kilo-desktop`) with its official app icon, launcher, installed version, MCPs, Skills, and custom Models. Its desktop runtime config is separate from the CLI's global config; Skills share `~/.kilo/skills`. Models use environment/file credential references and are selected inside each Kilo conversation. See [Kilo Desktop integration](docs/kilo-desktop.md).

ChatGPT (`codex-desktop`) opens ChatGPT.app and shares `~/.codex/config.toml` plus `~/.agents/skills` with Codex CLI. The model writer stays on Codex CLI. Agent types are CLI, Desktop, IDE, and Plugin; the launch form follows that type and does not switch among them.

Agent labels use the actual application name, or the official CLI/plugin product name. Region, version, and surface are not added to the name. Existing IDs and bindings stay stable; see the [name audit](docs/agent-name-audit.md).


### Agent Trace（本地会话）

顶部「Trace」或 Agent 详情页的「Trace」按钮打开只读会话工作台。它与「网络 → Agent 抓包」不同：直接读取已有会话文件，不拦截流量、启动 Agent 或修改原始日志。

自动发现 Pi、Codex CLI / ChatGPT、Claude Code 和 Gemini CLI 的默认用户级会话目录。左侧按 Agent、项目和路径查找会话，中间按页查看用户、助理、工具与其他事件，右侧按需读取完整正文、原始调用 JSON 和按调用 ID 配对的返回 JSON。复制与导出当前记录均保留原始结构及元数据，仅常见凭据脱敏。

其他 Agent 可选择其导出的 JSON / JSONL 文件导入；导入只登记本次 Arca 进程内的只读文件引用，不复制到中央资产库。无法识别的格式显示通用原始事件，不猜测角色或工具配对。搜索只覆盖已加载记录的摘要，不是全文 / 全历史搜索。

系统与内部推理条目不展示。Pi 时间线包含历史分支，原始 ID / parentId 保留。JSONL 单条记录上限 8 MiB，按 60 条原始记录分页；JSON / Gemini 文档上限 32 MiB。文件变化、无效 JSON、超限和未配对返回明确提示，不静默截断。来源路径和适配器权威在 `data/trace-sources.json` 与 Rust Core。更多边界见 [Agent Trace](docs/agent-traces.md)。

### Agent 抓包（macOS）

顶部「网络 → Agent 抓包」打开独立工作台；Agent 详情页的「抓包」按钮会预选当前 Agent。
先启动 Agent，选择实际运行的应用或独立 CLI 进程，再指定直连、Arca 已配置的代理或任意无认证 HTTP 上游地址。抓包出口独立于 Arca 自身联网设置，指定代理后失败不回退直连。

依赖本机官方 mitmproxy（`mitmdump`）。在系统设置「通用 → 登录项与扩展」启用 Mitmproxy Redirector 网络扩展，客户端还需信任 `~/.mitmproxy/mitmproxy-ca-cert.pem`。Arca 只检查系统信任，不自动安装证书或批准系统权限。「追加 CA 启动」给新进程传入临时公开证书集合，保留 TLS 校验，不保存启动偏好或凭据。设置会区分进程证书、Node / 浏览器混合网络栈和系统信任：Cursor、Qoder 的额外 CA 只覆盖采用它的 Node 请求；Codex 桌面端关闭了 Electron 的 NodeOptions fuse，无法依靠 `NODE_EXTRA_CA_CERTS`。系统 CA 未信任时，系统型客户端的启动和抓包均拒绝执行，避免中断现有 HTTPS 请求。浏览器网络栈、证书固定和已经打开的单实例应用仍需分别验证。

按实际 launcher 和可执行文件识别 Cursor、Grok Build、Qoder IDE/Desktop、Codex CLI/Desktop、Pi、Qoder CLI、OpenCode CLI/Desktop、QwenWork、QwenWorkCN 与 QoderWork。桌面应用可选择整个应用或独立顶层 PID 实例；CLI 实例也使用 PID 与启动时间绑定，跟踪其后代。Node/Bun 按脚本路径、已核验的包入口或 Pi 的进程标题识别，不会捕获所有 Node 进程。适配记录和「可启动」不代表该产品已经完成真实请求验证；未安装的客户端没有可选择的进程。

OpenCode Desktop 的本地模型服务继承公开 CA 集合，辅助进程随应用纳入记录；1.19.0 官方 GUI 的本机 HTTPS / gzip SSE 链路已验证。QoderWork 0.9.18 的 Node 网络探针已验证，GUI 模型请求仍待账号条件具备后检查。两者按混合网络栈处理，浏览器请求仍受系统信任条件限制。完整的逐客户端证据和未验证项见 [抓包验收记录](analysis/16-agent-capture-workbench.md)。

工作台提供 Structure 主机/路径树、Sequence 时序表和可调整的面板。Overview / Request / Response / Timing 显示状态、协议、Headers、Query、文本 Body、首字节耗时与流量大小。请求发出即记录，SSE 的完整事件会在结束前增量更新，选中详情自动刷新；支持分片 UTF-8、gzip、deflate 和多行 data。WebSocket 请求保持流式状态，Request / Response 分别展示客户端 / 服务端消息、序号和时间，完整文本分片重组后脱敏；最多保存 512 条 / 2 MiB 文本，二进制消息只记录长度与省略标记，原始传输不修改。复制与 JSON 导出只包含已脱敏内容。记录保存于 `~/.arca/captures/`（或 `ARCA_HOME/captures/`），目录 0700、文件 0600。常见认证头和令牌字段脱敏后才落盘；这不是任意正文的完整隐私审计。二进制、Multipart、无法安全解析的 JSON 和不支持的压缩正文不保存；正文最多 2 MiB，会话最多 5000 条 / 100 MiB。

只保证已实际验证的 HTTP / HTTPS 路径；不支持认证上游或 SOCKS。代理出口下 DNS 通过同一代理访问 Cloudflare DoH；解析失败不回退直连 DNS。无法通过 HTTP 上游的连接会失败。每两秒刷新进程范围，长 SSE 不再阻塞新进程纳入；短生命周期子进程和切换间隙仍不能算作全量覆盖。停止会话、目标退出或 Arca 退出会结束它启动的抓包器，并收尾未完成记录，不停止已有代理服务。


网络扩展可能按系统顺序先接管连接。若已有 ProxyBridge 等目标应用代理规则，请在抓包期间只暂停目标规则并重建目标连接，完成后恢复原规则；Arca 不自动修改其他工具的规则。普通回环 HTTPS 不解密，避免破坏内部页面和 IPC；中央 Provider 明确配置的回环 HTTPS 模型端点会纳入解密范围，仍需客户端信任与引擎实际覆盖。Agent 自己配置的代理/网关（包括继承的 HTTP_PROXY 等环境变量）可能让最终请求由其他进程发出，必须核对来源进程，不能据此宣称全量覆盖。

CLI 使用同一 core：`arca --json capture environment` 查询适配与实际进程；`arca --json capture run --agent pi --target "<target-id>" --seconds 60` 在前台限时记录；`capture state <session-id>` 查询已核验的实时状态；`capture list` / `capture show <session-id> --flow <flow-id>` 查看保存的脱敏记录。Desktop 可以查看同一中央目录中 CLI 发起的实时会话并请求停止。多实例共享抓包锁，所有者与引擎都以 PID 和启动时间核验；前台抓包依赖命令进程存活，不创建后台守护服务。引擎保留锁直到退出，异常退出或容量限制导致的未完成请求在观察视图中显示为已中断。
