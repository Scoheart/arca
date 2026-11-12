import { isSensitiveConfigKey, REDACTED_VALUE } from "./resourceWorkspace";

/** Flatten a Tauri command error (string | string[] | unknown) for display. */
export function formatError(e: unknown): string {
  if (Array.isArray(e)) return e.map(formatError).join("; ");
  if (e instanceof Error) return cleanErrorMessage(e.message);
  if (typeof e === "string") return cleanErrorMessage(e);
  if (e && typeof e === "object") {
    const record = e as Record<string, unknown>;
    for (const key of ["message", "error", "detail"]) {
      if (key in record) return formatError(record[key]);
    }
    try {
      return JSON.stringify(e);
    } catch {
      // Fall through for non-serializable host objects.
    }
  }
  return cleanErrorMessage(String(e));
}

function cleanErrorMessage(msg: string): string {
  if (/Launcher [\s\S]* failed with ExitStatus/i.test(msg)) {
    return "指定应用无法打开该文件或目录。";
  }
  return msg;
}

/** Keep command codes and context available without copying credentials. */
export function formatErrorDetails(error: unknown): string {
  if (error instanceof Error) return redactErrorText(error.toString());
  if (error && typeof error === "object") {
    try {
      const json = JSON.stringify(error,
        (key, value) => {
          if (key === "findings_hash" || isSensitiveConfigKey(key)) return REDACTED_VALUE;
          if (value instanceof Error) return redactErrorText(value.toString());
          return typeof value === "string" ? redactErrorText(value) : value;
        }, 2);
      if (json && json !== "{}") return json;
    } catch {
      // Some host objects cannot be serialized.
    }
  }
  return redactErrorText(String(error));
}

function redactErrorText(text: string): string {
  const mask = (_match: string, prefix: string, value: string) => {
    const quote = value[0] === '"' || value[0] === "'" ? value[0] : "";
    return prefix + quote + REDACTED_VALUE + quote;
  };
  return text
    .replace(/(\b(?:https?|socks5h?):\/\/)[^\s/@]+@/gi, "$1")
    .replace(/(\b(?:proxy-authorization|authorization|set-cookie|cookie)["']?\s*[:=][ \t]*)("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\r\n"]+)/gi, mask)
    .replace(/(\b[\w.-]*(?:password|secret|token|api[ _-]?key|access[ _-]?key|private[ _-]?key|credential)[\w.-]*["']?\s*[:=][ \t]*)("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^\s&,;}\]]+)/gi, mask)
    .replace(/(\b(?:bearer|basic)\s+)[a-z0-9_.+\/~=-]{8,}/gi, "$1" + REDACTED_VALUE)
    .replace(/\b(?:sk-[a-z0-9_-]{12,}|gh[pousr]_[a-z0-9_]{16,}|github_pat_[a-z0-9_]{16,})\b/gi, REDACTED_VALUE);
}
