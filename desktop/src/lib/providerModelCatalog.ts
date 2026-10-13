import { discoverProviderModels } from "./api";
import type { ModelProviderInstanceView, ProviderModelSummary } from "./types";
import { ByteBudgetCache, jsonMemoryWeight } from "./byteBudgetCache";

// A presentation cache only: discovered models are never written to the library.
const MAX_AGE_MS = 5 * 60_000;
const MAX_CATALOG_BYTES = 4 * 1024 * 1024;
const catalogs = new ByteBudgetCache<string, {
  key: string;
  at: number;
  request: Promise<ProviderModelSummary[]>;
}>(8, MAX_CATALOG_BYTES);

export function providerCatalogKey(provider: ModelProviderInstanceView, revision: number): string {
  return JSON.stringify([
    provider.id, provider.provider, provider.base_url, provider.model_catalog_url,
    provider.protocols, provider.auth_requirement, provider.api_key_source,
    provider.credential_saved, revision,
  ]);
}

export function loadProviderModelCatalog(
  provider: ModelProviderInstanceView,
  revision: number,
  force = false,
): Promise<ProviderModelSummary[]> {
  const key = providerCatalogKey(provider, revision);
  const cached = catalogs.get(provider.id);
  if (!force && cached?.key === key && Date.now() - cached.at < MAX_AGE_MS) return cached.request;
  const request = discoverProviderModels(provider.id).then(models => {
    const current = catalogs.get(provider.id);
    if (current?.request === request) {
      catalogs.set(provider.id, current, jsonMemoryWeight(models, MAX_CATALOG_BYTES) + key.length * 2,
        Math.max(0, MAX_AGE_MS - (Date.now() - current.at)));
    }
    return models;
  }).catch((error: unknown) => {
    if (catalogs.get(provider.id)?.request === request) catalogs.delete(provider.id);
    throw error;
  });
  catalogs.set(provider.id, { key, at: Date.now(), request }, 256 + key.length * 2, MAX_AGE_MS);
  return request;
}
