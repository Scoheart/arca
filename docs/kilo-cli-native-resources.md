# Kilo CLI native resources

The `kilo_cli` observer binds the official macOS arm64 **7.8.18** carrier.
It is independent of [Kilo Desktop](kilo-desktop-native-resources.md), whose
0.1.14 package contains a different 7.8.17 child CLI. A package version is
never substituted for an SDK plugin's own version.

Kilo CLI plugins are JavaScript/TypeScript SDK modules. SDK V1 means a default
object exposing server or TUI entry functions; legacy function exports also
exist. This does not establish Agent Plugins v1 compliance, and Desktop's
`plugin.json` is a separate native format. Arca never imports either format.

## Selected artifact and passive version

The official carrier is 238,593,506 bytes, SHA-256
`73e6626784cb314308e035f11b8cb8ab251624a593c2a110abeb94869514a237`.
The official ZIP hash is
`1ea2285a2f45ec27b7fa3b644f92700adf4a91fd671aaf18ba0cb8f26c795df5`.
Public guards are in `data/native-plugin-kilo-cli.json`.

A separately bounded 512 MiB identity pass streams complete files through
64 KiB buffers. Its finite 32-entry digest cache binds expected hashes,
selected alias, physical files, directory identities and nanosecond stamps.
Files must have one hard link. Alias changes, replacement, permission changes
and changed bytes invalidate the proof. No previous observer's budget is used.

The selected command may be the exact native carrier or the audited npm
`@kilocode/cli` wrapper plus its guarded `bin/.kilo` cache. The package JSON,
wrapper and postinstall source are verified but never executed. Without that
cache, the wrapper selects platform packages using Node's own architecture,
which may differ from Arca under Rosetta. This branch remains unknown; Arca
does not run Node, architecture probes, the wrapper or a version flag to fill it.
An inert static installation can stage the audited cache without postinstall.

The version additionally matches the compiled UTF-16 version marker inside
the whole-file-bound carrier. Unknown artifacts never fall back to execution.
Custom Arca launch targets, arguments, environment/preload overrides and a
changed default selection cannot inherit the official observation. Passive
version routing rechecks the multi-member proof outside the generic version
cache.

## Bundled groups and Skills

One application-owned Code group contains the 15 directly imported SDK
callbacks, bound to the actual compiled callback array. This is code
availability, not 15 independently installed manifest plugins or 15 active
providers. Callback factories, account gates, policy and loading stay unknown.

One separate application-owned group projects `kilo-config` to the Skills tab.
The Bun graph table and member metadata are verified without decoding its
Markdown body. Source labels identify Kilo's bundled Skill; it has no inferred
plugin owner or native frontmatter ID. User overrides and actual invocation
remain unknown. The indexing and atomic-chat compatibility package specs do
not manufacture additional bundled plugin rows.

## Configuration, catalog and local availability

The immutable global configuration namespace is XDG config home plus `kilo`,
normally `~/.config/kilo`. XDG cache home plus `kilo` is a separate namespace.
`KILO_TEST_HOME` affects the trimmed home used for home directories, not those
immutable XDG roots. Explicit path overrides must be confined, absolute and
bounded; variable expansion is never performed.

The server's five global files run in this order:

1. `config.json`
2. `kilo.json`
3. `kilo.jsonc`
4. `opencode.json`
5. `opencode.jsonc`

Later arrays replace earlier arrays. Legacy `config` TOML is read inertly when
present; no import, conversion, deletion or schema write occurs. Later origin
accumulation has a separate last-wins identity rule. Arca retains source
provenance and marks the observed five-file array membership without claiming
the effective session configuration.

The native marketplace detector instead unions plugin declarations from
`kilo.jsonc`, `kilo.json`, `opencode.jsonc`, `opencode.json`, `tui.jsonc`,
`tui.json` and `config.json`. Its installed map is declaration-based and does
not prove cache availability, version, exports or enablement. This exact
detector has no independent install ledger. That observation is not a claim
about every possible Kilo subsystem.

The observer also retains explicit JSON/JSONC configuration references,
global `.kilocode`/`.kilo` and explicit configuration directories, TUI sources,
and direct `{plugin,plugins}/*.{ts,js}` discovery. Private SDK files receive
stat-only observations. A custom override pointing to Markdown, JavaScript or
another non-metadata body stays incomplete and is never opened as JSON.

Only currently registered npm references join the native
`cache/packages/<full spec>/node_modules/<package>` namespace. A bare package
uses its native `@latest` cache spec. Historical caches are not enumerated.
Package metadata can provide its own version; multiple conflicting targets
remain version-unknown. `Local` means observed file availability, while
`Declared` means a registered reference. Neither claims successful installation
or module loading. Unsafe/dynamic URLs and unsupported alias/git dialects stay
opaque declarations without exposing credentials or option values.

## Read-only boundary and evidence

Metadata has its own finite 8 MiB aggregate, 2 MiB document, 512-bound-node,
1,024-entry directory and 64-component ancestor limits. Directory FDs and
stat-only leaves are revalidated before publication. Ancestor FDs provide
consistency anchors and never become filesystem-root watch registrations.
Malformed independent sources do not erase valid siblings.

No private module/Hook/Skill body, account/credential store, database or project
tree is inspected. Commands, arguments, environment, provider secrets and SDK
options are not emitted. Native resources create no AssetRef, import, assign,
delete or toggle action. No SDK hook can become a concrete Host MCP from this
metadata; MCP observations remain with their existing configuration authority.

The initial contract is `eab8aedefb6460ad693834d4c85f7d06a42a3d779711f167f3a22df4b3dde42a`.
The compiled authority supplement is
`76c227e648194cd0d8b4476a5a9bcffb4e96b3b8c22f309f7c5d88c61f82dff1`.
Its 43 bounded views match exact original UTF-16 carrier ranges; REA parsed
10,142 AST nodes and retained 2,623 semantic unknowns. This covers selected
static views, not the complete package or a running client. Production builds,
actual Arca metadata observations and GUI/release acceptance are separate
delivery evidence. Isolation regression source is retained unrun in fast mode.
