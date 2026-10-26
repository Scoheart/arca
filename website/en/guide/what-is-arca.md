# What is Arca

Arca manages Agent resources and configuration. Keep **Models, MCPs, and Skills** in central libraries, then choose which CLI, desktop app, or IDE uses them. The desktop app and native `arca` CLI share `~/.arca/` and the same management core.


## Start with one asset

| Resource | Create it in | Give it to an Agent |
|---|---|---|
| Model connections and Models | Models: add a Provider, then a Model | Select a compatible Model in the Agent workspace |
| Tool services | MCPs: add, paste, subscribe, or import | Select a central entry in the Agent's MCPs tab |
| Reusable instructions | Skills: download or import from GitHub, a folder, or an archive | Select the central copy in the Agent's Skills tab |

Creating an asset and assigning it are separate steps. Several compatible Agents can use one asset; Arca adapts their formats, files, and shared directories.

## Everyday workflow

1. Choose Models, MCPs, or Skills in the top bar and organize your central assets.
2. Open the Agent picker and select your client.
3. Add assets in its capability tabs. Enable, disable, or select a current Model when supported.
4. When external changes appear, inspect them and choose to adopt, restore, or detach the exact relationship.

Ordinary actions execute directly and report the result. Deletions, local overwrites, and high-risk Skills use one focused review when needed.

## Desktop and automation

Desktop includes resource cards, source navigation, Agent launching and version information, and local session Trace. The CLI manages the same assets and relationships. It can batch Agent status queries and keep an operation alive while a person or another Agent reviews it before committing that original plan.

The argument-free TUI focuses on MCP management; the full CLI covers more capabilities. See [CLI / TUI](/en/guide/cli).

## Configuration and synchronization

Arca preserves unrelated settings, comments, and policy fields. Discovered external configuration stays read-only until explicitly managed. Central changes persist first; each physical target then synchronizes independently. One failed target does not roll back others that already succeeded.

Central API keys belong in the system Keychain. Agents receive environment references, retrieval commands, or an explicitly reviewed native delivery method. See [Models](/en/guide/models) for the limits.

[Install Arca](/en/guide/install) · [Watch the demo](/en/guide/demo) · [Supported Agents](/en/guide/agents)
