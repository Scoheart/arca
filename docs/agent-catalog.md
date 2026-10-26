# Agent Catalog Methodology

Arca deliberately separates client discovery from writable configuration support. A product appearing in an MCP directory proves that it is relevant to users; it does not prove a stable global file path or wire schema.

## Data sets

| File | Purpose | Current size |
|---|---|---:|
| `data/agents.json` | Deep-audited definitions with product-specific capabilities and evidence | 89 |
| `data/agent-catalog.json` | Discovery-only client records; unknown paths and formats always fail closed | 251 |
| `data/agent-catalog-supplements.json` | Reviewed discovery entries, configuration guidance, and source evidence | 11 |

There are 64 overlapping IDs, producing 276 distinct retained identities as of 2026-10-16. Audited definitions override discovery entries. A retained identity is not necessarily visible: built-in entries require a bundled brand icon. Visibility never grants a writer.

## Discovery sources

The catalog combines [Glama](https://glama.ai/mcp/clients), pinned [ACP Registry](https://github.com/agentclientprotocol/registry) manifests, official supplements, and audited definition names. [MCP client support](https://modelcontextprotocol.io/extensions/client-matrix) and [Docker client documentation](https://docs.docker.com/reference/cli/docker/mcp/client/) provide cross-checks.

`node scripts/update-agent-catalog.mjs` refreshes network discovery, resolves ACP to one commit, and applies reviewed supplements without promoting them into writable targets. It aborts below 180 catalog entries. A fixed `ARCA_CATALOG_DATE` controls collection dates, not the changing network sources.

`node scripts/update-agent-catalog.mjs --reviewed-only` applies only the reviewed JSON layer, retaining unrelated records and ordering without fetching network discovery. All generated discovery records have null global/project paths, no transports, and `format: unknown`. Their evidence verifies identity and guidance, not a file-writing contract.

See [MCP Apps client coverage](mcp-apps-clients.md) for the 2026-10-16 additions and their management boundaries.

## Promotion criteria

A catalog client is promoted into `data/agents.json` only after all of these are known:

1. An official product page or official source confirms MCP client support.
2. For an MCP writer, a user-level global config path is stable for the target platform; otherwise the MCP capability is explicitly read-only. A separately verified Skills-only definition does not gain an MCP writer.
3. The top-level MCP key and map/list layout are known.
4. Local and remote transport fields are known independently.
5. Header, environment, working-directory, and command representations are known.
6. A codec test proves the exact on-disk shape.
7. Round-trip tests prove that unrelated settings and Agent-owned policy fields survive.

Community extensions are not presented as native support. Pi 0.99.0 added built-in MCP support; its audited writable record now cites the official MCP documentation. Earlier Pi releases still need an extension to consume the same `mcp.json` path.

## Agent Skills capabilities

Skills support is audited separately from MCP configuration. Only definitions from `data/agents.json` may carry a trusted nested `skills` capability, and only when official product documentation confirms a user-level Skills directory and read-only installation evidence. Any such field from the discovery-only `data/agent-catalog.json` is discarded before the data sets are merged. Custom Agent definitions cannot create or override this metadata, and upgrades refresh it from audited data.

| Field | Contract |
|---|---|
| `target_id` | Stable identity for the preferred physical assignment target. |
| `global_dir` | Preferred user-level writable directory; it must be a safe `~/.../skills` path outside `~/.arca`. |
| `aliases` | Other verified target IDs and user-level directories the Agent reads. Aliases are compatibility targets, not the preferred write location. |
| `docs` | Official documentation that supports the Skills capability. |
| `evidence` | Verification level for the capability; the initial audited records require `official`. |
| `verified_at` | Date the Skills-specific evidence was last checked. |
| `probes` | Read-only command, path, or macOS bundle checks that indicate the Agent is installed. Probe paths are evidence only and may be absolute application paths. |

All preferred directories and aliases are validated before the built-in catalog is returned. The same `target_id` must always name the same path, and a physical path cannot be assigned contradictory target IDs. The current audited data declares 46 Skills-capable Agents; missing definitions, evidence outside the reviewed `official` / `official-source` levels, blank documentation or verification dates, and empty probe lists reject the built-in catalog. Shared directories such as `~/.agents/skills` are never treated as installation evidence by themselves.

## Evidence levels

| Value | Meaning |
|---|---|
| `official` | Official product documentation describes the relevant path/schema. |
| `official-source` | Current official source code, including source shipped inside a signed official application bundle, resolves a platform path or field more precisely than the prose docs. |
| `community-extension` | MCP support belongs to a named third-party extension, not the core product. |
| `catalog` | Public discovery only; never writable. |

Every audited definition records a verification date and evidence URL; MCP-capable definitions additionally record transports, codec, and layout. The latest CLI and OpenClaw additions were verified on 2026-09-07; existing records retain their individual evidence dates. Grok Build was verified against its newly published official source and MiniMax Code against its signed `3.0.51` application bundle.

## Fail-closed rules

- Unknown/catalog-only formats never fall back to a generic JSON writer.
- Built-in schema metadata is refreshed on upgrade; user path and enabled-state overrides survive.
- Invalid or ambiguous JSON, TOML, or YAML is never repaired by overwriting it.
- Only the named MCP entry is exposed to scanners, previews, disable snapshots, and source caches.
- Full Agent files may exist only as local pre-write backups.

Run the validation suite after changing either data set:

```bash
ARCA_CATALOG_DATE=2026-07-14 node scripts/update-agent-catalog.mjs
cargo test --workspace --locked
bash desktop/scripts/prepare-sidecar.sh
cargo test --manifest-path desktop/src-tauri/Cargo.toml --locked
cd desktop && npm run build
```

## September 2026 additions

- Antigravity CLI shares `~/.gemini/config/mcp_config.json` with the IDE, with `serverUrl` for remote MCP. Its flat Markdown slash-command Skills are not advertised as directory-based Skills.
- Continue CLI (`cn`) shares `~/.continue/config.yaml` and the reviewed list codec with Continue IDE. Explicit `--config` selections require a user path override.
- TRAE CLI 2.0 uses `~/.trae/traecli.toml`, `mcp_servers`, and reviewed stdio entries. Arca does not fall back to the old YAML format or infer remote transport fields.
- OpenClaw exposes the verified `~/.openclaw/skills` directory and command/config installation probes. Its JSON5 MCP/Models configuration is not writable until `$include` and SecretRef ownership are handled.

Full source-linked market review: [Agent and Provider coverage](agent-provider-research-2026-09-07.md).
