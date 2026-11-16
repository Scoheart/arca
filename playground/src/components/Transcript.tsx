import { memo, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { letter, type ModelResponse, type ModelTarget, type RunDraft } from "../types";

interface Props {
  drafts: RunDraft[];
  models: ModelTarget[];
  active?: number;
  prompt: string | undefined;
  loading: boolean;
  notice: string | null;
  onSelect(id: number): void;
}

export function Transcript({ drafts, models, active, prompt, loading, notice, onSelect }: Props) {
  const empty = prompt === undefined;
  return (
    <div className="transcript">
      <div className="transcript-inner">
        {notice && <div className="app-notice" role="status">{notice}</div>}
        {empty ? (
          <div className="empty-state">
            <h2>同题对比</h2>
            <p>{loading ? "正在读取已保存的模型…" : "输入提示词，在运行设置中选择要对照的模型。"}</p>
          </div>
        ) : (
          <>
            <div className="sent-prompt" aria-label="本轮提示词">{prompt}</div>
            <section className="lanes" aria-label="对比结果" data-many={drafts.length > 1}>
              {drafts.map((draft, index) => (
                <ResultLane key={draft.id} draft={draft}
                  index={index} active={active === draft.id}
                  model={models.find((model) => model.id === draft.profileId)}
                  onSelect={() => onSelect(draft.id)} />
              ))}
            </section>
          </>
        )}
      </div>
    </div>
  );
}

const statuses = { ready: "待运行", running: "生成中", done: "已返回", error: "失败" };
function ResultLane({ draft, index, active, model, onSelect }: {
  draft: RunDraft; index: number; active: boolean; model?: ModelTarget; onSelect(): void;
}) {
  const result = draft.result;
  const [copyState, setCopyState] = useState<{text: string; label: string}>();
  const success = result.status === "done" ? result.response : undefined;
  return (
    <article className="lane" data-active={active} data-status={result.status}>
      <div className="lane-bar">
        <button type="button" className="lane-select" onClick={onSelect}
          aria-label={"打开请求 " + letter(index) + " 的设置"} title={model?.name ?? "选择模型"}>
          <span className="request-tag">{letter(index)}</span>
          <span className="lane-name">{model?.name ?? "选择模型"}</span>
        </button>
        <span className="card-state" aria-live="polite">{statuses[result.status]}</span>
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
      {success ? (
        <>
          <p className="result-meta">{metricText(success)}</p>
          <AnswerText text={success.text} />
        </>
      ) : result.status === "error" ? (
        <p className="result-error" role="status">
          {result.error.message}{" "}
          {[result.error.upstream_status ? "HTTP " + result.error.upstream_status : "",
            result.error.parameter ? "参数 " + result.error.parameter : "",
            result.error.code].filter(Boolean).join(" · ")}
        </p>
      ) : (
        <p className="result-text">{result.status === "running" ? "等待模型回答…" : "此请求本轮尚未运行。"}</p>
      )}
    </article>
  );
}

function metricText(response: ModelResponse): string {
  const items = [(response.elapsed_ms / 1000).toFixed(2) + " 秒", "HTTP " + response.upstream_status];
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
