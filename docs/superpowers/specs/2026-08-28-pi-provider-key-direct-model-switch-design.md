# Pi Provider Key and Direct Model Switch Design

## Goals

1. Pi `models.json` Provider keys follow the shared Arca Provider name instead of embedding a Model Profile ID.
2. Switching the current Model is a direct action and never exposes the generic asset review panel.

## Pi Provider identity

- A managed Profile without an adopted Pi-native identity derives its key from the normalized Arca Provider display name: `OpenRouter` becomes `openrouter`.
- Profiles linked to the same Arca Provider share one Pi Provider entry and are represented as sibling objects in that entry's `models` array.
- Provider names are unique in Arca. If distinct names normalize to the same slug, append the stable Arca Provider ID to the slug only for those collisions.
- An explicitly adopted `native_ids.pi` remains authoritative and is never renamed.
- On the next Pi write, remove the old Arca-generated `arca-{profile.id}` entry when it contains the same managed Model, then write the short Provider key.
- Removing one Profile removes only its model from the shared Pi Provider entry. Remove the Provider entry only after its final model is removed.
- Preserve unrelated Provider keys, sibling models, comments, unknown JSON fields, and Agent policy.

Provider display-name renames affect newly generated Pi identities on the next managed write. This change guarantees migration from the current `arca-{profile.id}` scheme; it does not guess ownership of arbitrary unprefixed legacy keys.

## Direct current-Model switching

- Expose a `setActiveModel` immediate operation from `useConsumptionState`, using the same plan-and-commit transaction path as MCP and Skill toggles.
- `AgentView` invokes that operation directly and shows the existing success or failure toast.
- Because the immediate operation never stores a pending review plan, `AssetOperationReviewDialog` does not render during the switch.
- Uncommittable plans and commit failures remain fail-closed. Delete, clear, conflict, cross-Agent, and other reviewed operations retain their existing confirmations.

## Verification contract

- `OpenRouter` produces `openrouter`, not `arca-openrouter-<model>-<hash>`.
- Two models on one Provider coexist under one Pi Provider key.
- A normalized-name collision gets a stable suffix.
- Adopted native identity and unrelated Pi configuration remain unchanged.
- Current-Model switching calls the immediate operation, never the review flow, and retains toast/error behavior.

