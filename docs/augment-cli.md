# Auggie native resources

The read-only plugin contract was inspected on 2026-10-29 against the official
`@augmentcode/auggie` npm distribution, version `0.36.0`. It extends the existing
`augment` Agent without changing its global MCP/Skill writers, codec or assets.

## Source identity and passive version

The [official npm metadata](https://registry.npmjs.org/@augmentcode/auggie/latest)
selects `augment.mjs` as the `auggie` entrypoint. The 4,100,903-byte tarball matches
its published SHA-512 integrity value. This establishes registry content identity,
not an independent publisher signature or the integrity of a running Node process.

Arca verifies both original package inputs before associating native facts:

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `augment.mjs` | 13,256,802 | `6229e1685a5c4f9305ad333d76cd1a05c4315e6a9c8af3bbc75170b524005306` |
| `package.json` | 492 | `0800a1c082237d4b497bc8e7c01fade746553b9f29e8ac1c5dbede45a7108b03` |

The public source contains an immutable build stamp for release `0.36.0`, commit
`7c61e5bb`, dirty `false`. Bootstrap replaces an initial placeholder version with
that stamp; its native formatter yields `0.36.0 (commit 7c61e5bb)`. Arca supplies
this source-derived label passively. A selected alias must resolve to the audited
entrypoint and both files must match. Unsupported wrappers or changed packages
provide no active `--version` fallback. No target, helper or installer is run.
The guard does not establish Node preload resolution or the full runtime closure.

## Native marketplace discovery

The [official plugin guide](https://docs.augmentcode.com/cli/plugins) and selected
loader establish these user-global observations:

- Native home uses nonempty `HOME`, then nonempty `USERPROFILE`, then the OS home.
  The marketplace suffix is `.augment/plugins/marketplaces`. General XDG settings
  do not relocate it. Relative or unresolved overrides stay incomplete.
- The root can itself be a marketplace; otherwise its nonhidden child directories
  are candidates. `.augment-plugin/marketplace.json` precedes
  `.claude-plugin/marketplace.json`. The first existing manifest masks the second.
- Marketplace and plugin manifests and root `.mcp.json` use JSON5. The shared
  metadata reader adds a dedicated `json5` dependency, bounded bytes and 128-level
  structural nesting; existing JSON and JSONC grammar stays unchanged.
- A local entry first resolves `metadata.pluginRoot` plus its source, then the
  source relative to the market. The first existing directory or link is selected
  before native realpath containment checks. Arca normalizes contained parent
  segments, never reads outside a market and preserves unsupported links as
  references. An unverified first candidate cannot unlock a fallback candidate.
- URL entries use `.plugin-cache/<sanitized-entry-name>` and require an existing
  `.git` marker. File or directory markers are sufficient; their contents are not
  read and Git is never invoked. Missing caches stay declarations, complete
  caches stay cache-only, and URLs, refs and credentials are not exported.

Native plugin identity is `<entry-name>@<market-name>`, independent of a plugin
manifest's display name. Raw bounded names are preserved rather than trimmed into
different identities. Revision records physical market/package locations; it is
not a Git commit. Duplicate physical identities retain separate snapshots and
lose callable MCP claims because native enumeration/session precedence is unknown.

## Fixed components and MCP declarations

The optional plugin manifest uses `.augment-plugin/plugin.json` before
`.claude-plugin/plugin.json`. Missing or malformed optional metadata does not
suppress fixed component discovery. Its version is metadata only. The selected
loader discovers recursive `.md`/`.txt` Commands, Agents and Rules, recursive
`.md` Skills and `.json` Hook files. Hidden children and child symlinks are skipped.
Arca records filenames without reading Markdown, frontmatter, scripts or hooks;
a candidate Skill filename does not prove the native body parser accepts it.

Only root `.mcp.json` supplies MCP metadata. Native IDs use
`<plugin-id>/<raw-server-name>`. Standard string stdio/HTTP/SSE declarations retain
their namespaces. Coercion-dependent, unsupported or malformed shapes remain MCP
references; a throwing stdio argument shape demotes the whole file. Arca never
expands native root placeholders or exports command, argument, environment,
header, URL, client ID or working-directory values.

The inspected loader does not consume custom contribution paths or inline MCP
fields from optional plugin manifests, even where newer documentation describes
them. Such claims are not projected into components. No builtin MCP server is
inferred and no Agent Plugins schema/version conformance is established here.

## Context and verification boundaries

CLI bootstrap can initialize settings and update Git-backed markets before help
or version parsing. Arca never invokes that path. Extra `--plugin-dir` roots,
project settings, managed policies and session arguments are excluded. The CLI
startup context, SDK feature gates and plugin-management command have different
selection paths. Positive settings entries alone cannot establish effective
enablement, so every observation keeps it unknown. Nothing here adds delete,
toggle, import, installation or assignment actions to native resources.

An inert whole-file syntax index parsed 2,176,828 AST nodes without failures.
Two separate focused REA analyses cover eight mechanism projections and three
build-stamp projections. Their full evidence exports were validated; semantic
coverage still reports 1,230 and 69 unknowns respectively. Selected projections
and source-derived names do not establish whole-program or runtime coverage.

Ten isolated regression cases were written before implementation for identity,
JSON5 grammar/bounds, paths/precedence, cache markers, duplicate identities, MCP
shapes, unread bodies, home selection and watches. Under the project's fast mode
these sources are retained without executing the test suites.
