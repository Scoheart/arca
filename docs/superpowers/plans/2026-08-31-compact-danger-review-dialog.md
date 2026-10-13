# Compact Danger Review Dialog Implementation Plan

**Goal:** Replace the sparse Clear Models confirmation with a compact, readable danger review while preserving modal safety behavior.

**Architecture:** Keep `Modal` and `DialogShell` as the accessibility infrastructure. Add narrowly scoped presentation hooks to `DialogShell`, make `ReviewDialog` own the reusable danger styling, and let `AgentView` provide operation-specific impact copy.

**Tech stack:** React, TypeScript, CSS, Vitest/Testing Library.

---

### Task 1: Lock the Clear Models presentation contract

**Files:**
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/components/ReviewDialog.test.tsx`

1. Update the Clear Models test to require the count-aware title area, destructive scope, preserved-data note, and `清空 <count> 个 Models` button.
2. Add a ReviewDialog assertion for the danger shell class, decorative glyph, and solid danger action.
3. Retain existing confirm, cancel, and busy-state assertions.

Per Arca fast mode, write the regression coverage but do not execute local tests unless the user explicitly requests it.

### Task 2: Add reusable danger-review presentation hooks

**Files:**
- Modify: `desktop/src/components/DialogShell.tsx`
- Modify: `desktop/src/components/ReviewDialog.tsx`
- Modify: `desktop/src/index.css`

1. Add optional `leading` and `className` props to `DialogShell`.
2. Render the leading visual before the heading without changing focus order or accessible naming.
3. Make danger `ReviewDialog` instances supply a red-tinted `TrashIcon`, danger shell class, and solid danger button class.
4. Add scoped CSS for compact header/body/footer spacing, the glyph tile, impact blocks, and responsive behavior.
5. Leave global modal, primary button, and inline danger button styles unchanged.

### Task 3: Apply the Clear Models hierarchy

**Files:**
- Modify: `desktop/src/components/AgentView.tsx`

1. Remove the question mark from the title.
2. Add the Agent/count subtitle.
3. Replace the single generic sentence with structured impact, danger, and preservation copy.
4. Make the confirmation label count-aware.

### Task 4: Static audit and Direct Stable delivery

**Files:**
- Add: `docs/superpowers/specs/2026-08-31-compact-danger-review-dialog-design.md`
- Add: `docs/superpowers/plans/2026-08-31-compact-danger-review-dialog.md`

1. Run `git diff --check` and inspect every changed path.
2. Skip local tests, builds, formatters, validator, and preflight per Arca fast mode.
3. Create a remote-only feature commit and PR, verify exact blobs and file list, then squash merge.
4. Follow the exact Direct Stable run and verify the four published assets.
5. Do not install or launch Arca because the user previously requested to update it themselves.
