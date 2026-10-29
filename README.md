<img src="desktop/src-tauri/icons/icon.png" width="104" align="right" alt="Arca icon" />

# Arca — Agent Asset Manager

**Configure MCP servers, reusable model endpoints, and user-level Agent Skills once, then let each Agent consume those central assets.**

Arca is a central asset and Agent configuration manager for Claude Code, Claude
Desktop, Codex, Cursor, QoderWork, OpenCode CLI/Desktop, and many other AI agents. MCPs, Model Profiles, and
Skills are created and maintained in their top-level libraries; each Agent then
selects which compatible assets it should consume. Arca adapts that desired state
to the Agent's native format while preserving unrelated settings.

The Agent picker also includes [Web clients and MCP development tools](docs/mcp-apps-clients.md),
including LibreChat, Open WebUI, ChatGPT, Claude.ai, Postman, MCPJam Inspector,
and MCP Inspector by mcp-use. Web entries provide official configuration guides;
Postman and MCPJam also offer native macOS launch entries. CopilotKit, AG-UI,
and mcp-use are labeled as development frameworks. These reference entries do
not imply local asset assignment or MCP Apps rendering inside Arca.

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
**安装文档** is a separate toolbar button that opens the official installation
steps for that Agent’s CLI or extension, or its desktop download page. Configure the application, executable
with arguments through **编辑配置**. Extension entries name the host editor they open. Missing
built-in runtimes offer their official installation page. On the first CLI launch,
macOS may ask for permission to let Arca control Terminal.
CLI launch commands remain visible and copyable, with the working directory,
PATH, environment variables and executable on separate continuation lines.
Ordinary environment values and URLs omit unnecessary quotes; paths and
arguments retain literal quoting, and long argument lists wrap between arguments.
Arca keeps the exact executable path it detected and does not clear terminal
history or change your shell configuration.
Installed Agent pages show a detected version beside the launch controls. Arca
reads macOS app bundle metadata or checks an audited default CLI with a bounded
`--version` call; custom CLI commands are not executed for version detection.
`arca agent launch show <agent-id>` exposes the same version in the CLI.

Agent details center the Agent identity and its launch, Trace, capture, and edit
actions above the workspace. A left sidebar lists Models, MCPs, Skills and, when
supported, Instructions; asset counts stay beside each resource. Use Up/Down,
Home or End to navigate with the keyboard. The right pane holds that resource's
actions, search, cards or native Instructions editor. MCP resources use compact
rows; Skill cards separate the title, description and controls. Their **…** menu
exposes details and removal when supported. Narrow windows keep the sidebar and
reduce the number of content columns.

Click **运行** to open launch settings. **编辑配置** holds the MCP, Model,
Skills and Instructions paths; path edits retain their impact review.

In the launch dialog, use **新增** to copy the current launch configuration into
a named profile. Each Agent can keep up to 32 profiles with separate arguments,
environment variables and launch options. Switching profiles keeps every draft;
**保存** saves them together and uses the selected profile for future launches.
A vertical list switches profiles. The dialog keeps a fixed size, with scrolling
inside the selected profile. Saving keeps the dialog open; running closes it
after a successful launch. Arguments and environment variables use compact
previews with separate multiline editing.
A fixed, empty **默认配置** always remains available and cannot be edited, renamed or deleted.
Existing custom launch settings are retained in a separate editable profile.

For Codex CLI, **编辑配置 → 编辑配置文件** opens an in-app editor with common
options and full TOML source. **原生配置** edits `$CODEX_HOME/config.toml`
(`~/.codex/config.toml` by default). **Arca 专属配置** edits the separate
`$CODEX_HOME/arca.config.toml`; files are created only when explicitly saved.
Comments, unknown fields and line endings are retained. Existing files are
updated in place, with revision checks and a private Keychain rollback snapshot.

In an editable Codex launch profile, select **Arca 专属配置** under **配置来源**
to add `--profile arca` to that profile's arguments. These overrides apply to
that launch and do not rewrite the native file. Unset options still inherit the
native configuration; project and explicit CLI overrides retain Codex's normal
precedence. Authentication and session storage are shared. A profile-specific
`CODEX_HOME` environment variable also determines which files this editor opens.

Arca manages six model request protocols: OpenAI Chat Completions, OpenAI
Responses, Anthropic Messages, Gemini Interactions, Gemini generateContent,
and AWS Bedrock Converse. Provider creation, model profiles and cURL export
use the same protocol definitions in the CLI and desktop app.

**Copy cURL** and `arca model curl <profile-id>` produce one `curl` command with
headers and an editable JSON body, including the protocol's native system
instruction example. Line breaks keep the command readable. Arca omits unset
top-level fields while generating the command; no script wrapper, temporary
variable, preprocessing or `jq` is needed. Filled defaults, `false`, `0`, empty
collections and nested values are retained. Token limits of `1024` are editable
Arca examples, not service defaults. The complete field catalog, official
references and compatibility notes remain in [the protocol reference](docs/model-curl-templates.md).

New Google AI Studio connections default to Interactions and also enable
generateContent. Interactions cURL examples use `store=false`. Amazon Bedrock
Converse has a separate regional `bedrock-runtime` connection using an official
Bedrock Bearer API key; enter the model or inference-profile ID manually.
AWS IAM access-key/secret-key and role administration are outside this connection
workflow. Bedrock Mantle remains its separate OpenAI Responses connection.

Agent assignment depends on its verified native adapter. OpenCode and Kilo
adapters support Converse through the Amazon Bedrock SDK. Arca can manage and
export Interactions profiles, but the current Agent writers cannot yet select
the Interactions factory and therefore reject assignment of those profiles.

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

Keychain labels and permission prompts use `Arca · Model Provider · <id>` (or
`Model Profile` for legacy items). Existing items keep their service lookup key
so Agent credential commands continue working. After a successful save,
Core attempts a metadata-only name update without another authorization
dialog; trusted apps and access rules are preserved. An already-open prompt
still shows its original name, and a denied metadata update leaves it unchanged.

Desktop opens one application credential session at startup, shared by all
Arca-managed Providers. Its Keychain-only aggregate is labelled
`Arca · Credentials`; unlike the individual compatibility items, this vault
trusts only the creating application, not `/usr/bin/security`. After the first
additive import (which may require authorizing the existing items), ordinary
Provider reads share the unlocked vault until Arca completely quits or eight
hours elapse. Closing the macOS window merely hides Arca and keeps this session.
Sleep time counts toward the limit; both wall-clock and process timers are checked.
No API keys are written to files or browser storage. Transaction rollback items
remain separate and are not retained in the application session.

Each read checks the original item's metadata revision without a password
dialog. External replacement requires the global **Unlock credentials** action;
external deletion immediately invalidates that cached key. Cancelling an unlock
is shared across pages until explicit retry. Existing Agent helper commands
continue to read their original items and follow those items' access controls.
Completing an old source item's write-permission migration can still require a
system confirmation when explicitly saving or deleting that key; reopening the
aggregate cannot grant a different item's write permissions. Ordinary navigation
never triggers this source-item recovery path.
If an aggregate refresh cannot be saved, the source key remains authoritative
and the app reports that the next startup may need authorization again.

Existing desktop updaters keep the same signature trust key and follow the
canonical [Scoheart/arca](https://github.com/Scoheart/arca) repository. GitHub
redirects former repository URLs so older clients can still reach releases.
A legacy CLI upgrade archive remains solely for
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
- **Unified Agent consumption center** — each Agent page shows desired central assets under MCPs, Model, and Skills, with a central picker for relationship changes and a separate read-only external section.
- **Native plugins and bundled MCPs** — Cursor's Plugins tab lists verified bundled providers, local plugin packages and completed marketplace caches, including versions, source paths and MCP/Skill/Rule/Agent/Command/Hook components. Host-provided MCPs also appear as read-only cards in the Agent's MCPs tab. Cached packages retain an explicit cache-only label; Arca does not infer installation, enablement or connection from files. Native observations have no delete, toggle or asset assignment actions. CLI `mcp status --agent cursor` exposes the same metadata in text and JSON.

Native resource lists use compact rows. Open a row to view its IDs, versions, observed state and component sources; declarations sharing a source are grouped in the details dialog. These observations preserve each host's native plugin format and built-in declarations. They do not certify package or client conformance to the Agent Plugins specification.
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

The stable build uses a shared Cargo target directory for CLI and Desktop, with frontend and CLI preparation in parallel. After the Draft and tag exist, independently verify the published artifacts without waiting for the build job's cache upload:

```bash
bash .github/scripts/verify-stable-release.sh --source-root "$PWD" --wait vX.Y.Z
```

The verifier downloads the five expected assets with bounded parallelism using
the current command environment's proxy settings. It reuses completed assets
only when repository, tag, asset ID, size and SHA-256 match, and limits stalled
downloads. Its cache defaults to `~/Library/Caches/Arca/release-assets`; use
`--cache-dir` to choose another location. Every run still checks SHA-256 digests,
updater payload and trusted-comment signatures, both the updater App and the
mounted DMG, the standalone CLI and the existing upgrade bridge. Verification
does not install or launch the app.

Build from source:

```bash
cd desktop
npm ci
npm run tauri build      # or: npm run tauri dev
```

`npm run tauri dev` starts **Arca Dev** with its own application identifier and
purple DEV icon. It shares your Arca data, while the installed **Arca** keeps its
existing Dock entry. The Run script stops only this checkout's development
processes and leaves the installed application running.

## CLI

The `arca` CLI is a native Rust binary built on the same `arca-core` as the desktop app. It is bundled with the desktop app, can be downloaded separately from Releases, or built from source:

```bash
cargo install --path cli    # installs the `arca` binary onto your PATH
# or just build it:
cargo build --release -p arca-cli   # → target/release/arca
```

Everything runs against `~/.arca/`, shared with the desktop app.

### Teach an Agent to use the CLI

The portable [arca-cli Skill](skills/arca-cli/SKILL.md) teaches Agents to discover
the installed CLI, configure launch paths/arguments/environment variables, manage
MCPs, Models and Skills, and verify changes through structured output. It includes
command recipes and guidance for shared targets, credentials and partial failures.

From a local checkout, install it into Arca's central library, then assign it to
an Agent (replace `codex` with the exact ID from `arca agent list --json`):

```bash
arca skill inspect-source --path ./skills/arca-cli --json
arca skill install --path ./skills/arca-cli --name arca-cli --dry-run --json
arca skill install --path ./skills/arca-cli --name arca-cli --yes --json
arca skill assign arca-cli --agent codex --dry-run --json
arca skill assign arca-cli --agent codex --yes --json
arca skill status --agent codex --json
```

Review any reported risk findings before explicitly adding `--accept-risk`; a
plain `--yes` does not accept those findings. Desktop users can import the
`skills/arca-cli` folder through **Skills → Add**, then select it for their Agent.
The Skill and its `references/` directory must stay together when using another
Agent's supported Skill installation method. Start a new Agent session or use
its supported reload mechanism after assignment.

### Interactive and scripted commands

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
arca capture {prepare,environment,run,state,list,show}
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

Skill registration and assignment retire only their own operation records. The
shared journal directory stays in place, so an empty directory or old cleanup
residue does not turn a completed operation into a recovery failure. Startup
still checks and recovers unfinished writes before further Skill mutations.

Operation feedback uses one notification bar at the top right, sliding in from
the right. Success, errors, validation, recovery, startup sync and software updates
share this surface. Repeated messages are deduplicated; background progress
updates one notice while other operation results remain visible above it.

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

`tauri dev` rebuilds the desktop app, not the standalone CLI. If your `arca`
command points to a debug binary, rebuild it with
`cargo build -p arca-cli --target-dir desktop/src-tauri/target` and check
`arca --version` before writing live configuration. A stale CLI can discard
newer launch-profile fields when it saves shared settings.

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

右下角的形态角标用于区分共用品牌图标的不同产品，共 **5 种**：

| 形态 | 符号 | 颜色 | 示例 |
| --- | --- | --- | --- |
| 命令行 | 终端提示符 `>_` | 石板灰 | Cursor CLI、Codex CLI |
| 桌面应用 | 显示器 | 蓝色 | ChatGPT、Claude Desktop |
| IDE 编辑器 | 代码括号 | 紫色 | Cursor、Qoder |
| IDE 插件 | 插头 | 琥珀色 | Codex IDE、Cline |
| Web 应用 | 地球 | 青绿色 | ChatGPT 网页版、Claude 网页版 |

Cursor 与 Cursor CLI 共用 Cursor 品牌图标，分别显示 IDE 与命令行角标。固定栏、Agent 选择卡片、详情页和资源消费者共用同一个角标组件；尺寸随品牌图标调整，边框在浅色和深色背景下保持可见，悬停显示完整名称与形态。独有品牌图标、同形态的多个发行版和未知自定义 Agent 不会被自动添加形态角标。完整分类、显示条件与组件职责见 [Agent 形态角标](docs/agent-surface-badges.md)。


### 原生全局 Instructions / Rules

Agent 详情页的 **Instructions** 展示已确认的本机全局 Prompt / Rules 文件，可直接查看原文、预览并编辑保存。保留原路径、规则条件、格式和优先级；不建立中央指令库，不提供文件删除、规则集合增删、分配或自动补写。

在 **Agent → 编辑 → 配置文件** 中可输入或选择 Instructions 目录，并恢复原生默认路径。目录偏好独立保存；只改变 Arca 读取和编辑的位置，不创建、迁移或删除原文件，也不修改 Agent 的原生加载设置。

只有完整检查确认没有已有原生入口时，才提供主动新建一个文件的入口；普通保存不会把外部已删除的文件重新创建。保存校验外部变化并保留草稿，已有文件保留 inode、BOM 和未编辑行的换行。同一路径的多个 Agent 读取者会标明共用影响。保存成功不代表当前模型请求已经加载，刷新方式仍遵循原生客户端。

未完整确认格式、初始化或自定义后端的入口保持只读/引导。详情见 [原生指令文件](docs/instructions.md)。

### Agent Trace（本地会话）

顶部「Trace」或 Agent 详情页的「Trace」按钮打开只读会话工作台。它与「网络 → Agent 抓包」不同：直接读取已有会话文件，不拦截流量、启动 Agent 或修改原始日志。

自动发现 Pi、Codex CLI / ChatGPT、Claude Code 和 Gemini CLI 的默认用户级会话目录。左侧按 Agent、项目和路径查找会话，右侧以对话形态阅读：用户靠右、Agent 靠左，Markdown 正文直接展示，工具调用展开查看完整输入与按原生 ID 配对的返回。原始 JSON 在独立查看器中按需读取。复制正文与复制 / 导出原始记录分别提供，原始记录保留结构及元数据，仅常见凭据脱敏。

其他 Agent 可选择其导出的 JSON / JSONL 文件导入；导入只登记本次 Arca 进程内的只读文件引用，不复制到中央资产库。无法识别的格式显示通用原始事件，不猜测角色或工具配对。搜索覆盖当前已加载的正文、摘要及调用 ID，不扫描未加载历史或工具的完整原始数据。

系统与内部推理条目不展示。Pi 时间线包含历史分支，原始 ID / parentId 保留。JSONL 单条记录上限 8 MiB，按 60 条原始记录分页；JSON / Gemini 文档上限 32 MiB。文件变化、无效 JSON、超限和未配对返回明确提示，不静默截断。来源路径和适配器权威在 `data/trace-sources.json` 与 Rust Core。更多边界见 [Agent Trace](docs/agent-traces.md)。

### Agent 抓包（macOS）

从 Agent 详情页点击「抓包」，再点击「开始记录」即可使用默认方案；进入页面不再自动弹出配置表。进程与历史会话只展示当前 Agent。「记录选项」用于选择已有实例、工作目录或自定义出口。
所有 Agent 共用 Core 的网络上下文：依次检查 Arca 代理、Agent 启动环境、Arca 进程环境、终端登录环境、已声明的原生网络配置和系统代理；全部未配置时才直连。记录选项会在开始前显示解析出的上游地址及来源；遇到读取失败、不可用或不支持的代理会明确提示，不自动绕过。仍可手动选择直连、Arca 代理或无认证 HTTP 上游。原生配置的位置、格式及字段由 `data/agent-network-sources.json` 声明；共享解析器支持 JSON、TOML 和 YAML 来源，扩展客户端时复用相同的来源接口、优先级、校验和错误处理。

首次缺少引擎时，按钮显示「下载并开始」。Arca 从 mitmproxy 官方地址下载固定的 12.2.1 包，验证固定 SHA-256 与 macOS 代码签名，原子安装到 `~/.arca/captures/runtime/`。无需预装 Homebrew、Python 或修改 shell 配置；已有私有组件优先复用，也可使用已有 `mitmdump`。下载或校验失败不会发布半成品，也不会关闭 TLS 校验。

Codex CLI、Pi 的「启动并记录」自动使用本机临时代理：引擎先就绪并分配空闲端口，再带代理与额外公开 CA 启动 Agent，无需网络扩展或系统证书授权，也能接到启动阶段的请求。策略由 `data/capture-agent-profiles.json` 控制，其他客户端不会被未经验证地切换到此方式。只有经过该代理的请求可见；绕过代理的 SDK、原始 TCP/UDP 不在此路径覆盖范围内。

临时抓包端口与网络出口是两层：Agent → Arca 临时端口 → 已配置的上游代理 → 模型服务，地址和端口均从上下文取得。需要代理访问模型时，在记录选项确认上游地址，或选择「自定义 HTTP 代理」；将客户端入口直接指向上游会跳过记录。尚未保存到配置或启动环境、仅在另一个终端窗口临时设置的变量，需要在记录选项显式指定。共享网络层可解析 HTTP/SOCKS 来源；当前抓包传输只接受 HTTP 上游，遇到其他协议会明确拒绝。

macOS 上，`⌘W` 和红色关闭按钮收起 Arca 主窗口，保留页面状态、后台任务与抓包；点击 Dock 图标重新打开。`⌘H` 隐藏应用，`⌘Q` 才退出并执行抓包收尾。

桌面客户端和已有进程仍使用按进程的系统网络扩展。首次开始时由官方引擎安装并注册 Mitmproxy Redirector；通知条提供「打开授权设置」，批准后自动继续，最多等待 3 分钟。普通引擎启动上限为 12 秒。Arca 不代替用户批准系统权限。启动失败和引擎意外退出会清理本次引擎的进程组并释放抓包锁；不会通过删除锁文件或结束 Agent 进程绕过占用。

HTTPS 记录涉及 `~/.mitmproxy/mitmproxy-ca-cert.pem`。「启动并记录」给支持的运行时传入临时公开证书集合，保留 TLS 校验，不保存启动偏好或凭据。需要系统信任的客户端提供「打开证书设置」：Arca 自动生成证书并打开钥匙串访问，用户完成一次 SSL 信任授权后返回，环境状态自动刷新。Arca 不静默信任根证书。Cursor、Qoder 的额外 CA 只覆盖采用它的 Node 请求；Codex 桌面端关闭了 Electron 的 NodeOptions fuse，无法依靠 `NODE_EXTRA_CA_CERTS`。系统 CA 未信任时，系统型客户端仍拒绝开始，避免中断现有 HTTPS 请求。浏览器网络栈、证书固定和已经打开的单实例应用仍需分别验证。

按实际 launcher 和可执行文件识别 Cursor、Grok Build、Qoder IDE/Desktop、Codex CLI/Desktop、Pi、Qoder CLI、OpenCode CLI/Desktop、QwenWork、QwenWorkCN 与 QoderWork。桌面应用可选择整个应用或独立顶层 PID 实例；CLI 实例也使用 PID 与启动时间绑定，跟踪其后代。Node/Bun 按脚本路径、已核验的包入口或 Pi 的进程标题识别，不会捕获所有 Node 进程。适配记录和「可启动」不代表该产品已经完成真实请求验证；未安装的客户端没有可选择的进程。

OpenCode Desktop 的本地模型服务继承公开 CA 集合，辅助进程随应用纳入记录；1.19.0 官方 GUI 的本机 HTTPS / gzip SSE 链路已验证。QoderWork 0.9.18 的 Node 网络探针已验证，GUI 模型请求仍待账号条件具备后检查。两者按混合网络栈处理，浏览器请求仍受系统信任条件限制。完整的逐客户端证据和未验证项见 [抓包验收记录](analysis/16-agent-capture-workbench.md)。

工作台提供 Structure 主机/路径树、Sequence 时序表和可调整的面板。Overview / Request / Response / Timing 显示状态、协议、Headers、Query、文本 Body、首字节耗时与流量大小。请求发出即记录，SSE 的完整事件会在结束前增量更新，选中详情自动刷新；支持分片 UTF-8、gzip、deflate 和多行 data。WebSocket 请求保持流式状态，Request / Response 分别展示客户端 / 服务端消息、序号和时间，完整文本分片重组后脱敏；最多保存 512 条 / 2 MiB 文本，二进制消息只记录长度与省略标记，原始传输不修改。复制与 JSON 导出只包含已脱敏内容。记录保存于 `~/.arca/captures/`（或 `ARCA_HOME/captures/`），目录 0700、文件 0600。常见认证头和令牌字段脱敏后才落盘；这不是任意正文的完整隐私审计。二进制、Multipart、无法安全解析的 JSON 和不支持的压缩正文不保存；正文最多 2 MiB，会话最多 5000 条 / 100 MiB。

只保证已实际验证的 HTTP / HTTPS 路径；不支持认证上游、SOCKS 或自动代理脚本。网络扩展路径的 DNS 通过同一代理访问 Cloudflare DoH；解析失败不回退直连 DNS。无法通过 HTTP 上游的连接会失败。每两秒刷新进程范围，长 SSE 不再阻塞新进程纳入；短生命周期子进程和切换间隙仍不能算作全量覆盖。

停止记录时收尾未完成记录并释放记录锁。系统网络扩展路径会退出引擎。由临时代理启动的 CLI 则保留仅转发、不保存请求的辅助进程，避免 Agent 断网；Arca 或 CLI 记录命令退出时也会停止保存并继续转发，直到绑定的 Agent 实例退出。它只监听本机随机端口，不修改全局网络设置。现有流上的记录到停止时截止，后续请求不再进入会话文件。


网络扩展可能按系统顺序先接管连接。若已有 ProxyBridge 等目标应用代理规则，请在抓包期间只暂停目标规则并重建目标连接，完成后恢复原规则；Arca 不自动修改其他工具的规则。普通回环 HTTPS 不解密，避免破坏内部页面和 IPC；中央 Provider 明确配置的回环 HTTPS 模型端点会纳入解密范围，仍需客户端信任与引擎实际覆盖。Agent 自己配置的代理/网关（包括继承的 HTTP_PROXY 等环境变量）可能让最终请求由其他进程发出，必须核对来源进程，不能据此宣称全量覆盖。

CLI 使用同一 core：`arca --json capture prepare` 提前安装并验证私有组件；`capture environment` 查询适配与实际进程；`arca --json capture run --agent pi --launch --seconds 60` 自动准备并启动记录，默认出口为 `auto`；也可用 `--target "<target-id>"` 记录已有进程。`capture state <session-id>` 查询实时状态；`capture list` / `capture show <session-id> --flow <flow-id>` 查看脱敏记录。Desktop 可查看 CLI 发起的会话并请求停止。多实例共享记录锁，所有者与引擎以 PID 和启动时间核验；转发辅助进程不占用记录锁，不再写入请求。异常退出或容量限制导致的未完成请求显示为已中断。

Codex CLI's Plugins tab reads the native `CODEX_HOME/plugins/cache` installation layout (`~/.codex` by default), selecting `local` first and otherwise using Codex's semver/string version comparison. It shows installed Skill, MCP, Hook and App connector declarations, including Agent Plugins v1 manifests and Codex vendor extensions. Older cached versions are not presented as selected installations, and disk installation does not imply effective session activation. Plugin IDs and marketplace provenance remain visible; all native items are read-only.

Claude Code's Plugins tab follows `CLAUDE_CONFIG_DIR/plugins/installed_plugins.json` (default `~/.claude`), reading only unambiguous user-scope installations at their registered paths. Cached versions and project/local installations are excluded. It shows the manifest, installation and matching marketplace provenance, default/declared Skills, Commands, Agents and Hook events, scoped plugin MCPs, LSP and other declared component metadata. Default MCP, LSP and Hook files merge with declarations; command/agent/style/workflow/theme paths replace their defaults. Marketplace additions follow the documented manifest precedence. Native MCP IDs use `plugin:<manifest-name>:<server-key>`.

These are static declarations, not proof that this Claude Code version supports or has activated them. Experimental monitor/theme/eval, channel and type metadata may require a newer Claude Code than the locally installed version. MCP bundles are shown as unresolved declarations; Arca does not download, extract or execute them. Root `CLAUDE.md`, credentials, command bodies, hook handlers and user configuration values are not exposed. All plugin resources remain read-only.

Gemini CLI's Plugins tab reads immediate user installations under `~/.gemini/extensions` (or `$GEMINI_CLI_HOME/.gemini/extensions` when set), requiring `gemini-extension.json` with a native name and version. A `.gemini-extension-install.json` entry with `type: link` selects its registered source directory; Arca does not pick historical cache versions or follow arbitrary symlinks. It shows declared MCP keys, context files, Skill files at the native two search depths, immediate Agent Markdown files, recursive Command TOML files, immediate Policy TOML files, Hook event names, themes and Plan configuration presence. Policy files remain distinct from Markdown instructions; their rules/checkers and all component bodies remain host-owned.

All Gemini extension rows are static, read-only declarations. A command, Skill, Agent or Policy file is not proof that Gemini's native parser accepted or activated its contents. Workspace enablement, `--extensions`, trust/admin settings, hook gates and ordinary MCP configuration can override loading; effective state remains unknown. Malformed enablement, duplicate extension names, escaping paths and unavailable linked sources produce diagnostics. The observer never reads extension `.env` values, starts servers, runs hooks or rewrites native settings.

OpenCode's Plugins tab reads user-global JSON/JSONC plugin declarations (and the native legacy TOML `config` file without performing its migration) and immediate `plugin/` or `plugins/` JS/TS files. It respects `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `OPENCODE_CONFIG`, `OPENCODE_CONFIG_DIR` and `OPENCODE_PLUGIN_META_FILE`. Global config arrays follow the native replacement order; later sources deduplicate by npm package or resolved file. Inline environment configuration, project discovery, managed/remote account overlays and process flags remain outside this global disk view.

Configured npm declarations remain **declared** until disk evidence is available. Historical `plugin-meta.json` rows only join matching current specifiers; their verified package files are shown as **cache**, with version read from the physical package. Unreferenced history never becomes an installed or enabled plugin. Package entry/theme paths are inspected without importing code; dynamic tools, hooks, Skills and MCP contributions remain unknown. Tuple options, auth state, URL credentials and code bodies are not exported.

For the audited OpenCode 1.3.17 macOS executable at `~/.opencode/bin/opencode`, Arca verifies the executable format, exact embedded plugin module hash and version marker through one pinned file before showing its six built-in authentication integrations. A changed or unsupported artifact produces a diagnostic instead of guessed built-ins. These integrations are separate from MCPs and Skills. Native plugins and providers remain read-only, and none of these disk observations establish successful activation.

OpenCode Desktop has an independent observer for the signed 2.2.0 Electron package (`ai.opencode.desktop`). Its bundled Bun module table and complete loader/definition members are digest-bound separately from the GUI ASAR members. Arca inventories 90 server plugin declarations and 13 GUI definitions available on macOS; the reserved Windows-only WSL definition is excluded. The browser plugin's packaged Skill remains a native declaration. Model login integrations, configuration adapters and tools are shown as code components rather than fabricated MCP servers. Bundled availability does not identify a running background service, selected model, account, effective enablement or connected browser.

Desktop's global disk view uses `OPENCODE_CONFIG_DIR` in place of the XDG/default `~/.config/opencode` root, and an absolute `OPENCODE_CONFIG` as an explicit document. It reads `opencode.json` and `opencode.jsonc`, concatenating the supported legacy `plugin` entries and V2 `plugins` entries. Native selection operations (`-id`, `*`, `prefix.*` and `opencode.*` IDs) are never treated as npm packages or proof of activation. Immediate `plugin` / `plugins` JS/TS files and local directories remain separate physical sources; configured standalone files are rejected as in V2. Inert package metadata can name server, GUI, TUI and RPC entries; conditional exports remain unresolved. Registry-package caches use V2's own `XDG_CACHE_HOME/opencode/npm/<package>@<specifier>/<latest numeric generation>/node_modules` layout, with a manifest-name check and a **cache** label. No npm operation or plugin source runs. Dynamic plugin IDs, MCP/Tool/Skill registrations, GUI database state, managed/remote/project layers and inline configuration remain outside this read-only inventory. The existing standalone CLI observer and writable MCP/Model/Skill contracts are unchanged.

ChatGPT / Codex Desktop also reads the shared Codex installation directory and its macOS bundled marketplace under `/Applications/ChatGPT.app/Contents/Resources` or `~/Applications/ChatGPT.app/Contents/Resources`. `CODEX_ELECTRON_BUNDLED_PLUGINS_RESOURCES_PATH` selects an explicit resources directory. The native order checks `plugins/openai-bundled` before `app.asar.unpacked/plugins/openai-bundled`; the index must list confined local packages with matching `.codex-plugin/plugin.json` names and versions.

Packaged and installed snapshots retain their own presence, version, paths and components, grouped under the same plugin identity when appropriate. Bundled availability is separate from installation. Feature flags, materialization variants, remote hosts, managed policy, browser/native permissions and live MCP overrides can all change runtime use; the disk view keeps effective activation unknown. Arca does not reconcile installations, rewrite bundled MCP definitions, read desktop account state or expose native-resource deletion.

ChatGPT Desktop's session constructors are also observed separately: `artifact_session`, `node_repl` and `cua_repl` are **built-in declarations**, verified against the actual app identity and complete packaged JavaScript member digests. They can be supplied at thread creation without appearing in the plugin manifests or saved user configuration. The packaged `cua_repl` declaration and its desktop constructor retain their separate sources. Feature gates, runtime dependencies, viewer availability, account/browser/computer permissions, host context and per-thread selection still determine actual use; enabled and connected state remain unknown. No constructor, service or target executable is invoked, and command/environment values are excluded. This desktop evidence is not projected onto Codex CLI, the IDE extension or ChatGPT's web account.

Goose Desktop observes its five default bundled catalog entries with distinct component types: Developer is a platform tool extension, while Auto Visualiser, Computer Controller, Memory and Tutorial have native in-process MCP factories. App identity, complete Electron member digests and the complete signed backend digest bind those MCP declarations to the audited package. Bundled availability leaves effective enablement and connections unknown; Arca never runs Goose's catalog initializer, discovery, auto-update or extension factories.

Goose CLI is observed independently from the Desktop package. Arca binds twelve compiled platform extensions and the two embedded `goose-doc-guide` / `web-search` Skills to the complete audited standalone executable; its four native MCP factories remain separate MCP observations. Platform tools do not become MCP servers, native hidden/default flags do not establish effective enablement, and embedded Skills may be shadowed by user resources at runtime. The shared Core command search selects one launcher, resolves only that alias and verifies its complete digest. Changed or unsupported executables are reported as incomplete observations, without falling back to another installation or the Desktop backend. Version labels use the same passive identity check rather than executing `goose --version`; launch candidates and the selected physical file are watched for installation or replacement.

Goose's user-global plugin directory is `~/.agents/plugins`, or `<GOOSE_PATH_ROOT>/.agents/plugins` for an absolute native override. Relative overrides are ignored. Immediate directories are local candidates; a recognized `.goose-plugin-install.json` record supplies an installed label without exporting its source URL. Manifest precedence is `.goose-plugin/plugin.json`, `.plugin/plugin.json`, then `plugin.json`. Skills include root and immediate child markers in the default `skills/` and explicit component paths, with a root `SKILL.md` fallback when no default directory or explicit selection exists. MCP metadata follows Goose's stdio grammar: default `.mcp.json` plus custom `paths`/`exclusive` files or a raw inline server map. Native extension names use the **folder name** followed by `:<server key>`; runtime IDs apply Goose's own normalization: remove whitespace, lowercase ASCII letters, preserve digits/underscores/hyphens, and replace other characters with underscores. Conflicting sources and normalized-key collisions retain their own paths without claiming a runtime winner. Hook/Agent/Command markers do not establish loaded runtime components, and imported Gemini packages follow the same current component loader. Only confined metadata is read; Markdown bodies, scripts, commands, environment values, credentials, private account/configuration state and project scopes are excluded. All observations remain read-only. The shared metadata reader serves the independently identified CLI and Desktop adapters; existing writable contracts are unchanged.

Mistral Vibe observes the audited wheel-backed CLI installation through its selected launcher and venv. Arca passively binds the generated `vibe` / `vibe-app-server` scripts, Python package metadata and loader modules, packaged Rust client and native harness; it never imports Python, starts the client/backend or falls back to `vibe --version`. A changed package, unsupported launcher/layout, script-local module or Python search override leaves the association incomplete. The audited bundled `vibe` plugin has its own version and four Skill markers: `create-plugin`, `skill-creator`, `vibe` and `worktree`. Added local files cannot become additional audited bundled MCPs or Skills. Markdown bodies are not read.

Vibe's user-global packages are immediate directories under `~/.vibe/plugins`, or `VIBE_HOME/plugins` for an absolute or tilde-expanded override. Relative overrides are incomplete; project roots, runtime data and account/configuration files are excluded. Exact Agent Plugins 1.0.0 manifests take precedence, newer schema families do not fall back, and distinct foreign formats remain ambiguous. Native Skills use immediate `skills/*/SKILL.md` markers; stdio and streamable HTTP MCP declarations use `mcp.json` with Vibe's digest-size-8 BLAKE2s private aliases. Native SSE is unsupported. A valid `ai.mistral.vibe` extension gates Hook, Knowledge, Agent, library and connector declarations, retaining their authored types. Strict native manifest recovery follows the audited Vibe implementation; this observation does not certify complete Agent Plugins client conformance.

Vibe's Codex, Claude, Kimi and OpenCode compatibility readers keep their different implicit/declared paths and MCP shapes. Command markers remain authored Commands, and unsupported executable/interface declarations remain references. Disabled or OAuth-dependent OpenCode translations do not become host MCPs; duplicate Codex MCP IDs and conflicting global package identities do not manufacture winners. Marker presence does not validate component bodies, establish current session membership or prove activation. All observations are read-only and leave effective enablement unknown.

Kimi Code CLI observes the selected audited `@moonshot-ai/kimi-code` npm distribution without running Node, `kimi --version`, install hooks or plugin code. Full `dist/main.mjs` and `package.json` digests bind the loader and version; a changed package, unsupported wrapper or Node preload/search override leaves runtime association incomplete. The shipped bundle contains nine compiled-in Skills (`mcp-config`, `import-from-cc-codex`, `update-config`, `custom-theme`, `write-goal`, `check-kimi-code-docs`, `sub-skill`, `sub-skill.review`, `sub-skill.consolidate`). Their actual storage path is `dist/main.mjs`; product/experiment flags, TUI scope and current session visibility remain unknown. This catalog does not introduce any builtin MCP server.

Kimi Code Desktop has a separate native-resource identity and an independently audited 1.2.0 package contract. Bundle identity/version and six packed ASAR entry/loader members bind its nine shipped Skill markers to `app.asar!out/screenshot-CT9AOv7C.cjs`. A changed or unsupported selected app cannot borrow the CLI package or another installed app's identity. The metadata gate covers these packed members; seven unpacked native companions in the inspected official package differ from their ASAR-declared digests, despite a valid signed/notarized bundle, and their execution/integrity is outside this observer's claim. Product flags, TUI scope, session visibility and effective enablement remain unknown. See [Desktop evidence and limitations](docs/kimi-code-desktop.md).

Kimi's global `plugins/installed.json` registry under `~/.kimi-code` selects each exact registered package root. An absolute `KIMI_CODE_HOME` override replaces that home; empty, relative or tilde overrides are incomplete and never fall back into a project or the default home. Arbitrary cache directories do not become installations. The root `kimi.plugin.json` takes precedence over `.kimi-plugin/plugin.json`, including when the root manifest is broken. This is Kimi's own format; generic Agent Plugins manifests and `.mcp.json` are not implicit sources. Inline stdio, HTTP and SSE MCP declarations retain native `plugin-<registered-id>:<trimmed-server-name>` identities; unknown or ambiguous declarations stay references. Registration flags do not establish effective enablement or connectivity.

CLI and Desktop share this global registry grammar while retaining independent package gates and host observations. Without a matching package contract, registered roots remain declared references. Plugin metadata is read-only; it adds no delete, enable, import or assignment action and does not change either host's existing MCP/Skill writers.

Amp independently observes its audited native CLI distribution without executing Bun, `amp version`, plugin code or initializers. The complete binary digest binds the compiled JavaScript, bytecode and native companion bytes. Three shipped internal plugin factories (`permissions`, `app-store-connect`, `thread-visibility`) appear as Code markers at the actual embedded Bun member. Authentication, configuration, feature gates and effective activation remain unknown; these markers introduce no MCP server or Skill. See [Amp native evidence and boundaries](docs/amp.md).

Machine-local Amp plugin entries come from `$XDG_CONFIG_HOME/amp/plugins` or `~/.config/amp/plugins`. Top-level `.ts`/`.js` files exclude `.test.ts`/`.test.js`; same-stem root files mask directories, whose `index.ts` precedes `index.js`. Arca reads entry metadata only, preserving unsafe or unverified entries as references. Private plugin bodies are not read. Tools, commands, events and Skills registered by executable plugin code remain unobserved; a `skills/` directory does not establish registration.

Amp's versioned personal/workspace package directories beneath `$XDG_CACHE_HOME/amp/global-plugins` or `~/.cache/amp/global-plugins` remain cache-only references, across server/scope/version snapshots. Cache presence does not identify the selected account, server response, format or entrypoint. Empty configuration overrides fall back natively; empty cache overrides are explicit and unsupported as relative paths. Relative/tilde overrides do not fall back to a project or another home. Candidate IDs identify filesystem snapshots, without claiming executable runtime aliases. This is Amp's native module API, not Agent Plugins manifest conformance. Project discovery, account APIs and existing MCP/Skill writers stay outside this read-only observer.

Cursor CLI has an independent source contract and passive version probe; it never inherits Cursor Desktop's SQLite activation state or probes the colliding bare `agent` command. The inspected `2026.10.22-27dcf5f` Node package exposes read-only metadata under the actual user-global `~/.cursor/plugins/cache` and `~/.cursor/plugins/local` roots. General `CURSOR_CONFIG_DIR` / `XDG_CONFIG_HOME` settings do not relocate those plugin roots. See [Cursor CLI native evidence and limits](docs/cursor-cli.md).

All complete cache snapshots retain their physical market, identity and version. Cached MCP names remain references because an authenticated backend response selects their source identities and versions; local declared MCP IDs use the actual entry folder. Both wrapped and direct JSONC MCP maps are recognized without returning command, argument, environment, header or auth values. Script/Markdown bodies, account policy, project settings and runtime registrations remain unobserved. Unsafe entries or incomplete caches remain references and never gain write actions.

The guarded CLI package also contains a bundled `computer-use` MCP declaration. Its native config factory can download and install a Helper, so Arca only reports the compiled declaration and never calls the factory or materializes the Helper. Feature flags, helper installation, authorization and effective activation are unknown. The inspected `2025.09.17-25b418f` package can supply a passive version label, while its native plugin contract stays unverified; new-package plugin facts are not attributed to it. Source guards cover the declared package inputs, not the whole runtime closure. No existing MCP/Skill writer or central asset lifecycle changes.

Explicit Skill directories, root-only `SKILL.md` fallback, Agent Markdown, explicit Commands, inline Hook events and context paths retain their authored types. Deeper Skills requiring private frontmatter remain conditional references; component bodies and runtime parsing are unobserved. Duplicate registered identities or normalized MCP-key collisions never manufacture a winner. Unverified runtimes retain declaration-only registry references. All observation uses bounded, confined metadata reads; Markdown bodies, prompts, commands, argument/environment/header values, account/configuration files, credentials and project scopes are excluded from the output. Native resources remain read-only and never enter the central asset lifecycle automatically.

Codex IDE's Plugins tab reads the same user-global Codex installation directory (`CODEX_HOME/plugins/cache` or `~/.codex/plugins/cache`). This editor surface keeps its own Agent identity and does not inherit ChatGPT Desktop's packaged marketplace. The installed Cursor and VS Code extension artifacts resolve that shared global home, but editor/runtime versions, remote or WSL contexts, feature gates and session selection remain separate; visible disk installations do not prove support or activation in the current editor session.

GitHub Copilot CLI's Plugins tab reads only `installedPlugins` from the native global `config.json` under `COPILOT_HOME` (default `~/.copilot`), using each exact registered package path. It separately reads JSONC `settings.json` plugin declarations. Missing packages stay declared with a diagnostic; arbitrary marketplace cache folders never become installations. URL-based declaration specifiers are represented by opaque identifiers without exporting source URLs or credentials.

The observer recognizes legacy manifests and root Agent Plugins v1.0.0/v1.1.0 manifests, rejecting unsupported Agent Plugins versions. It lists static Skill, Agent, Command, Rule, Hook, MCP, LSP and extension-directory metadata without reading Markdown/script bodies. Standard Skills use immediate `skills/` children; standard MCP metadata requires the matching schema URL, closed envelope and supported transport fields. Copilot-specific standard components use `com.github.copilot/`. Conflicting legacy default configuration files retain their individual declaration paths; Arca does not infer the native winning definition or list agent-scoped MCP frontmatter as a shared server.

Materialized Copilot distribution caches are inspected separately at the native platform/XDG cache roots and `COPILOT_PKG_CACHE_HOME`, `COPILOT_CACHE_HOME`, `COPILOT_HOME` roots. Only completed versions with the required runtime files and matching `@github/copilot` package metadata contribute the packaged `browser-use` and `computer-use` declarations, labeled **cache**. Explicit `COPILOT_CLI_DIST_DIR` / `COPILOT_CLI_RESOLVED_DIST_DIR` packages are shown as bundled availability. Versions are never ranked as active. Authentication fields are excluded from the projection, and MCP secret stores are not opened. Arca does not unpack or execute the Copilot binary, install plugins, resolve trust or feature gates, or expose native-resource mutations. Global records and declarations do not establish session activation.

Qwen Code's Plugins tab reads immediate user installations under `~/.qwen/extensions` (or `$QWEN_HOME/extensions` in the current native format). Registered `.qwen-extension-install.json` links select their exact package source. Native `qwen-extension.json` packages expose declared MCP names, context file paths, immediate Skill and Agent files, recursive Markdown Commands, Hook events, immediate Workflow JS files and Channel entry metadata. Commands retain the native colon-separated names. Explicit workflow paths replace the default; inline hooks precede a registered hook file or `hooks/hooks.json`. Only package-local substitutions are supported, with no workspace or arbitrary environment expansion.

Root Agent Plugins v1.0.0 manifests have native precedence and expose only portable immediate Skills and matching-schema MCP declarations; their vendor fields, Commands, Agents, Hooks, Workflows and Channels do not become Qwen components. Unsupported Agent Plugins versions never fall back to the Qwen manifest. The current native `extension-store/state.json` contributes declaration-only or missing-package records without invoking initialization, migration, recovery or activation changes; workspace policies and effective enablement remain unknown.

Arca also resolves the existing official `qwen` launcher through the shared runtime discovery code, checks its physical `@qwen-code/qwen-code` package identity/bin/version, and groups immediate packaged `bundled/*/SKILL.md` files as **Qwen Code bundled Skills**. This is a packaged resource group with the host package version. It does not install or import Qwen, read Skill/Agent/Command/Workflow/Channel code bodies, expose `.env`/account/settings values, or claim that Qwen's parser accepted or activated a component. All native resources remain read-only.

Kiro IDE's Plugins tab inventories only the native global `~/.kiro/powers/installed.json` records and their exact `installed/<name>/` packages (`KIRO_POWERS_HOME` selects the native IDE registry root when set). It does not enumerate the catalog, clone repositories, follow installation-source fields or rank cached versions. Missing packages remain declarations with a diagnostic. No installed registry is initialized or migrated. Registered package identity supplies the native MCP namespace `power-<registered-name>-<server-name>`.

As observed in shipped Kiro IDE 1.4.51, any present `plugin.json` suppresses legacy fallback. Supported Agent Plugins 1.x schema declarations expose immediate `skills/*/SKILL.md` file metadata, matching-version root `mcp.json` declarations and immediate `dev.kiro/steering/*.md` paths. Standard MCP fields follow the shipped transport, command, URL and header constraints. Legacy packages expose only `POWER.md` file presence, immediate `steering/*.md` paths and native IDE JSONC `mcp.json` names. Skill/Power/steering bodies, vendor executables, account settings and MCP configuration values are excluded. These observations establish registered package availability and component declarations; native parser acceptance, cloud/workspace precedence, session activation and MCP connections remain unknown. The Kiro CLI identity is audited separately; IDE observations do not establish older CLI support. Native cards have no mutations.

WorkBuddy AI's Plugins tab reads global records from `~/.workbuddy-ai/plugins/installed_plugins.json`, confined to their exact native `plugins/cache/<marketplace>/<name>/<version>/` snapshots. Managed records precede user records; the native update timestamp selects among records of the same scope, retaining the first record on ties. Missing or ambiguous snapshots remain declarations with a diagnostic. Unregistered cache versions, marketplace source repositories and project-scoped records do not become user installations.

The observer follows the shipped WorkBuddy AI 5.6.2 manifest order: root Agent Plugins v1.0.0, then `.codebuddy-plugin/plugin.json`, `.workbuddy-plugin/plugin.json` and `.claude-plugin/plugin.json`. A present invalid standard manifest never falls back to legacy. Portable packages expose immediate Skills and matching-schema root `mcp.json` declarations. Legacy packages expose immediate/default and explicitly declared Agent, Command, Skill and Rule paths, Hook events/file hints, LSP names, inline MCP names and native JSONC `.mcp.json` / `mcp/*.json` declarations. Empty Hook placeholders are absent. Multiple MCP declaration sources retain their paths without claiming a runtime winner. MCP-app packages without an explicit server declaration have an unresolved package marker rather than an invented MCP ID.

On macOS, the packaged `workbuddy-builtin` marketplace under `/Applications/WorkBuddy AI.app/Contents/Resources/app.asar.unpacked/resources/plugins/` contributes only its listed, contained local packages. Bundled and installed snapshots keep separate versions, components and presence under their plugin identity. File presence and metadata do not establish native parser acceptance, integrity verification, effective activation or MCP connections. Arca does not read Markdown/script bodies, account overlays, settings or credential stores, initialize native registries, or expose native-resource mutations. Connector-owned registrations and the separate coded IM/Claw plugin family are outside this adapter; WorkBuddy CN is a separate Agent identity.

WorkBuddy (`workbuddy-cn`) uses its own native `~/.workbuddy/plugins/installed_plugins.json` and the packaged marketplace under `/Applications/WorkBuddy.app/Contents/Resources/app.asar.unpacked/resources/plugins/workbuddy-builtin/`. The shipped WorkBuddy 5.8.3 registry selection, portable/legacy parsing and component discovery follow the same metadata contract above, while package names and versions come from that product's own manifests. Existing installations can retain older packages absent from the current bundled index. Its account policy, session overlays and runtime integrity checks remain outside disk observations; native resources and their children are read-only.

Both WorkBuddy identities honor the native `WORKBUDDY_CONFIG_DIR`, then `CODEBUDDY_CONFIG_DIR`, before their default user directory; the same selected root drives registry reads and watchers. Relative override paths remain unavailable because Arca does not infer the host process's working directory. These overrides select native inventory only and do not relocate central assets or change Agent settings. Empty legacy MCP/LSP path hints are absent, and unknown standard-manifest metadata is ignored as in the shipped portable parser.

Qoder IDE's Plugins tab reads global `~/.qoder/plugins/installed_plugins_v2.json` records, the native hyphenated alternative when the primary plugins map is absent, and missing global keys from `installed_plugins.json`. Managed records take precedence; later user records replace earlier ones as in the shipped reader. Registered absolute `installPath` values are authoritative, including versionless bundled-cache projections and explicitly registered local packages. The versioned cache path is only a fallback for records without a path. Project scope, unregistered cache history and the Desktop/CLI-owned `qoderapp-bundler` / `qodercli-bundler` markets are excluded. Effective enablement and account or organization gates remain unknown.

For the audited Qoder IDE 1.34.0 macOS installation, Arca also inventories the packaged bundle directories and exact official package fallbacks, plus metadata inside its Apple Silicon Computer Use plugin ZIP. Archives are bounded, checked for escaping, duplicate and symlink entries, and read without extraction or execution. Installed and bundled snapshots remain separate. Authored `.qoder-plugin/plugin.json` and root registration projections retain their source paths; the IDE's authored-manifest component rules determine immediate Skill, Command, Agent, Rule and Hook metadata. Skill path arrays select individual Skill directories; a string selects a directory of Skills. Examples elsewhere in a package are not recursively scanned. Markdown, script and handler bodies are excluded.

Qoder plugin MCP declarations follow the shipped IDE order: authored `mcpServers` path, `mcp.json`, then `.mcp.json`, selecting the first existing file even when it is malformed. Only the `mcpServers` envelope and entries with a nonempty command or URL contribute names. Native IDs use an explicit `identifier`, otherwise `<registered-plugin-key>:<server-name>`; configuration values and credentials are not exported. These are static component declarations and package availability, not native parser acceptance, session activation or a connected MCP. Arca never initializes or migrates Qoder registries, copies bundled packages, edits native settings or offers native-resource mutations. Qoder Desktop, Qoder CLI and QoderWork remain separate Agent identities.


Antigravity's Plugins tab observes the standalone App's default global `~/.gemini/config/plugins/<directory>/plugin.json` packages and materialized `~/.gemini/antigravity/builtin/skills`. A separate built-in Skill group has file provenance and an unknown package version; it does not invent a plugin manifest. Built-in plugin packages, if present under that App's `builtin/plugins`, remain separate bundled snapshots. Antigravity IDE, Antigravity CLI and Gemini CLI have separate builtin or extension roots and are not scanned through this App adapter. The current installed standalone App is represented by the existing `antigravity` identity with the Desktop surface.

The audited legacy default-path contract accepts JSONC metadata, a case-insensitive root manifest marker, optional manifest name, and a separate display title. Installation directory names remain the observation identity, including marketplace hash directories. Manifest version is shown as the package version; `installed_version.json` is retained independently as `installed-version:<value>` in the snapshot revision and sources, even when it disagrees with the manifest. Root `skills/SKILL.md` and immediate Skill children, immediate Agent/Rule/Workflow Markdown files, named `hooks.json` declarations and `mcp_config.json` server names contribute metadata only. Example trees, component bodies, hook commands, environment variables and connection values are excluded.

Antigravity MCP declaration IDs follow the shipped native naming rule: sanitize the plugin directory basename (preserve ASCII letters/digits, `_` and `-`; replace each other Unicode character with `_`), then append `_` and the original server name. These are static declaration names, not native parser acceptance or live connections. Effective enablement, manifest defaults, user choices, native kill switches and session availability remain unknown; Arca does not read user `config.json` or invoke language-server RPCs. Global `plugins.json` inheritance, explicit component selectors and gated Agent Plugins schemas are outside this initial default-path observer: their presence preserves package metadata and reports incomplete observation instead of guessing a legacy fallback. All native observations remain read-only and never enter asset writers.


ZCode's Plugins tab inventories its shipped `ZCode.app/Contents/Resources/glm/packages` manifests, the global installed registry and the official synchronized cache as separate snapshots. The default plugin storage is `~/.zcode/cli/plugins`; `storage.dir` in the native user `~/.zcode/cli/config.json` and a present `ZCODE_STORAGE_DIR` select a different storage base, with `cli` appended only when needed. Absolute user `plugins.dirs` are local package sources. Relative paths depend on the native workspace and are reported as incomplete instead of being resolved against Arca's working directory. Project/session configuration, effective enablement, account policy and MCP connections remain native-owned. Only these non-credential path selectors are retained from user configuration.

The observer reads both native installed-registry shapes, excludes workspace records, retains explicit installation paths and registered versions, and never invokes native atomic recovery. Missing or recovering installations remain unresolved declarations; backup, staging and transaction files are never changed. A valid official `bundled-marketplace.json` partition selects confined cache roots; absent/invalid partitions use the shipped reader's bounded two-level official-cache fallback. Cache records remain cache-only, including synchronized copies; App bundles remain bundled. Availability does not establish installation, activation or a connected server.

Manifest order is `.zcode-plugin/plugin.json`, `.claude-plugin/plugin.json`, then `.codex-plugin/plugin.json`, selecting the first existing marker. Agents, Commands and Skills combine default directories with authored directory paths; Skills include a root marker and immediate eligible children. Inline Agent/Command names, supported Hook events, MCP declarations and validated session UI surface IDs are metadata only. Skill/Markdown labels are file paths; native frontmatter naming and runtime de-duplication remain unobserved. Component bodies, scripts, configured options, credentials and connection values are excluded. Native diagnostic-only Channels/LSP/OutputStyle/Settings declarations are shown without asserting runtime support.

ZCode MCP sources merge root `.mcp.json` first, then manifest inline/path/array declarations in order; both envelope and direct server maps are supported. Later declarations replace earlier provenance for the same name. IDs use `plugin:<manifest-name>:<server-name>`, independent of marketplace. UI surface declarations must reference a declared MCP and a session `ui://` resource; Arca shows the declaration without fetching or rendering it. All native packages/components remain outside asset writers, with no delete, enable/disable, import or assignment action.

ZCode snapshot revisions use `location-sha256` to distinguish physical sources without bypassing CLI path redaction. This is Arca observation metadata, not a plugin Git commit or an installed-version claim; the actual paths remain in source metadata.

ZCode 3.16.3 also creates the shared `node_repl` MCP in host code. Arca verifies the complete shipped `zcode.cjs` against its audited SHA-256 before associating that server with `node-repl-host` and showing Browser Use / Computer Use as consumers. The server keeps its native bare ID, without a `plugin:` prefix. This is a conditional host capability: enabled plugins, selected package, process support and broker permissions remain unobserved. Unknown host artifacts leave this association incomplete while file-backed plugin metadata stays visible. The retired Computer Use MCP namespace is not treated as a current server.

Trae (`trae-ide`) and Trae CN (`trae-cn`) use independent read-only plugin profiles. The global profile verifies `Trae.app`, `com.trae.app`, the product name `Trae` and `.trae` metadata; the CN profile verifies `Trae CN.app`, `cn.trae.app`, `Trae CN` and `.trae-cn`. Each Plugins tab inventories every materialized package under the selected App's bundled `extensions` directory, including base VS Code extensions. Names, versions, default localization labels, command identifiers, theme declarations, model-tool names, chat output-renderer IDs and main/browser entrypoint paths are metadata only. Tool descriptions, input schemas, activation expressions and renderer code are excluded. Unmaterialized entrypoints are explicitly shown as manifest declarations; desktop bundles can omit their web entry files. Product-declared Trae/Solo extension identities without a corresponding package remain declarations. The observer uses the first installed App candidate and does not infer which host mode or extension profile is active.

For each audited native `byted-solo.builtin-mcp` package, Arca verifies complete source-file SHA-256 guards before showing `macos-connectors`, `computer-use` and `chronicle`, five macOS connector declarations, and the Record & Replay marketplace reference. The global and CN entrypoints have separate digest guards. Guards include the actual manifest entrypoint and any separate identity/registration modules. The reference's UUID is a public marketplace package identity; it does not establish installation or enablement. Unknown source bytes preserve independent manifest metadata while leaving native facts incomplete. Extension code, native commands, permission requests, lifecycle helpers and RPCs are never executed.

Both products' arbitrary marketplace registries, runtime permissions, effective activation and MCP connections remain host-owned and unobserved. Legacy Skill materializer/cleanup helpers do not establish a current Skill registration. Global Trae retains its existing `~/.trae/skills` writer; plugin observation adds no writable MCP, Model or Instructions target. Trae CN remains a separate read-only Agent without a writable Skills target. A product whose App is absent has no bundled inventory; downloading another product or caching its package does not create installed observations.

Qoder CLI has a separate read-only Plugins observer. It reads global user/managed installation records from `installed_plugins_v2.json`, preserving unknown activation and organization applicability. Project/local records are excluded; native state resets and migrations are never called. Authored `.qoder-plugin` / `.claude-plugin` manifests and convention folders supply component metadata; root IDE projection manifests are not reused. MCP declarations follow CLI precedence: first existing `.mcp.json` / `mcp.json`, then explicit manifest overlays. No connection options, component bodies or executable handler contents are exported.

The verified Qoder CLI 1.1.68 package exposes four packaged Skills, Security Scan, and the bounded Sites archive with its MCP and four Skills. The Sites artifact size/SHA is checked and archive member paths remain provenance, without extraction. Direct npm launchers identify bundled resources; matching npm-cache packages remain cache-only, including when a custom shell wrapper exists. Sites local-TUI gates, external overrides, hooks, startup registration and runtime connection remain unobserved. Existing Qoder IDE observations and writers remain independent.


QoderWork has an independent read-only Plugins observer. The verified 0.9.18 Electron package supplies ten app Skills, four SDK Skills, the separate SDK Security Scan package, one command, sixteen builtin connector declarations and three host-managed LegoKit MCP runtimes (Canvas, Slides and Writing). App identity and exact package/bootstrap/loader member digests bind these native facts; a changed loader yields partial observation instead of reusing stale registrations. Reading ASAR members is bounded and never executes or extracts app code.

The SDK package has its own verified identity; its Security Scan manifest version and hook-event/executable metadata stay separate from the CLI package. Physical `plugins-example` packages are shown as bundled install sources, separately from global `~/.qoderwork/plugins`, `plugins-custom` and external `legokits` declarations. Only authored `.qoder-plugin` / `.claude-plugin` manifests establish an installed plugin package. Plugin MCP IDs follow `plugin:<folder>:<server>` from `.mcp.json`; raw connection values, Skill/command bodies and credentials are not exported. Native blocked packages remain excluded. Marketplace JSON catalogs are not installed plugins.

Bundled availability and physical installation do not establish Work initialization, copied Skills, connector enablement, account/organization gates, OS permissions, SDK parsing or live connections. Native settings, defaults, migration, runtime startup, plugin installation and resource deletion/toggling remain owned by QoderWork. Its existing global MCP and user-Skill writers remain unchanged and independent from Qoder IDE, CLI and Desktop.

Qoder Desktop has its own read-only Plugins observer. The audited Qoder 0.4.11 package exposes its App Plugin catalog, declared SDK subpackages, Qoder Context and Computer Use archives, SDK Skills/Security Scan/Sites, collaboration and Qoder Helper. App Plugin entrypoints, view renderers, permission names and native-module references are metadata; code is never initialized. Digest-bound host factories and in-process MCP registrations are shown separately from file declarations. A channel's dynamically supplied MCP name stays an unresolved reference. ZIP members remain archive provenance and are never extracted or installed.

Desktop reads global user/managed `installed_plugins_v2.json` records under the native `QODER_CONFIG_DIR` / `QODER_CLI_HOME` / `QODER_CONFIG_DIR_NAME` root, defaulting to `~/.qoder`. Relative overrides remain unresolved. CLI-only cache paths and IDE bundled identities absent from the current Desktop resource manifest are excluded. Current bundled paths and the native explicit overwrite opt-out are checked independently. Authored `.qoder-plugin` / `.claude-plugin` manifests establish SDK ownership; root IDE projection manifests do not. SDK MCP convention files precede explicit manifest overlays. App Plugin development sources are observed only with the native explicit allow-unverified flag, and have no code-derived MCP claims.

Bundled App Plugins and SDK subpackages retain separate identities and physical sources, including OS-specific packages. Catalog entries and component declarations do not establish complete native catalog validation, OS/configuration/account applicability, activation, SDK acceptance, selected sessions or MCP connections. Every native observation remains outside central assets and their delete/toggle/import/assignment paths. Existing Desktop configuration writers are unchanged. Binary digest verification has its own bounded allowance, separate from metadata parsing, so several installed clients can be inspected together.

Qoder CN is a separate read-only Agent for the audited 0.4.11 Desktop package (`com.qodercn.app`). It uses independently verified CN host/SDK/resource digests and the native `QODERCN_CONFIG_DIR` / `QODERCN_CLI_HOME` / `QODERCN_CONFIG_DIR_NAME` root, defaulting to `~/.qoder-cn`. Its ten bundled App Plugins, local SDK subpackages, Context/Computer Use archives and global plugin registrations retain their physical sources. Global Qoder resources are not projected into this identity. The native CN icon and official launcher/download/documentation links are included. This entry provides no writable MCP, Model or Skill target; all activation, account applicability and connection limits above still apply.

QoderWork CN is an independent read-only Agent for the signed 0.9.18 App (`com.qoder.work.cn`). The shared Work observer selects its own identity and digest-bound native loader/path/connector contract; its global roots are `~/.qoderworkcn/plugins`, `plugins-custom` and `legokits`. CN connector declarations, LegoKits, bundled Skills/commands, SDK resources and physical plugin examples retain their own sources. Native initialization, downloads, accounts, enablement, permissions and connections stay host-owned. The CN entry adds no writable MCP, Model or Skill target; the existing QoderWork entry and writers are preserved.

Qoder CLI CN is an independent read-only Agent for the signed 1.1.80 `qoderclicn` distribution. Core reads its Mach-O Bun standalone module table and explicit embedded resource map without launching the CLI, evaluating bytecode, extracting resources or running hooks. The module table, relevant loaders and JSON metadata are digest-bound. The version label uses this passive package verification and never falls back to executing this CLI. Eleven native plugin factories, sixteen code/file Skills, Security 0.9.5 and Sites 0.1.0 retain their real binary member provenance. Current global `installed_plugins_v2.json` records use the CN environment/default root (`~/.qoder-cn/plugins`); IDE-only bundle IDs are filtered according to the shipped reader, and no unregistered cache history or project scope is inferred. The current native manifest grammar uses `workflowsPath` and `workflowsPaths`. Factory declarations and physical resource availability do not establish effective activation, parser acceptance, policy/account/OS applicability, dynamic command/tool sets, a selected process configuration or live MCP connections. This Agent adds no writable MCP, Model or Skill target.

QoderWake is an independent read-only Desktop Agent for `com.qoder.QoderWake.MenuBar`, shell 0.1.19 and backend 1.2.4. Plugins retain separate sources for six native plugin factories, fourteen native MCP declarations, two per-run MCP factories, seven packaged Skills, four `qoderwake.plugin.v1` packages and the bundled Qoder CLI 1.1.76. The complete backend source member, Bun tables, relevant CLI loaders and shipped JSON manifests are digest-bound. No Wake process, hook, setup action, plugin module or native migration is executed. Wake passes `--disable-builtin-skills` to its CLI, so physically bundled CLI Skills do not imply active Wake session Skills.

The Wake observer reads only current global `plugins/registry.json` registrations beneath the native declared home (default `~/.qoderwake`, or an absolute `QODERWAKE_HOME`). Relative overrides remain unresolved. Registered paths must be confined regular files and their manifest identity must match the registry. Unregistered cache history, worker databases, credentials, project configurations and runtime materialization are outside this inventory. Native parser acceptance, enabled versions, account/region/session applicability and live connections remain unknown. These observations provide no writable MCP, Model or Skill target and no delete, toggle, import or assignment action.

QoderWake CN is a separate read-only Desktop Agent for `com.qoder.QoderWakeCN.MenuBar`, shell 0.1.19/backend 1.2.4-cn and bundled Qoder CLI CN 1.1.76. The shared observer uses independently verified CN package, source-member, resource-map and manifest digests. Its declared home defaults to `~/.qoderwake-cn`; its Memory plugin is `qwake_cn_memory` and canonical Memory MCP is `wake_cn_memory`. Other shared callable bodies do not imply equivalent region/account/session behavior. The CN native catalog and webhook region gates remain host-owned, and the compiled-out CLI GitHub setup command is omitted. Bundled factories, packaged declarations and current global registrations keep their own CN sources; no global-product resources are projected into this Agent. All Wake read-only and activation limits above apply.

Claude Desktop's Plugins tab inventories current MCPB packages in `~/Library/Application Support/Claude/Claude Extensions/<id>/manifest.json` and installation declarations in `extensions-installations.json`. The observer understands the current host's native manifest versions 0.1 through 0.4, including its 0.4 `uv` server declaration. A physical supported manifest contributes MCP, tool, prompt and confined entry-point metadata; a ledger row without that manifest stays declared with no manufactured capabilities. `extension:<profile>:<id>` is Arca's stable observation identity; Claude owns display-name collision handling and the callable namespace. Private extension settings, user configuration, commands, arguments, environment and credentials stay outside the projection. The current third-party profile has its separate `Claude-3p` data root and observation identities. An absolute native `CLAUDE_USER_DATA_DIR` override selects its one declared root; relative or unsafe overrides produce a diagnostic. File presence does not select the active profile.

For signed Claude 4.29.0, nineteen exact packed ASAR member digests bind twenty-three Code/Cowork MCP factory declarations, six packaged Skills and three administrator connector adapters. Each factory retains its source and a session-gate reference; activation and connections remain unknown. Three factories with explicit build-time false gates are omitted. Administrator adapter kinds have code metadata only because their configured server names are supplied at runtime. The observer's packed-member contract excludes the unpacked native extensions whose ASAR integrity declarations conflict with their current bytes. Account/session data, guest VM files, remote connector snapshots and another product's global plugin inventory are not scanned. Native observations remain separate from writable central assets.
