# Devin Desktop native resources

This observer uses the stable Agent ID `windsurf` for the officially renamed
Windsurf IDE. The current App is `Devin.app`, bundle ID
`com.exafunction.windsurf`, client version `3.11.228`, VS Code base `1.126.0`,
build `053434014dbdb7a662a14e38752946a0dafb97f2`. Cloud Devin is a separate
identity. The observer does not read cloud, CLI, Gateway or legacy Windsurf state.

`data/native-plugin-devin.json` is the audited source of truth. Its contract,
guard, extension and Evidence structures reject unknown fields. Only
`/Applications/Devin.app` and `~/Applications/Devin.app`, in host descriptor
order, are eligible. The adapter/layout are `devin` / `devin_app`.

## Scope and meaning

The observer emits metadata from three separate sources:

| Source | Observation | State boundary |
| --- | --- | --- |
| 102 exact bundled VS Code extension manifests | `publisher.name`, package version, display metadata, entry references and the audited contribution subset | `bundled`; activation unknown |
| Default ordinary `extensions.json` | Registered versions and matching physical extension package metadata | `installed` only when ID/version/source match; active profile unknown |
| Default Agent-plugin `installed.json` | Current installation records and their selected physical plugin manifest/resources | `installed` only for supported physical sources; effective enablement/policy/loading unknown |

Ordinary IDE `contributes.commands` do not become Agent commands. The fixed
bundle contains one `languageModelTools` declaration,
`vscode.mermaid-markdown-features/renderMermaidDiagram`, with a native local-chat
condition. It is a Tool declaration, not evidence of activation or an MCP
connection. There are zero shipped `mcpServerDefinitionProviders` declarations.
User extension provider declarations remain `McpReference` components.
No component in this adapter is promoted to a concrete host MCP server.

The 12 direct `out/vs/sessions/skills/<name>/SKILL.md` paths form the explicit
application-owned `@builtin/sessions-skills` group, named
“Devin bundled session Skills”. This is an availability group, not a plugin
manifest or a standalone user-global resource group; do not add it to the
host's `resource_groups` exclusion list. Existing pure NativeSkill projection
therefore displays its Skills with neutral source attribution. Their version
is the version of the owning App group. Individual plugin versions are read
only from their own manifest/package and otherwise remain empty/unknown.

Skill leaves are FD-relative stat-only. No body or frontmatter is opened.
The native sessions loader requires valid name/description and a name matching
the folder; this observer has not evaluated those requirements. The native
user/local shadowing and disabled-URI filters are also unobserved. All emitted
plugins/groups have `enabled: null`; this never means a Skill is loaded.
The two Playwright dependency Skill paths are excluded from App/plugin ownership.

## Identity, cache and independent bounds

Before any registry or resource observation, Info.plist, package.json,
product.json and twelve whole public members must match the audited identity.
The checks bind bundle name/display name/executable, both bundle versions,
package name/base version/main, product name/application name/data folder,
client version and build commit. Unknown versions or changed guards fail closed;
there is no old-name, old-directory or migration fallback.

The twelve guards total **73,205,612 bytes**. They include the Mach-O entry,
`out/main.js`, sharedProcess, extensionHostProcess, Workbench, sessions,
`extensions/windsurf/{package.json,dist/extension.js}` and
`extensions/windsurf-remote-devin/package.json`. Full SHA256/lengths live in
the data contract. Main/source hashing streams through a 64 KiB buffer under a
separate 128 MiB identity budget, rather than allocating large JS bundles.

The bounded 32-entry digest cache keys the selected App path/stamp, member
path/stamp and expected SHA256. A stamp includes device, inode, length, mode,
mtime and ctime with nanoseconds. Every call opens fresh confined FDs, compares
FD and named-path identity, verifies the small semantic identity fields, and
rechecks App selection and every retained FD before publishing. Replacement,
movement, upgrades, permission/timestamp changes and changed expected digests
cannot reuse the old proof. Cache entries prove only a digest, never runtime state.

Metadata has its own finite allowance: 8 MiB total reads, 2 MiB per document,
8,192 visits, 1,024 retained paths, 1,024 entries per directory, 512 rows per
registry and 4,096 components. Stat-only file sizes are bounded independently
to 16 MiB per leaf and 64 MiB total. These counters do not consume or modify
the other native observers' shared ScanBudget. Invalid/exhausted sources mark
the inventory incomplete; independent safe sources can remain visible.

All paths use shared confined `NOFOLLOW` opens plus FD/path CAS. Directory
enumeration is confined and bounded; body leaves use `statat` with
`SYMLINK_NOFOLLOW`. Metadata FDs, parent FDs, leaf stamps and missing-path
observations are revalidated before publication. Watch paths use the same
source admission and never include envFile/credential targets or unsafe roots.

## Default global registries

Only these current default namespaces are observed:

- Ordinary extensions: `~/.devin/extensions/extensions.json`.
- Agent plugins:
  `~/Library/Application Support/Devin/User/agent-plugins/installed.json`.

These paths represent passive **default-profile records**, not the running
IDE's active profile. Actual argv, environment, remote/workspace authority,
feature/harness policy and native migration results are unobserved. A nonempty
`VSCODE_DEV`, `VSCODE_EXTENSIONS`, `VSCODE_PORTABLE` or `VSCODE_APPDATA` in Arca's
environment causes global observation to be skipped/incomplete; arbitrary
override locations are not followed. App-owned bundled metadata remains eligible.

The native ordinary root precedence is `--extensions-dir`,
`VSCODE_EXTENSIONS`, `VSCODE_PORTABLE/extensions`, then the product data-folder
default. This implementation supports only the admitted default namespace.
Its registry reader accepts the array format and validates identifier/version
and the native location shape. Nonempty `relativeLocation` takes precedence;
otherwise a file URI object can resolve only inside the selected extensions
root. A bare string `location` without relativeLocation is ignored by the
native reader; the observer retains only unresolved/declared provenance.
Registered versions remain separate; unregistered historical directories and
other profile directories are never scanned as installations. Empty/malformed
JSON or unsupported URI shapes remain incomplete rather than triggering native
repair or migration.

Agent-plugin records must have the **current written version 1** shape:
`{version:1, installed:[{pluginUri, marketplace, name?}]}`. The native reader is
more permissive about version, but this observer does not infer future/legacy
formats. Only local, authority/query/fragment-free file URIs resolving inside
the current agent-plugins home are followed. Percent encoding is decoded once;
encoded separators, traversal, symlinks and outside/remote sources remain
unresolved provenance. URI hashes give Agent plugins a separate identity
namespace, so they cannot merge with same-named VS Code extensions. Marketplace
text/remote query contents are not emitted. Repository caches, configured
locations, enterprise references and marketplace declarations do not independently
prove installation. Missing records cause no initialization or writes.

## Agent-plugin manifest and resources

The actual `Ejo` precedence is `.plugin/plugin.json`, then a `.claude` path
segment or `.claude-plugin/plugin.json`, otherwise root `plugin.json`. A selected
malformed/missing manifest never falls through to another format. Those formats
are called openPlugin, claude and copilot in the compiled source; shared
filenames/components do not establish full Agent Plugins v1 conformance.

`kjo`/`_jo` supplies the default resource path plus trimmed string/array overrides;
`{paths,exclusive:true}` suppresses that default. Path normalization remains
confined to the plugin. The observer supports direct Markdown files/direct
children for commands, agents and rules. Skills test `<path>/SKILL.md`, then
direct child markers, with a plugin-root marker fallback if none are found.
Skill display labels come from path metadata; native frontmatter naming and
deduplication are unobserved. Hook files/inline objects are provenance-only
TypeDeclaration references; no hook parser or command is executed.

Inline MCP objects and MCP path lists are separate native branches. File lists
start with `.mcp.json` unless exclusive, preserve order, and keep the first
duplicate server name. The observer admits JSON/JSONC metadata files only; a
path override cannot redirect the JSON reader into SKILL.md, Markdown bodies,
envFile or executable source. Unsupported syntax/shapes stay incomplete/basic
provenance. It does not implement the native parser's arbitrary error recovery.

Workbench `Tjo → pYn → hzi` accepts an own `mcpServers` map or a direct map.
`stdio`, or an omitted/empty type with command, requires a nonempty command;
`http`/`sse`, or an omitted/empty type with url, requires url. This branch does
not accept `serverUrl` or `ws`. Only accepted declaration names/source files
are emitted as **McpReference**, never configurations, command/args, env,
headers, URLs, tokens or connection state. Root/env interpolation is not run;
envFile and credential pointers are never followed.

This Workbench parser is separate from Cascade's MCP config. Current
[Cascade input documentation](https://docs.devin.ai/desktop/cascade/mcp)
supports the `mcpServers`/stdio/HTTP/SSE and `serverUrl`/`url` writer subset.
The actual extension getter selects `XDG_CONFIG_HOME/devin/mcp_config.json` or
`~/.config/devin/mcp_config.json`, but the native Cascade backend parser was
not decompiled/executed. This adapter does not certify its full input semantics,
share Workbench's parser with it, change its global writer path, migrate a
file or call startup helpers. Shared catalog/codec/path selection belongs to
the separate Agent integration.

## Evidence and remaining unknowns

The fixed official archive is
`https://windsurf-stable.codeiumdata.com/darwin-arm64/stable/053434014dbdb7a662a14e38752946a0dafb97f2/Devin-darwin-arm64-3.11.228.zip`.
Official and observed SHA256:
`30a2e5bc8b97d2c5ba94b09b98c0aed35734d87677bdbdf1760c3d0d3efd3ec7`.
It was installed without overwriting an existing App and without launching it.
The publisher is EXAFUNCTION, INC., Team ID `83Z2LHX6XW`; the ticket is stapled.
Main-only codesign verification used `--ignore-resources`; full resource seal
or Gatekeeper/runtime acceptance was not claimed.

Research contract SHA256:
`d46267f2925c6ec2e40bc9a16a2d16bbfe7715db8c68b2cc7cfbfb788745c498`.
Local evidence root:
`workspace/local/research/arca-native-plugin-audit-20261028/desktop/windsurf-followup-20261030/`.
It contains `contract.json`, `source-index.json`, `closed-source-ranges.json`,
`implementation-source-ranges.json`, installation/acquisition receipts and
`rea/complete-evidence-bundle.json`.

The 24 frozen source ranges bind actual source SHA, byte/character offsets,
minified line numbers and function anchors: `Ah`/profile roots, ordinary
registry/row schema, plugin formats/`_toPlugin`, installed store/repository,
MCP parser/factory, Cascade getter/status wire, Workbench Skill sources and
sessions discovery. Six additional inert ranges retain `_jo`, `Pjo`, `mzi`,
`CYn`, `Qr` and `Bo` used to delimit safe metadata subsets. Their source member
is the same pinned Workbench guard, not an unpinned upstream substitute.

REA parent `ev_f3e5bcaf31c833aa01b69490b0bc1f0b69a438925e96e8affccf9ed4953eca76`;
summary `ev_6dd70d3c16c8af07f4f5062dddc5dbc7deb749e5b63ed6375363e7b231e0785d`.
Input: 107 real public members, 24,132,194 bytes. Coverage is **partial**:
main/sharedProcess semantic workers reached the 1 GiB heap bound; Workbench
AST parsing failed with its exact cause unestablished. There are 24 semantic
unknowns. Source maps were unavailable via DNS/TLS. Full Evidence export SHA256
is `3fe9127a9fd6340eba86d3eae5692eec56e19a655cc324d5ec7d78df9472b9e3`;
only two of its four retained records belong to this target. Missing REA
findings do not establish absence. No native REA session was opened/closed.

Isolation regressions are preserved in `devin/tests.rs` but **not run** under
the current fast-mode task. They exercise registry separation/precedence,
versions, ownership, no body/helper reads, URI/path/symlink rejection, independent
budgets, cache/CAS/source selection and safe watches. No target CLI/App, login, module, factory or migration helper was executed
during acquisition and observer research. No private configuration/DB or Skill
body was manually inspected. Production Arca compilation, ordinary resource
queries and the separate development-preview gate are delivery checks, not
execution of the observed Devin target. Tests, validators and formatters remain
unrun in fast mode.
The adapter exports observe/watch_paths only; it has no CLI runtime_version
implementation or target-executing version fallback.

The separate Agent integration keeps ID `windsurf` while displaying and
launching the current `Devin.app`. The documented Cascade MCP default is
`~/.config/devin/mcp_config.json`; XDG/custom locations can be selected through
Arca’s existing configuration editor. The current Skill target is
`devin-desktop-user` at `~/.config/devin/skills`, with the positively observed
Workbench global sources `~/.agents/skills`, `~/.copilot/skills` and
`~/.claude/skills` as shared target aliases. Existing user MCP path overrides
are preserved. Updating declarations performs no native-file migration or
automatic asset projection.
