import { useState } from "react";
import type { InstallState } from "../hooks/useInstallState";
import { useToast } from "./Toast";
import { DialogShell } from "./DialogShell";
import { formatError } from "../lib/format";
import { LinkIcon, SparklesIcon } from "./icons";

/** Official curated collection preset for the shared subscription flow. */
const OFFICIAL_SOURCE = {
  url: "https://raw.githubusercontent.com/Scoheart/mux/main/data/registry.json",
  name: "Arca 精选",
};

/** Add a remote MCP config source, optionally using the Arca curated preset. */
export function SubscribeDialog({
  state,
  onClose,
}: {
  state: InstallState;
  onClose: () => void;
}) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const canSubmit = url.trim().length > 0 && !busy;

  const submit = async () => {
    if (!canSubmit) return;
    setBusy(true);
    try {
      const v = await state.subscribe(url.trim(), name.trim() || undefined);
      toast.show({ kind: "success", msg: `已订阅 ${v.name} · ${v.server_count} 项` });
      onClose();
    } catch (e) {
      toast.show({ kind: "error", msg: "订阅失败：" + formatError(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      className="arca-dialog-subscription"
      kind="editor"
      size="md"
      title="添加订阅"
      subtitle="输入 MCP 配置 URL，或使用 Arca 精选。"
      busy={busy}
      onClose={onClose}
      footerEnd={
        <>
          <button onClick={onClose} disabled={busy} className="btn-ghost">取消</button>
          <button disabled={!canSubmit} onClick={submit} className="btn-primary">
            {busy ? "订阅中…" : "订阅"}
          </button>
        </>
      }
    >
      <div className="arca-subscription-form">
        <button
          type="button"
          onClick={() => { setUrl(OFFICIAL_SOURCE.url); setName(OFFICIAL_SOURCE.name); }}
          className="arca-subscription-preset"
          title="使用 Arca 精选"
        >
          <SparklesIcon className="w-4 h-4" />
          <span>使用 Arca 精选</span>
        </button>
        <label className="arca-dialog-field">
          <span><LinkIcon className="w-4 h-4" />配置 URL <i>*</i></span>
          <input
            autoFocus
            className="arca-dialog-input arca-dialog-input-mono"
            placeholder="https://example.com/mcp.json"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") submit(); }}
          />
        </label>
        <label className="arca-dialog-field">
          <span>名称</span>
          <input
            className="arca-dialog-input"
            placeholder="留空则用 URL 主机名"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") submit(); }}
          />
        </label>
      </div>
    </DialogShell>
  );
}
