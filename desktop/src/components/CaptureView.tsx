import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import type { AgentInfo } from '../lib/types';
import { captureDetail, captureEnvironment, captureProxy, captureExport, captureInstallEngine, captureOpenCertificate, captureSessions, captureDelta, mergeCaptureDelta, captureStart, captureStop, type CaptureEnvironment, type CaptureProxy, type CaptureSession, type CaptureSnapshot, type CapturedFlow, type FlowSummary } from '../lib/capture';
import { copyToClipboard } from '../lib/api';
import { formatError } from '../lib/format';
import { ToastNotice, useToast } from './Toast';
import { DialogShell } from './DialogShell';
import { AgentGlyph } from './brandIcons';
import { RefreshIcon, DownloadIcon, SearchIcon, SlidersIcon } from './icons';
import { BoundedText } from './BoundedText';
import './CaptureView.css';

const stateLabel: Record<string, string> = { starting: '启动中', running: '正在记录', stopped: '已停止', failed: '记录失败' };
const phaseLabel: Record<string, string> = { pending: '等待响应', receiving: '接收中', streaming: '流式传输', complete: '完成', failed: '失败', cancelled: '已中断' };
function address(url: string, withParameters = false) {
  try { const parsed = new URL(url); let path = parsed.pathname + parsed.search; try { path = decodeURI(path); } catch { /* Preserve malformed escapes for inspection. */ } return { host: parsed.host, path, pathname: parsed.pathname, parameters: withParameters ? [...parsed.searchParams] : [] }; }
  catch { return { host: '(未知主机)', path: url, pathname: url, parameters: [] as [string, string][] }; }
}
function size(bytes?: number | null) { return bytes == null ? '—' : bytes < 1024 ? `${bytes} B` : bytes < 1048576 ? `${(bytes / 1024).toFixed(1)} KB` : `${(bytes / 1048576).toFixed(1)} MB`; }
function elapsed(ms?: number | null) { return ms == null ? '—' : ms >= 1000 ? `${(ms / 1000).toFixed(2)} s` : `${ms} ms`; }
function phase(flow: FlowSummary) { return flow.phase ?? (flow.error ? 'failed' : flow.status != null ? 'complete' : 'pending'); }
function pretty(body: string) { try { return JSON.stringify(JSON.parse(body), null, 2); } catch { return body; } }

interface Tree { name: string; path: string; children: Map<string, Tree>; flows: FlowSummary[]; count: number }
function requestTree(flows: FlowSummary[]) {
  const roots = new Map<string, Tree>();
  for (const flow of flows) {
    const url = address(flow.url);
    if (!roots.has(url.host)) roots.set(url.host, { name: url.host, path: url.host, children: new Map(), flows: [], count: 0 });
    let node = roots.get(url.host)!; node.count++;
    for (const folder of url.pathname.split('/').filter(Boolean).slice(0, -1).slice(0, 20)) {
      if (!node.children.has(folder)) node.children.set(folder, { name: folder, path: `${node.path}/${folder}`, children: new Map(), flows: [], count: 0 });
      node = node.children.get(folder)!; node.count++;
    }
    node.flows.push(flow);
  }
  return [...roots.values()].sort((a, b) => a.name.localeCompare(b.name));
}
function TreeNode({ node, selected, onSelect }: { node: Tree; selected: string; onSelect: (id: string) => void }) {
  return <details className="capture-tree-node" open><summary title={node.path}><span>{node.name}</span><small>{node.count}</small></summary><div>
    {[...node.children.values()].sort((a, b) => a.name.localeCompare(b.name)).map(child => <TreeNode key={child.path} node={child} selected={selected} onSelect={onSelect} />)}
    {node.flows.map(flow => <button key={flow.id} className="capture-tree-leaf" data-selected={selected === flow.id} onClick={() => onSelect(flow.id)} title={flow.url}>
      <span className="capture-method">{flow.method}</span><span>{address(flow.url).path.split('/').pop() || '/'}</span></button>)}
  </div></details>;
}

export function CaptureView({ agents, agentId, arcaProxy }: { agents: AgentInfo[]; agentId: string; arcaProxy: string | null }) {
  const toast = useToast();
  const [environment, setEnvironment] = useState<CaptureEnvironment | null>(null);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [snapshot, setSnapshot] = useState<CaptureSnapshot | null>(null);
  const [targetId, setTargetId] = useState('');
  const [egress, setEgress] = useState('auto');
  const [proxy, setProxy] = useState('');
  const [automaticProxy, setAutomaticProxy] = useState<CaptureProxy | null>(null);
  const [proxyFailure, setProxyFailure] = useState('');
  const [directory, setDirectory] = useState('');
  const [launchMode, setLaunchMode] = useState<'fresh' | 'existing'>('fresh');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [navigation, setNavigation] = useState<'structure' | 'sequence'>('structure');
  const [flow, setFlow] = useState<CapturedFlow | null>(null);
  const [flowId, setFlowId] = useState('');
  const [tab, setTab] = useState<'overview' | 'request' | 'response' | 'timing'>('overview');
  const [format, setFormat] = useState<'body' | 'headers' | 'parameters' | 'raw'>('body');
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [setupOnly, setSetupOnly] = useState(false);
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [awaitingExtension, setAwaitingExtension] = useState(false);
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState('');
  const [treeWidth, setTreeWidth] = useState(205);
  const [listHeight, setListHeight] = useState(195);
  const workbench = useRef<HTMLDivElement>(null);
  const rightPane = useRef<HTMLDivElement>(null);
  const requestRows = useRef(new Map<string, HTMLTableRowElement>());
  const drag = useRef<{ axis: 'x' | 'y'; id: number } | null>(null);
  const epoch = useRef(0);
  const revision = useRef<{ sessionId: string; value: string } | null>(null);
  const active = snapshot?.status.state === 'running' || snapshot?.status.state === 'starting';
  const support = environment?.agents?.find(agent => agent.agent_id === agentId);
  const processProxy = launchMode === 'fresh' && Boolean(support?.explicit_proxy);
  const targets = environment?.targets.filter(target => target.agent_ids.includes(agentId)) ?? [];
  const selectedAgent = agents.find(agent => agent.id === agentId);
  useEffect(() => {
    if (egress !== 'auto' || !environment?.supported || active || busy) return;
    let disposed = false;
    setAutomaticProxy(null); setProxyFailure('');
    const timer = setTimeout(() => {
      void captureProxy(agentId).then(value => { if (!disposed) setAutomaticProxy(value); })
        .catch(error => { if (!disposed) setProxyFailure(formatError(error)); });
    }, 200);
    return () => { disposed = true; clearTimeout(timer); };
  }, [agentId, egress, settingsOpen, arcaProxy, Boolean(environment?.supported), active, busy]);
  const refresh = useCallback(async () => {
    const [nextEnvironment, allSessions] = await Promise.all([captureEnvironment(), captureSessions()]);
    const agentSessions = allSessions.filter(session => session.agent_id === agentId);
    setEnvironment(nextEnvironment); setSessions(agentSessions);
    return [nextEnvironment, agentSessions] as const;
  }, [agentId]);
  useEffect(() => {
    if (!awaitingExtension && (!settingsOpen || busy)) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await captureEnvironment();
        if (disposed) return;
        setEnvironment(next);
        if (awaitingExtension && next.extension_enabled && !settingsOpen) return;
      } catch { /* The capture request owns setup errors; background checks retry. */ }
      if (!disposed) timer = setTimeout(() => void poll(), 2000);
    };
    timer = setTimeout(() => void poll(), 2000);
    return () => { disposed = true; clearTimeout(timer); };
  }, [awaitingExtension, settingsOpen, busy, agentId]);
  useEffect(() => {
    let disposed = false; const generation = ++epoch.current;
    setLoading(true); setFailure(''); setFlowId(''); setFlow(null); revision.current = null;
    void refresh().then(async ([, list]) => {
      if (disposed || generation !== epoch.current) return;
      const selected = list[0];
      if (!selected) { setSnapshot(null); }
      else {
        const current = await captureDelta(selected.id);
        if (!disposed && generation === epoch.current) {
          revision.current = { sessionId: current.session.id, value: current.revision };
          setSnapshot(mergeCaptureDelta(null, current));
        }
      }
    }).catch(error => { if (!disposed) setFailure(formatError(error)); }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; epoch.current++; };
  }, [refresh]);
  useEffect(() => {
    if (!settingsOpen || busy) return;
    const onFocus = () => { void refresh().catch(error => setFailure(formatError(error))); };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [settingsOpen, busy, refresh]);
  useEffect(() => {
    const matches = environment?.targets.filter(target => target.agent_ids.includes(agentId)) ?? [];
    setTargetId(current => matches.some(target => target.id === current) ? current : matches[0]?.id ?? '');
  }, [agentId, environment]);
  useEffect(() => {
    if (!environment) return;
    setLaunchMode(!support || support.ca_scope === 'system' && targets.length ? 'existing' : 'fresh');
  }, [agentId, support?.ca_scope, Boolean(environment)]);
  useEffect(() => {
    if (!snapshot || !active || busy) return;
    const id = snapshot.session.id; const generation = epoch.current;
    let disposed = false; let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      let delay = 500;
      try {
        const current = await captureDelta(id, revision.current?.sessionId === id ? revision.current.value : null);
        if (!disposed && generation === epoch.current && current.session.id === id) {
          revision.current = { sessionId: id, value: current.revision };
          setSnapshot(previous => previous?.session.id === id ? mergeCaptureDelta(previous, current) : previous);
          if (!current.upserts.length && !current.removed.length) delay = 1200;
        }
      } catch (error) { if (!disposed) { setFailure(formatError(error)); delay = 2000; } }
      if (!disposed) timer = setTimeout(() => void poll(), delay);
    };
    timer = setTimeout(() => void poll(), 500);
    return () => { disposed = true; clearTimeout(timer); };
  }, [snapshot?.session.id, active, busy]);
  const selectedSummary = snapshot?.flows.find(item => item.id === flowId);
  useEffect(() => {
    if (!snapshot || !flowId) { setFlow(null); return; }
    let disposed = false;
    void captureDetail(snapshot.session.id, flowId).then(value => { if (!disposed) setFlow(value); }).catch(error => { if (!disposed) setFailure(formatError(error)); });
    return () => { disposed = true; };
  }, [snapshot?.session.id, flowId, selectedSummary?.updated_at, selectedSummary?.status, selectedSummary?.phase]);
  const flows = useMemo(() => (snapshot?.flows ?? []).filter(value =>
    `${value.method} ${value.url}`.toLowerCase().includes(query.toLowerCase()) &&
    (filter === 'all' || (filter === 'errors' ? value.error || (value.status ?? 0) >= 400 : filter === 'streaming' ? phase(value) === 'streaming' : phase(value) === 'complete' && (value.status ?? 0) < 400))
  ).sort((a, b) => a.started_at - b.started_at || a.id.localeCompare(b.id)), [snapshot?.flows, query, filter]);
  const tree = useMemo(() => navigation === 'structure' ? requestTree(flows) : [], [flows, navigation]);
  const visibleSelection = flows.some(item => item.id === flowId);
  useEffect(() => { requestRows.current.get(flowId)?.scrollIntoView({ block: 'nearest' }); }, [flowId, snapshot?.session.id]);
  const perform = async (operation: () => Promise<void>) => {
    setBusy(true); try { await operation(); setFailure(''); }
    catch (error) { setFailure(formatError(error)); }
    finally { setBusy(false); }
  };
  const openSettings = () => {
    setSetupOnly(false);
    setSettingsOpen(true);
    void perform(async () => { await refresh(); });
  };
  const selectSession = async (id: string) => {
    const generation = ++epoch.current; revision.current = null; setFlowId(''); setFlow(null);
    if (!id) { setSnapshot(null); await refresh(); return; }
    const current = await captureDelta(id);
    if (generation === epoch.current) {
      revision.current = { sessionId: id, value: current.revision };
      setSnapshot(mergeCaptureDelta(null, current));
    }
  };
  const needsSystemCa = Boolean(environment?.engine) && support?.ca_scope === 'system' && !environment?.certificate_trusted;
  const prerequisite = loading ? '正在检测抓包环境…' : !environment?.supported ? '当前仅支持 macOS 按进程抓包。'
    : egress === 'auto' && proxyFailure ? proxyFailure
    : egress === 'auto' && !automaticProxy ? '正在检测上游代理…'
    : needsSystemCa ? '此客户端需要一次证书授权。在钥匙串访问中添加 mitmproxy 证书，并将 SSL 设为“始终信任”，完成后返回。'
    : launchMode === 'fresh' && !support ? '此 Agent 尚未支持自动启动，请选择运行中的进程。'
    : launchMode === 'fresh' && !support?.available ? '请先安装 Agent 或配置它的启动方式。'
    : launchMode === 'existing' && !targetId ? '未检测到运行中的进程，可切换为启动新进程。' : '';
  const canStart = !prerequisite && agentId && !active && !busy && (egress !== 'custom' || proxy.trim());
  const start = () => perform(async () => {
    const generation = ++epoch.current; revision.current = null; setFlowId(''); setFlow(null);
    setPreparing(true);
    try {
      if (!environment?.engine) {
        setInstalling(true);
        await captureInstallEngine();
        setInstalling(false);
        const [next] = await refresh();
        if (generation !== epoch.current) return;
        if (support?.ca_scope === 'system' && !next.certificate_trusted) { setSetupOnly(true); setSettingsOpen(true); return; }
      }
      setAwaitingExtension(!processProxy && !environment?.extension_enabled);
      const current = await captureStart({ agent_id: agentId, target_id: targetId, egress, proxy_url: egress === 'custom' ? proxy : null }, { launch: launchMode === 'fresh', directory: directory.trim() || undefined });
      if (generation === epoch.current) { setSnapshot(current); setTargetId(current.session.target_id); setSettingsOpen(false); await refresh(); }
    } finally { setAwaitingExtension(false); setPreparing(false); setInstalling(false); }
  });
  const begin = () => {
    if (needsSystemCa) { setSetupOnly(true); setSettingsOpen(true); }
    else if (prerequisite) openSettings();
    else void start();
  };
  const message = tab === 'request' || tab === 'response' ? flow?.[tab] : undefined;
  const content = format === 'headers' ? message?.headers.map(([key, value]) => `${key}: ${value}`).join('\n') ?? '' : format === 'parameters' && flow ? address(flow.url, true).parameters.map(([key, value]) => `${key}: ${value}`).join('\n') : message?.body ?? '';
  const pending = flow && ['pending', 'receiving', 'streaming'].includes(phase(flow));
  const showsBody = (tab === 'request' || tab === 'response') && (format === 'body' || format === 'raw');
  const bodyPreview = useMemo(() => !showsBody ? '' : content ? format === 'body' ? pretty(content) : content : pending ? '等待完整响应事件…' : flow?.error ?? '（空正文）', [showsBody, content, format, Boolean(pending), flow?.error]);
  const status = snapshot ? stateLabel[snapshot.status.state] ?? snapshot.status.state : '未开始';
  const css = { '--capture-tree-width': `${treeWidth}px`, '--capture-list-height': `${listHeight}px` } as CSSProperties;
  return <div className="arca-capture" style={css}>
    <header className="capture-titlebar"><div><h1>网络抓包</h1><span className="capture-recording-state" data-live={Boolean(active)}>{status}</span>
      {selectedAgent && <span className="capture-agent-label"><AgentGlyph id={selectedAgent.id} size={18} />{selectedAgent.name}</span>}</div>
      <div className="capture-title-actions"><button className="capture-button" disabled={busy || loading} onClick={openSettings}><SlidersIcon className="w-4 h-4" />记录选项</button>
        <button className="capture-button capture-record" data-live={Boolean(active)} disabled={busy || loading} onClick={() => active && snapshot ? void perform(async () => { setSnapshot(await captureStop(snapshot.session.id)); await refresh(); }) : begin()}>
          <span aria-hidden="true" />{preparing ? '正在准备…' : active ? '停止记录' : environment && !environment.engine ? '下载并开始' : '开始记录'}</button></div>
    </header>
    <div className="capture-toolbar"><select aria-label="抓包会话" value={snapshot?.session.id ?? ''} disabled={busy || Boolean(active)} onChange={event => void perform(() => selectSession(event.target.value))}>
      <option value="">新会话</option>{sessions.map(session => <option key={session.id} value={session.id}>{session.agent_name} · {new Date(session.started_at).toLocaleString()}</option>)}</select>
      <div className="capture-view-switch" aria-label="请求导航"><button aria-pressed={navigation === 'structure'} onClick={() => setNavigation('structure')}>Structure</button><button aria-pressed={navigation === 'sequence'} onClick={() => setNavigation('sequence')}>Sequence</button></div>
      <label className="capture-search"><SearchIcon className="w-4 h-4" /><input type="search" aria-label="搜索请求" placeholder="Filter · 主机、路径、方法" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <select aria-label="筛选请求" value={filter} onChange={event => setFilter(event.target.value)}><option value="all">全部</option><option value="success">已完成</option><option value="errors">错误</option><option value="streaming">流式</option></select>
      <button className="capture-button" title="导出脱敏会话 JSON" aria-label="导出会话" disabled={!snapshot || busy} onClick={() => void perform(async () => { if (snapshot && await captureExport(snapshot.session.id)) toast.show({ kind: 'success', msg: '会话已导出。' }); })}><DownloadIcon className="w-4 h-4" /></button>
    </div>
    <ToastNotice message={failure} />
    <ToastNotice kind="info" noticeKey="capture-starting" persistent message={preparing && !awaitingExtension ? installing ? '正在下载并校验抓包组件，首次使用需要准备一次…' : '正在准备网络记录…' : null} />
    <ToastNotice kind="info" noticeKey="capture-extension-setup" persistent message={awaitingExtension
      ? environment?.extension_enabled ? '网络扩展已启用，正在准备记录…'
        : '正在安装并启用官方网络扩展。请按 macOS 提示允许 Mitmproxy Redirector；批准后会自动继续记录，无需刷新。'
      : null} action={awaitingExtension && !environment?.extension_enabled
        ? <button type="button" className="btn-secondary" onClick={() => void openUrl('x-apple.systempreferences:com.apple.LoginItems-Settings.extension')
          .catch(error => toast.show({ kind: 'error', msg: `无法打开授权设置：${formatError(error)}` }))}>打开授权设置</button> : undefined} />
    <div className="capture-workbench" data-navigation={navigation} ref={workbench}>
      {navigation === 'structure' && <><aside className="capture-tree" aria-label="按主机和路径分组"><div className="capture-pane-title">Structure <small>{tree.length} hosts</small></div>
        {tree.length ? tree.map(node => <TreeNode key={node.path} node={node} selected={flowId} onSelect={setFlowId} />) : <div className="capture-empty">尚无请求</div>}</aside>
        <div className="capture-divider capture-divider-x" role="separator" tabIndex={0} aria-label="调整连接树宽度" aria-orientation="vertical" aria-valuemin={160} aria-valuemax={400} aria-valuenow={treeWidth}
          onPointerDown={event => { drag.current = { axis: 'x', id: event.pointerId }; event.currentTarget.setPointerCapture(event.pointerId); }}
          onPointerMove={event => { if (drag.current?.axis === 'x' && drag.current.id === event.pointerId && workbench.current) setTreeWidth(Math.max(160, Math.min(400, event.clientX - workbench.current.getBoundingClientRect().left))); }}
          onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}
          onKeyDown={event => { if (['ArrowLeft', 'ArrowRight'].includes(event.key)) { event.preventDefault(); setTreeWidth(value => Math.max(160, Math.min(400, value + (event.key === 'ArrowLeft' ? -16 : 16)))); } }} /></>}
      <div className="capture-right" ref={rightPane}>
        <section className="capture-sequence" aria-label="请求时序"><div className="capture-pane-title">Sequence <small>{flows.length} requests</small></div>
          <div className="capture-table-scroll"><table className="capture-table" role="grid" aria-label="请求列表"><thead><tr><th>#</th><th>Method</th><th>Status</th><th>Host / Path</th><th>Size</th><th>Duration</th></tr></thead>
            <tbody>{flows.map((item, index) => <tr key={item.id} ref={row => { if (row) requestRows.current.set(item.id, row); else requestRows.current.delete(item.id); }} data-selected={flowId === item.id} data-phase={phase(item)} tabIndex={visibleSelection ? flowId === item.id ? 0 : -1 : index === 0 ? 0 : -1} aria-selected={flowId === item.id}
              onClick={() => setFlowId(item.id)} onKeyDown={event => {
                if (['Enter', ' '].includes(event.key)) { event.preventDefault(); setFlowId(item.id); }
                if (['ArrowUp', 'ArrowDown'].includes(event.key)) { event.preventDefault(); const next = Math.max(0, Math.min(flows.length - 1, index + (event.key === 'ArrowUp' ? -1 : 1))); setFlowId(flows[next].id); (event.currentTarget.parentElement?.children[next] as HTMLElement | undefined)?.focus(); }
              }}><td>{index + 1}</td><td className="capture-method">{item.method}</td><td data-error={Boolean(item.error || (item.status ?? 0) >= 400)}>{item.status ?? (phase(item) === 'failed' ? 'ERR' : '—')}</td>
              <td className="capture-address" title={item.url}><strong>{address(item.url).host}</strong><span>{address(item.url).path}</span></td><td>{size(item.response_bytes)}</td><td>{phase(item) === 'complete' ? elapsed(item.duration_ms) : phaseLabel[phase(item)] ?? '—'}</td></tr>)}</tbody>
          </table>{!flows.length && <div className="capture-empty">{active ? '等待请求…在目标 Agent 中执行操作。' : query || filter !== 'all' ? '没有匹配的请求。' : <><strong>记录 {selectedAgent?.name ?? agentId} 的网络请求</strong><span>{processProxy ? '点击开始记录，Arca 会打开 Agent 并自动配置连接。' : '点击开始记录，Arca 会检查所需环境并连接 Agent。'}</span></>}</div>}</div>
        </section>
        <div className="capture-divider capture-divider-y" role="separator" tabIndex={0} aria-label="调整请求列表高度" aria-orientation="horizontal" aria-valuemin={100} aria-valuenow={listHeight}
          onPointerDown={event => { drag.current = { axis: 'y', id: event.pointerId }; event.currentTarget.setPointerCapture(event.pointerId); }}
          onPointerMove={event => { if (drag.current?.axis === 'y' && drag.current.id === event.pointerId && rightPane.current) { const bounds = rightPane.current.getBoundingClientRect(); setListHeight(Math.max(100, Math.min(bounds.height - 160, event.clientY - bounds.top))); } }}
          onPointerUp={() => { drag.current = null; }} onPointerCancel={() => { drag.current = null; }}
          onKeyDown={event => { if (['ArrowUp', 'ArrowDown'].includes(event.key)) { event.preventDefault(); setListHeight(value => Math.max(100, Math.min((rightPane.current?.clientHeight ?? 500) - 160, value + (event.key === 'ArrowUp' ? -16 : 16)))); } }} />
        <section className="capture-detail" aria-label="请求响应详情"><div className="capture-detail-tabs" role="tablist" aria-label="请求详情">
          {(['overview', 'request', 'response', 'timing'] as const).map(value => <button key={value} role="tab" aria-selected={tab === value} onClick={() => { setTab(value); if (format === 'parameters' && value === 'response') setFormat('body'); }}>{value === 'overview' ? 'Overview' : value === 'request' ? 'Request' : value === 'response' ? 'Response' : 'Timing'}</button>)}
          {flow && <span className="capture-detail-phase" data-phase={phase(flow)}>{phaseLabel[phase(flow)]}</span>}</div>
          {!flow || flow.id !== flowId ? <div className="capture-empty">{flowId ? '正在读取请求…' : '选择请求，查看 Headers、正文和耗时。'}</div> : tab === 'overview' ? <dl className="capture-overview">
            <dt>URL</dt><dd className="capture-url">{flow.url}</dd><dt>Method</dt><dd>{flow.method}</dd><dt>Status</dt><dd>{flow.status ?? '尚未收到响应'}</dd>
            <dt>Protocol</dt><dd>{flow.request_http_version}{flow.response_http_version ? ` → ${flow.response_http_version}` : ''}</dd><dt>Started</dt><dd>{new Date(flow.started_at * 1000).toLocaleString()}</dd>
            <dt>Request size</dt><dd>{size(flow.request_bytes)}</dd><dt>Response size</dt><dd>{size(flow.response_bytes)}</dd><dt>Content-Type</dt><dd>{flow.content_type ?? '—'}</dd><dt>出口</dt><dd>{flow.proxy_url ?? '直连'}</dd>{flow.error && <><dt>Error</dt><dd>{flow.error}</dd></>}
          </dl> : tab === 'timing' ? <dl className="capture-overview"><dt>First byte</dt><dd>{elapsed(flow.time_to_first_byte_ms)}</dd><dt>Total</dt><dd>{pending ? '请求仍在进行' : elapsed(flow.duration_ms)}</dd><dt>Started</dt><dd>{new Date(flow.started_at * 1000).toLocaleTimeString()}</dd><dt>Last update</dt><dd>{flow.updated_at ? new Date(flow.updated_at * 1000).toLocaleTimeString() : '—'}</dd></dl> : <>
            <div className="capture-body-toolbar"><div role="tablist" aria-label="正文格式">{['body', 'headers', ...(tab === 'request' ? ['parameters'] : []), 'raw'].map(value => <button key={value} role="tab" aria-selected={format === value} onClick={() => setFormat(value as typeof format)}>{value === 'body' ? 'Pretty' : value === 'headers' ? 'Headers' : value === 'parameters' ? 'Query' : 'Raw'}</button>)}</div>
              <button className="capture-button" disabled={!content} onClick={() => void perform(async () => { await copyToClipboard(content); toast.show({ kind: 'success', msg: '已复制脱敏内容。' }); })}>复制</button><button className="capture-button" disabled={busy} onClick={() => void perform(async () => { if (snapshot) await captureExport(snapshot.session.id, flow.id); })}>导出 req/res</button></div>
            {format === 'headers' ? <div className="capture-content-scroll"><table className="capture-key-values"><tbody>{message?.headers.map(([key, value], index) => <tr key={index}><th>{key}</th><td>{value}</td></tr>)}</tbody></table></div> : format === 'parameters' ? <div className="capture-content-scroll"><table className="capture-key-values"><tbody>{address(flow.url, true).parameters.map(([key, value], index) => <tr key={index}><th>{key}</th><td>{value}</td></tr>)}</tbody></table></div> : <BoundedText key={`${snapshot?.session.id}:${flow.id}:${tab}:${format}`} text={bodyPreview} />}
            <div className="capture-body-meta">{size(message?.bytes)} · {message?.encoding === 'binary' ? '二进制正文未保存' : '敏感字段已脱敏'}{message?.truncated ? ' · 正文未完整保存' : ''}{flow.stream_type === 'websocket' ? ' · WebSocket 消息' : phase(flow) === 'streaming' ? ' · SSE 实时更新' : ''}</div>
          </>}
        </section>
      </div>
    </div>
    <footer className="capture-statusbar"><span>{snapshot ? `${snapshot.session.target_name} · ${snapshot.session.pids.length} 个初始进程` : '仅记录当前 Agent 及其辅助进程'}{snapshot?.session.proxy_url ? ` · ${snapshot.session.proxy_url}` : snapshot ? ' · 直连' : ''}</span><span>{snapshot?.status.message || `${snapshot?.flows.length ?? 0} requests · 本机脱敏记录`}</span></footer>
    {settingsOpen && <DialogShell kind="editor" size={setupOnly ? 'md' : 'wide'} className="capture-settings" title={setupOnly ? '准备 HTTPS 记录' : '记录选项'} subtitle={selectedAgent?.name ?? agentId} busy={busy} onClose={() => setSettingsOpen(false)}
      footerStart={setupOnly ? null : <span className="capture-setting-note">本机脱敏保存 · 单条正文 2 MiB</span>}
      footerEnd={<><button className="btn-secondary" onClick={() => setSettingsOpen(false)} disabled={busy}>关闭</button><button className="btn-primary" disabled={!canStart} onClick={() => void start()}>{awaitingExtension && !environment?.extension_enabled ? '等待系统授权…' : busy ? '正在准备…' : environment && !environment.engine ? '下载并开始' : launchMode === 'fresh' ? '启动并记录' : '开始记录'}</button></>}>
      <div className="capture-setting-fields">
        <div className="capture-setup-heading"><AgentGlyph id={agentId} size={28} /><div className="capture-setup-agent"><span>Agent</span><strong title={selectedAgent?.name ?? agentId}>{selectedAgent?.name ?? agentId}</strong></div><button className="capture-button" aria-label="刷新抓包环境" disabled={busy || loading} onClick={() => void perform(async () => { await refresh(); })}><RefreshIcon className="w-4 h-4" />刷新</button></div>
        {!setupOnly && <>
        <div className="capture-launch-modes" aria-label="记录方式"><button aria-pressed={launchMode === 'fresh'} disabled={busy || Boolean(active) || !support?.available} onClick={() => setLaunchMode('fresh')}><strong>启动并记录</strong><span>{support?.explicit_proxy ? '自动配置，无需系统网络扩展' : '打开独立客户端实例'}</span></button><button aria-pressed={launchMode === 'existing'} disabled={busy || Boolean(active)} onClick={() => setLaunchMode('existing')}><strong>记录已有实例</strong><span>使用系统网络扩展</span></button></div>
        {launchMode === 'existing' ? <><label>目标进程<select value={targetId} disabled={Boolean(active) || busy} onChange={event => setTargetId(event.target.value)}><option value="">{targets.length ? '选择进程' : '未检测到运行进程'}</option>{targets.map(target => <option key={target.id} value={target.id}>{target.name} · {target.pids.length} 个进程</option>)}</select></label>{!environment?.certificate_trusted && support?.ca_scope !== 'system' && <p className="capture-setting-note">已有进程需自行信任抓包证书；遇到 TLS 错误时，请使用“启动新进程”。</p>}</>
          : support?.kind === 'cli' && <label>工作目录<input value={directory} disabled={busy} onChange={event => setDirectory(event.target.value)} placeholder="留空使用 Agent 的默认目录" /></label>}
        <div className="capture-setting-columns"><label>网络出口<select value={egress} disabled={Boolean(active) || busy} onChange={event => setEgress(event.target.value)}><option value="auto">自动沿用当前网络</option><option value="direct">直连</option><option value="arca" disabled={!arcaProxy}>Arca 代理</option><option value="custom">自定义 HTTP 代理</option></select></label>{egress === 'custom' ? <label>上游代理地址<input disabled={Boolean(active) || busy} value={proxy} onChange={event => setProxy(event.target.value)} placeholder="HTTP 上游代理 URL" /></label> : <div className="capture-egress-summary"><span>{egress === 'auto' ? automaticProxy?.proxy_url ?? automaticProxy?.source ?? (proxyFailure || '正在检测上游代理…') : egress === 'arca' ? arcaProxy : '直连'}</span><small>{egress === 'auto' && automaticProxy?.proxy_url ? `${automaticProxy.source} · 抓包后经此代理访问网络` : '需要代理访问网络时，请确认上游地址；代理失败不会绕过'}</small></div>}</div>
        <div className="capture-setting-health" aria-label="抓包环境"><span data-ready={Boolean(environment?.engine)}>抓包组件 {loading ? '检测中' : environment?.engine ? '已就绪' : '待安装'}</span><span data-ready={processProxy || Boolean(environment?.extension_enabled)}>网络扩展 {loading ? '检测中' : processProxy ? '无需启用' : environment?.extension_enabled ? '已启用' : '待授权'}</span><span data-ready={!needsSystemCa && Boolean(environment)}>证书 {loading ? '检测中' : processProxy ? '自动配置' : environment?.certificate_trusted ? '已授权' : support?.ca_scope === 'system' ? '待授权' : launchMode === 'fresh' ? '随启动配置' : '需客户端信任'}</span></div>
        {!processProxy && environment?.supported && environment.engine && !environment.extension_enabled && <p className="capture-setting-note">首次记录时，按 macOS 提示允许 Mitmproxy Redirector，Arca 会自动继续。</p>}
        </>}
        {setupOnly && !prerequisite && <p className="capture-setting-note">已准备就绪，可以开始记录。</p>}
        {environment?.supported && !environment.engine && <p className="capture-setting-note">首次开始时，Arca 会下载并校验官方 mitmproxy 组件，仅安装到 Arca 私有目录。</p>}
        {prerequisite && <div className="capture-setup-feedback" role="status"><p>{prerequisite}</p>{environment?.engine && needsSystemCa && <button className="btn-secondary" disabled={busy} onClick={() => void perform(captureOpenCertificate)}>打开证书设置</button>}</div>}
        {support?.ca_scope === 'mixed' && !environment?.certificate_trusted && <p className="capture-setting-note">浏览器网络栈仍需要系统信任抓包证书，追加证书仅覆盖支持它的运行时。</p>}
        {launchMode === 'existing' && environment?.other_extensions.length ? <p className="capture-setting-note">{environment.other_extensions.join('、')} 也在接管网络；请求为空时，检查目标应用的代理规则和实际发起进程。</p> : null}
      </div>
    </DialogShell>}
  </div>;
}
