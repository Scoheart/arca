# Curated MCP catalog provenance (2026-11-24)

`data/registry.json` is the optional Arca 精选 subscription. Entries are templates, not running servers.

Counts on this branch, after the expansion:

| | Entries | Distinct names |
| --- | ---: | ---: |
| Original `main` `710edda51bfc5f19786ff7d8cc1b391a83200773` | 28 | 27 |
| This PR | 85 | 83 |

Net-new projects versus the original 27 names: **56**. The goal was at least 100. This PR does not claim that 100 verified, runnable projects were added. Duplicate transports do not add names: `firecrawl` and `supabase` each have two transports and one name.

The research packet (164 rows, 72 recommended, 41 conditional) was a lead list. Each added row below was checked against the vendor's own page or repository README on 2026-11-24. No credentialed login was performed. Star counts from the research packet are not repeated here.

## Added in this expansion (47 names)

Official hosted or local templates:

| Name | Config | Primary source |
| --- | --- | --- |
| `linear` | `https://mcp.linear.app/mcp` | https://linear.app/docs/mcp |
| `sentry` | `https://mcp.sentry.dev/mcp` | https://docs.sentry.io/product/sentry-mcp/ |
| `datadog` | `https://mcp.datadoghq.com/v1/mcp` (US1; other sites differ) | https://docs.datadoghq.com/mcp_server/setup |
| `grafana` | `uvx mcp-grafana` | https://grafana.com/docs/grafana/latest/ai-tools/mcp/ |
| `gitlab` | `https://gitlab.com/api/v4/mcp` | https://docs.gitlab.com/user/model_context_protocol/mcp_server/ |
| `redis` | `uvx --from redis-mcp-server@latest redis-mcp-server --url redis://localhost:6379/0` | https://redis.io/docs/latest/integrate/redis-mcp/install/ |
| `terraform` | `docker run -i --rm hashicorp/terraform-mcp-server` | https://github.com/hashicorp/terraform-mcp-server |
| `mongodb` | `npx -y mongodb-mcp-server@latest --readOnly` | https://github.com/mongodb-js/mongodb-mcp-server |
| `prisma` | `https://mcp.prisma.io/mcp` | https://www.prisma.io/docs/ai/tools/mcp-server |
| `neon` | `https://mcp.neon.tech/mcp` | https://github.com/neondatabase/mcp-server-neon |
| `wordpress-com` | `https://public-api.wordpress.com/wpcom/v2/mcp/v1` | https://developer.wordpress.com/docs/mcp/ |
| `zapier` | `https://mcp.zapier.com/api/v1/connect` | https://docs.zapier.com/mcp/overview/how-connections-work |
| `aws-knowledge` | `https://knowledge-mcp.global.api.aws` | https://github.com/awslabs/mcp/blob/main/src/aws-knowledge-mcp-server/README.md |
| `posthog` | `https://mcp.posthog.com/mcp` | https://posthog.com/docs/model-context-protocol |
| `canva` | `npx -y @canva/cli@latest mcp` | https://www.canva.dev/docs/connect/mcp-server/ |
| `webflow` | `https://mcp.webflow.com/mcp` | https://developers.webflow.com/mcp/reference/getting-started |
| `asana` | `https://mcp.asana.com/v2/mcp` | https://developers.asana.com/docs/using-asanas-mcp-server |
| `honeycomb` | `https://mcp.honeycomb.io/mcp` | https://docs.honeycomb.io/integrations/mcp/configuration-guide |
| `pagerduty` | `https://mcp.pagerduty.com/mcp` | https://support.pagerduty.com/main/docs/pagerduty-mcp-server |
| `huggingface` | `https://huggingface.co/mcp` | https://github.com/huggingface/hf-mcp-server |
| `pulumi` | `https://mcp.ai.pulumi.com/mcp` | https://www.pulumi.com/docs/iac/using-pulumi/mcp-server/ |
| `azure` | `npx -y @azure/mcp@latest server start` | https://github.com/microsoft/mcp/tree/main/servers/Azure.Mcp.Server |
| `shopify-dev` | `npx -y @shopify/dev-mcp@latest` | https://shopify.dev/docs/apps/build/devmcp |
| `salesforce` | `npx -y @salesforce/mcp` with org and toolset flags | https://github.com/salesforcecli/mcp |
| `docker-mcp-gateway` | `docker mcp gateway run` (stdio, no published port) | https://github.com/docker/mcp-gateway |
| `clickhouse` | `uv run --with mcp-clickhouse --python 3.12 mcp-clickhouse` | https://github.com/ClickHouse/mcp-clickhouse |
| `amplitude` | `https://mcp.amplitude.com/mcp` | https://amplitude.com/docs/amplitude-ai/amplitude-mcp |
| `clickup` | `https://mcp.clickup.com/mcp` | https://developer.clickup.com/docs/connect-an-ai-assistant-to-clickups-mcp-server |
| `mixpanel` | `https://mcp.mixpanel.com/mcp` | https://developer.mixpanel.com/docs/mcp |
| `newrelic` | `https://mcp.newrelic.com/mcp/` | https://docs.newrelic.com/docs/agentic-ai/mcp/setup |
| `descope` | `https://mcp.descope.com` | https://docs.descope.com/mcp/mcp-server |
| `shortcut` | `https://api.shortcutco.com/mcp` | https://docs.shortcutco.com/developers/mcp/overview |
| `klaviyo` | `https://mcp.klaviyo.com/mcp` | https://developers.klaviyo.com/en/docs/connect_to_the_klaviyo_mcp_server |
| `next-devtools` | `npx -y next-devtools-mcp@latest` | https://nextjs.org/docs/app/guides/mcp |
| `browser-use` | `https://api.browser-use.com/v3/mcp` plus bearer placeholder | https://browser-use.com/mcp |
| `browserbase` | `https://mcp.browserbase.com/mcp` (API key query omitted) | https://docs.browserbase.com/integrations/mcp/setup |
| `apify` | `https://mcp.apify.com` | https://github.com/apify/apify-mcp-server |
| `deepwiki` | `https://mcp.deepwiki.com/mcp` | https://docs.devin.ai/work-with-devin/deepwiki-mcp |
| `nuxt-ui` | `https://ui.nuxt.com/mcp` | https://ui.nuxt.com/docs/getting-started/ai/mcp |
| `google-mcp-toolbox` | `npx @toolbox-sdk/server --config tools.yaml` | https://github.com/googleapis/mcp-toolbox |

Reference implementations, labeled in the name and description, from the live `modelcontextprotocol/servers` tree (not `servers-archived`):

| Name | Config | Primary source |
| --- | --- | --- |
| `reference-filesystem` | `npx -y @modelcontextprotocol/server-filesystem --read-only /path/to/allowed-dir` | https://github.com/modelcontextprotocol/servers/tree/main/src/filesystem |
| `reference-sequential-thinking` | `npx -y @modelcontextprotocol/server-sequential-thinking` | https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking |
| `reference-fetch` | `uvx mcp-server-fetch` | https://github.com/modelcontextprotocol/servers/tree/main/src/fetch |
| `reference-git` | `uvx mcp-server-git --repository /path/to/git/repo` | https://github.com/modelcontextprotocol/servers/tree/main/src/git |
| `reference-time` | `uvx mcp-server-time` | https://github.com/modelcontextprotocol/servers/tree/main/src/time |

Community servers, labeled `community`:

| Name | Config | Primary source |
| --- | --- | --- |
| `kubernetes-community` | `npx mcp-server-kubernetes` with `ALLOW_ONLY_READONLY_TOOLS=true` | https://github.com/Flux159/mcp-server-kubernetes |
| `desktop-commander` | `npx -y @wonderwhy-er/desktop-commander@latest` | https://github.com/wonderwhy-er/DesktopCommanderMCP |

`desktop-commander` can run local shell commands. It is included only as an explicitly labeled community template.

## Already on this PR before this expansion

`serena`, `playwright`, `microsoft-learn`, `qdrant`, `mem0`, `reference-memory`, `dbhub`, `notion`, `atlassian-rovo`, plus hosted `supabase` HTTP beside the existing community stdio package. Context7 stays at `https://mcp.context7.com/mcp` with `Authorization: Bearer ${CONTEXT7_API_KEY}`. A recheck of https://context7.com/docs/resources/all-clients on 2026-11-24 still shows `Authorization` and `Bearer`. The page also still contains the string `CONTEXT7_API_KEY`; the HTTP examples used for this catalog remain the bearer header.

## Not added

These were in the research recommendation or conditional lists and were not given a catalog row:

| Candidate | Reason |
| --- | --- |
| Google Drive | Unchanged skip. https://developers.google.com/workspace/drive/api/guides/configure-mcp-server requires an OAuth client id and secret in the client. `HttpConfig` has no such fields. |
| Google BigQuery | https://docs.cloud.google.com/mcp/supported-products lists `https://bigquery.googleapis.com/mcp`, but the use guide fetched here did not show a client config, and the research probe reported HTTP 404 on that URL. |
| Box | https://developer.box.com/guides/box-mcp says custom clients need OAuth client credentials. That does not fit the current schema. |
| Storybook | Official MCP is `http://localhost:6006/mcp` after an addon install. Remote catalog URLs must be `https`. |
| JetBrains IDE MCP | https://www.jetbrains.com/help/idea/mcp-server.html generates a per-IDE config. There is no single portable command or URL. |
| Snowflake Cortex MCP | https://docs.snowflake.com/en/user-guide/snowflake-cortex/cortex-agents-mcp uses an account, database, schema, and server name. There is no single endpoint. |
| dbt Cloud | The cloud URL is account-specific. The local README does not give one complete placeholder env template. |
| Hostinger | https://github.com/hostinger/api-mcp-server tells clients to use absolute `node` and script paths and not `npx`. |
| Home Assistant | https://www.home-assistant.io/integrations/mcp_server/ uses the user's own instance URL. |
| Shopify Storefront | Per-shop hostname. `shopify-dev` is the portable official package. |
| AWS specialized suite | `awslabs/mcp` has many local servers. Only the hosted AWS Knowledge server has one documented URL, so the suite is not one catalog row. |
| Cloudflare Docs and other Cloudflare MCP variants | Existing `cloudflare-api` is already `https://mcp.cloudflare.com/mcp` in https://github.com/cloudflare/mcp. Extra Cloudflare products were not split out. |
| Datadog non-US1 sites | Represented by the US1 URL plus a description, not extra names. |
| HubSpot | The HubSpot developer page fetched here documents building your own MCP server, not the hosted CRM endpoint. |
| Elasticsearch, Semgrep, Snyk, Canva remote URL, Mintlify Admin, Buildkite, Intercom, Airtable, Draw.io, incident.io, Fireflies, Gong, Framer, Mailchimp, Knock | A primary page was fetched or attempted and did not yield a concrete command or HTTPS endpoint that this schema can store. |
| n8n community server | https://github.com/czlonkowski/n8n-mcp quick start points at a dashboard site, not a single stdio command in that section. |
| Archived reference servers | Puppeteer, Postgres, and the old reference GitHub/Drive/Slack servers stay out. |
| Snowflake-only or private-preview listings | Apiiro, Avalara, CloudZero, PayPal, Wiz, Wrike, Zoom, Lucid, Statsig, and the other conditional rows without a confirmed first-party config were not added. |

## Validation

A Python check of `data/registry.json` confirmed 85 entries, 83 names, one transport each, HTTPS for remote URLs, and empty or `${...}` values for env and header fields whose names contain key, token, secret, or password.

`cargo +stable test -p arca-core --lib resources::mcp::registry::tests` does **not** compile on this head. The failure is unchanged from `main`:

```text
error[E0061]: this function takes 5 arguments but 4 arguments were supplied
   --> core/src/resources/model/discovery.rs:3274:38
    |
    config.credential = Some(super::super::credential_service::create_revision(
...
    b"fixture-store-key",
note: function defined here
   --> core/src/resources/model/credential_service.rs:96:15
    provider_name: &str,
```

That test call is not part of this catalog change and was not patched. The registry unit tests were therefore not executed by Cargo on a clean tree.
