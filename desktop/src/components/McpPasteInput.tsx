import { ToastNotice } from "./Toast";
import { useTranslation } from "react-i18next";
import type { PastedMcpSummary } from "../lib/api";

const EXAMPLE = `{
  "mcpServers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@playwright/mcp@latest"]
    }
  }
}`;
const HTTP_EXAMPLE = `{
  "mcpServers": {
    "my-server": {
      "type": "http",
      "url": "https://example.com/mcp"
    }
  }
}`;

export function McpPasteInput({ value, onChange, disabled, preview, error, pending, existingKeys }: {
  value: string;
  onChange(value: string): void;
  disabled: boolean;
  preview: PastedMcpSummary[] | null;
  error: string;
  pending: boolean;
  existingKeys: Set<string>;
}) {
  const { t } = useTranslation();
  return <div className="arca-mcp-paste-fields">
    <p>{t("mcpEditor.pasteHint")}</p>
    <div className="arca-mcp-code-editor">
    <div className="arca-mcp-code-toolbar">
      <span>JSON <i>·</i> TOML <i>·</i> YAML</span>
      <div>
        <button type="button" disabled={disabled || !!value.trim()} title={value.trim() ? t("mcpEditor.exampleHint") : undefined} onClick={() => onChange(EXAMPLE)}>{t("mcpEditor.localExample")}</button>
        <button type="button" disabled={disabled || !!value.trim()} title={value.trim() ? t("mcpEditor.exampleHint") : undefined} onClick={() => onChange(HTTP_EXAMPLE)}>{t("mcpEditor.remoteExample")}</button>
      </div>
    </div>
    <textarea
      autoFocus
      data-modal-initial-focus
      aria-label={t("mcpEditor.pasteLabel")}
      aria-invalid={!!error || undefined}
      className="arca-paste-config-input"
      placeholder={EXAMPLE}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      disabled={disabled}
      spellCheck={false}
      autoCapitalize="off"
      autoCorrect="off"
    />
    </div>
    {value.trim() && <div className="arca-mcp-paste-result" aria-live="polite">
      {pending ? <p>{t("mcpEditor.detecting")}</p> : error ? <ToastNotice kind="warning" delay={500} message={error} /> : preview && <>
        <strong>{t("mcpEditor.detected", { count: preview.length })}</strong>
        <ul aria-label={t("mcpEditor.previewLabel")}>
          {preview.map((entry) => {
            const key = `${entry.name}::${entry.transport}`;
            const updatesExisting = existingKeys.has(key);
            return <li key={key}>
              <span className="arca-mcp-import-name">{entry.name}</span>
              <code>{entry.transport}</code>
              <span className="arca-mcp-import-action" data-update={updatesExisting || undefined}>{t(updatesExisting ? "mcpEditor.updateEntry" : "mcpEditor.newEntry")}</span>
            </li>;
          })}
        </ul>
      </>}
    </div>}
  </div>;
}
