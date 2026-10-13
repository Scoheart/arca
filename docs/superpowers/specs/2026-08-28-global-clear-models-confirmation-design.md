# Global Clear Models Confirmation

## Problem

Clearing every Model for an Agent currently replaces the Models content area with the generic `AssetOperationReviewDialog`. The review surface is visually oversized for one destructive action, exposes implementation details such as target paths and plan sections, and causes the Agent page to reflow.

The user wants this inline review surface removed. Clearing Models must still retain the existing plan → confirm → commit safety boundary.

## Decision

`clear-models` uses a compact, window-level confirmation dialog. It is rendered through Arca's existing portal-backed `DialogShell`, so the overlay covers the application window and does not participate in the Models panel layout.

The generic `AssetOperationReviewDialog` remains unchanged for other operation kinds.

## Interaction

1. The user clicks **清空全部 Models**.
2. Arca prepares the existing `clear_agent_models` plan.
3. If planning fails, Arca shows the existing error toast and does not open a dialog.
4. If planning succeeds, Arca opens a global review dialog over the whole application.
5. The dialog contains only:
   - title: **清空全部 Models？**
   - message: **将删除 N 个 Models**
   - actions: **取消** and a danger-styled **清空** button
6. Confirming commits the exact prepared operation. Cancelling cancels the prepared operation.
7. While committing, the dialog stays open, disables closing/actions, and shows the existing busy state.
8. A commit failure keeps the dialog open and displays one concise error. Success closes it, refreshes state through the existing hook, and shows the existing success toast.

## Count

`N` is the number represented by the prepared clear operation, including Models observed directly in a native Agent registry and external/manual Models included by the reviewed clear-all contract.

The UI must prefer counts carried by the plan. It may use the current visible Model count only as a defensive fallback when older plan data lacks the clear binding.

## Layout and accessibility

- Use `DialogShell` with `kind="review"` and its small width.
- Rendering stays portal-backed under `document.body`.
- The scrim covers the full window; the Models panel remains visible behind it without reflow.
- Reuse the existing modal stack, focus trap, focus restoration, Escape handling, inert background, and accessible title behavior.
- Do not show target paths, Agent-change sections, external/manual labels, step badges, or generic “检查并应用 / 应用更改” copy.

## Scope

In scope:

- Agent-page `clear-models` confirmation only.
- A compact component or focused specialization using existing Dialog primitives.
- Regression coverage for portal placement, minimal copy, count, cancel, confirm, busy, and failure behavior.

Out of scope:

- Changing Core planning or transaction semantics.
- Changing what is deleted.
- Removing confirmation for destructive clears.
- Restyling MCP, Skill, configuration, registry, or other Model operation reviews.

## Failure and safety behavior

- The plan remains the source of the operation id, candidate hashes, warnings, and commit eligibility.
- A non-committable plan must not expose an enabled confirm action.
- Confirm commits the already prepared plan; it never prepares a second operation.
- Cancel invokes the existing operation cancellation path.
- Existing target incidents and post-commit convergence behavior remain unchanged.

## Acceptance criteria

- Clicking **清空全部 Models** never replaces or resizes the Models panel.
- A centered application-level modal appears above the entire Arca window.
- The modal displays only the compact title, `将删除 N 个 Models`, and two actions, plus a concise error only after failure.
- The destructive action remains plan → explicit confirmation → commit.
- Other operation review surfaces behave exactly as before.
