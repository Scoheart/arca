# Step Code integration

Verified on 2026-09-23 against the official Step Code documentation. No user configuration, credentials, or session files were read or changed.

| Arca Agent | Command | User MCP file | User Skills directory |
|---|---|---|---|
| `step-code` | `~/.stepcode/bin/step` | `~/.stepcode/config.toml` (`[mcp_servers]`) | `~/.stepcode/agent/skills` |

Step Code also reads `~/.agents/skills`. Arca records that directory as a compatibility alias. Project `.stepcode/config.toml` is not a write target.

## Verified contracts

- Official [MCP](https://platform.stepfun.com/docs/zh/step-code/customization/mcp) declares stdio with `command` / `args` / `env` and Streamable HTTP with `url`. Legacy SSE is not a supported substitute. HTTP headers use `http_headers`. Bearer env vars, OAuth tables, tool filters, and timeouts stay outside the fields Arca rewrites.
- `enabled = false` keeps a server out of Step Code's active list. Arca refuses to update that entry instead of presenting it as effective.
- Official [Skills](https://platform.stepfun.com/docs/zh/step-code/customization/skills) use `SKILL.md` packages in `~/.stepcode/agent/skills`.
- Official [models](https://platform.stepfun.com/docs/zh/step-code/configuration/models) and [data paths](https://platform.stepfun.com/docs/zh/step-code/configuration/data-paths) put credentials in `auth.json` and custom providers in `models.json`. Arca does not write either file.
- The installer places the binary at `~/.stepcode/bin/step`. Arca launches that path and does not probe a bare `step` command, because other products use the same name.

## Sources

- [Step Code quickstart](https://platform.stepfun.com/docs/zh/step-code/quickstart)
- [Configuration files](https://platform.stepfun.com/docs/zh/step-code/configuration/files)
- [Repository](https://github.com/stepfun-ai/Step-Code)
- Official README banner `https://static-openapi.stepfun.com/resource/test.jpg`; the bundled icon is the unmodified mascot region of that asset.
