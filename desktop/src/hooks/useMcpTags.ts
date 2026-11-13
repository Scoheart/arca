import { useCallback, useEffect, useState } from "react";
import {
  listMcpTags,
  setMcpTags,
  setMcpTagsBatch,
} from "../lib/api";
import type { McpTagPreferences } from "../lib/types";
import { formatError } from "../lib/format";

export function useMcpTags(enabled = true) {
  const [tagsByKey, setTagsByKey] = useState<McpTagPreferences>({});
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (typeof listMcpTags !== "function") return {};
    try {
      const next = await listMcpTags();
      setTagsByKey(next);
      setError(null);
      return next;
    } catch (cause) {
      setError(formatError(cause));
      return {};
    }
  }, []);

  useEffect(() => {
    if (enabled) void refresh();
  }, [enabled, refresh]);

  const mutate = useCallback(async (
    key: string,
    operation: () => Promise<McpTagPreferences>,
  ) => {
    setBusyKey(key);
    setError(null);
    try {
      const next = await operation();
      setTagsByKey(next);
      return next;
    } catch (cause) {
      const message = formatError(cause);
      setError(message);
      throw new Error(message);
    } finally {
      setBusyKey(null);
    }
  }, []);

  return {
    tagsByKey,
    busyKey,
    error,
    refresh,
    setTags: (assetKey: string, tags: string[]) =>
      mutate(assetKey, () => setMcpTags(assetKey, tags)),
    setTagsBatch: (updates: McpTagPreferences) =>
      mutate("batch", () => setMcpTagsBatch(updates)),
  };
}
