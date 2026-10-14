import { ToastNotice } from "./Toast";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatError } from "../lib/format";
import { loadProviderModelCatalog, providerCatalogKey } from "../lib/providerModelCatalog";
import type { ModelProfileView, ModelProtocol, ModelProviderInstanceView, ProviderModelSummary } from "../lib/types";
import { FormSelect, type FormSelectOption } from "./FormSelect";
import { PlusIcon, RefreshIcon } from "./icons";
import { ProviderGlyph } from "./providerIcons";

const PAGE_SIZE = 8;

export function ManualModelCard({ disabled, onAdd }: { disabled: boolean; onAdd(): void }) {
  const { t } = useTranslation();
  return (
    <div role="listitem">
      <button type="button" className="arca-model-manual-card" onClick={onAdd} disabled={disabled}>
        <span className="arca-model-manual-icon"><PlusIcon className="w-5 h-5" /></span>
        <span><strong>{t("models.manualAdd")}</strong><small>{t("models.manualAddHint")}</small></span>
      </button>
    </div>
  );
}

export function ProviderModelCatalog({
  provider, revision, profiles, query, protocols, disabled, addingId, onAdd,
}: {
  provider: ModelProviderInstanceView;
  revision: number;
  profiles: ModelProfileView[];
  query: string;
  protocols: FormSelectOption[];
  disabled: boolean;
  addingId: string | null;
  onAdd(model: ProviderModelSummary, protocol: ModelProtocol): Promise<boolean>;
}) {
  const { t } = useTranslation();
  const [models, setModels] = useState<ProviderModelSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [limit, setLimit] = useState(PAGE_SIZE);
  const requestGeneration = useRef(0);
  const previousConnectionKey = useRef<string | null>(null);
  const providerRef = useRef(provider);
  providerRef.current = provider;
  const catalogKey = providerCatalogKey(provider, revision);
  const connectionKey = providerCatalogKey(provider, 0);

  const load = useCallback(async (force = false) => {
    const generation = ++requestGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const result = await loadProviderModelCatalog(providerRef.current, revision, force);
      if (requestGeneration.current === generation) setModels(result);
    } catch (cause) {
      if (requestGeneration.current === generation) setError(formatError(cause));
    } finally {
      if (requestGeneration.current === generation) setLoading(false);
    }
  }, [catalogKey, revision]);

  useEffect(() => {
    if (previousConnectionKey.current !== connectionKey) setModels([]);
    previousConnectionKey.current = connectionKey;
    void load();
    return () => { requestGeneration.current += 1; };
  }, [load, connectionKey]);

  const existingIds = useMemo(() => new Set(
    profiles.filter((profile) => profile.provider_id === provider.id).map((profile) => profile.model),
  ), [profiles, provider.id]);
  const available = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return models.filter((model) => !existingIds.has(model.id)
      && (!needle || `${model.id} ${model.name ?? ""}`.toLocaleLowerCase().includes(needle)));
  }, [models, existingIds, query]);

  return (
    <section className="arca-model-catalog-section" aria-label={t("models.availableModels")}>
      <div className="arca-model-section-heading">
        <h2>{t("models.availableModels")} <span>{loading && models.length === 0 ? "…" : available.length}</span></h2>
        <button type="button" className="btn-ghost" disabled={loading} aria-busy={loading}
          title={t("models.refreshModelCatalog")} onClick={() => void load(true)}>
          <RefreshIcon className="w-3.5 h-3.5" />
          {t("models.refreshModelCatalog")}
        </button>
      </div>
      {loading && <p className="arca-model-catalog-notice" role="status">{t("models.loadingModelCatalog")}</p>}
      {error && <ToastNotice message={error.includes("credential_missing")
          ? t("models.discoveryNeedsCredential")
          : error.includes("model_discovery_endpoint_invalid") ? t("models.discoveryNeedsEndpoint")
          : t("models.discoveryError", { error })} />}
      {!loading && !error && available.length === 0 && <p className="arca-model-catalog-notice">
        {query.trim() ? t("models.noModelCatalogMatches") : models.length > 0
          ? t("models.catalogAllAdded") : t("models.catalogEmpty")}
      </p>}
      {available.length > 0 && <div className="arca-asset-list arca-model-candidate-list" role="list" aria-label={t("models.availableModels")}>
        {available.slice(0, limit).map((model) => <CandidateModelCard key={model.id}
          model={model} provider={provider} protocols={protocols}
          disabled={disabled || addingId !== null || loading}
          busy={addingId === model.id}
          onAdd={async (protocol) => { await onAdd(model, protocol); }} />)}
      </div>}
      {available.length > limit && <button type="button" className="arca-model-catalog-more btn-ghost"
        onClick={() => setLimit((current) => current + PAGE_SIZE)}>
        {t("models.showMoreAvailable", { count: Math.min(PAGE_SIZE, available.length - limit) })}
      </button>}
    </section>
  );
}

function CandidateModelCard({ model, provider, protocols, disabled, busy, onAdd }: {
  model: ProviderModelSummary;
  provider: ModelProviderInstanceView;
  protocols: FormSelectOption[];
  disabled: boolean;
  busy: boolean;
  onAdd(protocol: ModelProtocol): Promise<void>;
}) {
  const { t } = useTranslation();
  const [chosenProtocol, setChosenProtocol] = useState("");
  const [error, setError] = useState<string | null>(null);
  const protocol = protocols.length === 1 ? protocols[0].value : chosenProtocol;
  const validProtocol = protocols.some((option) => option.value === protocol);
  const add = async () => {
    if (!validProtocol || disabled) return;
    setError(null);
    try { await onAdd(protocol as ModelProtocol); }
    catch (cause) { setError(formatError(cause)); }
  };
  return (
    <article className="arca-model-candidate-card" role="listitem" aria-label={t("models.catalogCandidate", { name: model.name || model.id })}>
      <div className="arca-asset-list-identity">
        <ProviderGlyph id={provider.provider} name={provider.name} size={30} />
        <span className="arca-asset-list-copy">
          <strong title={model.name || model.id}>{model.name || model.id}</strong>
          <code title={model.id}>{model.id}</code>
        </span>
      </div>
      <div className="arca-model-candidate-facts">
        <span>{t("models.catalogCandidateLabel")}</span>
        {model.context_length != null && model.context_length > 0 && <span>
          {t("models.context")} {new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(model.context_length)}
        </span>}
      </div>
      <div className="arca-model-candidate-controls">
        {protocols.length > 1 ? <FormSelect ariaLabel={`${t("models.protocol")} · ${model.id}`}
          value={validProtocol ? protocol : ""} options={protocols} placeholder={t("models.chooseProtocol")}
          disabled={disabled} onChange={setChosenProtocol} />
          : <span>{protocols[0]?.label ?? t("models.protocolRequired")}</span>}
        <button type="button" className="btn-secondary" disabled={disabled || !validProtocol} aria-busy={busy}
          aria-label={t("models.addCatalogModel", { name: model.name || model.id })} onClick={() => void add()}>
          <PlusIcon className="w-3.5 h-3.5" />
          {busy ? t("models.addingAction") : t("models.addAction")}
        </button>
      </div>
      <ToastNotice message={error} />
    </article>
  );
}
