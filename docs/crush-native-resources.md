# Crush native resource boundary

Crush v0.98.2 has separate MCP, Agent Skills, Markdown command, LSP and shell-hook readers. The audited startup and resource paths do not establish an independent plugin/extension/package loader or an Agent Plugins v1 manifest reader. Arca therefore adds no native-plugin host, fabricated bundled plugin, or inferred MCP for this release. Compiled Go tools, embedded Skills, instruction files and Docker's MCP gateway keep their own resource types.

This finding is version-bound to official `charmbracelet/crush` commit [`d6329fbd900a5c63a452797b74ff8d1c499aebef`](https://github.com/charmbracelet/crush/tree/d6329fbd900a5c63a452797b74ff8d1c499aebef), released as [v0.98.2](https://github.com/charmbracelet/crush/releases/tag/v0.98.2) on 2026-10-23. It does not establish behavior for forks, future releases, wrappers or every third-party module.

## Passive runtime identity

`data/native-runtime-crush.json` is the source of truth for the one accepted artifact:

| Property | Audited value |
|---|---|
| Platform | Official Darwin arm64 Mach-O |
| Version | `0.98.2` |
| Complete file size | `90,672,627` bytes |
| Complete file SHA-256 | `3334ecda545cd804b1d34a5b0417649e81b809db50d67605dd1fa05f9c9e21e4` |

`plugin/crush.rs::runtime_version(selected)` returns the pinned version only after a complete-file match. It accepts an explicit selected symlink by resolving its physical target, then uses the shared confined opened-file verifier with a fresh `ScanBudget`. Hashing consumes the independent finite `MAX_VERIFIED_BYTES` allowance, not the private metadata read allowance. The verified file descriptor stays open while the selected path and physical metadata are rechecked. On Unix those checks include device, inode, size, modification time and change time; the selected canonical physical path must also remain the same. Relative/traversal/control/non-UTF-8 paths, nonregular files, dangling selections, digest/size mismatches and changed selections fail closed.

No executable is launched to obtain a label. The parent passive-version dispatcher owns the unconditional `crush` route: it must return `Some(crush::runtime_version(selected))` even when the leaf result is `None`, preventing generic CLI-version fallback for unknown Crush artifacts. This module does not discover another installation or initialize configuration. A different signed release, a modified file or a wrapper remains an unknown version until independently audited.

The release asset digests and official checksum file matched. Static Go buildinfo reported `go1.27.1`, module `v0.98.2`, the commit above and `vcs.modified=false`. Strict Developer ID signature verification passed for Charmbracelet Inc (`X3UBLT2DDB`); GitHub API reported the annotated tag signature valid. Sigstore was **not independently verified**. These are artifact/signature/buildinfo and corresponding-source evidence, not successful decompilation: the native REA attempt failed with `provider_unavailable` because Ghidra 12.1 was not configured and available Java 17 was insufficient; Hopper/IDA were unavailable. No successful native REA Evidence was produced, no engine was installed, and no target/help/version/login/plugin/helper was executed during this audit.

## Actual loading paths

The source chain is `main.main` (`main.go:15-25`) → `cmd.Execute` → root `RunE` → `setupWorkspace` → `setupLocalWorkspace` (`internal/cmd/root.go:274-340`) → `config.Init/Load` → `skills.DiscoverFromConfig` → `app.New`. App construction creates the LSP manager and asynchronously initializes configured MCPs (`internal/app/app.go:97-163`). `initCoderAgent` → `agent.NewCoordinator` → `buildAgent/buildTools` constructs explicit Go tools, filters built-ins and appends tools from the live MCP registry (`internal/agent/coordinator.go:839-984`). Backend workspaces use their own config and Skills manager (`internal/backend/backend.go:430-460`).

A content search over all Go, JSON, Markdown and go.mod files, including hidden/ignored files, found no Agent Plugins package markers/readers, plugin manifests, `plugin.Open`, `dlopen`, or `LoadLibrary`. A plugin/extension/package directory-name search also found none. The source inventory was 1,266 files totaling 9,782,627 bytes, including 755 Go files and 434 non-test Go files. The negative search supplements the positive call-chain review; it is not a comprehensive certification of every external dependency or the Agent Plugins v1 schemas. Agent Skills support is a different contract from Agent Plugins v1.

## Resource readers and activation

| Type | Source contract and boundary |
|---|---|
| Built-in tools | `config.Config.SetupAgents` and `coordinator.buildTools` assemble/filter compiled tools; `options.disabled_tools` affects the built-in allow-list. These are not installed plugin packages. |
| MCP | Crush's config key is `mcp`, mapping names to `type: stdio/http/sse` plus command/args/env or URL/headers and optional filtering/auth/channel fields. `app.New` obtains repository overrides; initialization gives a repository disable precedence over a force-enable of a config-disabled entry. Only live `ListTools/ListPrompts/ListResources` supplies capabilities and counts. Config presence is not a live connection assertion. |
| Skills | `SKILL.md` frontmatter reader plus embedded `builtin/*` with virtual `crush://skills/` paths. Exact name dedup and `disabled_skills`, `disable-model-invocation`, `user-invocable` are separate rules. User files override embedded names. Among user duplicates, the actual reader sorts by lowercase directory path/name and then keeps the last exact name; the project-first implication of some comments is not a reliable precedence rule. |
| Commands | Markdown under config-home `crush/commands`, `HOME/.crush/commands`, and workspace DataDirectory `commands`. Content becomes a user prompt; nested filename paths become colon-separated `user:`/`project:` IDs. Both user roots append without a dedup winner rule. User-invocable Skills and live MCP prompts can also appear in the UI. |
| LSP | Compiled powernap defaults plus `lsp` declarations. User-disabled servers are removed; explicit entries and implicit defaults have different auto-start gates. File type/root markers and executable availability still decide actual startup. LSPs are not MCPs or plugins. |
| Hooks/context | Configured `PreToolUse` shell hooks wrap top-level tools. ContextPaths/GlobalContextPaths supply instruction text. Neither is a plugin manifest reader. |

For Skills, configured `options.skills_paths` are supplemented by global defaults and project roots. Default global paths are `$XDG_CONFIG_HOME/crush/skills`, `$XDG_CONFIG_HOME/agents/skills` (config home defaults to `HOME/.config`), `HOME/.agents/skills`, and `HOME/.claude/skills`. Nonempty `CRUSH_SKILLS_DIR` replaces only that global default list; configured/project/embedded Skills remain. Project candidates are cwd and a distinct git root under `.agents/skills`, `.crush/skills`, `.claude/skills`, `.cursor/skills`. Discovery recursively follows symlinks, so a future metadata adapter needs explicit confinement/size limits and must report any stricter boundary. Relative custom roots and `$` templates must not be resolved by executing the runtime during passive inspection.

## Configuration is not a passive API

On macOS/Unix, source `internal/home/home.go:25-30` uses nonempty `XDG_CONFIG_HOME`, otherwise `HOME/.config`. `CRUSH_GLOBAL_CONFIG` is a directory override for `crush.json`; `CRUSH_GLOBAL_DATA` is a separate directory override for the global data `crush.json`. Without it, global data follows XDG_DATA_HOME/crush or HOME/.local/share/crush. These overrides do not automatically move command or default Skills directories. `CRUSH_PROFILE` is a pprof debug switch opening localhost:6060, not a named configuration profile.

The low-to-high source order is system `/etc/crush/crush.json`, global config JSON, sibling `crushrc`, global data JSON, project candidates from git root to cwd, then workspace DataDirectory/crush.json. Within a project directory: `crush.json < .crush.json < crushrc < .crushrc`. Config lookup is bounded by the git root (or cwd without one), with filesystem home/owner checks. DataDirectory follows `--data-dir`, explicit option, nearest bounded `.crush`, then cwd/.crush. `go-jsons.Merge` implements ordered merging; this audit did not separately establish every third-party array/null merge detail.

`crushrc/.crushrc` are executable shell configuration: `config.loadFromConfigPaths:953-965` → `shellconfig.LoadShellConfig:33-52` → `shell.Run`, supporting `source`, variables and command substitution. `config.Load` can also migrate/write config, refresh providers, resolve shell templates, alter environment and touch credentials/caches. Even JSON field resolution can execute `$(command)`. CLI startup autoloads dotenv. Do not invoke Crush, config.Load, its dirs/help/version helpers, resolver, MCP or LSP for passive metadata. The global prefix omits a data-directory shell sibling, but server startup uses the data directory as a meta-workspace cwd; do not infer a universal no-shell property from that prefix comment.

## Existing Arca JSON writer

The current Arca registry targets `~/.config/crush/crush.json` with key `mcp` and codec `explicit_type` (`data/agents.json`). The codec writes explicit `stdio` or `http/sse` values and standard transport fields (`core/src/resources/mcp/codec.rs:509-511,625-633`), which still align with Crush v0.98.2's JSON schema and loader. Static evidence therefore does not show that the JSON writer is completely invalid.

That is a narrower contract than arbitrary Crush settings. The writer does not evaluate or edit shell configuration, resolve an arbitrary runtime XDG/CRUSH selection, account for every higher-priority global-data/project/workspace payload, or determine repository MCP override state. A successful JSON write cannot by itself prove the MCP is effective or connected. Future work should retain the global-JSON scope and report unresolved higher-priority executable/config state rather than adding shell execution. This change does not modify the writer or broaden its supported settings.

## Verification boundary

TestHome-isolated regression **source** was written before implementation for full-file/same-length mutation checks, size changes, unknown-runtime suppression, selected-installation isolation, symlink retargeting, same-path inode replacement, special/path rejection, independent finite hashing budget and contract unknown-field rejection. The integration regression depends on the parent dispatcher wiring described above. Tests, suites, compilation, fmt, clippy, icon checks, validators and preflight were not run under the task's fast-mode restriction. No runtime success, green test result, UI acceptance or binary decompilation is claimed.
