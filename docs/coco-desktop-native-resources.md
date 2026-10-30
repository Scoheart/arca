# Cortex Code Desktop native resources

The read-only `coco_desktop` observer targets the actual **Cortex Code.app
1.23.1**, bundle **com.snowflake.coco**, independently of Snowflake's `cortex`
CLI. Current public documentation calls the product CoCo. The selected official
Desktop artifact supplies the App name and identity used here.

The minimum scope is 96 application-owned packaged Skill candidates plus global
native plugin manifests, registry records and settings references. There are no
target/module/helper calls or write actions. MCP observations are references;
this observer emits no promoted host MCP servers.

## Physical identity and passive version

The official Snowflake download page linked:

`https://sfc-repo.snowflakecomputing.com/coco-desktop/downloads/latest/Cortex-Code-darwin-arm64.dmg`

The acquired DMG is **231,115,470 bytes**, SHA256
`896b525239cc755d8404e0f21ef53ad7604b9630d48fb3daabfff96f772ae8fc`.
The public S3 object version was `Z3hZTGQ4T6ep8QvXiRIdvXwzm632TyCQ`,
ETag `0423c53b8a287ef86048017b241c5406-28`. Acquisition used If-Match
and checked the returned object identity. The mutable URL is not a versioned
release URL. The full carrier remains in private research; it was mounted
read-only for acquisition and unmounted without installation.

Static codesign display identifies Snowflake Computing INC., team
`W4NT6CRQ7U`, arm64, and a stapled notarization ticket. Static
`codesign --verify --deep --strict` exited 0 on that full mounted App. This
does not claim a target launch or authenticated acceptance.

Nine complete public files are independently guarded: Info.plist, the actual
MacOS executable, package.json, product.json, bootstrap-main.js, main.js,
workbench HTML, its script, and workbench.desktop.main.js. They total
**19,154,947 bytes**. Exact lengths and SHA256 values live in
`data/native-plugin-coco-desktop.json`, the source of truth with
`deny_unknown_fields`.

Info/package/App version is **1.23.1**. Product engine version **1.110.1**
is a separate value; product appVersion must still be 1.23.1. The product commit
is `ba895eca6729beb2d90232cb2b403d4391714be3`, applicationName is `coco`,
and the ordinary IDE dataFolderName is `.cortexcode`.

The reader uses Core's `info_unlocked` default App selection, restricted to
`/Applications/Cortex Code.app` and `~/Applications/Cortex Code.app`.
Configured targets, launch arguments, new-instance preferences and custom env
do not inherit this proof. Unknown guards or versions return None. The parent
must route this Agent unconditionally through the fresh passive reader so None
cannot fall back to executing a version flag.

Public files stream through a separate 64 MiB hash allowance and 64 KiB buffer.
The 32-entry bounded cache binds selected App/path, expected digest and full
physical stamps: device, inode, length, mode, links, nanosecond mtime and ctime.
Fresh no-follow FDs and final path/selection checks remain mandatory on cache
hits. Regular metadata/identity files require one hard link.

## Actual entry and native loader

The guarded package selects `out/bootstrap-main.js`, which imports main.js.
Main selects the Electron workbench HTML; the HTML loads workbench.js, which
imports workbench.desktop.main.js. The latter registers native PluginRegistry
`nA/v9t` and SkillRegistry `Lk/Qoi`. These are delayed services. Consumers
request plugin lists, and the registry constructor initializes and activates
contributions. Static registration does not prove that it ran successfully.

The exact Desktop global first-name-wins sequence is profile references,
settings.plugins, user physical/registry sources, then bundled. Project results
are merged after global if the name is absent. Actual project convention roots
are `.cortex/plugins` and `.snowflake/cortex/plugins`; the observer does not
scan them. Current public documentation differs on some ordering/defaults, so
the implementation follows the selected shipped Desktop source.

Ordinary IDE extensions are separate. Research retained 83 bundled package
manifests, one gated Mermaid Renderer Tool declaration and zero declared MCP
providers. Those ordinary extensions and their default-profile installed state
are outside this minimum native agent-plugin observer.

## Registry, settings and physical sources

The native snowflakeHome getter uses a truthy `SNOWFLAKE_HOME`, otherwise
userHome/.snowflake. The default global metadata sources are:

| Source | Relative to snowflakeHome |
|---|---|
| Native registry | cortex/plugins/registry.json |
| Settings references | cortex/settings.json, plugins array |
| Physical discovery root | cortex/plugins/ direct subdirectories |

These are independent of the ordinary IDE `.cortexcode` data directory.
Absolute safe SNOWFLAKE_HOME roots are supported as a default metadata scope;
the native running environment/profile is unknown.

Each admitted registry row requires source:string, installedAt:string and
active:boolean. Optional location filtering follows the native normalization
needed for source discovery: catalog becomes stage; an invalid enum or type
becomes undefined. Invalid installKind, Git/GitHub descriptors or stage
descriptors are discarded individually, without rejecting the row or its
source. A stage cachePath outside stage_cache removes only that optional
descriptor. The observer does not inspect or follow those provenance fields.
Raw remote URLs, Git provenance, account values and errors are not emitted.
Registry-only entries stay Declared references.
A physical manifest is Local availability; a registry record is not proof that
its code imported. If a typed record matches the manifest's actual namespace,
its true/false value is preserved as `registry-active-request` in revision.
Public `enabled` remains None for both physical manifests and references:
effective enablement, profile, policy, trust and runtime state are unknown.

Direct subdirectories of the physical user root are a genuine native discovery
authority even without a registry row. Native xb does not probe a manifest in
the discovery root itself; that root can still be a plugin source when named
explicitly by a safe settings/registry reference. Safe absolute or ~/ references
can provide manifests. URI, relative, escaping or unsafe sources remain
references or incomplete, without downloads. Profiles, connections, project
state, DBs and account files are not inspected.

Physical paths remain separate in revision identity, including equal native
names/versions. The observer preserves source availability without guessing the
current winner among profiles, collisions, session roots or workspace trust.
Component ownership follows the physical manifest; plugin names are retained
case-sensitively.

The upstream constructor/getter is unsuitable for scanning: `hb -> Zb`
migrates snowflake.plugins.enabledPlugins into registry.json and clears the
setting. This observer never invokes migration, refresh, install, clone, sync
or native collection registration.

## Manifest and component boundaries

The native parser checks `.cortex-plugin/plugin.json` before
`.claude-plugin/plugin.json`. It picks the first parseable manifest with a
nonempty name, then validates its namespace. An invalid selected namespace
does not fall through. A native name starts with ASCII alphanumeric, then
uses alphanumeric, underscore or hyphen, with at most 64 characters.

Only name, optional version/description, skills, hooks and mcpServers survive
the actual normalization. Safe literal plugin versions are retained, with
unknown otherwise; App version never replaces a plugin's own version.
Commands and custom agents fields are not manufactured as supported features.

Skill roots are a string or an all-string array. They replace the default
skills/ root; an empty array selects no roots. The bounded stat-only walk stops
at the first SKILL.md. Agent leaves use the fixed agents/ directory and direct
.md filenames. No frontmatter, description or instructions are read. Component
native IDs stay None.

Hooks use inline event names or a stat-only hooks/hooks.json reference. Explicit
hook path declarations are tracked only to prevent a file from being read
through another component's metadata field. Nested/absolute references and
bounded unknowns fail closed for file-based MCP reads. Hook files and commands
are never opened.

MCP accepts inline maps or JSON file strings/arrays; unsupported primitive
values use .mcp.json. Empty string/array is exclusive and does not fall back.
Independent safe files survive an error in another source. Only server names
and metadata paths are emitted as McpReference, never endpoint/env/OAuth
values. Non-JSON, hook/Skill/agent/credential payloads, escaping paths and URIs
stay opaque references/incomplete.

Native factories expand private interpolation files and shell environment and
choose URL/stdio transports. They are not called or reproduced. Compatibility
with a Claude directory is not a claim of Agent Plugins v1 conformance.

## Application-owned packaged Skill candidates

The selected App has no resources/snowflake/plugins directory. A newly appearing
directory marks the view incomplete rather than inventing bundled plugin rows.

Its resources/snowflake/skills tree contains 580 SKILL.md leaves. Native
discovery stops at a first Skill root instead of treating nested support Skills
as independent registrations, yielding **96 candidates**. The descriptor binds
their exact relative paths and public stat lengths; runtime performs stat only.

They are projected through one application-owned `@builtin/skills` group.
Titles are directory names, with native_id=None. Parsed names, valid/nonempty
instructions, preview/feature gates, disabled settings, aliases and actual
activation remain unknown. A changed stat length marks incompleteness; no
component body hash or parse is attempted.

The independent global user Skill directories are not turned into fake plugin
groups. Plugin-contributed candidates come only from their own manifest paths.

## Limits and evidence

Each metadata snapshot has separate 8 MiB aggregate reads, 2 MiB per JSON,
8,192 visits, 2,048 pinned paths, 1,024 entries per directory, 512 source roots
and 4,096 components. Ancestry is bounded before descent at 64 components;
Skill traversal is bounded to depth 16. Consistency anchors are excluded from
watch paths. FDs, leaf stamps, missing paths, source environment and selected
App are rechecked before publication. Independent bundle data survives a
malformed/unsafe global source; shared host scan allowances are untouched.

Private research is in coco-desktop-followup-20261030:

| Evidence | SHA256 |
|---|---|
| contract.json | 3a2b58b761203fb76460737ef19479e05f127beca28f17061ce107f86724a7a7 |
| source-index.json | a563ece5625a0e600be9a28df61f509d9831f769e37eea9eb0137ab6c3faa5d3 |
| resource-inventory.json | 916d951722c03b18dd0151b77913a0d334483d74bc95949d9b17792e2054ca11 |

The source index binds 178 complete methods and exact entry fragments. Relevant
native methods include v9t.jb/ub/wb/yb/zb/Ub, Fb/Ib/Kb/Mb, and Qoi.gb/sb/ob/tb.
Exact source ranges/digests distinguish observed bytes from inferred behavior.

REA parent `ev_e7e01d496686e6f47c01a1c8669ad82f39513e8b28a4d9635d5cb1bfc20518c9`
covered 36 views, 101,447 text bytes and 23,934 AST nodes. Application and
semantic coverage are partial after bootstrap-main semantic heap exhaustion;
4,845 semantic unknowns remain. The full exported bundle is 49,248,461 bytes,
SHA256 `66605f3c850da0b1b65561ede5aa4f5a99f4b854bf08069d077c251a7142cfbf`.
The analysis was not retried or enlarged, and no native session was opened.

Thirty-two isolated TestHome regression sources are retained but not run. This worker ran
no product tests/build/fmt/validator, target, lifecycle, GUI or installation.
Root owns shared wiring, production compilation and actual-HOME acceptance.
