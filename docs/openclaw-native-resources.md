# OpenClaw bundled native resources

Arca observes physical bundled metadata from two independent OpenClaw surfaces.
Both display as **OpenClaw**; the existing CLI is agent ID openclaw and the
read-only macOS Desktop is openclaw-desktop. Desktop has no global config
writer, user Skills assignment directory or projection of a remote Gateway.

| Surface | Audited release/build | Public bundled package |
| --- | --- | --- |
| CLI | 2026.9.8 / fc23bc864e4553c2d215e479eeec47b67a0bf943 | ~/.openclaw/tools/node-v24.21.0/lib/node_modules/openclaw |
| Desktop private worker | 2026.9.8 / 39f4baf9968201ac9ae743eb314eab5686d8db64 | OpenClaw.app/Contents/Resources/node-worker/arm64/lib/node_modules/openclaw |

Desktop selection checks /Applications/OpenClaw.app, then ~/Applications/OpenClaw.app,
with bundle ID ai.openclaw.mac, name/executable OpenClaw, build 2609000890 and
matching worker version, commit, build timestamp and build ID. The private worker
entry accepts node worker; the independent CLI bin is openclaw.mjs. Matching
version labels and six identical loader files do not merge their identities,
roots or scope. General local/remote Gateway selection remains unknown.

The CLI observer accepts only the audited managed tools/node symlink to the
same physical sibling node-v24.21.0. An unrelated prefix, changed link,
symlinked ancestor or guard mismatch fails closed. This is metadata availability:
it does not make the CLI installed or launchable when no openclaw command is
discoverable. Existing CLI launcher arguments, configuration and user Skills
behavior remain unchanged. Future selected CLI paths use a passive version
reader; unknown wrappers stay unknown without a --version execution fallback.

## Ownership and state

| Observed metadata | Result |
| --- | --- |
| CLI native openclaw.plugin.json | 64 bundled plugin rows |
| Desktop private worker native manifests | 7 bundled plugin rows |
| Manifest-owned CLI Skills | browser/browser-automation, canvas/canvas, crabbox/crabbox-apps, memory-wiki/obsidian-vault-maintainer, memory-wiki/wiki-maintainer |
| Manifest-owned Desktop Skill | browser/browser-automation |
| Package-global, custodian, dependency or extra undeclared Skill paths | Excluded from plugin ownership |
| Optional mcpServers metadata | References only; no configured Host MCP, command, credential or connection |
| Runtime tools/hooks | Not inferred from static manifests |

The native manifest must have its audited ID and object configSchema. Skill
ownership comes from its native skills paths, intersected with audited public
leaf paths. Skill files are stat-only, including unreadable/invalid-UTF-8 bodies.
No package-global or dependency Skill is represented as a synthetic plugin.
All rows have bundled presence and unknown enablement; physical availability
does not establish import success, trust, compatibility or actual Skill loading.

Plugin name/version use native manifest metadata or audited adjacent public
package metadata. Missing plugin version stays unknown. The host version and
build commit are explicitly recorded as host provenance in the revision.

The user's installed/declared plugin ledger is **not observed**. Arca does not
read the private SQLite database, old installs.json, user configuration,
accounts, sessions or Keychain. The upstream loader is not called: discovery
can chmod directories, worker bootstrap can initialize state, and CLI startup
can complete pending package lifecycle before version output. There is no
Agent Plugins conformance or execution acceptance claim.

## Guards and limits

data/native-plugin-openclaw.json is the strict source of truth for separate
package/build/entry/loader and manifest/package guards. The App's
115,921,664-byte main is verified completely against SHA-256
2c61f2ee2eb30c1d331fe71a54f921617a9efbacb1b9e776423eed962371daba.
Only its full digest is cached, in eight bounded entries keyed by physical
root/file paths, dev/inode, length, mode, links, nanosecond mtime/ctime and
expected digest. Fresh no-follow FDs and small public guards are checked on
every observation, including cache hits.

Each host scan has its own finite budget: 4,096 visits, 8 MiB public metadata,
192 MiB large identity hashing, 128 Skill leaves, 256 KiB per leaf and 4 MiB
aggregate declared leaf sizes. Shared observer budgets are untouched. Runtime
guards are verified once per host scan. Manifest/package FDs, directory FDs,
Skill stat identities and selected App/managed link are rechecked before
publication. No directory recursion, Skill body read or target execution occurs.

## Evidence and verification limits

The frozen local research contract SHA-256 is
311d6dd20bb624967be645c23f2f6cc25a11119436610c06d25fc93b07884377.
The official npm archive SHA-256 is
317e0a58db32b386e01187fe9c5c4de541f4ce6d815657bf79a102609b81752a;
published SHA-512 SRI was verified before retained-member analysis.

Exact public source references include
[CLI native manifests](https://github.com/openclaw/openclaw/blob/fc23bc864e4553c2d215e479eeec47b67a0bf943/src/plugins/manifest.ts)
and the
[App private worker](https://github.com/openclaw/openclaw/blob/39f4baf9968201ac9ae743eb314eab5686d8db64/apps/macos/Sources/OpenClaw/NodeMode/BundledNodeWorker.swift).
Focused inert JavaScript REA Evidence is
ev_f48eb817f3e7da5839feef0d6c90824b7f12a431c4fa36b3340f75b588c04a9f;
its complete export SHA-256 is
773ec3d146b57a19bc9d3d733ec2fbdea4bc2bec67cd2eff03467b08ef9d023d.
This does not claim complete runtime coverage, reproducible native builds,
target launch or live MCP/Skill acceptance.

TestHome-isolated regression sources accompany the adapter. They have not been
executed in fast mode. Production compilation and real-HOME inventory
comparison belong to the parent task.
