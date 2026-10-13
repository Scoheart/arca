# Qoder CLI custom model storage audit

Audited on 2026-09-12 against the official npm package `@qoder-ai/qodercli@1.1.50`.

- Announcement: https://x.com/qoder_ai_ide/status/2098620121363399105
- Package: https://registry.npmjs.org/@qoder-ai/qodercli/1.1.50
- `bundle/qodercli.js` SHA-256: `788081c288899817431425b2445bbf1c67d1ee74db9c74d4f885e31a813b0f3b`.
- User documentation: https://docs.qoder.com/cli/custom-models (still describes the older curated-provider wizard at audit time).

The announcement introduces arbitrary Base URL, Model ID and API key for individual accounts. Package inspection confirms the local custom-provider form calls `JGo`, validates through `nBe`, and persists through `yOA` to `settings.providers`. The path comes from `fi.getGlobalSettingsPath()`, normally `~/.qoder/settings.json`. `KX`/`J2l` recursively expand `$VAR` and `${VAR}`. `D2n` saves the global selection in user `model.name`; public model IDs use `<providerId>/<modelId>`. The model ID may itself contain slashes.

The registry is shared with [Qoder Desktop](qoder-desktop-models.md): `type: openai-compatible`, `protocol`, `authType`, `baseUrl`, `apiKey`, provider-local `model`, and `models[]`. Verified protocols are `openai`, `openai-responses`, and `anthropic`. Anthropic uses `api-key` authentication; the OpenAI protocols use `bearer`. The CLI global selection is independent of Desktop conversation selection.

Arca now exposes CLI as a managed native registry with multiple installed models and one current model. Writes preserve comments, MCP, hooks, provider metadata, and `model.preferences`. Adding an inactive model preserves the current selection. Removing a selected record clears only its matching selection; removing an unrelated record preserves it.

New CLI providers use `arca_cli_<hex-profile-id>`, distinct from Desktop providers. External CLI imports create a central asset without taking over the shared provider; explicitly adding the imported asset creates its own CLI slot. Desktop cannot adopt CLI-owned slots. Reviewed clear-all is blocked while the other Qoder target has consumption relationships in the same file; individual removal remains available. Without such relationships, clear-all removes the reviewed native custom models, including external records, from the shared registry.

Credential delivery is native environment reference only. Set the central Provider credential source to an environment variable and make it available to the Qoder CLI process; Arca writes `${VARIABLE_NAME}`. A Keychain-only credential cannot be passed automatically because the audited runtime has no credential-helper contract. Arca does not export it to a plaintext key or `.env` file. The CLI writer still uses private encrypted rollback snapshots and suppresses ordinary plaintext backups, including custom config path overrides, because the shared file can already contain Desktop or manually entered keys.

The package was inspected without a Qoder account, real credentials, or paid inference. Production compilation verifies the Arca implementation; regression cases cover shared-registry isolation, selection, malformed JSONC, discovery identity and credential constraints. This is a version-specific source audit, not a live authenticated inference test. Restart CLI after changing its configuration; CLI account access checks and runtime environment resolution remain Qoder's responsibility.
