# API Key Sources and Agent Delivery Design

## Summary

Arca will model API-key handling as two separate decisions:

1. **Where Arca resolves the credential**: Arca secure storage, environment variable, an existing private file, or a structured helper command.
2. **How Arca delivers the resolved credential to an Agent**: the safest supported automatic route, a verified Agent-owned credential store, or an explicitly approved private plaintext target.

This split gives Arca six concise user-facing ways without pretending that every Agent supports every way natively. Provider records own the default source. Each Agent consumption may override the source and chooses a delivery. No secret value is stored in the catalog, consumption record, plan, log, ordinary backup, or repository.

## Goals

- Implement all six user-facing API-key ways end to end: `arca-store`, `env`, `file`, `helper`, `agent-store`, and `plaintext`.
- Fix the current OpenCode failure in which a model can be added without any usable API key.
- Keep Provider credentials reusable while allowing per-Agent delivery differences.
- Use verified native Agent mechanisms when they exist and fail closed when they do not.
- Preserve existing Agent configuration fields and make credential writes atomic, private, and reversible.
- Migrate existing `env_key` and Provider Keychain configurations without exposing secrets.

## Non-goals

- Inventing undocumented credential-store formats for Agents.
- Creating or editing user-managed API-key files.
- Executing shell snippets, pipelines, redirection, command substitution, or `sh -c` helpers.
- Silently falling back to plaintext.
- Putting secrets in Arca catalog JSON, plan JSON, telemetry, error messages, backups, screenshots, fixtures, or Git.
- Making every Agent expose all six UI choices.

## Terminology and Data Model

The UI presents six concise labels:

| Identifier | Chinese label | Meaning |
|---|---|---|
| `arca-store` | Arca 安全存储 | Secret bytes live in the operating-system Keychain. |
| `env` | 环境变量 | Arca stores only an environment-variable name. |
| `file` | 密钥文件 | Arca stores only a path to an existing private regular file. |
| `helper` | 凭据助手 | Arca stores a structured executable and argument list. |
| `agent-store` | Agent 凭据存储 | Arca writes to a verified Agent-owned private credential store. |
| `plaintext` | 明文配置 | Arca writes to a verified private Agent configuration target after explicit danger confirmation. |

Internally, source and delivery remain separate:

```ts
type ApiKeySource =
  | { kind: "arca-store" }
  | { kind: "env"; name: string }
  | { kind: "file"; path: string }
  | { kind: "helper"; command: string; args: string[]; ttl_ms?: number };

type ApiKeyDelivery =
  | { kind: "auto" }
  | { kind: "agent-store" }
  | { kind: "plaintext" };
```

Provider persistence becomes:

```json
{
  "auth_requirement": "required",
  "api_key_source": {
    "kind": "env",
    "name": "MAX_AI_API_KEY"
  }
}
```

`auth_requirement` is `required`, `optional`, or `none`:

- Official remote Providers default to `required`.
- Known local Providers such as Ollama, LM Studio, and local vLLM default to `none`.
- Custom Providers default to `optional` and remain user-editable.

An Agent consumption stores policy only:

```json
{
  "profile_id": "profile-id",
  "enabled": true,
  "credential": {
    "source_override": null,
    "delivery": "agent-store"
  }
}
```

No secret value appears in either record.

## Source Resolution

### Arca secure storage

The Provider record contains `{ "kind": "arca-store" }`. Secret bytes remain in the existing system Keychain entry for that Provider. Planning and preview may report whether a credential exists but never return its value. Commit resolves the Keychain value again and compares a cryptographic digest with the planned digest.

### Environment variable

The Provider record contains `{ "kind": "env", "name": "MAX_AI_API_KEY" }`. Names must match a portable environment-variable identifier. The value is resolved only when a delivery requires materialization. Agents with a native environment reference receive the variable name rather than its current value.

### Key file

The Provider record contains `{ "kind": "file", "path": "/absolute/path" }`. Arca does not create or modify the file. Resolution rejects relative paths, symlinks, non-regular files, group/world permissions, empty content, and multiline content. The required mode is `0600`. Planning records non-secret identity evidence such as canonical path, file identity, mode, size, modification identity, and a secret digest; commit reopens the file without following symlinks and verifies the evidence and digest.

### Credential helper

The Provider record contains a structured command:

```json
{
  "kind": "helper",
  "command": "/usr/local/bin/read-maxai-key",
  "args": ["--profile", "coding"],
  "ttl_ms": 300000
}
```

`command` names one executable; it is not a shell program string. Arca passes `args` directly to the process API. It never invokes a shell. The command must be absolute or a specifically trusted system executable, must exit successfully within the bounded timeout, and must emit exactly one non-empty line on stdout. Stderr may be reduced to a non-secret diagnostic category but is never copied verbatim when it may contain credential material. Resolved bytes are zeroized after use. `ttl_ms` is bounded and optional; it controls only an in-memory secret cache.

## Delivery Selection

`auto` selects the safest compatible route in this order:

1. Native environment, file, or structured helper reference supported by the Agent.
2. A Arca Keychain helper that lets the Agent resolve the Provider Keychain entry without writing secret bytes.
3. A verified Agent-owned credential store.
4. `credential_delivery_unsupported`.

`auto` never chooses plaintext.

`agent-store` is enabled only for a verified, stable, writable store. Initial verified implementations are:

- OpenCode: `~/.local/share/opencode/auth.json`.
- Claude Desktop: its existing private Profile configuration mechanism.

`plaintext` is a per-Agent explicit override. The first use shows one compact global danger dialog: “将把 Max Ai API Key 明文写入 OpenCode 私有配置”, followed by the exact target path and “权限将设为 0600”. The confirmation applies to the candidate operation only. The target must be a verified private file whose permissions can be enforced to `0600`; otherwise the operation fails.

## Initial Agent Capability Matrix

The matrix is declarative and is the only source used by core and UI availability checks.

| Agent | Native env | Native file | Native helper | Verified agent store | Explicit plaintext |
|---|---:|---:|---:|---:|---:|
| Claude Code | yes | no | yes | no | no |
| Claude Desktop | no | no | no | yes | no |
| Codex | no | no | yes | no | no |
| Pi Coding Agent | no | no | yes | no | no |
| OpenCode | yes | yes | no | yes | yes |
| Kilo Code | yes | yes | no | no | no |
| Qwen Code | yes | no | no | no | no |
| Grok Build | yes | no | no | no | no |
| Crush | yes | no | no | no | no |
| Mistral Vibe | yes | no | no | no | no |
| Hermes | yes | no | no | no | no |
| Factory Droid | yes | no | no | no | no |
| Goose | yes | no | no | no | no |
| Qoder | guided | guided | guided | no | no |
| MiniMax Agent | guided | guided | guided | no | no |

Unsupported choices remain disabled with a precise reason. Arca does not infer support from similar products.
The first plaintext adapter is intentionally OpenCode-only: its private JSON target, precedence, rollback, and `0600` behavior are verified end to end. Additional plaintext adapters require the same evidence before their capability flag can be enabled.

## OpenCode Agent Store

For `agent-store`, Arca maps the Provider to the OpenCode authentication key expected by `~/.local/share/opencode/auth.json`, merges only the owned Provider entry, preserves unrelated providers and unknown fields, writes through the private transaction layer, and enforces `0600`. The model projection and auth-store projection are one logical Agent target: both pass compare-and-swap checks and either both commit or both roll back.

When a Provider requires authentication and neither its default source nor the consumption override resolves, planning fails with `credential_missing`. Arca must not add a usable-looking OpenCode model without a corresponding credential path.

## Private Transactions and Stale Plans

Planning records:

- source kind and non-secret metadata;
- Agent delivery and resolved target paths;
- file identity and permission evidence when applicable;
- secret digest only;
- candidate hashes for every target file.

Commit re-resolves the source, compares the secret digest, rechecks target candidates, and aborts on any source or filesystem change. For `agent-store` and `plaintext`, filesystem backups contain only the pre-existing target state. Any rollback value that includes newly supplied secret bytes remains transient or in Keychain, never in an ordinary backup. Secret buffers use zeroizing containers and are dropped immediately after the atomic write.

The logical transaction is:

1. Validate and resolve the credential source.
2. Prepare Agent model-config and credential-target candidates in memory.
3. Verify permissions, hashes, and ownership boundaries.
4. Atomically replace all files for that one Agent physical target.
5. Persist the Arca consumption record.
6. On failure, restore the complete pre-operation Agent state and leave the Arca consumption unchanged.

Unknown and external fields are preserved throughout.

## Migration

Catalog loading migrates legacy Provider records in memory and persists the upgraded representation on the next owned write:

- Existing `env_key` becomes `{ "kind": "env", "name": env_key }`.
- A Provider with an existing Arca Keychain entry becomes `{ "kind": "arca-store" }`.
- A known local Provider with neither becomes `auth_requirement: "none"`.
- A known remote Provider with neither becomes `auth_requirement: "required"` with a missing credential state.
- A custom Provider with neither becomes `auth_requirement: "optional"`.

Legacy data remains readable during the migration window, but all new writes use the new fields. Secrets are never copied during migration.

## UI

### Provider form

The Provider dialog uses a dropdown plus adaptive form:

1. Authentication requirement: required, optional, or none.
2. Credential source: Arca secure storage, environment variable, key file, or credential helper.
3. Source-specific fields:
   - Arca secure storage: masked key input, replace, and clear actions.
   - Environment variable: variable-name input.
   - Key file: path input and permission/identity validation status.
   - Credential helper: command, repeatable arguments, TTL, and a non-secret “test” result.

### Agent assignment

The Agent consumption editor exposes `Auto (recommended)`, `Agent credential store`, and `Plaintext configuration`. It shows the resolved route, for example: “Provider Arca secure storage → OpenCode Agent credential store → auth.json”. Unsupported routes are disabled with the core-provided reason. Source override is optional and uses the same adaptive source fields.

The plaintext confirmation is a small application-level modal overlay, not an inline Models card or a large technical-plan panel.

## Error Contract

Core returns stable machine-readable codes and a concise safe message:

- `credential_missing`
- `credential_source_changed`
- `credential_file_insecure`
- `credential_helper_failed`
- `credential_helper_invalid_output`
- `credential_delivery_unsupported`
- `agent_store_conflicted`
- `plaintext_target_insecure`

Messages may include non-secret source metadata and target paths, but never resolved credential bytes or raw helper output.

## Validation and Acceptance

Focused tests must prove:

- Serialization and legacy migration for all source/delivery variants.
- Environment-name validation.
- File rejection for symlinks, non-regular files, wrong modes, empty content, and multiline content.
- Helper execution without a shell, timeout/failure handling, and empty/multiline-output rejection.
- Secret digests rather than values in plan serialization and errors.
- Stale source and stale target rejection between plan and commit.
- Capability-matrix selection and the `auto` rule that never chooses plaintext.
- OpenCode `auth.json` merge, unknown-field preservation, `0600`, conflict rejection, and rollback with model config.
- Provider-required credential failure before model assignment.
- Plaintext first-use confirmation and insecure-target rejection.
- Provider and Agent adaptive UI forms, disabled reasons, resolved-route preview, and compact global danger modal.
- Existing Provider/model configurations continue to round-trip without unrelated changes.

The implementation is accepted when all focused tests pass, production Rust and desktop builds pass, the diff contains no credential material, and the remote-only PR is merged through the existing Arca Direct Stable release path. The local installed Arca application is not updated for this task.
