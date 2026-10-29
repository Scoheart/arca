# Pi native package and resource observation

## Audited distribution

The installed CLI is `@earendil-works/pi-coding-agent@1.2.0`, whose npm bin
is `pi` → `dist/bundle/cli.js`. The official [npm version metadata](https://registry.npmjs.org/@earendil-works%2Fpi-coding-agent/1.2.0)
and [tarball](https://registry.npmjs.org/@earendil-works/pi-coding-agent/-/pi-coding-agent-1.2.0.tgz)
were acquired independently. SHA-512 SRI and SHA-1 matched official metadata.
All 1275 ordinary archive members, 23199269 unpacked bytes, matched the
installed package byte for byte. npm signatures and provenance attestations
were not independently verified.

The archive is 7568475 bytes, SHA-256
`1dfed308f46a69aec06bf9dc20700ddc6fe2370b1898f01334b7898397541e66`.
The source repository is [earendil-works/pi](https://github.com/earendil-works/pi),
under `packages/coding-agent`. Public [package](https://pi.dev/docs/latest/packages)
and [extension](https://pi.dev/docs/latest/extensions) documentation corroborate
the installed artifact. Those latest-documentation URLs are moving sources;
the inspected 1.2.0 distribution is the implementation authority here.

| Bound member | Bytes | SHA-256 |
| --- | ---: | --- |
| `package.json` | 3767 | `d4231a02b6a2ccb357a8e9e420fab9c217490137ade958d4a45a12e169daad0c` |
| `dist/bundle/cli.js` | 160 | `e79626f2dd6f94aa45d30f3fa63cd84319a6eefcd150b353cfaf274366926774` |
| `dist/bundle/cli-runtime.js` | 1015 | `0a7ba07c94a89733a5ace8eac6bced09c20ed099d153b02cfaa878202b74878f` |
| `dist/bundle/chunks/chunk-TJXZSVTD.js` | 3510500 | `3aced1320c6c7a36ff53be75b539099310971e9b54b15df5b1efba3d2be75c10` |
| `dist/bundle/chunks/chunk-WJUNYZC5.js` | 28659 | `9ef22c5791095c0c720064c043da184f01e0d569598dd87d83b742e95a164d13` |

## Actual bootstrap and static evidence

The 160-byte entry enables Node's compile cache and requires `cli-runtime.js`.
That runtime imports `main` and `configureHttpDispatcher` from the bound
`chunk-TJXZSVTD.js`, and `APP_NAME` through `chunk-WJUNYZC5.js`, before
configuring the process and calling `main`. Even `pi --version` traverses
that bootstrap. Arca therefore uses passive identity checks for the version
label; an unsupported Pi identity returns unknown without an execution
fallback. No Pi entry, version/help command, module, extension, hook, helper,
native addon or package manager was executed in this investigation.

Own inert AST indexing covered all 334 JavaScript members, 11916502 bytes
and 1854225 AST nodes, with zero parse failures. The actual bundled package,
settings, resource and extension loaders retain named source bindings.
116 source-bound inspection projections preserve member digests and UTF-16
ranges, with case-insensitive unique filenames. Method wrappers only make
fragments parseable; they are not reconstructed executable modules.

REA focused parent:
`ev_4cb46d08f1374cde04c1247ed61c90c506ac818c9584517d308be88f549ec466`.
Summary:
`ev_87e1fcc8763ae17a4fc27cc51f544ab10537e24fb408e92afe139f5051ff8334`.
The projection root digest is
`4384cbaf127cec865404295efbed928c17342fbcc6f93a68eb4a2369ca908cd8`.
REA parsed 116 projections, 99035 bytes, with zero parse failures and 4672
explicit semantic unknowns. Its fully parsed two-record export is 25265015
bytes, SHA-256
`cf0ec27a438fb9bdc853f20de8b96c7aab8c04df9ca9242248554fa69a26013f`.
Static syntax and reader inspection do not establish execution, successful
imports, parser acceptance, effective activation or live MCP connections.

## Compiled extensions

Actual `builtInExtensions` declarations enter `main`'s extension factories
and the default resource loader's builtin resolver.

| Native ID | Extension | Native replacement flag |
| --- | --- | --- |
| `builtin:llama.cpp` | `llama.cpp` | Not declared replaceable |
| `builtin:codemode` | `codemode` | Replaceable |
| `builtin:tool-search` | `tool-search` | Replaceable |
| `builtin:mcp` | `mcp` | Replaceable |

All four are bundled extension declarations with unknown effective
enablement. Global and project configuration, CLI selection, disabled
builtins and replacement factories can change the selected session.

`mcp_default = createMcpExtension()` supplies an MCP client integration.
Servers come from configuration and from the extension API's
`registerMcpServer(name, config)` runtime registry. Ownership and namespace
checks happen at runtime. The factory is not a fixed server catalog: this
observer emits no MCP components or HostMCP records for it, and does not
infer dynamic server names by reading or executing private extension code.
Existing external MCP configuration observation remains independent.

## Global registration and paths

The public manifest sets `piConfig.configDir` to `.pi`. `getAgentDir()` uses
a nonempty `PI_CODING_AGENT_DIR` override, otherwise `~/.pi/agent`.
`settings.json` is strict, BOM-aware JSON. Arca projects only `packages`,
`extensions`, `skills`, `prompts` and `themes`; unknown fields and private
values never enter resource observations. The current native reader's
`skills.customDirectories` input is projected read-only, without invoking
its settings migration, lock or write operations.

| Global input | Native location or identity |
| --- | --- |
| Package registrations | `~/.pi/agent/settings.json` → `packages` |
| Managed npm materialization | `~/.pi/agent/npm/node_modules/<name>` |
| Managed Git materialization | `~/.pi/agent/git/<host>/<path>` |
| Automatic resource directories | `~/.pi/agent/{extensions,skills,prompts,themes}` |
| Shared user skills | `~/.agents/skills` |
| Additional resource paths | Four resource arrays in global `settings.json` |

Registrations may be strings or objects with `source`, resource filters and
`autoload`. npm identities are `npm:<name>`, Git identities omit the ref and
one terminal `.git`, and local identities retain the resolved physical path.
The first duplicate global source identity wins. Arca associates only those
registered managed locations, never unregistered cache history. Existing
local files are extension candidates; local directories retain their package
resource metadata or an unresolved extension-directory reference.

For npm, the installed version must satisfy a supported faithful subset of
the native npm range grammar. Missing materializations, mismatches and
unsupported compound ranges remain declarations. Native startup can install
packages and consult a legacy global npm location; Arca performs neither
operation and does not invent a global npm root. Git ref freshness, package
installation success and project overrides remain unknown.

Safe global paths resolve relative to the agent directory, with tilde and
file-URI handling. Unresolved relative environment bases are rejected.
`PI_PACKAGE_DIR` is accepted only when it resolves to the already bound
public package root. Other roots and unsupported host versions fail closed.

## Manifest and resource candidates

Pi's native package format is `package.json` with a `pi` object and string
arrays named `extensions`, `skills`, `prompts` and `themes`. It does not
require the Agent Plugins v1 schema or define a fixed `mcp.json` component.
This is a Pi-specific adapter, not a general Agent Plugins conformance claim.

Native fallback differs by registration shape. For a string registration,
even `pi: {}` suppresses all conventional directory fallbacks. For an object
registration, an omitted resource filter uses a declared manifest array,
including an empty array, or falls back to that conventional directory when
the field is absent. An explicit resource filter uses nonempty manifest
entries or the conventional directory, then narrows native selection. It
does not discover arbitrary files outside the package's resource grammar.
`autoload: false` without filter entries contributes no candidates.

Arca retains literal confined manifest paths and physical filenames. Module
discovery follows the automatic reader's literal `pi.extensions` entries,
then `index.ts` before `index.js`, then one level of visible JS/TS files and
child module entries. Skill discovery respects a regular `SKILL.md` marker
and the distinct Pi/shared-skill traversal modes. Package or explicit prompt
and theme directories recurse; automatic global prompt/theme directories
include only immediate files.

Glob/override patterns, package filters and ignore-file markers remain
references. Arca reads no private Markdown, prompt, theme, module or ignore
body, and does not guess effective resource names or visibility. All such
component native IDs and session enablement remain unknown.

## Read-only boundary

The selected CLI path, all five complete public guard files, file identities
and override binding are checked without bootstrap execution. Metadata
reads have an independent finite Pi budget in full and per-Agent queries,
so unrelated preceding observers cannot change Pi package presence. Reads
are confined; symlinks, escapes, invalid metadata and
unsupported inputs remain unavailable references. One failed component or
registration does not erase valid siblings. Runtime acceptance remains
unknown even when a physical candidate exists.

Watch targets come from the same confined scan: the settings file, package
manifests and approved resource paths. Neither the whole agent directory
nor a package root is added merely because it owns resources. The observer
does not walk sessions, accounts, credentials or unregistered caches.

No private configuration or component body is copied into evidence or
documentation. This integration changes no MCP/Model/Skill writer, central
asset lifecycle, codec or frontend. Observations have no edit/delete action.
Eighteen isolated regression cases are retained; fourteen preceded the
implementation. Suites remain unexecuted under the project's fast mode.
