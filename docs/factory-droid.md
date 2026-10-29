# Factory Droid native resources

This read-only CLI contract was inspected on 2026-10-29 against official macOS
arm64 Droid `0.251.0`. Factory Desktop is a separate target. This increment
changes no writable MCP, Model or Skill contract and adds no native-resource
delete, toggle, import, installation or assignment action.

## Shipped source identity

The [official installer](https://app.factory.ai/cli) selects the versioned
[Droid image](https://downloads.factory.ai/factory-cli/releases/0.251.0/darwin/arm64/droid)
and its [SHA-256](https://downloads.factory.ai/factory-cli/releases/0.251.0/darwin/arm64/droid.sha256).
The 294,010,544-byte arm64 Mach-O matches that checksum:
`d3c0b87ff7cf942ee40a954ffa00204125d29241fe5e4ef458643622b74c25f9`.
Deep strict codesign validation passed for Developer ID Application
The San Francisco AI Factory Inc. (`SW6TL4V6Q5`). Notarization was not independently
established. Neither installer nor target was executed for this investigation.

The Bun graph has 878 members. Its entry is `/$bunfs/root/droid`; plugin storage
and component loading reside in `/$bunfs/root/chunk-4nb7ztsk.js`. Arca guards the
complete image, including encoded JavaScript, bytecode and native companions.
The public source function `ad()` in `chunk-emt6zafp.js` returns `0.251.0`.
Matching images supply that version passively. Changed images and unsupported
wrappers have no active `--version` fallback. Artifact identity does not establish
runtime execution, active code paths or a live session's environment.

## Global directory and registration authority

Native `F()` selects truthy `FACTORY_HOME_OVERRIDE`, otherwise `os.homedir()`.
The production build defaults to `FACTORY_ENV=production`; a present environment
value replaces that default. Production joins `.factory`, development joins
`.factory-dev`. The override replaces the home base, not the full plugin path.
Empty/unknown profiles, relative or unsafe paths and an unresolved home remain
incomplete. Arca does not read dotenv files, account configuration or session
arguments to reconstruct a different process's environment. XDG settings do not
relocate these plugin roots.

Under the selected user directory, the native reader merges:

- `plugins/installed_plugins.json`, with `plugins` mapping IDs to scope records.
- `plugins/installed_plugins/*.json`, each carrying `pluginId` and `entry`.

Registry JSON accepts one leading BOM. This uses a separate shared format;
ordinary JSON, JSONC and JSON5 readers keep their existing grammars. Per-entry
filenames sort lexically by UTF-16 units, including hidden `.json` files. Their
contents supply identity; a filename need not match the native generated hash.
Each valid entry replaces prior records for the same exact plugin ID and scope.
Other scopes survive. Unsupported links or unreadable registration inputs keep
selection incomplete instead of unlocking a legacy winner. No registry is
initialized, repaired, moved or migrated.

An entry requires string `installPath`, `version`, `installedAt`, `lastUpdated`
and `source`, with scope `org`, `project` or `user`. Optional source-hash, npm
metadata and `portableOnly` fields retain their native shape checks. Only org
and user records enter this global observer. Org applicability, source trust
and effective activation remain unknown. Absolute registered paths are
authoritative; arbitrary cache versions and marketplace source checkouts are
excluded. Missing/unsafe packages remain declarations.

IDs retain the native registered `<name>@<market>` form, including scoped names
and pinned market names. The recorded version can be a Git commit or resolved
npm version. Plugin manifest fields do not establish installed identity or
tracked version and are not read. Revision labels distinguish physical paths
and scopes; Arca does not choose a runtime winner among duplicate snapshots.

## Fixed component metadata

The [plugin guide](https://docs.factory.com/harness/plugins) describes a native
`.factory-plugin` layout and copy-time Claude layout translation. Arca observes
the registered destination and never repeats that translation. A manifest or
layout marker is not required by the selected component loader.

- Skills recursively discover child directories containing `SKILL.md` and stop
  below a found Skill root. Private frontmatter/body acceptance remains unknown.
- Commands recursively consider every file, including hidden files and arbitrary
  extensions. Native command names use the basename without its final extension;
  unread metadata does not establish which colliding name is selected.
- Droids and output styles use immediate `.md` files in `droids/` and
  `output-styles/`. Arca keeps filename candidates rather than reading prompts.
- Hooks select a regular `hooks/hooks.json` before regular root `hooks.json`.
  Arca records the selected file without reading events or executable bodies.

Native discovery can follow links with realpath cycle suppression. Arca retains
unsupported links as references and uses bounded, confined metadata traversal.
Hidden component entries are not implicitly excluded. A `portableOnly=true`
record suppresses native MCP and output-style loading; their private bodies are
not read, while other filename candidates remain observable.

## MCP declarations and interoperability

Only root `mcp.json` starts discovery. It uses strict JSON and a `mcpServers`
object. Server IDs are the original keys; Droid adds no automatic plugin prefix.
The entire map is validated atomically. A malformed/unsupported server, invalid
permission metadata or unsafe selected file leaves MCP references instead of
promoting the remaining entries. Supported static checks include stdio, HTTP and
SSE shapes, string argument/environment/header maps, tool lists, timeouts, OAuth
fields and their combinations. OAuth is metadata here: credentials, URLs,
headers, arguments and policy values are never returned or used for a connection.

A `$schema` beginning `https://agent-plugins.org/schemas/` in `mcp.json` selects
`.mcp.json` when present. That fallback can contain a wrapped or direct map.
An isolated `.mcp.json` is not an implicit source. This prefix check establishes
interoperability behavior, not Agent Plugins manifest/version conformance.
No other layout is translated during observation and no builtin MCP is invented.
Duplicate physical plugin identities or raw MCP keys lose callable-namespace
claims rather than manufacturing a winning definition. Other native settings
may also affect final MCP selection and are outside this read-only inventory.

## Evidence and limits

All 775 `.js` chunks parsed as inert syntax: 24,219,414 derived UTF-8 bytes,
5,585,576 AST nodes, zero failures. Entry/native/resource/bytecode members retain
separate identities. Fifty-five exact source projections were also analyzed
with REA (25,571 bytes, 7,029 nodes). Full evidence was exported and validated
before session closure. Semantic coverage retains 1,825 unknowns. Later OAuth
and permission details were traced directly in their original source members.
These observations do not certify whole-program semantics or runtime behavior.

Effective enabled IDs, scoped settings, source provenance, managed policy,
account applicability and selected sessions remain native-owned and unobserved.
Global cache presence does not establish activation. No real account/settings,
Keychain, component body or plugin executable was inspected for this research.

Thirteen initial isolated regression cases preceded implementation. They cover image
identity, overlays/scopes, hidden files, fixed components, raw namespace clashes,
portable restrictions, BOM separation, MCP atomic validation/fallback, OAuth,
home selection, watch inputs and unsafe links. Under project fast mode the
sources are retained without executing suites. A further source-review case
retains physical metadata under an unverified registration overlay without
claiming current MCP bindings. Production builds and real-data
preservation checks are recorded separately.
