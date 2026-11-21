# Provider templates

This directory comes from the Arca Core templates: official APIs, regional and Coding Plans, model platforms, gateways, and local services. Search a name or category to narrow it down.

<ProviderReference lang="en" />

Templates provide default endpoints, protocols, and official links. They do not create accounts, obtain keys, or verify plans. Use your actual region, account, and Model ID. Coding Plan and pay-as-you-go keys are not interchangeable just because they share a brand.

Create several Provider instances for one platform when accounts, Base URLs, or credential references differ. A Provider is the access channel, not necessarily the model developer: when using a gateway, the gateway remains the Provider.

Local services can use unauthenticated connections. Complete any account, resource, port, or other placeholders before saving.

## Free and trial connections

- **BlockRun Free** accepts anonymous requests and lists currently available free chat models. Leave the Key field blank in **OVHcloud AI Endpoints** to use its anonymous quota. [BlockRun Free](https://blockrun.ai/free) · [OVHcloud limits](https://docs.ovhcloud.com/en/guides/public-cloud/ai-machine-learning/ai-endpoints-getting-started)
- **Infomaniak AI Services** needs the actual AI Services product ID in the Base URL and a token scoped to that product. Enabling the API requires a credit card. [Product IDs](https://developer.infomaniak.com/docs/api/get/1/ai) · [Trial requirements](https://www.infomaniak.com/en/support/faq/2845/getting-started-guide-ai-services-sovereign-ai-services)
- **Vertex AI Express** uses an API key issued by Google Cloud Express. Use the Google Gemini template for keys issued by Google AI Studio. [Express setup](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/start/express-mode/overview)

[Add a Provider and Model](/en/guide/models)
