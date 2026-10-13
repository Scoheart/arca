# MiniMax Code Support Design

## Goal

Add MiniMax Code as a first-class Arca Agent without guessing configuration paths or weakening Arca's credential boundary.

## Evidence

- The signed and notarized MiniMax Code `3.0.51` macOS bundle uses bundle ID `com.minimax.agent`.
- Its packaged `@mavis/config` source resolves the user data directory to `~/.mavis`, the main configuration to `~/.mavis/config.yaml`, and the MCP configuration to `~/.mavis/mcp.json`.
- Its packaged MCP runtime reads the `mcpServers` map and accepts stdio, HTTP, Streamable HTTP, and SSE entries.
- Its custom model provider flow stores `options.apiKey` as a literal YAML value. It does not expose a documented command or environment indirection that can consume a Arca Keychain credential safely.
- The public `MiniMax-AI/minimax-code` repository currently collects issue reports; it is not the product source repository. The official M3 article says open-sourcing is planned for the future.

## Design

### MCP management

Add a writable `minimax-code` Agent definition with:

- main config: `~/.mavis/config.yaml`
- MCP config: `~/.mavis/mcp.json`
- MCP key: `mcpServers`
- JSON map layout
- explicit transport codec
- stdio plus URL-based HTTP, Streamable HTTP, and SSE entries

Arca continues to own only the `mcpServers` subtree. Existing root fields and unknown server fields must survive apply, disable, remove, and rewrite operations.

### Models

Add MiniMax Code to Models as a guided target. Show `~/.mavis/config.yaml`, all three profile protocols, and the official product page. Do not assign a Arca profile automatically because that would require writing the Keychain secret into plaintext YAML.

### Discovery and presentation

Installed detection covers the `mavis` command, `~/.mavis`, and `/Applications/MiniMax Code.app`. Use the official application icon extracted from the signed vendor bundle and record its source. The Agent page will naturally show distinct main and MCP paths through the existing configuration-center UI.

## Verification

- A codec fixture proves stdio, HTTP, SSE, disabled state, root metadata, and unknown entry fields are preserved.
- Model tests prove MiniMax Code is guided, exposes the expected path and protocols, and refuses automated assignment.
- The icon completeness gate includes the new writable Agent.
- Core tests, desktop build, website build, and formatting checks remain green.
