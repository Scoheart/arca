# QwenWork international desktop native resources

The `qwenwork` observer binds **QwenWork.app 1.0.4**, build **26090501**,
bundle `com.qwenwork.desktop.mac`. QwenWorkCN 1.4.0 has different entry,
identity, engine and plugin-catalog mechanisms and is outside this contract.

## Artifact and source authority

The official international homepage links to
`https://assets.qwenwork.ai/release/latest/qwenwork-darwin-arm64.dmg`.
The acquired object is 295,535,645 bytes, SHA256
`bc4b95d136627117534693303bfe27d4c735d9db2e04f7ae22bd74f64608a51a`.
Acquisition bound GET ranges and the retained serial prefix to ETag
`DC84CDB069EF271726424F480915C190-116`. Native code-signature verification
passed with Taobao team `XN6U3EV979`; this is a static package check.

The package is `qwen-work`, with entry `out/main/index.js` requiring `./main`.
Its ASAR header SHA matches the signed Info.plist declaration. Main imports
the fixed constants module, whose exported home resolver derives `.qwenwork`
from the actual profile name. The internal `qwenworkcn` product literal does
not change the international profile's home into `.qwenworkcn`.

`data/native-plugin-qwenwork.json` pins 27 complete members, including the
entire ASAR, Mach-O entry, Info.plist, SDK metadata/entry/worker, three LegoKit
MCP bundles, the vendor manifest and 15 example manifests. Bounded FD-based
hashing and metadata snapshots reject symlinks, metadata hardlinks, replaced
paths and changed files. Unsupported packages and custom launch selections
retain an unknown passive version; no vendor bootstrap or version command runs.

## Skills and ownership

The shipped candidate inventory contains **206 Skills**. App, SDK and LegoKit
groups retain application ownership. `Qoder Security` retains its own native
manifest version and plugin ownership. Fifteen example plugin packages retain
their own names and versions and account for **140** of these candidates;
their provenance explicitly says **bundled installation source**, which does
not mean that they were installed into the native user's plugin directories.

Discovery checks paths, file types, sizes and directory metadata. It never reads
Skill, command or Hook bodies. Names come from folder metadata, with no claimed
frontmatter name, disabled value, parser acceptance or actual runtime activation.
Skills uses the existing Core projection and built-in/client/plugin source labels.
Native entries have no delete, toggle, import, assignment or body-opening action.

The application Skill scanner and plugin UI scan direct child directories for
the exact `SKILL.md` filename. LegoKit Skills additionally pass native frontmatter,
mode and runtime projection checks, which are deliberately unobserved here.
The shipped SDK is `@ali/qodercn-agent-sdk-next` 1.0.28 under the application's
`@qoder-ai/qoder-agent-sdk` package path, with worker metadata 1.1.32. Physical
SDK resources do not establish that a particular worker was loaded.

## Physical user plugins

`PluginService.listInstalled` scans `.qwenwork/plugins` and `plugins-custom`.
The custom directory wins for the same physical folder once its native manifest
path exists. Hidden/unsafe folder names and the three source-defined blocked
folders are excluded. First-existing `.qoder-plugin/plugin.json` precedes
`.claude-plugin/plugin.json`; an invalid selected document retains unknown
metadata instead of selecting the second manifest. A folder without a native
manifest is not promoted into a plugin.

Native getters can normalize manifests or synchronize CLI state. Arca invokes
none of them. It reads only the selected manifest through its ordinary metadata
reader and observes one-level Skill and Markdown command candidates by stat.
No settings database, account, identity, credential or installed ledger is read.
Persisted switch requests and effective enablement both remain unobserved;
all `enabled` fields are `None`.

Manifest connector names and an independently unaliased `.mcp.json` file are
**MCP references**. The MCP file stays opaque. Hook/content declarations cause
the generic file reference to be omitted conservatively, while independent
manifest connector references remain. Source-bound LegoKit server declarations
are also references. No endpoint, auth value or concrete HostMCP is fabricated.
Other built-in connector factories and external LegoKit discovery are outside
this scope and do not inherit QoderWork's older adapter.

## Evidence and verification boundary

The source index binds original package members and SHA256. Fifty exact source
views (55,144 bytes), including original entry/constants and explicitly wrapped
function/method ranges, parsed in REA with no syntax failure. Parent Evidence:
`ev_db30b167a4284bdfa7a28816b37ac2713432a3a3611083c6b804dd8c3ef085e3`.
The complete session export is retained in local research. Its 2,518 semantic
unknowns remain explicit; complete view parsing is not complete application or
runtime semantics. Subsequent original byte-bound policy/scanner ranges close
the native direct-child and blocked-folder rules without executing the bundle.
That five-file follow-up (1,805 bytes) retained parent Evidence
`ev_a0f7bc497bb7d18d6639e1532dd038391a2fea69752d1b7a857cd9f310938a12`
and 117 semantic unknowns in its separately exported complete ledger.

Twelve isolated regression cases cover ownership/candidate counts, preferred
manifest failure, one-level opaque discovery, custom precedence, Hook/MCP alias
handling, symlinks/hardlinks, concurrent replacement, passive unknown routing
preservation of settings while withholding stale CN writers, and independent
Command discovery when a Skill directory or leaf is rejected.
They are retained unrun under the project's fast delivery mode. Production
CLI/frontend/native compilation and actual-HOME metadata acceptance are recorded
separately from this static source contract.

GUI acceptance reproduced a descriptor-limit difference with the exact same CLI: under an isolated process-local `RLIMIT_NOFILE=256`, the original snapshot returned zero QwenWork rows while passive version1.0.4 still resolved. Metadata snapshots now retain parent/file stamps instead of one FD per directory; enumeration and JSON reads bind an operation-local FD and revalidate it before closing, and final path/ancestor CAS remains. No system or application descriptor limit is raised. A retained isolated subprocess case covers400 metadata candidates with27 guard descriptors under a128-FD child limit; suites remain unrun in fast mode.
