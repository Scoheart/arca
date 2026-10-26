# Provider Model Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task by task.

**Goal:** Let users fetch a Provider's current model catalog while adding a Model, search it, choose an ID, refresh it, or keep typing any valid ID manually.

**Architecture:** Arca Core remains the authority for Provider capability, credentials, URL construction, HTTP policy, and response parsing. The Tauri command accepts only a persisted Provider instance ID and performs discovery off the UI thread. React receives a small normalized model summary and renders an optional combobox without persisting catalog data or weakening the existing free-text workflow.

**Tech stack:** Rust (`arca-core`, `ureq`, `url`, `zeroize`, `serde_json`), Tauri 2, React 19, TypeScript, Vitest/Testing Library.

**Delivery constraint:** Never edit and push from the user's local `workspace/apps/arca` checkout. Stage and verify in a disposable `/private/tmp` clone, then create every commit directly on `Scoheart/arca:codex/provider-model-discovery` through the GitHub API. Keep PR #128 as the delivery surface.

---

### Task 1: Lock the contract with failing tests

**Files:**

- Create: `core/src/resources/model/discovery.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `desktop/src/components/ModelsView.test.tsx`

**Step 1: Add Core contract tests**

Add tests for:

- the exact 48 supported built-in Provider types and the 3 intentional exclusions;
- same-level `/models` URL derivation from persisted base URLs;
- native Anthropic, Gemini, Cohere, and Fireworks request/response adapters;
- OpenAI-compatible `data[]` normalization;
- optional-key versus required-key behavior;
- redirect, timeout, body-size, page-count, and item-count limits;
- deterministic sort/dedup and safe, credential-free error messages.

**Step 2: Add UI behavior tests**

Add tests proving that:

- creating a Model auto-fetches once for a supported Provider;
- results are searchable and selecting a result changes only `draft.model`;
- users can type an arbitrary ID even when results exist or discovery fails;
- refresh repeats discovery;
- editing an existing Model does not fetch until refresh/provider switch;
- switching Provider caches results per instance and stale responses cannot overwrite the active Provider;
- unsupported Providers keep the existing plain-input behavior.

**Step 3: Verify the red state**

Run:

```bash
cargo test -p arca-core resources::model::discovery::tests -- --test-threads=1
NODE_OPTIONS=--localstorage-file=/private/tmp/arca-provider-model-discovery-localstorage npm test -- ModelsView.test.tsx -t "discovers provider models"
```

Expected: failure caused by the missing discovery implementation/API/UI, not by unrelated baseline tests.

**Step 4: Commit remotely**

Commit message: `test(models): define provider discovery behavior`

### Task 2: Implement Core discovery safely

**Files:**

- Modify: `core/src/resources/model/discovery.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/application/models.rs`

**Step 1: Define normalized public data**

Implement serializable `ProviderModelSummary { id, name, context_length }` and expose read-only `model_discovery_supported` on both Provider template and persisted instance views.

**Step 2: Define Provider discovery strategies**

Create an explicit built-in Provider strategy table:

- 44 OpenAI-compatible providers use the same-level `/models` endpoint;
- Anthropic uses its native models endpoint and pagination token;
- Google Gemini uses `v1beta/models` and filters to `generateContent` models;
- Cohere uses its native models endpoint and pagination token;
- Fireworks uses its account-scoped native catalog;
- GitHub Models, W&B, and Custom remain unsupported for the documented reasons.

No blind endpoint probing is allowed.

**Step 3: Enforce transport and secret boundaries**

Load only the persisted Provider instance and its Keychain credential. Wrap credential bytes/string in `zeroize::Zeroizing`, use `crate::network::build_ureq_agent`, keep the existing Provider URL validation policy, disable redirects, set a 15-second global timeout, cap each response at 4 MiB, cap native pagination at 10 pages, and cap normalized results at 2,000 records. Never include response bodies, URLs containing keys, or credentials in errors/logs.

**Step 4: Normalize results**

Parse only required fields, trim and reject empty IDs, deduplicate by first occurrence while preserving Provider response order, and return optional names/context lengths without inferring unrelated Model fields.

**Step 5: Run Core tests**

Run:

```bash
cargo fmt --all -- --check
cargo test -p arca-core resources::model::discovery::tests -- --test-threads=1
cargo test -p arca-core resources::model::tests -- --test-threads=1
```

Expected: all targeted Core tests pass.

**Step 6: Commit remotely**

Commit message: `feat(core): discover provider model catalogs`

### Task 3: Expose the Tauri and TypeScript boundary

**Files:**

- Modify: `desktop/src-tauri/src/commands.rs`
- Modify: `desktop/src-tauri/src/lib.rs`
- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src/lib/api.ts`

**Step 1: Add the asynchronous command**

Add `discover_provider_models(provider_id: String)` and execute the blocking Core request with the existing blocking-task pattern so the Tauri UI thread stays responsive.

**Step 2: Register and type it**

Register the command, export `ProviderModelSummary`, extend Provider view types with `model_discovery_supported`, and add `discoverProviderModels(providerId)` to the frontend API.

**Step 3: Verify the bridge**

Run:

```bash
cargo check --manifest-path desktop/src-tauri/Cargo.toml
cd desktop && npm run build
```

Expected: Rust and TypeScript boundaries compile.

**Step 4: Commit remotely**

Commit message: `feat(desktop): expose provider model discovery`

### Task 4: Build the searchable Model ID picker

**Files:**

- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/i18n/index.ts`
- Modify: `desktop/src/index.css`

**Step 1: Add request state and race protection**

Maintain per-dialog, per-Provider-instance result/error/loading caches plus a monotonically increasing request token. Ignore any completion whose token or Provider no longer matches the active selection.

**Step 2: Apply fetch timing rules**

For a new Model, fetch once after the initial supported Provider is selected. For an existing Model, do not fetch on open; fetch after an explicit refresh or after switching to a supported Provider. Never let discovery errors affect save validity.

**Step 3: Render the control**

Keep the existing text input as the canonical editable value. Add a compact refresh/status action and an accessible searchable listbox under the field when results are available. Match Arca's restrained utility styling, keyboard behavior, dark theme, and existing dialog density. Selecting an option sets only the Model ID.

**Step 4: Cover localized states**

Add Simplified Chinese and English copy for loading, refresh, result count, no match, unavailable, and retry states.

**Step 5: Run UI tests and build**

Run:

```bash
cd desktop
NODE_OPTIONS=--localstorage-file=/private/tmp/arca-provider-model-discovery-localstorage npm test -- ModelsView.test.tsx -t "discovers provider models"
npm run build
```

Expected: discovery tests and production build pass. Record the five unrelated pre-existing `ModelsView.test.tsx` failures separately instead of hiding them.

**Step 6: Commit remotely**

Commit message: `feat(models): add searchable provider model picker`

### Task 5: Final verification and PR handoff

**Files:**

- Modify: PR #128 body only

**Step 1: Verify formatting and focused suites**

Run:

```bash
cargo fmt --all -- --check
cargo test -p arca-core resources::model::discovery::tests -- --test-threads=1
cargo test -p arca-core resources::model::tests -- --test-threads=1
cargo check --manifest-path desktop/src-tauri/Cargo.toml
cd desktop && npm run build
NODE_OPTIONS=--localstorage-file=/private/tmp/arca-provider-model-discovery-localstorage npm test -- ModelsView.test.tsx -t "discovers provider models"
```

**Step 2: Audit the final diff**

Confirm:

- no credentials, response bodies, generated output, schema migration, changelog, or version bump entered the diff;
- React passes only Provider instance IDs;
- the Core owns URL/auth/pagination and uses the configured Arca proxy;
- all changes exist only on the remote PR branch and the user's local Arca checkout is unchanged.

**Step 3: Update PR #128**

Replace the design-only status with implementation summary, supported Provider count, test evidence, known unrelated baseline failures, and review notes. Keep the PR draft until verification is fully green for the new behavior.
