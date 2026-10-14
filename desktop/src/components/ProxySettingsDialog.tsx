import { ToastNotice } from "./Toast";
import { FormEvent, useEffect, useState } from "react";
import type { ProxySettings } from "../lib/types";
import { DialogShell } from "./DialogShell";

export function ProxySettingsDialog({
  proxyUrl,
  onClose,
  onSave,
}: {
  proxyUrl: string | null;
  onClose: () => void;
  onSave: (proxyUrl: string | null) => Promise<ProxySettings>;
}) {
  const [value, setValue] = useState(proxyUrl ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    setValue(proxyUrl ?? "");
  }, [proxyUrl]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (saving) return;
    setSaving(true);
    setError("");
    try {
      await onSave(value.trim() || null);
      onClose();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  };

  return (
    <DialogShell
      className="arca-dialog-proxy-settings"
      kind="editor"
      size="sm"
      title="网络代理"
      subtitle="用于 Arca 的联网请求。"
      busy={saving}
      onClose={onClose}
      footerEnd={(
        <>
          <button type="button" className="btn-ghost" disabled={saving} onClick={onClose}>
            取消
          </button>
          <button type="submit" className="btn-primary" form="arca-proxy-settings-form" disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
        </>
      )}
    >
      <form id="arca-proxy-settings-form" className="arca-proxy-form" onSubmit={submit}>
        <label htmlFor="arca-proxy-url">代理地址</label>
        <input
          id="arca-proxy-url"
          data-modal-initial-focus
          type="text"
          inputMode="url"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          value={value}
          placeholder="http://127.0.0.1:7890"
          disabled={saving}
          aria-describedby="arca-proxy-help"
          aria-invalid={error ? "true" : undefined}
          onChange={(event) => {
            setValue(event.target.value);
            if (error) setError("");
          }}
        />
        <div id="arca-proxy-help" className="arca-proxy-help">
          <span>HTTP · SOCKS4 · SOCKS5</span>
          <span>留空即关闭</span>
        </div>
        <ToastNotice message={error} />
      </form>
    </DialogShell>
  );
}
