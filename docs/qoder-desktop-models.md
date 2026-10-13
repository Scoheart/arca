# Qoder Desktop model storage audit

Audited on 2026-09-06 against the official macOS arm64 Qoder Desktop 0.1.8 distribution. This is the new `com.qoder.app` product, separate from the Qoder IDE.

- Download: https://download.qoder.com/qoder-app/releases/0.1.8/Qoder-Installer-mac-arm64.zip
- Installer SHA-256: `7c08cbac579f4980fb00d276b3ed482116aaa109ab4874e5cbdd1291d4e53c02`
- Distribution commit: `8b672f8c18899e825714bf4710d6adfd27e0ca31`
- Bundled agent SDK: `@ali/qoder-agent-sdk-next@1.0.34`.
- Public announcement: https://docs.qoder.com/release-notes/qoder
- User guide: https://docs.qoder.com/qoder/custom-models

The public guide describes the settings UI without documenting a writable file. Inspecting the signed application resolves that omission: the renderer calls the main BYOK service, which delegates custom-endpoint creation to the bundled agent runtime's BYOK configuration control interface. That interface persists a provider record in the user `settings.json`, under `providers`. Its listing and execution paths read the same record. Desktop passes `~/.qoder` as the runtime configuration directory. It shares this file with Qoder CLI and MCP configuration.

The provider record contains `type: openai-compatible`, `baseUrl`, `apiKey`, `protocol`, `authType`, `displayName`, a provider-local default `model`, and `models[]`. Each model has `model`, `displayName`, optional `contextWindow`, `maxOutputTokens`, and `capabilities`. Protocol values are `openai`, `openai-responses`, and `anthropic`; Anthropic uses `authType: api-key`. The runtime performs recursive `$VAR` / `${VAR}` expansion. A credential containing such literal syntax cannot be exported losslessly and is rejected by Arca; an environment reference remains available.

The Desktop database also contains legacy encrypted BYOK profiles and model capability metadata. Those tables are not the current custom-endpoint credential authority. Arca does not edit them or decrypt Desktop credentials. Native providers do not require a matching database metadata row to appear in the model list.

Desktop model selection belongs to each conversation. Its BYOK selection key refers to the provider/model record; the CLI's `model.name` does not select the Desktop conversation model. Arca therefore installs/removes models without manufacturing a global current state. Restart Desktop to rebuild the cached provider list, then select the model in a conversation. BYOK account/plan access checks remain Qoder's responsibility.

Arca uses one stable provider identity per managed profile, lossless JSONC edits, existing-file inode preservation, CAS, and private transactional snapshots. Environment delivery stores only a reference; the existing explicit plaintext delivery policy stores the native `apiKey` with `0600` permissions. Central credentials remain in Keychain. Because MCP shares the file, its reviewed mutations also use encrypted rollback snapshots and suppress ordinary whole-file plaintext backups. Unknown fields and unrelated providers remain intact. Multi-model external provider containers remain read-only to avoid taking over a sibling model's connection or credential.

The storage contract was established through package inspection. No real Qoder account, user credential, or live paid inference was used in this audit. Future releases that change this private schema require another compatibility review.
