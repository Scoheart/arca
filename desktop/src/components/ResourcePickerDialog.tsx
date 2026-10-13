import { useId, useMemo, useRef, useState, type ReactNode } from "react";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { RefreshIcon } from "./icons";
import { navigatePickerOptions, pickerOptionLabels, SearchBar } from "./ui";

export interface ResourcePickerOption {
  id: string;
  name: string;
  description?: string;
  meta?: ReactNode;
  avatar?: ReactNode;
  disabled?: boolean;
}

export function ResourcePickerDialog({
  title,
  subtitle,
  options,
  addLabel = "添加",
  onAdd,
  onClose,
}: {
  title: string;
  subtitle?: ReactNode;
  options: ResourcePickerOption[];
  addLabel?: string;
  onAdd: (option: ResourcePickerOption) => Promise<unknown> | unknown;
  onClose: () => void;
}) {
  const pickerId = useId();
  const optionLabels = useMemo(() => pickerOptionLabels(options), [options]);
  const [query, setQuery] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [error, setError] = useState<string | null>(null);
  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return options;
    return options.filter((option) =>
      `${option.name} ${option.description ?? ""}`.toLocaleLowerCase().includes(needle),
    );
  }, [options, query]);
  const selected = options.find((option) => option.id === selectedId) ?? null;

  const add = async () => {
    if (!selected || selected.disabled || pending.current) return;
    pending.current = true;
    setError(null);
    setBusy(true);
    try {
      await onAdd(selected);
    } catch (cause) {
      setError(formatError(cause));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };

  return (
    <DialogShell
      kind="picker"
      title={title}
      subtitle={subtitle}
      busy={busy}
      status={busy ? (
        <span className="arca-operation-progress" role="status">
          <RefreshIcon data-spinning="true" />
          正在检查并添加…
        </span>
      ) : error ? <span role="alert">{error}</span> : undefined}
      onClose={onClose}
      footerStart={<span className="arca-picker-count">{filtered.length} 个可选项</span>}
      footerEnd={
        <>
          <button type="button" className="btn-ghost" disabled={busy} onClick={onClose}>取消</button>
          <button type="button" className="btn-primary" disabled={!selected || selected.disabled || busy} onClick={() => void add()}>
            {busy && <RefreshIcon className="arca-button-progress-icon" data-spinning="true" />}
            {busy ? "添加中…" : addLabel}
          </button>
        </>
      }
    >
      <div className="arca-picker-search">
        <SearchBar value={query} onChange={setQuery} placeholder="搜索资源" autoFocus onKeyDown={navigatePickerOptions} />
      </div>
      <div className="arca-picker-list" onKeyDown={navigatePickerOptions} role="listbox" aria-label={title}>
        {filtered.length === 0 ? (
          <div className="arca-picker-empty">没有匹配项</div>
        ) : filtered.map((option) => (
          <button
            key={option.id}
            data-picker-option
            aria-label={optionLabels.get(option.id)}
            aria-describedby={option.description ? `${pickerId}-${encodeURIComponent(option.id)}` : undefined}
            type="button"
            role="option"
            aria-selected={selectedId === option.id}
            className="arca-picker-option"
            data-selected={selectedId === option.id ? "true" : undefined}
            disabled={option.disabled}
            onClick={() => setSelectedId(option.id)}
          >
            {option.avatar}
            <span className="arca-picker-option-copy">
              <strong>{option.name}</strong>
              {option.description && <small id={`${pickerId}-${encodeURIComponent(option.id)}`}>{option.description}</small>}
            </span>
            {option.meta && <span className="arca-picker-option-meta">{option.meta}</span>}
          </button>
        ))}
      </div>
    </DialogShell>
  );
}
