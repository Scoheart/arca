# Models and Providers

A Provider defines an API connection. A Model references it with a protocol and Model ID. One Provider can serve several Models, and several compatible Agents can consume a central Model.

## Add a connection and Model

1. In **Models → Add Provider**, choose a template or a custom connection.
2. Set a name and Base URL. For authenticated endpoints, choose a credential store and enter the API Key.
3. Edit the **Provider portal URL** when needed; the adjacent button opens its key page, console, or setup guide. Use the correct region and Coding Plan portal.
4. Optionally set the **Models list URL**, enable protocols, and edit **Endpoint Paths**. These fields are visible directly and update the full request URL preview.
5. Save the Provider, then add a Model with the Provider, protocol, and exact Model ID. Set token limits according to the actual model.
6. Select the central Model in an Agent workspace. Restart the client or create a new conversation when required.


Protocols share one Base URL but can have different relative paths. Separate origins or accounts belong in separate Providers. For example, `https://gateway.example.com/v1` plus `/responses` resolves to `https://gateway.example.com/v1/responses`.

Arca refuses to disable a protocol still referenced by a Model. It also refuses a relationship when the client cannot represent a custom request path losslessly.

## Model playground

Open **Models → Model playground** in Arca Desktop or run `arca model playground`. Choose a saved Model Profile and its parameters for each request in **Run settings**, then enter one shared prompt at the bottom. You can run up to six requests in parallel. Mix Providers and protocols, or duplicate a request to compare the same model with different settings.

Blank parameters use the service's defaults. Use **Advanced JSON** for protocol-specific parameters such as Anthropic's `thinking` or Gemini Interactions' `generation_config`. Each result shows its own reply, latency, provider-reported token usage, or error. Edits apply to the request only. Press Ctrl+C to stop a server started from the terminal.

- **Response mode** streams text as it arrives and measures the time to the first text. Switch to a complete reply for gateways or models without streaming. Each request can be stopped locally or retried on its own.
- **Request preview** shows the endpoint, authentication header name, and protocol JSON before sending. A preview does not read credentials.
- **Run history** keeps the parameters and results of recent rounds in the current browser tab. Restore a round's settings to run it again, or download the round as JSON.

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

## Credential stores

Open **Models → Credential stores** to manage connections shared by Providers. macOS Keychain is built in. To use 1Password, install the official `op` CLI and grant a service account read and write access to the chosen vault. [1Password service account setup](https://www.1password.dev/service-accounts/get-started)

Select **Add connection → 1Password** and enter the connection name, absolute path to `op`, Vault ID, and token environment variable name. The default is `OP_SERVICE_ACCOUNT_TOKEN`. Supply its value in the environment of each process that reads the vault, such as Arca Desktop, the CLI, or the Agent's credential helper. After saving, use **Check connection** to verify access to the vault. You can rename a connection and remove it once all Providers that reference it have been updated. [Using service accounts with 1Password CLI](https://www.1password.dev/service-accounts/use-with-1password-cli)

### Bitwarden and Vaultwarden

Choose **Add connection → Bitwarden / Vaultwarden** to use an existing personal folder. Create an Arca folder in the vault, then configure a dedicated data directory for the official `bw` CLI and sign in to the chosen server. Unlock that profile and look up the account and folder IDs.

```bash
export BITWARDENCLI_APPDATA_DIR="$HOME/.config/arca-bitwarden-cli"
mkdir -p -m 700 "$BITWARDENCLI_APPDATA_DIR"
bw config server https://vault.example.com
bw login
export BW_SESSION="$(bw unlock --raw)"
bw status | jq -r '.userId'
bw get folder Arca | jq -r '.id'
```

Register the absolute CLI path, server URL, profile directory, account ID, and folder ID. Supply the unlocked session through the environment variable configured in Arca, which defaults to `BW_SESSION`. Use **Check connection** after saving. Arca stores each revision in a hidden field of a secure note and moves retired revisions to the trash. [Bitwarden CLI documentation](https://bitwarden.com/help/cli/)

### NordPass

Sign in and unlock the official `np` CLI, then create a personal folder for Arca. Use `np whoami --format json` and `np folder list --type personal --format json` to confirm the active account and folder ID. Add a `nordpass` connection with the absolute path to `np`, your sign-in email, and the personal folder ID. The CLI is available on the personal Free plan. [NordPass CLI guide](https://support.nordpass.com/hc/en-us/articles/51095299365137-How-to-use-NordPass-CLI) · [Plan availability](https://nordpass.com/blog/command-line-interface-feature/)

Arca titles the password item with the Provider name and saves the key in the password field.

### Proton Pass

Install official `pass-cli` 2.6.0 or later. Sign in with system keyring storage and a dedicated session directory with mode `0700`. Use the same environment to look up the account ID and the share ID of a writable vault. Proton Pass CLI requires a plan that includes CLI access. [Proton Pass CLI configuration](https://protonpass.github.io/pass-cli/get-started/configuration/) · [Plan availability](https://proton.me/pass/developer-features)

```bash
proton_session_dir="$HOME/.config/arca-proton-pass"
mkdir -p -m 700 "$proton_session_dir"
export PROTON_PASS_SESSION_DIR="$proton_session_dir"
export PROTON_PASS_KEY_PROVIDER=keyring
pass-cli login
pass-cli info --output json
pass-cli share list --only-vaults true --output json
```

Add a `proton-pass` connection with the absolute path to `pass-cli`, the session directory, the account ID, and the vault share ID. Arca saves the key in a hidden field on a secure note. The title includes the Provider name and a revision marker so Arca can find the same note after an interrupted save.

### Keeper Secrets Manager

Install the official `ksm` CLI. Use a Keeper Business account with Secrets Manager enabled, give the application edit access to a dedicated shared folder, and enroll a client. Add a `keeper` connection with the absolute path to `ksm`, the shared folder UID, the client ID, and the configuration environment variable name. Supply the registered client's Base64 configuration in the launch environment, using `KSM_CONFIG` by default. Arca titles the record with the Provider name and saves the key in the password field. [Keeper requirements](https://docs.keeper.io/keeperpam/secrets-manager/quick-start-guide) · [Keeper Secrets Manager CLI](https://docs.keeper.io/keeperpam/secrets-manager/secrets-manager-command-line-interface)

### KeePassXC

Choose **Add connection → KeePassXC**. Enter the absolute path to the official `keepassxc-cli` and an existing KDBX database of up to 64 MiB. Enter the key file path when the database uses one. Supply the database password in the environment of Arca or its credential helper. The default variable is `ARCA_KEEPASSXC_PASSWORD`.

Arca writes a separate entry for each credential revision, checks for concurrent database changes, and saves an encrypted KDBX rollback copy. Retirement removes the active entry. Databases with the recycle bin enabled can recover it from there. [KeePassXC CLI manual](https://github.com/keepassxreboot/keepassxc/blob/develop/docs/man/keepassxc-cli.1.adoc)

### Passbolt

Choose **Add connection → Passbolt**. Enter the HTTPS server URL, account ID, account OpenPGP private key file, server OpenPGP fingerprint, and optional personal folder ID. The default passphrase variable is `ARCA_PASSBOLT_PASSPHRASE`. Use an owner-only regular key file, such as mode `0600` or `0400`.

Build Arca's helper from the Arca checkout and enter its absolute path. See the helper's `go.mod` for its Go version requirement.

```bash
mkdir -p "$HOME/.local/bin"
cd integrations/passbolt-helper
go build -mod=readonly -trimpath -o "$HOME/.local/bin/arca-passbolt" .
```

Arca creates private credential resources for the account. API keys can be up to 4,096 characters. Rotation creates a new resource. Retirement verifies that Arca owns the resource. [Official Passbolt SDK](https://github.com/passbolt/go-passbolt)

### OpenBao

Choose **Add connection → OpenBao**. Enter the HTTPS server origin, an existing KV v2 mount, and a prefix reserved for Arca. Supply the token in the environment of Arca or its credential helper. The default variable is `BAO_TOKEN`. Use **Check connection** after saving.

This policy uses the `secret` mount and the `arca/providers` prefix. Substitute the mount and prefix entered in Arca.

```hcl
path "secret/config" {
  capabilities = ["read"]
}
path "secret/data/arca/providers/*" {
  capabilities = ["create", "read"]
}
path "secret/delete/arca/providers/*" {
  capabilities = ["update"]
}
path "sys/capabilities-self" {
  capabilities = ["update"]
}
```

Arca creates a separate record using the credential revision ID. Retirement soft-deletes the corresponding version. [OpenBao KV v2](https://openbao.org/docs/secrets/kv/kv-v2/)

### Choose a store for a Provider

Choose a store and enter the API Key in the Provider editor. Leave the key field blank to keep the existing credential. To switch stores, enter a new key. Providers that allow unauthenticated access can explicitly remove a saved key.

This update starts a fresh Model library in `~/.arca/assets/models/registry.json`. The previous central Model catalog and Keychain items remain available for manual recovery. Recreate the Providers and Models you need, then review Agent assignments. Existing Agent configuration is still observed and supported candidates can be adopted explicitly.

## Agent credential delivery

Available delivery options depend on each Agent's native capabilities. Agents that support a credential command can resolve an enabled Model's credential through the Arca CLI. An environment reference records the variable name. With a supported CLI Agent launched through Arca, the launcher reads the selected store and sets the variable for that process. When launching the Agent yourself, supply it in that launch environment.

For Agents that support private configuration delivery, Core writes to verified targets with protected rollback. The explicit plaintext option also requires review, with `0600` permissions and checks against concurrent file changes. Credential read failures stop the affected operation. After a Provider edit, Core converges affected Agent targets independently.

## External configuration and deletion

External Models stay observations until explicitly adopted or imported. Assign separately when needed; ordinary assignment does not overwrite drift.

Deleting a central Model reviews its consumers and preserves other Models and Providers. Provider deletion is separate and must account for referencing Models. An Agent's clear-all action affects its own Model scope, retaining central assets and credentials; Core rejects shared-store conflicts.

[Provider templates](/en/guide/providers) · [CLI authoring and review](/en/guide/cli#model)
