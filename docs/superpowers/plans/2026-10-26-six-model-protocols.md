# Six model protocols

**Goal:** Add Gemini Interactions and AWS Bedrock Converse to the four existing model protocols, using the same central Provider/Profile workflows and native Agent capability boundaries.

**Architecture:** One shared protocol metadata file supplies labels and endpoint paths to Core and Desktop. Rust retains typed protocol values, constructs cURL requests, validates Providers, and determines native Agent support. Bedrock uses its official Bearer API key contract through existing non-secret credential sources and Keychain storage; this change does not administer AWS accounts or IAM credentials.

**Implementation:** Native changes were developed in the existing checkout, preserving all pre-existing edits. Initial delivery completed local compilation and the actual-data Arca Dev preview. The user subsequently authorized publication on 2026-10-27; only this feature is ported onto the latest main in an isolated local checkout for commit, push and Direct Stable publication.

## Constraints

- Protocols: `openai-completions`, `openai-responses`, `anthropic-messages`, `gemini-interactions`, `gemini-generate-content`, `aws-bedrock-converse`.
- Interactions: `/interactions`, `model` + `input`, `x-goog-api-key`; cURL examples use `store=false`.
- Converse: `/model/{model}/converse`, content-block `messages`, model only in the escaped URL path, Bearer API key.
- Google keeps both native protocols; new Google connections default to Interactions. Bedrock Converse is a separate regional template from Bedrock Mantle.
- New protocols must never pass existing permissive Agent checks accidentally. Bedrock uses verified native OpenCode/Kilo SDK mapping. Interactions is assignable only when an exact native factory/entrypoint is verified; central management and cURL export remain available independently.
- Bedrock native model discovery stays explicitly unavailable; users enter model/inference-profile IDs. Gemini Interactions shares Google's reviewed Models endpoint.
- Unknown fields, comments, credential delivery and safe writes retain their current boundaries.
- Project fast mode: do not run cargo/npm tests, formatters, clippy, icon checks, validators or preflight. Add focused regression coverage for future explicit runs; use production builds and development launch now.

## Work

- [x] 1. Add typed protocols/shared metadata and regression cases for serialization, request path/body/header construction and hostile model/path inputs.
- [x] 2. Add Provider templates/discovery boundaries, explicit Agent support, native package mapping and external-model observation round trips.
- [x] 3. Update Desktop's protocol selector/type contracts and user documentation from the shared metadata.
- [x] 4. Compile CLI and Desktop, repair compiler failures in the changed surface, launch `npm run tauri dev` with actual HOME/ARCA_HOME, and obtain one focused final review.

## Validation — 2026-10-27

- `cargo build -p arca-cli`: passed, no warnings.
- `npm run build`: passed (TypeScript and Vite).
- `npm run tauri dev`: rebuilt the native app successfully and launched Arca Dev with inherited real HOME, without HOME/ARCA_HOME overrides. The installed Arca app was not replaced.
- Focused final source review: no remaining Critical or Important findings. Google Provider/Profile default selection now follows the Core template, and explicit choices remain intact.
- Added focused regression cases without running test suites, as required by project fast mode. Native Interactions assignment remains explicitly unavailable for current stable Agents; six-protocol management and cURL export are available.
- Release port onto v2.10.0 main: installed the committed Desktop dependency lock with `npm ci`; CLI and Desktop rebuilt successfully. Reused the original actual-data preview, and reviewed the port separately. Agent-page protocol labels now also consume the shared definitions.

## References

- https://ai.google.dev/gemini-api/docs/interactions-overview
- https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html
- https://docs.aws.amazon.com/bedrock/latest/userguide/api-keys-use.html
- https://ai-sdk.dev/providers/ai-sdk-providers/amazon-bedrock
- https://ai-sdk.dev/providers/ai-sdk-providers/google
