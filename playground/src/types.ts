export const protocols = {
  "openai-completions": "Chat Completions",
  "openai-responses": "Responses",
  "anthropic-messages": "Anthropic Messages",
  "gemini-interactions": "Gemini Interactions",
  "gemini-generate-content": "Gemini generateContent",
  "aws-bedrock-converse": "Bedrock Converse",
} as const;

export type Protocol = keyof typeof protocols;

export interface ModelTarget {
  id: string;
  name: string;
  model: string;
  protocol: Protocol;
  provider: string;
  origin: string;
  revision: string;
}

export interface ResponseMetrics {
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  reasoning_tokens: number | null;
}

export interface ModelResponse {
  text: string;
  usage: ResponseMetrics;
  finish: "length" | "filtered" | "incomplete" | null;
  upstream_status: number;
  elapsed_ms: number;
}

export interface RunError {
  code: string;
  message: string;
  upstream_status?: number;
  parameter?: string;
}

export type RunResult =
  | { status: "ready" | "running" }
  | { status: "done"; response: ModelResponse }
  | { status: "error"; error: RunError };

export interface Parameters {
  temperature: string;
  top_p: string;
  max_output_tokens: string;
  reasoning_effort: string;
  extra: string;
}

export interface RunDraft {
  id: number;
  profileId: string;
  parameters: Parameters;
  result: RunResult;
}

export const defaultParameters = (): Parameters => ({
  temperature: "",
  top_p: "",
  max_output_tokens: "",
  reasoning_effort: "",
  extra: "",
});
export const letter = (index: number) => String.fromCharCode(65 + index);
