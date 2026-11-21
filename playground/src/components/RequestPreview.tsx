import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { describeError, previewRequest, type RequestPreviewData } from "../api";
import type { RequestSnapshot, SharedInput } from "../types";

interface Props {
  token: string;
  request: RequestSnapshot;
  shared: SharedInput;
  onClose(): void;
}

export function RequestPreview({ token, request, shared, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [preview, setPreview] = useState<RequestPreviewData>();
  const [error, setError] = useState<string>();
  const [copyLabel, setCopyLabel] = useState("复制 JSON");

  useEffect(() => {
    const controller = new AbortController();
    void previewRequest(token, request.parameters, request.model, shared, controller.signal).then(
      (result) => { if (!controller.signal.aborted) setPreview(result); },
      (problem: unknown) => { if (!controller.signal.aborted) setError(describeError(problem).message); },
    );
    return () => controller.abort();
  }, [token, request, shared]);

  useEffect(() => {
    dialog.current?.showModal();
  }, []);

  const body = preview ? JSON.stringify(preview.body, null, 2) : "";
  return createPortal(
    <dialog className="request-preview" ref={dialog} aria-labelledby="preview-heading"
      onClose={onClose} onCancel={(event) => { event.preventDefault(); onClose(); }}
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <header>
        <h2 id="preview-heading">请求 {request.label} 的预览</h2>
        <button className="icon-button" type="button" aria-label="关闭请求预览" onClick={onClose}>×</button>
      </header>
      <div className="preview-body">
        {preview ? (
          <>
            <p className="preview-route"><strong>{preview.method}</strong> <code>{preview.url}</code></p>
            <p className="preview-meta">{preview.accept}
              {preview.authentication_header ? " · 凭据请求头 " + preview.authentication_header : ""}
            </p>
            <pre><code>{body}</code></pre>
            <button type="button" className="tool-button" onClick={async () => {
              try {
                await navigator.clipboard.writeText(body);
                setCopyLabel("已复制");
              } catch {
                setCopyLabel("请选中并复制");
              }
            }}>{copyLabel}</button>
          </>
        ) : <p role="status" className={error ? "result-error" : "help"}>{error ?? "正在生成请求预览…"}</p>}
      </div>
    </dialog>,
    document.body,
  );
}
