# Native Plugin Observations

## Intent and scope
Expose native/bundled/plugin-distributed capabilities for every Agent registered by Arca. Preserve a coverage ledger for all 303 current identities and extend it if the registry changes. Verify each mechanism from official source/shipped artifacts with REA where artifacts are available. Deliver each completed Agent as a separate release owned by a dedicated release subagent.

## Approach
Use a shared Rust read-only observer contract and audited data descriptors with per-host adapters. Plugins are independent observations, never central assets or writable consumption relationships. A new Agent Plugins page shows package versions, provenance and MCP/Skill/Rule/Agent/Command/Hook components. Host MCP metadata also appears read-only in the Agent MCP list.

Alternatives considered: parsing only mcp.json misses plugins; scanning every cache as installed produces false states; adding a runtime bridge requires new host installation and is outside this task. Read disk artifacts with explicit provenance/presence and unknown runtime status, augment with authoritative installed registries where verified. This is an implementation decision under the explicit continuing user goal, not an additional user approval claim.

## Source states and ownership
Distinguish bundled, installed, local and cache-only. Multiple versions/aliases remain traceable. Cache-only observations must never be labeled installed/enabled/connected. Never expose credentials, commands, URLs, env, headers or script bodies. Metadata includes host ID, plugin identity, declared version, cache revision, source paths, component names and bounded diagnostics.

No delete, toggle, adoption, assignment, convergence or writer route accepts host observations. Bulk removal only touches the existing user configuration fields and preserves plugin files, cached content, central assets and credentials.

## First delivery: Cursor
Reuse verified Cursor 3.27.12 REA/source evidence. Observe its two bundled MCP providers with manifest + registration evidence; observe ~/.cursor/plugins/cache and ~/.cursor/plugins/local. Cache requires .cache-complete; manifest candidates are .cursor-plugin/plugin.json, .claude-plugin/plugin.json, plugin.json. MCP defaults .mcp.json then mcp.json, first default wins; manifest mcpServers overrides and may reference safe relative files or inline objects/arrays. Host MCP IDs are plugin-${pluginName}-${serverName}. Component paths and reads are bounded and confined, no symlink escape or special-file reads. Installed/enabled/connection remain unknown for cache entries.

## Verification and delivery
Keep primary dirty checkout unchanged. Work in isolated clean main based on current origin/main; do not absorb pending current-contract changes. Project fast mode: retain meaningful fixture/regression code, do not run tests/fmt/clippy/preflight. Compile CLI/Desktop/native, launch actual-data Arca Dev and inspect 1200x820 and 900x600. Fresh focused review per Agent delivery, exact local commit/push, then release subagent triggers Direct Stable and independently verifies official four assets. Release scope does not authorize replacing installed Arca.

## Evidence
Cursor REA dossier and full filesystem inventory remain ignored local material under workspace/local/research/cursor-plugins-rea-20261027 and cursor-plugins-20261028. Agent coverage/release ledger is workspace/local/tmp/arca-native-plugin-rollout-20261028/coverage-ledger.json.

## Credential-independent host reads
Native package metadata has a scoped Core query and thin Tauri command, independent of central asset transactions and model credentials. The Agent page subscribes to MCP/central observation events and rejects stale Agent responses. A pending system credential prompt must not prevent the Plugins tab or host MCP cards from rendering. This does not alter credential access or unlock policy.
