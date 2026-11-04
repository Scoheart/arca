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
  if (/Launcher .* failed with ExitStatus/i.test(msg)) {
    return "指定应用无法打开该文件或目录。";
  }
  return msg;
}
