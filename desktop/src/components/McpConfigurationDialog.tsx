import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../lib/api";
import { CheckIcon, CopyIcon } from "./icons";
import type { McpConfigurationView, McpInspectionRequest } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { ResourceIcon } from "./resourcePresentation";
import "./NativePluginsPanel.css";

async function copyConfigText(text: string): Promise<boolean> {
  try {
    if (typeof api.copyToClipboard === "function") {
      await api.copyToClipboard(text);
      return true;
    }
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    return false;
  }
  return false;
}

export function McpConfigurationSection({ request }: { request: McpInspectionRequest }) {
  const { t } = useTranslation();
  const requestKey = JSON.stringify(request);
  const [attempt, setAttempt] = useState(0);
  const [copiedIndex, setCopiedIndex] = useState<number | null>(null);
  const [loaded, setLoaded] = useState<{ key: string; value?: McpConfigurationView; failed?: boolean } | null>(null);
  const pending = useRef<{ key: string; promise: Promise<McpConfigurationView> } | null>(null);
  const copyTimer = useRef<number | null>(null);
  const key = `${requestKey}:${attempt}`;
  useEffect(() => {
    setCopiedIndex(null);
    let active = true;
    // StrictMode reuses this request; nothing is cached outside the open viewer.
    if (pending.current?.key !== key) pending.current = {
      key, promise: Promise.resolve().then(() => api.inspectMcpConfiguration(JSON.parse(requestKey) as McpInspectionRequest)),
    };
    void pending.current.promise.then(value => {
      if (active) setLoaded(value.agent_id === (JSON.parse(requestKey) as McpInspectionRequest).agent_id
        ? { key, value } : { key, failed: true });
    }, () => { if (active) setLoaded({ key, failed: true }); });
    return () => {
      active = false;
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current);
        copyTimer.current = null;
      }
    };
  }, [key, requestKey]);

  const handleCopy = async (index: number, value: string) => {
    const copied = await copyConfigText(value);
    if (!copied) return;
    setCopiedIndex(index);
    if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => {
      setCopiedIndex(null);
      copyTimer.current = null;
    }, 1500);
  };

  const current = loaded?.key === key ? loaded : null;
  return <section className="arca-mcp-configuration" aria-label={t("mcpInspector.configuration")}>
    {!current ? <p role="status" className="arca-native-muted">{t("common.loading")}</p>
      : current.failed ? <div role="status"><p className="arca-native-muted">{t("mcpInspector.unavailable")}</p>
        <button type="button" className="btn-secondary" onClick={() => setAttempt(value => value + 1)}>{t("common.retry")}</button></div>
      : current.value?.documents.length ? current.value.documents.map((document, index) => {
        const formatted = document.configuration !== null ? JSON.stringify(document.configuration, null, 2) : null;
        return (
          <div key={`${document.source_path ?? "source"}:${index}`} className="arca-mcp-configuration-source">
            {(document.source_path || formatted !== null) && (
              <div className="arca-preview-card-bar">
                {document.source_path ? <code className="arca-mcp-configuration-path">{document.source_path}</code> : <span />}
                {formatted !== null && (
                  <button
                    type="button"
                    className="arca-preview-copy-btn"
                    onClick={() => void handleCopy(index, formatted)}
                  >
                    {copiedIndex === index ? <CheckIcon className="w-3 h-3" /> : <CopyIcon className="w-3 h-3" />}
                    <span>{copiedIndex === index ? t("notifications.copied") : "复制"}</span>
                  </button>
                )}
              </div>
            )}
            {formatted !== null ? (
              <pre tabIndex={0} aria-label={t("mcpInspector.parsedConfiguration")}>
                <code>{formatted}</code>
              </pre>
            ) : (
              <p role="status" className="arca-native-muted arca-mcp-configuration-empty">
                {document.unavailable_reason || t("mcpInspector.noDeclaration")}
              </p>
            )}
          </div>
        );
      }) : <p role="status" className="arca-native-muted">{t("mcpInspector.noDeclaration")}</p>}
  </section>;
}

export function McpConfigurationDialog({
  name,
  subtitle,
  leading,
  size = "md",
  request,
  onClose,
}: {
  name: string;
  subtitle?: ReactNode;
  leading?: ReactNode;
  size?: "sm" | "md" | "wide" | "lg" | "xl";
  request: McpInspectionRequest;
  onClose(): void;
}) {
  const { t } = useTranslation();
  return <DialogShell kind="review" size={size} className="arca-native-dialog" title={name} subtitle={subtitle}
    leading={leading ?? <ResourceIcon domain="mcp" className="w-6 h-6" />} onClose={onClose}
    footerEnd={<button type="button" className="btn-secondary" onClick={onClose}>{t("common.close")}</button>}>
    <McpConfigurationSection request={request} />
  </DialogShell>;
}
