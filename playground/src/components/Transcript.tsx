import { memo, useEffect, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { protocols, type ModelResponse, type RoundRequest, type RunRound } from "../types";

interface Props {
  round: RunRound | undefined;
  rounds: RunRound[];
  loading: boolean;
  running: boolean;
  canRetry: boolean;
  notice: string | null;
  onSelect(id: number): void;
  onRestore(round: RunRound): void;
  onExport(round: RunRound): void;
  onRetry(round: RunRound, request: RoundRequest): void;
  onStop(roundId: number, requestId: number): void;
}

export function Transcript({ round, rounds, loading, running, canRetry, notice, onSelect,
  onRestore, onExport, onRetry, onStop }: Props) {
  return (
    <div className="transcript">
      <div className="transcript-inner">
        {notice && <div className="app-notice" role="status">{notice}</div>}
        {!round ? (
          <div className="empty-state">
            <h2>同题对比</h2>
            <p>{loading ? "正在读取已保存的模型…" : "输入提示词，在运行设置中选择要对照的模型。"}</p>
          </div>
        ) : (
          <>
            <RoundToolbar round={round} rounds={rounds} restoreDisabled={running || loading}
              onSelect={onSelect} onRestore={onRestore} onExport={onExport} />
            <div className="sent-prompt" aria-label="本轮提示词">{round.prompt}</div>
            <section className="lanes" aria-label="对比结果" data-many={round.requests.length > 1}>
              {round.requests.map((request) => (
                <ResultLane key={round.id + "/" + request.id} request={request} canRetry={canRetry}
                  onRetry={() => onRetry(round, request)}
                  onStop={() => onStop(round.id, request.id)} />
              ))}
            </section>
          </>
        )}
      </div>
    </div>
  );
}

function RoundToolbar({ round, rounds, restoreDisabled, onSelect, onRestore, onExport }: {
  round: RunRound; rounds: RunRound[]; restoreDisabled: boolean;
  onSelect(id: number): void; onRestore(round: RunRound): void; onExport(round: RunRound): void;
}) {
  const isRunning = round.requests.some(({ result }) => result.status === "running");
  return (
    <div className="round-bar">
      <label className="round-selector">
        <span>运行记录</span>
        <select value={round.id} onChange={(event) => onSelect(Number(event.target.value))}>
          {rounds.map((item) => (
            <option value={item.id} key={item.id}>
              第 {item.id} 轮 · {new Date(item.createdAt).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
              {" · " + item.prompt.trim().replace(/\s+/g, " ").slice(0, 30)}
            </option>
          ))}
        </select>
      </label>
      <div className="round-actions">
        <button type="button" className="tool-button" disabled={restoreDisabled} onClick={() => onRestore(round)}>载入本轮设置</button>
        <button type="button" className="tool-button" disabled={isRunning} onClick={() => onExport(round)}>导出 JSON</button>
      </div>
    </div>
  );
}

const statuses = { running: "生成中", done: "已返回", error: "失败", cancelled: "已停止等待" };
function ResultLane({ request, canRetry, onRetry, onStop }: {
  request: RoundRequest; canRetry: boolean; onRetry(): void; onStop(): void;
}) {
  const { model, label, result } = request;
  const [copyState, setCopyState] = useState<{text: string; label: string}>();
  const success = result.status === "done" ? result.response : undefined;
  return (
    <article className="lane" data-status={result.status}>
      <div className="lane-bar">
        <h3 className="lane-title" title={model.name}>
          <span className="request-tag">{label}</span>
          <span className="lane-name">{model.name}</span>
        </h3>
        <span className="card-state" aria-live="polite">{statuses[result.status]}</span>
        {result.status === "running" && <button type="button" className="tool-button" onClick={onStop}>停止等待</button>}
        {(result.status === "error" || result.status === "cancelled") &&
          <button type="button" className="tool-button retry" disabled={!canRetry} onClick={onRetry}>重试</button>}
        {success && (
          <button type="button" className="tool-button copy" onClick={async () => {
            try {
              await navigator.clipboard.writeText(success.text);
              setCopyState({ text: success.text, label: "已复制" });
            } catch {
              setCopyState({ text: success.text, label: "请选择并复制" });
            }
          }}>
            {copyState?.text === success.text ? copyState.label : "复制"}
          </button>
        )}
      </div>
      <p className="lane-route">{[model.provider !== model.name ? model.provider : "", protocols[model.protocol]].filter(Boolean).join(" · ")}</p>
      {success ? (
        <>
          <p className="result-meta">
            {metricText(success)}
            {request.parameters.streaming && model.streaming_available && !success.streamed ? " · Provider 完整返回" : ""}
          </p>
          <AnswerText text={success.text} />
        </>
      ) : result.status === "running" ? (
        <>
          <RunClock since={result.startedAt} receiving={!!result.text} />
          {result.text && <p className="result-text stream-text">{result.text}</p>}
        </>
      ) : result.status !== "done" && (
        <>
          {result.status === "error" && <p className="result-error" role="status">
            {result.error.message}{" "}
            {[result.error.upstream_status ? "HTTP " + result.error.upstream_status : "",
              result.error.parameter ? "参数 " + result.error.parameter : "",
              result.error.code].filter(Boolean).join(" · ")}
          </p>}
          {result.text && <AnswerText text={result.text} />}
        </>
      )}
      <RunParameters request={request} />
    </article>
  );
}

function RunClock({ since, receiving }: { since: number; receiving: boolean }) {
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(performance.now()), 500);
    return () => clearInterval(timer);
  }, []);
  return <p className="result-meta">{receiving ? "正在接收" : "等待 Provider"} · {Math.max(0, (now - since) / 1000).toFixed(1)} 秒</p>;
}

function RunParameters({ request: { model, parameters: p } }: { request: RoundRequest }) {
  return (
    <details className="run-parameters">
      <summary>本轮参数</summary>
      <dl>
        <div><dt>Model ID</dt><dd>{model.model}</dd></div>
        <div><dt>返回方式</dt><dd>{p.streaming && model.streaming_available ? "逐步显示" : "完整返回"}</dd></div>
        {model.protocol !== "gemini-interactions" && <>
          <div><dt>Temperature</dt><dd>{p.temperature || "默认"}</dd></div>
          <div><dt>Top P</dt><dd>{p.top_p || "默认"}</dd></div>
        </>}
        <div><dt>最大输出 Token</dt><dd>{p.max_output_tokens || "默认"}</dd></div>
        {p.reasoning_effort && <div><dt>推理强度</dt><dd>{p.reasoning_effort}</dd></div>}
      </dl>
      {p.extra.trim() && <pre className="saved-json"><code>{p.extra}</code></pre>}
    </details>
  );
}

function metricText(response: ModelResponse): string {
  const items = [(response.elapsed_ms / 1000).toFixed(2) + " 秒", "HTTP " + response.upstream_status];
  if (response.first_token_ms != null) items.push("首段 " + (response.first_token_ms / 1000).toFixed(2) + " 秒");
  const labels = { input_tokens: "输入", output_tokens: "输出", reasoning_tokens: "推理" } as const;
  for (const key of Object.keys(labels) as Array<keyof typeof labels>) {
    const count = response.usage[key];
    if (count !== null) items.push(labels[key] + " " + count.toLocaleString() + " Token");
  }
  const finishLabels = { length: "达到输出上限", filtered: "Provider 过滤", incomplete: "回答未完成" };
  if (response.finish) items.push(finishLabels[response.finish]);
  return items.join(" · ");
}

const AnswerText = memo(function AnswerText({ text }: { text: string }) {
  return (
    <div className="result-text">
      <Markdown skipHtml remarkPlugins={[remarkGfm]} components={{
        a: ({ href, children }) => href && /^https?:\/\//i.test(href)
          ? <a href={href} target="_blank" rel="noreferrer">{children}</a>
          : <span>{children}</span>,
        img: ({ alt }) => <span>{alt || "图片"}</span>,
        table: ({ children }) => <div className="table-overflow"><table>{children}</table></div>,
      }}>{text}</Markdown>
    </div>
  );
});
