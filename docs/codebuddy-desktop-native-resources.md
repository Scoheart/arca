# CodeBuddy IDE native resources

The `codebuddy_desktop` observer reads metadata for two distinct macOS Apps:

| Agent ID | App / bundle identity | Ordinary extension default |
| --- | --- | --- |
| `codebuddy-ide` | CodeBuddy.app / `com.tencent.codebuddy` | `~/.codebuddy/extensions/extensions.json` |
| `codebuddy-cn-ide` | CodeBuddy CN.app / `com.tencent.codebuddycn` | `~/.codebuddycn/extensions/extensions.json` |

Both use the fixed official release **4.12.2.41207131**, outer commit
`425ab70e0139db0a8f17377172fed0c4376b4343`. App Info version is **4.12.2**,
the outer VSCode package version is **1.106.1**, and bundled
`Tencent-Cloud.coding-copilot` is **3.10.0**. These separate version fields are
checked independently. A plugin's version comes from its own native manifest
or explicit marketplace metadata, with the source stated in its revision;
otherwise it stays unknown. App version is never substituted for an installed
plugin's version.

The source of truth is `data/native-plugin-codebuddy-desktop.json`, parsed with
`deny_unknown_fields` for every contract structure. Each host accepts only its
own App in `/Applications` or `~/Applications`, layout `codebuddy_app`. A
different region, layout, symlink, version or changed public guard fails closed.
This module does not install either App or add a CLI entry.

## Public identity and source binding

Both archives were obtained from the region's official
`/v2/update?platform=ide-darwin-arm64` response and checked against its complete
SHA-256:

| Region | Archive bytes | SHA-256 |
| --- | ---: | --- |
| Global | 221,196,660 | `4c1a424958eeb3ee4068b363acd062849699f4d48b1cf5adba56209831569491` |
| CN | 221,221,340 | `acbf4321ca93fd9d3a3982d8cde76444235232f1d73362d1ddb1ba09d014c96c` |

Signature display metadata identifies Tencent Technology (Shanghai), team
`FN2V63AD2J`. This does not claim complete bundle trust, notarization, launch or
runtime acceptance. Acquisition receipts retain URLs, redirect chains,
headers, bounds and whole-file hashes; original Skill bodies were not extracted.

Each profile guards ten complete public members: Info.plist, the actual
`Contents/MacOS/Electron`, outer package/product, `out/main.js`, Genie
package/product/complete `out/extension/index.js`, workbench JS and extension
host JS. Their combined sizes are **40,422,943 bytes global** and **40,383,396
bytes CN**. Hashing streams through 64 KiB buffers in a separate 128 MiB identity
budget. Only small public metadata is allocated for parsing.

A bounded 32-entry LRU cache keys successful digests by selected App path and
physical stamp, member path and stamp, and expected digest. Stamps contain
device/inode, length, mode, link count, nanosecond mtime and ctime. Regular
identity/metadata files must have exactly one hard link before this observer
opens them.
Stat-only Skill/hook leaves may have multiple links; their link counts remain
part of final snapshot revalidation. Cache hits still require
fresh confined FDs and matching FD/path stamps. App selection and every bound
member are checked after each guard and again before publication. Moves,
replacement, upgrades and changed bytes invalidate identity. A guard-only
`runtime_version(selected)` is available for the selected App root or its actual
Electron member; it reads no native registry and never executes a version flag.

App selection reuses Core's `info_unlocked(agent_id)` default launcher resolver,
including its bundle-ID admission, without acquiring another workspace gate.
A configured target, arguments, environment overrides or a new-instance request
cannot inherit this default observation. The resolved App must belong to that
host's fixed allowed roots; the observer never falls back to a different root
when Core selects an App outside that set. Selection and launch preferences are
rechecked with FD/path identity before publication.

The frozen research contract is
`workspace/local/research/arca-native-plugin-audit-20261028/codebuddy-followup-20261030/contract.json`
(SHA-256 `23aeea7f6fb3b7d013ea03e52631f17b9bfa36f9b3bb53ad5f6f15a75dd3cfb5`).
Its source index is `5883f48ffe69fa6ecca477374b3eeae748f64a69130ad8c665caa4b3a39abd35`:
63 exact module slices, 1,216 method/function ranges and six ordinary registry
schema helpers, each tied to original member SHA and byte ranges. Every
selected slice has identical bytes in the opposite region, but each region
retains its own complete App/Genie guards and product configuration.

The entry chain is outer `package.main` → ordinary extension scanning → Genie
manifest (`activationEvents: ["*"]`, `main: out/extension/index.js`) → shipped
webpack entry **786500** → DI application services. Source bindings include
native PluginManager **553017**, LocalMarketplace **811205**, format/constants
**365400**, BaseMarketplace **689245**, brand-root helpers **729907**,
ClientProductProvider **873559**, its base-path binding **403320**,
SettingsConfigManager **608968**, SkillsManager **387891**, MCP settings
**252124**, dispatcher **944034**, schema **385788** and transport **367044**.
They establish static parsing and loading branches; they do not observe an
activated extension, current user policy or connected server.

REA parent `ev_fc504a7c919b0530b56eef9e5a42a5e2f159f7ae536321f2382cfe60bcdbb281`
and summary `ev_76ab6a6a162a52eb5046a614cbc20dd67e31593acaec4509da5401a55f24ef0d`
cover 19 initial source ranges / 458,478 bytes / 105,455 AST nodes with **partial
application and semantic coverage**, 13,663 semantic unknowns. Some initial
ranges contain adjacent complete webpack members; their filenames are not
unique module IDs. The later exact one-property source index is authoritative
for the source facts above; it does not expand the claimed REA coverage. Full
Evidence exports were retained; the owned native metadata session was exported
and closed. Unavailable deep native providers were not installed or retried.

## Bundled availability and declared installation

Each exact App has **100 direct ordinary extension manifests**. These produce
extension metadata, plus supported Tool/MCP-provider declarations from their
manifest keys. The pinned bundle contributes one conditional Tool:
`vscode.mermaid-chat-features/renderMermaidDiagram`, guarded upstream by
`config.mermaid-chat.enabled`. Its gate is not evaluated here. No pinned ordinary
manifest contributes an MCP provider. Ordinary editor commands do not become
Agent commands, and no extension entrypoint or provider factory is invoked.

Two explicitly application-owned source groups preserve **3 builtin** and
**12 WeCom** Skill files from `extensions/genie/out/extension/{builtin,wecom-skills}`.
Names are concise directory names, `native_id` is null and enablement remains
unknown. Only fixed roots, direct children and regular-file stat metadata are
used; expected names and lengths come from original ZIP metadata. No
frontmatter, body, reference file or helper is opened. WeCom has a native default
gate of false and is toggled from the request's `isWecomAiBot`; availability
does not prove that this request exists or that a Skill loaded successfully.
These source groups are not native plugin manifests and are not standalone
user-global resource groups.

The WeCom group also retains one application-owned Tool declaration,
`wecom_mcp`, sourced from the guarded public Genie `out/extension/index.js`.
Its name, source and request gate are declared once in
`data/native-plugin-codebuddy-desktop.json`; the reader projects that data into
the existing WeCom resources group. This adds a second bundled Tool declaration
beside Mermaid, without adding a plugin, MCP provider or host MCP server.
Enablement, successful registration, request presence and runtime remain unknown.

The inner product also declares nine remote user-scope installation requests:
pptx, pdf, docx, xlsx, agent-browser, playwright-cli, hotskills, skills-sec-audit
and find-skills. `hotskills` requests autoEnable false; the others true. They
remain **declared-only**, with unknown plugin version, installation and active
state. The exact App ZIP contains no native plugin.json. A product request,
remote catalog or autoEnable flag cannot become a bundled/installed plugin.

## Two independent global registries

Ordinary default-profile registration and the native Genie marketplace are
different mechanisms. The outer IDE chooses ordinary extension roots with
`--extensions-dir > VSCODE_EXTENSIONS > VSCODE_PORTABLE/extensions >
HOME/product.dataFolderName/extensions`. The observer reads only the pinned
default namespace. A visible override suppresses that ordinary view and marks
it incomplete; it does not redirect arbitrary IO or suppress the independent
native default view. Running argv, remote state, portable selection and active
profile remain unobserved.

Ordinary `extensions.json` preserves every registered version. Nonempty
`relativeLocation` takes native priority over the URI object. Otherwise only a
confined local `file` URI object with empty authority/query/fragment is resolved.
String locations, remote URIs, traversal, symlinks, missing or mismatched
physical package metadata retain registered provenance as declared-only.
Unregistered version-history folders are not enumerated as installations.
The default profile is not asserted to be active; no profile/session database
or enablement store is opened.

Genie's brand helper reads the **inner** ProductManager.current.dataFolderName,
then falls back to `.codebuddy`. Its ClientProductProvider starts at the
extension directory and first finds `extensions/genie/product.json`; neither
pinned inner product declares dataFolderName. Thus both App builds have the
proved **no-override native default** `~/.codebuddy`, even while the CN ordinary
extension default is `.codebuddycn`. Remote/cache/environment ProductProviders
may change the running root or skill scan directories. This observer labels a
default namespace and never claims that it is globally active.

Native registry is `~/.codebuddy/plugins/known_marketplaces.json`; user
membership is `enabledPlugins` in fixed `~/.codebuddy/settings.json`. The native
IDE has no CLI-style installed_plugins.json in this loader. False is still
membership. Native effective precedence also includes project and project-local
settings; this observer reads neither. A user configured boolean is stated as
such in revision provenance, while `enabled` stays null.

The observer preserves registry storage name and native marketplace manifest
name separately. It only follows an explicit installLocation confined under
the fixed `~/.codebuddy/plugins/marketplaces` root, then the first existing
`.codebuddy-plugin/marketplace.json` or `.claude-plugin/marketplace.json`.
Preferred malformed JSON does not permit an invented legacy fallback.
Remote/local source declarations do not trigger downloads, repositories,
installation, external path reads or helper calls.

Physical plugin destinations must stay inside that current marketplace.
Local string/{source:local,path} references are confined; typed Git/URL/ZIP
sources can bind only the existing `plugins/<catalog name>` destination. The
native plugin manifest uses `.codebuddy-plugin/plugin.json` before
`.claude-plugin/plugin.json`, preserving native names and own version metadata.
Unique current membership plus a current confined native manifest yields
installed metadata; ambiguous identity remains cache-only with declared user
membership. Materialized catalog entries without membership are cache-only.
External, missing, malformed or unresolved sources remain references.

Format-1 `.plugins-cache.json` contributes cache-only basic provenance, never an
installed assertion or component ownership. Cached installedPath, Skill,
command and MCP paths are not followed. Current physical manifest metadata is
preferred; a stale cache cannot invent Skills or bypass confinement.

## Plugin-owned components and MCP limits

Native Skill string/array and file/path metadata forms resolve only inside the
current physical plugin. The default skills subtree and root Skill filename
fallback are observed with stat-only scans, `SKILL.md` before `skill.md`, native
depth bounded by 20 plus independent visits/components/FD limits. Dependency
directories are skipped. Filenames/directory names remain availability titles,
and all Skill native IDs stay null because frontmatter is unread. Commands,
agents and rules likewise use actual filenames and bounded stat-only discovery;
hook/LSP file availability or inline declarations never execute code.

Native MCP inline maps and declared JSON-file/string-array references retain
only server-name/source metadata. The proved branch falls back to `.mcp.json`
when explicit files contain no entries, and then inspects `mcp/*.json`; conflicting
names retain their separate source references because native readdir order is
not observed. Qualified native IDs use `marketplace/plugin/server`. No envFile,
credential, command, OAuth value, endpoint or factory is exposed or invoked.
Every such component is `McpReference`; none is promoted to host MCP inventory.
The shared `component_payload_is_opaque` predicate rejects sensitive path
components, including credential/secret/token/account/OAuth/cookie stores,
auth files, hooks, sessions and dependency directories. Before any MCP file
read, the observer also collects the fixed `hooks/hooks.json` and explicit
`manifest.hooks` string/array/object `file`/`path` body references without
opening those payloads. Malformed or over-budget body provenance keeps all file
sources opaque; inline MCP metadata may still retain its own declarations.
Opaque overrides and non-JSON component bodies retain only a filename reference
and mark the view incomplete; those
files are not opened and their unknown contents do not trigger a guessed
fallback. Other independent MCP metadata files continue to be considered.
This is a conservative observation limit, not a different native
MCP schema.

The IDE's global MCP input is separately `~/.codebuddy/mcp.json`, wrapped
`mcpServers`, whereas CLI selection prefers `.mcp.json` and has a different
loader/ledger. The observer does not read, migrate or write that global MCP
file. The pinned IDE parser supports comments/trailing separators; its schema
and runtime merge/factory were independently bound in research, including
project > plugin > user precedence and user disabled overrides. That evidence
does not authorize a shared CLI schema or a writer change.

`wecom_mcp` is a request-gated special Tool using IPC proxy list/call operations;
it is not a concrete server configuration or ordinary MCP provider manifest.
No MCP server is fabricated from the WeCom name or Skill filenames. Compatibility
with selected `.claude-plugin` forms does not assert full Agent Plugins v1
compliance.

The additional Tool projection is bound to exact bytes in both already acquired
Genie entry members, each **21,365,905 bytes**: global SHA256
`d8833f7b86e0c59c798aee800a5ce3e3053f7e919e6bd28290448b2b47ecd72b`,
CN SHA256 `b8fad5cc96e9919841ef9d01d17943bd5bc879d563508f52d9b822dc84cc7934`.
The following zero-based UTF-8 ranges are identical in those two public members
(ends are exclusive); they supplement the existing partial REA/source index,
without claiming another REA run or complete semantic coverage:

| Source fact | Byte range | Slice SHA256 |
|---|---|---|
| Module 358175 assigns `WECOM_MCP_TOOL_NAME="wecom_mcp"` | 7256606–7256640 | `6664632d9ae779eb3961550a0300e216381e3d808a5552196c6fac038c4dd7e6` |
| Module 90629 defines `createWeComMcpTool` | 1579293–1579347 | `36233a62dafbd616c77b6d76cfd3fddf7a4ce8855ef1ed5ec9ffb5214e3e16e4` |
| Request `isWecomAiBot` guards its registration branch | 1788273–1788320 | `cc20a5955aa90fbde5d4c2d2e9774099d294267c32ce69817574a54a9ec50de6` |
| Factory result is registered using the shared Tool-name constant | 1788620–1788722 | `690fa93191337dd8ab89be10e52a48d7c7ee796b4469de5aabf51842344ea199` |

## Bounds, races and validation status

Metadata reads use a separate **8 MiB total**, **2 MiB per document**, **16,384
visits**, **2,048 bound paths**, **1,024 entries per directory**, **512 registry
rows/declaration items**, and **4,096 components** budget. Stat-only leaves have 2 MiB per-leaf and
64 MiB aggregate declared-length bounds. Neither metadata nor identity touches
the parent PluginScan allowance, so another host cannot spend this observer's
budget or lose its own allowance to these guards.

All file/directory opens use the shared confined no-follow primitives. Resource
enumeration is FD-relative; Skill/command/rule/hook leaves use FD-relative
`SYMLINK_NOFOLLOW` stat and are never opened. Bound FDs, selected paths, missing
paths, stat leaves, App selection and namespace selection are rechecked before
publication. Fingerprints bind snapshot physical stamps without exposing
unrelated native values. Watch discovery uses the same bounded capture and only
adds user registry paths after public App identity succeeds. Unsupported or
malformed siblings can mark the view incomplete while already bound metadata
survives; a final identity/CAS failure publishes no scan.

Upstream APIs are unsuitable discovery helpers: activation starts application
services and rules migration; PluginManager.init writes configuration and
installs/updates markets; ordinary profile scanning can repair/migrate/write;
the MCP getter creates a missing directory/file and its settings reader may
migrate old storage. None is called. No native account, credential, DB, session
store, user Skill body, module, lifecycle, App or CLI is executed/read here.

The 48 isolated TestHome regression cases are retained in
`core/src/resources/plugin/codebuddy_desktop/tests.rs`. It was written before
the implementation and has **not been run** under the task's explicit fast
mode. No test, fmt, clippy, validator, build, install or GUI command was run by
this worker. Production compilation, installation, actual-HOME comparison and
desktop acceptance belong to the root task and its delivery owner; no runtime
or target-execution acceptance is claimed here.
