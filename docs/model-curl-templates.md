# Model cURL templates

Reviewed against official request references on **2026-10-27**. Desktop **Copy cURL** and `arca model curl <profile-id>` share Core's renderer and `data/model-curl-templates.json`.

The exported text is **one cURL command** containing the URL, headers and an editable JSON request body. Line continuations and JSON indentation keep it readable. There are no comments, subshell wrappers, temporary variables, here-documents, preprocessing steps or response pipelines in the copied command. Only cURL is required. Export does not make an API request itself.

## Values and defaults

The internal `data/model-curl-templates.json` catalog retains every reviewed top-level key. `null` is **Arca's unset marker**, not a claim that an API accepts JSON null or has a null default. Core removes those top-level keys before rendering the command, so they do not clutter copied text or get sent to the API. False, zero, empty strings/arrays/objects and nested nulls are preserved exactly. To enable an omitted option or deliberately send a literal top-level null, add it directly to the copied JSON body.

Required fields and basic text examples have concrete values. Output-token limits of 1024 and the instruction “You are a helpful assistant. Answer clearly and concisely. If you are unsure, say so.” are editable **Arca example choices**, not universal API defaults. Model-dependent sampling, reasoning, audio, caching and account resources remain unset. An empty safety-settings list leaves the API's safety policy in effect.

Responses explicitly uses its documented `store=true` default. Chat Completions uses `store=false`; Interactions retains Arca's stateless `store=false` example. Where the protocol has a `stream` field, it defaults to false. To enable SSE, set the supported streaming options and optionally add cURL's `--no-buffer`. generateContent requires the separate `streamGenerateContent?alt=sse` endpoint; Converse uses a separate `/converse-stream` endpoint with AWS event-stream, not SSE. Neither accepts a `stream` request-body field.

Reviewed service restrictions are declared in `provider_overrides`, keyed by the stored Provider type and protocol. These replace whole top-level request values before model selection and unset-field filtering; protocol defaults remain unchanged for other providers. decuff's stateless Responses endpoint exports `store=false`, as required by its [documentation](https://decuff.ai/docs).

## Protocol coverage

| Protocol | Top-level keys | System example | Official reference |
| --- | ---: | --- | --- |
| OpenAI Chat Completions | 37 | `messages` with `system` then `user` | [Create chat completion](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) |
| OpenAI Responses | 32 | `instructions`, with user `input` | [Create response](https://developers.openai.com/api/reference/resources/responses/methods/create) |
| Anthropic Messages | 20 | top-level `system`, with user `messages` | [Create message](https://platform.claude.com/docs/en/api/messages/create), [version migration](https://platform.claude.com/docs/en/api/migration-2026-10-21) |
| Gemini Interactions | 17 | `system_instruction`, with user `input` | [Interactions request](https://ai.google.dev/api/interactions-api), [overview](https://ai.google.dev/gemini-api/docs/interactions-overview) |
| Gemini generateContent | 11 | `systemInstruction.parts[].text`, with user `contents` | [generateContent request](https://ai.google.dev/api/generate-content) |
| AWS Bedrock Converse | 12 | `system[].text`, with user `messages` | [Converse request](https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html) |

Counts describe the internal catalog, not the number of fields in a copied command. They cover documented standard request-body fields, including deprecated keys kept in the catalog but unset. They exclude response-only properties, headers, URL/query parameters, beta-only fields requiring extra opt-in headers, provider-specific extensions, and SDK transport options. For Anthropic, the count includes the current standard schema plus the three legacy sampling fields described below. The model remains in the URL for generateContent and Converse.

## Compatibility details

- **Chat Completions:** some reasoning models require `developer` instead of `system`; change the first message role for those models. `max_tokens` is deprecated and must not be set alongside `max_completion_tokens`. `functions`/`function_call` are legacy alternatives to tools. `stream_options` requires streaming; `top_logprobs` requires `logprobs=true`; audio output requires the corresponding modalities and model support.
- **Responses:** `conversation` and `previous_response_id` are alternative continuation mechanisms. Saved prompts, access programs, moderation and cache settings need valid resources or model/account support. The template does not invent resource IDs. Deprecated fields remain unset in the catalog and are omitted from copied commands.
- **Anthropic:** the command retains the existing `anthropic-version:2023-06-01` header and `x-api-key` authentication for gateway compatibility. The catalog retains current standard optional fields; copied commands omit those needing model/version configuration. `temperature`, `top_p` and `top_k` belong to the legacy version; version **2026-10-21 removes all three**, even when set to default values. Newer models may restrict them on the old version too. Do not enable beta-only features without the relevant header and documentation.
- **Interactions:** use either `model` plus `generation_config` or `agent` plus `agent_config`. Server-side continuation/background workflows require storage; do not enable them while leaving `store=false`. Structured output, webhooks and environments require supported models or real resources. This API uses snake_case.
- **generateContent:** this API uses camelCase. Sampling defaults are model-specific. `cachedContent` and `continuationToken` require actual cache resources or tokens and can constrain the inline instruction/tool fields. `store=null` inherits project logging behavior.
- **Converse:** normal model IDs and inference-profile ARNs use inline system/inference configuration. **Prompt-management ARNs** cannot override `system`, `inferenceConfig`, `toolConfig` or `additionalModelRequestFields`; Core omits those four from copied commands for prompt-resource models. Guardrails and tools require valid configurations. Authentication here is a Bedrock Bearer API key; this does not configure IAM keys or SigV4.

A provider advertising a compatible protocol may implement only part of its official schema. The catalog documents the standard API, not every vendor extension. Adjust concrete example fields and newly enabled options to that provider/model's supported subset. Arca does not infer capabilities from arbitrary model names.
