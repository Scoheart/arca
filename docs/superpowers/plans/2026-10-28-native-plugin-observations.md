# Native Plugin Observations Implementation Plan

> Execute inline with superpowers:executing-plans; perform source review per completed Agent before delivery; honor the user’s preference to keep research and review in the primary agent.

**Goal:** Show all verified native/plugin capabilities across registered Agents and release each completed Agent separately through a dedicated subagent.

**Architecture:** Shared Rust metadata DTOs, bounded readers and audited per-host descriptors/adapters; thin CLI/Desktop projection; physically isolated from all mutation contracts.

**Tech Stack:** Rust/serde/SHA-256 and React/TypeScript.

**Spec:** docs/superpowers/specs/2026-10-28-native-plugin-observations-design.md

## Global Constraints
No credentials, target writes or plugin execution. Preserve cache vs installed and unknown runtime states. No tests/fmt/clippy/preflight under project fast mode. Preserve dirty primary checkout. Only local task files in commits; each Agent gets its own subagent-owned release and independent asset verification.

## Review Focus
Untrusted manifest paths/symlinks/special files; incomplete caches and source failures; alias/version deduplication; metadata escaping and credential omission; native-only MCP pages and bulk operation ownership boundaries.

## Tasks
- [x] Add shared HostMcpObservation/NativePluginObservation metadata and bounded source reader; add Cursor cache/local descriptor and bundled provider observation.
- [x] Integrate inventory scope/revision/watches and safe CLI JSON/text. Preserve all existing writer contracts.
- [x] Add Agent Plugins tab, version/provenance/components search and read-only host MCP cards; add isolated regression code without running suites.
- [x] Compile CLI/Desktop/native, actual-data native preview at both required sizes, source review, exact local commit and push.
- [x] Hand off Cursor feature SHA to release subagent; inspect release/verification evidence before marking Cursor delivered.
- [ ] Repeat per Agent from full coverage ledger; every supported plugin-like mechanism has verified discovery/UI, remaining identities have adequate evidence rather than guessed absence. Never mark the overall goal complete from Cursor alone.

Ruling: current explicit autonomous goal overrides skill conversational approval gates. Existing architecture determines the shared Rust/thin frontend implementation; no redundant confirmation.

Cursor verification: CLI, Desktop and native Dev compile passed; actual HOME preview showed 15 plugin groups / 28 snapshots and 13 host MCPs at 1200×820 and 900×600, including expanded wrapped provenance. Native metadata loaded with the credential session locked. Static reviewer reported no remaining issues. Isolated regressions are retained but were not run under fast mode. Commit/push and delegated release follow.

Current priority: finish mainstream Agents first (Codex CLI, Claude Code, Gemini CLI and OpenCode). Research/review subagents have ended at the user’s request; only the explicitly requested dedicated release subagent is retained. Cursor v2.12.0 completed independent four-asset verification.
