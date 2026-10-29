# Zed extension metadata

Arca observes the default global extension installation of Zed Stable on macOS.
The selected App is `/Applications/Zed.app`, then `~/Applications/Zed.app`, with
bundle identity `dev.zed.Zed` and name `Zed`. The audited version is **1.25.3**.
This observer reads authored declarations and physical resource availability;
it does not load WASM, initialize an extension or prove runtime acceptance.

## Storage and selection

The native default is `~/Library/Application Support/Zed/extensions/installed`.
Immediate physical package directories are observed. `work`, `build`, `staging`,
remote extension uploads, project sources and custom command-line data roots stay
outside this view. Development symlinks are not followed. Unknown or unsafe
sources produce incomplete observation rather than borrowing another directory.

`extensions/index.json` is a derived cache. Native startup may load it and then
asynchronously rebuild it from installed directories, writing the index and
creating work directories. Arca never invokes that flow and does not treat a
cached row as an installed or active package.

An existing `extension.toml` takes precedence over `extension.json`; malformed
TOML does not fall back. Schema 1 TOML requires ID, name, version and schema
version. Its ID must agree with the physical directory. Legacy JSON uses the
directory as ID and only the old language/theme/grammar conversion. Foreign
runtime fields in that legacy format do not acquire new capabilities. Unknown
schemas or host versions retain basic identity with incomplete interpretation.

## Contributions and boundaries

| Authored metadata | Observation |
| --- | --- |
| `context_servers` | MCP factory references; commands and connection definitions come from runtime WASM. No host MCP server is created. |
| `slash_commands` | Command declarations, without descriptions or execution. |
| `language_servers` | LSP declarations, independent from MCP. |
| Language/grammar/theme/icon-theme/snippet resources | Bounded path/name availability; resource bodies remain unread. |
| `language_model_providers`, debugger declarations | Type declarations; no managed Model Provider, credential or executable action is created. |
| `lib` and `extension.wasm` | Authored SDK version and file availability; no WASM parsing, compilation, imports or execution. |

Immediate native language/theme resource directories are included as path
metadata. Names that require parsing their file bodies remain unknown. The
audited manifest has no established Skill contribution field; nearby `SKILL.md`
files do not become plugin Skills. This format is Zed's native extension schema,
with no Agent Plugins v1 conformance claim.

The current installation contains HTML 0.3.2, a WASM component and an HTML language
server declaration, with no MCP declaration. Effective enablement, compatibility,
trust/capability grants, selected workspace and runtime registration remain
unknown. User settings, accounts, credentials, Keychain and databases are not read.
All resources are read-only and outside asset assignment/import/deletion.

The observer uses its own finite Core scan budget. App identity, physical paths,
manifest sizes, entry visits and component totals remain bounded; existing hosts'
budgets and observations are not changed.

## Evidence

Official tag `v1.25.3` resolves to
`8dd255c3eafb5231056fe4e6f601cedd011f1c93`. Six complete Rust source files were
verified against their Git blob hashes. That full commit literal also occurs in
the installed main at byte offset 135906172, corroborating the selected source
version without claiming a reproducible-build proof. The 175,367,104-byte main
has SHA-256 `17398176f29b339ec272ed99e6824a796a1df8c329093aea939b5b41abdf3602`.

REA package/plist/signature evidence includes
`ev_838c58179b59b1bb78a86823181c9000ea5d63d72eca76eeb444812fc6b57fba`,
`ev_87ef0d406762bc7df007b4838294752ec5ce688afca0fcce322639a2aa25cab6`
and `ev_fa0c15a4c022d0ec0b63ff190e8b02ba497f4e7c74d5bfcac6739e9d71e014f2`.
The complete bundle and source index are retained in the local Zed research
directory. Native deep providers were unavailable; public source analysis and
inert artifact metadata do not establish target execution or active MCPs.
