# Provider-owned Credential Modes Implementation Plan

> Historical plan for the v1.8.163 implementation. Its Provider four-mode UI was superseded by the final design in `../specs/2026-09-02-provider-owned-credential-modes-design.md`: new Providers accept only an API Key stored in Keychain, while delivery belongs to Agent adapters.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Provider the only credential configuration surface and make every Agent consume it through automatic safe routing.

**Architecture:** Preserve the four existing source wire variants, relabel `arca-store` as 明文, and remove the per-Agent delivery mutation surface. Core ignores legacy consumption overrides and always selects `ApiKeyDelivery::Auto`, retaining wire compatibility without retaining user-visible behavior.

**Tech Stack:** Rust/serde, Tauri 2, React 19, TypeScript, Vitest.

---

### Task 1: Lock Provider-owned routing in core

**Files:**
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/resources/model/credential.rs`
- Test: `core/src/resources/model/mod.rs`

- [ ] Add a failing regression that stores `delivery: plaintext` and a source override on one consumption, then asserts routing still uses the Provider source and never writes a literal credential.
- [ ] Run `cargo test -p arca-core provider_owned_credential -- --nocapture`; expect the old per-Agent policy to win and the assertion to fail.
- [ ] Make `credential_route_for` read only `provider.api_key_source` (plus legacy Profile fallback) and call `select_delivery(..., ApiKeyDelivery::Auto)`.
- [ ] Remove the public `set_model_credential_delivery` resource function and capability/policy fields that exist solely for Agent UI.
- [ ] Re-run the focused Rust test; expect pass.

### Task 2: Remove Agent credential controls

**Files:**
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/index.css`
- Modify: `desktop/src/lib/api.ts`
- Modify: `desktop/src/lib/api.test.ts`
- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `core/src/application/models.rs`

- [ ] Add a failing source-level React regression asserting AgentView contains no delivery selector, `setModelCredentialDelivery`, or plaintext confirmation copy.
- [ ] Run that one Vitest case and observe failure against `v1.8.162`.
- [ ] Remove the selector state/handler/rendering/modal, the thin API/Tauri command, and CSS used only by these controls.
- [ ] Keep Agent model rows focused on current/enabled state and Provider-derived credential description.
- [ ] Re-run the focused test; expect pass.

### Task 3: Rename the Provider modes

**Files:**
- Modify: `desktop/src/i18n/index.ts`
- Modify: `desktop/src/components/ModelsView.test.tsx`

- [ ] Update the existing Provider-source test to require the four Chinese labels 明文、环境变量、文件、命令 and reject “Arca 安全存储 / 凭据助手”.
- [ ] Run the focused test and observe failure.
- [ ] Change only the labels/help text; keep persisted `kind` values unchanged and state explicitly that 明文 is stored in Keychain.
- [ ] Re-run the focused test and desktop production build; expect pass.

### Task 4: Validate and deliver remotely

**Files:** all changed files and these two docs.

- [ ] Run the focused Rust tests, focused React tests, desktop production build, Tauri compile check with a temporary uncommitted sidecar placeholder, `git diff --check`, and a secret-literal scan.
- [ ] Build an exact manifest, create a remote Git Data API commit from live `main`, verify every remote blob, open PR, inspect its exact file list, squash merge, and delete the remote branch.
- [ ] Verify the resulting Direct Stable tag and all four published assets without installing or launching the local app.
