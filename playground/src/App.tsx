import { useCallback, useEffect, useRef, useState } from "react";
import { describeError, loadModels } from "./api";
import { RequestPreview } from "./components/RequestPreview";
import { RunSettings } from "./components/RunSettings";
import { Transcript } from "./components/Transcript";
import { defaultParameters, letter, type ModelTarget, type Parameters, type RequestSnapshot,
  type RunDraft, type RunRound, type SharedInput } from "./types";
import { exportRound, useRounds } from "./useRounds";

const MAX_REQUESTS = 6;
const NARROW = "(max-width: 820px)";

export function App({ token }: { token: string | null }) {
  const [models, setModels] = useState<ModelTarget[]>([]);
  const [drafts, setDrafts] = useState<RunDraft[]>([]);
  const [selectedId, setSelectedId] = useState<number>();
  const [prompt, setPrompt] = useState("");
  const [instructions, setInstructions] = useState("");
  const [narrow, setNarrow] = useState(() => !!window.matchMedia?.(NARROW).matches);
  const [open, setOpen] = useState(() => !narrow);
  const [preview, setPreview] = useState<{ request: RequestSnapshot; shared: SharedInput }>();
  const [loading, setLoading] = useState(!!token);
  const [notice, setNotice] = useState<string | null>(token ? null : "请在 Arca 的 Models 页面打开模型试验台。");
  const nextId = useRef(1);
  const loadInFlight = useRef(false);
  const settingsButton = useRef<HTMLButtonElement>(null);
  const { rounds, round, runningRound, select, start, stop, retry } = useRounds(token);

  const running = !!runningRound;
  const selected = drafts.find((draft) => draft.id === selectedId) ?? drafts[0];
  const profileFor = (draft: RunDraft) => models.find((model) => model.id === draft.profileId);
  const selectedProfile = selected ? profileFor(selected) : undefined;
  const readyCount = drafts.filter(profileFor).length;
  const canRun = !!token && !loading && !running && !!prompt.trim();
  const canAdd = !loading && !running && models.length > 0 && drafts.length < MAX_REQUESTS;

  const refresh = useCallback(async () => {
    if (!token || loadInFlight.current) return;
    setLoading(true);
    setNotice(null);
    loadInFlight.current = true;
    try {
      const updated = await loadModels(token);
      const defaults: RunDraft[] = updated.slice(0, 2).map((target) => ({
        id: nextId.current++, profileId: target.id, parameters: defaultParameters(),
      }));
      const current = new Set(updated.map((target) => target.id));
      setModels(updated);
      setDrafts((rows) => {
        if (!rows.length) return defaults;
        return rows.map((row) => current.has(row.profileId) ? row : { ...row, profileId: "" });
      });
    } catch (error) {
      setNotice(describeError(error).message);
    } finally {
      loadInFlight.current = false;
      setLoading(false);
    }
  }, [token]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    const query = window.matchMedia?.(NARROW);
    const update = () => setNarrow(!!query?.matches);
    query?.addEventListener("change", update);
    return () => query?.removeEventListener("change", update);
  }, []);

  function updateDraft(id: number, change: Partial<Pick<RunDraft, "profileId" | "parameters">>) {
    setDrafts((list) => list.map((draft) => draft.id === id ? { ...draft, ...change } : draft));
  }

  function addDraft(copy?: RunDraft) {
    if (!canAdd) return;
    const unselected = models.find((model) => !drafts.some((draft) => draft.profileId === model.id));
    const id = nextId.current++;
    const draft: RunDraft = {
      id, profileId: copy?.profileId ?? (unselected ?? models[0]).id,
      parameters: copy ? { ...copy.parameters } : defaultParameters(),
    };
    setDrafts((list) => [...list, draft]);
    setSelectedId(id);
  }

  function removeSelected() {
    if (!selected || running || drafts.length <= 1) return;
    const index = drafts.findIndex((draft) => draft.id === selected.id);
    const remaining = drafts.filter((draft) => draft.id !== selected.id);
    setDrafts(remaining);
    setSelectedId(remaining[Math.min(index, remaining.length - 1)].id);
  }

  function changeParameter<K extends keyof Parameters>(name: K, value: Parameters[K]) {
    if (!selected || running) return;
    updateDraft(selected.id, { parameters: { ...selected.parameters, [name]: value } });
  }

  function closeSettings() {
    setOpen(false);
    if (narrow) requestAnimationFrame(() => settingsButton.current?.focus());
  }

  function snapshot(draft: RunDraft, model: ModelTarget): RequestSnapshot {
    return { id: draft.id, label: letter(drafts.indexOf(draft)), model, parameters: { ...draft.parameters } };
  }

  function run(chosen: RunDraft[]) {
    if (!canRun || loadInFlight.current) return;
    const requests = chosen.flatMap((draft) => {
      const model = profileFor(draft);
      return model ? [snapshot(draft, model)] : [];
    });
    if (start({ prompt, instructions }, requests)) {
      setNotice(null);
      if (narrow) closeSettings();
    }
  }

  function restore(round: RunRound) {
    if (running || loading) return;
    const drafts = round.requests.map(({ model, parameters }) => {
      const current = models.find((candidate) => candidate.id === model.id && candidate.revision === model.revision);
      return { id: nextId.current++, profileId: current?.id ?? "", parameters: { ...parameters } };
    });
    setDrafts(drafts);
    setSelectedId(drafts[0]?.id);
    setPrompt(round.prompt);
    setInstructions(round.instructions);
    setNotice(drafts.some((draft) => !draft.profileId) ? "有模型或连接已更改，请在运行设置中重新选择。" : null);
    setOpen(true);
  }

  const results = round?.requests.map(({ result }) => result.status) ?? [];
  const finished = results.filter((status) => status !== "running").length;
  const failed = results.filter((status) => status === "error").length;
  const status = loading ? "正在连接" : round
    ? "已完成 " + finished + " / " + results.length + (failed ? " · " + failed + " 失败" : "")
    : notice ? "未连接" : "本机连接";

  return (
    <main className="studio" data-settings={open ? "open" : "closed"}
      onKeyDown={(event) => {
        if (preview) return;
        if (narrow && open && event.key === "Escape") closeSettings();
        if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && canRun && readyCount) {
          event.preventDefault();
          run(drafts);
        }
      }}>
      <section className="workspace" aria-labelledby="workspace-title" inert={open && narrow}>
        <header className="workspace-bar">
          <div className="workspace-title">
            <span className="wordmark" aria-hidden="true">Arca</span>
            <h1 id="workspace-title">模型试验台</h1>
            <span className="status" id="batch-status" role="status" aria-live="polite">{status}</span>
          </div>
          <button type="button" className="tool-button settings-toggle" id="settings-toggle" ref={settingsButton}
            aria-label="运行设置" aria-expanded={open} aria-controls="run-settings" title="运行设置"
            onClick={() => setOpen(!open)}>
            <span className="panel-icon" aria-hidden="true" /><span>运行设置</span>
          </button>
        </header>
        <Transcript round={round} rounds={rounds} loading={loading} notice={notice}
          running={running} canRetry={!loading && (!runningRound || runningRound.id === round?.id)}
          onSelect={select} onRestore={restore} onExport={exportRound}
          onRetry={retry} onStop={stop} />
        <div className="composer-wrap">
          <div className="composer">
            <label className="sr-only" htmlFor="prompt">共同提示词</label>
            <textarea id="prompt" rows={2} maxLength={65536} placeholder="输入提示词…"
              value={prompt} onChange={(event) => setPrompt(event.target.value)} disabled={running} />
            <div className="composer-actions">
              <button type="button" className="tool-button compare-summary" aria-controls="run-settings"
                title="选择模型及运行参数" onClick={() => setOpen(true)}>
                <span className="compare-icon" aria-hidden="true" />
                <span>{readyCount ? readyCount + " 个对照请求" : "选择模型"}</span>
              </button>
              <button type="button" className="run-button" id="send" disabled={!canRun || !readyCount}
                title="⌘ Enter / Ctrl Enter" onClick={() => run(drafts)}>
                <span>{running ? "运行中…" : readyCount > 1 ? "运行 " + readyCount + " 个" : "运行"}</span>
                <span className="run-arrow" aria-hidden="true">↑</span>
              </button>
            </div>
          </div>
        </div>
      </section>
      <button type="button" className="settings-scrim" onClick={closeSettings} tabIndex={-1} aria-hidden="true" hidden={!open} />
      <RunSettings open={open} models={models} drafts={drafts} selected={selected} profile={selectedProfile}
        instructions={instructions} running={running} loading={loading}
        canRun={canRun && !!selectedProfile} canAdd={canAdd} canRefresh={!!token && !loading && !running}
        onClose={closeSettings} onRefresh={() => void refresh()} onAdd={() => addDraft()}
        onSelect={setSelectedId} onInstructions={setInstructions} onChangeParameter={changeParameter}
        onChangeProfile={(id) => selected && updateDraft(selected.id, {
          profileId: id, parameters: selected.profileId ? defaultParameters() : selected.parameters,
        })}
        onDuplicate={() => addDraft(selected)} onRemove={removeSelected}
        onRun={() => selected && run([selected])}
        onPreview={() => {
          if (selected && selectedProfile && canRun) {
            setPreview({ request: snapshot(selected, selectedProfile), shared: { prompt, instructions } });
          }
        }} />
      {token && preview && <RequestPreview token={token} {...preview} onClose={() => setPreview(undefined)} />}
    </main>
  );
}
