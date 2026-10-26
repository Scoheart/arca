export const SIDEBAR_WIDTH_STORAGE_KEY = "arca.resourceWorkspace.sidebarWidth";
export const DEFAULT_SIDEBAR_WIDTH = 224;
export const MIN_SIDEBAR_WIDTH = 184;
export const MAX_SIDEBAR_WIDTH = 340;
export const REDACTED_VALUE = "••••••••";

const SENSITIVE_KEY = /(authorization|cookie|token|secret|password|api[_-]?key|access[_-]?key|private[_-]?key|credential)/i;

export function clampSidebarWidth(value: number): number {
  return Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, value));
}

export function parseSidebarWidth(value: string | null): number {
  if (value === null || value.trim() === "") return DEFAULT_SIDEBAR_WIDTH;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < MIN_SIDEBAR_WIDTH || parsed > MAX_SIDEBAR_WIDTH) {
    return DEFAULT_SIDEBAR_WIDTH;
  }
  return parsed;
}

export function redactSensitiveConfig(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(redactSensitiveConfig);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value).map(([key, child]) => [
      key,
      SENSITIVE_KEY.test(key) ? REDACTED_VALUE : redactSensitiveConfig(child),
    ])
  );
}
