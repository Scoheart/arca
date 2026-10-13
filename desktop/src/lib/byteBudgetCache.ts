// Presentation data only. Budgets bound retained values, not an OS heap metric.
export class ByteBudgetCache<K, V> {
  private entries = new Map<K, { value: V; bytes: number; expiresAt: number }>();
  private retained = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(private maxEntries: number, private maxBytes: number) {}

  get size() { return this.entries.size; }
  get bytes() { return this.retained; }

  get(key: K): V | undefined {
    const entry = this.entries.get(key);
    if (!entry) return undefined;
    if (entry.expiresAt <= Date.now()) { this.delete(key); return undefined; }
    this.entries.delete(key);
    this.entries.set(key, entry);
    return entry.value;
  }

  set(key: K, value: V, bytes: number, ttl = Infinity) {
    this.delete(key);
    if (!Number.isFinite(bytes) || bytes < 0 || bytes > this.maxBytes || ttl <= 0) return;
    while (this.entries.size >= this.maxEntries || this.retained + bytes > this.maxBytes) {
      const oldest = this.entries.keys().next();
      if (oldest.done) return;
      this.delete(oldest.value);
    }
    this.entries.set(key, { value, bytes, expiresAt: Date.now() + ttl });
    this.retained += bytes;
    this.scheduleExpiry();
  }

  delete(key: K) {
    const entry = this.entries.get(key);
    if (entry) { this.retained -= entry.bytes; this.entries.delete(key); }
    if (!this.entries.size && this.timer) { clearTimeout(this.timer); this.timer = null; }
  }

  clear() {
    this.entries.clear(); this.retained = 0;
    if (this.timer) { clearTimeout(this.timer); this.timer = null; }
  }

  private scheduleExpiry() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    const next = Math.min(...[...this.entries.values()].map(entry => entry.expiresAt));
    if (!Number.isFinite(next)) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      for (const [key, entry] of this.entries) if (entry.expiresAt <= Date.now()) this.delete(key);
      this.scheduleExpiry();
    }, Math.max(0, next - Date.now()));
  }
}

// Conservative accounting for JSON-shaped IPC values without allocating a
// second serialized multi-megabyte body just to decide whether to cache it.
export function jsonMemoryWeight(value: unknown, limit: number): number {
  let bytes = 0;
  const visit = (item: unknown) => {
    if (bytes > limit || item == null) return;
    if (typeof item === 'string') { bytes += 24 + item.length * 2; return; }
    if (typeof item !== 'object') { bytes += 16; return; }
    if (Array.isArray(item)) {
      bytes += 32 + item.length * 8;
      for (const child of item) { visit(child); if (bytes > limit) break; }
    } else {
      const keys = Object.keys(item);
      bytes += 32 + keys.length * 48;
      for (const key of keys) {
        bytes += key.length * 2;
        visit((item as Record<string, unknown>)[key]);
        if (bytes > limit) break;
      }
    }
  };
  visit(value);
  return bytes;
}
