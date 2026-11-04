# Warp custom model setup

Audited on 2026-11-04 using Warp's official documentation and public client source
at commit `2d5398a4f10aac2efb914b71104e518e83594d16`.

## What Arca exposes

Warp is a **guided** Model target. Its Models tab describes native setup and links
to the official configuration instructions. It supports multiple endpoint models,
but Arca does not offer central Model assignment, removal, current-model switching,
credential delivery or an editable model-config path for this target. MCP and
Skills remain independently configurable.

To configure a model, open Warp Settings and search for `inference endpoint`.
Enter the endpoint Base URL, API key and model ID, save, then select the specific
custom model in Warp's model picker. The current public guide documents OpenAI
Chat Completions. It requires a public HTTPS endpoint because inference requests
pass through Warp's servers. Localhost/private-network URLs are not directly
usable. Warp's Auto models use Warp-provided inference instead of the custom
endpoint.

- [Custom inference endpoints](https://docs.warp.dev/agents/inference/custom-inference-endpoint/)
- [Model choice](https://docs.warp.dev/agents/inference/model-choice/)

## Why this is native setup guidance

The audited source separates non-secret endpoint definitions from credentials:

- `app/src/ai/custom_endpoints.rs` reads and updates typed endpoint definitions
  through `AISettings`, synchronizes the active catalog, and supports migration
  from the legacy secure-storage record.
- `crates/ai/src/api_keys.rs` defines `CustomEndpointDefinitions`, keyed by stable
  endpoint ID. Each definition contains `name`, `base_url`, `schema`, and `models`;
  each model contains `name`, optional `alias`, and a stable `config_key`.
- The audited source's three schema variants are `openai_chat_completions`,
  `openai_responses`, and `anthropic_messages`. This is a source-level observation,
  not evidence that all three have shipped or are available in the installed app.
  Arca's guided capability advertises only the protocol in the current public
  endpoint documentation.
- The new setting is `agents.custom_endpoints`. The GUI's default public settings
  file is `~/.warp/settings.toml`, but `app/src/settings/init.rs` still selects that
  backend through the `SettingsFile` feature flag. When disabled, public settings
  use the platform-native preferences backend. A guessed TOML write could be
  ignored or interfere with native migration.
- API keys are held separately in device secure storage. The current source uses
  a single `AiCustomEndpointKeys` JSON map and retains a legacy `AiApiKeys` record.
  Endpoint definitions have no verified environment, file or helper reference.
- `app/src/lib.rs` deliberately gives the GUI and TUI separate secure-storage
  namespaces: the GUI's data domain versus that domain plus `.tui`. Its comments
  describe the different signing identities and Keychain ACLs. A CLI key setup is
  not proof of credential delivery into the desktop app.
- The native key-map write clones and replaces the collection. The inspected
  interface supplies neither an external cross-process CAS contract nor a
  verified third-party GUI credential writer. Arca therefore does not read or
  overwrite that aggregate, change its ACL, or export central credentials to a
  plaintext workaround.

Pinned source:
[endpoint definitions](https://github.com/warpdotdev/warp/blob/2d5398a4f10aac2efb914b71104e518e83594d16/app/src/ai/custom_endpoints.rs),
[schema and keys](https://github.com/warpdotdev/warp/blob/2d5398a4f10aac2efb914b71104e518e83594d16/crates/ai/src/api_keys.rs),
[public settings](https://github.com/warpdotdev/warp/blob/2d5398a4f10aac2efb914b71104e518e83594d16/app/src/settings/init.rs),
[secure-storage namespaces](https://github.com/warpdotdev/warp/blob/2d5398a4f10aac2efb914b71104e518e83594d16/app/src/lib.rs).

The selected installed Warp app remains `0.2026.08.19.08.15.01`; the November source
has not been bound to that release's loader. Neither the model API-key store nor
user settings, databases or conversations were opened for this audit. No Warp
executable, setup command or inference request was run.

The isolated regression sources cover the guided capability, blocked credential
and config writers, the Models UI without mutation controls, and independent MCP
editing. Under project fast mode these suites remain unrun; compilation and
development-preview results are reported separately.
