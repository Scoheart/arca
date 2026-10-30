# CoCo CLI native resources

The `coco_cli` observer covers the selected official macOS arm64
**1.2.24+183154.baac4cb1308e** distribution. It retains the existing Arca
identity `cortex-code` and command `cortex`. [Cortex Code Desktop](coco-desktop-native-resources.md)
has a different application, manifest parser, registry and Skill scanner.

CoCo accepts its native `.cortex-plugin/plugin.json` and an external
`.claude-plugin/plugin.json` format. This does not certify Agent Plugins v1
conformance. Native observations have no import, assignment, toggle, deletion
or component-body operation.

## Artifact and passive version

The official archive is 196,470,986 bytes, SHA-256
`8b8cd88476ff8d7e14486832c0982519d11c9b7da3608035ee6ecc7489b7a64f`.
Its checksum matches the fixed release manifest selected through the official
installer's stable-version endpoint. The installer was read as inert text;
its shell-profile edits, replacement and `--help` smoke test were not invoked.

The signed carrier is 142,613,520 bytes, SHA-256
`a1b4c7faee074cd206f7c464392710a02c68e2e83a8558b894df7323434d9f39`.
The research signature identifies Snowflake team `W4NT6CRQ7U` and
`com.snowflake.cortex-code`; strict verification passed without launching it.

This is a Node SEA image. The 30,935,782-byte SEA section starts at original
file offset 85,409,792. The 30,935,634-byte JavaScript main starts at
85,409,940 and has SHA-256
`8f4a2d067335d5364b8cceb5f499bf70978d4f2df4448bd401f53f3e38fb47a9`.
Node's upstream 22.22.0 SEA serializer and embedding entry establish the
layout and `__filename`/executable relationship. The recorded build code path
is not substituted for the installed executable path.

The selected runtime's `Wyy` reads adjacent `cortex-version.txt` through `wB`.
Arca binds both complete files, the SEA header, selected alias and physical
paths, single-link regular files, directory/file FDs and nanosecond stamps.
The independent 256 MiB streaming limit and finite 32-entry digest cache do
not spend another observer's allowance. Path replacement, permission changes,
changed bytes or a changed selection invalidate the proof.

Custom Arca targets/arguments/environment and nonempty `COCO_DIST_DIR`,
`NODE_OPTIONS` or `NODE_PATH` do not inherit the default proof. Unknown images
remain version-unknown. Neither the target, Node bootstrap, dependencies,
installer, helper nor a version/help flag is executed by the observer.

`data/native-plugin-coco-cli.json` separates runtime identity guards from the
four audited manifest records and the original 89 Skill candidate paths.
Those inventory records reproduce the official archive; they do not assert
that today's mutable resource files still have their original contents.

## Native discovery authority

The shipped CLI plugin-list handler calls `hqe`, which constructs `ACr` with
`bCr`; runtime refresh uses the same `bCr`. The separate `w2e.discoverPlugins`
method is not used as a substitute for that selected inventory authority.

`bCr` keeps the first plugin with each manifest name, in this order:

1. Explicit CLI plugin directories.
2. Active connection/profile references.
3. `settings.json` plugin references.
4. Physical managed-plugin directories, joined with registry metadata.
5. Trusted project plugin directories.
6. Internal bundled plugins, subject to feature gates.
7. Bundled external plugins.

The selected code places physical managed directories before project plugins.
This resolves the conflicting project-over-managed example in the current
public documentation for this carrier only. Arca observes global source
facts; it does not reconstruct CLI arguments, private profiles, project
trust, name shadowing or effective session membership.

The global namespace is `CORTEX_HOME` when nonempty, otherwise
`SNOWFLAKE_HOME/cortex`, normally `~/.snowflake/cortex`. Relative overrides
depend on runtime CWD and remain unavailable in this global observer.
No native getter, migration, activation stub, data-directory creator, updater
or registry writer is called.

The registry is `plugins/registry.json`. Its top-level object requires each
row to have nonempty string `source` and `installedAt`, plus boolean `active`.
A malformed row makes the native registry reader fail the document. Empty
content is valid; a BOM is not silently stripped. Arca retains an incomplete
observation while still publishing independently valid physical resources.

Direct nonhidden plugin directories are also native discovery authorities.
They are not all cache-only. A registry join first checks directory name,
then manifest name. Registry membership, physical availability, requested
activation, loaded state and current permissions remain distinct facts.
Unavailable registry entries appear as declarations without installation or
version guesses. Remote source values and private update errors are not
rendered or resolved.

Internal bundled plugins default to requested-active and do not join the
registry in their own discovery tier. Bundled external plugins default to
requested-inactive and do join it. A higher-priority physical plugin can
still shadow either. Requested flags are kept in details; `enabled` remains
unknown because policy, harness, profile and the running session are unseen.

## Manifest and component observations

The actual loader selects the first existing manifest, preferring
`.cortex-plugin` to `.claude-plugin`. A malformed preferred manifest does not
fall through. A separate identity pre-read has different error behavior and
does not replace the loader's rule. Names must be lowercase kebab-case;
versions come from each manifest, independently of the CLI version.

Commands, Skills and agents accept string or string-array sources. An invalid
shape falls back to the conventional directory; an explicit empty array
does not. Arca retains command/agent source references and stat-only Skill
candidate leaves. It does not parse their instruction bodies or infer IDs
from frontmatter.

Hook sources are metadata references. Safe inline or JSON MCP declarations
expose server names as `McpReference`, with the native `pluginName-serverKey`
identifier. Commands, arguments, environment, URLs and credential values are
not emitted. Placeholder expansion, plugin data creation, native environment
expansion, merge precedence and administrator policy are not executed or
claimed. No reference is promoted to a concrete Host MCP.

Component metadata reads reject non-JSON inputs, Hook/account/credential paths,
escaping or dynamic paths, and paths crossing declared component-body roots
before I/O. File leaves remain stat-only. Every metadata snapshot has finite
byte, document, entry, ancestry and component bounds, with FD/path revalidation
before publication. Ancestor anchors do not become root filesystem watches.

## Bundled Skills and resource paths

The selected package has two internal manifests (`code-review`, `data`) and
two bundled external manifests (`databricks`, `blueprints`). Their own
versions are `0.1.0`; this does not describe account availability or activation.

The application-owned `bundled_skills` tree has 554 raw `SKILL.md` leaves.
The native scanner first considers nonhidden/non-`__` top-level directories.
For each root it uses its own `SKILL.md` if present, otherwise only direct
child directories with that filename. It does not recursively descend into
fixtures and reference trees. The selected artifact consequently has **89**
application-owned candidate roots. Desktop's scanner is different.

These candidates appear in Skills with built-in and CoCo source labels.
Plugin-owned candidates retain their actual manifest owner. Body validity,
frontmatter names, native name deduplication, user overrides, experimental and
account gates, disabled settings and invocation remain unknown.

The native `Joe` resolver tests development/adjacent/parent/`_internal` paths,
then shared install locations and immediate installed-version entries.
Arca snapshots ordered fixed candidates instead of assuming adjacency. Later
version fallback is accepted only when one possible resource source exists;
ambiguous enumeration remains unknown. It never assigns an unrelated cached
resource to the selected version merely because a similar directory exists.

## Evidence and validation boundary

The initial research contract is
`d67d7dafea7c96ad72880d970c1b29f8771bca11033ae59c7fb97f5c7e88c94a`.
Its exact loader follow-up is
`6dbadee08a404d9c0ee29e5a17c5d0d8045128f08129e68eea0379c32409ea8d`.
Complete function and entry views match original UTF-8 carrier ranges.

REA analyzed three bounded view sets: 28 initial files, 18 loader dependencies
and 17 path/Skill/entry dependencies. The initial semantic view has explicit
resource-limited coverage; the two focused sets have complete parse inventories
and unresolved semantic relationships. This is selected static evidence, not
whole-package or running-client coverage. No Markdown leaf/frontmatter,
private account, credential or database was opened for research.

Isolation regressions remain as source and are unrun in fast mode. Production
builds, actual Arca metadata queries, GUI acceptance and release verification
are separate delivery evidence.
