import type { ModelResponse, ModelTarget, RunDraft, RunError } from "./types";

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

export interface SharedInput {
  prompt: string;
  instructions: string;
}

async function request<T>(token: string, path: string, data?: object): Promise<T> {
  const headers: Record<string, string> = { "X-Arca-Playground": token, "Accept": "application/json" };
  if (data) headers["Content-Type"] = "application/json";
  const response = await fetch(path, {
    method: data ? "POST" : "GET",
    headers,
    cache: "no-store",
    body: data ? JSON.stringify(data) : undefined,
  }).catch(() => invalid("local_connection", "无法连接 Arca，请确认试验台仍在运行。"));
  const output = await response.json().catch(() => invalid("invalid_local_response", "无法读取 Arca 的响应。"));
  if (!response.ok) throw describeError(output.error);
  return output as T;
}

export const loadModels = (token: string) => request<ModelTarget[]>(token, "/api/models");

export function runModel(token: string, draft: RunDraft, model: ModelTarget, shared: SharedInput): Promise<ModelResponse> {
  const p = draft.parameters;
  return request<ModelResponse>(token, "/api/run", {
    ...shared,
    profile_id: model.id,
    revision: model.revision,
    temperature: supportsSampling(model) ? number(p.temperature) : null,
    top_p: supportsSampling(model) ? number(p.top_p) : null,
    max_output_tokens: number(p.max_output_tokens),
    reasoning_effort: supportsEffort(model) ? p.reasoning_effort || null : null,
    extra: advanced(p.extra),
  });
}
