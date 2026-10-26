# Claude Desktop Direct Models Design

## Goal

Add Claude Desktop as a managed Arca Model Agent on macOS. Applying an Anthropic Messages-compatible Model creates one Arca-owned third-party inference profile; non-Claude model routes automatically opt out of Claude Desktop's Claude-name verification.

The direct Claude Desktop Profile remains a user-approved security exception to Arca's normal credential policy: Claude Desktop requires `inferenceGatewayApiKey` to be a literal string, so an explicitly reviewed apply exports the selected Provider credential from Keychain into Claude Desktop's private `0600` profile file.

## Product contract

- Claude Desktop appears in the Agent workspace with a managed, single-active-Model capability.
- The adapter accepts only `anthropic-messages` Profiles. It does not silently translate OpenAI or Gemini protocols.
- Applying a Profile creates or updates one deterministic `Arca` entry in Claude Desktop's `configLibrary` and makes that entry current.
- A route is Claude-compatible when its final model path segment starts with `claude-`. For example, both `claude-sonnet-4-6` and `anthropic/claude-sonnet-4-6` are Claude-compatible; `qwen3.7-max` is not.
- Non-Claude routes set `unstableDisableModelVerification: true`.
- Claude-compatible routes omit `unstableDisableModelVerification`; Arca never weakens verification when it is unnecessary.
- Every Arca profile disables Claude Desktop model discovery and writes one explicit `inferenceModels` entry. Provider model discovery remains a Arca Models-page concern and does not depend on the Provider returning Claude Desktop's expected `/v1/models` envelope.
- Claude Desktop must be restarted after apply or clear for the running app to refresh; only apply currently returns `restart_required`.

## Configuration targets

The macOS adapter owns narrowly scoped fields across these targets:

1. `~/Library/Application Support/Claude/claude_desktop_config.json`
   - Set only `deploymentMode` to `3p`.
   - Preserve MCP servers and every unrelated field.
2. `~/Library/Application Support/Claude-3p/claude_desktop_config.json`
   - Set only `deploymentMode` to `3p`.
   - Preserve MCP servers and every unrelated field.
3. `~/Library/Application Support/Claude-3p/configLibrary/_meta.json`
   - Add the deterministic Arca entry when absent.
   - Set `appliedId` to the Arca profile on apply.
   - Preserve every external entry and unknown field.
4. `~/Library/Application Support/Claude-3p/configLibrary/6d757800-0000-4000-8000-000000000001.json`
   - This is the only Arca-owned inference profile.
   - The entry name is `Arca`.

The fixed UUID makes ownership unambiguous across restarts without searching or taking over another tool's profile.

## Arca profile projection

The generated profile contains:

```json
{
  "inferenceProvider": "gateway",
  "inferenceGatewayBaseUrl": "<Provider Anthropic base URL>",
  "inferenceGatewayApiKey": "<credential exported from Keychain>",
  "inferenceGatewayAuthScheme": "bearer",
  "modelDiscoveryEnabled": false,
  "inferenceModels": [
    {
      "name": "<Agent-native model route>",
      "labelOverride": "<Arca Model name>"
    }
  ],
  "coworkEgressAllowedHosts": ["*"],
  "disableDeploymentModeChooser": true
}
```

For non-Claude routes it additionally contains:

```json
{
  "unstableDisableModelVerification": true
}
```

The effective base URL uses the selected Provider's Anthropic Messages connection. Arca computes the Profile's full request URL and accepts it only when the terminal endpoint is `/v1/messages`; `inferenceGatewayBaseUrl` is the URL with that terminal suffix removed. For example, `https://max-ai.amap.com` plus `/v1/messages` becomes `https://max-ai.amap.com`, while `https://api.z.ai/api/anthropic` plus `/v1/messages` becomes `https://api.z.ai/api/anthropic`. Any other endpoint shape fails closed instead of producing a plausible but incorrect configuration.

## Credential handling

- The central Provider credential remains authoritative in macOS Keychain.
- Planning never reads or serializes the credential.
- Commit reads the credential only after the user confirms the operation.
- The operation review explicitly states that the credential will be exported to Claude Desktop's private configuration.
- The Arca profile is created with mode `0600`; an existing Arca profile with broader permissions is tightened before success is reported.
- Credential values never enter operation DTOs, logs, errors, screenshots, test snapshots, or repository fixtures.
- Secret-bearing in-memory carriers use zeroizing storage and redacted debug/output representations. They are cleared after commit or rollback.
- The Arca-owned secret-bearing Profile is not copied into Arca's filesystem backup directory. The outer transaction stores its pre-state in an operation-and-path-bound Keychain item for crash recovery; this is durable recovery evidence, not a memory-only rollback promise.

## Apply transaction

1. Planning reads and hashes all four physical targets, validates their JSON, confirms the Profile protocol, and records the exact write set without reading the credential.
   The four configured roles must be distinct. Commit rechecks canonical destinations and exact inode identities so an ordinary target cannot hardlink or alias the private Profile into filesystem backups.
2. Commit rechecks the candidate hash and target hashes, then reads the credential from Keychain.
3. Existing listened-to Claude configuration and `_meta.json` files are updated in place while preserving inode, permissions, unknown fields, and unrelated formatting where the existing safe-write layer supports it.
   These ordinary JSON/JSONC files are edited through CST-owned fields with recursive duplicate-key rejection; Arca never accepts last-key-wins ambiguity.
4. A new Arca profile is published atomically in the same directory with mode `0600`; an existing Arca profile is replaced only after CAS revalidation.
5. The transaction records the previously applied non-Arca profile ID in typed Arca operational state, then switches `_meta.json.appliedId` to the Arca profile.
6. Any failure rolls every changed target back. Cross-process changes fail closed rather than being overwritten.

Central desired-state persistence and Claude projection remain separate phases under the existing `Agent × capability × physical target` operation model. The direct adapter writes Claude's files itself; it does not introduce a local gateway, LaunchAgent, or background proxy.

### Private transaction evidence and recovery

Before the private pre-state is written to Keychain, the outer transaction writes a nonsecret preparation ledger. That ledger binds the operation to the configured Claude Desktop fourth target (the private Arca Profile path) and to its Keychain snapshot subject. The filesystem rollback manifest, mutation intent, and write evidence for that private target contain only path, mode, identity, version, content hash, and Keychain reference fields. They never contain the Profile bytes. Ordinary, nonsecret Claude configuration backups may retain their prior file content under the normal backup policy.

The private pre-state and its metadata are cleaned up as one fail-closed lifecycle:

| Situation | Required handling |
|---|---|
| Pre-state persistence fails | Remove the partial Keychain item and preparation ledger. If cleanup fails, retain evidence and require recovery. |
| User cancels before a rollback manifest exists | Clear the ledger-bound private Keychain item and operation artifacts; do not touch Claude files. |
| Commit succeeds | After the commit boundary is recorded, clear the private pre-state and ledger, then retire the operation journal. |
| Commit fails and rollback is owned | Restore only with matching hash/mode/identity evidence, then clear private evidence. A mismatch or cleanup failure retains evidence and fails closed. |
| Startup with no rollback manifest | Use the nonsecret ledger to clear its exact private Keychain item and operation root; never hydrate or guess Profile bytes. |
| Resolved target incident | Authorize cleanup only after the incident is resolved, clear private Keychain evidence before retiring the journal, and retain evidence on any failure. |

Any incomplete cleanup returns a recovery-required result and leaves the evidence needed for a later, exact recovery. It never broad-deletes a path or silently treats missing evidence as success.

## Clear and restore

- Clearing the Arca Model relationship never deletes CC Switch, Default, OpenRouter, or any other external Claude profile.
- If the Arca profile is active, clear restores the previously applied profile when that entry still exists.
- If the remembered profile was removed externally, clear chooses the existing entry named `Default`; if no safe restore target exists, planning fails with a repairable conflict.
- After `_meta.json` points away from Arca, the Arca entry and Arca-owned profile file are removed.
- Deployment mode remains `3p` because external profiles may depend on it.
- Clearing central Models or Providers remains outside this Agent-scoped operation.

## Observation and drift

- Synced means the Arca profile matches the active desired Profile and `_meta.json.appliedId` selects it.
- Missing means an assigned Arca Profile or meta entry is absent.
- Drifted means Arca-owned fields differ while the files remain parseable.
- Conflicted means JSON is invalid, the deterministic UUID belongs to an unexpected entry, the active selection is ambiguous, or a target changed during planning.
- A symlinked, non-regular, or non-`0600` private Arca Profile is conflicted even when its JSON bytes otherwise match.
- External Claude profiles are preserved and are not silently adopted into the central Model library.
- The exact non-Claude route classifier is evaluated only while projecting the configured Claude Desktop fourth target; it is not a global model-name rule and does not affect other Agents or external Profiles.

## UI surface

No Claude-specific page or dialog is added. The existing Agent Model workspace consumes the new core capability:

- Agent name: `Claude Desktop`
- Storage authority: `arca-mapping`
- Supports multiple Models: `false`
- Credential mode: `keychain-export`
- Supported protocols: `anthropic-messages`

The normal operation review gains a Claude Desktop credential-export warning. Existing add, switch, clear, success, error, and restart messaging remain shared.

Arca Desktop is React; no Vue component changes are involved.

## Alternatives rejected

### Only patch the currently active external profile

Rejected because it would make Arca overwrite CC Switch or another tool's ownership and could not reliably restore prior state.

### Local Arca gateway

Rejected for this release because the direct configuration already works, while a durable proxy would require a background service, request streaming, lifecycle management, and protocol routing.

### Write every Arca Model as a separate Claude profile

Rejected because Arca's current mapping contract needs only one active Profile. One deterministic entry is simpler to observe, update, clear, and restore safely.

## Verification contract

- Claude Desktop appears as a managed Model Agent only on macOS and reports the four exact targets.
- A failing test first proves `qwen3.7-max` needs `unstableDisableModelVerification`, while `anthropic/claude-sonnet-4-6` does not receive it.
- Apply preserves unrelated JSON and external config-library entries, writes the explicit model, selects the Arca profile, exports the Keychain credential only at commit, and enforces `0600`.
- Planning and serialized operation results contain no credential.
- Clear restores the previous external profile and removes only Arca-owned state.
- Invalid JSON, unsupported protocols, missing credentials, UUID collisions, and CAS changes fail closed.
- Focused Rust and React regression tests are retained, including the `private_transaction` tests for configured-path classification, Keychain-backed hash-only pre-state recovery, external-edit refusal, missing evidence, no-manifest startup/cancel cleanup, persistence failure, and resolved-incident cleanup. Per the repository's fast-delivery policy, broader test suites run only when explicitly requested; production Stable build and release verification remain mandatory.
