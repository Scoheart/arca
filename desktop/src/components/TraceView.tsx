import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { traceDetail, traceExport, traceImport, traceIndex, traceMatches, tracePage, type TraceDetail, type TraceEvent, type TraceIndex, type TracePage } from '../lib/traces';
import { conversationRows, retainConversationText, traceJson, traceSessionLabel } from '../lib/traceConversation';
import { copyToClipboard } from '../lib/api';
import { formatError } from '../lib/format';
import { ByteBudgetCache, jsonMemoryWeight } from '../lib/byteBudgetCache';
import { ToastNotice, useToast } from './Toast';
import { ChevronDownIcon, CopyIcon, DocumentIcon, DownloadIcon, FolderIcon, RefreshIcon, SearchIcon, TerminalIcon } from './icons';
import { AgentGlyph } from './brandIcons';
import { BoundedText } from './BoundedText';
import { DialogShell } from './DialogShell';
import { TraceMarkdown } from './TraceMarkdown';
import './TraceView.css';

function time(value: string | null) {
  return value && !Number.isNaN(Date.parse(value))
    ? new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';
}

const ToolContent = memo(function ToolContent({ detail }: { detail: TraceDetail }) {
  const { t } = useTranslation();
  const input = useMemo(() => {
    if (detail.text.length > 128 * 1024) return detail.text;
    try { return JSON.stringify(JSON.parse(detail.text), null, 2); } catch { return detail.text; }
  }, [detail.text]);
  return <div className="arca-trace-tool-content">
    <section><h4>{t(detail.event.kind === 'tool_call' ? 'trace.input' : 'trace.output')}</h4>
      <BoundedText key={detail.event.id} text={input || t('trace.noText')} raw={detail.event.kind === 'tool_call'} className="arca-trace-tool-body" />
    </section>
    {detail.related.map((result, index) => <section key={index}><h4>{t('trace.output')}{detail.related.length > 1 ? ` ${index + 1}` : ''}</h4>
      <BoundedText text={result.text || traceJson(result.raw)} className="arca-trace-tool-body" />
    </section>)}
    {detail.pairing_note && <p className="arca-trace-pairing-note">{detail.pairing_note}</p>}
  </div>;
});

export function TraceView({ initialAgentId }: { initialAgentId?: string }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [index, setIndex] = useState<TraceIndex | null>(null);
  const [sourceId, setSourceId] = useState('all');
  const [sessionQuery, setSessionQuery] = useState('');
  const [query, setQuery] = useState('');
  const [role, setRole] = useState('all');
  const [page, setPage] = useState<TracePage | null>(null);
  const [events, setEvents] = useState<TraceEvent[]>([]);
  const [detail, setDetail] = useState<TraceDetail | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [tab, setTab] = useState<'text' | 'raw' | 'response'>('raw');
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [loadingIndex, setLoadingIndex] = useState(false);
  const [loadingPage, setLoadingPage] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [operationBusy, setOperationBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(0), indexEpoch = useRef(0), detailEpoch = useRef(0);
  const selectedSession = useRef<string | null>(null);
  const detailCache = useRef(new ByteBudgetCache<string, TraceDetail>(6, 4 * 1024 * 1024));
  const pendingDetails = useRef(new Map<string, Promise<TraceDetail>>());
  const scroll = useRef<HTMLDivElement>(null);

  const clearConversation = useCallback(() => {
    ++epoch.current; ++detailEpoch.current;
    detailCache.current.clear(); pendingDetails.current.clear();
    setPage(null); setEvents([]); setDetail(null); setExpanded(null); setInspectorOpen(false);
    setLoadingDetail(false); setQuery(''); setRole('all');
  }, []);

  const selectSession = useCallback(async (id: string) => {
    clearConversation();
    const generation = epoch.current;
    selectedSession.current = id;
    setSessionsOpen(false); setLoadingPage(true); setError(null);
    if (scroll.current) scroll.current.scrollTop = 0;
    try {
      const value = await tracePage(id);
      if (generation === epoch.current) { setPage(value); setEvents(retainConversationText(value.events)); }
    } catch (error) { if (generation === epoch.current) setError(formatError(error)); }
    finally { if (generation === epoch.current) setLoadingPage(false); }
  }, [clearConversation]);

  const refresh = useCallback(async (first = false) => {
    const generation = ++indexEpoch.current;
    setLoadingIndex(true); setError(null);
    try {
      const value = await traceIndex();
      if (generation !== indexEpoch.current) return;
      setIndex(value);
      const initialSource = value.sources.find(source => source.agent_ids.includes(initialAgentId ?? ''));
      if (first && initialAgentId) setSourceId(initialSource?.id ?? 'unsupported');
      const activeSource = first && initialAgentId ? initialSource?.id : sourceId;
      const candidates = value.sessions.filter(session => activeSource === 'all'
        || (activeSource === 'imported' || activeSource === 'unsupported' ? session.imported : session.source_id === activeSource));
      const id = candidates.find(session => session.id === selectedSession.current)?.id ?? candidates[0]?.id;
      if (id) await selectSession(id);
      else { clearConversation(); selectedSession.current = null; setLoadingPage(false); }
    } catch (error) { if (generation === indexEpoch.current) setError(formatError(error)); }
    finally { if (generation === indexEpoch.current) setLoadingIndex(false); }
  }, [initialAgentId, sourceId, selectSession, clearConversation]);
  const refreshRef = useRef(refresh); refreshRef.current = refresh;
  useEffect(() => {
    void refreshRef.current(true);
    return () => { ++epoch.current; ++indexEpoch.current; ++detailEpoch.current; detailCache.current.clear(); pendingDetails.current.clear(); };
  }, [initialAgentId]);

  const sessions = useMemo(() => (index?.sessions ?? []).filter(session => {
    const matches = sourceId === 'all' || (sourceId === 'imported' || sourceId === 'unsupported' ? session.imported : session.source_id === sourceId);
    return matches && [session.title, session.agent_name, session.project ?? '', session.path].join('\n')
      .toLocaleLowerCase().includes(sessionQuery.trim().toLocaleLowerCase());
  }), [index, sourceId, sessionQuery]);
  const rows = useMemo(() => conversationRows(events).filter(row => traceMatches(row.event, query, role)
    || row.result && traceMatches(row.result, query, role)), [events, query, role]);
  const sessionSource = index?.sources.find(source => source.id === page?.session.source_id);
  const source = index?.sources.find(source => source.id === sourceId);
  const agentId = sessionSource?.agent_ids[0] ?? initialAgentId ?? '';

  const changeSource = (id: string) => {
    setSourceId(id); setSessionQuery('');
    const first = index?.sessions.find(session => id === 'all' || (id === 'imported' || id === 'unsupported' ? session.imported : session.source_id === id));
    if (first) void selectSession(first.id);
    else { clearConversation(); selectedSession.current = null; setError(null); setLoadingPage(false); }
  };
  const loadMore = async () => {
    if (!page?.next_cursor || loadingPage) return;
    const generation = epoch.current;
    setLoadingPage(true);
    try {
      const value = await tracePage(page.session.id, page.next_cursor);
      if (generation === epoch.current) { setPage(value); setEvents(previous => retainConversationText([...previous, ...value.events])); }
    } catch (error) { if (generation === epoch.current) setError(formatError(error)); }
    finally { if (generation === epoch.current) setLoadingPage(false); }
  };

  const readDetail = async (event: TraceEvent) => {
    if (!page) throw new Error(t('trace.chooseSession'));
    const key = `${page.session.id}:${page.revision}:${event.id}`, generation = epoch.current;
    const cached = detailCache.current.get(key);
    if (cached) return cached;
    let pending = pendingDetails.current.get(key);
    if (!pending) {
      pending = traceDetail(page.session.id, event.id, page.revision);
      pendingDetails.current.set(key, pending);
    }
    try {
      const value = await pending;
      if (generation === epoch.current) detailCache.current.set(key, value, jsonMemoryWeight(value, 4 * 1024 * 1024));
      return value;
    } finally { if (pendingDetails.current.get(key) === pending) pendingDetails.current.delete(key); }
  };
  const inspect = async (event: TraceEvent, raw = false) => {
    if (loadingDetail || operationBusy) return;
    if (!raw && expanded === event.id) { setExpanded(null); setDetail(null); return; }
    const generation = ++detailEpoch.current;
    setExpanded(event.id); setInspectorOpen(raw); setTab('raw'); setDetail(null); setLoadingDetail(true);
    try {
      const value = await readDetail(event);
      if (generation === detailEpoch.current) setDetail(value);
    } catch (error) { if (generation === detailEpoch.current) toast.show({ kind: 'error', msg: formatError(error) }); }
    finally { if (generation === detailEpoch.current) setLoadingDetail(false); }
  };
  const perform = async (action: () => Promise<void>) => {
    if (operationBusy) return;
    setOperationBusy(true);
    try { await action(); } catch (error) { toast.show({ kind: 'error', msg: formatError(error) }); }
    finally { setOperationBusy(false); }
  };
  const copyMessage = (event: TraceEvent) => perform(async () => {
    const generation = epoch.current;
    const text = event.text != null && !event.text_truncated ? event.text : (await readDetail(event)).text;
    if (generation !== epoch.current) return;
    if (!text) { toast.show({ kind: 'info', msg: t('trace.noText') }); return; }
    await copyToClipboard(text); toast.show({ kind: 'success', msg: t('trace.textCopied') });
  });
  const rawText = useMemo(() => !inspectorOpen || !detail ? '' : tab === 'text' ? detail.text
    : tab === 'response' ? detail.related.length ? traceJson(detail.related.map(result => result.raw)) : detail.pairing_note ?? t('trace.noResponse')
      : traceJson(detail.raw), [inspectorOpen, detail, tab, t]);
  const disabled = loadingDetail || operationBusy;

  return <div className="arca-trace">
    <header className="arca-trace-heading"><div><h1><DocumentIcon className="w-5 h-5" />{t('trace.title')}</h1><p>{t('trace.subtitle')}</p></div>
      <div className="arca-trace-heading-actions"><span className="arca-trace-local">{t('trace.local')}</span>
        <button type="button" className="arca-trace-button" disabled={loadingIndex || operationBusy} onClick={() => void refresh()}><RefreshIcon className="w-4 h-4" data-spinning={loadingIndex || undefined} />{t('trace.refresh')}</button>
        <button type="button" className="arca-trace-button" disabled={operationBusy} onClick={() => void perform(async () => {
          const imported = await traceImport(); if (!imported) return;
          setIndex(previous => previous ? { ...previous, sessions: [imported, ...previous.sessions.filter(session => session.id !== imported.id)] } : { sources: [], sessions: [imported], warnings: [] });
          setSourceId('imported'); setSessionQuery(''); await selectSession(imported.id);
        })}><FolderIcon className="w-4 h-4" />{t('trace.import')}</button>
      </div>
    </header>
    <ToastNotice message={error} action={<button type="button" className="btn-ghost" onClick={() => selectedSession.current ? void selectSession(selectedSession.current) : void refresh()}>{t('trace.reload')}</button>} />
    <ToastNotice kind="warning" noticeKey="trace-index" message={index?.warnings.join(' · ')} />
    <ToastNotice kind="warning" noticeKey="trace-page" message={page?.warnings.join(' · ')} />
    <div className="arca-trace-workspace" data-sessions-open={sessionsOpen}>
      <aside className="arca-trace-sessions" aria-label={t('trace.sessions')}>
        <div className="arca-trace-source-controls"><div className="arca-trace-sidebar-label"><strong>{t('trace.sessions')}</strong><small>{sessions.length}</small><button type="button" className="arca-trace-button arca-trace-session-toggle" onClick={() => setSessionsOpen(false)}>{t('common.close')}</button></div>
          <select aria-label={t('trace.source')} value={sourceId} onChange={event => changeSource(event.target.value)}><option value="all">{t('trace.allSources')}</option>
            {index?.sources.map(source => <option key={source.id} value={source.id}>{source.name}</option>)}<option value="imported">{t('trace.imported')}</option>
            {sourceId === 'unsupported' && <option value="unsupported">{t('trace.unsupported')}</option>}</select>
          <label className="arca-trace-search"><SearchIcon className="w-3.5 h-3.5" /><input type="search" aria-label={t('trace.sessionSearch')} placeholder={t('trace.sessionSearch')} value={sessionQuery} onChange={event => setSessionQuery(event.target.value)} /></label>
        </div>
        <div className="arca-trace-session-list">{sessions.length === 0 ? <div className="arca-trace-empty">{loadingIndex ? t('trace.loading') : t('trace.emptySessions')}</div>
          : sessions.map(session => <button type="button" className="arca-trace-session" key={session.id} data-selected={selectedSession.current === session.id} aria-current={selectedSession.current === session.id ? 'true' : undefined} onClick={() => void selectSession(session.id)}>
            <span>{session.agent_name}<time>{new Date(session.modified_at).toLocaleDateString()}</time></span>
            <strong title={session.title}>{traceSessionLabel(session)}</strong><code title={session.project ?? session.path}>{session.project ?? session.path}</code>
          </button>)}</div>
        {source && <code className="arca-trace-source-path" title={source.roots.join('\n')}>{source.roots.join('\n')}</code>}
      </aside>
      <section className="arca-trace-conversation" aria-label={t('trace.conversation')}>
        <div className="arca-trace-conversation-heading"><button type="button" className="arca-trace-button arca-trace-session-toggle" aria-label={t('trace.sessions')} aria-expanded={sessionsOpen} onClick={() => setSessionsOpen(value => !value)}><FolderIcon className="w-4 h-4" /></button>
          <div><h2 title={page?.session.title}>{page ? traceSessionLabel(page.session) : t('trace.conversation')}</h2><p>{page ? `${page.session.agent_name}${page.session.project ? ` · ${page.session.project}` : ''}` : t('trace.chooseSession')}</p></div>
          {page && <span className="arca-trace-format">{page.session.format}</span>}
        </div>
        <div className="arca-trace-event-controls"><label className="arca-trace-search"><SearchIcon className="w-3.5 h-3.5" /><input type="search" aria-label={t('trace.eventSearch')} placeholder={t('trace.eventSearch')} value={query} onChange={event => setQuery(event.target.value)} /></label>
          <select aria-label={t('trace.records')} value={role} onChange={event => setRole(event.target.value)}>{['all', 'user', 'assistant', 'tools', 'event'].map(kind => <option key={kind} value={kind}>{t(`trace.${kind}`)}</option>)}</select><small>{t('trace.loaded', { count: events.length })}</small>
        </div>
        {sourceId === 'unsupported' && <p className="arca-trace-hint">{t('trace.unsupportedHint')}</p>}
        <div className="arca-trace-conversation-scroll" ref={scroll}>
          <div className="arca-trace-messages" role="log" aria-live="off" aria-label={t('trace.conversation')}>
            {!page && !loadingPage ? <div className="arca-trace-empty"><DocumentIcon className="w-8 h-8" />{t('trace.chooseSession')}</div>
              : rows.length === 0 ? <div className="arca-trace-empty">{loadingPage ? t('trace.loading') : t('trace.emptyEvents')}</div>
                : rows.map(({ event, result }) => {
                  const conversation = event.kind === 'user' || event.kind === 'assistant';
                  const current = expanded === event.id, full = current && detail?.event.id === event.id ? detail : null;
                  const text = full?.text ?? event.text ?? event.preview;
                  return conversation ? <article className="arca-trace-message" data-role={event.kind} key={event.id}>
                    <header>{event.kind === 'assistant' && agentId && <AgentGlyph id={agentId} size={18} />}<strong>{event.kind === 'user' ? t('trace.you') : page?.session.agent_name ?? t('trace.assistant')}</strong><time>{time(event.timestamp)}</time></header>
                    <div className="arca-trace-message-bubble"><TraceMarkdown key={`${event.id}:${Boolean(full)}`} text={text || t('trace.noText')} />
                      {current && loadingDetail && <div className="arca-trace-reading" role="status">{t('trace.loading')}</div>}
                      {!full && (event.text == null || event.text_truncated) && <button type="button" className="arca-trace-read-more" disabled={disabled} onClick={() => void inspect(event)}>{t('trace.readFull')}</button>}
                    </div>
                    <footer><button type="button" title={t('trace.copyText')} aria-label={t('trace.copyText')} disabled={disabled} onClick={() => void copyMessage(event)}><CopyIcon className="w-3.5 h-3.5" /></button>
                      <button type="button" disabled={disabled} onClick={() => void inspect(event, true)}>{t('trace.inspect')}</button></footer>
                  </article> : <article className="arca-trace-tool" key={event.id} data-error={event.is_error || result?.is_error || undefined}>
                    <button type="button" className="arca-trace-tool-summary" aria-expanded={current && !inspectorOpen} disabled={disabled} onClick={() => void inspect(event)}>
                      <span className="arca-trace-tool-icon"><TerminalIcon className="w-4 h-4" /></span><span className="arca-trace-tool-label"><strong>{event.title}</strong><small>{event.preview.replace(/\s+/g, ' ') || event.call_id || t(`trace.${event.kind}`)}</small></span>
                      <span className="arca-trace-tool-status" data-error={event.is_error || result?.is_error || undefined}>{event.is_error || result?.is_error ? t('trace.error') : result ? t('trace.returned') : t(`trace.${event.kind}`)}</span><time>{time(event.timestamp)}</time><ChevronDownIcon className="w-3.5 h-3.5" style={{ transform: current && !inspectorOpen ? 'rotate(180deg)' : undefined }} />
                    </button>
                    {current && !inspectorOpen && <div className="arca-trace-tool-expanded">{loadingDetail ? <div className="arca-trace-reading" role="status">{t('trace.loading')}</div> : full ? <ToolContent detail={full} /> : <span>{t('trace.detailUnavailable')}</span>}
                      <div className="arca-trace-tool-actions">{event.call_id && <code title={event.call_id}>{event.call_id}</code>}<button type="button" disabled={disabled} onClick={() => void inspect(event, true)}>{t('trace.inspect')}</button></div>
                    </div>}
                  </article>;
                })}
            {page?.next_cursor ? <button type="button" className="arca-trace-load-more" disabled={loadingPage} onClick={() => void loadMore()}>{loadingPage ? t('trace.loading') : t('trace.loadMore')}</button>
              : page && <div className="arca-trace-end"><span />{t(page.warnings.length ? 'trace.stopped' : 'trace.finished')}<span /></div>}
          </div>
        </div>
      </section>
    </div>
    <footer className="arca-trace-footer"><span>{t('trace.previewHint')}</span><details><summary>{t('trace.local')}</summary><p>{t('trace.sourceHint')}</p><p>{t('trace.privacyHint')}</p>{page && <code>{page.session.path}</code>}</details></footer>
    {inspectorOpen && <DialogShell kind="review" size="lg" className="arca-trace-inspector" leading={<DocumentIcon className="w-4 h-4" />} title={t('trace.inspect')} subtitle={detail ? `${t(`trace.${detail.event.kind}`)} · ${time(detail.event.timestamp)}` : t('trace.loading')} onClose={() => setInspectorOpen(false)}
      footerEnd={<><button type="button" className="btn-secondary" disabled={!detail || operationBusy} onClick={() => void perform(async () => { await copyToClipboard(rawText); toast.show({ kind: 'success', msg: t(tab === 'text' ? 'trace.textCopied' : 'trace.copied') }); })}><CopyIcon className="w-4 h-4" />{t(tab === 'text' ? 'trace.copyText' : 'trace.copy')}</button>
        <button type="button" className="btn-secondary" disabled={!detail || operationBusy || !page} onClick={() => void perform(async () => { if (page && detail && await traceExport(page.session.id, detail.event.id, page.revision)) toast.show({ kind: 'success', msg: t('trace.exported') }); })}><DownloadIcon className="w-4 h-4" />{t('trace.export')}</button></>}>
      {loadingDetail ? <div className="arca-trace-empty">{t('trace.loading')}</div> : detail ? <><div className="arca-trace-inspector-meta"><code>{detail.event.call_id ?? detail.event.id}</code><span>{t('trace.rawHint')}</span></div>
        <div className="arca-trace-tabs" role="tablist" aria-label={t('trace.inspect')}>{(['text', 'raw', ...(detail.event.kind === 'tool_call' ? ['response'] as const : [])] as const).map(value => <button type="button" role="tab" aria-selected={tab === value} key={value} onClick={() => setTab(value)}>{t(`trace.${value}`)}</button>)}</div>
        <BoundedText key={`${detail.event.id}:${tab}`} text={rawText || t('trace.noText')} raw={tab !== 'text'} className="arca-trace-body" /></> : <div className="arca-trace-empty">{t('trace.detailUnavailable')}</div>}
    </DialogShell>}
  </div>;
}
