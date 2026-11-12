import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { inspectMcpConfiguration } from "../lib/api";
import type { McpConfigurationView, McpInspectionRequest } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { ResourceIcon } from "./resourcePresentation";
import "./NativePluginsPanel.css";

export function McpConfigurationSection({ request }: { request: McpInspectionRequest }) {
  const { t } = useTranslation();
  const requestKey = JSON.stringify(request);
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState<{ key: string; value?: McpConfigurationView; failed?: boolean } | null>(null);
  const pending = useRef<{ key: string; promise: Promise<McpConfigurationView> } | null>(null);
  const key = `${requestKey}:${attempt}`;
  useEffect(() => {
    let active = true;
    // StrictMode reuses this request; nothing is cached outside the open viewer.
    if (pending.current?.key !== key) pending.current = {
      key, promise: Promise.resolve().then(() => inspectMcpConfiguration(JSON.parse(requestKey) as McpInspectionRequest)),
    };
    void pending.current.promise.then(value => {
      if (active) setLoaded(value.agent_id === (JSON.parse(requestKey) as McpInspectionRequest).agent_id
        ? { key, value } : { key, failed: true });
    }, () => { if (active) setLoaded({ key, failed: true }); });
    return () => { active = false; };
  }, [key, requestKey]);
  const current = loaded?.key === key ? loaded : null;
  return <section className="arca-native-detail-section arca-mcp-configuration" aria-label={t("mcpInspector.configuration")}>
    <h3>{t("mcpInspector.configuration")}</h3>
    <p className="arca-native-muted">{t("mcpInspector.privacy")}</p>
    {!current ? <p role="status" className="arca-native-muted">{t("common.loading")}</p>
      : current.failed ? <div role="status"><p>{t("mcpInspector.unavailable")}</p>
        <button type="button" className="btn-ghost" onClick={() => setAttempt(value => value + 1)}>{t("common.retry")}</button></div>
      : current.value?.documents.length ? current.value.documents.map((document, index) => <div key={`${document.source_path}:${index}`} className="arca-mcp-configuration-source">
        {document.source_path && <code className="arca-mcp-configuration-path">{document.source_path}</code>}
        {document.configuration !== null ? <pre tabIndex={0} aria-label={t("mcpInspector.parsedConfiguration")}>
          <code>{JSON.stringify(document.configuration, null, 2)}</code>
        </pre> : <p role="status" className="arca-native-muted">{document.unavailable_reason || t("mcpInspector.noDeclaration")}</p>}
      </div>) : <p role="status" className="arca-native-muted">{t("mcpInspector.noDeclaration")}</p>}
  </section>;
}

export function McpConfigurationDialog({ name, request, onClose }: {
  name: string; request: McpInspectionRequest; onClose(): void;
}) {
  const { t } = useTranslation();
  return <DialogShell kind="review" size="wide" title={name} subtitle={t("mcpInspector.readOnly")}
    leading={<ResourceIcon domain="mcp" className="w-6 h-6" />} onClose={onClose}
    footerEnd={<button type="button" className="btn-secondary" onClick={onClose}>{t("common.close")}</button>}>
    <McpConfigurationSection request={request} />
  </DialogShell>;
}
