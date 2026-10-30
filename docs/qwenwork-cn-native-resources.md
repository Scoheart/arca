# QwenWorkCN public native resources

The `qwenwork_cn` adapter observes the selected, audited QwenWorkCN 1.4.0 App (build `26102705`, bundle `cn.qwenwork.desktop.mac`). Its scope is public App metadata and optional local-market installation sources. The complete reviewed bundle yields **17 source groups / 156 Skill candidates**: 15 App Skills, one profile Skill and 140 candidates in 15 optional market packages. Every row has `Bundled` presence and `enabled: None`; every Skill has `native_id: None`. There are no concrete Host MCP servers in this scope.

This adapter does not establish the active identity, engine, harness, installed-plugin ledger, profile policy, Skill frontmatter acceptance or session loading. It does not call vendor getters, migration, initialization or validation-cache helpers. It does not read Skill, Agent, Hook or command bodies, private native state, accounts, databases or credentials. Optional market packages are installation sources, not proof of installation. Application-owned Skill groups are not manifest plugins, and this observer does not assert Agent Plugins specification compliance.

## Physical identity and passive version

The allowed App paths are `/Applications/QwenWorkCN.app` and `~/Applications/QwenWorkCN.app`, using layout `qwenwork_cn_app`, Agent ID `qwenwork-cn` and adapter `qwenwork_cn`. Selection uses Core's default App resolution without acquiring a nested credential gate. A configured target, arguments, environment overrides, a new-instance request or an unexpected physical path is unknown. The final selected App and all physical guard stamps are checked again before returning rows or a version. The path passed to `runtime_version` must be that same App or its exact `Contents/MacOS/QwenWorkCN` executable.

`data/native-plugin-qwenwork-cn.json` is the sole authored contract, with unknown fields rejected. It binds these complete public files:

| Public file | Bytes | SHA-256 |
| --- | ---: | --- |
| `Contents/Info.plist` | 4,739 | `43b5f546f1545b4da18f5dc6a27500daafe2953fd21d0624fa7d2a7fe8e30eec` |
| `Contents/MacOS/QwenWorkCN` | 53,616 | `8168e226ef82a418482bcb5f408814ae073cd1e2cb2561ac35ecfacf3c7e8797` |
| `Contents/Resources/app.asar` | 291,118,182 | `be9726be0b529084a6c670dfe2a2d5014371e9e014ebc18d7639b0f55655c594` |

Cold identity verification streams 291,176,537 bytes within an independent 384-MiB hash budget. It uses a 64-KiB buffer, not a whole-bundle allocation. A bounded 32-entry cache requires the physical App and file paths, device, inode, length, mode, link count, modification/change times and expected digest to match. FD and path identity are rechecked before and after hashing, on cache reuse and before publication. Regular metadata and guard files must have one hard link; symlinks and escaping paths are rejected through the confined opener.

The complete ASAR digest binds the exact package, entry and bootstrap members recorded in the contract: `package.json`, `out/main/index.js`, `entry-app.js`, `local-identity-bootstrap.cjs`, `main.js` and the two audited constants chunks. Only the approved fixed package-JSON range is read for `name=qwen-work-cn`, version `1.4.0` and `main=out/main/index.js`. Info name, executable, bundle, version, build and Electron ASAR integrity must agree. The ASAR header is 8,854,756 bytes, so the adapter deliberately does not use the older 8-MiB-header ASAR helper. Whole-ASAR identity authenticates the published header and member offsets without parsing that header at observation time.

The entry has an explicit Electron-worker branch; the normal App branch requires `entry-app`, then local-identity bootstrap and the shipped main module. These branches were analyzed statically, not run. Integration must route this Agent unconditionally to `Some(runtime_version(selected))`, including `Some(None)` for unknown artifacts. A failed passive proof must never fall through to vendor execution.

## App and profile metadata

The public `Contents/Resources/builtin-skill-metadata-index.json` is a format-3 envelope for App version `1.4.0`, sources `skills` then `profile-skills`, with 16 records. Its whole-byte guard precedes parsing. The published payload digest is `7bb184d1af8218ce3f036fa7e673bf39e3d5da46a7d1473c23070b56df8035f8`, independently checked against native `JSON.stringify(payload)`. The observer checks that pinned digest and the approved typed records; it does not recompute a different protocol by sorting a serde map.

Native folder discovery de-duplicates by folder name, main source first. The observer checks the same direct-child inventory and uses only the approved record's source, display name, optional version, file length and mode. Missing or invalid index data does not trigger a Skill-body fallback. The two source groups are `@builtin/skills` and `@builtin/profile-skills`. Their version is explicitly App context; per-Skill published versions, including unknown versions, are retained separately in provenance. No package or plugin version is fabricated for an individual Skill.

Native metadata loading can read Skill bodies, validate content digests and write a userData cache. Those actions are intentionally not invoked. A stat match and published metadata are availability evidence, not a claim that native validation, policy or loading succeeded.

## Optional market installation sources

`Contents/Resources/plugin-market-data.json` is a separate public local-market authority. The native API conversion sets these entries to `installed: false`. Each of its 15 entries maps to an approved physical `Contents/Resources/plugins-example/<folder>` package. The observer keeps the catalog ID and published title, while version comes from the selected package manifest. Source IDs are `@market-source/<folder>`.

The CN UI/local-market parser chooses the first existing manifest in this order:

1. `.qwenwork-plugin/plugin.json`
2. `.qoder-plugin/plugin.json`
3. `.claude-plugin/plugin.json`

This authority is separate from the runtime catalog's two-format convention and from the international App. The audited packages have no first-format manifest. An unexpected first-format file is statted, then rejected as unreviewed without reading it. A malformed or changed selected manifest does not fall through to another variant. The product-design package contains both second and third variants with identical approved bytes; the second still has priority.

Only approved manifest JSON metadata is read. Skill leaves are the 140 recorded direct `skills/<folder>/SKILL.md` paths; titles use the directory name, without reading body or frontmatter. The observer does not infer dynamic MCP, Tool, Hook, command or Agent declarations from these packages. An unsafe leaf is omitted with an incomplete marker while other independent leaves and sources remain available. A failed App index does not hide the market source inventory, and a failed market package does not hide unrelated packages.

## Read boundaries and descriptor lifetime

The source snapshot keeps complete ancestor, leaf and missing-path stamps, not open directory or metadata FDs. Every JSON read or directory enumeration opens a local confined `Bound`, compares its original stamp, checks FD/path identity before and after the operation, and closes it immediately. Cached JSON and directory results first repeat path pinning. The final snapshot CAS covers all recorded ancestors and leaves. The proof retains only the App and three guard FDs. Watch paths include only the two public App roots and fixed public metadata/resource paths; consistency ancestors and private writer roots are not watchers.

Metadata I/O has independent limits: 2 MiB per JSON document, 8 MiB total reads, 16,384 visits, 4,096 component slots, 4,096 pinned paths, 1,024 entries per directory and depth 64. No other host's scan budget is consumed. No process or OS descriptor limit is changed.

The previous CN writer restriction remains necessary. The audited phase-7 identity code can route MCP through scoped identity data; the old flat MCP file is not a proven current global authority. The device-managed `~/.qwenworkcn/skills` path exists in the native code, but is not a universal personal-Skill assignment target. This observer neither reinstates those writers nor scans those directories. Active identity and engine-specific ownership remain unknown.

## Evidence and verification limits

The official arm64 carrier was frozen from `https://assets.qwenwork.cn/release/latest/qwenworkcn-darwin-arm64.dmg`: 417,053,186 bytes, SHA-256 `c1b3d54257a338edf518a34df2a23a5a469be5a8597b8f90bc08bc4d88b73568`, ETag `"0C193FD6920F05B9452847EFFE0D3561-163"`. Static acquisition inspection found the QwenWorkCN App signed by Taobao (China) Software CO.,LTD, team `XN6U3EV979`, with strict/deep codesign success and a stapled ticket. Integration copied that carrier opaquely to the previously absent `/Applications/QwenWorkCN.app`, checked all 21 identity and public metadata guards and repeated strict/deep signature verification. Publication used an exclusive rename without overwriting an existing App. Both owned mounts were detached. The vendor App, helpers and modules were never executed; native configuration and credentials were not changed.

The private evidence set is `workspace/local/research/arca-native-plugin-audit-20261028/qwenwork-cn-followup-20261030/`. The original contract SHA is `bab9236fc6c4609539a7ea91a02c0031b6afb40b65089ea0708f9c9b9fe3dee2`; `implementation-contract.json` SHA is `aae8e30b8a62ad5de7e9157328df82d4359f42d459ad51835b97f633aac70279`. The latter independently binds the metadata index and CN UI's three-format priority, correcting the older runtime-catalog/UI distinction without changing writer evidence.

All ranges below are UTF-8 byte ranges (start inclusive, end exclusive) in the public `out/main/main.js`, whole SHA `1d7868f3c163c352d90609fd2c38cf358080ccc97843e3ace7f4b67840bcc42d`; exact range hashes are retained in that supplement.

| Native authority | Source ranges |
| --- | --- |
| Ordered App/profile folders | `getBuiltinSkillsSourceDirs$1` 789290–789456; `orderedBuiltinSkillSourceDirs` 789456–789543; `listBuiltinSkillFolderEntries` 789712–790113 |
| Metadata validation/loading | `validEnvelope` 849366–850103; `loadBuiltinSkillMetadataIndex` 850103–852936 |
| CN manifest priority | `PLUGIN_MANIFEST_DIRECTORIES` 3091807–3091888; `findPluginJsonPath` 3092262–3092486 |
| Local market is not installed state | `apiResponseToCloudPlugin` 12121114–12122952; `loadLocalMarketData` 12131725–12132345 |
| Physical market detail | `parsePluginJson` 13620346–13622900; `getDetail` 13670336–13672187 |

Target-free REA covered 54 selected source views / 133,748 bytes, with 27,699 AST nodes. It did not prove the complete runtime: semantic coverage remained unknown with 6,815 unknowns. The owned parent is `ev_8832779a114435c1b1d7fad04b9b5312824f0277157b16e0f5d2d0ee8a6d920a`, summary `ev_0d526587f30349daddc8620f93785ceb317b6dd7c619acf3caf055c1e440c431`; the full retained evidence export SHA is `fd15c137946e56784d35acd42f9440944a13520ff7204d910b335a853a847a85`.

Twenty isolated regression cases are retained as source, covering identity/cache/CAS, metadata authority, manifest priority, independent failures, stat-only bodies, path/hardlink boundaries, watch scope and a child-only descriptor-limit scenario. They have not been run under the project's fast delivery mode. CLI, frontend and desktop production compilation passed with unchanged source inputs at version 2.63.0 before automatic release versioning.

Normal Arca metadata queries against the actual HOME observed 17 CN groups and 156 Skill candidates after static installation, with no scoped error or concrete HostMCP endpoint. A separate child process limited to 256 file descriptors returned the same native rows. Passive launch metadata reported version 1.4.0 without executing QwenWorkCN. Global MCP and Skill writer counts remained 73 and 70; MCP consumption count 12, Skill consumption count 51 and concrete HostMCP count 256 were unchanged. The unrelated pre-existing global `mcp_host_observation_unavailable` error also remained unchanged. All previous 1,517 native rows retained their semantic fields; 30 existing rows refreshed only their context digest fields. Shared `/Applications` and local CLI-parent stamp changes were recorded separately; the latter change's producer was unobserved. Native namespace stat snapshots matched before and after. GUI acceptance and formal release are recorded separately by the delivery owner; production compilation and metadata acceptance alone do not establish GUI or release success.
