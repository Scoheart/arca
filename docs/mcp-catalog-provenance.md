# Curated MCP catalog provenance (2026-11-24)

`data/registry.json` is the optional Arca 精选 subscription. Entries are templates, not running servers.

Counts on this branch, after the expansion:

| | Entries | Distinct names |
| --- | ---: | ---: |
| Original `main` `710edda51bfc5f19786ff7d8cc1b391a83200773` | 28 | 27 |
| This PR | 130 | 128 |

Net-new projects versus the original 27 names: **101**. The goal was at least 100 distinct new projects, which is 127 names. This catalog has 128 names, so the net-new count is 101. Duplicate transports do not add names: `firecrawl` and `supabase` each have two transports and one name.

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
| `browserbase` | `https://mcp.browserbase.com/mcp?browserbaseApiKey=${BROWSERBASE_API_KEY}` | https://docs.browserbase.com/integrations/mcp/setup |
| `apify` | `https://mcp.apify.com` with no Authorization header | https://github.com/apify/apify-mcp-server |
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

## Audit corrections

Checked again on 2026-11-24 against the current vendor pages:

- Apify's recommended hosted config is `https://mcp.apify.com` with OAuth and no header. `Authorization: Bearer <APIFY_TOKEN>` is the documented fallback and skips OAuth, so it stays out of the template. The description no longer says the template itself carries a bearer. Local stdio is `npx -y @apify/actors-mcp-server` with `APIFY_TOKEN`. Source: https://docs.apify.com/platform/integrations/mcp and https://github.com/apify/apify-mcp-server.
- Browserbase tool calls require `browserbaseApiKey`. The template URL is now `https://mcp.browserbase.com/mcp?browserbaseApiKey=${BROWSERBASE_API_KEY}`. `Mcp-Session-Id` is the MCP transport session header, not a catalog field. A client that opens a new transport per call must reuse the Browserbase `sessionId` returned by `start`. Source: https://docs.browserbase.com/integrations/mcp/setup.
- monday.com, Squarespace, Steel, and Upstash Redis were omitted from the previous provenance table. They are real templates and are listed below. monday.com's documented client JSON is URL-only plus `Api-Version: 2026-07`. Steel's OAuth path is URL-only; the bearer header is the non-OAuth fallback. Upstash Redis is the stdio package `@upstash/redis-mcp`, not a guessed `mcp.upstash.com` host and not Context7.

## Added after the audit (45 names)

| Name | Config | Primary source |
| --- | --- | --- |
| `monday` | `https://mcp.monday.com/mcp` plus `Api-Version: 2026-07` | https://developer.monday.com/api-reference/docs/integrate-with-monday-mcp |
| `squarespace` | `https://mcp.squarespace.com/mcp` | https://developers.squarespace.com/mcp |
| `steel` | `https://mcp.steel.dev/mcp` (OAuth; bearer not preset) | https://docs.steel.dev/overview/mcp |
| `upstash-redis` | `npx -y @upstash/redis-mcp` with REST URL placeholder and empty token | https://upstash.com/docs/redis/sdks/mcp |
| `intercom` | `https://mcp.intercom.com/mcp` | https://developers.intercom.com/docs/guides/mcp |
| `semgrep` | `https://semgrep.dev/api/mcp` | https://docs.semgrep.dev/mcp/authentication |
| `render` | `https://mcp.render.com/mcp` | https://render.com/docs/mcp-server |
| `twilio-docs` | `https://mcp.twilio.com/docs` | https://www.twilio.com/docs/ai/mcp |
| `replicate` | `npx -y replicate-mcp` | https://replicate.com/docs/reference/mcp |
| `sanity` | `https://mcp.sanity.io` | https://www.sanity.io/docs/mcp |
| `resend` | `https://mcp.resend.com/mcp` | https://github.com/resend/resend-mcp |
| `draw-io` | `https://mcp.draw.io/mcp` | https://github.com/jgraph/drawio-mcp |
| `together-docs` | `https://docs.together.ai/mcp` | https://docs.together.ai/docs/mcp |
| `pinecone` | `npx -y @pinecone-database/mcp` | https://github.com/pinecone-io/pinecone-mcp |
| `chroma` | `uvx chroma-mcp` | https://github.com/chroma-core/chroma-mcp |
| `elevenlabs` | `uvx elevenlabs-mcp` | https://github.com/elevenlabs/elevenlabs-mcp |
| `paypal` | `npx -y @paypal/mcp --tools=all` in sandbox | https://github.com/paypal/paypal-mcp-server |
| `xero` | `npx -y @xeroapi/xero-mcp-server@latest` | https://github.com/XeroAPI/xero-mcp-server |
| `elasticsearch` | `docker run -i --rm -e ES_URL -e ES_API_KEY docker.elastic.co/mcp/elasticsearch stdio` | https://github.com/elastic/mcp-server-elasticsearch |
| `netlify` | `npx -y @netlify/mcp` | https://github.com/netlify/netlify-mcp |
| `railway` | `https://mcp.railway.com` | https://docs.railway.com/cli/mcp |
| `motherduck` | `uvx mcp-server-motherduck --db-path /absolute/path/to/your.duckdb` | https://github.com/motherduckdb/mcp-server-motherduck |
| `prefect` | `uvx --from prefect-mcp prefect-mcp-server` | https://github.com/PrefectHQ/prefect-mcp-server |
| `deepgram` | `deepgram-mcp` | https://github.com/deepgram/mcp |
| `confluent` | `npx -y @confluentinc/mcp-confluent --config ./config.yaml` | https://github.com/confluentinc/mcp-confluent |
| `buildkite` | `https://mcp.buildkite.com/mcp` | https://buildkite.com/docs/apis/mcp-server |
| `contentful` | `npx -y @contentful/mcp-server` | https://github.com/contentful/contentful-mcp-server |
| `appwrite` | `https://mcp.appwrite.io/` | https://github.com/appwrite/mcp |
| `langfuse-docs` | `https://langfuse.com/api/mcp` | https://langfuse.com/docs/docs-mcp |
| `convex` | `npx -y convex@latest mcp start` | https://docs.convex.dev/ai/convex-mcp-server |
| `gitbook` | `https://mcp.gitbook.com/mcp` | https://gitbook.com/docs/gitbook-mcp |
| `square` | `https://mcp.squareup.com/mcp` | https://developer.squareup.com/docs/mcp |
| `planetscale` | `https://mcp.pscale.dev/mcp/planetscale` | https://planetscale.com/docs/mcp-server |
| `hostinger` | `https://mcp.hostinger.com` | https://github.com/hostinger/api-mcp-server |
| `snowflake` | account placeholder `https://YOUR_ACCOUNT.snowflakecomputing.com/api/v2/databases/YOUR_DATABASE/schemas/YOUR_SCHEMA/mcp-servers/YOUR_SERVER` | https://docs.snowflake.com/en/user-guide/snowflake-cortex/cortex-agents-mcp |
| `n8n` | instance placeholder ending in `/mcp-server/http` | https://docs.n8n.io/connect/connect-to-n8n-mcp-server |
| `firebase` | `npx -y firebase-tools@latest mcp` | https://firebase.google.com/docs/ai-assistance/mcp-server |
| `calendly` | `https://mcp.calendly.com` | https://developer.calendly.com/docs/mcp/calendly-mcp-server |
| `circleci` | `https://mcp.circleci.com/v1/mcp` | https://circleci.com/docs/guides/toolkit/circleci-mcp-overview/ |
| `mailgun` | `npx -y @mailgun/mcp-server` | https://documentation.mailgun.com/docs/mailgun/mcp |
| `doppler` | `npx -y @dopplerhq/mcp-server` | https://docs.doppler.com/docs/mcp |
| `inngest` | `https://api.inngest.com/mcp` | https://www.inngest.com/docs/ai-dev-tools/mcp |
| `braintrust` | `https://api.braintrust.dev/mcp` | https://www.braintrust.dev/docs/guides/mcp |
| `daytona` | `daytona-v0 mcp start` | https://www.daytona.io/docs/en/mcp |
| `statsig` | `https://api.statsig.com/v3/mcp` | https://docs.statsig.com/integrations/mcp/manual-setup |

## Not added

These were in the research recommendation or conditional lists and were not given a catalog row:

| Candidate | Reason |
| --- | --- |
| Google Drive | Unchanged skip. https://developers.google.com/workspace/drive/api/guides/configure-mcp-server requires an OAuth client id and secret in the client. `HttpConfig` has no such fields. |
| Google BigQuery | https://docs.cloud.google.com/mcp/supported-products lists `https://bigquery.googleapis.com/mcp`, but the use guide fetched here did not show a client config, and an earlier probe of that URL returned HTTP 404. |
| Box | https://developer.box.com/guides/box-mcp says custom clients need OAuth client credentials. That does not fit the current schema. |
| Storybook | Official MCP is `http://localhost:6006/mcp` after an addon install. Remote catalog URLs must be `https`. |
| JetBrains IDE MCP | https://www.jetbrains.com/help/idea/mcp-server.html generates a per-IDE config. There is no single portable command or URL. |
| dbt | https://docs.getdbt.com/docs/dbt-ai/about-mcp describes `uvx dbt-mcp` and a remote server. The fetched README did not include a complete env template or one remote URL. |
| Home Assistant | https://www.home-assistant.io/integrations/mcp_server/ documents `/api/mcp` on the user's instance. The examples use `http`, which this catalog cannot store. |
| Shopify Storefront | Per-shop hostname. `shopify-dev` remains the portable official package. |
| AWS specialized suite | `awslabs/mcp` has many local servers. Only the hosted AWS Knowledge server has one documented URL. |
| Cloudflare extra products | https://developers.cloudflare.com/agents/model-context-protocol/mcp-servers-for-cloudflare/ says `https://mcp.cloudflare.com/mcp` is the server for the Cloudflare API. Existing `cloudflare-api` already uses it. |
| HubSpot | https://developers.hubspot.com/mcp did not show `https://mcp.hubspot.com`. A guessed host was not added. |
| Airtable, Fireflies, Gong, Framer, Mailchimp, Mintlify Admin, Knock, incident.io | Vendor-looking hosts or HTML shells were checked. None produced a concrete official client config in this pass. |
| Snyk | The fetched Snyk docs paths did not show `snyk mcp`. The command was not copied from a secondary guide. |
| Inngest Dev Server | Documented as `http://127.0.0.1:8288/mcp`. The Cloud URL is the catalog row. |
| Windmill | The docs say the server is at `/api/mcp` on the user's instance, without a complete auth template in the fetched text. |
| Chargebee, Databricks, DigitalOcean, Fly.io's own server, Discord, ReadMe, Front | Pages loaded, but the fetched text did not include a concrete command or HTTPS MCP URL. |
| Weaviate standalone | The fetched README says the standalone server is deprecated. |
| Archived reference servers | Puppeteer, Postgres, and the old reference GitHub, Drive, and Slack servers stay out. |
| Conditional rows without a first-party config | Apiiro, Avalara, CloudZero, Wiz, Wrike, Zoom, Lucid, and the other conditional rows without a confirmed config were not added. PayPal and Statsig are no longer in this group. |

## Validation

A Python check of `data/registry.json` confirmed 130 entries, 128 names, one transport each, HTTPS for remote URLs, and empty or `${...}` values for env and header fields whose names contain key, token, secret, or password.

The previous clean `cargo test` failure was `create_revision` in `core/src/resources/model/discovery.rs` being called with four arguments. `credential_service::create_revision` requires `provider_name` between the provider id and the key. This change passes `&config.name`, the same value the other model tests pass. It does not change model discovery behavior.
