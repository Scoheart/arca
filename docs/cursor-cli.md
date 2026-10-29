# Cursor CLI native resources

The independent CLI contract was inspected on 2026-10-29. It is separate from
Cursor Desktop plugin policy and from Arca's existing global MCP/Skill writers.

## Package identity

The [official installer](https://cursor.com/install), linked from the
[CLI installation guide](https://cursor.com/docs/cli/installation), supplied
`2026.10.22-27dcf5f` at inspection time. Its official macOS arm64 archive is
`https://downloads.cursor.com/lab/2026.10.22-27dcf5f/darwin/arm64/agent-cli-package.tar.gz`:
180,412,627 bytes, locally computed SHA-256
`2f5b0a3dd5fb951cecffac813867d8a08d810e59837d59fd5f12d69b922398d7`.
The reference was retained separately, without replacing an existing CLI or its
command links. No independent upstream checksum over the entire archive was supplied.

Arca binds the launcher, main source, package metadata and both relevant source
carriers. These are source identity guards, not a whole-program integrity or
runtime-execution certificate. The primary launcher uses neighboring Node and
index.js; direct SEA entrypoints remain separate and unverified. No CLI, target
Node, native addon, helper, plugin, hook, initializer or login is executed.

The older `2025.09.17-25b418f` installation has an independently guarded passive
version label. Its entry was parsed and searched within explicit bounds, but
its native plugin contract remains unverified. It does not inherit the reference
package's capabilities. Only `cursor-agent` is a Cursor probe: the generic
`agent` command can belong to another product.

## Global plugin metadata

The shipped module keys `../cursor-plugins/dist/index.js` and
`../local-exec/dist/index.js`, with startup wiring in `./src/shared/resources.ts`,
establish independent CLI behavior:

- Complete marketplace snapshots use `~/.cursor/plugins/cache/<market>/<id>/<version>`
  and `.cache-complete`. Arca retains all physical snapshots without ranking an
  active version or reconstructing the server's normalized cache key.
- User-local entries use `~/.cursor/plugins/local`. Contained links can be native
  candidates, while Arca retains linked or unsupported entries as references
  without following them. Incomplete cache candidates also remain references.
- These roots come from the CLI's explicit home directory, independently of
  general `CURSOR_CONFIG_DIR` and `XDG_CONFIG_HOME` configuration overrides.
- Effective-user APIs, local marketplace enablement, project settings, team
  policy and gated Claude imports determine session selection. Partial source
  failures can retain older in-memory state. Desktop SQLite state is not a CLI
  activation authority. All effective enablement remains unknown.

Manifest precedence is `.cursor-plugin/plugin.json`, `.claude-plugin/plugin.json`,
then root `plugin.json`. The native parser recognizes Agent Plugins 1.0.0 schema
URLs but logs and continues on unknown schema IDs. Cursor-native fields coexist
with the standard format; this is compatibility evidence, not strict conformance
certification. See the [official plugin guide](https://cursor.com/docs/plugins).

Both `mcpServers` envelopes and direct JSONC server maps are metadata sources.
Fallbacks are `.mcp.json` then `mcp.json`, with explicit manifest paths/arrays/
inline declarations also observed. Arca does not expand variables, connect,
materialize a package or reproduce native configuration winners. Non-JSON
pointers, invalid shapes and unsafe paths remain incomplete observations.

Native IDs use `plugin-<sourceInfo.name>-<server-key>`. For local imports the
source name is the entry folder, which can differ from the manifest's name.
Cached declarations keep MCP-reference roles because the selected backend
source name is unknown. Skill, rule, agent, command and hook entries are authored
filesystem markers; Markdown/script bodies and runtime acceptance are not read.
No command, argument, environment, header, auth or private account value is
returned. Native observations have no delete, toggle, import or assignment action.

## Bundled computer-use declaration

The CLI's `enableMcp` and `computer_use_mcp_cli` gates control a bundled source
whose server ID is `computer-use`. Its helper module key
`../computer-use-mac/dist/index.js` occurs in both `3863.index.js` and
`2478.index.js`; runtime factory selection was not observed. The registration
source is `./src/shared/resources.ts` in the former carrier.

The native config factory can fetch a channel manifest, download and validate a
ZIP, extract, check bundle identity/signature and install or replace
`~/.cursor/agent-helper/Cursor Agent Helper.app`. Arca never calls it. The
compiled declaration does not prove helper installation, permission, activation
or a running MCP connection.

## Analysis limits and validation

Complete source ranges were bound to original package hashes and byte/UTF-16
locations, with explicit syntax prefixes for static projections. REA parsed five
initial module projections (442,432 bytes, 119,439 AST nodes, zero failures).
Its graph is partial with a 100,000-node ceiling and 17,621 semantic unknowns;
hook/bridge static facts remain retained. A focused followup parsed four class/
factory projections (23,803 bytes, 5,820 AST nodes, zero failures), with complete
application coverage only within those projections and 1,256 semantic unknowns.
Neither analysis certifies the full CLI or observes execution. A zero exported
unknown count is bookkeeping, not semantic completeness.

The canonical five-record evidence bundle was fully parsed and its analysis
identities validated before session closure. Regression source precedes the
implementation and is retained without suite execution in project fast mode.
Existing codecs, native writers, central assets and lifecycle rules are unchanged.
