// Optional window preferences must not break resource loading or update checks.
// A failed write remains available for this session; business state stays in core.
const sessionValues = new Map<string, string | null>();

export function readLocalSetting(key: string): string | null {
  if (sessionValues.has(key)) return sessionValues.get(key) ?? null;
  try {
    const saved = window.localStorage.getItem(key);
    if (saved !== null || !key.startsWith("arca")) return saved;
    const legacy = window.localStorage.getItem(`mux${key.slice(4)}`);
    if (legacy !== null) window.localStorage.setItem(key, legacy);
    return legacy;
  } catch { return null; }
}

export function writeLocalSetting(key: string, value: string | null): void {
  try {
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, value);
    sessionValues.delete(key);
  } catch { sessionValues.set(key, value); }
}
