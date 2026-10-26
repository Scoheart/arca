# Kilo Desktop

Arca provides a separate `kilo-desktop` Agent for the official desktop application, displayed as **Kilo**. It is separate from Kilo Code CLI (`kilo-code`) and the VS Code extension (`kilo-vscode`).

## Verified identity and configuration

The official macOS arm64 ZIP served from the [Kilo installer page](https://kilo.ai/install) was inspected on 2026-10-06/07 without installing or executing the application. Version 0.1.11 declares:

- Bundle: `Kilo.app`, display name `Kilo`, identifier `ai.kilo.desktop`.
- Launcher locations: `/Applications/Kilo.app` and `~/Applications/Kilo.app`.
- Desktop user configuration: `~/Library/Application Support/Kilo Desktop/plugins/kilo-server/runtime/config/kilo/kilo.jsonc`.
- Native MCP entries: top-level `mcp`, using Kilo's `local`/`remote` format.
- Native custom models: top-level `provider.<id>.models`.

The packaged main process resolves stable Electron user data as `Kilo Desktop`, each feature's storage as `plugins/<feature-id>`, and the `kilo-server` configuration as `runtime/config/kilo`. It passes this directory to its bundled CLI through `KILO_CONFIG_DIR`. This is an additional desktop runtime configuration; the embedded engine also reads the CLI's global configuration. Arca writes the dedicated Desktop file to keep its additions separate from CLI providers and settings.

Evidence: [Desktop overview](https://kilo.ai/docs/desktop/overview), [AI settings](https://kilo.ai/docs/desktop/settings/ai), [official package](https://autoupdate.desktop.anaconda.com/kilo-desktop/latest/latest/macos/arm64/Kilo.zip), and the [pinned engine configuration loader](https://github.com/Kilo-Org/kilocode/blob/82fddfdd608f336a1c3083fe37fdd80933fb5569/packages/opencode/src/config/config.ts). The package's `KiloConfigManager`, `KiloServerManager`, `app-identity`, and `featureStorage` modules establish the desktop-specific path.

## Arca behavior

- **MCPs:** Assign and remove central assets through the existing lossless OpenCode codec. Preserve Desktop-owned entries, permissions, comments, and providers. Restart Kilo after applying changes.
- **Models:** Add multiple custom profiles through the existing native provider adapter. Arca uses a dedicated provider namespace and edits only its profile entries. Select the model inside the Kilo conversation; Arca does not advertise a global current-model switch for this surface.
- **Credentials:** Environment and file references are supported. Arca does not export Keychain keys as literal values into Desktop configuration. A credential stored only in Arca Keychain requires a supported reference source before it can be delivered to this Agent.
- **Skills:** Assign central skill links to `~/.kilo/skills`, shared with Kilo CLI and the extension. The existing physical-target graph reports the affected Kilo consumers. See [official Skills locations](https://kilo.ai/docs/customize/skills).
- **Installation and version:** Use the actual `Kilo.app` launcher and its `Info.plist` version. Having CLI installed does not mean Desktop is installed. The official download action opens the vendor installer page.
- **Safety:** MCP and Model operations use encrypted private snapshots for the whole Desktop config, including path overrides, because existing native providers may contain credentials. Imports create central assets without adopting external provider IDs, changing native files, or assigning the new asset to an Agent.

The official bright-yellow Desktop icon was decoded from its ICNS resource; the older CLI/IDE artwork is not reused. No Kilo account credentials, model files, app preferences, or user configuration are included in the repository.

## Local verification

Frontend production compilation and the Tauri development build are used for this change. The development preview uses the user's existing Arca data. Guard fixtures cover lossless MCP round trips, native-service preservation, isolated model IDs, reference-only credential capabilities, private path overrides, and imports without implicit assignment. Automated suites remain available for an explicitly requested run.

Live Kilo launch and authenticated inference require the official application to be installed and a provider to be configured; inspection of the download does not claim those actions happened.
