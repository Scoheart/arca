# Arca repository Rulesets

Arca permanently accepts validated direct pushes to `main`; there is no main branch Ruleset in the current delivery model.

`tags.json` is the auditable request body for the live **Arca immutable stable tags** Ruleset. It allows a new `v*` tag to be created, then blocks every update, force-push, and deletion. It deliberately has no `creation` rule and no bypass actor, so release automation can create a tag but cannot move it later.

The committed JSON remains non-enforcing because it is a reviewable API request body, not a declaration that an external mutation has already happened. The live tag Ruleset must remain `active`. Read its effective state with:

```bash
gh api repos/Scoheart/arca/rulesets --jq '.[] | {id,name,target,enforcement}'
```

Do not delete, disable, replace, or bypass the live immutable-tag Ruleset during ordinary delivery. Product defects use a new patch release; partial asset publication is retried only against the existing Draft and tag.
