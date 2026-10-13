# Agent Model Source of Truth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make each Agent Models panel use the Agent's declared storage authority, and make native-registry clear-all remove every configured Model—including external/manual entries—without deleting central assets or unrelated configuration.

**Architecture:** Core declares `native-registry`, `arca-mapping`, or `guided` for every Model Agent. A dedicated `clear_agent_models` reviewed operation follows the existing MCP clear lifecycle: clear desired mapping, run an authority-specific target writer, re-observe, and report incidents rather than presenting optimistic empty state. Desktop renders native counts from managed plus external observed rows and mapping-only counts from Arca consumptions.

**Tech Stack:** Rust (`arca-core`, serde, JSONC/TOML/YAML adapters, target-scoped transaction engine), React 19, TypeScript, Vitest/Testing Library.

**Delivery constraint:** Keep the user's original Arca checkout unchanged. Add implementation to Draft PR #135 through one exact-manifest GitHub API commit; do not use local `git commit` or `git push`. Arca fast-delivery policy skips local tests/build unless the current user explicitly requests them; test code is still required, and the Stable production build plus four-asset verifier remain mandatory.

---

### Task 1: Declare Model storage authority in Core

**Files:**

- Modify: `core/src/domain/agents.rs`
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/application/agents.rs`
- Modify: `desktop/src/lib/types.ts`
- Test: `core/src/application/agents.rs`

- [ ] **Step 1: Add the serialized authority enum**

Add to `core/src/domain/agents.rs`:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelStorageAuthority {
    NativeRegistry,
    ArcaMapping,
    Guided,
}
```

Add `pub storage_authority: ModelStorageAuthority` to `ModelAgentCapabilityView`.

- [ ] **Step 2: Carry authority from the canonical Model matrix**

Add the same typed field to `ModelAgentView` in `core/src/resources/model/mod.rs`. Set:

```text
native-registry: pi, grok-build, opencode, kilo-code, qwen-code, crush,
                 mistral-vibe, hermes, factory-droid, goose
arca-mapping:     claude-code, codex
guided:          minimax-code, qoder
```

Pass the field through `core/src/application/agents.rs` without recreating the matrix.

- [ ] **Step 3: Add the Desktop wire type**

In `desktop/src/lib/types.ts` add:

```ts
export type ModelStorageAuthority = "native-registry" | "arca-mapping" | "guided";
```

and add `storage_authority: ModelStorageAuthority` to the Model capability and `ModelAgentView` interfaces.

- [ ] **Step 4: Lock the exact matrix in a Core test**

Assert the 14 current Agent IDs map exactly to the three sets above. The test must fail if a future Agent is added without an authority.

- [ ] **Step 5: Record verification policy**

Targeted command when tests are authorized:

```bash
cargo test -p arca-core application::agents -- --test-threads=1
```

Expected: the capability projection and exact authority matrix pass. Under the active fast-delivery rule, keep the test unexecuted locally.

### Task 2: Add authority-symmetric native clear-all adapters

**Files:**

- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/resources/model/adapters.rs`
- Test: `core/src/resources/model/mod.rs`
- Test: `core/src/resources/model/adapters.rs`

- [ ] **Step 1: Define the native clear-all entry point**

Expose from `core/src/resources/model/mod.rs`:

```rust
pub(crate) fn clear_all_configured_models(agent_id: &str) -> Result<(), String>;
pub(crate) fn agent_has_configured_models(agent_id: &str) -> Result<bool, String>;
```

Both functions must reject `arca-mapping` and `guided` Agents. The presence check must inspect the same adapter-owned registry fields that clear-all removes.

- [ ] **Step 2: Implement Pi as one two-file transaction**

Add a candidate builder that preserves the JSONC root but replaces the `providers` object with an empty object:

```rust
fn prepare_clear_all_pi_models(path: &Path) -> Result<(Option<String>, String), String> {
    let (root, original) = read_jsonc(path)?;
    let Some(_) = original else { return Ok((None, String::new())); };
    let object = json_root_object(&root, path)?;
    ensure_unique_keys(&object, path, "$root")?;
    set_json_property(
        &object,
        "providers",
        Some(Value::Object(Default::default())),
        path,
        "$root",
    )?;
    Ok((original, root.to_string()))
}
```

Reuse `prepare_clear_pi_settings` to delete `defaultProvider/defaultModel`, then use the existing backup and `write_pi_transaction` path. Missing files remain missing. After commit, `agent_has_configured_models("pi")` must be false.

- [ ] **Step 3: Add clear-all candidates for the remaining native adapters**

In `adapters.rs` add:

```rust
pub(crate) fn prepare_clear_all(
    agent_id: &str,
    config_paths: &[String],
) -> Result<Vec<PreparedModelFile>, String>;

pub(crate) fn has_configured_models(
    agent_id: &str,
    config_paths: &[String],
) -> Result<bool, String>;
```

For every adapter, clear exactly the registry/current fields already observed by that adapter:

- `grok-build`: custom Model entries plus `[models].default`; preserve `fork_secondary_model` and unrelated TOML.
- `opencode`/`kilo-code`: configured provider/models and the coupled current `model` pointer; preserve unrelated JSON/JSONC.
- `qwen-code`: `modelProviders` and the coupled `model.name`; preserve other `model` options.
- `crush`: configured providers and every entry in the native `models` registry, because the approved clear boundary is all real Models.
- `mistral-vibe`: configured providers/models and `active_model`.
- `hermes`: configured custom providers, model aliases, and primary Model pointer; preserve auxiliary task settings.
- `factory-droid`: `customModels` and the coupled current `model`.
- `goose`: declarative custom providers and `active_provider`; preserve unrelated extensions/settings.

Malformed roots, duplicate keys, wrong field types, or ambiguous structures return an error before any write.

- [ ] **Step 4: Reuse existing safe writers**

`clear_all_configured_models` must use existing adapter `PreparedModelFile`, backup, CAS, permission tightening, same-directory temporary files, and atomic commit helpers. It must never remove the whole configuration file.

- [ ] **Step 5: Add destructive-boundary fixtures**

Add isolated HOME/ARCA_HOME tests proving:

```text
Pi: 8 Providers / 16 Models -> providers {}, default pointers absent
Every native adapter: observed registry present -> clear-all candidate -> absent
Unknown fields/comments/non-Model policy remain
Missing file remains missing
Duplicate keys and malformed structures fail before write
```

Targeted commands when tests are authorized:

```bash
cargo test -p arca-core resources::model::tests -- --test-threads=1
cargo test -p arca-core resources::model::adapters::tests -- --test-threads=1
```

Expected: every native observer/clear surface is symmetric. Keep unexecuted under the current fast-delivery rule.

### Task 3: Add a dedicated reviewed `clear_agent_models` operation

**Files:**

- Modify: `core/src/domain/assets.rs`
- Modify: `core/src/application/operations.rs`
- Modify: `core/src/application/assets.rs`
- Modify: `core/src/assets/planner.rs`
- Modify: `core/src/assets/transaction.rs`
- Modify: `core/src/assets/store.rs`
- Modify: `core/src/assets/inventory.rs`
- Modify: `core/src/lib.rs`
- Test: `core/src/assets/planner.rs`
- Test: `core/src/assets/transaction.rs`
- Test: `core/tests/central_assets_e2e.rs`

- [ ] **Step 1: Add the public request and operation kind**

In `core/src/domain/assets.rs` add:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanClearAgentModelsRequest {
    pub agent_id: String,
}
```

Add `ClearModels` to `AssetOperationKind`. Add `ClearAgentModels(PlanClearAgentModelsRequest)` to `PlanOperationRequest` and route it through `core/src/application/assets.rs` to `plan_clear_agent_models`.

- [ ] **Step 2: Add a lifecycle binding modeled after MCP clear**

Add:

```rust
LifecycleBinding::ModelClear {
    agent_id: String,
    storage_authority: ModelStorageAuthority,
    configured_count: usize,
    external_count: usize,
}
```

The counts are non-sensitive reviewed metadata. Include `ClearModels` wherever planner logic classifies relationship target writes and target hash binding.

- [ ] **Step 3: Plan native and mapping clears differently**

`plan_clear_agent_models` must:

1. validate and load the canonical Model capability;
2. reject `guided` with `model_agent_guided`;
3. load current Arca Model selection and current inventory;
4. count all Model `consumptions + external` rows for a native Agent, and only mapped rows for mapping-only;
5. create `DomainPlan::Model { before, after: empty }`;
6. bind every configured Model target path for native authority and the existing writer path for mapping authority;
7. add warnings containing total and external counts for native clear;
8. persist `LifecycleBinding::ModelClear` and exact target hashes.

No-op is allowed only when both the authority's source and Arca mapping are empty.

- [ ] **Step 4: Commit central state then the authority target**

In transaction lifecycle handling:

```text
1. remove agent from model_consumptions and model_assignments;
2. native-registry -> clear_all_configured_models(agent_id);
3. arca-mapping -> clear every Profile in the reviewed before selection using
   clear_profile_consumption with the reviewed active flag;
4. guided -> unreachable because planning rejects it;
5. re-observe; native must have no configured Model, mapping must have no mapping;
6. clear incident on success or record model target_convergence_failed on failure.
```

Target failure must not restore the central mapping. Inventory remains observed-first, so native cards remain visible with an incident.

- [ ] **Step 5: Verify postconditions and recovery**

Extend `verify_operation` so Model clear always verifies empty desired mapping. Native target convergence is verified by `agent_has_configured_models`; a non-empty target leaves/creates an incident and makes the returned commit `converged: false`.

- [ ] **Step 6: Add planner/transaction regressions**

Cover:

- native plan includes external counts and all exact paths;
- mapping plan does not include external observations;
- guided plan is rejected;
- Pi mixed target becomes empty and central catalog/Provider/credentials are unchanged;
- target write failure leaves real rows observable and records one Model incident;
- stale target hash rejects commit before deletion;
- another Agent's Model relationships stay unchanged.

Targeted commands when authorized:

```bash
cargo test -p arca-core assets::planner::tests -- --test-threads=1
cargo test -p arca-core assets::transaction::tests -- --test-threads=1
cargo test -p arca-core --test central_assets_e2e -- --test-threads=1
```

Expected: all clear lifecycle contracts pass. Keep unexecuted locally under fast-delivery policy.

### Task 4: Make Desktop authority-aware and observed-first

**Files:**

- Modify: `desktop/src/lib/types.ts`
- Modify: `desktop/src/hooks/useConsumptionState.ts`
- Modify: `desktop/src/hooks/useConsumptionState.test.tsx`
- Modify: `desktop/src/lib/operations.test.ts`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.tsx`
- Modify: `desktop/src/i18n/index.ts`

- [ ] **Step 1: Add the plan request and hook**

Extend `PlanOperationRequest`:

```ts
| { operation: "clear_agent_models"; request: { agent_id: string } }
```

Extend `AssetOperationPlan.kind` with `"clear-models"` and update the operation serialization test so the exact request shape is locked.

Add to `ConsumptionState` and its implementation:

```ts
planClearAgentModels(agentId: string): Promise<AssetOperationPlan>;
```

using `startPlan({ operation: "clear_agent_models", request: { agent_id: agentId } })`. It must always remain pending for review and never auto-commit.

- [ ] **Step 2: Render counts from authority**

In `AgentView` derive:

```ts
const modelConfiguredCount = modelRows.length + modelExternal.length;
const modelVisibleCount = modelAgent?.storage_authority === "native-registry"
  ? modelConfiguredCount
  : modelRows.length;
```

Use:

```text
native-registry -> 配置中 X 个 · 同一时间使用其中一个
arca-mapping     -> Arca 管理 X 个
guided          -> existing Agent-managed guidance
```

Keep both managed and external cards visible for native Agents. Mapping-only continues to render mapped rows, with actual current drift as status rather than as an added card.

Pass `modelExternal` to `AgentConsumptionPanel` only for `native-registry`; use an empty external list for `arca-mapping`. This prevents a single-slot Agent's observed external current value from being presented as a Arca-added Model.

- [ ] **Step 3: Replace the old empty-selection clear handler**

`clearModels` must call `planClearAgentModels(agentId)`. Set:

```text
label: 清空全部 Models
native title: 清空 <Agent> 真实配置中的全部 Model，包括外部和手工配置；中央资产与凭据保留
mapping title: 清空 <Agent> 的全部 Arca Model 映射；中央资产与凭据保留
```

Enable native clear from `modelConfiguredCount > 0`, mapping clear from `modelRows.length > 0`, and keep guided hidden.

- [ ] **Step 4: Make review risk explicit**

Use lifecycle warnings returned by Core. The review must visibly state total/external counts and target files without duplicating adapter logic in React.

- [ ] **Step 5: Gate success on convergence**

Reuse the hook's existing `pending_convergence` error when commit returns `converged: false`. Do not show the success Toast in that case. Refresh from returned/current inventory so failed native rows remain visible.

- [ ] **Step 6: Add focused UI tests**

Assert:

```text
native 0 managed + 16 external -> “配置中 16 个”, clear enabled
mapping 2 mapped -> “Arca 管理 2 个”
mapping external-current -> status only, not an added card
guided -> no add/clear controls
click clear -> one clear_agent_models plan, no immediate commit
non-converged commit -> cards remain and no success Toast
```

Targeted command when authorized:

```bash
cd desktop
NODE_OPTIONS=--localstorage-file=/private/tmp/arca-model-source-localstorage npx vitest run src/components/AgentView.test.tsx src/hooks/useConsumptionState.test.tsx
```

Expected: authority-aware UI tests pass. Keep unexecuted locally under fast-delivery policy.

### Task 5: Update the repository contract and deliver

**Files:**

- Modify: `AGENTS.md`
- Modify: `docs/superpowers/specs/2026-08-28-agent-model-source-of-truth-design.md`
- Create: `docs/superpowers/plans/2026-08-28-agent-model-source-of-truth.md`
- Review: every file modified in Tasks 1–4

- [ ] **Step 1: Record the reviewed external-clear exception**

Update `AGENTS.md` so external observations remain read-only except for the dedicated, reviewed Agent-scope `clear_agent_models` operation on `native-registry` Agents. Individual external cards remain non-mutating.

- [ ] **Step 2: Audit the exact manifest**

Run `git diff --check`, inspect all intended source/test/docs files, and confirm there are no version, Changelog, generated artifacts, credentials, real configuration paths, or unrelated changes.

- [ ] **Step 3: Create one remote implementation commit**

Use the GitHub Git Data API on `codex/agent-model-source-of-truth` with message:

```text
feat(models): use Agent config as source of truth
```

Verify every remote blob against `git hash-object`, update Draft PR #135 with implementation summary and skipped-local-validation disclosure, wait once for checks, mark ready, inspect mergeability/file list, then squash merge and delete the remote branch.

- [ ] **Step 4: Verify Direct Stable without local installation**

Track the merge-specific `Direct stable release` and `Build desktop` runs. From a source copy at the generated tag run the single-pass four-asset verifier without `--install` or `--launch` because the user updates the App independently.

- [ ] **Step 5: Report evidence**

Report the Stable version/URL, PR, remote implementation commit, merge/release SHAs, Direct Stable/Desktop conclusions, four asset digests, Quality paused state, skipped local tests/build, unchanged installed App, and preserved original checkout.
