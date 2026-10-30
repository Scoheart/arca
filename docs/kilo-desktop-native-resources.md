# Kilo Desktop native resources

The `kilo_desktop` observer reads pinned bundle metadata and fixed default
managed-script file declarations for Agent ID
`kilo-desktop`. It binds the actual **Kilo.app 0.1.14**, bundle identifier
`ai.kilo.desktop`, and its independent **Kilo CLI 7.8.17** child. Standalone
Kilo CLI **7.8.18** has a separate observer and identity. These versions and
loaders must not be interchanged.

It preserves twelve native Desktop plugins, one application-owned builtin Skill,
one gated MCP reference, and stat-only declarations from the fixed managed SDK
directory. It does not claim that this closes every global native plugin
installation mechanism. The distinct authorities and remaining gaps are below.

## Identity and public evidence

The official install page links an Anaconda-hosted current macOS arm64 ZIP.
The acquired ZIP is 478,881,645 bytes, SHA256
`6dffef597c0c7a68ea0d960700bdccf469a77f47f56a86c980b4c83c6f3ea7ce`.
Its URL is a mutable latest alias, so this exact carrier/version is frozen in
the research receipts. The App and its child signature display identify
Anaconda team `Z5788K4JT7`; no deep signing/notarization verdict is claimed.

The observer verifies Info.plist, the actual executable, complete app.asar and
complete bundled CLI with separate stream hashes. The combined whole-file
length is **681,555,968 bytes**. It also verifies the ASAR header and the complete
2,159,592-byte public main entry without allocating a large bundle. Identity
hashing has its own **768 MiB** bound and a 32-entry LRU cache; it cannot spend
another observer's shared scan allowance.

The exact guard data is `data/native-plugin-kilo-desktop.json`. Important guards:

| Member | SHA256 |
|---|---|
| Complete app.asar | `d4a57a08c927247dc7b5ffbe230f483091bebbd47d54721c08a71c9c2f914892` |
| ASAR JSON header | `d81972a41dd9554c4f77e554c83ec9f6ed4319e311d3966a0cafcf12752503f8` |
| Bundled CLI | `28823161786fa39aca8fbfab64b92e2284fa62248c6fab0ac4e7481e07f32a75` |

Info.plist's `ElectronAsarIntegrity` matches that actual header. Package
`@kilo-desktop/desktop`, version, `.vite/build/main.js`, bundle identity and
child version literal must all agree. Unknown versions/guards return no
inventory or passive version. The child version check reads only its fixed
UTF-16 version literal after full-carrier verification; it never starts Bun.

Core's `info_unlocked` supplies the selected default App without nesting a
workspace gate. The selected App must be one of `/Applications/Kilo.app` or
`~/Applications/Kilo.app`; configured targets, args, env and new-instance
preferences cannot inherit this default observation. Fresh confined no-follow
FDs and path stamps are checked before/after hashing and at publication. Stamps
include device/inode, length, mode, link count, nanosecond mtime and ctime.
Regular files with multiple hard links are rejected before metadata reads.
Cache keys also bind exact expected digest and span offset/length. Moves,
selection changes, replacements, upgrades and changed expected digests invalidate
the proof.

## Twelve Desktop plugins and their actual loader

The native Desktop format is `plugin.json` with `id`, `displayName`, own
`version`, `main`, `web`, and optional UI/renderer contributions. It is distinct
from Kilo CLI's JavaScript SDK modules and does not assert Agent Plugins v1
compliance.

The guarded main embeds manifests in `I$`, binds the actual compiled modules in
`G$`, and invokes `fr(G$, "main", ...)`. `fr` validates native shape, reserved
IDs, duplicate IDs and entry/register availability. `J$` orders kilo-server and
kilo-ui before core features and other plugins. `fa(d1)` boots registrations
sequentially and catches per-plugin registration failures. Static availability
does not prove that any callback ran successfully.

The twelve physical manifest IDs are anaconda-account, browser, environments,
files, git, kilo-marketplace, kilo-server, kilo-ui, local-inference, notebook,
terminal and todos. Own manifest versions are retained; they are not replaced
by the App version. Metadata sources use exact ASAR virtual paths, with guard
offsets/digests from the shipped header. UI providers/routes/nav/zones/settings
remain renderer declarations. They are not Agent commands, tools or MCP servers.
Effective enablement and feature/sign-in gates remain unknown.

The fixed child carrier contains the code-defined builtin `kilo-config` Skill.
It is projected through an application-owned `@builtin/skills` group, with the
child version in source context. The logical source is the known Bun asset path;
full-carrier identity binds its table metadata. The observer never opens that
Skill's resource range, decodes frontmatter, or scans user Skills. Its component
native ID is unknown, and external sources may shadow it at runtime. No Desktop
plugin manifest declares Skills; the two dotenv dependency Skills are excluded.

Kilo-server's public code can reconcile a `desktop` MCP template after a runtime
endpoint becomes available. Its URL/token come from generated environment state
and further policy applies. This view retains only `McpReference` name/source;
it emits **zero promoted host MCP servers**, endpoints, headers or credentials.

## Storage and distinct installation authorities

Stable userData defaults to `appData/Kilo Desktop`, with argv overrides and
legacy migration possible. `ca` derives per-plugin storage as
`userData/plugins/<id>`; this storage directory is not installation evidence.
Kilo-server derives `runtime/config/kilo/kilo.jsonc` and
`runtime/kilo-plugins/*.js` inside its storage. The default user configuration
resolver separately chooses `(XDG_CONFIG_HOME or ~/.config)/kilo`.

The selected compiled Desktop registry is a fixed bundle list; storage/cache
does not extend it. There is also a separate managed SDK-module chain.
`gw` lists direct, case-sensitive `.js` names and sorts them; `_w` places
their file URLs in generated `KILO_CONFIG_CONTENT.plugin`. After public App
identity succeeds, this observer stats those direct leaves at:

`~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/kilo-plugins`

Each regular leaf becomes a `Declared` filename reference. Its Arca identity
`@sdk/runtime/<filename>` identifies a physical source, not a native plugin ID.
Version, enabled state, imported exports, owner and installation state remain
unknown. It is not one of the twelve Desktop manifest plugins, and it contributes
no inferred Skills, MCPs, tools or hooks. Symlinks are not followed. A confined
directory FD, its final path identity and leaf stat stamps are rechecked before
publication; changed leaves invalidate the snapshot. At most 512 directory
entries are visited. An unsafe or over-budget managed source leaves the
independent bundle inventory intact and marks the scan incomplete.

The native `tE` install/remove/list-owned methods validate ownership using the
private script prefix produced by `$T(source,id)`. The observer never opens
that header or body. Filename patterns therefore cannot prove ownership or
installed state, even when they resemble `anaconda-<id>.js`.

Marketplace is another authority. `PC` obtains authenticated child
`/kilocode/marketplace` responses, whose schema contains global/project installed
maps and supports type `plugin` as well as agent/skill/mcp. `MC` filters plugin
entries from the Desktop UI; this does not prove the child lacks a plugin
installation ledger. `GC` also combines runtime agent/Skill/MCP APIs. Calling
these helpers may start the child; description helpers may read Skill bodies.
No helper or API is called here. Reconstruction of the child backend's installed
map is an independent CLI follow-up, not a completed Desktop observation.

Default userData is not the active profile: command-line overrides, remote
connections, inherited global child configuration and private state may select
other sources. No arbitrary project, user configuration, private KV/DB, account,
session or component body is read. Startup/install/config helpers can mkdir,
rename legacy data, delete old scripts, reconcile MCP and spawn the CLI; none is
called.

## Runtime environment boundaries

The guarded `XT` server builder starts from its supplied environment (default
`process.env`) plus sanitized contributed variables. Its contribution filter
blocks `KILO_*`, `XDG_*`, loader variables and PATH case-insensitively; HOME is
not reserved and can be contributed. Actual values are unknown.

`KILO_CONFIG_DIR` is overwritten with the per-plugin `runtime/config/kilo`
directory. Inherited `KILO_CONFIG_CONTENT` variants are removed and generated
content may be added. Inherited `XDG_CONFIG_HOME` is preserved, including the
builder's case-insensitive lookup; `XDG_STATE_HOME` is preserved when defined
and otherwise defaults to plugin storage. `XDG_DATA_HOME` and
`XDG_CACHE_HOME` are not explicitly rewritten in this builder. These facts do
not authorize inferring the effective HOME or scanning arbitrary environment
paths. The fixed managed directory above is a default-scope declaration only.

## Research and validation limits

Research is under the private `kilo-followup-20261030` directory of the native
plugin audit. Frozen `source-index.json` binds 17 released CLI TS sources and
31 exact shipped Desktop AST closures to their complete member SHA and source
byte/line ranges. Relevant Desktop functions are fr/hr/gr, J$/fa, fI/pI/mI,
nE/SI, PT and the server/config classes. The full main is retained for manual
source context; selected closure wrappers preserve exact original expressions.

The global-authority supplement adds fourteen exact closures, including
`PC/MC/GC`, `tE`, `gw/_w`, and environment helpers. Its source index binds
them to original main bytes and physical ASAR spans; the prior complete `XT`
class binding supplies the environment construction context.

| Frozen research file | SHA256 |
|---|---|
| `source-index.json` | `cca0f182ac17fafd3ce05db4f029a47aa8d457590fd56bf897bf36ebab296df5` |
| `desktop-global-source-index.json` | `8161854e0fd98c2d41461021f60ec13187eee03ca6dd375a3a82cd989d47fccc` |
| `desktop-global-authority-supplement.json` | `62df9f5ef09e9f0735f807bc9599c24aaf97a11266c08f85b388d6d00c491ce6` |

Desktop REA parent
`ev_ba8a9fdb89aa0ad421ccbbc01eccc2e9ab8a190c47a2463216e7d35a22ea25db`
examined 31 selected closures and 8,311 AST nodes. Coverage is complete only for
that bounded input; semantic coverage remains partial/unknown with 1,907
unknowns. The separate global-authority parent
`ev_ae3e410f1d30b282db182adf200518f213f040578aa99b57a714fffccf10a036`
examined fourteen closures and 3,327 AST nodes, with 918 semantic unknowns.
Complete Evidence was exported for both bounded views; neither proves complete
App semantics. No native target was opened or closed by this worker, and no
helper or factory was evaluated.

Metadata uses independent 1 MiB aggregate reads, 128 KiB per document, 2,048
visits and 1,024 components. Large code/carrier digests stream through 64 KiB
buffers and use separate identity limits. No whole Bun entry is allocated by the
observer. Watch discovery includes fixed public App/carrier paths and, only
after identity validation, the fixed managed directory and observed stat leaves.
It does not watch private native configuration or DB content.

Twenty-two isolated TestHome regression sources are retained. Each stage's
regressions were written before its implementation; none has been run in fast
mode. They include independent source preservation, managed declaration
ownership limits, leaf replacement, symlink rejection and bounded enumeration.
No test, fmt,
clippy, validator, build, target, helper, GUI or installation command was run by
this worker. Root owns production compilation, real-HOME comparison and delivery.
