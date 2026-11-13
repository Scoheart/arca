import { ResourceIcon } from "./resourcePresentation";
import { useState, useMemo, useCallback, useEffect, useRef, useDeferredValue } from "react";
import { useTranslation } from "react-i18next";
import type { InstallState } from "../hooks/useInstallState";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import type { McpIconPreference, RegistryEntry, RegistryOrigin, CatalogItem, ResourceNavigationIntent } from "../lib/types";
import { keyOf, resolveMcpTags, transportOf } from "../lib/mcp";
import { assetIdentity, observedAgentIndex } from "../lib/consumption";
import { exportEffectiveDialog } from "../lib/api";
import { useMcpIconPreferences } from "../hooks/useMcpIconPreferences";
import { useMcpTags } from "../hooks/useMcpTags";
import { formatError } from "../lib/format";
import { redactSensitiveConfig } from "../lib/resourceWorkspace";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SourcesSidebar, type McpTagSummary } from "./SourcesSidebar";
import { AgentGlyph, agentName } from "./brandIcons";
import {
  CopyIcon,
  EditIcon,
  LinkIcon,
  CloudIcon,
  DownloadIcon,
  FolderIcon,
  NetworkIcon,
  SparklesIcon,
  TagIcon,
  TerminalIcon,
  TrashIcon,
} from "./icons";
import { Badge, IconButton, TransportPill } from "./ui";
import { McpAvatar } from "./McpIcon";
import { McpIconPickerDialog } from "./McpIconPickerDialog";
import { McpTagInput, McpTagManagerDialog, McpTagPickerDialog } from "./McpTagEditor";
import { ResourceState } from "./ResourceState";
import { useToast } from "./Toast";
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
        <CloudIcon className="w-3.5 h-3.5 flex-shrink-0" style={{ color: "var(--text-accent)" }} />
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
  const mcpTags = useMcpTags();

  const [q, setQ] = useState("");
  const deferredQuery = useDeferredValue(q);
  const [selectedSource, setSelectedSource] = useState<string | null>(null);
  const [selectedTag, setSelectedTag] = useState<string | null>(null);
  const [detail, setDetail] = useState<CatalogItem | null>(null);
  const [editingDetail, setEditingDetail] = useState(false);
  const [iconPickerEntry, setIconPickerEntry] = useState<RegistryEntry | null>(null);
  const [tagPickerEntry, setTagPickerEntry] = useState<RegistryEntry | null>(null);
  const [tagManagerState, setTagManagerState] = useState<{ open: boolean; initialTag: string | null }>({
    open: false,
    initialTag: null,
  });
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

  const tagSummary = useMemo<McpTagSummary[]>(() => {
    const counts = new Map<string, McpTagSummary>();
    for (const item of catalog) {
      for (const tag of resolveMcpTags(item.entry, mcpTags.tagsByKey)) {
        const folded = tag.toLowerCase();
        const existing = counts.get(folded);
        if (existing) existing.count += 1;
        else counts.set(folded, { name: tag, count: 1 });
      }
    }
    const collator = new Intl.Collator(undefined, { sensitivity: "base" });
    return [...counts.values()].sort((a, b) => collator.compare(a.name, b.name));
  }, [catalog, mcpTags.tagsByKey]);

  const availableTags = useMemo(() => tagSummary.map((item) => item.name), [tagSummary]);

  const activeTagSummary = useMemo(
    () => selectedTag === null
      ? undefined
      : tagSummary.find((item) => item.name.toLowerCase() === selectedTag.toLowerCase()),
    [tagSummary, selectedTag],
  );

  const sourceScoped = useMemo(() => {
    if (selectedTag !== null) {
      const needle = selectedTag.toLowerCase();
      return catalog.filter((item) =>
        resolveMcpTags(item.entry, mcpTags.tagsByKey).some((tag) => tag.toLowerCase() === needle),
      );
    }
    if (selectedSource === null) return catalog;
    return catalog.filter((item) => inSource(item.entry, selectedSource));
  }, [catalog, selectedSource, selectedTag, mcpTags.tagsByKey]);
  const activeSource = selectedTag === null ? sources.find((source) => source.id === selectedSource) : undefined;
  const sourceLocation = activeSource?.url ? sourceAddress(activeSource.url) : activeSource?.path;

  const searchIndex = useMemo(() => {
    const collator = new Intl.Collator(undefined, { sensitivity: "base" });
    // Alphabetical by name, then transport; in-effect copy first within a group.
    return [...sourceScoped].sort(
      (a, b) =>
        collator.compare(a.entry.name, b.entry.name) ||
        transportOf(a.entry).localeCompare(transportOf(b.entry)) ||
        Number(b.in_effect) - Number(a.in_effect)
    ).map((item) => {
      const tags = resolveMcpTags(item.entry, mcpTags.tagsByKey);
      return {
        item,
        text: `${item.entry.name}\n${item.entry.description}\n${tags.join("\n")}`.toLowerCase(),
      };
    });
  }, [sourceScoped, mcpTags.tagsByKey]);

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
        toast.show({ kind: "error", msg: `无法删除 MCP「${entry.name}」`, details: e });
      }
    },
    [consumptionState, deletable, toast]
  );

  const filtered = useMemo(() => {
    const needle = deferredQuery.trim().toLowerCase();
    return searchIndex.filter((row) => !needle || row.text.includes(needle)).map((row) => row.item);
  }, [deferredQuery, searchIndex]);
  const cards = useMemo(() => filtered.map((item) => {
    const assetKey = keyOf(item.entry);
    return (
      <div role="listitem" key={`${assetKey}@${item.entry.origin?.source ?? item.entry.origin?.kind ?? ""}`} className="arca-model-card arca-mcp-card">
        <RegistryCard
          item={item}
          tags={resolveMcpTags(item.entry, mcpTags.tagsByKey)}
          iconPreference={mcpIcons.preferences[assetKey]}
          selected={detail === item}
          installedAgents={agentsForServer(assetKey)}
          sourceName={sourceName}
          onOpen={() => { setEditingDetail(false); setDetail(item); }}
          onEditTags={() => setTagPickerEntry(item.entry)}
          onDelete={consumptionState && deletable(item.entry) && item.in_effect ? () => void deleteEntry(item.entry) : undefined}
        />
      </div>
    );
  }), [filtered, mcpTags.tagsByKey, mcpIcons.preferences, detail, agentsForServer, sourceName, consumptionState, deletable, deleteEntry]);

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
    setSelectedTag(null);
    setDetail(item ?? null);
    setEditingDetail(false);
    if (!item) toast.show({ kind: "error", msg: `未找到 MCP“${intent.name}”。` });
    onIntentConsumed?.(intent.id);
  }, [catalog, intent, onCreate, onIntentConsumed, state.loading, toast]);

  useEffect(() => {
    if (!selectedTag || state.loading) return;
    if (!tagSummary.some((item) => item.name.toLowerCase() === selectedTag.toLowerCase())) {
      setSelectedTag(null);
    }
  }, [selectedTag, state.loading, tagSummary]);

  const changeQuery = (value: string) => {
    setDetail(null);
    setEditingDetail(false);
    setQ(value);
  };

  const changeSource = (sourceId: string | null) => {
    setDetail(null);
    setEditingDetail(false);
    setSelectedTag(null);
    setSelectedSource(sourceId);
  };

  const changeTag = (tag: string | null) => {
    setDetail(null);
    setEditingDetail(false);
    setSelectedSource(null);
    setSelectedTag(tag);
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

  const reviewingMcpDeletion = !editingDetail && !suppressOperationReview
    && consumptionState?.plan?.kind === "delete-asset"
    && consumptionState.plan.domain_plan.domain === "mcp";

  return (
    <div className="arca-registry-workspace">
      <ResourceWorkspace
      overview={
        <ResourceOverview
          eyebrow={
            selectedTag !== null
              ? `${t("mcpTags.label")} · ${sourceScoped.length} MCPs`
              : activeSource
                ? `来源 · ${sourceScoped.length} MCPs`
                : `${catalog.length} MCPs · ${sources.length} 个来源`
          }
          title={
            selectedTag !== null
              ? (activeTagSummary?.name ?? selectedTag)
              : (activeSource?.name ?? "全部 MCPs")
          }
          description={
            selectedTag !== null
              ? t("mcpTags.filterDescription", { tag: activeTagSummary?.name ?? selectedTag })
              : activeSource
                ? activeSource.kind === "remote" ? "订阅来源" : activeSource.managed ? "Arca 管理的来源" : "本地配置来源"
                : "集中管理可复用的 MCP 连接、来源与配置"
          }
          icon={
            selectedTag !== null
              ? <TagIcon className="w-6 h-6" />
              : activeSource?.kind === "remote"
                ? <CloudIcon className="w-6 h-6" />
                : <ResourceIcon domain="mcp" className="w-6 h-6" />
          }
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
          selectedTag={selectedTag}
          tags={tagSummary}
          onSelect={changeSource}
          onSelectTag={changeTag}
          onManageTags={(initialTag) => setTagManagerState({ open: true, initialTag: initialTag ?? null })}
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
      searchPlaceholder={
        selectedTag !== null
          ? t("mcpTags.searchInTag", { tag: activeTagSummary?.name ?? selectedTag })
          : activeSource
            ? `搜索 ${activeSource.name} 中的 MCPs`
            : "搜索 MCPs"
      }
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
            resolvedTags={resolveMcpTags(detail.entry, mcpTags.tagsByKey)}
            availableTags={availableTags}
            onTagsSync={(assetKey, nextTags) => mcpTags.setTags(assetKey, nextTags)}
            transport={transportOf(detail.entry)}
            presentation="inspector"
            onBack={closeDetail}
          />
        ) : detail ? (
          <RegistryDetail
            entry={detail.entry}
            tags={resolveMcpTags(detail.entry, mcpTags.tagsByKey)}
            availableTags={availableTags}
            tagsSaving={mcpTags.busyKey === keyOf(detail.entry)}
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
            onSaveTags={async (nextTags) => {
              const targetKey = keyOf(detail.entry);
              try {
                await mcpTags.setTags(targetKey, nextTags);
              } catch (e) {
                toast.show({ kind: "error", msg: `保存「${detail.entry.name}」标签失败`, details: e });
              }
            }}
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
            let convergenceError: unknown = null;
            try {
              await consumptionState.commit();
            } catch (error) {
              if (typeof error !== "object" || error === null
                || !("code" in error) || error.code !== "pending_convergence") {
                // A failed central commit keeps the review and its error visible.
                return;
              }
              // Core committed the central deletion; only Agent projection is pending.
              convergenceError = error;
            }
            if (kind === "delete-asset") {
              closeDetail();
              setIconPickerEntry(null);
            }
            const refreshError: unknown = await state.refreshRegistry().then(() => null, (error) => error);
            toast.show({
              kind: convergenceError || refreshError ? "warning" : "success",
              msg: convergenceError
                ? kind === "delete-asset"
                  ? "MCP 已从中央库删除，部分 Agent 尚未同步完成，请在对应 Agent 页面处理。"
                  : "MCP 已保存，部分 Agent 尚未同步完成，请在对应 Agent 页面处理。"
                : refreshError ? "MCP 操作已完成，列表刷新失败"
                  : kind === "delete-asset" ? "MCP 资产已删除。" : "MCP 资产已保存。",
              details: convergenceError && refreshError ? { convergence: convergenceError, refresh: refreshError }
                : convergenceError ?? refreshError,
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
              setSelectedTag(null);
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
      {tagPickerEntry && (
        <McpTagPickerDialog
          assetKey={keyOf(tagPickerEntry)}
          entry={tagPickerEntry}
          iconPreference={mcpIcons.preferences[keyOf(tagPickerEntry)]}
          initialTags={resolveMcpTags(tagPickerEntry, mcpTags.tagsByKey)}
          availableTags={availableTags}
          onSave={(nextTags) => mcpTags.setTags(keyOf(tagPickerEntry), nextTags)}
          onClose={() => setTagPickerEntry(null)}
        />
      )}
      {tagManagerState.open && (
        <McpTagManagerDialog
          catalog={catalog}
          tagPreferences={mcpTags.tagsByKey}
          iconPreferences={mcpIcons.preferences}
          initialTag={tagManagerState.initialTag}
          availableTags={availableTags}
          onSaveBatch={mcpTags.setTagsBatch}
          onClose={() => setTagManagerState({ open: false, initialTag: null })}
        />
      )}
    </div>
  );
}

function RegistryCard({
  item,
  tags,
  iconPreference,
  selected,
  installedAgents,
  sourceName,
  onOpen,
  onEditTags,
  onDelete,
}: {
  item: CatalogItem;
  tags: string[];
  iconPreference?: McpIconPreference;
  selected: boolean;
  installedAgents: string[];
  sourceName: (id: string) => string;
  onOpen: () => void;
  onEditTags: () => void;
  onDelete?: () => void;
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
    <>
      <button
        type="button"
        className="arca-asset-list-row arca-mcp-list-row"
        data-selected={selected ? "true" : undefined}
        data-attention={item.in_effect ? undefined : "shadowed"}
        aria-label={`${t("centralAssets.openMcpDetails", { name: entry.name })} · ${transport} · ${source}`}
        aria-pressed={selected}
        onClick={onOpen}
      >
        <span className="arca-mcp-card-top">
          <span className="arca-asset-list-identity">
            <McpAvatar assetKey={keyOf(entry)} entry={entry} preference={iconPreference} size={36} />
            <span className="arca-asset-list-copy">
              <strong title={entry.name}>{entry.name}</strong>
              <code title={ep.text}>{ep.text}</code>
            </span>
          </span>
        </span>
        {tags.length > 0 && (
          <span className="arca-mcp-card-tags">
            {tags.slice(0, 3).map((tag) => (
              <span key={tag} className="arca-mcp-tag-chip" title={tag}>
                {tag}
              </span>
            ))}
            {tags.length > 3 && (
              <span className="arca-mcp-tag-more">+{tags.length - 3}</span>
            )}
          </span>
        )}
        <span className="arca-mcp-card-meta">
          <span className="arca-mcp-card-meta-left">
            <span className="arca-asset-list-transport">{transport}</span>
            <span className="arca-mcp-card-dot" aria-hidden="true">·</span>
            <span className="arca-asset-list-source" title={source}>{source}</span>
          </span>
          <span className="arca-asset-list-status">
            <Badge tone={item.in_effect ? "success" : "info"}>
              {item.in_effect
                ? t("centralAssets.effective")
                : t("centralAssets.shadowed")}
            </Badge>
          </span>
        </span>
      </button>
      <div className="arca-model-card-actions" data-count={onDelete ? "2" : "1"}>
        <button
          type="button"
          className="arca-model-card-action arca-mcp-card-tag-btn"
          aria-label={t("mcpTags.editFor", { name: entry.name })}
          title={t("mcpTags.editFor", { name: entry.name })}
          onClick={(event) => {
            event.stopPropagation();
            onEditTags();
          }}
        >
          <TagIcon className="w-3.5 h-3.5" />
        </button>
        {onDelete && (
          <button
            type="button"
            className="arca-model-card-action arca-model-card-delete"
            aria-label={`删除 ${entry.name}`}
            title={`删除 ${entry.name}`}
            onClick={(event) => {
              event.stopPropagation();
              onDelete();
            }}
          >
            <TrashIcon className="w-4 h-4" />
          </button>
        )}
      </div>
    </>
  );
}

function RegistryDetail({
  entry,
  tags,
  availableTags,
  tagsSaving,
  iconPreference,
  overriddenBy,
  installedAgents,
  sourceName,
  onClose,
  onCopy,
  onCustomizeIcon,
  onSaveTags,
  onEdit,
  onDelete,
}: {
  entry: RegistryEntry;
  tags: string[];
  availableTags: string[];
  tagsSaving?: boolean;
  iconPreference?: McpIconPreference;
  overriddenBy?: string;
  installedAgents: string[];
  sourceName: (id: string) => string;
  onClose: () => void;
  onCopy: () => void;
  onCustomizeIcon: () => void;
  onSaveTags: (nextTags: string[]) => Promise<void>;
  onEdit?: () => void;
  onDelete?: () => void;
}) {
  const { t } = useTranslation();
  const toast = useToast();
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
      footerStart={onDelete && (
        <button onClick={onDelete} className="arca-btn-soft-danger" title="删除中央 MCP 资产">
          <TrashIcon className="w-4 h-4" />
          删除
        </button>
      )}
      footer={
        <>
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
      {overriddenBy && <p className="arca-inspector-description">已被覆盖 · 当前使用「{overriddenBy}」</p>}

      {entry.description && <p className="arca-inspector-description">{entry.description}</p>}

      <InspectorSection title="连接" icon={<LinkIcon className="w-4 h-4" />}>
        <InspectorField
          icon={<NetworkIcon className="w-4 h-4" />}
          label="地址"
          mono
          action={
            <IconButton
              onClick={() => {
                void navigator.clipboard.writeText(endpoint.text);
                toast.show({ kind: "info", msg: "地址已复制到剪贴板" });
              }}
              title="复制地址"
            >
              <CopyIcon className="w-3.5 h-3.5" />
            </IconButton>
          }
        >
          {endpoint.text}
        </InspectorField>
        {entry.repo && (
          <InspectorField icon={<LinkIcon className="w-4 h-4" />} label="主页">
            <button onClick={() => openUrl(entry.repo!)} className="arca-inline-link" title="在浏览器中打开">
              <LinkIcon className="w-3.5 h-3.5" />
              {entry.repo}
            </button>
          </InspectorField>
        )}
      </InspectorSection>

      <InspectorSection title={t("mcpTags.label")} icon={<TagIcon className="w-4 h-4" />}>
        <McpTagInput
          key={keyOf(entry)}
          tags={tags}
          availableTags={availableTags}
          disabled={tagsSaving}
          onChange={(next) => void onSaveTags(next)}
        />
      </InspectorSection>

      <InspectorSection title="配置" icon={<TerminalIcon className="w-4 h-4" />}>
        <div className="arca-config-card">
          <div className="arca-config-card-header">
            <span className="arca-config-card-lang">JSON</span>
            <button
              type="button"
              className="arca-config-card-copy"
              onClick={() => {
                const configText = JSON.stringify(redactSensitiveConfig(entry.config), null, 2);
                void navigator.clipboard.writeText(configText);
                toast.show({ kind: "info", msg: "配置已复制到剪贴板" });
              }}
              title="复制配置"
            >
              <CopyIcon className="w-3.5 h-3.5" />
              <span>复制</span>
            </button>
          </div>
          <pre className="arca-config-preview">
            {JSON.stringify(redactSensitiveConfig(entry.config), null, 2)}
          </pre>
        </div>
      </InspectorSection>
    </ResourceInspector>
  );
}
