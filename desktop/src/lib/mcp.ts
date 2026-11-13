import type { RegistryEntry, InstalledMcp } from "./types";

export type Transport = "stdio" | "http";

export const MAX_MCP_TAGS_PER_ASSET = 20;
export const MAX_MCP_TAG_LEN = 48;

export function normalizeMcpTags(tags: Iterable<string>): string[] {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const raw of tags) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    if ([...trimmed].length > MAX_MCP_TAG_LEN) continue;
    if (/[\u0000-\u001f\u007f]/.test(trimmed)) continue;
    const key = trimmed.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(trimmed);
    if (result.length >= MAX_MCP_TAGS_PER_ASSET) break;
  }
  return result;
}

export function resolveMcpTags(
  entry: RegistryEntry,
  tagPreferences?: Record<string, string[]>,
): string[] {
  if (tagPreferences) {
    const key = keyOf(entry);
    if (Object.prototype.hasOwnProperty.call(tagPreferences, key)) {
      return normalizeMcpTags(tagPreferences[key] ?? []);
    }
  }
  return normalizeMcpTags(entry.tags ?? []);
}

/** Transport bucket of an entry: "stdio" (local process) or "http" (remote,
 *  covers http+sse). An entry carries exactly one transport; stdio wins if both. */
export function transportOf(entry: RegistryEntry): Transport {
  return entry.config.stdio ? "stdio" : "http";
}

/** Composite identity `name::transport`. Two entries with the same name but
 *  different transports (e.g. figma stdio vs http) are distinct catalog items. */
export function keyOf(entry: RegistryEntry): string {
  return `${entry.name}::${transportOf(entry)}`;
}

/** Composite key of an installed (scanned) server. */
export function installedKey(item: InstalledMcp): string {
  return `${item.name}::${item.transport}`;
}

/** Precise transport label for display: stdio → "stdio"; http → the http type
 *  ("http" or "sse"). */
export function transportLabel(entry: RegistryEntry): string {
  if (entry.config.stdio) return "stdio";
  return entry.config.http?.type ?? "http";
}
