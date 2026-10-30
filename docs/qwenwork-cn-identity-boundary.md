# QwenWorkCN 1.4.0 identity and writer boundary

The official domestic carrier is QwenWorkCN.app **1.4.0**, build **26102705**,
bundle `cn.qwenwork.desktop.mac`. The official arm64 DMG is 417,053,186 bytes,
SHA256 `c1b3d54257a338edf518a34df2a23a5a469be5a8597b8f90bc08bc4d88b73568`.
Static native signature verification passed for Taobao team `XN6U3EV979`.
This contract is independent of international QwenWork 1.0.4.

The compiled policy activates phase7 identity behavior for darwin; shared startup
can instead take the shared branch. `readCustomMcpConfig` reads the scoped
identity connector configuration under phase7. The fixed `.qwenworkcn/mcp.json`
path is therefore not a universal active target.

Personal Skills resolve under the active SDK root's
`identity-data/v1/SHA256(identityKey)/skills`. The device Skills directory is
still valid for device-managed resources, but phase7's device scan requires
managed metadata. A generic ordinary Skill symlink in `.qwenworkcn/skills` does
not establish personal Skill loading. Native identity bootstrap can select an
isolated `.identity-env/<UUID>` SDK root.

Arca removes both unqualified writable capabilities from this audited Agent
definition until an identity-aware contract can be established without reading
accounts, databases or credentials. Existing user files, path overrides,
assignments and central assets are preserved. Loading an obsolete saved Agent
definition cannot restore the two built-in writers; no automatic migration,
cleanup, projection or native helper is introduced.

Local research binds 28 exact source ranges for this safety decision in
`capability-tightening-source-index.json`, SHA256
`5871f85debdbd6425606291c6d47774d79f28bfb77e64e0ae5b2790582ce5208`.
Key original main ranges are `[822129,822730)` for the config dispatcher,
`[816766,817369)` for the scoped reader, `[792326,792559)` for personal Skills,
and `[865610,865901)` / `[865901,867503)` for user/device scanning. All are
zero-based UTF-8 byte ranges with exclusive ends. No actual identity, state
database or component body was read.

The frozen local contract has SHA256
`bab9236fc6c4609539a7ea91a02c0031b6afb40b65089ea0708f9c9b9fe3dee2`;
its source index has SHA256
`bb37b8f15a2d268edfd5c0645491ad4bf2f86821366d67da0afd8ba3b90801a9`.
REA parsed 54 source-bound views (133,748 bytes), with parent Evidence
`ev_8832779a114435c1b1d7fad04b9b5312824f0277157b16e0f5d2d0ee8a6d920a`.
The complete export is retained locally; 6,815 semantic unknowns remain.

The new PluginCatalog, enterprise generation packages and multiple engine
manifests are retained as separate research. Their presence does not identify
the active engine, account, loaded plugin or runtime enablement. No CN plugin
observer or generic directory alias is fabricated from the international one.
