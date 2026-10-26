# Scoped Model Provider Lock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove Provider switching when a user adds a Model from inside a specific Provider while preserving Provider selection in global create and edit flows.

**Architecture:** `ModelsView` derives a narrow UI-only lock from the active Provider filter and passes it to `ModelProfileDialog`. The dialog renders the resolved Provider name as a read-only field when locked and keeps the existing `FormSelect` otherwise; Core contracts and persisted data remain unchanged.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library.

**Delivery constraint:** Work only in the isolated worktree, create the commit through the GitHub Git Data API, merge via PR, and verify the Direct Stable release without updating the local Arca app. Arca fast-delivery policy skips local tests/build unless the user explicitly requests them.

---

### Task 1: Lock the scoped-create contract

**Files:**

- Test: `desktop/src/components/ModelsView.test.tsx`

- [ ] **Step 1: Change the existing scoped Provider test**

Replace the expectation that finds a Provider combobox with assertions for a read-only textbox containing `OpenRouter Team` and for the absence of a Provider combobox.

```tsx
const provider = screen.getByRole("textbox", { name: "模型提供商" });
expect(provider).toHaveValue("OpenRouter Team");
expect(provider).toHaveAttribute("readonly");
expect(screen.queryByRole("combobox", { name: "模型提供商" })).not.toBeInTheDocument();
```

- [ ] **Step 2: Preserve global and edit coverage**

Keep the existing tests that call `chooseFormSelect(user, "模型提供商", ...)` from the all-models create flow and the existing Model editor. Those tests remain the regression contract for selectable Provider behavior outside a scoped create.

- [ ] **Step 3: Record the red-state policy**

Do not run the test locally because repository `AGENTS.md` enables fast delivery unless the current user explicitly requests tests. The new assertion is expected to fail against v1.8.142 because the scoped dialog still renders `FormSelect`.

### Task 2: Implement the scoped lock

**Files:**

- Modify: `desktop/src/components/ModelsView.tsx`

- [ ] **Step 1: Pass explicit context into the dialog**

At the create-only `ModelProfileDialog` call, pass:

```tsx
providerSelectionLocked={providerFilter !== null}
```

Do not pass the flag to the inspector editor, so editing continues to allow Provider migration.

- [ ] **Step 2: Extend the dialog contract**

Add the optional prop with a default of `false`:

```tsx
providerSelectionLocked = false,
```

```ts
providerSelectionLocked?: boolean;
```

- [ ] **Step 3: Render a read-only Provider field when scoped**

When `providerSelectionLocked && providerInstance`, render:

```tsx
<input
  aria-label={t("models.provider")}
  className="arca-model-field"
  readOnly
  value={providerInstance.name}
/>
```

Otherwise render the existing `FormSelect` and empty-provider recovery UI unchanged.

### Task 3: Verify and deliver remotely

**Files:**

- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Create: `docs/superpowers/specs/2026-08-27-scoped-model-provider-lock-design.md`
- Create: `docs/superpowers/plans/2026-08-27-scoped-model-provider-lock.md`

- [ ] **Step 1: Audit the diff**

Use `git diff --check`, confirm the manifest contains only the four files above, and inspect the exact diff. Do not run local tests/build under the active fast-delivery policy.

- [ ] **Step 2: Commit through GitHub**

Create remote branch `codex/lock-model-provider-context` from the live `main` SHA and commit the exact manifest with message:

```text
fix(models): lock Provider in scoped Model creation
```

- [ ] **Step 3: Review and merge**

Create a PR, compare remote branch blobs against the prepared worktree, wait for required PR checks, then squash merge and delete the remote branch.

- [ ] **Step 4: Verify Direct Stable read-only**

Wait for the next patch release and verify the Direct Stable workflow plus the complete Release asset set. Do not install or launch the release locally.

