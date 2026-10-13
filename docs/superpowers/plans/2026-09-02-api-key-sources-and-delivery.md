# API Key Sources and Agent Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add six safe API-key workflows to Arca, resolve Provider credentials consistently, and deliver them only through Agent mechanisms that Arca has explicitly verified.

**Architecture:** Core owns a four-variant credential-source resolver and a three-variant Agent-delivery policy. Provider catalog entries store default non-secret source metadata, Model consumption entries store per-Agent overrides and delivery policy, and a declarative capability matrix selects native references, verified Agent stores, or an explicitly confirmed private plaintext transaction. React and Tauri remain thin adapters over typed core views and commands.

**Tech Stack:** Rust 2021, serde, sha2, zeroize, existing Keychain/safe-write infrastructure, Tauri 2, React 19, TypeScript, Vitest.

---

## File map

- Create `core/src/resources/model/credential.rs`: source validation, secure resolution, digest evidence, helper execution, and capability-based delivery selection.
- Create `core/src/resources/model/open_code_auth.rs`: verified `auth.json` codec and private candidate preparation for OpenCode Agent storage.
- Modify `core/src/domain/types.rs`: `AuthRequirement`, `ApiKeySource`, and Provider wire migration.
- Modify `core/src/domain/assets.rs`: `ApiKeyDelivery`, consumption credential policy, and backwards-compatible defaults.
- Modify `core/src/domain/agents.rs`: typed credential capability/delivery view exposed to every frontend.
- Modify `core/src/resources/model/mod.rs`: module wiring, Provider/profile hydration, validation, capability matrix, route preview, and required-credential gating.
- Modify `core/src/resources/model/adapters.rs`: consume an already-selected credential route instead of consulting only `env_key`.
- Modify `core/src/assets/transaction.rs`: include credential evidence and OpenCode/private plaintext files in the single-Agent transaction.
- Modify `core/src/settings.rs`: persist policy fields, strip Provider-owned runtime fields from profiles, and preserve legacy reads.
- Modify `core/src/application/models.rs`, `desktop/src-tauri/src/commands.rs`, and `desktop/src-tauri/src/lib.rs`: thin validation/test commands for non-secret source metadata and resolved routes.
- Modify `desktop/src/lib/types.ts` and `desktop/src/lib/api.ts`: mirror core types and commands.
- Modify `desktop/src/components/ModelsView.tsx`: Provider dropdown/adaptive form, Agent delivery selector, route preview, and app-level danger modal.
- Modify `desktop/src/index.css`: compact adaptive credential controls and modal layout.
- Modify locale resources found by `rg 'credentialEnv' desktop/src`: concise labels, reasons, and safe errors.
- Modify focused Rust and Vitest tests beside the changed modules; do not add redundant snapshots or broad full-suite runs.

### Task 1: Persist source and delivery policy

**Files:**
- Modify: `core/src/domain/types.rs`
- Modify: `core/src/domain/assets.rs`
- Modify: `core/src/settings.rs`
- Modify: `desktop/src/lib/types.ts`

- [ ] **Step 1: Write failing serialization and migration tests**

Add table-driven tests covering these exact values:

```rust
let sources = [
    ApiKeySource::ArcaStore,
    ApiKeySource::Env { name: "MAX_AI_API_KEY".into() },
    ApiKeySource::File { path: "/private/maxai.key".into() },
    ApiKeySource::Helper {
        command: "/usr/local/bin/maxai-key".into(),
        args: vec!["--profile".into(), "coding".into()],
        ttl_ms: Some(300_000),
    },
];
```

Assert that legacy `env_key` deserializes to `ApiKeySource::Env`, new serialization omits `env_key`, missing consumption policy becomes `{ source_override: None, delivery: Auto }`, and no fixture contains a credential value.

- [ ] **Step 2: Run the focused tests and verify red**

Run:

```bash
cargo test -p arca-core domain::types::tests::api_key_source -- --nocapture
cargo test -p arca-core settings::tests::provider_credential_policy -- --nocapture
```

Expected: compilation fails because the new enums and fields do not exist.

- [ ] **Step 3: Add the typed schema and legacy wire migration**

Implement:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthRequirement { Required, Optional, None }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ApiKeySource {
    ArcaStore,
    Env { name: String },
    File { path: String },
    Helper { command: String, #[serde(default)] args: Vec<String>, #[serde(default)] ttl_ms: Option<u64> },
}
```

Add `auth_requirement` and `api_key_source` to `ModelProviderConfig`, while retaining `env_key` only in `ModelProviderConfigWire`. Add `ApiKeyDelivery`, `ModelCredentialPolicy`, and a defaulted `credential` field to `ModelConsumptionRecord`. Hydrate `ModelProfile.env_key` only for an environment source so existing adapter code remains compatible during the refactor.

- [ ] **Step 4: Run focused schema tests and verify green**

Run the two Task 1 commands. Expected: all selected tests pass.

### Task 2: Resolve sources without exposing secrets

**Files:**
- Create: `core/src/resources/model/credential.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/Cargo.toml` only if an already-locked dependency must be exposed directly

- [ ] **Step 1: Write failing resolver tests**

Create tests for:

```rust
resolve_env("MAX_AI_API_KEY", &test_env)
resolve_file(&private_regular_file)
resolve_helper(&HelperSpec {
    command: helper_executable,
    args: vec!["one-line".into()],
    ttl_ms: Some(1_000),
})
```

Use isolated temporary paths. Assert rejection of invalid environment names; missing values; relative file paths; symlinks; mode `0644`; directories; empty/multiline file content; shell-like command strings; timeout; failed exit; and empty/multiline helper stdout. Assert debug/error/serialized evidence contains only SHA-256 digests and never the test secret.

- [ ] **Step 2: Run the resolver test module and verify red**

Run `cargo test -p arca-core resources::model::credential::tests -- --nocapture`.

Expected: compilation fails because `credential` is not defined.

- [ ] **Step 3: Implement validation, resolution, and evidence**

Use a zeroizing value and non-secret evidence:

```rust
pub struct ResolvedCredential(Zeroizing<Vec<u8>>);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialEvidence {
    pub source_kind: String,
    pub source_identity: String,
    pub secret_sha256: String,
}
```

Open key files without following symlinks on Unix, verify a regular file owned by the current user with mode `0600`, cap input size, trim one trailing newline, and reject embedded newlines. Execute helpers with `std::process::Command::new(command).args(args)` only, clear inherited sensitive variables where practical, enforce a bounded timeout, cap captured output, and never format stdout/stderr into an error.

- [ ] **Step 4: Run resolver tests and verify green**

Run the Task 2 test command. Expected: all selected tests pass and test output contains no secret fixture values.

### Task 3: Declare Agent credential capabilities and select routes

**Files:**
- Modify: `core/src/domain/agents.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `desktop/src/lib/types.ts`

- [ ] **Step 1: Write failing matrix and route-selection tests**

Table-drive the approved matrix for Claude Code, Claude Desktop, Codex, Pi, OpenCode, Kilo, Qwen, Grok Build, Crush, Mistral Vibe, Hermes, Factory Droid, Goose, Qoder, and MiniMax. Assert:

```rust
assert_eq!(select_delivery(opencode, Env, Auto), NativeEnvReference);
assert_eq!(select_delivery(opencode, ArcaStore, AgentStore), OpenCodeAuthStore);
assert_eq!(select_delivery(pi, Helper, Auto), NativeHelper);
assert_eq!(select_delivery(goose, ArcaStore, Auto), Err(CREDENTIAL_DELIVERY_UNSUPPORTED));
assert_ne!(select_delivery(any_agent, any_source, Auto), Plaintext);
```

- [ ] **Step 2: Run the matrix tests and verify red**

Run `cargo test -p arca-core resources::model::tests::credential_route -- --nocapture`.

Expected: the typed matrix and route result do not exist.

- [ ] **Step 3: Implement one core-owned capability matrix**

Replace free-form `credential_mode` decisions with a serializable view containing supported native sources, verified Agent store availability, plaintext availability, and exact unsupported reasons. Keep the legacy string populated for wire compatibility until the desktop consumes the new fields. Implement `auto` priority as native source reference, Arca Keychain helper, verified Agent store, then error. Never select plaintext from `auto`.

- [ ] **Step 4: Run route tests and verify green**

Run the Task 3 command. Expected: all matrix cases pass.

### Task 4: Add OpenCode Agent credential storage

**Files:**
- Create: `core/src/resources/model/open_code_auth.rs`
- Modify: `core/src/resources/model/adapters.rs`
- Modify: `core/src/resources/model/mod.rs`

- [ ] **Step 1: Write failing `auth.json` codec tests**

Given an isolated file containing unrelated providers and unknown fields, prepare a Provider credential update and assert:

```rust
assert_eq!(candidate["openrouter"]["type"], "api");
assert_eq!(candidate["openrouter"]["key"], TEST_SECRET);
assert_eq!(candidate["unrelated"], original["unrelated"]);
assert_eq!(candidate["openrouter"]["futureField"], "preserved");
```

Also assert malformed owned entries fail closed, concurrent candidate changes produce `agent_store_conflicted`, output mode is `0600`, and serialized plans/errors do not contain `TEST_SECRET`.

- [ ] **Step 2: Run focused OpenCode tests and verify red**

Run `cargo test -p arca-core resources::model::open_code_auth::tests -- --nocapture`.

Expected: module and candidate preparation do not exist.

- [ ] **Step 3: Implement the verified codec and private candidate**

Use the OpenCode store path `~/.local/share/opencode/auth.json`. Map the model Provider identity to one owned top-level auth entry, merge only the owned `type` and `key` fields, preserve every other entry/field, and return a private write candidate plus candidate digest rather than writing directly. Enforce a regular non-symlink target and `0600` on creation/replacement.

- [ ] **Step 4: Run OpenCode tests and verify green**

Run the Task 4 command. Expected: all selected tests pass.

### Task 5: Make one Agent model update transactional

**Files:**
- Modify: `core/src/assets/transaction.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/safe_write.rs` only for a reusable multi-file private primitive

- [ ] **Step 1: Write failing transaction tests**

Cover three operations in isolated homes:

1. OpenCode model config plus `auth.json` both commit.
2. A stale credential digest after planning returns `credential_source_changed` and writes neither file.
3. Failure after the first physical replacement restores both pre-operation files and leaves consumption unchanged.

Assert backups and operation JSON do not contain the supplied secret.

- [ ] **Step 2: Run focused transaction tests and verify red**

Run `cargo test -p arca-core assets::transaction::tests::model_credential -- --nocapture`.

Expected: credential evidence and multi-file model target are absent.

- [ ] **Step 3: Extend model planning and commit**

The plan contains source metadata, target candidate hashes, and secret digest only. Commit re-resolves the source, compares the digest, prepares every Agent-owned file in memory, performs compare-and-swap checks, replaces the files under the existing same-target recovery rules, and only then persists `ModelConsumptionRecord.credential`. Plaintext writes reuse this path but require an operation-scoped `allow_plaintext` confirmation and a verified `0600` target.

- [ ] **Step 4: Run focused transaction tests and verify green**

Run the Task 5 command. Expected: all three atomicity cases pass and no test artifact contains the secret.

### Task 6: Adapt native Agent projections

**Files:**
- Modify: `core/src/resources/model/adapters.rs`
- Modify: `core/src/resources/model/claude_desktop.rs`
- Modify: `core/src/resources/model/mod.rs`

- [ ] **Step 1: Write failing table-driven adapter tests**

For each supported route, assert the exact native reference rather than a materialized secret:

- Claude Code: helper reference.
- Codex: `{ command, args }` helper table.
- Pi: helper command reference.
- OpenCode/Kilo: environment or file reference.
- Qwen, Grok Build, Crush, Mistral Vibe, Hermes, Factory Droid, Goose: documented environment reference.
- Claude Desktop: existing private Profile Agent store.

Assert unsupported routes return `credential_delivery_unsupported` before any candidate write.

- [ ] **Step 2: Run only the named adapter tests and verify red**

Run `cargo test -p arca-core resources::model::tests::credential_adapters -- --nocapture`.

Expected: adapters still branch only on `env_key` or old `credential_mode`.

- [ ] **Step 3: Pass a resolved route into adapters**

Introduce an internal `PreparedCredentialRoute` parameter. Each adapter consumes only the route variants it declared. It may write non-secret source metadata or an explicitly materialized private value, but it cannot independently read Keychain/environment/files/helpers. Remove the required-auth gap: if `auth_requirement == Required` and resolution is missing, return `credential_missing` before preparing model configuration.

- [ ] **Step 4: Run named adapter tests and verify green**

Run the Task 6 command. Expected: supported native forms pass and unsupported forms fail before writing.

### Task 7: Expose safe validation and route previews

**Files:**
- Modify: `core/src/application/models.rs`
- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src/lib/api.ts`
- Modify: `desktop/src/lib/api.test.ts`

- [ ] **Step 1: Write failing command-boundary tests**

Add thin commands for source validation/test and Agent route preview. Their results contain status, source kind, route kind, target display path, and safe error code only. Assert JSON never includes resolved secret bytes or raw helper output.

- [ ] **Step 2: Run focused API tests and verify red**

Run `cd desktop && npm test -- src/lib/api.test.ts`.

Expected: the new invoke wrappers are absent.

- [ ] **Step 3: Implement thin wrappers**

Call core-owned logic through the application gate. Keep Tauri argument names camel-case compatible and return typed non-secret views. Do not add credential logic to Tauri or TypeScript.

- [ ] **Step 4: Run focused API tests and verify green**

Run the Task 7 command. Expected: all API wrapper assertions pass.

### Task 8: Build dropdown and adaptive forms

**Files:**
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/index.css`
- Modify: locale files returned by `rg -l 'credentialEnv' desktop/src`

- [ ] **Step 1: Write failing component tests**

Test one representative of each interaction rather than duplicate snapshots:

- Provider dropdown shows four source choices and source-specific fields.
- Requirement `none` hides source fields; `required` blocks save when validation reports missing.
- Helper uses separate command, arguments, and TTL controls.
- Agent delivery dropdown disables unsupported options with the exact reason and shows the resolved route.
- Plaintext opens one small application-level modal showing model count/name, Agent, exact target, and `0600`; cancel does not commit, confirm passes the operation-scoped flag.

- [ ] **Step 2: Run the ModelsView test file and verify red**

Run `cd desktop && npm test -- src/components/ModelsView.test.tsx`.

Expected: the current key/env tab UI fails the new assertions.

- [ ] **Step 3: Implement the adaptive UI**

Replace the two credential tabs with native/select-style dropdown controls. Keep secret reveal/replace/clear only for `arca-store`; use plain metadata inputs for `env`, `file`, and `helper`. Display core validation results and delivery reasons without duplicating the matrix. Render the danger dialog at the Models view overlay root, not inside the model grid or review panel.

- [ ] **Step 4: Run ModelsView tests and TypeScript build**

Run:

```bash
cd desktop
npm test -- src/components/ModelsView.test.tsx src/lib/api.test.ts
npm run build
```

Expected: focused tests and production desktop build pass.

### Task 9: Focused security and regression verification

**Files:**
- Inspect all files changed by Tasks 1–8
- Modify only the responsible test/module when a focused check exposes a defect

- [ ] **Step 1: Run one consolidated Rust test command**

Run:

```bash
cargo test -p arca-core \
  api_key_source credential::tests credential_route open_code_auth::tests \
  model_credential credential_adapters -- --nocapture
```

Expected: every selected test passes. If Cargo filtering cannot accept multiple names, run each named module once; do not run the full suite.

- [ ] **Step 2: Run one consolidated desktop command**

Run:

```bash
cd desktop
npm test -- src/components/ModelsView.test.tsx src/lib/api.test.ts
npm run build
```

Expected: selected tests and production build pass.

- [ ] **Step 3: Inspect the diff for secret leaks and unrelated edits**

Run:

```bash
git diff --check
git diff --stat
git diff -- core desktop docs/superpowers
rg -n 'TEST_SECRET|credential-value|api[_-]?key\s*[:=]\s*["'"'][^$<{]' core desktop docs/superpowers
```

Expected: no whitespace errors, no runtime secret values, no generated artifacts, and no unrelated files.

### Task 10: Remote-only PR, merge, and Stable release

**Files:**
- No local commit
- No local push
- No local installed-app update

- [ ] **Step 1: Read the remote-delivery and release skills and fetch live `main`**

Resolve live `origin/main`, verify the implementation branch is based on it or regenerate the remote tree against it, and build an exact manifest of intended paths. Abort on upstream overlap that changes semantics.

- [ ] **Step 2: Create the remote commit through GitHub Git Data API**

Upload blobs, create the tree with live `main` as base, create one conventional commit such as `feat(models): support credential sources and agent delivery`, and update a new remote branch ref. Do not run `git commit` or `git push` locally.

- [ ] **Step 3: Open, inspect, and squash-merge the PR**

The PR body lists the six ways, OpenCode fix, migration behavior, security invariants, focused validation evidence, and the explicit omission of local installation. Verify the remote diff manifest before merging.

- [ ] **Step 4: Verify Direct Stable**

Wait for the automatic release commit/tag/build, then verify the new immutable Stable tag, draft-to-published Release state, expected macOS/CLI/updater asset set, checksums, and latest-version ordering. Do not download or replace `/Applications/Arca.app` because the user explicitly asked to update it themselves.
