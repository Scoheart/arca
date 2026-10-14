import { ResourceIcon } from "./resourcePresentation";
import { useState, useMemo, useCallback, useEffect, useRef, useDeferredValue } from "react";
import { useTranslation } from "react-i18next";
import type { InstallState } from "../hooks/useInstallState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type { McpIconPreference, RegistryEntry, RegistryOrigin, CatalogItem, ResourceNavigationIntent } from "../lib/types";
import { keyOf, transportOf } from "../lib/mcp";
import { assetIdentity, observedAgentIndex } from "../lib/consumption";
import { exportEffectiveDialog } from "../lib/api";
import { useMcpIconPreferences } from "../hooks/useMcpIconPreferences";
import { formatError } from "../lib/format";
import { redactSensitiveConfig } from "../lib/resourceWorkspace";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SourcesSidebar } from "./SourcesSidebar";
import { AgentGlyph, agentName } from "./brandIcons";
import {
  CopyIcon,
  EditIcon,
  LinkIcon,
  CloudIcon,
  DownloadIcon,
  FolderIcon,
  LayersIcon,
  NetworkIcon,
  SparklesIcon,
  TerminalIcon,
  TrashIcon,
} from "./icons";
import { Badge, IconButton, TransportPill } from "./ui";
import { McpAvatar } from "./McpIcon";
import { McpIconPickerDialog } from "./McpIconPickerDialog";
import { ResourceState } from "./ResourceState";
import { ToastNotice, useToast } from "./Toast";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";
import { RegistryEditPage } from "./RegistryEditPage";
import {
  InspectorField,
  InspectorSection,
  ResourceInspector,
  ResourceOverview,
  ResourceWorkspace,
} from "./ResourceWorkspace";

interface RegistryViewProps {
  state: InstallState;
  consumptionState?: ConsumptionState;
  intent?: Extract<ResourceNavigationIntent, { domain: "mcp" }>;
  onIntentConsumed?(id: number): void;
  onCreate: () => void;
  suppressOperationReview?: boolean;
  onRetryLoad?(): Promise<void>;
  retryLoadDisabled?: boolean;
}

/** Origin buckets — still used to decide which entries are user-deletable. */
type OriginBucket = "remote" | "local" | "manual" | "discovered";
/** Classify an entry's origin into a bucket. Entries with no origin, or a
 *  legacy/unknown kind, fall into "discovered" (scanned-from-machine). */
function bucketOf(entry: RegistryEntry): OriginBucket {
  const k = entry.origin?.kind;
  if (k === "remote") return "remote";
  if (k === "local") return "local";
  if (k === "manual") return "manual";
  return "discovered";
}

/** User-owned entries (手动添加 / 自动探索) can be deleted here.
 *  Source-owned entries can still be edited: saving creates a higher-priority
 *  manual override while leaving the subscribed/local source untouched. */
function isUserOwned(entry: RegistryEntry): boolean {
  const b = bucketOf(entry);
  return b === "manual" || b === "discovered";
}

/** Does `entry` belong to the sidebar-selected source? Managed sources match by
 * origin kind ("manual" / "discovered"); remote/local match by origin.source id. */
function inSource(entry: RegistryEntry, sourceId: string): boolean {
  if (sourceId === "manual") return entry.origin?.kind === "manual";
  if (sourceId === "discovered") return entry.origin?.kind === "discovered";
  return entry.origin?.source === sourceId;
}

function endpointOf(entry: RegistryEntry): { text: string; link: boolean } {
  if (entry.config.http?.url) return { text: entry.config.http.url, link: true };
  if (entry.config.stdio) {
    const { command, args } = entry.config.stdio;
    return { text: [command, ...(args ?? [])].join(" "), link: false };
  }
  return { text: entry.description || "—", link: false };
}

/** Provenance indicator: 订阅:X (remote source) / 本地:X (local source) / 手动添加 /
 *  来自 {agent} (discovered). There is no built-in bucket anymore. */
function OriginTag({
  entry,
  installedAgents,
  sourceName,
}: {
  entry: RegistryEntry;
  installedAgents: string[];
  sourceName: (id: string) => string;
}) {
  const origin = entry.origin;
  if (origin?.kind === "remote") {
    return (
      <span className="inline-flex items-center gap-1 min-w-0" title={`订阅：${origin.source ? sourceName(origin.source) : ""}`}>
        <CloudIcon className="w-3.5 h-3.5 flex-shrink-0" style={{ color: "var(--color-blue)" }} />
        <span className="text-[11px] truncate" style={{ color: "var(--text-secondary)" }}>
          {origin.source ? sourceName(origin.source) : "订阅"}
        </span>
      </span>
    );
  }
  if (origin?.kind === "local") {
    return (
      <span className="inline-flex items-center gap-1 min-w-0" title={`本地：${origin.source ? sourceName(origin.source) : ""}`}>
        <FolderIcon className="w-3.5 h-3.5 flex-shrink-0" style={{ color: "var(--text-secondary)" }} />
        <span className="text-[11px] truncate" style={{ color: "var(--text-secondary)" }}>
          {origin.source ? sourceName(origin.source) : "本地"}
        </span>
      </span>
    );
  }
  if (origin?.kind === "manual") return <Badge tone="info">手动</Badge>;
  // discovered (recorded) or legacy custom: show the source app when known.
  const agent = origin?.agent ?? installedAgents[0];
  if (agent) {
    return (
      <span className="inline-flex items-center gap-1 min-w-0" title={`来自 ${agentName(agent)}`}>
        <span className="flex-shrink-0 inline-flex"><AgentGlyph id={agent} size={16} /></span>
        <span className="text-[11px] truncate" style={{ color: "var(--text-secondary)" }}>
          {agentName(agent)}
        </span>
      </span>
    );
  }
  return <Badge tone="neutral">探索</Badge>;
}

/** Short human label for an origin, for the "被『X』覆盖" tooltip. */
function originLabel(origin: RegistryOrigin | undefined, sourceName: (id: string) => string): string {
  if (!origin) return "其它来源";
  if (origin.kind === "manual") return "手动添加";
  if (origin.kind === "discovered") return origin.agent ? agentName(origin.agent) : "自动探索";
  const label = origin.source ? sourceName(origin.source) : "";
  return label || (origin.kind === "remote" ? "订阅" : "本地");
}

function sourceAddress(url: string): string {
  try {
    const parsed = new URL(url);
    // Subscription credentials can live in userinfo or query parameters.
    return `${parsed.origin}${parsed.pathname}${parsed.search ? "?…" : ""}`;
  } catch {
    return "无效来源地址";
  }
}

export function RegistryView({ state, consumptionState, intent, onIntentConsumed, onCreate, suppressOperationReview = false, onRetryLoad, retryLoadDisabled = false }: RegistryViewProps) {
  const { t, i18n } = useTranslation();
  const { catalog, entries, sources } = state;
  const toast = useToast();
  const mcpIcons = useMcpIconPreferences();

  const [q, setQ] = useState("");
  const deferredQuery = useDeferredValue(q);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [detail, setDetail] = useState<CatalogItem | null>(null);
  const [editingDetail, setEditingDetail] = useState(false);
  const [iconPickerEntry, setIconPickerEntry] = useState<RegistryEntry | null>(null);
  const lastConsumedIntentId = useRef<number | null>(null);
  const observedAgents = useMemo(
    () => observedAgentIndex(consumptionState?.inventory ?? null, "mcp"),
    [consumptionState?.inventory],
  );
  const agentsForServer = useCallback((key: string) =>
    observedAgents.get(assetIdentity({ domain: "mcp", key })) ?? [], [observedAgents]);

  const sourceName = useCallback(
    (id: string) => sources.find((s) => s.id === id)?.name ?? id,
    [sources]
  );

  // For each composite key, the origin of the in-effect (winning) copy — used to
  // tell an overridden card which source actually takes effect.
  const winningOriginByKey = useMemo(() => {
    const m = new Map<string, RegistryOrigin | undefined>();
    for (const item of catalog) {
      if (item.in_effect) m.set(keyOf(item.entry), item.entry.origin);
    }
    return m;
  }, [catalog]);

  const sourceScoped = useMemo(() => {
    if (selectedSource === null) return catalog;
    return catalog.filter((item) => inSource(item.entry, selectedSource));
  }, [catalog, selectedSource]);
  const activeSource = sources.find((source) => source.id === selectedSource);
  const sourceLocation = activeSource?.url ? sourceAddress(activeSource.url) : activeSource?.path;

  const searchIndex = useMemo(() => {
    const collator = new Intl.Collator(undefined, { sensitivity: "base" });
    // Alphabetical by name, then transport; in-effect copy first within a group.
    return [...sourceScoped].sort(
      (a, b) =>
        collator.compare(a.entry.name, b.entry.name) ||
        transportOf(a.entry).localeCompare(transportOf(b.entry)) ||
        Number(b.in_effect) - Number(a.in_effect)
    ).map((item) => ({ item, text: `${item.entry.name}\n${item.entry.description}`.toLowerCase() }));
  }, [sourceScoped]);

  const filtered = useMemo(() => {
    const needle = deferredQuery.trim().toLowerCase();
    return searchIndex.filter((row) => !needle || row.text.includes(needle)).map((row) => row.item);
  }, [deferredQuery, searchIndex]);
  const cards = useMemo(() => filtered.map((item) => (
    <div role="listitem" key={`${keyOf(item.entry)}@${item.entry.origin?.source ?? item.entry.origin?.kind ?? ""}`}>
      <RegistryCard item={item} iconPreference={mcpIcons.preferences[keyOf(item.entry)]}
        selected={detail === item} installedAgents={agentsForServer(keyOf(item.entry))}
        sourceName={sourceName} onOpen={() => { setEditingDetail(false); setDetail(item); }} />
    </div>
  )), [filtered, mcpIcons.preferences, detail, agentsForServer, sourceName]);

  useEffect(() => {
    if (!intent || state.loading || lastConsumedIntentId.current === intent.id) return;
    lastConsumedIntentId.current = intent.id;
    if (intent.kind === "create") {
      setDetail(null);
      setEditingDetail(false);
      onCreate();
      onIntentConsumed?.(intent.id);
      return;
    }
    const item = catalog.find(
      (candidate) =>
        candidate.entry.name === intent.name &&
        transportOf(candidate.entry) === intent.transport &&
        candidate.in_effect,
    ) ?? catalog.find(
      (candidate) => candidate.entry.name === intent.name && transportOf(candidate.entry) === intent.transport,
    );
    setQ("");
    setSelectedSource(null);
    setDetail(item ?? null);
    setEditingDetail(false);
    if (!item) toast.show({ kind: "error", msg: `未找到 MCP“${intent.name}”。` });
    onIntentConsumed?.(intent.id);
  }, [catalog, intent, onCreate, onIntentConsumed, state.loading, toast]);

  const changeQuery = (value: string) => {
    setDetail(null);
    setEditingDetail(false);
    setQ(value);
  };

  const changeSource = (sourceId: string | null) => {
    setDetail(null);
    setEditingDetail(false);
    setSelectedSource(sourceId);
  };

  const closeDetail = useCallback(() => {
    setDetail(null);
    setEditingDetail(false);
  }, []);

  useEffect(() => {
    if (!detail || state.loading || state.registryError) return;
    const stillExists = catalog.some((item) => keyOf(item.entry) === keyOf(detail.entry)
      && item.entry.origin?.kind === detail.entry.origin?.kind
      && item.entry.origin?.source === detail.entry.origin?.source);
    if (!stillExists) closeDetail();
  }, [catalog, detail, state.loading, state.registryError, closeDetail]);

  const copyConfig = useCallback(
    (entry: RegistryEntry) => {
      navigator.clipboard
        .writeText(JSON.stringify({
          mcpServers: { [entry.name]: entry.config.stdio ?? entry.config.http },
        }, null, 2))
        .then(() => toast.show({ kind: "success", msg: `已复制 ${entry.name} 配置` }))
        .catch(() => toast.show({ kind: "error", msg: "复制失败" }));
    },
    [toast]
  );

  const doExport = useCallback(async () => {
    try {
      const path = await exportEffectiveDialog();
      if (path) toast.show({ kind: "success", msg: `已导出 ${entries.length} 项 → ${path}` });
    } catch (e) {
      toast.show({ kind: "error", msg: "导出失败：" + formatError(e) });
    }
  }, [entries.length, toast]);

  // Every catalog copy can be edited. Source-owned edits are materialized as a
  // manual override by the central lifecycle planner; deletion remains limited
  // to user-owned copies.
  const deletable = useCallback((entry: RegistryEntry) => isUserOwned(entry), []);

  const deleteEntry = useCallback(
    async (entry: RegistryEntry) => {
      if (!deletable(entry) || !consumptionState) return;
      try {
        const sourceId = entry.origin?.source ?? entry.origin?.kind;
        await consumptionState.planDelete(
          { domain: "mcp", key: keyOf(entry) },
          sourceId,
        );
      } catch (e) {
        toast.show({ kind: "error", msg: `无法删除：${String(e)}` });
      }
    },
    [consumptionState, deletable, toast]
  );

  const reviewingMcpDeletion = !editingDetail && !suppressOperationReview
    && consumptionState?.plan?.kind === "delete-asset"
    && consumptionState.plan.domain_plan.domain === "mcp";

  return (
    <div className="arca-registry-workspace">
      <ResourceWorkspace
      overview={
        <ResourceOverview
          eyebrow={activeSource ? `来源 · ${sourceScoped.length} MCPs` : `${catalog.length} MCPs · ${sources.length} 个来源`}
          title={activeSource?.name ?? "全部 MCPs"}
          description={activeSource
            ? activeSource.kind === "remote" ? "订阅来源" : activeSource.managed ? "Arca 管理的来源" : "本地配置来源"
            : "集中管理可复用的 MCP 连接、来源与配置"}
          icon={activeSource?.kind === "remote"
            ? <CloudIcon className="w-6 h-6" />
            : <ResourceIcon domain="mcp" className="w-6 h-6" />}
        >
          {activeSource && <>
            <dl className="arca-resource-overview-facts">
              {(activeSource.url || activeSource.path) && <div className="arca-resource-overview-location">
                <dt>{activeSource.url ? "来源地址" : "配置路径"}</dt>
                <dd><code title={sourceLocation ?? undefined}>{sourceLocation}</code></dd>
              </div>}
              <div>
                <dt>同步状态</dt>
                <dd>{activeSource.error ? "同步失败" : !activeSource.enabled ? "已停用" : activeSource.synced_at
                  ? `上次同步 ${new Date(activeSource.synced_at).toLocaleString(i18n.resolvedLanguage)}`
                  : activeSource.managed ? "自动管理" : "尚未同步"}</dd>
              </div>
            </dl>
          </>}
        </ResourceOverview>
      }
      sidebar={
        <SourcesSidebar
          state={state}
          selectedId={selectedSource}
          onSelect={changeSource}
          onCreate={() => {
            closeDetail();
            onCreate();
          }}
          footer={<button className="btn-ghost" type="button" onClick={doExport} disabled={entries.length === 0}>
            <DownloadIcon className="w-4 h-4" />
            导出生效配置
          </button>}
        />
      }
      query={q}
      onQueryChange={changeQuery}
      searchPlaceholder={activeSource ? `搜索 ${activeSource.name} 中的 MCPs` : "搜索 MCPs"}
      listLabel="MCPs"
      resultCount={filtered.length}
      inspector={
        reviewingMcpDeletion ? undefined : detail && editingDetail && consumptionState ? (
          <RegistryEditPage
            state={state}
            consumptionState={consumptionState}
            name={detail.entry.name}
            entry={detail.entry}
            iconPreference={mcpIcons.preferences[keyOf(detail.entry)]}
            transport={transportOf(detail.entry)}
            presentation="inspector"
            onBack={closeDetail}
          />
        ) : detail ? (
          <RegistryDetail
            entry={detail.entry}
            iconPreference={mcpIcons.preferences[keyOf(detail.entry)]}
            overriddenBy={
              detail.in_effect
                ? undefined
                : originLabel(winningOriginByKey.get(keyOf(detail.entry)), sourceName)
            }
            installedAgents={agentsForServer(keyOf(detail.entry))}
            sourceName={sourceName}
            onClose={closeDetail}
            onCopy={() => copyConfig(detail.entry)}
            onCustomizeIcon={() => setIconPickerEntry(detail.entry)}
            onEdit={
              consumptionState
                ? () => setEditingDetail(true)
                : undefined
            }
            onDelete={deletable(detail.entry) && detail.in_effect ? () => void deleteEntry(detail.entry) : undefined}
          />
        ) : undefined
      }
      onInspectorClose={closeDetail}
    >
      {consumptionState?.plan && !editingDetail && !suppressOperationReview ? (
        <AssetOperationReviewDialog
          plan={consumptionState.plan}
          assetDisplayNames={detail ? { [`mcp:${keyOf(detail.entry)}`]: detail.entry.name } : undefined}
          agentDisplayNames={Object.fromEntries(consumptionState.plan.affected_agent_ids.map((id) => [id, agentName(id)]))}
          busy={consumptionState.committing}
          error={consumptionState.error}
          onCancel={consumptionState.cancel}
          onCommit={async () => {
            const kind = consumptionState.plan?.kind;
            let pendingConvergence = false;
            try {
              await consumptionState.commit();
            } catch (error) {
              if (typeof error !== "object" || error === null
                || !("code" in error) || error.code !== "pending_convergence") {
                // A failed central commit keeps the review and its error visible.
                return;
              }
              // Core committed the central deletion; only Agent projection is pending.
              pendingConvergence = true;
            }
            if (kind === "delete-asset") {
              closeDetail();
              setIconPickerEntry(null);
            }
            void state.refreshRegistry().catch((error) => {
              toast.show({ kind: "error", msg: `操作已完成，但列表刷新失败：${formatError(error)}` });
            });
            toast.show({
              kind: pendingConvergence ? "error" : "success",
              msg: pendingConvergence
                ? kind === "delete-asset"
                  ? "MCP 已从中央库删除，部分 Agent 尚未同步完成，请在对应 Agent 页面处理。"
                  : "MCP 已保存，部分 Agent 尚未同步完成，请在对应 Agent 页面处理。"
                : kind === "delete-asset" ? "MCP 资产已删除。" : "MCP 资产已保存。",
            });
          }}
        />
      ) : null}
      {state.loading ? (
        <ResourceState kind="loading" title="正在读取 MCP…" />
      ) : state.registryError && catalog.length === 0 ? (
        <ResourceState
          kind="read-error"
          icon={<ResourceIcon domain="mcp" className="w-6 h-6" />}
          title="读取 MCP 失败"
          detail={state.registryError}
          action={(
            <button
              type="button"
              className="btn-secondary"
              disabled={retryLoadDisabled}
              onClick={() => void (onRetryLoad?.() ?? state.refreshRegistry()).catch(() => undefined)}
            >
              重试
            </button>
          )}
        />
      ) : filtered.length === 0 ? (
        <ResourceState
          kind={catalog.length === 0 ? "empty" : "no-match"}
          icon={<ResourceIcon domain="mcp" className="w-6 h-6" />}
          title={catalog.length === 0 ? "暂无 MCPs" : "没有匹配项"}
          detail={catalog.length === 0 ? "添加订阅、导入配置或新建 MCP" : "调整搜索或来源筛选后重试。"}
          action={catalog.length === 0 ? undefined : (
            <button type="button" className="btn-secondary" onClick={() => {
              setQ("");
              setSelectedSource(null);
            }}>清除筛选</button>
          )}
        />
      ) : (
        <div
          className="arca-asset-list arca-mcp-list"
          role="list"
          aria-label={t("centralAssets.mcpList")}
        >
          {cards}
        </div>
      )}

      </ResourceWorkspace>
      {iconPickerEntry && (
        <McpIconPickerDialog
          assetKey={keyOf(iconPickerEntry)}
          entry={iconPickerEntry}
          preference={mcpIcons.preferences[keyOf(iconPickerEntry)]}
          onSelectBuiltin={(iconId) => mcpIcons.selectBuiltin(keyOf(iconPickerEntry), iconId)}
          onUpload={() => mcpIcons.upload(keyOf(iconPickerEntry))}
          onReset={() => mcpIcons.reset(keyOf(iconPickerEntry))}
          onClose={() => setIconPickerEntry(null)}
        />
      )}
    </div>
  );
}

function RegistryCard({
  item,
  iconPreference,
  selected,
  installedAgents,
  sourceName,
  onOpen,
}: {
  item: CatalogItem;
  iconPreference?: McpIconPreference;
  selected: boolean;
  installedAgents: string[];
  sourceName: (id: string) => string;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const { entry } = item;
  const ep = endpointOf(entry);
  const transport = transportOf(entry).toUpperCase();
  const discoveredAgent = entry.origin?.kind === "discovered" ? entry.origin.agent ?? installedAgents[0] : undefined;
  const source = discoveredAgent
    ? agentName(discoveredAgent)
    : originLabel(entry.origin, sourceName);

  return (
    <button
      type="button"
      className="arca-asset-list-row arca-mcp-list-row"
      data-selected={selected ? "true" : undefined}
      data-attention={item.in_effect ? undefined : "shadowed"}
      aria-label={`${t("centralAssets.openMcpDetails", { name: entry.name })} · ${transport} · ${source}`}
      aria-pressed={selected}
      onClick={onOpen}
    >
      <span className="arca-asset-list-identity">
        <McpAvatar assetKey={keyOf(entry)} entry={entry} preference={iconPreference} />
        <span className="arca-asset-list-copy">
          <strong title={entry.name}>{entry.name}</strong>
          <code title={ep.text}>{ep.text}</code>
        </span>
      </span>
      <span className="arca-asset-list-transport">{transport}</span>
      <span className="arca-asset-list-source" title={source}>{source}</span>
      <span className="arca-asset-list-status">
        <Badge tone={item.in_effect ? "success" : "info"}>
          {item.in_effect
            ? t("centralAssets.effective")
            : t("centralAssets.shadowed")}
        </Badge>
      </span>
    </button>
  );
}

function RegistryDetail({
  entry,
  iconPreference,
  overriddenBy,
  installedAgents,
  sourceName,
  onClose,
  onCopy,
  onCustomizeIcon,
  onEdit,
  onDelete,
}: {
  entry: RegistryEntry;
  iconPreference?: McpIconPreference;
  overriddenBy?: string;
  installedAgents: string[];
  sourceName: (id: string) => string;
  onClose: () => void;
  onCopy: () => void;
  onCustomizeIcon: () => void;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  const { t } = useTranslation();
  const endpoint = endpointOf(entry);
  return (
    <ResourceInspector
      title={entry.name}
      avatar={<McpAvatar assetKey={keyOf(entry)} entry={entry} preference={iconPreference} size={40} />}
      subtitle={
        <div className="flex items-center gap-1.5">
          <TransportPill entry={entry} />
          <OriginTag entry={entry} installedAgents={installedAgents} sourceName={sourceName} />
        </div>
      }
      onClose={onClose}
      footer={
        <>
          {onDelete && (
            <button onClick={onDelete} className="btn-danger" title="删除中央 MCP 资产">
              <TrashIcon className="w-4 h-4" />
              删除
            </button>
          )}
          <div className="flex-1" />
          <IconButton onClick={onCustomizeIcon} title={t("mcpIcons.action")}>
            <SparklesIcon className="w-4 h-4" />
          </IconButton>
          <IconButton onClick={onCopy} title="复制配置">
            <CopyIcon className="w-4 h-4" />
          </IconButton>
          {onEdit && (
            <button onClick={onEdit} className="btn-primary">
              <EditIcon className="w-4 h-4" />
              编辑
            </button>
          )}
        </>
      }
    >
      {overriddenBy && <ToastNotice kind="info" message={`已被覆盖 · 当前使用「${overriddenBy}」，此副本不参与安装。`} />}

      {entry.description && <p className="arca-inspector-description">{entry.description}</p>}

      <InspectorSection title="连接" icon={<LinkIcon className="w-4 h-4" />}>
        <InspectorField icon={<NetworkIcon className="w-4 h-4" />} label="地址" mono>{endpoint.text}</InspectorField>
        {entry.repo && (
          <InspectorField icon={<LinkIcon className="w-4 h-4" />} label="主页">
            <button onClick={() => openUrl(entry.repo!)} className="arca-inline-link" title="在浏览器中打开">
              <LinkIcon className="w-3.5 h-3.5" />
              {entry.repo}
            </button>
          </InspectorField>
        )}
      </InspectorSection>

      {entry.tags.length > 0 && (
        <InspectorSection title="标签" icon={<LayersIcon className="w-4 h-4" />}>
          <div className="flex flex-wrap gap-1.5">
            {entry.tags.map((tag) => <Badge key={tag} tone="info">{tag}</Badge>)}
          </div>
        </InspectorSection>
      )}

      <InspectorSection title="配置" icon={<TerminalIcon className="w-4 h-4" />}>
        <pre className="arca-config-preview">
          {JSON.stringify(redactSensitiveConfig(entry.config), null, 2)}
        </pre>
      </InspectorSection>
    </ResourceInspector>
  );
}
