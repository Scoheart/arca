import { ToastNotice } from "./Toast";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { McpIconPreference, RegistryEntry } from "../lib/types";
import { DialogShell } from "./DialogShell";
import { DownloadIcon, RefreshIcon } from "./icons";
import {
  inferMcpIcon,
  McpAvatar,
  McpIconGlyph,
  MCP_ICON_OPTIONS,
  type McpIconId,
} from "./McpIcon";

export function McpIconPickerDialog({
  assetKey,
  entry,
  preference,
  onSelectBuiltin,
  onUpload,
  onReset,
  onClose,
}: {
  assetKey: string;
  entry: RegistryEntry;
  preference?: McpIconPreference;
  onSelectBuiltin: (iconId: string) => Promise<unknown>;
  onUpload: () => Promise<boolean>;
  onReset: () => Promise<unknown>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const recommended = inferMcpIcon(entry);

  const run = async (operation: () => Promise<unknown>, close = true) => {
    setBusy(true);
    setError(null);
    try {
      const result = await operation();
      if (close && result !== false) onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const iconName = (id: McpIconId) => t(`mcpIcons.options.${id}`);
  const recommendation = recommended
    ? iconName(recommended)
    : t("mcpIcons.monogramFallback");

  return (
    <DialogShell
      kind="picker"
      size="md"
      title={t("mcpIcons.title")}
      subtitle={t("mcpIcons.subtitle")}
      busy={busy}
      onClose={onClose}
      status={error && <ToastNotice message={error} />}
      footerStart={preference && (
        <button type="button" className="btn-ghost" disabled={busy} onClick={() => void run(onReset)}>
          <RefreshIcon className="w-4 h-4" />
          {t("mcpIcons.restoreAutomatic")}
        </button>
      )}
      footerEnd={
        <>
          <button type="button" className="btn-secondary" disabled={busy} onClick={() => void run(onUpload)}>
            <DownloadIcon className="w-4 h-4" />
            {t("mcpIcons.upload")}
          </button>
          <button type="button" className="btn-primary" disabled={busy} onClick={onClose}>
            {t("common.close")}
          </button>
        </>
      }
    >
      <div className="arca-mcp-icon-picker">
        <section className="arca-mcp-icon-current">
          <McpAvatar assetKey={assetKey} entry={entry} preference={preference} size={52} />
          <div className="arca-mcp-icon-current-copy">
            <strong>{entry.name}</strong>
            <span>{t("mcpIcons.recommended", { name: recommendation })}</span>
          </div>
          {recommended && (
            <span className="arca-mcp-icon-recommendation" data-icon-tone={MCP_ICON_OPTIONS.find((item) => item.id === recommended)?.tone}>
              <McpIconGlyph id={recommended} />
            </span>
          )}
        </section>

        <section className="arca-mcp-icon-catalog" aria-labelledby="arca-mcp-icon-catalog-title">
          <div className="arca-mcp-icon-section-head">
            <h3 id="arca-mcp-icon-catalog-title">{t("mcpIcons.allIcons")}</h3>
            <span>{t("mcpIcons.iconCount", { count: MCP_ICON_OPTIONS.length })}</span>
          </div>
          <div className="arca-mcp-icon-grid">
            {MCP_ICON_OPTIONS.map((option) => {
              const selected = preference?.kind === "builtin" && preference.value === option.id;
              const name = iconName(option.id);
              return (
                <button
                  type="button"
                  key={option.id}
                  className="arca-mcp-icon-choice"
                  data-selected={selected ? "true" : undefined}
                  data-icon-tone={option.tone}
                  aria-label={t("mcpIcons.chooseBuiltin", { name })}
                  aria-pressed={selected}
                  disabled={busy}
                  onClick={() => void run(() => onSelectBuiltin(option.id))}
                >
                  <McpIconGlyph id={option.id} />
                  <span>{name}</span>
                </button>
              );
            })}
          </div>
        </section>

        <p className="arca-mcp-icon-upload-hint">{t("mcpIcons.uploadHint")}</p>
      </div>
    </DialogShell>
  );
}
