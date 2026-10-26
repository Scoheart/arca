# Clear Models from one Agent

## Remove every managed Model together

Add one destructive action that removes every Arca-managed Model from the currently selected Agent in a single reviewed operation.

## Keep central assets and external Models unchanged

The action applies only to the selected Agent's desired Model consumption set. It removes the Agent relationships and the corresponding Arca-owned entries from that Agent's configured Model files.

It does not:

- delete Models from the central Models library;
- delete Providers or Keychain credentials;
- remove Models observed as external and not managed by Arca;
- affect another Agent's Model relationships.

## Confirm the complete removal before writing

The Models panel places a danger-style `清空 Models` button immediately before `添加 Model`. The button is enabled only when the Agent has at least one Arca-managed Model and no Model change or asset plan is in progress.

Clicking the button always prepares a review. It never commits immediately, even if the generated plan has no warnings. The existing review surface lists every relationship removal, every Model state transition, the Agent configuration files that will be updated, and any current-Model fallback computed by Core. The user must confirm the plan before Arca writes anything.

After success, the Models panel shows its empty state and reports that every Model was removed from the selected Agent. External observations remain visible as read-only cards.

## Reuse the existing consumption transaction

The Desktop reuses the existing `set_agent_consumption` operation with an empty Model selection:

```ts
{
  agent_id: currentAgentId,
  selection: { domain: "model", profile_ids: [] },
}
```

Core already interprets this as one replacement of the Agent's complete desired Model set. Planning calculates all relationship removals and Model state changes together; commit continues to use the existing operation ID, candidate hash, target hashes, backups, CAS checks, rollback behavior, and post-write inventory verification.

No new Core lifecycle verb or central deletion path is added. The UI does not loop over individual Models.

## Fail closed when reviewed state changes

- A conflict, ambiguous target, or unsafe observed state leaves the plan non-committable and shows the existing warning copy.
- A stale candidate or concurrent Agent configuration change fails closed without silently recomputing a different deletion set.
- A commit failure remains visible in the review surface and keeps the current inventory available for refresh or retry.
- The control stays disabled while another Model switch, plan, or commit is active.

## Verify the clear action and its boundaries

Frontend regression coverage verifies that:

- the action is visible when managed Models exist;
- clicking it plans `{ domain: "model", profile_ids: [] }` for the selected Agent;
- the bulk action does not commit before review;
- external Model observations are not included in the requested desired selection;
- the action is disabled when there are no managed Models.

The repository's current fast-delivery policy leaves local test and build commands unexecuted unless the user explicitly requests them. The test code remains in the change, and the merged Stable release must still pass the production build, packaging, signing, and independent artifact verification gates.

## Acceptance criteria

1. A selected Agent with managed Models exposes `清空 Models` beside `添加 Model`.
2. One click opens a single review for all managed Models on that Agent.
3. Confirming the review removes all of those relationships and Arca-owned Agent configuration entries atomically through the existing planner/transaction contract.
4. Central Models, Providers, Keychain credentials, other Agents, and external observations remain unchanged.
5. Empty, busy, conflicting, stale, and failed states are safe and explicit.
