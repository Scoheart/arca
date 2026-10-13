# Model Card Context Implementation Plan

> **For Codex:** Implement against the live remote `main` in the isolated worktree. Follow Arca fast mode: retain focused test coverage but do not run local tests/builds unless the user explicitly requests them.

**Goal:** Make context-window size visible and explicit on Model cards, and preserve Provider-discovered context metadata when users select a model.

**Architecture:** Keep the change in the React adapter. The existing `ModelProfile.context_window` remains the source of truth; Provider discovery only seeds the editable draft. No core schema or writer changes are required.

**Tech Stack:** React, TypeScript, Vitest/Testing Library, CSS, i18next.

---

### Task 1: Lock the behavior in focused component tests

**Files:**
- Modify: `desktop/src/components/ModelsView.test.tsx`

1. Require the Model card to render an explicit localized context label using the persisted profile value.
2. Require Provider catalog selection to populate `context_window`.
3. Require a later manual Model ID edit to clear the stale auto-filled value.

### Task 2: Preserve discovered context metadata

**Files:**
- Modify: `desktop/src/components/ModelsView.tsx`

1. Track whether the current context value came from the Provider model picker.
2. Copy a positive `context_length` into the draft on selection.
3. Clear only that auto-filled value when the user manually edits Model ID or changes Provider.
4. Treat direct context-field edits as user-owned.

### Task 3: Make card context explicit

**Files:**
- Modify: `desktop/src/components/ModelsView.tsx`
- Modify: `desktop/src/index.css`

1. Replace the unlabeled separator with a localized `Context <size>` chip.
2. Add an exact-token tooltip.
3. Keep the chip quiet, compact, and responsive beside the Model ID.

### Task 4: Deliver through remote PR and Stable

1. Inspect the exact diff and run `git diff --check`; skip local tests/builds per Arca fast mode.
2. Create a remote-only feature commit from the current GitHub `main`, verify every remote blob, open a PR, and squash merge it.
3. Observe Direct Stable and run the single release verifier against the immutable tag without installing or launching the app.

