# Arca Credential Session Implementation Plan

> For agentic workers: use superpowers:executing-plans to implement this plan task-by-task. Project validation and delivery instructions override generic test/commit steps.

**Goal:** A desktop credential session shared by all Providers, unlocked once per process or eight-hour authorization window.

**Architecture:** An additive aggregate vault in macOS Keychain stores revision-bound credential copies. Core owns cache, migration and concurrency. Existing source items and Agent commands stay compatible; all secret reads consult the shared session before a native per-item lookup.

**Tech Stack:** Rust, Security.framework, serde, zeroize, existing private file locks; thin Tauri startup adapter.

**Spec:** ../specs/2026-10-25-credential-session-design.md

## Global Constraints

- No UI tests or normal cargo/npm test suites unless explicitly requested.
- No credentials outside Keychain/zeroizing memory; no destructive migration.
- Maximum session eight hours; source revision checks remain mandatory.
- No automatic delivery or installed-app replacement.

## Review Focus

- A cancelled startup unlock must not re-prompt on every page visit.
- Two desktop/CLI writers must not drop each other's credentials.
- An external rotation/deletion must not reuse an old secret.
- Failed additive import must preserve every original entry.
- Legacy Agent helper commands must remain usable throughout migration.

## Tasks

- [x] Implement a focused session engine with injectable metadata/load/store backend and a zeroizing versioned document. Native adapter uses the existing Keychain implementation and a separate cross-process lock. Exercise isolated in-memory scenarios, including multi-Provider/concurrent reads, expiry and denied retry.
- [x] Integrate native reads, writes and deletions in the shared Model credential boundary. Add desktop startup warming, explicit retry and exit clearing. Metadata-only inventory must remain non-interactive.
- [x] Build Core/CLI/desktop, run focused non-UI probes, document actual authorization semantics and limits, launch actual-data development preview, and report the verified result without publishing.

## Verification results

Core/CLI and frontend production builds pass. A standalone Rust probe exercises the actual session engine with an isolated in-memory backend: shared startup unlock and 12 concurrent readers, expiry/exit, sleep and clock rollback, cancellation coalescing and explicit retry, external key rotation/deletion, additive-import failure/race, cross-process three-write merge, deferred persistence, malformed and oversized payloads, and rollback exclusion. An isolated CLI Provider inventory query also passes. Static code review findings were fixed. Development preview runs with actual HOME and Arca data. No UI tests, real-Keychain automated probes, commits, pushes, publishing, or installed-app replacement were performed.
