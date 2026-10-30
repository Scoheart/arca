# CodeBuddy CLI native resources

Agent `codebuddy-code` uses the official `@tencent-ai/codebuddy-code` npm
distribution, audited at **2.178.1**. The two desktop IDE Apps have separate
identities and loaders; see [the desktop contract](codebuddy-desktop-native-resources.md).
Recognizing Claude-style metadata directories does not establish complete
Agent Plugins v1 compliance.

The selected default command must resolve to the verified `bin/codebuddy` in
one physical package. Ten public member guards bind package metadata, the
shipped entry, all three TUI/headless/WorkBuddy-lite bundles and all five
published product profiles. The 74,319,704 bytes stream through an independent
128 MiB identity budget, with a finite 32-entry FD-stamp/digest cache. A changed
member, expected digest, alias, parent directory or selected launcher invalidates
the proof. Unknown selections never execute a version fallback.

The real wrapper has a `--version`/`-v` fast path before bundle loading. Optional
profiling and Node compile-cache setup occur before that path, so Arca never
calls it. Package version is read passively. The source-bound version route
rechecks the member cache rather than relying only on the entry's timestamp.
Nonempty Node preload/search-path overrides remain unobserved. Custom Arca
launch targets are not silently replaced with an official default.

The native home resolver prefers a product `dataLayout.configDir.userDirName`,
then nonempty `CODEBUDDY_CONFIG_DIR`, then `~/.codebuddy`. The pinned public
profiles do not declare that layout override. The observer accepts a confined
absolute home override; relative homes or custom product configuration stay
unknown. It never reads a remote/current product snapshot, account store,
credentials, Keychain, database or project tree.

`<home>/plugins/installed_plugins.json` is current registration authority. The
native writer emits version 2 with a `plugins` map whose `name@marketplace`
values are scope-specific records. The current reader also normalizes array
records and its documented field aliases. Arca independently handles these
native inputs; it does not invoke migration, repair or initialization. Empty
installation paths are ignored as in the native reader, and an explicit
`pluginId` overrides a map key even when it is invalid. Only user/managed global
records can be followed. Project/local records and arbitrary source locations
are not scanned.

Current records must join safe physical caches under
`<home>/plugins/cache/<sanitized marketplace>/<sanitized name>/<version>`.
Unregistered historical caches do not become installed plugins. A referenced
missing, unsafe or invalid cache stays a declaration. Registered versions keep
their own version metadata; host version is never substituted. USER/PROJECT/
LOCAL/CLI/session/policy precedence is not observed, so effective enablement,
the selected runtime version of a plugin, loading and activation remain unknown.

The native manifest preference is `.codebuddy-plugin/plugin.json`, then
`.workbuddy-plugin/plugin.json`, then `.claude-plugin/plugin.json`. A malformed
preferred manifest does not justify a lower-priority fallback. This CLI order
is not the desktop IDE's format order.

Commands, agents, Skills and output styles expose bounded filenames and confined
declared paths. Skill candidates require `SKILL.md`; an arbitrary Markdown file
is not a native Skill. Titles are concise filesystem labels and `native_id` is
unknown because frontmatter and bodies are never opened. The audited plugin
extension enum does not contain Rule: a `rules/` directory is not manufactured
as a supported plugin resource.

MCP and LSP declarations expose names/references only. MCP JSON, inline maps and
independent source files retain their own provenance, including conflicts; no
runtime merge winner or concrete Host MCP is invented. Commands, arguments,
URLs, environment, headers, OAuth and option values never enter the observation.
The shared opaque-payload policy prevents an override from reclassifying a
Hook, credential/account payload or non-JSON component body as metadata.
Explicit Hook file declarations are also opaque. Such paths remain unresolved
filename references, with incomplete coverage, while other independent valid
MCP sources can still be observed. Hooks/client/settings/options declarations
never cause a body read, module import, substitution or lifecycle execution.

One application-owned group projects the packaged `mods-create` Skill from
`builtin/skills/mods-create/SKILL.md` using stat metadata only. The other two
Skill entries in this npm archive are under mod templates and are not promoted
to default bundled Skills or installed plugins. This group reaches the existing
Skills tab with its built-in/source labels and readonly compact cards. Body
validity, native frontmatter name, overrides and actual invocation are unknown.

Native MCP user configuration tries `<home>/.mcp.json`, `<home>/mcp.json`, then
`<CODEBUDDY_CONFIG_DIR or OS home>/.codebuddy.json`; the first readable candidate
wins, and a non-ENOENT error is not an empty fallback. Arca's existing explicit
MCP target is the first path; other actual paths require explicit configuration.
No file is migrated or shadow-copied during observation. Native runtime pause
for USER/LOCAL/PLUGIN servers is per-workspace LOCAL state in
`projects[workdir].disabledMcpServers`. Arca's global-only management does not
write that project state or claim a connection is active.

Metadata has a separate finite 8 MiB read budget, 2 MiB document limit, 8,192
visits, 512 bound paths/registration records, 1,024 entries per directory and
4,096 components. Files, directories, missing paths and stat-only leaves are
revalidated before publication. JSON metadata cannot be hardlinked or reached
through a symlink. No previous observer's shared allowance is consumed.

Official archive SHA256:
`fd3c38175b142920228ef6cf3b69f7b6d81d57a2b746da6c9bc68e87fd6ec934`.
Full TUI bundle SHA256:
`032191737e2ff681e1223798613c0ca61343c9f008675872d6b0b119161fd16d`.
Exact public member guards live in `data/native-plugin-codebuddy-cli.json`.
Research retains 43 original UTF-8 byte-bound methods plus selected module
bindings. REA's seven-member inert view parsed 125,565 AST nodes but has partial
application/semantic coverage and 2,185 semantic unknowns. That static view is
not whole-package or runtime coverage. The limited producer was not rerun.

Isolation regressions are retained in `codebuddy_cli/tests.rs`; tests were not
run in fast mode. Production compilation, actual Arca metadata observations and
GUI/release acceptance are separate delivery evidence.
