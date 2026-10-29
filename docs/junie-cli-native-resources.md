# Junie CLI native resources

The `junie_cli` adapter belongs only to Agent `junie-cli`. It binds the audited
standalone CLI **3893.6 / release 26.10.27 / macOS arm64**. The JetBrains IDE
plugin, IDE bridge, gateway mode, other channels/builds and other installation
wrappers have separate or unobserved contracts.

`data/native-plugin-junie-cli.json` is the source of truth; every declaration
structure denies unknown fields. The host descriptor uses the single root
`~/.local/bin/junie`, layout `junie_shim`, presence `installed`. Dispatch must
route `junie-cli` to `Some(runtime_version(selected))` unconditionally, so a
passive `None` can never become a target-executing version fallback.

## Public identity and selection

The selected executable must be the known regular shipped shim at
`~/.local/bin/junie`, with the same ordered command selection used by Core's
launcher. The supported managed prefix is `~/.local/share/junie`, with a
single `current` symlink pointing exactly to `versions/3893.6`. It may use that
exact absolute target or its confined relative equivalent. There is no
choose-highest-version, unknown shim, directory-current or outside-link fallback.

The actual shipped shim's priority is `--use-version=`, then JUNIE_VERSION,
then the current-link basename. Channels/manual update/restart handling can
change what a subsequent invocation launches. This observer does not invoke
the shim to resolve them. Nonempty JUNIE_DATA, JUNIE_VERSION, JUNIE_HOME,
JUNIE_INSTALL_BASE_URL, JAVA_TOOL_OPTIONS, JDK_JAVA_OPTIONS or _JAVA_OPTIONS,
an excessive PATH, or a different selected executable fails closed. Actual
argv/JVM properties and active runtime remain unobserved.

The macOS package declares
`Applications/junie.app/Contents/MacOS/junie` → `Contents/app/junie.cfg` →
`junie-release-3893.6-macos-aarch64.jar` →
`com.intellij.ml.llm.matterhorn.ej.app.cli.standalone.MainKt`. Info.plist,
native launcher, CFG, JAR, channel and other small public members have exact
length/SHA256 guards. Bundle ID is
`com.intellij.ml.llm.matterhorn.ej.app.cli.standalone`, bundle/executable name
`junie`, and both bundle versions must match the audited version. CFG must
declare the exact classpath/main class and jpackage app version.

Eight public guards total **181,553,240 bytes**, including the 181,320,085-byte
whole JAR. SHA256 streams through 64 KiB buffers under a private 256 MiB
identity allowance. No large JAR/ZipArchive allocation, Java reflection,
class initialization, module import or target execution occurs.

A bounded 32-entry digest cache binds selected shim/root/current context,
physical member path, device/inode/length/mode/link count/mtime/ctime with
nanoseconds, and expected digest. Each request opens fresh no-follow FDs and
revalidates named-path and FD identity. Both selection and the single symlink
are checked again through final publication CAS. Moving/replacing files,
changing an expected guard, changing the current link or source environment
invalidates the proof. Small semantic identity fields are re-read each time.

`runtime_version` reads only this public identity chain. It never opens the
extension ledger, MCP/account configuration, logs, pending update manifests,
credentials, Keychain or databases. Even informational invocations are unsafe
for observation: the shipped shim sanitizes updates and writes upgrade logs,
creates restart state and launches the JVM; Main also initializes logging and
has a runtime permission helper. These are not called by this adapter.

## Global extension authority and ownership

The standalone project factory resolves `junieHome/extensions` and constructs
`ExtensionsService`. Its current USER registry is
`~/.junie/extensions/extensions.json`. This is a flat map:

```json
{"marketplace":{"extensions":["name"],"source":{"type":"git","url":"public-url"}}}
```

There is no marketplaces wrapper. Each value is an object, extensions is an
optional list of strings (default empty), url is optional/nullable, and source
is optional/nullable. Present typed sources are git(url, optional ref),
local(path), or url(url), using the native `type` discriminator. Unknown fields
are ignored as in the native serializer. Wrong known types fail the entire
registry read; they do not create partially valid installations or an empty
success. Source absence/null does not remove a membership candidate: the
native enabled computation does not gate on source. Source URLs/local paths
are provenance only and are never followed, fetched or watched.

Native qualified IDs preserve the original Kotlin strings and case:
`name@marketplace`. The selected physical cache is exactly
`~/.junie/extensions/<marketplace>/<name>`, whose native construction normalizes
and checks confinement. The observer adds stricter no-follow/single-segment
admission. Listed membership plus a safe materialized directory is `installed`;
missing/unsafe caches retain `declared` provenance and mark incomplete. A cache
without current membership is never enumerated as an installation.

Native `computeEnabledExtensions` iterates USER then PROJECT and later scope
overwrites the same `(name,marketplace)` key. Project configuration is explained
here but never scanned. Neither membership nor a physical cache establishes
the active project, policy, enablement or loading. All observations therefore
use `enabled: null`. Plugin version stays empty/unknown; the CLI version appears
only in revision context. It is not substituted for a plugin's own version.

The bound path does not consult an individual plugin.json for ownership,
version or overrides. Marketplace discovery separately recognizes
`.claude-plugin/marketplace.json` and `.junie-extension/marketplace.json`;
marketplace declarations/indexes do not prove installation. This is a real
Junie extension host, not a claim of complete Agent Plugins v1 compliance.
The native service constructor/refresh/install/migration paths can write,
download or replace caches and are never instantiated or called.

## Resource observations

For admitted global extensions, only the fixed native roots are used:

- `skills/<direct-child>/SKILL.md`: stat-only candidates, directory-name title.
- Direct Markdown files in commands, agents and guidelines: filename metadata.
- Direct `mcp/*.json` and `.mcp.json`: declared names and source paths only.
- `hooks/hooks.json`: stat-only TypeDeclaration reference, never parsed/executed.

Skill/Markdown titles are concise path labels. They do not certify valid
frontmatter, native parsed names, eligibility, enablement or activation;
`native_id` is None. Revisions retain the file-availability/unknown-state
boundary. No Markdown/Skill/hook body, manifest override, arbitrary component
path, project tree or unrelated cache is opened.

MCP JSON uses a bounded native-compatible JSONC subset for comments/trailing
commas and wrapped mcpServers/direct maps. The core Bean fields are command,
args, env, url, headers, oauth and enabled; url is inferred by the native
transport factory (Streamable HTTP with SSE fallback). The shared CLI codec
owns the complete writer/acceptance rules. This observer emits **McpReference**
only, never commands/args, URLs, env/headers/OAuth values or connection state.
It does not follow envFile, auth/credential or URL pointers.

Each declaration source is retained when names conflict. Native directory
enumeration and the later custom/local/project last-wins merge are not observed
by this global plugin view, so stable display ordering is not represented as
an active winner. No component is promoted to a concrete HostMcpObservation.
The conditional `ijembedded` IDE-bridge factory in the JAR is not fabricated
as a shipped/configured/connected server.

Two independent App-owned resources are projected under `@builtin/skills`,
“Junie bundled Skills”, with the owning App's version:

| Known JAR entry | Uncompressed bytes | CRC32 | Local header offset |
| --- | ---: | ---: | ---: |
| agent-skills/session-history/SKILL.md | 5,964 | 1955519926 | 70,335,094 |
| agent-skills/demo-setup/SKILL.md | 12,448 | 1515101278 | 70,337,850 |

They use logical `jar!/entry` source paths and directory-name titles. Their
body/frontmatter acceptance and actual activation remain unknown. They have
no plugin manifest owner and must not be excluded as standalone user-global
resource groups. Native pure Skill projection therefore retains both rows.
The native SkillsService would extract missing bundled resources to
`junieHome/versions/<version>/skills`; this observer does not call that helper,
extract resources or inspect the runtime copy. The whole-JAR guard binds the
audited ZIP metadata directly.

The JAR has 86,176 ZIP64 entries. Its central directory starts at 169,394,899,
is 11,925,088 bytes long, and ends at 181,319,987; EOCD is at 181,320,063.
Those bounds and the two entry records are declarative provenance, not a reason
to traverse dependencies or decompress Skill entries at runtime.

## Budgets, races and evidence

Global metadata/stat observation has a separate allowance: 8 MiB reads,
2 MiB per JSON document, 8,192 visits, 512 retained paths, 1,024 entries per
directory, 512 configured extensions and 4,096 components. Stat-only leaves
are limited to 16 MiB each/64 MiB aggregate. No existing host's shared
ScanBudget is consumed. Malformed/unsafe/exhausted sources mark incomplete
while independently verified bundled resources remain available.

Confined FD/path identity, directory snapshots, leaf stat identity and missing
paths are revalidated before publication. Hardlinked metadata bodies are
rejected. Availability fingerprints include opaque physical metadata stamps
without reading Skill bodies. Watches use the same admission and finite
snapshot; arbitrary sources, project paths, credentials and body parsers are
excluded. Missing registries remain missing: no initialization, linking,
installation, repair, migration or updater runs.

Official release:
`https://github.com/JetBrains/junie/releases/tag/3893.6`.
Archive SHA256:
`917bd6a481f0e0c0dd18abfeb5a6da3e23d5c9c529beeddffbb61742a925ff49`.
Research contract SHA256:
`869dc97df65a1390a8e4f56d56eb80f2743847befdf36baa8a92b7c83da6dea4`.
Evidence is under
`workspace/local/research/arca-native-plugin-audit-20261028/cli/junie-followup-20261030/`:
contract/notes, source-index, class-method-index, shim-source-ranges,
jar-central-directory, bundled-resource-inventory, acquisition/install receipts,
and complete REA export. Parent MCP/OAuth supplements are separate artifact-bound
evidence, including the sensitive clientSecret writer boundary; this observer
only reports names and paths and never emits that field's value.

55 selected original classfiles (598,195 bytes) have exact member/Code hashes,
byte ranges, SourceFile and LineNumberTable bindings for 677 methods. Original
Kotlin sources were unavailable. The inert decoder is not Java execution or a
full JVM verifier; long/multiline ldc literals were omitted and no Skill entry
body was read. Native loader reachability and active state remain unproven.

REA observed only the owned Mach-O/plist/signature metadata, then exported and
closed its session. Junie Evidence IDs are
`ev_957240697f8d729c35f88e332875acdc2851ec11d729537d941896d292350a67`,
`ev_49217aea98083cfb9dddbbd62e3227ba87fc65a0e148545897ec0a3c8b25e03f`,
`ev_e6fb969eb53081d949a98759d122590908abc0e1699731b47c6be61324e4d36e`.
The export's other four historical records are not Junie coverage. Native deep
providers were unavailable; no engine/global runtime was installed or retried.
Official docs returned 403, which is not a negative mechanism finding.

Isolation regressions are preserved in junie_cli/tests.rs but were **not run**.
No test, formatter, validator, build, target CLI/JVM/App/helper, login, GUI or
release operation was performed by the implementation worker.
