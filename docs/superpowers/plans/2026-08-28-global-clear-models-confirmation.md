# Global Clear Models Confirmation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Agent Models inline clear-all review surface with a compact portal-backed global confirmation dialog while preserving the existing plan/commit safety contract.

**Architecture:** Reuse the existing `ReviewDialog` and `DialogShell` modal stack instead of adding another modal primitive. `AgentView` special-cases only `clear-models`: it keeps the resource panel rendered, portals the compact dialog above the application, and commits the already prepared plan. The generic operation review explicitly rejects both clear-all kinds so it cannot regress into the Models layout.

**Tech Stack:** React 19, TypeScript, Vitest, Testing Library, existing Arca `ReviewDialog`/`DialogShell`/`Modal` primitives.

---

## File structure

- Modify `desktop/src/components/AgentView.tsx`: route the prepared `clear-models` plan to the global `ReviewDialog` while keeping Models content mounted.
- Modify `desktop/src/components/AgentView.test.tsx`: extend the existing clear-all scenario to prove portal placement, compact copy, retained Models layout, confirm, and cancel behavior.
- Modify `desktop/src/components/AssetOperationReviewDialog.tsx`: refuse to render the generic inline review for `clear-models` as a defensive boundary.
- Modify `desktop/src/components/AssetOperationReviewDialog.test.tsx`: consolidate the existing clear-all guard test across MCP and Models.
- Add `docs/superpowers/plans/2026-08-28-global-clear-models-confirmation.md`: implementation and verification record.

### Task 1: Lock the clear-all presentation contract

**Files:**
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.test.tsx`

- [ ] **Step 1: Extend the Agent clear Models scenario before implementation**

After the existing click and plan assertion, rerender `AgentView` with the prepared plan active and assert both the still-mounted Models panel and the portal-backed compact dialog:

```tsx
view.rerender(
  <ToastProvider>
    <AgentView
      state={{ ...state, agents: [piAgent] } as unknown as InstallState}
      skillsState={taskSkillsState}
      consumptionState={{ ...consumptionState, plan }}
      agentId="pi"
    />
  </ToastProvider>,
);

expect(screen.getByText("配置中 3 个 · 同一时间使用其中一个")).toBeVisible();
const dialog = screen.getByRole("dialog", { name: "清空全部 Models？" });
expect(dialog.closest('[data-modal-overlay="true"]')).not.toBeNull();
expect(within(dialog).getByText("将删除 3 个 Models")).toBeVisible();
expect(within(dialog).queryByText(/models\.json|settings\.json|external|检查并应用/)).not.toBeInTheDocument();

await userEvent.click(within(dialog).getByRole("button", { name: "清空" }));
await waitFor(() => expect(commit).toHaveBeenCalledTimes(1));
```

Add a `cancel` mock to the scenario and rerender once more with the plan active so clicking **取消** proves the existing cancellation path is used:

```tsx
view.rerender(
  <ToastProvider>
    <AgentView
      state={{ ...state, agents: [piAgent] } as unknown as InstallState}
      skillsState={taskSkillsState}
      consumptionState={{ ...consumptionState, plan }}
      agentId="pi"
    />
  </ToastProvider>,
);
const cancelDialog = screen.getByRole("dialog", { name: "清空全部 Models？" });
await userEvent.click(within(cancelDialog).getByRole("button", { name: "取消" }));
expect(cancel).toHaveBeenCalledTimes(1);
```

- [ ] **Step 2: Consolidate the generic review guard test**

Replace the single clear-MCP guard with a table covering both clear-all kinds:

```tsx
it.each(["clear-mcp", "clear-models"] as const)(
  "never renders the generic confirmation view for %s",
  (kind) => {
    const plan = assetOperationPlanFixture();
    plan.kind = kind;
    plan.domain_plan = kind === "clear-mcp"
      ? { domain: "mcp", before: {}, after: {} }
      : { domain: "model", before: {}, after: {} };

    const { container } = render(
      <AssetOperationReviewDialog
        plan={plan}
        busy={false}
        agentId="pi"
        agentName="Pi Coding Agent"
        onCommit={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  },
);
```

- [ ] **Step 3: Record the project validation gate**

Arca fast mode forbids local Vitest/build execution unless the current user explicitly requests it. Keep the tests as executable regression coverage but do not run them locally. The red evidence is the current installed v1.8.146 screenshot and source behavior: an active `clear-models` plan selects the inline review branch and suppresses the Models content.

### Task 2: Route clear Models to the global modal stack

**Files:**
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.tsx`

- [ ] **Step 1: Import the existing global review primitive**

Add the existing component import beside the other dialogs:

```tsx
import { ReviewDialog } from "./ReviewDialog";
```

- [ ] **Step 2: Commit the prepared clear plan without the generic review helper**

Add a focused handler next to `clearModels` so failures propagate into `ReviewDialog`'s concise status and success retains the existing toast:

```tsx
const commitClearModels = async () => {
  await consumptionState.commit();
  showToast({
    kind: "success",
    msg: `${agent.name} 的全部 Model 已从权威配置中移除。`,
  });
};
```

- [ ] **Step 3: Portal the compact dialog without replacing the resource panel**

Render the special dialog as a sibling before the existing review/content branch:

```tsx
{consumptionState.plan?.kind === "clear-models" && !preparingChange && (
  <ReviewDialog
    title="清空全部 Models？"
    confirmLabel="清空"
    onConfirm={commitClearModels}
    onClose={() => void consumptionState.cancel()}
  >
    <p>将删除 {modelVisibleCount} 个 Models</p>
  </ReviewDialog>
)}
```

Restrict the generic inline branch so clear Models falls through to the normal resource content:

```tsx
// Before
{consumptionState.plan && !preparingChange ? (
```

```tsx
// After
{consumptionState.plan
  && consumptionState.plan.kind !== "clear-models"
  && !preparingChange ? (
```

Only this condition changes. Keep the existing `AssetOperationReviewDialog` props and the complete fallback fragment byte-for-byte unchanged.

This preserves the Models DOM behind the portal, prevents layout reflow, and lets the existing modal scrim/focus stack own window-level behavior.

- [ ] **Step 4: Make the generic review boundary explicit**

Update the early return:

```tsx
if (plan.kind === "clear-mcp" || plan.kind === "clear-models") return null;
```

No other review rendering or copy changes.

### Task 3: Inspect and deliver the exact change

**Files:**
- Modify: `desktop/src/components/AgentView.tsx`
- Modify: `desktop/src/components/AgentView.test.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.tsx`
- Modify: `desktop/src/components/AssetOperationReviewDialog.test.tsx`
- Add: `docs/superpowers/plans/2026-08-28-global-clear-models-confirmation.md`

- [ ] **Step 1: Run non-executing source checks only**

```bash
git diff --check
rg -n 'clear-models|清空全部 Models|将删除 .* 个 Models' \
  desktop/src/components/AgentView.tsx \
  desktop/src/components/AgentView.test.tsx \
  desktop/src/components/AssetOperationReviewDialog.tsx \
  desktop/src/components/AssetOperationReviewDialog.test.tsx
```

Expected: `git diff --check` exits 0; the clear plan has exactly one global dialog route and the generic review has the defensive exclusion.

- [ ] **Step 2: Create one remote implementation commit**

Use the existing `codex/global-clear-models-confirmation` branch and an exact manifest. Do not run local `git commit` or `git push`.

Commit message:

```text
fix(models): use a global clear confirmation
```

- [ ] **Step 3: Verify the remote tree and merge through PR**

Compare `main...codex/global-clear-models-confirmation`, verify every remote blob against `git hash-object`, create the PR, wait once for configured checks, inspect mergeability and the exact file list, then squash merge and delete the remote branch.

- [ ] **Step 4: Verify the generated Stable once**

Track the exact `Direct stable release` run for the merge SHA and the dispatched `Build desktop` run. Run:

```bash
RELEASE_TAG=$(gh release list --repo Scoheart/arca --limit 1 --json tagName --jq '.[0].tagName')
RELEASE_SHA=$(gh api "repos/Scoheart/arca/git/ref/tags/$RELEASE_TAG" --jq '.object.sha')
VERIFY_WORKTREE="/Users/scoheart/.config/superpowers/worktrees/arca/verify-${RELEASE_TAG}"
git worktree add --detach "$VERIFY_WORKTREE" "$RELEASE_SHA"
bash .agents/skills/arca-release/scripts/verify-release.sh \
  --repo Scoheart/arca \
  --source-root "$VERIFY_WORKTREE" \
  --wait \
  "$RELEASE_TAG"
```

Expected: published non-prerelease Stable; immutable tag/release/main identity; exactly four verified assets; signed arm64 App/DMG/CLI/updater; no local installation because the user updates Arca independently.
