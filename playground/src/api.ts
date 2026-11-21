import type { ModelResponse, ModelTarget, Parameters, RunError, SharedInput } from "./types";

export function sessionToken(): string | null {
  const matches = (value: string | null): value is string => /^[0-9a-f]{32}$/.test(value ?? "");
  const key = "arca-playground";
  const fragment = window.location.hash.slice(1);
  if (matches(fragment)) {
    try { sessionStorage.setItem(key, fragment); } catch { /* Browser storage may be disabled. */ }
    window.history.replaceState(null, "", "/");
    return fragment;
  }
  try {
    const stored = sessionStorage.getItem(key);
    return matches(stored) ? stored : null;
  } catch {
    return null;
  }
}

export function describeError(error: unknown): RunError {
  if (error && typeof error === "object" && "code" in error && "message" in error
    && typeof error.code === "string" && typeof error.message === "string") {
    return error as RunError;
  }
  return { code: "request_failed", message: "无法完成请求，请检查试验台是否仍在运行。" };
}

function invalid(code: string, message: string): never {
  throw { code, message } satisfies RunError;
}

export function supportsEffort(model?: ModelTarget): boolean {
  return model?.protocol === "openai-responses" || model?.protocol === "openai-completions";
}

export function supportsSampling(model?: ModelTarget): boolean {
  return !!model && model.protocol !== "gemini-interactions";
}

function number(text: string): number | null {
  if (!text.trim()) return null;
  const value = Number(text);
  if (!Number.isFinite(value)) invalid("invalid_number", "请检查数字参数。");
  return value;
}

function advanced(text: string): Record<string, unknown> {
  if (!text.trim()) return {};
  let parsed: unknown;
  try { parsed = JSON.parse(text); } catch { invalid("invalid_json", "高级 JSON 必须是一个合法的 JSON 对象。"); }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    invalid("invalid_json", "高级 JSON 必须是一个合法的 JSON 对象。");
  }
  return parsed as Record<string, unknown>;
}

export interface RequestPreviewData {
  method: string;
  url: string;
  accept: string;
  authentication_header: string | null;
  body: Record<string, unknown>;
}

function request(token: string, path: string, data?: object, signal?: AbortSignal, accept = "application/json") {
  const headers: Record<string, string> = { "X-Arca-Playground": token, "Accept": accept };
  if (data) headers["Content-Type"] = "application/json";
  return fetch(path, {
    method: data ? "POST" : "GET",
    headers,
    cache: "no-store",
    signal,
    body: data ? JSON.stringify(data) : undefined,
  }).catch((error: unknown) => {
    if (signal?.aborted) throw error;
    return invalid("local_connection", "无法连接 Arca，请确认试验台仍在运行。");
  });
}

async function json<T>(response: Response): Promise<T> {
  const output = await response.json().catch(() => invalid("invalid_local_response", "无法读取 Arca 的响应。"));
  if (!response.ok) throw describeError(output.error);
  return output as T;
}

export const loadModels = (token: string) => request(token, "/api/models").then(json<ModelTarget[]>);

function payload(p: Parameters, model: ModelTarget, shared: SharedInput) {
  return {
    prompt: shared.prompt,
    instructions: shared.instructions,
    profile_id: model.id,
    revision: model.revision,
    temperature: supportsSampling(model) ? number(p.temperature) : null,
    top_p: supportsSampling(model) ? number(p.top_p) : null,
    max_output_tokens: number(p.max_output_tokens),
    reasoning_effort: supportsEffort(model) ? p.reasoning_effort || null : null,
    streaming: p.streaming && model.streaming_available,
    extra: advanced(p.extra),
  };
}

export async function previewRequest(
  token: string, parameters: Parameters, model: ModelTarget, shared: SharedInput, signal: AbortSignal,
): Promise<RequestPreviewData> {
  return json(await request(token, "/api/preview", payload(parameters, model, shared), signal));
}

type Frame =
  | { type: "started" }
  | { type: "delta"; text: string }
  | { type: "complete"; response: ModelResponse }
  | { type: "error"; error: RunError };

const MAX_FRAME_LENGTH = 16 * 1024 * 1024;

function frame(line: string): Frame {
  let parsed: unknown;
  try { parsed = JSON.parse(line); } catch { invalid("invalid_local_response", "无法解析 Arca 的流式响应。"); }
  if (parsed && typeof parsed === "object" && "type" in parsed) {
    if (parsed.type === "started") return { type: "started" };
    if (parsed.type === "delta" && "text" in parsed && typeof parsed.text === "string") {
      return { type: "delta", text: parsed.text };
    }
    if (parsed.type === "error" && "error" in parsed) return { type: "error", error: describeError(parsed.error) };
    if (parsed.type === "complete" && "response" in parsed) return parsed as Frame;
  }
  return invalid("invalid_local_response", "Arca 返回了未知的响应类型。");
}

export async function runModel(
  token: string, parameters: Parameters, model: ModelTarget, shared: SharedInput,
  signal: AbortSignal, onDelta: (text: string) => void,
): Promise<ModelResponse> {
  const response = await request(token, "/api/run/stream", payload(parameters, model, shared), signal, "application/x-ndjson");
  if (!response.ok || !response.headers.get("Content-Type")?.includes("application/x-ndjson")) {
    return json<ModelResponse>(response);
  }
  if (!response.body) return invalid("invalid_local_response", "浏览器无法读取 Arca 的回答。");
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let pending = "";
  try {
    while (true) {
      const { value, done } = await reader.read();
      pending += decoder.decode(value, { stream: !done });
      let start = 0;
      let end = pending.indexOf("\n");
      while (end !== -1) {
        if (end - start > MAX_FRAME_LENGTH) invalid("invalid_local_response", "Arca 的单条响应太长。");
        const line = pending.slice(start, end).trim();
        if (line) {
          const event = frame(line);
          if (event.type === "delta") onDelta(event.text);
          if (event.type === "complete") return event.response;
          if (event.type === "error") throw event.error;
        }
        start = end + 1;
        end = pending.indexOf("\n", start);
      }
      pending = pending.slice(start);
      if (pending.length > MAX_FRAME_LENGTH) invalid("invalid_local_response", "Arca 的单条响应太长。");
      if (done) return invalid("local_stream_interrupted", "试验台在回答结束前断开了连接，可以单独重试。");
    }
  } finally {
    void reader.cancel().catch(() => {});
  }
}
