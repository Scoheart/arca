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

## 2026-11-25 source URL and additions

`source_url` records the official documentation or first-party source page. It is optional on `RegistryEntry`, so an older subscription that omits it still parses. Curated templates are not auto-installed.

Backfill for the previous 128 names uses the first-party URL already stored in `repo`, except where that field is the code repository and the 2026-11-25 directory recorded a distinct documentation page: `context7`, `firecrawl`, `tavily`, `serena`, `terraform`, `mongodb`, `dbhub`, `qdrant`, and `elasticsearch`. Those documentation pages were fetched again on 2026-11-25 and returned HTTP 200. `luma` still has no `source_url` because no first-party URL was recorded, and none was invented. The Salesforce docs URL from the directory returned HTTP 403 from this environment, so `salesforce` keeps the previously verified GitHub source.

This pass adds 57 names whose official pages showed a client config the catalog can store: an HTTPS URL, with headers omitted when OAuth is the documented default, or a stdio command. Secret env and header values are empty or `${...}`. Instance hosts use the documented placeholder, the same way `snowflake` and `n8n` already do.

Counts after this pass: **187 entries, 185 names**. The directory under consideration had 302 products. 128 were already present. 57 are new templates. 117 remain absent. That is not a claim that all 302 were added.

### Added

| Name | Config | Source |
| --- | --- | --- |
| `1password` | `1password-mcp` | https://www.1password.dev/environments/mcp-server |
| `21st-dev-magic` | `https://21st.dev/api/mcp` with `x-api-key` placeholder | https://github.com/21st-dev/magic-mcp |
| `adobe` | `https://adobe-creativity.adobe.io/mcp` | https://claude.com/marketplace/connectors/adobe-creativity |
| `ahrefs` | `https://api.ahrefs.com/mcp/mcp` | https://docs.ahrefs.com/en/mcp/docs/introduction |
| `airbyte` | `https://mcp.airbyte.com/mcp` | https://docs.airbyte.com/platform/airbyte-mcp |
| `airtable` | `https://mcp.airtable.com/mcp` | https://support.airtable.com/articles/9897799762-using-the-airtable-mcp-server |
| `aiven` | `https://mcp.aiven.live/mcp` | https://github.com/Aiven-Open/mcp-aiven |
| `alchemy` | `https://mcp.alchemy.com/mcp` | https://github.com/alchemyplatform/alchemy-mcp-server |
| `algolia` | `https://mcp.algolia.com/mcp` | https://www.algolia.com/doc/guides/model-context-protocol/productivity-mcp |
| `alibaba-cloud-ops` | `uvx alibaba-cloud-ops-mcp-server@latest` | https://github.com/aliyun/alibaba-cloud-ops-mcp-server |
| `alibaba-devops` | `https://openapi-rdc.aliyuncs.com/ai/mcp` with bearer placeholder | https://github.com/aliyun/alibabacloud-devops-mcp-server |
| `alpaca` | `uvx alpaca-mcp-server` | https://github.com/alpacahq/alpaca-mcp-server |
| `anki` | `npx -y @ankimcp/anki-mcp-server --stdio` | https://github.com/ankimcp/anki-mcp-server |
| `antv-chart` | `npx -y @antv/mcp-server-chart` | https://github.com/antvis/mcp-server-chart |
| `apache-doris` | `doris-mcp-server --transport stdio` | https://github.com/apache/doris-mcp-server |
| `apollo-io` | `https://mcp.apollo.io/mcp` | https://github.com/apolloio/apollo-mcp-plugin |
| `ashby` | `https://mcp.ashbyhq.com/mcp/v1` | https://docs.ashbyhq.com/mcp |
| `auth0` | `npx -y @auth0/auth0-mcp-server run` | https://auth0.com/docs/get-started/auth0-mcp-server/getting-started-with-auth0-mcp-server |
| `axiom` | `https://mcp.axiom.co/mcp` | https://github.com/axiomhq/mcp-server-axiom |
| `azure-devops` | `https://mcp.dev.azure.com/{organization}` | https://learn.microsoft.com/en-us/azure/devops/mcp-server/remote-mcp-server |
| `blender` | `uvx mcp-for-blender` | https://github.com/ahujasid/blender-mcp |
| `booking` | `https://demandapi-mcp.booking.com/v1/sp/mcp/` | https://developers.booking.com/mcp-server/docs/about |
| `brex` | `https://api.brex.com/mcp` | https://developer.brex.com/docs/mcp |
| `bright-data` | `https://mcp.brightdata.com/mcp?token=${BRIGHT_DATA_API_TOKEN}` | https://github.com/brightdata/brightdata-mcp |
| `buddy` | `https://mcp.buddy.works/mcp` | https://buddy.works/docs/ai-agents/mcp |
| `cal-com` | `npx @calcom/cal-mcp@latest` | https://github.com/calcom/cal-mcp |
| `clerk` | `https://mcp.clerk.com/mcp` | https://clerk.com/docs/guides/ai/mcp/clerk-mcp-server |
| `coingecko` | `https://mcp.api.coingecko.com/mcp` | https://docs.coingecko.com/ai-integration/mcp-server |
| `couchbase` | `uvx couchbase-mcp-server operational` | https://github.com/couchbase/mcp-server-couchbase |
| `crowdstrike` | `uvx falcon-mcp` | https://github.com/CrowdStrike/falcon-mcp |
| `dbt` | `https://YOUR_DBT_HOST_URL/api/ai/v1/mcp/` | https://docs.getdbt.com/docs/dbt-ai/mcp-quickstart-remote |
| `deepl` | `https://mcp.deepl.com/v1/mcp` | https://developers.deepl.com/docs/mcp |
| `devcycle` | `https://mcp.devcycle.com/mcp` | https://github.com/DevCycleHQ/cli |
| `docling` | `uvx --from=docling-mcp docling-mcp-server` | https://github.com/docling-project/docling-mcp |
| `excalidraw` | `https://mcp.excalidraw.com` | https://github.com/excalidraw/excalidraw-mcp |
| `expo` | `https://mcp.expo.dev/mcp` | https://docs.expo.dev/mcp/ |
| `fireflies` | `https://api.fireflies.ai/mcp` | https://docs.fireflies.ai/getting-started/mcp-configuration |
| `firefox-devtools` | `npx -y @mozilla/firefox-mcp@latest --headless --viewport 1280x720` | https://github.com/mozilla/firefox-devtools-mcp |
| `gamma` | `https://mcp.gamma.app/mcp` | https://developers.gamma.app/ |
| `gitkraken` | `gk mcp` | https://github.com/gitkraken/gk-cli |
| `gitmcp` | `https://gitmcp.io/{owner}/{repo}` | https://github.com/idosal/git-mcp |
| `globalping` | `https://mcp.globalping.dev/mcp` | https://github.com/jsdelivr/globalping-mcp-server |
| `google-bigquery` | `https://bigquery.googleapis.com/mcp` | https://docs.cloud.google.com/bigquery/docs/use-bigquery-mcp |
| `incident-io` | `https://mcp.incident.io/mcp` | https://docs.incident.io/ai/remote-mcp |
| `launchdarkly` | `https://mcp.launchdarkly.com/mcp` | https://launchdarkly.com/docs/home/getting-started/mcp-hosted |
| `lucid` | `https://mcp.lucid.app/mcp` | https://github.com/lucidsoftware/lucid-mcp-server |
| `mailchimp` | `https://mandrillapp.com/mcp` with bearer placeholder | https://mailchimp.com/developer/transactional/guides/how-to-use-mailchimps-transactional-messaging-mcp/ |
| `obsidian` | `uvx mcp-obsidian` | https://github.com/MarkusPfundstein/mcp-obsidian |
| `snyk` | `snyk mcp -t stdio` | https://docs.snyk.io/agent-security/agentic-security-with-snyk-studio/getting-started-with-snyk-studio |
| `zoominfo` | `https://mcp.zoominfo.com/mcp` | https://docs.zoominfo.com/docs/connect-to-zoominfo-mcp |
| `growthbook` | `https://mcp.growthbook.io/mcp` | https://github.com/growthbook/growthbook-mcp |
| `lovable` | `https://mcp.lovable.dev` | https://docs.lovable.dev/integrations/lovable-mcp-server |
| `plane` | `https://mcp.plane.so/http/mcp` | https://github.com/makeplane/plane-mcp-server |
| `postman` | `https://mcp.postman.com/minimal` | https://github.com/postmanlabs/postman-mcp-server |
| `tableau` | `https://mcp.tableau.com` | https://github.com/tableau/tableau-mcp |
| `todoist` | `https://ai.todoist.net/mcp` | https://github.com/Doist/todoist-mcp |
| `wix` | `https://mcp.wix.com/mcp` | https://github.com/wix/wix-mcp |

### Not added in this pass

| Candidate | Reason |
| --- | --- |
| Google Drive | The published client JSON requires an OAuth client id and client secret beside `https://drivemcp.googleapis.com/mcp/v1`. `HttpConfig` has no fields for those. |
| Box | Hosted `https://mcp.box.com` still needs an OAuth client id and client secret from Box Integration Credentials. |
| HubSpot | `https://mcp.hubspot.com/` is documented, and the client must also use a connector client id and client secret. |
| Wrike | `https://mcp.wrike.com/v2` requires a Wrike OAuth client id and client secret. |
| Zoom | Zoom MCP servers use manual client registration. Connecting starts by creating a Zoom app. |
| Apollo GraphQL | The quickstart client URL is `http://127.0.0.1:8000`. Remote catalog URLs must be `https`. |
| Home Assistant | The documented example is `http://homeassistant.local:8123/api/mcp` on the user's instance. |
| Storybook | Preview status, and the only client URL is `http://localhost:6006/mcp`. |
| JetBrains IDE MCP | The IDE generates a per-installation SSE, stdio, or HTTP config. There is no portable command or URL. `JetBrains/mcp-jetbrains` says it is no longer maintained. |
| Shopify Storefront | Catalog and cart tools on `https://{shop}/api/mcp` were removed. The replacement is still per shop, `https://{shop}/api/ucp/mcp`. |
| Databricks | The fetched MCP tools page describes workspace-registered servers, not one portable client URL or command. |
| Cycode | The CLI marks `cycode mcp` as an experiment. |
| Apache IoTDB | The client JSON runs `uv` against a local clone path. No published stdio package command or HTTPS URL was shown. |
| Arize Phoenix | No MCP command in the product README. The install link shown is `http://localhost:6006/mcp`. |
| Chargebee | The in-repo `@chargebee/mcp` package is marked deprecated, and the replacement page did not show a portable client config. |
| Chrome MCP extension | The recommended URL is `http://127.0.0.1:12306/mcp`. |
| Docker Hub | Client examples run `node` against a cloned `dist/index.js`. No published npx or HTTPS template was shown. |
| DocuSign | No portable HTTPS URL or command. Custom connectors require an allowlisted integration key. |
| Descript, Dropbox | The fetched pages are Claude connector installs and do not print a portable URL or command. |
| No first-party URL | Apiiro, Avalara, CloudZero, Framer, Gong, Google Chat, Google Docs, Google Sheets, Knock, Linx, Mastra Docs, Meta Ads, Microsoft OneDrive, Microsoft Outlook, Microsoft SharePoint, Microsoft Teams, Mintlify Admin, Okta as listed with the Cortex qualifier, Outreach, PipeCD Docs, Sigma, Spinach, Telnyx, Torii, Upwind, Wiz, and Agent Device. No URL was invented for these. |
| Neo4j Labs bundle | https://github.com/neo4j-contrib/mcp-neo4j says it is a Labs bundle, not the supported product server, and points at https://github.com/neo4j/mcp. A single portable product command was not copied from the Labs index. |
| Tinybird repository | The fetched `tinybirdco/mcp-tinybird` README says the repository is deprecated and points at a newer docs page. That page was not turned into a template in this pass. |
| Make legacy package | https://github.com/integromat/make-mcp-server says it is legacy and recommends https://developers.make.com/mcp-server. The legacy API-key command was not added. |
| Readwise npm package | The fetched README says `@readwise/readwise-mcp` is deprecated. The replacement page https://readwise.io/mcp was not fetched far enough to copy a client config. |
| Still being checked | Other newly found vendor servers, including Neo4j's product server, Trello, and the pages that did not show a portable command in this pass, are not templates yet. |

## Validation

A Python check of `data/registry.json` confirmed 187 entries, 185 names, one transport each, HTTPS for remote URLs, and empty or `${...}` values for env and header fields whose names contain key, token, secret, or password.

`cargo test -p arca-core --lib resources::mcp::registry` passed on Linux with Rust 1.100: 9 tests, including the curated collection checks. `cargo test -p arca-cli` did not compile. `credential_service` is gated with `#[cfg(any(target_os = "macos", test))]`, and the Linux library build fails on that pre-existing model module. That failure is outside this MCP catalog change. Desktop tests were not run because `desktop/node_modules` is not installed in this environment.

The previous clean `cargo test` failure was `create_revision` in `core/src/resources/model/discovery.rs` being called with four arguments. `credential_service::create_revision` requires `provider_name` between the provider id and the key. This change passes `&config.name`, the same value the other model tests pass. It does not change model discovery behavior.
