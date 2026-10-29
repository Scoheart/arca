# Cline Desktop and CLI native plugin observation

## Separate audited distributions

The inspected Desktop is the official universal `0.0.49` DMG, released on
2026-10-26 under [desktop-v0.0.49](https://github.com/cline/cline/releases/tag/desktop-v0.0.49).
Its app is `Cline`, bundle `bot.cline.app`, executable `cline-app`. Deep strict
code-signature verification passed for Cline Bot, Inc. (`6F2AYU54ZH`); signature
metadata reports a stapled ticket. The annotated release tag resolves to
`975039fd6370ef01859e3fa549b5e74eeec188fb`; its annotation is unsigned.

The independently acquired npm CLI is `cline@3.0.74`, with
`@cline/cli-darwin-arm64@3.0.74`. Official npm SHA-512 integrity and SHA-1
metadata matched both archives. Its native Mach-O signature is ad-hoc,
linker-signed, with no Developer ID identity. The Desktop bundled CLI and npm
CLI are separate artifacts; their SDK dependency versions do not establish
binary equivalence.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `Cline_0.0.49_universal.dmg` | 221952524 | `eb7fc7365073f7011f9400a03437eee97426b7aa8332ae8aeaadd50c691f5bab` |
| Desktop `Contents/MacOS/cline-app` | 41171184 | `2e62b648ac7c9fbd118416e04a94dd5826746b45f61dd75dbade01da5a7e287c` |
| Desktop `Contents/MacOS/cline-cli` | 246004704 | `29efb7b861e094388cb5ea654a7a75c6c8d38e08c843caa96b6ba7065aff107e` |
| Desktop `desktop-backend/index.js` | 13775652 | `6c6107b9547e37d2048070fca08e719394518e15daaa2551db9f1097b0c42986` |
| npm `cline-3.0.74.tgz` | 17865 | `4daea6d75508760b18447d6f00c6bac88ef20af8fe17da4252a90a20fe36c692` |
| npm native platform archive | 38261579 | `75aa9293bf88517634c1bf832abae4c9e311f44929735e3858c0b5ba6667e38a` |
| npm native `bin/cline` | 119787762 | `bd70efb9fb05365143fe0c2d4dd7e809c13631e168ac0abbc62b9f3e62e4ab91` |

The Desktop download's size and SHA-256 matched the official GitHub asset
metadata. Installation copied the already verified signed app into the
user's Applications directory. npm installation used `--ignore-scripts`:
the postinstall can otherwise set aside a running hub's discovery record.
Neither client, its help/version command, plugins, hooks, helpers, native
addons nor login flow was executed during this investigation.

## Shipped source chain

Desktop is a native app with a shipped JavaScript backend. Its actual backend
contains `rut` (default package roots), `UBs` (discovery), `BBs` (v1 manifest),
`$Bs` (skill component), `GBs`/`VBs` (transports), `KBs` (MCP component), and
`tA` (package loading and duplicate precedence). Native JS/TS module discovery
uses `XJ`, `vwe`, `EI` and `eY`. The shipped `@cline/core` ESM export/import
chain was independently resolved to the corresponding SDK loader; an export
named `loadAgentPluginFromPath` actually imports a JS module and is separate
from the data-only `loadAgentPluginPackages` reader.

The npm native image contains a 486-member Bun graph. Its entry is
`/$bunfs/root/cline`; the actual compiled loader is
`/$bunfs/root/chunk-md1sqt8f.js`
(`3789ef204e1957587e56dcf606a76ce530e5e482ca6bc7b0da29a8473ec2b2e0`).
`Fse`, `Bse`, `Gse`, `Kse`/`Vse`, `Qse` and `Ch` implement discovery,
manifest validation, skills, transports, MCP metadata and package loading.
The `zg`/`Nt`/`oe` defaults resolve through its actual import of
`chunk-k3amaccv.js`. Vendored Claude Agent SDK loaders are excluded from this
Cline grammar.

Own inert AST indexing covered 103 Desktop JavaScript files and 445 uniquely
mapped CLI source paths with zero parse failures. Source-bound inspection
projections preserve original member digests and UTF-16 ranges. They use
case-insensitive unique filenames and retain separate Desktop, CLI and SDK
namespaces. The projections are inspection inputs, not reconstructed
executable modules.

REA archive evidence:
`ev_8a94edeb067c554dc98e01191ea81cc540b3a06f82e650105051c4ddc8da1b1a`.
The DMG inventory has 163 nodes, 167 occurrences, 165 edges and zero integrity
contradictions. Its fully parsed export is 415201 bytes, SHA-256
`7cc389d19d29afb15f4fedec52077b57ab3fecfe74c8623ca1123c7cb41668be`.

REA focused parent:
`ev_f9f4649aa22b15b1028ff71cd4ae34efcf28d2f4ad60c48c4dfab635fc330a37`.
It parsed 182 projections, 74591 bytes, with zero parse failures; 4858 semantic
unknowns remain explicit. Its fully parsed export is 24839711 bytes, SHA-256
`ed2fe302c89924e19035b82286cc45f1f5b875b642b889f2c83c9c73ab75fb67`.
Static syntax and source inspection establish the reader contract, not
runtime reachability, activation, policy decisions or successful connections.

## Global paths and two plugin mechanisms

The shipped HOME resolver prefers trimmed `HOME` except `~`, then
`USERPROFILE`, then `HOMEDRIVE` plus `HOMEPATH`, then the OS home fallback.
`CLINE_DIR` overrides `~/.cline`; `CLINE_DATA_DIR` overrides its `data` child.
Arca accepts safe absolute bases and keeps unsupported or ambiguous paths
unavailable. Runtime setters, CLI flags, configured search paths, policies and
a running app's inherited environment can change effective selection. These
observations describe physical global candidates under the current context.

| Mechanism | Default location | Native reader | Arca observation |
| --- | --- | --- | --- |
| Agent Plugins v1 data package | `~/.agents/plugins/<folder>/plugin.json` | Immediate child directories, root manifest, fixed `skills/` and `mcp.json` | Whitelisted manifest metadata, skill filenames, validated MCP names |
| Native JS/TS module | `~/.cline/plugins`, `~/Documents/Cline/Plugins` | Imports module exports/setup and can use `package.json` entry paths | Physical JS/TS file references; contributions and module identity unknown |

The `.agents` root is independent of the Cline directory overrides. Native
package discovery includes hidden child directories. Module discovery skips
hidden entries, `node_modules`, and directories with a root `plugin.json`.
Arca's module references deliberately do not infer executable entrypoint
selection from file presence or read module bodies. Fixed component paths in
v1 packages are independent from this JS module search.

## Agent Plugins v1 acceptance

Both native package readers require strict JSON and the exact
`https://agent-plugins.org/schemas/1.0.0/plugin.schema.json` schema. Names are
1–64 lowercase alphanumeric, hyphen or period characters, start and end
alphanumeric, and contain neither `--` nor `..`. Optional metadata fields
must have the native types. Version is optional and has no native semver
requirement. Unknown root fields and non-object extensions are ignored with
native warnings; unknown author fields invalidate the manifest.

`skills/<child>/SKILL.md` is a physical skill file candidate. Arca leaves
frontmatter acceptance, effective skill name and activation unknown because
it reads no Markdown body.

The fixed `mcp.json` requires the exact v1 MCP schema and only `$schema` plus
an object `mcpServers` at its root. Each server has an explicit `stdio`,
`streamable-http` or `sse` type. The observed registration ID is
`<manifest-name>.<raw-server-key>`; raw keys are not prefixed with Claude-style
IDs or rewritten to match physical folder names.

The observer checks native transport shapes, optional string arguments and
environment maps, reserved `PLUGIN_ROOT`/`PLUGIN_DATA`, relative command and
cwd confinement, HTTP(S) endpoints, HTTPS except native loopback cases, and
case-insensitive unique valid HTTP headers. A missing future relative command
can remain a valid declaration. The data placeholder path is computed from
the data root, manifest name and canonical package-root hash; it is never
created. Executable command strings, arguments, environment values, endpoint
URLs and headers are discarded from the exported metadata.

Arca applies conservative bounds and rejects symlinks, unresolved bases and
path traversal rather than reproducing every native realpath/WHATWG edge
case. Such declarations remain references with unavailable observations.
An invalid server does not erase valid sibling servers or skills; a damaged
MCP component remains a reference. Duplicate manifest names and colliding
dotted registration IDs lose their native IDs instead of guessing the
native locale-dependent first winner. Effective enablement remains unknown.
These checks do not certify general Agent Plugins conformance.

## Runtime binding and product boundary

`cline-desktop` and `cline-cli` keep distinct identities for shared physical
packages. Desktop selection prefers `/Applications/Cline.app`, then the
user Applications directory. It binds Info.plist identity/version and exact
Info.plist, native launcher, bundled CLI and backend bytes. A present invalid
first copy blocks fallback. Its gate does not borrow the npm installation.

CLI binding verifies the complete native image. For the npm wrapper it also
binds the wrapper, certificate helper and package manifest, then follows the
native precedence: nonempty `CLINE_BIN_PATH`, existing `bin/.cline`, first
platform package found while walking parent `node_modules`. A failed first
candidate blocks fallback. Current support pins the macOS arm64 distribution.
File identity is rechecked across each complete gate. The passive CLI version
reader intercepts unsupported images as unknown and suppresses `--version`
execution; it never invokes the certificate helper or postinstall.

Observers use finite read/entry/hash budgets and confined file opens. Watchers
track the runtime guards, global roots and observed component locations.
All output is read-only. Existing MCP/Model/Skill writers and central asset
lifecycle remain outside this observer, with no delete, toggle, assignment,
import, hook execution or login operation added.
