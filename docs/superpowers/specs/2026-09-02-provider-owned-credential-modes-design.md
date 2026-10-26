# Provider API Key and Agent-owned delivery

## Decision

Providers own one API Key. Arca stores newly entered keys in the system Keychain. Agents own the delivery decision: core selects the best verified route automatically, and an Agent shows a dedicated selector only when it has more than one reliable route.

## Provider form

- The form exposes one optional or required API Key field, according to the Provider template.
- It exposes no authentication-requirement selector and no generic source-method selector.
- A newly entered key persists as `arca-store`, with bytes only in Keychain.
- Known local Providers remain unauthenticated. Custom Providers may remain keyless.
- Existing `env`, `file`, and `helper` records remain readable and operational. Leaving the API Key blank preserves them; entering a key migrates that Provider to Keychain.

## Agent behavior

- New consumptions default to `auto`; core chooses the safest verified adapter.
- Agents with only one reliable route show no credential control.
- Agents with multiple reliable routes expose only their own concrete options, never one cross-Agent source form.
- OpenCode currently offers automatic Agent credential storage and explicit plaintext configuration.
- Plaintext requires an explicit danger confirmation, writes only the verified target, enforces `0600`, uses atomic compare-and-swap, and keeps rollback payloads out of ordinary files.
- Historical per-consumption source overrides remain deserializable but are ignored. Historical delivery choices remain effective.

## Security and compatibility

- Provider API Key bytes remain out of Provider JSON, consumption JSON, plans, logs, ordinary backups, screenshots, and Git.
- Required Providers without a resolvable source remain blocked before Agent configuration is written.
- Plaintext is never an automatic fallback.
- Existing Provider sources and Agent policy records continue to deserialize.

## Acceptance

- Provider UI contains only the API Key field.
- New keys save to Keychain; existing external sources survive an untouched edit.
- Agent Model rows show no selector unless at least two verified routes exist.
- OpenCode automatic mode uses `auth.json`; explicit plaintext mode uses literal `options.apiKey` in a private `0600` config.
- Provider source wins over every historical Agent source override.
