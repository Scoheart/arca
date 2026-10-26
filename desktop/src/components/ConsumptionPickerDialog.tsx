import { ToastNotice } from "./Toast";
import { ResourceIcon, type ResourceDomain } from "./resourcePresentation";
import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { RefreshIcon } from "./icons";
import { navigatePickerOptions, pickerOptionLabels, SearchBar } from "./ui";

export interface ConsumptionPickerOption {
  id: string;
  name: string;
  description?: string;
  icon?: ReactNode;
  meta?: ReactNode;
  disabled?: boolean;
  reason?: string;
}

export function ConsumptionPickerDialog({
  title,
  domain,
  subtitle,
  mode,
  options,
  actionLabel,
  busyLabel,
  emptyMessage = "没有可选资产",
  searchPlaceholder = "搜索资产",
  onSelect,
  onClose,
}: {
  title: string;
  domain?: ResourceDomain;
  subtitle: ReactNode;
  mode: "single" | "multiple";
  options: ConsumptionPickerOption[];
  actionLabel: string;
  busyLabel?: string;
  emptyMessage?: string;
  searchPlaceholder?: string;
  onSelect(ids: string[]): Promise<unknown> | unknown;
  onClose(): void;
}) {
  const pickerId = useId();
  const optionLabels = useMemo(() => pickerOptionLabels(options), [options]);
  const [query, setQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState(() => new Set<string>());
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const selected = useMemo(() => options.filter((option) => selectedIds.has(option.id) && !option.disabled)
    .map((option) => option.id).sort(), [options, selectedIds]);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    return options.filter((option) =>
      !needle || `${option.name} ${option.description ?? ""}`.toLocaleLowerCase().includes(needle),
    );
  }, [options, query]);

  const toggle = (id: string) => {
    setSelectedIds((current) => {
      if (mode === "single") return new Set([id]);
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const apply = async () => {
    if (selected.length === 0 || pending.current) return;
    pending.current = true;
    setError(null);
    setBusy(true);
    try {
      await onSelect(selected);
    } catch (cause) {
      setError(formatError(cause));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };

  return (
    <DialogShell
      leading={domain ? <span className="arca-dialog-shell-glyph"><ResourceIcon domain={domain} /></span> : undefined}
      kind="picker"
      title={title}
      subtitle={subtitle}
      busy={busy}
      status={<ToastNotice kind={busy ? "info" : "error"} persistent={busy}
        noticeKey={`consumption-picker:${pickerId}`} message={busy ? "正在检查兼容性并同步到 Agent…" : error} />}
      onClose={onClose}
      footerStart={
        <span className="arca-picker-count">
          {selected.length > 0 ? `已选 ${selected.length} · ` : ""}{filtered.length} 项
        </span>
      }
      footerEnd={
        <>
          <button type="button" className="btn-ghost" disabled={busy} onClick={onClose}>取消</button>
          <button type="button" className="btn-primary" disabled={selected.length === 0 || busy} onClick={() => void apply()}>
            {busy && <RefreshIcon className="arca-button-progress-icon" data-spinning="true" />}
            {busy
              ? (busyLabel ?? `${actionLabel}中…`)
              : mode === "multiple" && selectedIds.size > 1
                ? `${actionLabel}（${selectedIds.size}）`
                : actionLabel}
          </button>
        </>
      }
    >
      <div className="arca-picker-search">
        <SearchBar value={query} onChange={setQuery} placeholder={searchPlaceholder} autoFocus onKeyDown={navigatePickerOptions} />
      </div>
      <div className="arca-picker-list" onKeyDown={navigatePickerOptions} role={mode === "single" ? "listbox" : "group"} aria-label={title}>
        {filtered.length === 0 ? (
          <div className="arca-picker-empty">{emptyMessage}</div>
        ) : filtered.map((option) => (
          <button
            key={option.id}
            data-picker-option
            aria-label={optionLabels.get(option.id)}
            aria-describedby={option.description || option.reason ? `${pickerId}-${encodeURIComponent(option.id)}` : undefined}
            type="button"
            role={mode === "single" ? "option" : undefined}
            className="arca-picker-option arca-consumption-picker-option"
            data-selected={selectedIds.has(option.id) ? "true" : undefined}
            disabled={option.disabled || busy}
            aria-selected={mode === "single" ? selectedIds.has(option.id) : undefined}
            aria-pressed={mode === "multiple" ? selectedIds.has(option.id) : undefined}
            onClick={() => toggle(option.id)}
          >
            <span className="arca-consumption-picker-check" aria-hidden="true">
              {selectedIds.has(option.id) ? "✓" : ""}
            </span>
            {option.icon && <span className="arca-consumption-picker-icon">{option.icon}</span>}
            <span className="arca-picker-option-copy">
              <strong>{option.name}</strong>
              <span id={`${pickerId}-${encodeURIComponent(option.id)}`}><small>{option.description}</small>
              {option.reason && <em>{option.reason}</em>}</span>
            </span>
            {option.meta && <span className="arca-picker-option-meta">{option.meta}</span>}
          </button>
        ))}
      </div>
    </DialogShell>
  );
}
