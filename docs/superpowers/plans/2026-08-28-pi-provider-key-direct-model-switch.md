# Pi Provider Key and Direct Model Switch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Use readable Arca Provider names in Pi configuration and make current-Model switching a direct action without a review panel.

**Architecture:** Core derives a collision-safe Pi Provider identity from shared `ModelProviderConfig`, materializes it only for managed Pi writes, and updates Pi Provider entries model-by-model so shared Providers remain intact. Desktop routes current-Model changes through the existing immediate plan/commit transaction instead of storing a review plan.

**Tech Stack:** Rust, JSONC CST editing, React, TypeScript, Vitest, GitHub Git Data API.

---

### Task 1: Specify readable Pi Provider identity and shared-model behavior

**Files:**
- Modify: `core/src/resources/model/mod.rs`
- Modify: `core/src/assets/model_migration.rs`

- [ ] Add focused Rust tests proving `OpenRouter` maps to `openrouter`, explicit `native_ids.pi` wins, normalized-name collisions receive a stable Provider-ID suffix, and `observe_active_model_for_settings` recognizes the generated key.
- [ ] Add Pi JSONC tests proving two Profiles on one Provider coexist in one `models` array, the old `arca-{profile.id}` entry is removed, unrelated Providers and unknown fields survive, and clearing one Profile preserves its sibling.
- [ ] Implement `generated_pi_provider_id(settings, profile)` and use it in runtime materialization, active-model observation, and migration ownership matching.
- [ ] Split Pi Provider/model value construction so upsert replaces only the matching model ID and clear removes only that model.
- [ ] Remove a legacy `arca-{profile.id}` entry only when its model array contains the same managed Model.

Expected contract:

```json
{
  "providers": {
    "openrouter": {
      "baseUrl": "https://openrouter.ai/api/v1",
      "models": [
        { "id": "nvidia/nemotron-3-ultra-550b-a55b:free" },
        { "id": "qwen/qwen3" }
      ]
    }
  }
}
```

### Task 2: Make current-Model switching immediate

**Files:**
- Modify: `desktop/src/hooks/useConsumptionState.ts`
- Modify: `desktop/src/hooks/useConsumptionState.test.tsx`
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentView.test.tsx`

- [ ] Update tests so the switch calls `setActiveModel("pi", "qwen")`, commits through the immediate operation, shows success/error toasts, and never renders `确认切换当前 Model` or `检查并应用`.
- [ ] Add `setActiveModel` to `ConsumptionState`, implemented with `executeImmediately({ operation: "set_active_model", ... })`.
- [ ] Replace `planActiveModel` plus `requiresAgentReview` in `AgentView` with the immediate method while retaining the `changingModel` disabled state.
- [ ] Leave generic review policy and destructive confirmation components unchanged.

### Task 3: Static audit and remote-only delivery

**Files:**
- Add: `docs/superpowers/specs/2026-08-28-pi-provider-key-direct-model-switch-design.md`
- Add: `docs/superpowers/plans/2026-08-28-pi-provider-key-direct-model-switch.md`

- [ ] Run `git diff --check` and inspect every changed path. Per Arca fast mode, retain tests but do not run local tests/build/formatter/preflight.
- [ ] Create one exact remote manifest, remote commit, PR, blob comparison, squash merge, and remote branch deletion.
- [ ] Observe the exact Direct Stable run and verify the immutable tag plus DMG, updater, CLI, and `latest.json` without installing or launching Arca.

