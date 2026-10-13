import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type { AgentInfo } from '../lib/types';
import { captureDetail, captureEnvironment, captureExport, captureSessions, captureDelta, mergeCaptureDelta, captureStart, captureStop, captureLaunchAgent, type CaptureEnvironment, type CaptureSession, type CaptureSnapshot, type CapturedFlow, type FlowSummary } from '../lib/capture';
import { copyToClipboard } from '../lib/api';
import { formatError } from '../lib/format';
import { useToast } from './Toast';
import { DialogShell } from './DialogShell';
import { AgentGlyph } from './brandIcons';
import { RefreshIcon, DownloadIcon, SearchIcon, SlidersIcon } from './icons';
import { BoundedText } from './BoundedText';
import './CaptureView.css';

const stateLabel: Record<string, string> = { starting: '启动中', running: '正在记录', stopped: '已停止', failed: '启动失败' };
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

export function CaptureView({ agents, initialAgentId, muxProxy }: { agents: AgentInfo[]; initialAgentId?: string; muxProxy: string | null }) {
  const toast = useToast();
  const [environment, setEnvironment] = useState<CaptureEnvironment | null>(null);
  const [sessions, setSessions] = useState<CaptureSession[]>([]);
  const [snapshot, setSnapshot] = useState<CaptureSnapshot | null>(null);
  const [agentId, setAgentId] = useState(initialAgentId ?? agents[0]?.id ?? '');
  const [targetId, setTargetId] = useState('');
  const [egress, setEgress] = useState(muxProxy ? 'mux' : 'direct');
  const [proxy, setProxy] = useState('');
  const [directory, setDirectory] = useState('');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [navigation, setNavigation] = useState<'structure' | 'sequence'>('structure');
  const [flow, setFlow] = useState<CapturedFlow | null>(null);
  const [flowId, setFlowId] = useState('');
  const [tab, setTab] = useState<'overview' | 'request' | 'response' | 'timing'>('overview');
  const [format, setFormat] = useState<'body' | 'headers' | 'parameters' | 'raw'>('body');
  const [settingsOpen, setSettingsOpen] = useState(Boolean(initialAgentId));
  const [busy, setBusy] = useState(false);
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
  const captureAgents = agents.filter(agent => environment?.agents?.some(profile => profile.agent_id === agent.id) || environment?.targets.some(target => target.agent_ids.includes(agent.id)));
  const targets = environment?.targets.filter(target => target.agent_ids.includes(agentId)) ?? [];
  const selectedAgent = agents.find(agent => agent.id === (snapshot?.session.agent_id ?? agentId));
  const refresh = useCallback(async () => {
    const result = await Promise.all([captureEnvironment(), captureSessions()]);
    setEnvironment(result[0]); setSessions(result[1]); return result;
  }, []);
  useEffect(() => {
    if (!initialAgentId && captureAgents.length && !captureAgents.some(agent => agent.id === agentId)) {
      setAgentId(captureAgents.find(agent => agent.id === 'cursor')?.id ?? captureAgents[0].id);
    }
  }, [environment, agents, initialAgentId, agentId]);
  useEffect(() => {
    let disposed = false; const generation = ++epoch.current;
    void refresh().then(async ([, list]) => {
      if (disposed || generation !== epoch.current) return;
      if (!list.length) setSettingsOpen(true);
      else {
        const current = await captureDelta(list[0].id);
        if (!disposed && generation === epoch.current) {
          revision.current = { sessionId: current.session.id, value: current.revision };
          setSnapshot(mergeCaptureDelta(null, current));
        }
      }
    }).catch(error => { if (!disposed) setFailure(formatError(error)); }).finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; epoch.current++; };
  }, [refresh]);
  useEffect(() => { if (initialAgentId) { setAgentId(initialAgentId); setSettingsOpen(true); } }, [initialAgentId]);
  useEffect(() => {
    const matches = environment?.targets.filter(target => target.agent_ids.includes(agentId)) ?? [];
    setTargetId(current => matches.some(target => target.id === current) ? current : matches[0]?.id ?? '');
  }, [agentId, environment]);
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
    catch (error) { const text = formatError(error); setFailure(text); toast.show({ kind: 'error', msg: text }); }
    finally { setBusy(false); }
  };
  const selectSession = async (id: string) => {
    const generation = ++epoch.current; revision.current = null; setFlowId(''); setFlow(null);
    if (!id) { setSnapshot(null); setSettingsOpen(true); return; }
    const current = await captureDelta(id);
    if (generation === epoch.current) {
      revision.current = { sessionId: id, value: current.revision };
      setSnapshot(mergeCaptureDelta(null, current));
    }
  };
  const needsSystemCa = support?.ca_scope === 'system' && !environment?.certificate_trusted;
  const canStart = !loading && environment?.supported && environment.engine && environment.extension_enabled && !needsSystemCa && agentId && targetId && !active && !busy;
  const start = () => perform(async () => {
    const generation = ++epoch.current; revision.current = null; setFlowId(''); setFlow(null);
    const current = await captureStart({ agent_id: agentId, target_id: targetId, egress, proxy_url: egress === 'custom' ? proxy : null });
    if (generation === epoch.current) { setSnapshot(current); setSettingsOpen(false); await refresh(); }
  });
  const message = tab === 'request' || tab === 'response' ? flow?.[tab] : undefined;
  const content = format === 'headers' ? message?.headers.map(([key, value]) => `${key}: ${value}`).join('\n') ?? '' : format === 'parameters' && flow ? address(flow.url, true).parameters.map(([key, value]) => `${key}: ${value}`).join('\n') : message?.body ?? '';
  const pending = flow && ['pending', 'receiving', 'streaming'].includes(phase(flow));
  const showsBody = (tab === 'request' || tab === 'response') && (format === 'body' || format === 'raw');
  const bodyPreview = useMemo(() => !showsBody ? '' : content ? format === 'body' ? pretty(content) : content : pending ? '等待完整响应事件…' : flow?.error ?? '（空正文）', [showsBody, content, format, Boolean(pending), flow?.error]);
  const status = snapshot ? stateLabel[snapshot.status.state] ?? snapshot.status.state : '未开始';
  const css = { '--capture-tree-width': `${treeWidth}px`, '--capture-list-height': `${listHeight}px` } as CSSProperties;
  return <div className="mux-capture" style={css}>
    <header className="capture-titlebar"><div><h1>网络抓包</h1><span className="capture-recording-state" data-live={Boolean(active)}>{status}</span>
      {selectedAgent && <span className="capture-agent-label"><AgentGlyph id={selectedAgent.id} size={18} />{selectedAgent.name}</span>}</div>
      <div className="capture-title-actions"><button className="capture-button" onClick={() => setSettingsOpen(true)}><SlidersIcon className="w-4 h-4" />设置</button>
        <button className="capture-button capture-record" data-live={Boolean(active)} disabled={busy} onClick={() => active && snapshot ? void perform(async () => { setSnapshot(await captureStop(snapshot.session.id)); await refresh(); }) : setSettingsOpen(true)}>
          <span aria-hidden="true" />{active ? '停止记录' : '开始记录'}</button></div>
    </header>
    <div className="capture-toolbar"><select aria-label="抓包会话" value={snapshot?.session.id ?? ''} disabled={busy || Boolean(active)} onChange={event => void perform(() => selectSession(event.target.value))}>
      <option value="">新会话</option>{sessions.map(session => <option key={session.id} value={session.id}>{session.agent_name} · {new Date(session.started_at).toLocaleString()}</option>)}</select>
      <div className="capture-view-switch" aria-label="请求导航"><button aria-pressed={navigation === 'structure'} onClick={() => setNavigation('structure')}>Structure</button><button aria-pressed={navigation === 'sequence'} onClick={() => setNavigation('sequence')}>Sequence</button></div>
      <label className="capture-search"><SearchIcon className="w-4 h-4" /><input type="search" aria-label="搜索请求" placeholder="Filter · 主机、路径、方法" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <select aria-label="筛选请求" value={filter} onChange={event => setFilter(event.target.value)}><option value="all">全部</option><option value="success">已完成</option><option value="errors">错误</option><option value="streaming">流式</option></select>
      <button className="capture-button" title="导出脱敏会话 JSON" aria-label="导出会话" disabled={!snapshot || busy} onClick={() => void perform(async () => { if (snapshot && await captureExport(snapshot.session.id)) toast.show({ kind: 'success', msg: '会话已导出。' }); })}><DownloadIcon className="w-4 h-4" /></button>
    </div>
    {failure && <p className="capture-notice" role="alert">{failure}</p>}
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
          </table>{!flows.length && <div className="capture-empty">{active ? '等待请求…在目标 Agent 中执行操作。' : '开始记录后，请求会显示在这里。'}</div>}</div>
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
    <footer className="capture-statusbar"><span>{snapshot ? `${snapshot.session.target_name} · ${snapshot.session.pids.length} 个初始进程` : '按 Agent 选择实际进程'}{snapshot?.session.proxy_url ? ` · ${snapshot.session.proxy_url}` : snapshot ? ' · 直连' : ''}</span><span>{snapshot?.status.message || `${snapshot?.flows.length ?? 0} requests · 本机脱敏记录`}</span></footer>
    {settingsOpen && <DialogShell kind="editor" size="wide" className="capture-settings" title="抓包设置" subtitle="选择 Agent、实际进程和出口" busy={busy} onClose={() => setSettingsOpen(false)}
      footerEnd={<><button className="btn-secondary" onClick={() => setSettingsOpen(false)} disabled={busy}>关闭</button><button className="btn-primary" disabled={!canStart} title={needsSystemCa ? '需先让系统信任抓包 CA' : undefined} onClick={() => void start()}>开始记录</button></>}>
      <div className="capture-setting-fields"><label>Agent<select value={agentId} disabled={Boolean(active)} onChange={event => setAgentId(event.target.value)}>{!captureAgents.some(agent => agent.id === agentId) && agentId && <option value={agentId}>{agents.find(agent => agent.id === agentId)?.name ?? agentId} · 尚未适配</option>}{captureAgents.map(agent => <option key={agent.id} value={agent.id}>{agent.name}</option>)}</select></label>
        <label>运行中的进程<select value={targetId} disabled={Boolean(active)} onChange={event => setTargetId(event.target.value)}><option value="">{targets.length ? '选择进程' : '未检测到运行进程'}</option>{targets.map(target => <option key={target.id} value={target.id}>{target.name} · {target.pids.length} 个进程</option>)}</select></label>
        <div className="capture-launch-row"><button className="btn-secondary" disabled={busy || Boolean(active) || !support?.available || !environment?.certificate_present || support.ca_scope === 'system' && !environment.certificate_trusted} onClick={() => void perform(async () => { await captureLaunchAgent(agentId, directory || undefined); await refresh(); })}>{support?.ca_scope === 'system' ? '启动客户端' : '追加 CA 启动'}</button><button className="btn-secondary" disabled={busy} onClick={() => void perform(async () => { await refresh(); })}><RefreshIcon className="w-4 h-4" />刷新进程</button></div>
        {support?.kind === 'cli' && <label>工作目录（使用默认目录可留空）<input value={directory} onChange={event => setDirectory(event.target.value)} placeholder="/path/to/project" /></label>}
        <p className="capture-setting-note">追加 CA 保留证书校验，只影响新进程。已打开的桌面应用可能复用原进程，需要结束原实例后重新启动。</p>
        {support && support.ca_scope !== 'process' && <p className="capture-setting-note">{support.ca_scope === 'mixed' ? '追加 CA 覆盖采用它的运行时及辅助进程请求；浏览器网络栈仍需要系统信任抓包 CA。' : '此桌面客户端需要系统信任抓包 CA；进程环境变量不能覆盖全部网络请求。'}</p>}
        <div className="capture-setting-columns"><label>出口<select value={egress} disabled={Boolean(active)} onChange={event => setEgress(event.target.value)}><option value="direct">直连</option><option value="mux" disabled={!muxProxy}>MUX 代理</option><option value="custom">自定义 HTTP 代理</option></select></label>{egress === 'custom' && <label>代理地址<input disabled={Boolean(active)} value={proxy} onChange={event => setProxy(event.target.value)} placeholder="http://127.0.0.1:6789" /></label>}</div>
        <div className="capture-setting-health"><span>mitmproxy {environment?.engine ? '可用' : '未安装'}</span><span>网络扩展 {environment?.extension_enabled ? '已启用' : '未启用'}</span><span>CA {environment?.certificate_trusted ? '系统已信任' : environment?.certificate_present ? '系统未信任，需验证客户端' : '未生成'}</span></div>
        {environment?.other_extensions.length ? <p className="capture-setting-note">{environment.other_extensions.join('、')} 也在接管网络。流量为空时检查目标应用的规则；不要停用其他应用的连接。</p> : null}
        <p className="capture-setting-note">只记录所选进程及后代的 HTTP／HTTPS。上游失败不回退直连；内部环回 TLS、证书固定及 QUIC 需按客户端验证。正文上限 2 MiB，会话上限 5000 条／100 MiB。</p>
        <p className="capture-setting-note">Agent 自己的代理或继承的代理环境变量可能把请求交给其他进程；流量为空时核对实际发起进程。</p>
      </div>
    </DialogShell>}
  </div>;
}
