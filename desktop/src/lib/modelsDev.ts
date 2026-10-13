import type { ModelProfileView } from "./types";

export const MODELS_DEV_API_URL = "https://models.dev/api.json";
export const MODELS_DEV_CACHE_TTL_MS = 24 * 60 * 60 * 1000;

const CACHE_KEY = "arca.models-dev.metadata.v1";
const REQUEST_TIMEOUT_MS = 8_000;
const CATALOG_MEMORY_TTL_MS = 5 * 60 * 1000;
const catalogRequests = new WeakMap<typeof fetch, { expiresAt: number; request: Promise<{ catalog: unknown; fetchedAt: number }> }>();

function loadCatalog(fetchImpl: typeof fetch, now: () => number) {
  const previous = catalogRequests.get(fetchImpl);
  if (previous && now() < previous.expiresAt) return previous.request;
  const controller = new AbortController();
  const timeout = globalThis.setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  const request = (async () => {
    const response = await fetchImpl(MODELS_DEV_API_URL, {
      headers: { Accept: "application/json" }, signal: controller.signal,
    });
    if (!response.ok) throw new Error(`models.dev returned ${response.status}`);
    return { catalog: await response.json() as unknown, fetchedAt: now() };
  })().catch((error: unknown) => {
    if (catalogRequests.get(fetchImpl)?.request === request) catalogRequests.delete(fetchImpl);
    throw error;
  }).finally(() => globalThis.clearTimeout(timeout));
  const entry = { expiresAt: now() + CATALOG_MEMORY_TTL_MS, request };
  catalogRequests.set(fetchImpl, entry);
  // Bound memory even if no subsequent request arrives to evict the catalog.
  globalThis.setTimeout(() => {
    if (catalogRequests.get(fetchImpl) === entry) catalogRequests.delete(fetchImpl);
  }, CATALOG_MEMORY_TTL_MS);
  return request;
}

export interface ModelsDevMetadata {
  name?: string;
  description?: string;
  reasoning?: boolean;
  toolCall?: boolean;
  structuredOutput?: boolean;
  modalities?: string[];
  releaseDate?: string;
  contextWindow?: number;
  maxOutputTokens?: number;
  inputCost?: number;
  outputCost?: number;
}

interface ModelsDevCache {
  version: 1;
  fetchedAt: number;
  entries: Record<string, ModelsDevMetadata | null>;
}

interface LoadOptions {
  fetchImpl?: typeof fetch;
  storage?: Pick<Storage, "getItem" | "setItem"> | null;
  now?: () => number;
}

type ProfileMetadataMap = Record<string, ModelsDevMetadata>;

function browserStorage(): Pick<Storage, "getItem" | "setItem"> | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

function normalizedProvider(profile: ModelProfileView) {
  try {
    if (new URL(profile.base_url).hostname.toLocaleLowerCase() === "openrouter.ai") {
      return "openrouter";
    }
  } catch {
    // Invalid or incomplete user URLs simply cannot contribute to catalog matching.
  }
  const provider = profile.provider.trim().toLocaleLowerCase();
  return provider === "custom" ? "" : provider;
}

function profileCacheKey(profile: ModelProfileView) {
  return `${normalizedProvider(profile)}::${profile.model.trim().toLocaleLowerCase()}`;
}

function readCache(storage: Pick<Storage, "getItem"> | null): ModelsDevCache | null {
  if (!storage) return null;
  try {
    const parsed = JSON.parse(storage.getItem(CACHE_KEY) ?? "null") as Partial<ModelsDevCache> | null;
    if (
      parsed?.version !== 1
      || typeof parsed.fetchedAt !== "number"
      || !parsed.entries
      || typeof parsed.entries !== "object"
    ) {
      return null;
    }
    return parsed as ModelsDevCache;
  } catch {
    return null;
  }
}

function mapCachedProfiles(profiles: ModelProfileView[], cache: ModelsDevCache | null) {
  const result: ProfileMetadataMap = {};
  if (!cache) return result;
  for (const profile of profiles) {
    const metadata = cache.entries[profileCacheKey(profile)];
    if (metadata) result[profile.id] = metadata;
  }
  return result;
}

export function getCachedModelsDevMetadata(
  profiles: ModelProfileView[],
  storage: Pick<Storage, "getItem"> | null = browserStorage(),
) {
  return mapCachedProfiles(profiles, readCache(storage));
}

function modelCandidates(profile: ModelProfileView, provider: string) {
  const model = profile.model.trim();
  const catalogKey = profile.catalog_key.trim();
  const candidates = new Set([model, catalogKey]);
  for (const prefix of [`${provider}/`, "openrouter/"]) {
    if (model.toLocaleLowerCase().startsWith(prefix)) candidates.add(model.slice(prefix.length));
    if (catalogKey.toLocaleLowerCase().startsWith(prefix)) {
      candidates.add(catalogKey.slice(prefix.length));
    }
  }
  return [...candidates]
    .map((candidate) => candidate.replace(/^\/+/, "").toLocaleLowerCase())
    .filter(Boolean);
}

function finiteNumber(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function optionalString(value: unknown) {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function compactMetadata(value: unknown): ModelsDevMetadata | null {
  if (!value || typeof value !== "object") return null;
  const model = value as Record<string, unknown>;
  const limit = model.limit && typeof model.limit === "object"
    ? model.limit as Record<string, unknown>
    : {};
  const cost = model.cost && typeof model.cost === "object"
    ? model.cost as Record<string, unknown>
    : {};
  const modalitiesValue = model.modalities && typeof model.modalities === "object"
    ? (model.modalities as Record<string, unknown>).input
    : undefined;
  const modalities = Array.isArray(modalitiesValue)
    ? modalitiesValue.filter((item): item is string => typeof item === "string")
    : undefined;
  return {
    name: optionalString(model.name),
    description: optionalString(model.description),
    reasoning: typeof model.reasoning === "boolean" ? model.reasoning : undefined,
    toolCall: typeof model.tool_call === "boolean" ? model.tool_call : undefined,
    structuredOutput: typeof model.structured_output === "boolean"
      ? model.structured_output
      : undefined,
    modalities: modalities?.length ? modalities : undefined,
    releaseDate: optionalString(model.release_date),
    contextWindow: finiteNumber(limit.context),
    maxOutputTokens: finiteNumber(limit.output),
    inputCost: finiteNumber(cost.input),
    outputCost: finiteNumber(cost.output),
  };
}

function buildProviderIndex(catalog: unknown, provider: string) {
  if (!provider || !catalog || typeof catalog !== "object") return null;
  const providerValue = (catalog as Record<string, unknown>)[provider];
  if (!providerValue || typeof providerValue !== "object") return null;
  const modelsValue = (providerValue as Record<string, unknown>).models;
  if (!modelsValue || typeof modelsValue !== "object") return null;
  const models = modelsValue as Record<string, unknown>;
  const index = new Map<string, unknown>();
  for (const [key, value] of Object.entries(models)) {
    index.set(key.toLocaleLowerCase(), value);
    if (value && typeof value === "object") {
      const id = optionalString((value as Record<string, unknown>).id);
      if (id) index.set(id.toLocaleLowerCase(), value);
    }
  }
  return index;
}

function matchProfile(
  index: Map<string, unknown> | null,
  profile: ModelProfileView,
  provider: string,
) {
  if (!index) return null;
  for (const candidate of modelCandidates(profile, provider)) {
    const metadata = compactMetadata(index.get(candidate));
    if (metadata) return metadata;
  }
  return null;
}

export async function loadModelsDevMetadata(
  profiles: ModelProfileView[],
  options: LoadOptions = {},
): Promise<ProfileMetadataMap> {
  if (profiles.length === 0) return {};
  const storage = options.storage === undefined ? browserStorage() : options.storage;
  const now = options.now ?? Date.now;
  const cache = readCache(storage);
  const keys = profiles.map(profileCacheKey);
  if (
    cache
    && now() - cache.fetchedAt < MODELS_DEV_CACHE_TTL_MS
    && keys.every((key) => Object.prototype.hasOwnProperty.call(cache.entries, key))
  ) {
    return mapCachedProfiles(profiles, cache);
  }

  const fetchImpl = options.fetchImpl ?? globalThis.fetch;
  if (!fetchImpl) return mapCachedProfiles(profiles, cache);
  try {
    const { catalog, fetchedAt } = await loadCatalog(fetchImpl, now);
    const providerIndexes = new Map<string, Map<string, unknown> | null>();
    for (const profile of profiles) {
      const provider = normalizedProvider(profile);
      if (!providerIndexes.has(provider)) {
        providerIndexes.set(provider, buildProviderIndex(catalog, provider));
      }
    }
    const entries = Object.fromEntries(
      profiles.map((profile) => {
        const provider = normalizedProvider(profile);
        return [
          profileCacheKey(profile),
          matchProfile(providerIndexes.get(provider) ?? null, profile, provider),
        ];
      }),
    );
    // Another caller may have saved a different profile subset while this one
    // awaited the same download. Merge only metadata from the same generation.
    const latest = readCache(storage);
    const nextCache: ModelsDevCache = { version: 1, fetchedAt, entries: {
      ...(latest?.fetchedAt === fetchedAt ? latest.entries : {}), ...entries,
    } };
    try {
      storage?.setItem(CACHE_KEY, JSON.stringify(nextCache));
    } catch {
      // Cache quotas and privacy settings must never break the Models view.
    }
    return mapCachedProfiles(profiles, nextCache);
  } catch {
    return mapCachedProfiles(profiles, cache);
  }
}
