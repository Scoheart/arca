# Full-width Model Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Model ID picker and its discovery menu the full form width so Provider model names and IDs remain readable.

**Architecture:** Keep the existing two-column grids. In the Protocol/Model grid, let Protocol remain in the first column and make the Model picker span both columns with `grid-column: 1 / -1`, so its existing absolutely positioned suggestion menu naturally spans the entire row.

**Tech Stack:** React 19, TypeScript, CSS, Vitest, Testing Library.

**Delivery constraint:** Prepare in the isolated worktree, create the commit with the GitHub Git Data API, merge through a PR, and verify Direct Stable without installing the App. Under Arca fast-delivery policy, do not run local tests/build unless explicitly requested.

---

### Task 1: Define the layout contract

**Files:**

- Test: `desktop/src/components/ModelsView.test.tsx`

- [ ] **Step 1: Add focused DOM assertions**

Extend the existing provider-discovery creation test after locating the Model ID combobox:

```tsx
const modelPicker = modelId.closest(".arca-provider-model-picker");
expect(modelPicker).toHaveClass("arca-model-form-wide");
expect(modelPicker?.parentElement).toHaveClass("arca-model-form-grid");
expect(screen.getByRole("combobox", { name: "协议" }).closest(".arca-model-form-grid"))
  .toBe(modelPicker?.parentElement);
expect(css).toMatch(/\.arca-provider-model-picker\s*\{[^}]*grid-column: 1 \/ -1/);
```

These assertions verify structure instead of pixel values: Protocol and Model stay in one grid, while CSS makes the picker span the entire row.

- [ ] **Step 2: Record the red-state policy**

Do not execute the test locally under the repository fast-delivery rule. Against v1.8.143, the Model picker lacks the cross-column rule, so the new contract describes the missing behavior.

### Task 2: Make the picker full width

**Files:**

- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/index.css`

- [ ] **Step 1: Mark the Model picker as the wide grid item**

Keep the Protocol/Model grid and render the picker as:

```tsx
<div className="arca-model-form-field arca-provider-model-picker arca-model-form-wide">
```

Keep its inner controls, discovery states, options, and handlers unchanged.

- [ ] **Step 2: Span both grid columns**

Add:

```css
.arca-provider-model-picker { grid-column: 1 / -1; }
```

The existing `.arca-provider-model-options { left: 0; right: 0; }` then spans the full-width picker without new positioning or media rules. Protocol remains in the first grid column on desktop and naturally fills the single column under the existing narrow-screen rule.

### Task 3: Verify and deliver remotely

**Files:**

- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/components/ModelsView.test.tsx`
- Modify: `desktop/src/index.css`
- Create: `docs/superpowers/specs/2026-08-27-full-width-model-picker-design.md`
- Create: `docs/superpowers/plans/2026-08-27-full-width-model-picker.md`

- [ ] **Step 1: Audit the exact diff**

Run `git diff --check`, inspect the five-file manifest, and confirm no version, changelog, discovery, persistence, or Provider-selection logic changed.

- [ ] **Step 2: Commit and merge remotely**

Create `codex/widen-model-picker` from live `main`, commit the exact manifest through the GitHub API with `fix(models): widen the Model picker`, verify remote blobs, create the PR, wait once for checks, and squash merge.

- [ ] **Step 3: Verify the Stable release**

Track the merge-specific Direct Stable and Desktop runs, then run the release verifier once for all four assets without `--install` or `--launch`.
