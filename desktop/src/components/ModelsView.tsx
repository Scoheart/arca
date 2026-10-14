import { ResourceIcon } from "./resourcePresentation";
import { memo, type ReactNode, useCallback, useDeferredValue, useEffect, useId, useMemo, useRef, useState } from "react";
import { useModelObservationRevision } from "../lib/modelObservation";
import { cachedModelLibrary, loadModelLibrary } from "../lib/modelLibrary";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  copyToClipboard,
  exportModelCurl,
  revealModelProviderCredential,
} from "../lib/api";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type {
  ApiKeySource,
  CentralAssetDraft,
  ModelProfile,
  ModelProfileView,
  ModelProviderConfig,
  ModelProviderInstanceView,
  ModelProviderView,
  ModelProtocol,
  ProviderModelSummary,
  ResourceNavigationIntent,
} from "../lib/types";
import { formatError } from "../lib/format";
import { requiresAgentReview } from "../lib/agentOperation";
import { ManualModelCard, ProviderModelCatalog } from "./ProviderModelCatalog";
import {
  getCachedModelsDevMetadata,
  loadModelsDevMetadata,
  type ModelsDevMetadata,
} from "../lib/modelsDev";
import { Avatar, Badge } from "./ui";
import { ResourceState } from "./ResourceState";
import { DialogShell } from "./DialogShell";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";
import { FormSelect } from "./FormSelect";
import { ProviderGlyph } from "./providerIcons";
import {
  CalendarIcon,
  ChevronDownIcon,
  CopyIcon,
  EditIcon,
  ExternalLinkIcon,
  EyeIcon,
  EyeOffIcon,
  GaugeIcon,
  KeyIcon,
  LayersIcon,
  LinkIcon,
  NetworkIcon,
  PlusIcon,
  RefreshIcon,
  SearchIcon,
  SparklesIcon,
  TerminalIcon,
  TrashIcon,
} from "./icons";
import { ToastNotice, useToast } from "./Toast";
import {
  InspectorField,
  InspectorMetric,
  InspectorMetrics,
  ResourceInspector,
  ResourceOverview,
  ResourceWorkspace,
  SidebarItem,
  SidebarSection,
  WorkspaceSidebar,
} from "./ResourceWorkspace";

const PROTOCOLS: Array<{ id: ModelProtocol; label: string }> = [
  { id: "anthropic-messages", label: "Anthropic Messages" },
  { id: "openai-responses", label: "OpenAI Responses" },
  { id: "openai-completions", label: "OpenAI Chat Completions" },
  { id: "gemini-generate-content", label: "Gemini GenerateContent" },
];

const DEFAULT_ENDPOINT_PATHS: Record<ModelProtocol, string> = {
  "anthropic-messages": "/v1/messages",
  "openai-responses": "/responses",
  "openai-completions": "/chat/completions",
  "gemini-generate-content": "/models/{model}:generateContent",
};

const emptyProfile = (): ModelProfile => ({
  id: "",
  name: "",
  provider: "",
  protocol: "openai-responses",
  base_url: "",
  model: "",
});

function protocolLabel(protocol: ModelProtocol) {
  return PROTOCOLS.find((item) => item.id === protocol)?.label ?? protocol;
}

function providerLabel(providers: ModelProviderView[], provider: string) {
  return providers.find((item) => item.id === provider)?.name ?? (provider || "Custom Provider");
}

function isCustomProviderInstance(
  instance: ModelProviderInstanceView,
  templates: ModelProviderView[],
) {
  return instance.provider === "custom"
    || templates.find((template) => template.id === instance.provider)?.category === "custom";
}

function partitionProviderInstances(
  instances: ModelProviderInstanceView[],
  templates: ModelProviderView[],
) {
  const official: ModelProviderInstanceView[] = [];
  const custom: ModelProviderInstanceView[] = [];
  for (const instance of instances) {
    (isCustomProviderInstance(instance, templates) ? custom : official).push(instance);
  }
  return { official, custom };
}

const SIDEBAR_PROVIDER_PREVIEW = 5;

function previewProviderInstances(
  items: ModelProviderInstanceView[],
  expanded: boolean,
  selectedId: string | null,
) {
  if (expanded || items.length <= SIDEBAR_PROVIDER_PREVIEW) return items;
  const preview = items.slice(0, SIDEBAR_PROVIDER_PREVIEW);
  const selected = selectedId ? items.find((item) => item.id === selectedId) : undefined;
  if (selected && !preview.some((item) => item.id === selected.id)) {
    return [...preview, selected];
  }
  return preview;
}

function ProviderSidebarGroup({
  title,
  items,
  selectedId,
  onSelect,
}: {
  title: string;
  items: ModelProviderInstanceView[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const visible = previewProviderInstances(items, expanded, selectedId);
  if (items.length === 0) return null;
  const hiddenCount = Math.max(0, items.length - SIDEBAR_PROVIDER_PREVIEW);

  return (
    <SidebarSection title={title}>
      {visible.map((provider) => (
        <SidebarItem
          key={provider.id}
          active={selectedId === provider.id}
          icon={<ProviderGlyph id={provider.provider} name={provider.name} size={18} />}
          label={provider.name}
          count={provider.model_count}
          onClick={() => onSelect(provider.id)}
        />
      ))}
      {hiddenCount > 0 && (
        <button
          type="button"
          className="arca-sidebar-more"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          <ChevronDownIcon className="arca-sidebar-more-icon" />
          {expanded
            ? t("models.showLessProviders")
            : t("models.showMoreProviders", { count: hiddenCount })}
        </button>
      )}
    </SidebarSection>
  );
}

function providerTemplateConnection(provider: ModelProviderView | null | undefined) {
  return {
    base_url: provider?.base_url ?? "",
    protocols: provider
      ? Object.fromEntries(
          Object.entries(provider.protocols)
            .map(([protocol, config]) => [protocol, { ...config! }]),
        ) as ModelProviderConfig["protocols"]
      : {},
  };
}

function providerTemplatePath(
  provider: ModelProviderView | null | undefined,
  protocol: ModelProtocol,
) {
  return provider?.protocols[protocol]?.endpoint_path ?? DEFAULT_ENDPOINT_PATHS[protocol];
}

function normalizeBaseUrl(value: string) {
  const normalized = value.trim().replace(/\/+$/, "");
  if (!normalized || /[\s<>{}]/.test(normalized) || /%(?:3c|3e|7b|7d)/i.test(normalized)) return null;
  try {
    const url = new URL(normalized);
    if (
      !["http:", "https:"].includes(url.protocol)
      || !url.hostname
      || url.username
      || url.password
      || url.search
      || url.hash
    ) return null;
    return normalized;
  } catch {
    return null;
  }
}

function normalizeModelCatalogUrl(value: string) {
  const normalized = value.trim();
  if (!normalized || /\s/.test(normalized)) return null;
  try {
    const url = new URL(normalized);
    if (
      !["http:", "https:"].includes(url.protocol)
      || !url.hostname
      || url.username
      || url.password
      || url.hash
    ) return null;
    return normalized;
  } catch {
    return null;
  }
}

function normalizeProviderPortalUrl(value: string) {
  const normalized = value.trim();
  if (!normalized || /\s/.test(normalized)) return null;
  try {
    const url = new URL(normalized);
    if (url.protocol !== "https:" || !url.hostname || url.username || url.password || url.hash) return null;
    if ([...url.searchParams.keys()].some((name) =>
      /^(key|api_key|apikey|token|access_token|secret|password|authorization|credential|sig|signature)$/i.test(name))) return null;
    return normalized;
  } catch {
    return null;
  }
}

// Decode only for presentation. The editable and persisted URL remains the
// original string: decoded '&', '#', or '=' may belong inside a query value.
function readablePortalUrl(value: string) {
  const decode = (part: string) => {
    try {
      return decodeURIComponent(part).replace(/[\u0000-\u001f\u007f\u202a-\u202e\u2066-\u2069]/g, encodeURIComponent);
    } catch { return part; }
  };
  const queryStart = value.indexOf("?");
  if (queryStart < 0) return decode(value);
  const query = value.slice(queryStart + 1).split("&").map((parameter) => {
    const separator = parameter.indexOf("=");
    const decodeQuery = (part: string) => decode(part.replace(/\+/g, " "));
    return separator < 0
      ? decodeQuery(parameter)
      : `${decodeQuery(parameter.slice(0, separator))}=${decodeQuery(parameter.slice(separator + 1))}`;
  }).join("&");
  return `${decode(value.slice(0, queryStart))}?${query}`;
}

function normalizeEndpointPath(value: string) {
  const trimmed = value.trim();
  if (
    !trimmed
    || /\s/.test(trimmed)
    || trimmed.includes("#")
    || trimmed.includes("?")
    || trimmed.includes("://")
    || trimmed.startsWith("//")
    || trimmed.includes("\\")
  ) return null;
  const normalized = `/${trimmed.replace(/^\/+/, "")}`;
  if (normalized.split("/").some((segment) => {
    const lower = segment.toLocaleLowerCase();
    const decodedDots = lower.replaceAll("%2e", ".");
    return decodedDots === "." || decodedDots === ".."
      || lower.includes("%2f") || lower.includes("%5c");
  })) {
    return null;
  }
  const first = normalized.replace(/^\/+/, "").split("/", 1)[0];
  if (!trimmed.startsWith("/") && (first.includes(".") || first.includes(":"))) return null;
  return normalized;
}

function fullRequestUrl(baseUrl: string, endpointPath: string) {
  const base = normalizeBaseUrl(baseUrl);
  const path = normalizeEndpointPath(endpointPath);
  return base && path ? `${base}${path}` : "";
}

function profileProviderName(
  profile: ModelProfileView,
  instances: ModelProviderInstanceView[],
  providers: ModelProviderView[],
) {
  return instances.find((item) => item.id === profile.provider_id)?.name
    ?? providerLabel(providers, profile.provider);
}

function formatTokens(value: number) {
  if (value >= 1_000_000) {
    return `${Number((value / 1_000_000).toFixed(value % 1_000_000 === 0 ? 0 : 2))}M`;
  }
  if (value >= 1_000) {
    return `${Number((value / 1_000).toFixed(value % 1_000 === 0 ? 0 : 1))}K`;
  }
  return String(value);
}

function formatCatalogCost(value: number) {
  return `$${Number(value.toFixed(value < 0.01 ? 4 : 2))}/M`;
}

function readableModelName(
  profile: ModelProfileView,
  providerName: string,
  metadata?: ModelsDevMetadata,
) {
  const profileName = profile.name.trim();
  const isProviderPlaceholder = [providerName, profile.provider]
    .some((candidate) => candidate.trim().toLocaleLowerCase() === profileName.toLocaleLowerCase());
  return ((!profileName || isProviderPlaceholder ? metadata?.name : profileName) || profileName || profile.model);
}

export function ModelsView({
  consumptionState,
  intent,
  onIntentConsumed,
}: {
  consumptionState?: ConsumptionState;
  intent?: Extract<ResourceNavigationIntent, { domain: "model" }>;
  onIntentConsumed?(id: number): void;
} = {}) {
  const initialLibrary = cachedModelLibrary();
  const [profiles, setProfiles] = useState<ModelProfileView[]>(initialLibrary?.profiles ?? []);
  const [providers, setProviders] = useState<ModelProviderView[]>(initialLibrary?.providers ?? []);
  const [providerInstances, setProviderInstances] = useState<ModelProviderInstanceView[]>(initialLibrary?.instances ?? []);
  const [providerFilter, setProviderFilter] = useState<string | null>(null);
  const [providerCatalogOpen, setProviderCatalogOpen] = useState(false);
  const [creatingForProviderId, setCreatingForProviderId] = useState<string | null>(null);
  const [creatingProviderTemplate, setCreatingProviderTemplate] = useState<ModelProviderView | null>(null);
  const [editingProvider, setEditingProvider] = useState<ModelProviderInstanceView | null | undefined>(undefined);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [editing, setEditing] = useState<ModelProfileView | null | undefined>(undefined);
  const [loading, setLoading] = useState(!initialLibrary);
  const [readError, setReadError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [addingCatalogId, setAddingCatalogId] = useState<string | null>(null);
  const catalogAddPending = useRef(false);
  const deferredQuery = useDeferredValue(query);
  const [modelsDevByProfileId, setModelsDevByProfileId] = useState<Record<string, ModelsDevMetadata>>({});
  const toast = useToast();
  const showToast = toast.show;
  const { t } = useTranslation();
  const lastConsumedIntentId = useRef<number | null>(null);

  const modelRevision = useModelObservationRevision();
  const queryGeneration = useRef(0);
  const refresh = useCallback(async (force = false) => {
    const generation = ++queryGeneration.current;
    const { profiles: nextProfiles, providers: nextProviders, instances: nextProviderInstances } = await loadModelLibrary(force);
    if (generation !== queryGeneration.current) return;
    setProfiles(nextProfiles);
    setProviders(nextProviders);
    setProviderInstances(nextProviderInstances);
    setProviderFilter((current) =>
      current && nextProviderInstances.some((provider) => provider.id === current) ? current : null
    );
    setSelectedProfileId((current) =>
      current && nextProfiles.some((profile) => profile.id === current) ? current : null
    );
  }, []);

  useEffect(() => {
    let active = true;
    refresh()
      .then(() => { if (active) setReadError(null); })
      .catch((error) => {
        if (!active) return;
        const message = formatError(error);
        setReadError(message);
        showToast({ kind: "error", msg: t("models.readFailed", { error: message }) });
      })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; queryGeneration.current += 1; };
  }, [refresh, showToast, t, modelRevision]);

  useEffect(() => {
    let active = true;
    setModelsDevByProfileId(getCachedModelsDevMetadata(profiles));
    if (profiles.length > 0) {
      void loadModelsDevMetadata(profiles).then((metadata) => {
        if (active) setModelsDevByProfileId(metadata);
      });
    }
    return () => { active = false; };
  }, [profiles]);

  const searchIndex = useMemo(() => {
    const instanceNames = new Map(providerInstances.map((item) => [item.id, item.name]));
    const templateNames = new Map(providers.map((item) => [item.id, item.name]));
    return profiles.map((profile) => ({ profile, text: [
        profile.name,
        profile.id,
        profile.model,
        profile.base_url,
        profile.provider,
        instanceNames.get(profile.provider_id ?? "") ?? templateNames.get(profile.provider) ?? profile.provider,
        profile.catalog_key,
        protocolLabel(profile.protocol),
      ]
        .join(" ")
        .toLocaleLowerCase() }));
  }, [profiles, providerInstances, providers]);
  const filteredProfiles = useMemo(() => {
    const needle = deferredQuery.trim().toLocaleLowerCase();
    return searchIndex.filter(({ profile, text }) =>
      (!providerFilter || profile.provider_id === providerFilter) && (!needle || text.includes(needle)),
    ).map(({ profile }) => profile);
  }, [searchIndex, providerFilter, deferredQuery]);

  const refreshAfterSave = () => {
    void refresh(true).catch((error) => {
      setReadError(formatError(error));
    });
  };
  const saveDraft = async (draft: CentralAssetDraft) => {
    if (!consumptionState) throw new Error(t("models.saveUnavailable"));
    const plan = await consumptionState.planUpdate(draft, { reviewOnlyWhenNeeded: true });
    if (requiresAgentReview(plan)) return false;
    await consumptionState.commit({ background: true });
    toast.show({ kind: "success", msg: t("models.saved") });
    await refresh(true).catch((error) => setReadError(formatError(error)));
    return true;
  };

  const selectedProfile = profiles.find((profile) => profile.id === selectedProfileId) ?? null;
  const selectedProvider = providerInstances.find((provider) => provider.id === providerFilter) ?? null;
  const creationDisabled = !consumptionState || consumptionState.committing || Boolean(consumptionState.plan) || addingCatalogId !== null;
  const selectedProviderProtocols = selectedProvider
    ? PROTOCOLS.filter(({ id }) => selectedProvider.protocols[id]).map(({ id, label }) => ({ value: id, label }))
    : [];
  const { official: officialProviders, custom: customProviders } = useMemo(
    () => partitionProviderInstances(providerInstances, providers),
    [providerInstances, providers],
  );

  useEffect(() => {
    if (!intent || loading || lastConsumedIntentId.current === intent.id) return;
    lastConsumedIntentId.current = intent.id;
    if (intent.kind === "create") {
      setSelectedProfileId(null);
      setProviderCatalogOpen(true);
      onIntentConsumed?.(intent.id);
      return;
    }
    const profile = profiles.find((candidate) => candidate.id === intent.profileId);
    setQuery("");
    setProviderFilter(null);
    setSelectedProfileId(profile?.id ?? null);
    if (!profile) toast.show({ kind: "error", msg: t("models.notFound", { id: intent.profileId }) });
    onIntentConsumed?.(intent.id);
  }, [intent, loading, onIntentConsumed, profiles, t, toast]);

  const clearSelection = useCallback(() => {
    setSelectedProfileId(null);
    setEditing(undefined);
    setCreatingForProviderId(null);
    setCreatingProviderTemplate(null);
  }, []);
  const selectProvider = (id: string | null) => {
    clearSelection();
    setProviderFilter(id);
  };
  const planProfileDelete = useCallback(async (profile: ModelProfileView) => {
    if (!consumptionState) return;
    try {
      await consumptionState.planDelete({ domain: "model", profile_id: profile.id });
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.cannotDelete", { error: formatError(error) }) });
    }
  }, [consumptionState?.planDelete, toast.show, t]);
  const createManualModel = () => {
    if (!selectedProvider || creationDisabled) return;
    clearSelection();
    setCreatingForProviderId(selectedProvider.id);
    setEditing(null);
  };
  const addCatalogModel = async (model: ProviderModelSummary, protocol: ModelProtocol) => {
    if (!selectedProvider || creationDisabled || catalogAddPending.current) return false;
    catalogAddPending.current = true;
    setAddingCatalogId(model.id);
    try {
      return await saveDraft({
        domain: "model",
        profile: {
          ...emptyProfile(),
          name: model.name || "",
          provider_id: selectedProvider.id,
          provider: selectedProvider.provider,
          model: model.id,
          protocol,
          base_url: selectedProvider.base_url,
          ...(model.context_length != null && model.context_length > 0
            ? { context_window: model.context_length } : {}),
        },
      });
    } finally {
      catalogAddPending.current = false;
      setAddingCatalogId(null);
    }
  };

  return (
    <div className="arca-models-workspace">
      <ResourceWorkspace
        overview={selectedProvider ? (
          <ProviderOverview
            provider={selectedProvider}
            portal={selectedProvider.portal}
            onEdit={consumptionState ? () => setEditingProvider(selectedProvider) : undefined}
            onDelete={consumptionState ? async () => {
              try {
                await consumptionState.planDelete({
                  domain: "model-provider",
                  provider_id: selectedProvider.id,
                });
              } catch (error) {
                toast.show({
                  kind: "error",
                  msg: t("models.cannotDeleteProvider", { error: formatError(error) }),
                });
              }
            } : undefined}
          />
        ) : (
          <ResourceOverview
            eyebrow={`${profiles.length} Models · ${providerInstances.length} Providers`}
            title={t("models.allModels")}
            description={t("models.description")}
            icon={<ResourceIcon domain="model" className="w-6 h-6" />}
          />
        )}
        sidebar={
          <WorkspaceSidebar title={t("models.title")} count={profiles.length}
            actions={
              <button className="btn-primary" type="button" disabled={creationDisabled} onClick={() => {
                clearSelection();
                setProviderCatalogOpen(true);
              }}>
                <PlusIcon className="w-4 h-4" />
                {t("models.addProvider")}
              </button>
            }
          >
            <SidebarSection title={t("models.library")}>
              <SidebarItem
                active={providerFilter === null}
                icon={<ResourceIcon domain="model" className="w-3.5 h-3.5" />}
                label={t("models.allModels")}
                count={profiles.length}
                onClick={() => selectProvider(null)}
              />
            </SidebarSection>
            <ProviderSidebarGroup
              title={t("models.customProviders")}
              items={customProviders}
              selectedId={providerFilter}
              onSelect={selectProvider}
            />
            <ProviderSidebarGroup
              title={t("models.officialProviders")}
              items={officialProviders}
              selectedId={providerFilter}
              onSelect={selectProvider}
            />
          </WorkspaceSidebar>
        }
        query={query}
        onQueryChange={(value) => { clearSelection(); setQuery(value); }}
        searchPlaceholder={t("models.search")}
        listLabel="Models"
        resultCount={selectedProvider ? undefined : filteredProfiles.length}
        inspector={selectedProfile && editing?.id === selectedProfile.id ? (
          <ModelProfileDialog
            initial={editing}
            providerInstances={providerInstances}
            preferredProviderId={editing.provider_id}
            presentation="inspector"
            onClose={clearSelection}
            onReview={async (profile) => {
              if (!consumptionState) throw new Error(t("models.saveUnavailable"));
              await saveDraft({
                domain: "model",
                existing_id: editing.id,
                profile,
              });
              clearSelection();
            }}
          />
        ) : selectedProfile ? (
          <ModelInspector
            profile={selectedProfile}
            providerName={profileProviderName(selectedProfile, providerInstances, providers)}
            provider={providerInstances.find((provider) => provider.id === selectedProfile.provider_id) ?? null}
            metadata={modelsDevByProfileId[selectedProfile.id]}
            onClose={clearSelection}
            onEdit={consumptionState ? () => setEditing(selectedProfile) : undefined}
          />
        ) : undefined}
        onInspectorClose={clearSelection}
      >
        {consumptionState?.plan ? (
          <AssetOperationReviewDialog
            plan={consumptionState.plan}
            busy={consumptionState.committing}
            error={consumptionState.error}
            assetDisplayNames={Object.fromEntries(
              [
                ...profiles.map((profile) => [`model:${profile.id}`, profile.name] as const),
                ...providerInstances.map((provider) => [
                  `model-provider:${provider.id}`,
                  provider.name,
                ] as const),
              ],
            )}
            onCancel={consumptionState.cancel}
            onCommit={async () => {
              const kind = consumptionState.plan?.kind;
              await consumptionState.commit();
              refreshAfterSave();
              if (kind === "delete-asset") setSelectedProfileId(null);
              toast.show({
                kind: "success",
                msg: kind === "delete-asset" ? t("models.deleted") : t("models.saved"),
              });
            }}
          />
        ) : null}
        {loading ? (
          <ResourceState kind="loading" title={t("models.loading")} />
        ) : readError ? (
          <ResourceState
            kind="read-error"
            icon={<ResourceIcon domain="model" className="w-6 h-6" />}
            title={t("models.readFailedTitle")}
            detail={readError}
            action={<button className="btn-primary" type="button" onClick={() => {
              setLoading(true);
              setReadError(null);
              void refresh(true)
                .catch((error) => setReadError(formatError(error)))
                .finally(() => setLoading(false));
            }}>{t("common.retry")}</button>}
          />
        ) : selectedProvider ? (
          <>
            <section aria-label={t("models.addedModels")}>
              <div className="arca-model-section-heading">
                <h2>{t("models.addedModels")} <span>{filteredProfiles.length}</span></h2>
              </div>
              <ModelList profiles={filteredProfiles} providerInstances={providerInstances} providers={providers}
                metadata={modelsDevByProfileId} selectedProfileId={selectedProfileId}
                leading={<ManualModelCard disabled={creationDisabled} onAdd={createManualModel} />}
                onDelete={consumptionState && !creationDisabled ? planProfileDelete : undefined}
                onOpen={(profileId) => { setEditing(undefined); setSelectedProfileId(profileId); }} />
            </section>
            {selectedProvider.model_discovery_supported ? (
              <ProviderModelCatalog key={selectedProvider.id} provider={selectedProvider} revision={modelRevision}
                profiles={profiles} query={deferredQuery} protocols={selectedProviderProtocols}
                disabled={creationDisabled} addingId={addingCatalogId} onAdd={addCatalogModel} />
            ) : <p className="arca-model-catalog-notice">{t("models.discoveryUnsupported")}</p>}
          </>
        ) : filteredProfiles.length === 0 ? (
          <ResourceState
            kind={profiles.length === 0 ? "empty" : "no-match"}
            icon={<ResourceIcon domain="model" className="w-6 h-6" />}
            title={profiles.length === 0 ? t("models.empty") : t("models.noMatches")}
            detail={profiles.length === 0 ? t("models.emptyDetail") : t("models.noMatchesDetail")}
            action={profiles.length === 0 ? undefined : (
              <button className="btn-secondary" type="button" onClick={() => {
                setQuery("");
                setProviderFilter(null);
              }}>{t("models.clearFilters")}</button>
            )}
          />
        ) : (
          <ModelList
            profiles={filteredProfiles}
            providerInstances={providerInstances}
            providers={providers}
            metadata={modelsDevByProfileId}
            selectedProfileId={selectedProfileId}
            onDelete={consumptionState ? planProfileDelete : undefined}
            onOpen={(profileId) => {
              setEditing(undefined);
              setSelectedProfileId(profileId);
            }}
          />
        )}
      </ResourceWorkspace>

      {providerCatalogOpen && (
        <ProviderCatalogDialog
          providers={providers}
          onClose={() => setProviderCatalogOpen(false)}
          onUse={(provider) => {
            setProviderCatalogOpen(false);
            setCreatingProviderTemplate(provider);
            setEditingProvider(null);
          }}
        />
      )}

      {editing === null && (
        <ModelProfileDialog
          initial={editing}
          providerInstances={providerInstances}
          preferredProviderId={creatingForProviderId}
          onClose={() => {
            setEditing(undefined);
            setCreatingProviderTemplate(null);
          }}
          onReview={async (profile) => {
            if (!consumptionState) throw new Error(t("models.saveUnavailable"));
            await saveDraft({
              domain: "model",
              existing_id: undefined,
              profile,
            });
            setEditing(undefined);
            setCreatingForProviderId(null);
            setCreatingProviderTemplate(null);
          }}
        />
      )}

      {editingProvider !== undefined && (
        <ModelProviderDialog
          initial={editingProvider}
          providerTemplate={creatingProviderTemplate}
          providers={providers}
          onClose={() => {
            setEditingProvider(undefined);
            setCreatingProviderTemplate(null);
          }}
          onReview={async (provider, credential) => {
            if (!consumptionState) throw new Error(t("models.saveUnavailable"));
            await saveDraft({
              domain: "model-provider",
              existing_id: editingProvider?.id,
              provider,
              credential,
            });
            setEditingProvider(undefined);
            setCreatingProviderTemplate(null);
          }}
        />
      )}

    </div>
  );
}

function ProviderPortalButton({ portal }: { portal?: ModelProviderView["portal"] }) {
  const { t } = useTranslation();
  const toast = useToast();
  if (!portal || !portal.url.startsWith("https://")) return null;
  const label = t({
    "api-key": "models.getApiKey",
    console: "models.openProviderConsole",
    setup: "models.providerSetupGuide",
  }[portal.kind]);
  return (
    <button type="button" className="btn-ghost shrink-0 whitespace-nowrap" title={portal.url}
      aria-label={label}
      onClick={() => void openUrl(portal.url).catch((error) => {
        toast.show({ kind: "error", msg: t("models.openProviderLinkFailed", { error: formatError(error) }) });
      })}>
      <ExternalLinkIcon className="w-4 h-4" />
      {label}
    </button>
  );
}

function ProviderOverview({
  provider,
  portal,
  onEdit,
  onDelete,
}: {
  provider: ModelProviderInstanceView;
  portal?: ModelProviderView["portal"];
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <ResourceOverview
      eyebrow={`Provider · ${t("models.providerModelCount", { count: provider.model_count })}`}
      title={provider.name}
      icon={<ProviderGlyph id={provider.provider} name={provider.name} size={34} />}
      actions={<>
        <button className="btn-secondary" type="button" disabled={!onEdit} onClick={onEdit}>
          <EditIcon className="w-4 h-4" />
          {t("models.editProvider")}
        </button>
        <button className="btn-ghost" type="button" disabled={!onDelete} onClick={onDelete}
          title={t("common.delete")} aria-label={`${t("common.delete")} ${provider.name}`}>
          <TrashIcon className="w-4 h-4" />
        </button>
      </>}
    >
      <dl className="arca-resource-overview-facts">
        <div>
          <dt>Base URL</dt>
          <dd><code title={provider.base_url}>{provider.base_url}</code></dd>
        </div>
        <div>
          <dt>{t("models.protocolsShort")}</dt>
          <dd className="arca-resource-overview-tags">
            {PROTOCOLS.filter(({ id }) => provider.protocols[id]).map(({ id, label }) => (
              <span key={id} title={provider.protocols[id]?.endpoint_path}>{label}</span>
            ))}
          </dd>
        </div>
      </dl>
      <div className="arca-resource-overview-links">
        <span className="arca-resource-overview-credential" data-saved={provider.credential_saved || undefined}>
          <KeyIcon className="w-3.5 h-3.5" />
          {provider.credential_saved ? t("models.keychainSaved") : `Keychain · ${t("models.keychainNotSaved")}`}
        </span>
        <ProviderPortalButton portal={portal} />
      </div>
    </ResourceOverview>
  );
}

const ModelList = memo(function ModelList({
  profiles,
  providerInstances,
  providers,
  metadata,
  selectedProfileId,
  onOpen,
  onDelete,
  leading,
}: {
  profiles: ModelProfileView[];
  providerInstances: ModelProviderInstanceView[];
  providers: ModelProviderView[];
  metadata: Record<string, ModelsDevMetadata>;
  selectedProfileId: string | null;
  onOpen: (profileId: string) => void;
  onDelete?: (profile: ModelProfileView) => Promise<void>;
  leading?: ReactNode;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const [copyingId, setCopyingId] = useState<string | null>(null);
  const copyPending = useRef(false);
  const instanceIndex = useMemo(() => new Map(providerInstances.map((item) => [item.id, item])), [providerInstances]);
  const templateIndex = useMemo(() => new Map(providers.map((item) => [item.id, item.name])), [providers]);
  const copyCurl = async (profile: ModelProfileView) => {
    if (copyPending.current) return;
    copyPending.current = true;
    setCopyingId(profile.id);
    try {
      await copyToClipboard(await exportModelCurl(profile.id, true));
      toast.show({ kind: "success", msg: t("models.curlCopied") });
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.curlCopyFailed", { error: formatError(error) }) });
    } finally {
      copyPending.current = false;
      setCopyingId(null);
    }
  };
  return (
    <div className="arca-asset-list arca-model-list" role="list" aria-label={t("models.asset")}>
      {leading}
      {profiles.map((profile) => {
        const provider = instanceIndex.get(profile.provider_id ?? "");
        const providerName = provider?.name ?? templateIndex.get(profile.provider) ?? profile.provider;
        const profileMetadata = metadata[profile.id];
        const displayName = readableModelName(profile, providerName, profileMetadata);
        const contextWindow = profile.context_window ?? profileMetadata?.contextWindow;
        const copyLabel = `${t("models.copyCurl")} · ${displayName}`;
        const deleteLabel = `${t("common.delete")} · ${displayName}`;
        return (
          <div role="listitem" key={profile.id} className="arca-model-card">
            <button
              type="button"
              className="arca-asset-list-row arca-model-list-row"
              data-selected={profile.id === selectedProfileId ? "true" : undefined}
              aria-label={t("models.openDetails", { name: profile.name })}
              onClick={() => onOpen(profile.id)}
            >
              <span className="arca-asset-list-identity arca-model-list-identity">
                <ProviderGlyph id={profile.provider || "custom"} name={providerName} size={34} />
                <span className="arca-asset-list-copy">
                  <strong title={displayName}>{displayName}</strong>
                  <span className="arca-model-list-subline">
                    <code title={profile.model}>{profile.model}</code>
                    {contextWindow && (
                      <span
                        className="arca-model-list-context"
                        aria-label={`${t("models.context")} ${formatTokens(contextWindow)}`}
                        title={`${t("models.contextWindow")}: ${contextWindow.toLocaleString()} tokens`}
                      >
                        <span>{t("models.context")}</span>
                        <strong>{formatTokens(contextWindow)}</strong>
                      </span>
                    )}
                  </span>
                </span>
              </span>
            </button>
            <div className="arca-model-card-actions">
              <button type="button" className="arca-model-card-action"
                aria-label={copyLabel} title={copyLabel}
                aria-busy={copyingId === profile.id}
                disabled={copyingId !== null}
                onClick={() => void copyCurl(profile)}>
                <TerminalIcon className="w-4 h-4" />
              </button>
              <button type="button" className="arca-model-card-action arca-model-card-delete"
                aria-label={deleteLabel} title={deleteLabel}
                disabled={!onDelete || copyingId === profile.id}
                onClick={() => void onDelete?.(profile)}>
                <TrashIcon className="w-4 h-4" />
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
});

function ModelInspector({
  profile,
  providerName,
  provider,
  metadata,
  onClose,
  onEdit,
}: {
  profile: ModelProfileView;
  providerName: string;
  provider: ModelProviderInstanceView | null;
  metadata?: ModelsDevMetadata;
  onClose: () => void;
  onEdit?: () => void;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const contextWindow = profile.context_window ?? metadata?.contextWindow;
  const maxOutputTokens = profile.max_output_tokens ?? metadata?.maxOutputTokens;
  const capabilities = [
    metadata?.toolCall && t("models.tools"),
    metadata?.structuredOutput && t("models.structuredOutput"),
    metadata?.modalities?.some((modality) => modality !== "text") && t("models.multimodal"),
  ].filter((item): item is string => Boolean(item));
  const showReasoning = profile.reasoning !== undefined || metadata?.reasoning === true;
  const requestUrl = provider
    ? fullRequestUrl(
        provider.base_url,
        provider.protocols[profile.protocol]?.endpoint_path ?? "",
      )
    : "";
  const priceSummary = [
    metadata?.inputCost != null && t("models.inputMetric", { value: formatCatalogCost(metadata.inputCost) }),
    metadata?.outputCost != null && t("models.outputPriceMetric", { value: formatCatalogCost(metadata.outputCost) }),
  ].filter(Boolean).join(" · ");
  const copyValue = async (label: string, value: string) => {
    try {
      await copyToClipboard(value);
      toast.show({ kind: "success", msg: t("models.copiedValue", { label }) });
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.copyValueFailed", { error: formatError(error) }) });
    }
  };
  const copyAction = (label: string, value: string) => (
    <button
      type="button"
      aria-label={t("models.copyValue", { label })}
      title={t("models.copyValue", { label })}
      onClick={() => void copyValue(label, value)}
    >
      <CopyIcon className="w-3.5 h-3.5" />
    </button>
  );
  return (
    <ResourceInspector
      title={readableModelName(profile, providerName, metadata)}
      avatar={<Avatar seed={profile.name} kind="model" size={40} />}
      subtitle={<Badge tone="neutral">{protocolLabel(profile.protocol)}</Badge>}
      onClose={onClose}
      footer={
        <>
          <div className="flex-1" />
          <button className="btn-primary" type="button" disabled={!onEdit} onClick={onEdit}>
            <EditIcon className="w-4 h-4" />
            {t("common.edit")}
          </button>
        </>
      }
    >
      {(contextWindow || maxOutputTokens || capabilities.length > 0 || priceSummary) && (
        <InspectorMetrics>
          {contextWindow && (
            <InspectorMetric icon={<LayersIcon />} label={t("models.context")} value={formatTokens(contextWindow)} />
          )}
          {maxOutputTokens && (
            <InspectorMetric icon={<GaugeIcon />} label={t("models.outputLimit")} value={formatTokens(maxOutputTokens)} />
          )}
          {capabilities.length > 0 && (
            <InspectorMetric icon={<SparklesIcon />} label={t("models.capabilities")} value={capabilities.join(" · ")} />
          )}
          {priceSummary && (
            <InspectorMetric icon={<KeyIcon />} label={t("models.catalogPrice")} value={priceSummary} />
          )}
        </InspectorMetrics>
      )}
      {metadata?.description && <p className="arca-model-inspector-description">{metadata.description}</p>}
      <section className="arca-model-inspector-fields" aria-label={t("models.detailsFields")}>
        <InspectorField icon={<LayersIcon />} label={t("models.provider")}>{providerName}</InspectorField>
        <InspectorField icon={<NetworkIcon />} label={t("models.protocol")}>{protocolLabel(profile.protocol)}</InspectorField>
        {showReasoning && (
          <InspectorField icon={<SparklesIcon />} label={t("models.reasoningMode")}>
            {profile.reasoning === undefined
              ? t("models.reasoningAuto")
              : profile.reasoning
                ? t("models.reasoningOn")
                : t("models.reasoningOff")}
          </InspectorField>
        )}
        {metadata?.releaseDate && <InspectorField icon={<CalendarIcon />} label={t("models.releaseDate")}>{metadata.releaseDate}</InspectorField>}
        <InspectorField icon={<TerminalIcon />} label={t("models.modelId")} mono wide action={copyAction(t("models.modelId"), profile.model)}>{profile.model}</InspectorField>
        <InspectorField
          icon={<LinkIcon />}
          label={t("models.fullRequestUrl")}
          mono
          wide
          action={requestUrl ? copyAction(t("models.fullRequestUrl"), requestUrl) : undefined}
        >
          {requestUrl || t("common.notSet")}
        </InspectorField>
        {profile.env_key && <InspectorField icon={<KeyIcon />} label={t("models.environmentVariable")} mono wide>{profile.env_key}</InspectorField>}
      </section>
    </ResourceInspector>
  );
}

function ProviderCatalogDialog({
  providers,
  onClose,
  onUse,
}: {
  providers: ModelProviderView[];
  onClose: () => void;
  onUse: (provider: ModelProviderView) => void;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState(
    providers.find((provider) => provider.id === "custom")?.id ?? providers[0]?.id ?? "",
  );
  const orderedProviders = [
    ...providers.filter((provider) => provider.id === "custom"),
    ...providers.filter((provider) => provider.id !== "custom"),
  ];
  const visibleProviders = orderedProviders.filter((provider) => {
    const needle = query.trim().toLocaleLowerCase();
    return !needle || [
      provider.name,
      provider.id,
      provider.base_url ?? "",
      ...Object.values(provider.protocols).flatMap((config) => [
        config?.endpoint_path ?? "",
        fullRequestUrl(provider.base_url ?? "", config?.endpoint_path ?? ""),
      ]),
      provider.default_base_url ?? "",
      ...provider.additional_endpoints.map(({ base_url }) => base_url),
    ]
      .join(" ")
      .toLocaleLowerCase()
      .includes(needle);
  });
  const selected = visibleProviders.find((provider) => provider.id === selectedId) ?? null;

  return (
    <DialogShell
      className="arca-dialog-provider-catalog"
      kind="picker"
      size="lg"
      title={t("models.providerCatalogTitle")}
      onClose={onClose}
      footerStart={selected ? (
        <span className="arca-provider-catalog-selection">
          <span aria-hidden="true">✓</span>
          <strong>{selected.name}</strong>
          <ProviderPortalButton portal={selected.portal} />
        </span>
      ) : undefined}
      footerEnd={(
        <>
          <button type="button" className="btn-ghost" onClick={onClose}>{t("common.cancel")}</button>
          <button
            type="button"
            className="btn-primary"
            disabled={!selected}
            onClick={() => selected && onUse(selected)}
          >
            {t("models.useProviderTemplate")}
          </button>
        </>
      )}
    >
      <div className="arca-provider-catalog">
        <label className="arca-provider-catalog-search">
          <SearchIcon className="w-4 h-4" />
          <input
            autoFocus
            type="search"
            className="arca-model-field"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t("models.searchProviders")}
            aria-label={t("models.searchProviders")}
          />
        </label>
        <div className="arca-provider-catalog-grid" role="radiogroup" aria-label={t("models.providerCatalogTitle")}>
          {visibleProviders.map((provider) => (
            <button
              type="button"
              role="radio"
              aria-checked={provider.id === selectedId}
              data-selected={provider.id === selectedId ? "true" : undefined}
              className="arca-provider-catalog-item"
              key={provider.id}
              onClick={() => setSelectedId(provider.id)}
            >
              <span className="arca-provider-catalog-icon">
                <ProviderGlyph id={provider.id} name={provider.name} size={26} />
              </span>
              <span className="arca-provider-catalog-copy">
                <strong>{provider.name}</strong>
                <code>
                  {provider.base_url
                    ? fullRequestUrl(
                        provider.base_url,
                        provider.protocols[provider.default_protocol]?.endpoint_path
                          ?? DEFAULT_ENDPOINT_PATHS[provider.default_protocol],
                      )
                    : t("models.providerEndpointRequired")}
                </code>
              </span>
              <span className="arca-provider-catalog-check" aria-hidden="true">✓</span>
            </button>
          ))}
        </div>
        {visibleProviders.length === 0 && (
          <div className="arca-provider-catalog-empty">{t("models.noProviderMatches")}</div>
        )}
      </div>
    </DialogShell>
  );
}

function ModelProfileDialog({
  initial,
  providerInstances,
  preferredProviderId,
  onClose,
  onReview,
  presentation = "dialog",
}: {
  initial: ModelProfileView | null;
  providerInstances: ModelProviderInstanceView[];
  preferredProviderId?: string | null;
  onClose: () => void;
  onReview: (profile: ModelProfile) => Promise<void>;
  presentation?: "dialog" | "inspector";
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const reasoningGroupId = useId();
  const preferredProvider = providerInstances.find((provider) =>
    provider.id === (initial?.provider_id ?? preferredProviderId),
  ) ?? null;
  const [draft, setDraft] = useState<ModelProfile>(() => initial ?? {
    ...emptyProfile(),
    provider_id: preferredProvider?.id,
    provider: preferredProvider?.provider ?? "",
    protocol: PROTOCOLS.find(({ id }) => preferredProvider?.protocols[id])?.id ?? "openai-responses",
    base_url: preferredProvider?.base_url ?? "",
    env_key: preferredProvider?.env_key,
  });
  const [busy, setBusy] = useState(false);
  const providerInstance = providerInstances.find((provider) => provider.id === draft.provider_id) ?? null;
  const availableProtocols = PROTOCOLS.filter(({ id }) => providerInstance?.protocols[id]);
  const requestUrl = providerInstance
    ? fullRequestUrl(providerInstance.base_url, providerInstance.protocols[draft.protocol]?.endpoint_path ?? "")
    : "";
  const contextValid = draft.context_window == null
    || (Number.isSafeInteger(draft.context_window) && draft.context_window > 0);
  const outputValid = draft.max_output_tokens == null
    || (Number.isSafeInteger(draft.max_output_tokens) && draft.max_output_tokens > 0);
  const valid = Boolean(providerInstance && requestUrl && draft.model.trim() && contextValid && outputValid && !busy);
  const reasoningValue = draft.reasoning === undefined ? "auto" : draft.reasoning ? "on" : "off";

  const selectProvider = (providerId: string) => {
    const provider = providerInstances.find((candidate) => candidate.id === providerId);
    if (!provider) return;
    setDraft((current) => ({
      ...current,
      provider_id: provider.id,
      provider: provider.provider,
      protocol: provider.protocols[current.protocol] ? current.protocol
        : PROTOCOLS.find(({ id }) => provider.protocols[id])?.id ?? current.protocol,
      base_url: provider.base_url,
      env_key: provider.env_key,
    }));
  };
  const save = async () => {
    if (!valid) return;
    setBusy(true);
    try {
      await onReview({ ...draft, id: initial?.id ?? "", name: draft.name.trim(), model: draft.model.trim() });
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.saveFailed", { error: formatError(error) }) });
    } finally {
      setBusy(false);
    }
  };
  const copyRequestUrl = async () => {
    if (!requestUrl) return;
    try {
      await copyToClipboard(requestUrl);
      toast.show({ kind: "success", msg: t("models.copiedValue", { label: t("models.fullRequestUrl") }) });
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.copyValueFailed", { error: formatError(error) }) });
    }
  };
  const providerIdentity = initial ? (
    <div className="arca-model-editor-provider-select">
      <FormSelect ariaLabel={t("models.provider")} value={draft.provider_id ?? ""}
        placeholder={t("models.providerPlaceholder")}
        options={providerInstances.map((provider) => ({ value: provider.id, label: provider.name }))}
        onChange={selectProvider} disabled={busy} />
    </div>
  ) : <span className="arca-model-editor-provider-name" title={providerInstance?.name}>{providerInstance?.name ?? t("models.providerRequired")}</span>;
  const providerAvatar = <span className="arca-model-editor-avatar">
    <ProviderGlyph id={providerInstance?.provider ?? initial?.provider ?? "custom"}
      name={providerInstance?.name ?? initial?.name ?? "Provider"} size={30} />
  </span>;
  const footer = <>
    <button type="button" className="btn-ghost" disabled={busy} onClick={onClose}>{t("common.cancel")}</button>
    <button type="button" className="btn-primary" disabled={!valid} onClick={() => void save()}>
      {initial ? busy ? t("common.saving") : t("common.save")
        : busy ? t("models.addingAction") : t("models.addAction")}
    </button>
  </>;
  const form = (
    <div className="arca-model-form arca-model-editor-form">
      <label className="arca-model-editor-id">
        <span>{t("models.modelId")}</span>
        <input autoFocus data-modal-initial-focus className="arca-model-field"
          aria-label={t("models.modelId")} value={draft.model} spellCheck={false}
          autoComplete="off" autoCorrect="off" autoCapitalize="none"
          placeholder={t("models.modelIdPlaceholder")}
          onChange={(event) => setDraft({ ...draft, model: event.currentTarget.value })} />
      </label>
      <div className="arca-model-form-grid">
        <label>
          <span>{t("models.optionalName")}</span>
          <input className="arca-model-field" value={draft.name}
            onChange={(event) => setDraft({ ...draft, name: event.currentTarget.value })}
            placeholder={t("models.generatedNameShort")} />
        </label>
        <div className="arca-model-form-field">
          <span>{t("models.protocol")}</span>
          <FormSelect ariaLabel={t("models.protocol")} value={draft.protocol}
            options={availableProtocols.map(({ id, label }) => ({ value: id, label }))}
            onChange={(protocol) => setDraft({ ...draft, protocol: protocol as ModelProtocol })} />
        </div>
      </div>
      <div className="arca-model-form-grid arca-model-editor-limits">
        <label>
          <span>{t("models.contextWindow")} <small>tokens</small></span>
          <input type="number" min={1} step={1} className="arca-model-field"
            value={draft.context_window ?? ""} aria-invalid={!contextValid || undefined}
            placeholder={t("models.modelDefault")}
            onChange={(event) => setDraft({ ...draft,
              context_window: event.currentTarget.value ? Number(event.currentTarget.value) : undefined })} />
          {!contextValid && <ToastNotice kind="warning" delay={500} message={t("models.positiveIntegerRequired")} />}
        </label>
        <label>
          <span>{t("models.maxOutput")} <small>tokens</small></span>
          <input type="number" min={1} step={1} className="arca-model-field"
            value={draft.max_output_tokens ?? ""} aria-invalid={!outputValid || undefined}
            placeholder={t("models.modelDefault")}
            onChange={(event) => setDraft({ ...draft,
              max_output_tokens: event.currentTarget.value ? Number(event.currentTarget.value) : undefined })} />
          {!outputValid && <ToastNotice kind="warning" delay={500} message={t("models.positiveIntegerRequired")} />}
        </label>
      </div>
      <div className="arca-model-form-field">
        <span>{t("models.reasoningMode")}</span>
        <div className="arca-model-reasoning-options" role="radiogroup" aria-label={t("models.reasoningMode")}>
          {[
            { value: "auto", label: t("models.reasoningAuto") },
            { value: "on", label: t("models.reasoningOn") },
            { value: "off", label: t("models.reasoningOff") },
          ].map(({ value, label }) => <label key={value}>
            <input type="radio" name={reasoningGroupId} value={value} checked={reasoningValue === value}
              onChange={() => setDraft({ ...draft, reasoning: value === "auto" ? undefined : value === "on" })} />
            <span>{label}</span>
          </label>)}
        </div>
      </div>
      <div className="arca-model-editor-endpoint">
        <div>
          <span>{t("models.fullRequestUrl")}</span>
          <button type="button" className="arca-model-card-action" disabled={!requestUrl}
            title={t("models.copyValue", { label: t("models.fullRequestUrl") })}
            aria-label={t("models.copyValue", { label: t("models.fullRequestUrl") })}
            onClick={() => void copyRequestUrl()}><CopyIcon className="w-3.5 h-3.5" /></button>
        </div>
        <code>{requestUrl || t("models.fullRequestUrlUnavailable")}</code>
      </div>
    </div>
  );
  if (presentation === "inspector" && initial) {
    return <ResourceInspector title={t("models.editTitle")} avatar={providerAvatar}
      subtitle={providerIdentity} onClose={onClose} footer={<><div className="flex-1" />{footer}</>}>
      {form}
    </ResourceInspector>;
  }
  return <DialogShell className="arca-dialog-model-editor" kind="editor" size="md"
    leading={providerAvatar} title={initial ? t("models.editTitle") : t("models.createTitle")}
    subtitle={providerIdentity} busy={busy} onClose={onClose} footerEnd={footer}>
    {form}
  </DialogShell>;
}

function ModelProviderDialog({
  initial,
  providerTemplate,
  providers,
  onClose,
  onReview,
}: {
  initial: ModelProviderInstanceView | null;
  providerTemplate: ModelProviderView | null;
  providers: ModelProviderView[];
  onClose: () => void;
  onReview: (provider: ModelProviderConfig, credential?: string) => Promise<void>;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const initialProviderType = initial?.provider ?? providerTemplate?.id ?? "";
  const template = providerTemplate
    ?? providers.find((provider) => provider.id === initialProviderType)
    ?? null;
  const templateConnection = providerTemplateConnection(template);
  const initialProtocols = initial
    ? Object.fromEntries(
        Object.entries(initial.protocols)
          .map(([protocol, config]) => [protocol, { ...config! }]),
      ) as ModelProviderConfig["protocols"]
    : templateConnection.protocols;
  const initialSource = initial?.api_key_source
    ?? (initial?.env_key ? { kind: "env", name: initial.env_key } satisfies ApiKeySource : undefined)
    ?? (initial?.credential_saved ? { kind: "arca-store" } satisfies ApiKeySource : undefined);
  const initialAuthRequirement: ModelProviderConfig["auth_requirement"] = initial?.auth_requirement
    ?? (["ollama", "lm-studio", "vllm"].includes(initialProviderType) ? "none" : initialProviderType === "custom" ? "optional" : "required");
  const [draft, setDraft] = useState<ModelProviderConfig>({
    id: initial?.id ?? "",
    name: initial?.name ?? providerTemplate?.name ?? "",
    provider: initialProviderType,
    base_url: initial?.base_url ?? templateConnection.base_url,
    model_catalog_url: initial?.model_catalog_url,
    protocols: initialProtocols,
    auth_requirement: initialAuthRequirement,
    api_key_source: initialSource,
  });
  const defaultPortal = template?.portal;
  const [portalUrl, setPortalUrl] = useState(initial?.portal_url ?? defaultPortal?.url ?? "");
  const [portalUrlFocused, setPortalUrlFocused] = useState(false);
  const [protocolPaths, setProtocolPaths] = useState<Record<ModelProtocol, string>>(
    Object.fromEntries(
      PROTOCOLS.map(({ id }) => [
        id,
        initialProtocols[id]?.endpoint_path ?? providerTemplatePath(template, id),
      ]),
    ) as Record<ModelProtocol, string>,
  );
  const [selectedProtocol, setSelectedProtocol] = useState<ModelProtocol>(
    PROTOCOLS.find(({ id }) => Boolean(initialProtocols[id]))?.id ?? "openai-responses",
  );
  const [credential, setCredential] = useState("");
  const [credentialDirty, setCredentialDirty] = useState(false);
  const [credentialLoading, setCredentialLoading] = useState(Boolean(initial?.credential_saved));
  const [credentialReadError, setCredentialReadError] = useState<string | null>(null);
  const [credentialLoadAttempt, setCredentialLoadAttempt] = useState(0);
  const [credentialVisible, setCredentialVisible] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setCredential("");
    setCredentialDirty(false);
    setCredentialVisible(false);
    setCredentialReadError(null);
    if (!initial?.credential_saved) {
      setCredentialLoading(false);
      return;
    }
    setCredentialLoading(true);
    void revealModelProviderCredential(initial.id)
      .then((savedCredential) => {
        if (!cancelled) setCredential(savedCredential);
      })
      .catch((error) => {
        if (!cancelled) setCredentialReadError(formatError(error));
      })
      .finally(() => {
        if (!cancelled) setCredentialLoading(false);
      });
    return () => { cancelled = true; };
  }, [initial?.id, initial?.credential_saved, credentialLoadAttempt]);
  const enabledProtocols = PROTOCOLS.filter(({ id }) => Boolean(draft.protocols[id]));
  const selectedProtocolInfo = PROTOCOLS.find(({ id }) => id === selectedProtocol) ?? PROTOCOLS[0];
  const selectedProtocolPath = protocolPaths[selectedProtocol];
  const selectedProtocolPreview = fullRequestUrl(draft.base_url, selectedProtocolPath);
  const selectedProtocolEnabled = Boolean(draft.protocols[selectedProtocol]);
  const normalizedBaseUrl = normalizeBaseUrl(draft.base_url);
  const normalizedModelCatalogUrl = draft.model_catalog_url
    ? normalizeModelCatalogUrl(draft.model_catalog_url) ?? undefined
    : undefined;
  const normalizedPortalUrl = portalUrl.trim() ? normalizeProviderPortalUrl(portalUrl) : null;
  const portal = normalizedPortalUrl
    ? { url: normalizedPortalUrl, kind: defaultPortal?.kind ?? (initialAuthRequirement === "none" ? "setup" : "api-key") }
    : undefined;
  const portalCustomized = portalUrl.trim() !== (defaultPortal?.url ?? "");
  const protocolsValid = enabledProtocols.length > 0
    && enabledProtocols.every(({ id }) => {
      const path = draft.protocols[id]?.endpoint_path ?? "";
      return Boolean(normalizeEndpointPath(path) && fullRequestUrl(draft.base_url, path));
    });
  const enteredCredential = Boolean(credential.trim());
  const clearCredential = credentialDirty && !enteredCredential;
  const preservedCredential = Boolean(initial?.credential_saved && !credentialDirty);
  // A pending/failed read must never look like an editable empty credential.
  const showSavedCredentialMask = Boolean(initial?.credential_saved && (credentialLoading || credentialReadError));
  const preservesLegacySource = Boolean(initialSource && initialSource.kind !== "arca-store");
  const authWithoutCredential: ModelProviderConfig["auth_requirement"] =
    ["ollama", "lm-studio", "vllm"].includes(initialProviderType)
      ? "none"
      : initialProviderType === "custom"
        ? "optional"
        : "required";
  const sourceValid = authWithoutCredential !== "required"
    || enteredCredential
    || preservedCredential
    || preservesLegacySource
    || Boolean(initial && clearCredential);
  const valid = Boolean(
    draft.name.trim()
      && draft.provider.trim()
      && normalizedBaseUrl
      && (!draft.model_catalog_url || normalizedModelCatalogUrl)
      && (!portalUrl.trim() || normalizedPortalUrl)
      && protocolsValid
      && sourceValid
      && !busy
      && !credentialLoading
      && !credentialReadError,
  );

  const save = async () => {
    if (!valid) return;
    setBusy(true);
    try {
      const protocols = Object.fromEntries(
        Object.entries(draft.protocols)
          .filter(([, config]) => config?.endpoint_path.trim())
          .map(([protocol, config]) => [
            protocol,
            { endpoint_path: normalizeEndpointPath(config!.endpoint_path)! },
          ]),
      ) as ModelProviderConfig["protocols"];
      const hasNewCredential = credentialDirty && enteredCredential;
      const apiKeySource = hasNewCredential
        ? { kind: "arca-store" as const }
        : clearCredential
          ? undefined
          : initialSource;
      const authRequirement = hasNewCredential
        ? "required"
        : clearCredential
          ? authWithoutCredential
          : initialSource || preservedCredential
            ? initialAuthRequirement
            : authWithoutCredential;
      await onReview({
        ...draft,
        name: draft.name.trim(),
        provider: draft.provider.trim(),
        base_url: normalizedBaseUrl!,
        model_catalog_url: normalizedModelCatalogUrl,
        portal_url: normalizedPortalUrl && normalizedPortalUrl !== defaultPortal?.url
          ? normalizedPortalUrl : undefined,
        protocols,
        auth_requirement: authRequirement,
        api_key_source: authRequirement === "none" ? undefined : apiKeySource,
        env_key: undefined,
      }, authRequirement === "none"
        ? initial?.credential_saved ? "" : undefined
        : hasNewCredential ? credential : clearCredential ? "" : undefined);
    } catch (error) {
      toast.show({ kind: "error", msg: t("models.saveFailed", { error: formatError(error) }) });
    } finally {
      setBusy(false);
    }
  };

  const dialogName = initial?.name || providerTemplate?.name || draft.name || t("models.provider");

  return (
    <DialogShell
      className="arca-dialog-provider-editor"
      kind="editor"
      size="wide"
      borderRadius="10px"
      title={initial
        ? t("models.editProviderNamed", { name: dialogName })
        : t("models.addProviderNamed", { name: dialogName })}
      busy={busy}
      onClose={onClose}
      footerEnd={(
        <>
          <button type="button" className="btn-secondary" disabled={busy} onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button type="button" className="btn-primary" disabled={!valid} onClick={() => void save()}>
            {busy ? t("common.saving") : t("common.save")}
          </button>
        </>
      )}
    >
      <div className="arca-model-form arca-provider-form">
        <div className="arca-provider-basic-grid">
          <label>
            <span>{t("models.providerNameShort")}</span>
            <input
              autoFocus
              className="arca-model-field"
              value={draft.name}
              onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            />
          </label>
          <label>
            <span>{t("models.baseUrl")}</span>
            <input
              aria-label={t("models.baseUrl")}
              className="arca-model-field"
              value={draft.base_url}
              onChange={(event) => setDraft({ ...draft, base_url: event.currentTarget.value })}
              placeholder={template?.setup?.base_url_placeholder ?? "https://gateway.example.com/api/v2"}
              spellCheck={false}
            />
            {draft.base_url && !normalizedBaseUrl && <ToastNotice kind="warning" delay={500} message={t("models.invalidBaseUrl")} />}
            {template?.setup && <small>{t(`models.providerSetup.${template.setup.hint}`)}</small>}
          </label>

        </div>

        {initialAuthRequirement !== "none" && (
        <section className="arca-provider-form-section arca-provider-credential" aria-label={t("models.apiKey")}>
          <div className="arca-provider-section-head">
            <strong>{t("models.apiKey")}</strong>
            <small>{t("models.credentialHelp")}</small>
          </div>

          <div className="arca-model-form-field arca-provider-credential-field">
            <div className="arca-provider-credential-input" data-mode="arca-store">
                <input
                  type={!credentialVisible ? "password" : "text"}
                  autoComplete="new-password"
                  aria-label={t("models.apiKey")}
                  value={credential}
                  disabled={busy || credentialLoading || Boolean(credentialReadError)}
                  onChange={(event) => {
                    setCredential(event.target.value);
                    setCredentialDirty(true);
                  }}
                  placeholder={showSavedCredentialMask
                    ? "••••••••"
                    : preservesLegacySource && !clearCredential
                        ? t("models.legacyCredentialPreserved")
                        : initial ? t("models.emptyCredentialDeletes") : t("models.optionalCredential")}
                />
                  <button
                    type="button"
                    className="arca-provider-icon-button"
                    aria-label={credentialVisible ? t("models.hideApiKey") : t("models.showApiKey")}
                    title={credentialVisible ? t("models.hideApiKey") : t("models.showApiKey")}
                    disabled={busy || credentialLoading || Boolean(credentialReadError)}
                    aria-busy={credentialLoading}
                    onClick={() => setCredentialVisible((visible) => !visible)}
                  >
                    {credentialVisible
                      ? <EyeOffIcon className="w-4 h-4" />
                      : <EyeIcon className="w-4 h-4" />}
                  </button>
            </div>
          </div>

          {credentialReadError && <ToastNotice message={t("models.revealApiKeyFailed", { error: credentialReadError })}
            action={<button type="button" className="btn-ghost" onClick={() => setCredentialLoadAttempt((attempt) => attempt + 1)}>
              <RefreshIcon className="w-4 h-4" />{t("common.retry")}
            </button>} />}
        </section>
        )}

        <section className="arca-provider-form-section arca-provider-portal" aria-label={t("models.providerPortalUrl")}>
          <div className="arca-provider-section-head">
            <strong>{t("models.providerPortalUrl")}</strong>
            <small>{defaultPortal
              ? portalCustomized ? t("models.providerPortalCustom") : t("models.providerPortalDefault")
              : t("models.providerPortalOptional")}</small>
          </div>
          <div className="arca-provider-portal-row">
            <textarea
              aria-label={t("models.providerPortalUrl")}
              className="arca-model-field arca-provider-portal-url"
              rows={2}
              value={portalUrlFocused ? portalUrl : readablePortalUrl(portalUrl)}
              title={portalUrl}
              onFocus={(event) => {
                const field = event.currentTarget;
                setPortalUrlFocused(true);
                requestAnimationFrame(() => field.select());
              }}
              onBlur={() => setPortalUrlFocused(false)}
              onChange={(event) => setPortalUrl(event.currentTarget.value)}
              placeholder="https://provider.example.com/api-keys"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
            />
            <ProviderPortalButton portal={portal} />
          </div>
          {((portalUrl.trim() && !normalizedPortalUrl) || (portalCustomized && defaultPortal)) && (
          <div className="arca-provider-portal-note">
            {portalUrl.trim() && !normalizedPortalUrl && (
              <ToastNotice kind="warning" delay={500} message={t("models.invalidProviderPortalUrl")} />
            )}
            {portalCustomized && defaultPortal && (
              <button type="button" className="arca-provider-portal-reset" onClick={() => setPortalUrl(defaultPortal.url)}>
                {t("models.providerPortalReset")}
              </button>
            )}
          </div>
          )}
        </section>

        <section className="arca-provider-form-section">
          <label className="arca-provider-model-catalog-field">
            <span>{t("models.modelCatalogUrl")}</span>
            <input
              aria-label={t("models.modelCatalogUrl")}
              className="arca-model-field"
              value={draft.model_catalog_url ?? ""}
              onChange={(event) => setDraft({
                ...draft,
                model_catalog_url: event.currentTarget.value || undefined,
              })}
              placeholder="https://gateway.example.com/v1/models"
              spellCheck={false}
            />
            {draft.model_catalog_url && !normalizedModelCatalogUrl && (
              <ToastNotice kind="warning" delay={500} message={t("models.invalidModelCatalogUrl")} />
            )}
          </label>
        </section>
        <section className="arca-provider-form-section arca-provider-protocols" aria-label={t("models.supportedProtocols")}>
          <div className="arca-provider-section-head">
            <strong>{t("models.protocolsShort")}</strong>
            {enabledProtocols.length === 0 && (
              <ToastNotice kind="warning" delay={500} message={t("models.protocolRequired")} />
            )}
          </div>
          <div className="arca-provider-protocol-list">
            {PROTOCOLS.map((protocol) => {
              const enabled = Boolean(draft.protocols[protocol.id]);
              const path = protocolPaths[protocol.id];
              const pathSummary = normalizeEndpointPath(path) ?? "—";
              const selected = selectedProtocol === protocol.id;
              return (
                <article
                  className="arca-provider-protocol"
                  data-enabled={enabled ? "true" : undefined}
                  data-selected={selected ? "true" : undefined}
                  key={protocol.id}
                >
                  <button
                    type="button"
                    className="arca-provider-protocol-trigger"
                    aria-pressed={selected}
                    onClick={() => setSelectedProtocol(protocol.id)}
                  >
                    <span className="arca-provider-protocol-signal">
                      <span className="arca-model-protocol-dot" data-protocol={protocol.id} />
                    </span>
                    <span className="arca-provider-protocol-copy">
                      <strong>{protocol.label}</strong>
                      <code className="arca-provider-protocol-path" title={pathSummary}>{pathSummary}</code>
                    </span>
                  </button>
                  <label className="arca-provider-protocol-toggle">
                    <input
                      aria-label={protocol.label}
                      className="arca-provider-protocol-switch-input"
                      type="checkbox"
                      role="switch"
                      checked={enabled}
                      onChange={(event) => {
                        const checked = event.target.checked;
                        setSelectedProtocol(protocol.id);
                        setDraft((current) => {
                          const protocols = { ...current.protocols };
                          if (checked) protocols[protocol.id] = { endpoint_path: protocolPaths[protocol.id] };
                          else delete protocols[protocol.id];
                          return { ...current, protocols };
                        });
                      }}
                    />
                    <span className="arca-provider-protocol-switch" aria-hidden="true" />
                  </label>
                </article>
              );
            })}
          </div>
          <div
            className="arca-provider-protocol-editor"
            data-enabled={selectedProtocolEnabled ? "true" : undefined}
          >
            <div
              className="arca-provider-route-builder"
              data-enabled={selectedProtocolEnabled ? "true" : undefined}
              data-invalid={selectedProtocolPath && !normalizeEndpointPath(selectedProtocolPath) ? "true" : undefined}
            >
              <input
                aria-label={`${selectedProtocolInfo.label} ${t("models.endpointPath")}`}
                className="arca-provider-route-path"
                value={selectedProtocolPath}
                onChange={(event) => {
                  const endpointPath = event.currentTarget.value;
                  setProtocolPaths((current) => ({ ...current, [selectedProtocol]: endpointPath }));
                  if (selectedProtocolEnabled) {
                    setDraft((current) => ({
                      ...current,
                      protocols: {
                        ...current.protocols,
                        [selectedProtocol]: { endpoint_path: endpointPath },
                      },
                    }));
                  }
                }}
                placeholder={DEFAULT_ENDPOINT_PATHS[selectedProtocol]}
                spellCheck={false}
              />
              <output
                aria-label={t("models.fullRequestUrl")}
                className="arca-provider-route-preview"
                data-empty={!selectedProtocolPreview ? "true" : undefined}
                title={selectedProtocolPreview || undefined}
              >
                {selectedProtocolPreview || "—"}
              </output>
            </div>
            {selectedProtocolEnabled && selectedProtocolPath && !normalizeEndpointPath(selectedProtocolPath) && (
              <ToastNotice kind="warning" delay={500} message={t("models.invalidEndpointPath")} />
            )}
          </div>
        </section>
      </div>
    </DialogShell>
  );
}
