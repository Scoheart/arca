import { listModelProfiles, listModelProviderInstances, listModelProviders } from "./api";
import { modelObservationRevision } from "./modelObservation";
import { ByteBudgetCache, jsonMemoryWeight } from "./byteBudgetCache";
import type { ModelProfileView, ModelProviderInstanceView, ModelProviderView } from "./types";

interface ModelLibrary {
  profiles: ModelProfileView[];
  providers: ModelProviderView[];
  instances: ModelProviderInstanceView[];
}

// Presentation-only cache. No credential values; mutations and cURL copies
// always call core. Watcher/rescan invalidation takes effect immediately.
const MAX_AGE_MS = 30_000;
const MAX_BYTES = 4 * 1024 * 1024;
const cached = new ByteBudgetCache<string, { revision: number; value: ModelLibrary }>(1, MAX_BYTES);
let pending: { revision: number; request: Promise<ModelLibrary> } | null = null;

export function cachedModelLibrary(): ModelLibrary | null {
  const entry = cached.get("library");
  if (entry?.revision === modelObservationRevision()) return entry.value;
  cached.delete("library");
  return null;
}

export function loadModelLibrary(force = false): Promise<ModelLibrary> {
  if (force) cached.clear();
  const revision = modelObservationRevision();
  const value = !force && cachedModelLibrary();
  if (value) return Promise.resolve(value);
  if (!force && pending?.revision === revision) return pending.request;
  const request = Promise.all([listModelProfiles(), listModelProviders(), listModelProviderInstances()])
    .then(([profiles, providers, instances]) => {
      const value = { profiles, providers, instances };
      if (pending?.request === request && revision === modelObservationRevision()) {
        cached.set("library", { revision, value }, jsonMemoryWeight(value, MAX_BYTES), MAX_AGE_MS);
      }
      return value;
    }).finally(() => {
      if (pending?.request === request) pending = null;
    });
  pending = { revision, request };
  return request;
}
