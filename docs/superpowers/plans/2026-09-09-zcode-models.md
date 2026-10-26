# ZCode Models Implementation Plan

**Goal:** Native ZCode model discovery and multi-model consumption with explicitly selected private plaintext delivery.
**Architecture:** Rust core adapter owns the JSON registry; shared consumption and credential routes serve every frontend. External imports create new Arca provider identities.
**Tech Stack:** Rust, jsonc-parser CST, existing private transaction ledger, React capability-driven views.

- [x] Audit installed ZCode conversion and registry; obtain narrowly scoped credential export authorization.
- [x] Implement adapters/zcode.rs: recursive duplicate/type validation, idempotent apply, model-only clear, reviewed custom-only clear-all.
- [x] Connect model paths, native-registry capabilities, protocol whitelist and private writes in model/mod.rs and transaction.rs.
- [x] Add credential capabilities; Auto never silently exports. Explicit Plaintext uses existing Keychain resolution.
- [x] Add external discovery/import with no native-ID adoption, private credentials and source/model ownership checks.
- [x] Preserve synthetic regression fixtures without running tests under repository fast-mode instructions.
- [x] Review diff and complete required release compilation; update documentation and agent description.
- [ ] Land scoped local main commit, observe Direct Stable, independently verify and install release under standing authorization.
