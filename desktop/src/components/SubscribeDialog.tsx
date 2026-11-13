import { ImeInput } from "./ImeInput";
import { useState } from "react";
import type { InstallState } from "../hooks/useInstallState";
import { useToast } from "./Toast";
import { DialogShell } from "./DialogShell";
import { formatError } from "../lib/format";
import { LinkIcon, SparklesIcon } from "./icons";

/** Official curated collection preset for the shared subscription flow. */
const OFFICIAL_SOURCE = {
  url: "https://raw.githubusercontent.com/Scoheart/arca/main/data/registry.json",
  name: "Arca 精选",
};

/** Shared subscription form fields reused across the standalone dialog and Add MCP editor. */
export function SubscriptionFields({
  url,
  name,
  disabled = false,
  autoFocus = false,
  urlLabel,
  nameLabel,
  namePlaceholder,
  presetLabel = "使用 Arca 精选",
  onUrlChange,
  onNameChange,
  onUsePreset,
  onSubmit,
}: {
  url: string;
  name: string;
  disabled?: boolean;
  autoFocus?: boolean;
  urlLabel?: string;
  nameLabel?: string;
  namePlaceholder?: string;
  presetLabel?: string;
  onUrlChange: (url: string) => void;
  onNameChange: (name: string) => void;
  onUsePreset?: () => void;
  onSubmit?: () => void;
}) {
  return (
    <div className="arca-subscription-form">
      <button
        type="button"
        disabled={disabled}
        onClick={() => {
          if (onUsePreset) onUsePreset();
          else {
            onUrlChange(OFFICIAL_SOURCE.url);
            onNameChange(OFFICIAL_SOURCE.name);
          }
        }}
        className="arca-subscription-preset"
        title={presetLabel}
      >
        <SparklesIcon className="w-4 h-4" />
        <span>{presetLabel}</span>
      </button>
      <label className="arca-dialog-field">
        <span><LinkIcon className="w-4 h-4" />{urlLabel ?? "配置 URL"} <i>*</i></span>
        <ImeInput
          autoFocus={autoFocus}
          disabled={disabled}
          className="arca-dialog-input arca-dialog-input-mono"
          placeholder="https://example.com/mcp.json"
          value={url}
          onChange={(e) => onUrlChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onSubmit?.();
          }}
        />
      </label>
      <label className="arca-dialog-field">
        <span>{nameLabel ?? "名称"}</span>
        <ImeInput
          disabled={disabled}
          className="arca-dialog-input"
          placeholder={namePlaceholder ?? "留空则用 URL 主机名"}
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onSubmit?.();
          }}
        />
      </label>
    </div>
  );
}

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
          <button onClick={onClose} disabled={busy} className="btn-secondary">取消</button>
          <button disabled={!canSubmit} onClick={submit} className="btn-primary">
            {busy ? "订阅中…" : "订阅"}
          </button>
        </>
      }
    >
      <SubscriptionFields
        url={url}
        name={name}
        disabled={busy}
        autoFocus
        onUrlChange={setUrl}
        onNameChange={setName}
        onSubmit={submit}
      />
    </DialogShell>
  );
}
