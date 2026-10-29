# Visual Studio Code native resources

Arca observes the installed macOS `com.microsoft.VSCode` application, its bundled
extensions and its global installation records. Cline, Roo Code, Continue, Kilo,
Amazon Q, Codex IDE and other hosts keep their separate Agent identities and
configuration writers.

The audited host is VS Code **1.106.3**, product name `Code`, data folder `.vscode`.
Native field meanings are bound to the complete shipped host files declared in
[`native-plugin-vscode.json`](../data/native-plugin-vscode.json). Unknown host
bytes preserve basic bundled package metadata while leaving native contribution
interpretation unavailable. A changed environment/installation reader also
prevents unverified user-registration discovery.

The observer owns finite Core metadata, entry, component and verification budgets
for this physical host. Earlier clients cannot truncate its package inventory by
exhausting a shared global allowance. This does not change the budgets or source
interpretation of existing observers.

## Sources and state

App selection uses `/Applications/Visual Studio Code.app`, then
`~/Applications/Visual Studio Code.app`. The first present candidate must have
the expected bundle/product identity and consistent package/plist versions.
App and resource symlinks are rejected; no executable is started.

Bundled packages come from `Contents/Resources/app/extensions/*/package.json`.
Names, publisher-qualified IDs, versions, localized display names and inert
`main`/`browser` entrypoint paths are metadata. Entrypoint contents never load.
An unmaterialized entrypoint remains a declaration.

For the bound native reader, a nonempty absolute `VSCODE_EXTENSIONS` selects the
installation directory. Otherwise an absolute `VSCODE_PORTABLE` selects its
`extensions` child; the default is `~/.vscode/extensions`. Relative overrides
and development mode remain unresolved. These are Arca's inherited environment
selectors, not proof of an already-running VS Code process's arguments.

Only that directory's `extensions.json` registration records establish user
snapshots. A nonempty `relativeLocation` takes precedence over a stale serialized
location. Otherwise a local file URI path must stay within the selected root.
External/linked paths remain unresolved declarations. The recorded ID/version
must agree with the physical manifest. Missing, unsafe or inconsistent packages
retain their registration as declared metadata, without invented components.
Malformed registry structures do not become partial installation inventories.

Multiple registered versions and bundled/user copies remain distinct physical
snapshots. Arca does not choose the highest version, apply `.obsolete`, initialize
or migrate the registry, read profile/state databases, or infer effective
enablement. Unregistered directory history, alternate profiles, remote hosts,
project scope and command-line overrides remain outside this inventory.

## Native contributions

The shipped workbench establishes these extension points:

| Field | Arca metadata | Boundary |
| --- | --- | --- |
| `mcpServerDefinitionProviders` | MCP reference with provider ID/label | A provider supplies server definitions at runtime; no configured server or connection is synthesized. |
| `chatPromptFiles` | Command file | Native name, required description and confined physical path are checked; body parsing/loading is unknown. |
| `chatInstructions` | Rule file | Same metadata-only path checks. |
| `chatAgents` | Agent file | Same metadata-only path checks. |
| `languageModelTools` | Tool declaration | Only names/identities are exported; descriptions and schemas are excluded. |
| `languageModelToolSets` | Type declaration with its authored name | A tool set is a named grouping, not an MCP server or callable tool. |

Ordinary `contributes.commands` entries are IDE commands and do not become Agent
slash commands. The audited host does not establish a `chatSkills` extension
point; that field or a nearby `skills/` folder alone does not create Skill rows.
This is a bounded installed-version contract, not a claim about future versions
or private/dynamic extension APIs.

Component identities retain the authored provider/tool/file names in their source
extension scope. They do not assert a globally qualified runtime tool namespace
or an MCP server ID.

The inspected installation contains Pylance provider declarations and model-tool
declarations from Mermaid, Python and C/C++. Their package availability does not
establish extension activation, trust, account applicability, successful native
validation, a selected profile, or an MCP connection. All observations remain
read-only and outside asset import, assignment, convergence and deletion.

## Evidence and limits

The complete workbench is SHA-256
`ab955705f930e8c3551f9ba5babdb93de6ee8ca28e49b715d2c7495cc16a6fcf`;
the environment/profile reader is
`a550521b973178b34ef8fa698a6c5536fd1629509e6b8acb46f2bca50e5b8070`.
Exact inert AST selections retain source byte ranges and complete-file hashes.
REA parsed all 18 selected declarations/classes without executing them
(`ev_338d4bc53f4c19b9fae89ef6e5ade8efa9db6911c2aa4e17ae6303dd1d69579b`).
That coverage applies to those selections; omitted dependencies and dynamic
semantics remain unknown. Its module traces expose containment only, not an
end-to-end execution proof.

The initial whole-input REA analysis was partial: the workbench parser failed
and a shared-process semantic worker exhausted its heap. That partial record
(`ev_c18b917a231960f7d8577996cbbfc8fc603dd6831cf35e2c91d91a9fa260d706`)
was retained rather than treated as a negative capability result. No private
configuration, credentials, Keychain records, component bodies, hooks, extension
modules or target executables were used to resolve these limits.
