import { useCallback, useEffect, useRef, useState } from "react";
import { describeError, loadModels, runModel, type SharedInput } from "./api";
import { RunSettings } from "./components/RunSettings";
import { Transcript } from "./components/Transcript";
import { defaultParameters, type ModelTarget, type Parameters, type RunDraft, type RunResult } from "./types";

const MAX_REQUESTS = 6;

interface Batch extends SharedInput {
  total: number;
  finished: number;
  failed: number;
}

export function App({ token }: { token: string | null }) {
  const [models, setModels] = useState<ModelTarget[]>([]);
  const [drafts, setDrafts] = useState<RunDraft[]>([]);
  const [selectedId, setSelectedId] = useState<number>();
  const [prompt, setPrompt] = useState("");
  const [instructions, setInstructions] = useState("");
  const [batch, setBatch] = useState<Batch>();
  const [open, setOpen] = useState(() => !window.matchMedia?.("(max-width: 820px)").matches);
  const [loading, setLoading] = useState(!!token);
  const [running, setRunning] = useState(false);
  const [notice, setNotice] = useState<string | null>(token ? null : "请在 Arca 的 Models 页面打开模型试验台。");
  const nextId = useRef(1);
  const inFlight = useRef(false);
  const loadInFlight = useRef(false);
  const revisions = useRef(new Map<string, string>());
  const settingsButton = useRef<HTMLButtonElement>(null);
  const mobile = () => !!window.matchMedia?.("(max-width: 820px)").matches;

  const selected = drafts.find((draft) => draft.id === selectedId) ?? drafts[0];
  const profileFor = (draft: RunDraft) => models.find((model) => model.id === draft.profileId);
  const selectedProfile = selected ? profileFor(selected) : undefined;
  const readyCount = drafts.filter(profileFor).length;
  const canRun = !!token && !loading && !running && !!prompt.trim();
  const canAdd = !loading && !running && models.length > 0 && drafts.length < MAX_REQUESTS;

  const refresh = useCallback(async () => {
    if (!token || loadInFlight.current || inFlight.current) return;
    setLoading(true);
    setNotice(null);
    loadInFlight.current = true;
    try {
      const updated = await loadModels(token);
      const defaults: RunDraft[] = updated.slice(0, 2).map((target) => ({
        id: nextId.current++, profileId: target.id, parameters: defaultParameters(), result: { status: "ready" },
      }));
      const previous = revisions.current;
      const current = new Map(updated.map((target) => [target.id, target.revision]));
      revisions.current = current;
      setModels(updated);
      setDrafts((rows) => {
        if (!rows.length) return defaults;
        return rows.map((row) => {
          const revision = current.get(row.profileId);
          if (revision && revision === previous.get(row.profileId)) return row;
          return { ...row, profileId: revision ? row.profileId : "", result: { status: "ready" } };
        });
      });
    } catch (error) {
      setNotice(describeError(error).message);
    } finally {
      loadInFlight.current = false;
      setLoading(false);
    }
  }, [token]);

  useEffect(() => { void refresh(); }, [refresh]);

  function updateDraft(id: number, change: Partial<Pick<RunDraft, "profileId" | "parameters" | "result">>) {
    setDrafts((list) => list.map((draft) => draft.id === id ? { ...draft, ...change } : draft));
  }

  function addDraft(copy?: RunDraft) {
    if (!canAdd) return;
    const unselected = models.find((model) => !drafts.some((draft) => draft.profileId === model.id));
    const id = nextId.current++;
    const draft: RunDraft = {
      id, profileId: copy?.profileId ?? (unselected ?? models[0]).id,
      parameters: copy ? { ...copy.parameters } : defaultParameters(), result: { status: "ready" },
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

  function changeParameter(name: keyof Parameters, value: string) {
    if (!selected || running) return;
    updateDraft(selected.id, { parameters: { ...selected.parameters, [name]: value }, result: { status: "ready" } });
  }

  function closeSettings() {
    setOpen(false);
    if (mobile()) requestAnimationFrame(() => settingsButton.current?.focus());
  }

  async function run(chosen: RunDraft[]) {
    if (!token || inFlight.current || loadInFlight.current || !prompt.trim()) return;
    const runs = chosen.flatMap((draft) => {
      const model = profileFor(draft);
      return model ? [{ draft, model }] : [];
    });
    if (!runs.length) return;
    const shared = { prompt, instructions };
    const isNewInput = !batch || batch.prompt !== prompt || batch.instructions !== instructions;
    const ids = new Set(runs.map(({ draft }) => draft.id));
    inFlight.current = true;
    setRunning(true);
    setBatch({ ...shared, total: runs.length, finished: 0, failed: 0 });
    setDrafts((list) => list.map((draft) => {
      if (ids.has(draft.id)) return { ...draft, result: { status: "running" } };
      return isNewInput ? { ...draft, result: { status: "ready" } } : draft;
    }));
    if (mobile()) setOpen(false);

    try {
      await Promise.all(runs.map(async ({ draft, model }) => {
        let result: RunResult;
        try {
          result = { status: "done", response: await runModel(token, draft, model, shared) };
        } catch (error) {
          result = { status: "error", error: describeError(error) };
        }
        updateDraft(draft.id, { result });
        setBatch((prior) => prior && ({
          ...prior, finished: prior.finished + 1, failed: prior.failed + (result.status === "error" ? 1 : 0),
        }));
      }));
    } finally {
      setRunning(false);
      inFlight.current = false;
    }
  }

  const status = loading ? "正在连接" : batch
    ? "已返回 " + batch.finished + " / " + batch.total + (batch.failed ? " · " + batch.failed + " 失败" : "")
    : notice ? "未连接" : "本机连接";

  return (
    <main className="studio" data-settings={open ? "open" : "closed"}
      onKeyDown={(event) => {
        if (mobile() && open && event.key === "Escape") closeSettings();
        if ((event.metaKey || event.ctrlKey) && event.key === "Enter" && canRun && readyCount) {
          event.preventDefault();
          void run(drafts);
        }
      }}>
      <section className="workspace" aria-labelledby="workspace-title" inert={open && mobile()}>
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
        <Transcript drafts={drafts} models={models} active={selected?.id} prompt={batch?.prompt}
          loading={loading} notice={notice} onSelect={(id) => { setSelectedId(id); setOpen(true); }} />
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
                title="⌘ Enter / Ctrl Enter" onClick={() => void run(drafts)}>
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
          profileId: id, parameters: defaultParameters(), result: { status: "ready" },
        })}
        onDuplicate={() => addDraft(selected)} onRemove={removeSelected}
        onRun={() => selected && void run([selected])} />
    </main>
  );
}
