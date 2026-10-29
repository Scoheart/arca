# Warp application-owned Skill resources

The `warp` observer exposes two read-only builtin resource groups from the selected
Warp Stable macOS app: `@builtin/skills` (Warp 随包 Skills) and
`@builtin/mcp-skills/figma` (Figma 随包 Skills). The audited installation contains
14 general and 8 Figma Skill files. The observation means that each regular
`SKILL.md` path exists in the application bundle. It does not claim successful
parsing, availability in a conversation, effective enablement, or loading.

These groups are application-owned resources, not verified Agent Plugins manifests.
Their presence is `bundled` and their enabled state is always unknown (`None`).
They contribute Skill components to the shared native-Skill projection, preserving
the exact group and physical source path even when user assets have the same name.
The observer creates no MCP server records. Figma Skill files do not establish an
installed, authenticated, enabled, or running Figma MCP connection.

## Passive identity and bounded caching

[`native-plugin-warp.json`](../data/native-plugin-warp.json) is the source of truth
for this audited build, `0.2026.08.19.08.15.01`. Its complete 414,615,504-byte
`Contents/MacOS/stable` SHA-256 is
`273206011d55499a63d12dcd87bc6634f18e541e02a4a72a368e8a5200f31fec`.
The observer verifies the exact Info.plist and bundled version JSON byte guards,
their bundle name/ID, executable and version fields, the actual arm64 Mach-O main,
and the selected application. Unknown or changed builds fail closed.

Main-file SHA-256 reads have their own 512 MiB limit. Small public identity reads
have a separate 2 MiB/32-entry limit; neither consumes the shared native scan
budget. A four-entry LRU caches only a proven main-file digest. Its key contains
the app and main paths, app/main device, inode, length, mode, mtime and ctime
(including nanoseconds), and the expected digest. Cached entries hold no FDs.
Each use opens fresh confined no-follow FDs, rereads both small guards and the
Mach-O header, and compares FD metadata with the path binding before and after
verification. Final publication rechecks every guard and the first-existing app
selection, so an inode replacement, upgrade, move, changed guard or newly appearing
higher-priority app cannot borrow a previous observation.

The two fixed roots are enumerated through directory FDs. Only their direct child
directory names and `SKILL.md` no-follow `statat` metadata are used. Leaf stats
resolve against retained parent directory FDs, including during the final checks,
so a replaced intermediate path cannot redirect the scan. Skill bodies are never
opened or hashed. The observer retains directory FDs and leaf identity stamps
through publication, rejects symlinks/special leaves and unsafe paths, and bounds
the inventory to 512 entries, 128 candidate leaves, 256 KiB per declared leaf and
4 MiB of aggregate declared leaf lengths. Supporting scripts and nested Skill
files are not traversed. Both groups are staged until the final checks succeed;
failure does not publish a partial Warp inventory.

The watcher lists only app/identity/resource paths from the two declared app roots.
No user Skill root, Warp config, Group Container, SQLite/session data, account,
credential, Keychain, log, remote host or cloud catalog is scanned. No Warp app,
CLI, module, loader, hook, helper, Skill script or native addon is executed. There
is no `--version` fallback in this observer.

## Evidence and remaining limits

The static audit is retained locally under
`workspace/local/research/arca-native-plugin-audit-20261028/desktop/warp-followup-20261030/`.
Its `contract.json`, member/source indices and complete REA bundle bind the installed
main, all 243 shipped regular files and the 22 Skill paths. The complete REA bundle
has six records and SHA-256
`7f484e953c37ed5c87569a76dc228a2c798b9ef7aec2a92a4eaa5ecb17354fe0`.
The actual `WarpDockTilePlugin.docktileplugin`, declared by `NSDockTilePlugIn`, is
a macOS Dock component and is outside these groups.

Official public source at commit
`5a55a2aca77339ad75637bed650352369ee7fcb9` describes
`stable::main → warp::run → initialize_app → SkillManager::new → BundledSkill::detect`
and the two resource loaders. It includes a bundled-Skills feature gate, Figma MCP
running-state gates and other activation conditions. That source commit has not
been matched exactly to the installed release. Native deep providers were
unavailable; native symbols establish function families, not calls or execution.
The observer therefore keeps actual Skill activation/loading and MCP state unknown.

Isolated `TestHome` regression sources cover identities, cache reuse/invalidation,
FD/selection races, stat-only leaves, provenance, symlinks/special nodes and budgets.
They were written before implementation. Fast mode leaves the test suites unrun;
production compilation and development GUI acceptance are reported separately
during delivery. This document does not claim target execution or native loading.
