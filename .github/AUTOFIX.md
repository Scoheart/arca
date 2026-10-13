# Automated issue repair

Arca uses a guarded automation chain:

1. `Quality monitor` runs on pushes, pull requests, manual dispatches, and every day at 09:17 Asia/Shanghai.
2. A failure is reported only while its commit is still the current `main` head. One sticky `ci-failure` Issue is reopened and its body updated across failure cycles; superseded commits do not comment or create Issues.
3. When Copilot dispatch is configured, a newly created or reopened failure cycle is assigned to Copilot cloud agent once.
4. Copilot works on a branch and creates a PR. It never writes directly to `main`.
5. When the PR is ready, `Notify repair review` requests review from `Scoheart`, adds `needs-review`, and posts an @mention.

Public Issue text is untrusted. An arbitrary contributor cannot start a repair: manual dispatch requires the repository owner to add the `autofix` label or run the workflow.

Recovery closes the sticky Issue without adding a recovery comment or mentioning anyone. When
`COPILOT_PAT` is absent, that state appears in the Issue body instead of a separate @mention
comment. Dispatch failures still mention the owner because they require intervention.

## Notification policy

Repository workflows intentionally keep every quality and release check visible in Actions, but
routine successful runs should not be email. In GitHub **Settings → Notifications → System → Actions**:

1. Enable notifications only for failed workflows.
2. Leave participating notifications enabled for Issues that require your response.
3. Keep `ci-failure` subscribed; it is one reused Issue rather than one Issue per failure cycle.
4. Do not watch all repository activity solely to receive CI failures; use the Actions failure
   setting instead.
5. Expect `Quality monitor`, `Build desktop`, and `Direct stable release` to have frequent
   successful runs during Fast Lane. `Fast Lane Expiry` runs at the exact deadline plus two
   low-noise checks per day, and restoration failures remain visible as a failed run and Issue.

## One-time setup

1. Enable Copilot cloud agent for the account and this repository. Confirm `copilot-swe-agent` appears in the Issue assignee picker.
2. Create a fine-grained user token following GitHub's Copilot assignment API requirements: metadata read plus Actions, Contents, Issues, and Pull requests read/write access for `Scoheart/arca`.
3. Add the token as the repository Actions Secret `COPILOT_PAT`.
4. Run `Quality monitor` manually once. To test the repair path safely, create a small Issue and add the `autofix` label yourself.

Do not put a broad account token into `COPILOT_PAT`; scope it to this repository and rotate it independently.

Official references:

- https://docs.github.com/en/copilot/how-tos/use-copilot-agents/cloud-agent/use-cloud-agent-via-the-api
- https://docs.github.com/en/copilot/how-tos/copilot-on-github/use-copilot-agents/review-copilot-output
