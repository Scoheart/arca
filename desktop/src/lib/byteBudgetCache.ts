import { LRUCache } from "lru-cache";

// Presentation data only. Budgets bound retained values, not an OS heap metric.
export class ByteBudgetCache<K extends string, V extends object> {
  private readonly entries: LRUCache<K, V>;

  constructor(maxEntries: number, maxBytes: number) {
    this.entries = new LRUCache({
      max: maxEntries,
      maxSize: maxBytes,
      ttlAutopurge: true,
      ttlResolution: 0,
    });
  }

  get size() { return this.entries.size; }
  get bytes() { return this.entries.calculatedSize; }

  get(key: K): V | undefined {
    return this.entries.get(key);
  }

  set(key: K, value: V, bytes: number, ttl = Infinity) {
    // A resolved catalog can update the weight of the same cached object.
    this.delete(key);
    if (!Number.isFinite(bytes) || bytes < 0 || ttl <= 0) return;
    this.entries.set(key, value, {
      size: Math.max(1, Math.ceil(bytes)),
      ttl: Number.isFinite(ttl) ? ttl : 0,
    });
  }

  delete(key: K) {
    this.entries.delete(key);
  }

  clear() {
    this.entries.clear();
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
