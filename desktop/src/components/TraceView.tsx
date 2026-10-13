import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { traceDetail, traceExport, traceImport, traceIndex, traceMatches, tracePage, type TraceDetail, type TraceEvent, type TraceIndex, type TracePage } from "../lib/traces";
import { copyToClipboard } from "../lib/api";
import { formatError } from "../lib/format";
import { useToast } from "./Toast";
import { CopyIcon, DocumentIcon, DownloadIcon, FolderIcon, RefreshIcon } from "./icons";
import { BoundedText } from "./BoundedText";
import { ByteBudgetCache, jsonMemoryWeight } from "../lib/byteBudgetCache";
import "./TraceView.css";

function jsonText(value: unknown): string {
  const compact = JSON.stringify(value);
  // Deep native UI trees can expand enormously with indentation. Preserve all
  // fields, but leave very large records compact instead of truncating them.
  return compact.length > 1024 * 1024 ? compact : JSON.stringify(value, null, 2);
}
function time(value: string | null) { return value && !Number.isNaN(Date.parse(value)) ? new Date(value).toLocaleTimeString() : "—"; }

export function TraceView({ initialAgentId }: { initialAgentId?: string }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [index, setIndex] = useState<TraceIndex | null>(null);
  const [sourceId, setSourceId] = useState("all");
  const [sessionQuery, setSessionQuery] = useState("");
  const [query, setQuery] = useState("");
  const [role, setRole] = useState("all");
  const [page, setPage] = useState<TracePage | null>(null);
  const [events, setEvents] = useState<TraceEvent[]>([]);
  const [detail, setDetail] = useState<TraceDetail | null>(null);
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [tab, setTab] = useState<"text" | "raw" | "response">("raw");
  const [loadingIndex, setLoadingIndex] = useState(false);
  const [loadingPage, setLoadingPage] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [operationBusy, setOperationBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(0);
  const indexEpoch = useRef(0);
  const detailEpoch = useRef(0);
  const selectedSession = useRef<string | null>(null);
  const detailCache = useRef(new ByteBudgetCache<string, TraceDetail>(6, 4 * 1024 * 1024));

  const selectSession = useCallback(async (id: string) => {
    const generation = ++epoch.current;
    ++detailEpoch.current;
    selectedSession.current = id;
    detailCache.current.clear();
    setPage(null); setEvents([]); setDetail(null); setSelectedEvent(null); setLoadingDetail(false);
    setQuery(""); setRole("all"); setLoadingPage(true); setError(null);
    try {
      const value = await tracePage(id);
      if (generation === epoch.current) { setPage(value); setEvents(value.events); }
    } catch (error) { if (generation === epoch.current) setError(formatError(error)); }
    finally { if (generation === epoch.current) setLoadingPage(false); }
  }, []);

  const refresh = useCallback(async (first = false) => {
    const generation = ++indexEpoch.current;
    setLoadingIndex(true);
    try {
      const value = await traceIndex();
      if (generation !== indexEpoch.current) return;
      setIndex(value);
      const initialSource = value.sources.find(s => s.agent_ids.includes(initialAgentId ?? ""));
      if (first && initialAgentId) setSourceId(initialSource?.id ?? "unsupported");
      const candidates = initialAgentId && first ? value.sessions.filter(s => s.source_id === initialSource?.id) : value.sessions;
      const selected = selectedSession.current;
      const id = selected && value.sessions.some(s => s.id === selected) ? selected : candidates[0]?.id;
      if (id) await selectSession(id);
      else { ++epoch.current; ++detailEpoch.current; detailCache.current.clear(); selectedSession.current = null; setPage(null); setEvents([]); setDetail(null); setSelectedEvent(null); setLoadingPage(false); setLoadingDetail(false); }
    } catch (error) { if (generation === indexEpoch.current) setError(formatError(error)); }
    finally { if (generation === indexEpoch.current) setLoadingIndex(false); }
  }, [initialAgentId, selectSession]);

  useEffect(() => {
    void refresh(true);
    return () => { ++epoch.current; ++indexEpoch.current; ++detailEpoch.current; detailCache.current.clear(); };
  }, [refresh]);

  const sessions = useMemo(() => (index?.sessions ?? []).filter(s => {
    const sourceMatches = sourceId === "all" || (sourceId === "imported" || sourceId === "unsupported" ? s.imported : s.source_id === sourceId);
    return sourceMatches && [s.title, s.agent_name, s.project ?? "", s.path].join("\n").toLocaleLowerCase().includes(sessionQuery.trim().toLocaleLowerCase());
  }), [index, sourceId, sessionQuery]);
  const visibleEvents = useMemo(() => events.filter(e => traceMatches(e, query, role)), [events, query, role]);

  const changeSource = (id: string) => {
    setSourceId(id); setSessionQuery("");
    const first = index?.sessions.find(s => id === "all" || (id === "imported" || id === "unsupported" ? s.imported : s.source_id === id));
    if (first) void selectSession(first.id);
    else { ++epoch.current; ++detailEpoch.current; detailCache.current.clear(); selectedSession.current = null; setPage(null); setEvents([]); setDetail(null); setSelectedEvent(null); setError(null); setLoadingPage(false); setLoadingDetail(false); }
  };
  const loadMore = async () => {
    if (!page?.next_cursor || loadingPage) return;
    const generation = epoch.current;
    setLoadingPage(true);
    try {
      const value = await tracePage(page.session.id, page.next_cursor);
      if (generation === epoch.current) {
        setPage(value); setEvents(previous => [...previous, ...value.events]);
      }
    } catch (error) { if (generation === epoch.current) setError(formatError(error)); }
    finally { if (generation === epoch.current) setLoadingPage(false); }
  };
  const selectEvent = async (event: TraceEvent) => {
    if (!page) return;
    const generation = ++detailEpoch.current;
    setSelectedEvent(event.id); setDetail(null); setLoadingDetail(true);
    setTab(event.kind === "tool_call" ? "raw" : "text");
    try {
      const cached = detailCache.current.get(event.id);
      const value = cached ?? await traceDetail(page.session.id, event.id, page.revision);
      if (generation !== detailEpoch.current) return;
      if (!cached) {
        // Large records remain viewable without retaining six full copies.
        detailCache.current.set(event.id, value, jsonMemoryWeight(value, 4 * 1024 * 1024));
      }
      setDetail(value);
    } catch (error) { if (generation === detailEpoch.current) toast.show({ kind: "error", msg: formatError(error) }); }
    finally { if (generation === detailEpoch.current) setLoadingDetail(false); }
  };
  const perform = async (action: () => Promise<void>) => {
    if (operationBusy) return;
    setOperationBusy(true);
    try { await action(); } catch (error) { toast.show({ kind: "error", msg: formatError(error) }); }
    finally { setOperationBusy(false); }
  };
  const raw = useMemo(() => !detail || tab === "text" ? "" :
    jsonText(tab === "response" ? detail.related.map(r => r.raw) : detail.raw), [detail, tab]);
  const preview = detail ? tab === "text" ? detail.text || t("trace.noText") : tab === "response" && detail.related.length === 0 ? detail.pairing_note ?? t("trace.noResponse") : raw : "";
  const source = index?.sources.find(s => s.id === sourceId);

  return <div className="arca-trace">
    <header className="arca-trace-heading"><div><h1><DocumentIcon className="w-5 h-5" />{t("trace.title")}</h1><p>{t("trace.subtitle")}</p></div><div className="arca-trace-heading-actions"><span className="arca-trace-local">{t("trace.local")}</span><button className="arca-trace-button" disabled={loadingIndex || operationBusy} onClick={() => void refresh()}><RefreshIcon className="w-4 h-4" data-spinning={loadingIndex || undefined} />{t("trace.refresh")}</button><button className="arca-trace-button" disabled={operationBusy} onClick={() => void perform(async () => {
      const imported = await traceImport(); if (!imported) return;
      setIndex(previous => previous ? { ...previous, sessions: [imported, ...previous.sessions.filter(s => s.id !== imported.id)] } : { sources: [], sessions: [imported], warnings: [] });
      setSourceId("imported"); setSessionQuery(""); await selectSession(imported.id);
    })}><FolderIcon className="w-4 h-4" />{t("trace.import")}</button></div></header>
    {error && <div className="arca-trace-alert" role="alert">{error}<button disabled={loadingPage} onClick={() => selectedSession.current ? void selectSession(selectedSession.current) : void refresh()}>{t("trace.reload")}</button></div>}
    {sourceId === "unsupported" && <p className="arca-trace-hint">{t("trace.unsupportedHint")}</p>}
    {(index?.warnings.length ?? 0) > 0 && <details className="arca-trace-warnings"><summary>{index?.warnings[0]}</summary>{index?.warnings.slice(1).map(w => <p key={w}>{w}</p>)}</details>}
    <div className="arca-trace-workspace">
      <aside className="arca-trace-sessions" aria-label={t("trace.sessions")}>
        <div className="arca-trace-source-controls"><select aria-label={t("trace.source")} value={sourceId} onChange={e => changeSource(e.target.value)}><option value="all">{t("trace.allSources")}</option>{index?.sources.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}<option value="imported">{t("trace.imported")}</option>{sourceId === "unsupported" && <option value="unsupported">{t("trace.unsupported")}</option>}</select><input type="search" aria-label={t("trace.sessionSearch")} placeholder={t("trace.sessionSearch")} value={sessionQuery} onChange={e => setSessionQuery(e.target.value)} /><small>{t("trace.counts", { count: sessions.length })}</small></div>
        <div className="arca-trace-session-list">{sessions.length === 0 ? <div className="arca-trace-empty">{loadingIndex ? t("trace.loading") : t("trace.emptySessions")}</div> : sessions.map(s => <button className="arca-trace-session" key={s.id} data-selected={selectedSession.current === s.id} onClick={() => void selectSession(s.id)}><span>{s.agent_name}<time>{new Date(s.modified_at).toLocaleDateString()}</time></span><strong title={s.title}>{s.title}</strong><code title={s.project ?? s.path}>{s.project ?? s.path}</code><small>{(s.bytes / 1024 / 1024).toFixed(1)} MiB · {s.format}</small></button>)}</div>
        {source && <code className="arca-trace-source-path">{source.roots.join("\n")}</code>}
      </aside>
      <section className="arca-trace-timeline" aria-label={t("trace.records")}>
        <div className="arca-trace-event-controls"><input type="search" aria-label={t("trace.eventSearch")} placeholder={t("trace.eventSearch")} value={query} onChange={e => setQuery(e.target.value)} /><div><select aria-label={t("trace.records")} value={role} onChange={e => setRole(e.target.value)}>{["all", "user", "assistant", "tools", "event"].map(k => <option key={k} value={k}>{t(`trace.${k}`)}</option>)}</select><small>{t("trace.loaded", { count: events.length })}</small></div></div>
        <div className="arca-trace-event-list">{!page && !loadingPage ? <div className="arca-trace-empty">{t("trace.chooseSession")}</div> : visibleEvents.length === 0 ? <div className="arca-trace-empty">{loadingPage ? t("trace.loading") : t("trace.emptyEvents")}</div> : visibleEvents.map(event => <button className="arca-trace-event" key={event.id} data-kind={event.kind} data-selected={selectedEvent === event.id} onClick={() => void selectEvent(event)}><span><b>{t(`trace.${event.kind}`)}</b>{event.is_error && <em>{t("trace.error")}</em>}<time>{time(event.timestamp)}</time></span><strong>{event.kind.startsWith("tool_") ? event.title : event.preview.slice(0, 100) || event.title}</strong>{event.kind.startsWith("tool_") && <p>{event.preview || event.call_id}</p>}{event.call_id && <code title={event.call_id}>{event.call_id}</code>}</button>)}
          {page?.next_cursor ? <button className="arca-trace-load-more" disabled={loadingPage} onClick={() => void loadMore()}>{loadingPage ? t("trace.loading") : t("trace.loadMore")}</button> : page && <div className="arca-trace-end">{t(page.warnings.length ? "trace.stopped" : "trace.finished")}</div>}
          {page?.warnings.map(w => <p className="arca-trace-inline-warning" key={w}>{w}</p>)}
        </div>
      </section>
      <section className="arca-trace-detail" aria-label={t("trace.raw")}>
        {!detail ? <div className="arca-trace-empty">{loadingDetail ? t("trace.loading") : t("trace.chooseEvent")}</div> : <>
          <div className="arca-trace-detail-heading"><div><small>{t(`trace.${detail.event.kind}`)} · {time(detail.event.timestamp)}</small><h2>{detail.event.kind.startsWith("tool_") ? detail.event.title : t(`trace.${detail.event.kind}`)}</h2><code>{detail.event.call_id ?? detail.event.id}</code></div><button className="arca-trace-button" title={t("trace.export")} aria-label={t("trace.export")} disabled={operationBusy || !page} onClick={() => void perform(async () => { if (page && await traceExport(page.session.id, detail.event.id, page.revision)) toast.show({ kind: "success", msg: t("trace.exported") }); })}><DownloadIcon className="w-4 h-4" /></button></div>
          <div className="arca-trace-tabs"><div className="arca-seg">{(["text", "raw", ...(detail.event.kind === "tool_call" ? (["response"] as const) : [])] as const).map(k => <button className="arca-seg-item" key={k} aria-pressed={tab === k} data-active={tab === k || undefined} onClick={() => setTab(k)}>{t(`trace.${k}`)}</button>)}</div><button className="arca-trace-button" aria-label={t("trace.copy")} title={t("trace.copy")} disabled={operationBusy} onClick={() => void perform(async () => { await copyToClipboard(tab === "text" && detail ? jsonText(detail.raw) : raw); toast.show({ kind: "success", msg: t("trace.copied") }); })}><CopyIcon className="w-4 h-4" /></button></div>
          <p className="arca-trace-detail-note">{t("trace.rawHint")}</p>
          <BoundedText key={`${page?.session.id}:${selectedEvent}:${tab}`} text={preview} className="arca-trace-body" raw={tab !== "text"} />
        </>}
      </section>
    </div>
    <footer className="arca-trace-footer"><span>{t("trace.previewHint")}</span><details><summary>{t("trace.local")}</summary><p>{t("trace.sourceHint")}</p><p>{t("trace.privacyHint")}</p>{page && <code>{page.session.path}</code>}</details></footer>
  </div>;
}
