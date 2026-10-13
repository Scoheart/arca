# Models and Providers

A Provider defines an API connection. A Model references it with a protocol and Model ID. One Provider can serve several Models, and several compatible Agents can consume a central Model.

## Add a connection and Model

1. In **Models → Add Provider**, choose a template or a custom connection.
2. Set a name, one Base URL, and the credential source. Unauthenticated local services can leave credentials empty.
3. Edit the **Provider portal URL** when needed; the adjacent button opens its key page, console, or setup guide. Use the correct region and Coding Plan portal.
4. Optionally set the **Models list URL**, enable protocols, and edit **Endpoint Paths**. These fields are visible directly and update the full request URL preview.
5. Save the Provider, then add a Model with the Provider, protocol, and exact Model ID. Set token limits according to the actual model.
6. Select the central Model in an Agent workspace. Restart the client or create a new conversation when required.


Protocols share one Base URL but can have different relative paths. Separate origins or accounts belong in separate Providers. For example, `https://gateway.example.com/v1` plus `/responses` resolves to `https://gateway.example.com/v1/responses`.

Arca refuses to disable a protocol still referenced by a Model. It also refuses a relationship when the client cannot represent a custom request path losslessly.

## Protocols

| Protocol | Common default path |
|---|---|
| Anthropic Messages | `/v1/messages`, depending on the Provider template |
| OpenAI Responses | `/responses` |
| OpenAI Chat Completions | `/chat/completions` |
| Gemini GenerateContent | `/models/{model}:generateContent` |

Protocol compatibility does not guarantee that every model runs. Exact Model IDs, capabilities, account access, and client requirements still matter.

## Agent support and current Models

This table comes from the released Core capability output. Agent names and identities come from the audited registry; CLI and desktop forms remain separate.

<AgentReference lang="en" models-only />

**Assigned, enabled, and current** are separate states. Native multi-model clients can retain several Models; single-model clients accept at most one. Disabling or removing a current Model reviews the affected selection.

- **Codex CLI and its desktop entry** share user configuration; the `codex` Model writer manages that configuration.
- **OpenCode CLI and Desktop** share native configuration and both expose managed Models.
- **Qoder IDE, Desktop, and CLI** have distinct identities. IDE Models use official guidance; Desktop 0.1.8+ and CLI 1.1.50+ support managed custom endpoints.
- **Qoder Desktop** selects Models per conversation; **Qoder CLI** supports global selection. Their settings file is shared, with separate Arca Provider identities. Other consumers in a shared store prevent clearing the entire registry.

## Credentials

Central key material lives in the system Keychain. Native Agent capabilities determine delivery: retrieval commands, environment references, or explicitly reviewed export to a client's private configuration. Claude desktop, ZCode, and supported Qoder native delivery have individual constraints; the UI shows available options.

When an environment reference is used, that variable must be available to the Agent's launch environment. A key saved in Arca Keychain does not automatically become an environment variable for every client. Keep real keys out of command arguments, example files, and shared screenshots.

Keychain read failures stop the affected operation instead of being treated as missing credentials. Provider edits include affected consumers; unrelated MCP and Skill operations remain available.

## External configuration and deletion

External Models stay observations until explicitly adopted or imported. Assign separately when needed; ordinary assignment does not overwrite drift.

Deleting a central Model reviews its consumers and preserves other Models and Providers. Provider deletion is separate and must account for referencing Models. An Agent's clear-all action affects its own Model scope, retaining central assets and credentials; Core rejects shared-store conflicts.

[Provider templates](/en/guide/providers) · [CLI authoring and review](/en/guide/cli#model)
