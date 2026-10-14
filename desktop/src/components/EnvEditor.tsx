import { ToastNotice } from "./Toast";
import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { EyeIcon, EyeOffIcon, PlusIcon, TrashIcon } from "./icons";

let _uid = 0;
const nextId = () => ++_uid;

interface Row { id: number; k: string; v: string; revealed?: boolean; }

interface EnvEditorProps {
  value: Record<string, string>;
  onChange: (env: Record<string, string>) => void;
  onValidityChange?: (valid: boolean) => void;
  kind?: "env" | "headers";
  disabled?: boolean;
}

function rowsToEnv(rows: Row[]): Record<string, string> {
  return Object.fromEntries(rows.filter((row) => row.k.trim()).map((row) => [row.k.trim(), row.v]));
}

function envToRows(env: Record<string, string>): Row[] {
  const rows = Object.entries(env).map(([k, v]) => ({ id: nextId(), k, v }));
  return rows.length > 0 ? rows : [{ id: nextId(), k: "", v: "" }];
}

export function EnvEditor({ value, onChange, onValidityChange, kind = "env", disabled = false }: EnvEditorProps) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<Row[]>(() => envToRows(value));
  const normalizedKey = (key: string) => kind === "headers" ? key.trim().toLowerCase() : key.trim();
  const errors = rows.map((row) => {
    const key = normalizedKey(row.k);
    if (!key) return row.v ? t("mcpEditor.missingKey") : "";
    return rows.some((other) => other.id !== row.id && normalizedKey(other.k) === key)
      ? t("mcpEditor.duplicateKey") : "";
  });
  const valid = errors.every((error) => !error);
  useEffect(() => { onValidityChange?.(valid); }, [valid, onValidityChange]);

  // Resync only when the external value genuinely diverges from what we already
  // project (an external reset) — never clobber in-progress edits or churn on a
  // fresh `{}` reference from the parent.
  useEffect(() => {
    if (JSON.stringify(rowsToEnv(rows)) !== JSON.stringify(value)) {
      setRows(envToRows(value));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);

  const updateRows = (next: Row[]) => {
    const clean = next.length > 0 ? next : [{ id: nextId(), k: "", v: "" }];
    setRows(clean);
    onChange(rowsToEnv(clean));
  };

  const setKey = (id: number, k: string) =>
    updateRows(rows.map((r) => (r.id === id ? { ...r, k } : r)));

  const setVal = (id: number, v: string) =>
    updateRows(rows.map((r) => (r.id === id ? { ...r, v } : r)));

  const removeRow = (id: number) =>
    updateRows(rows.filter((r) => r.id !== id));

  const addRow = () => updateRows([...rows, { id: nextId(), k: "", v: "" }]);

  const label = t(kind === "headers" ? "mcpEditor.headers" : "mcpEditor.environment");
  return (
    <div className="arca-key-value-editor">
      {rows.map((row, index) => (
        <div key={row.id} className="arca-key-value-item">
        <div className="arca-key-value-row">
          <input
            className="arca-dialog-input arca-dialog-input-mono"
            placeholder={t(kind === "headers" ? "mcpEditor.header" : "mcpEditor.variable")}
            aria-label={t("mcpEditor.keyLabel", { label, row: index + 1 })}
            aria-invalid={!!errors[index] || undefined}
            value={row.k}
            onChange={(e) => setKey(row.id, e.target.value)}
            disabled={disabled}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
          />
          <div className="arca-key-value-value">
          <input
            className="arca-dialog-input arca-dialog-input-mono"
            type={row.revealed ? "text" : "password"}
            placeholder={t("mcpEditor.value")}
            aria-label={t("mcpEditor.valueLabel", { label, row: index + 1 })}
            value={row.v}
            onChange={(e) => setVal(row.id, e.target.value)}
            disabled={disabled}
            spellCheck={false}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
          />
          <button type="button" className="arca-key-value-reveal" disabled={disabled}
            aria-label={t(row.revealed ? "mcpEditor.hideValue" : "mcpEditor.showValue", { row: index + 1 })}
            title={t(row.revealed ? "mcpEditor.hideValue" : "mcpEditor.showValue", { row: index + 1 })}
            aria-pressed={!!row.revealed}
            onClick={() => setRows(rows.map((item) => item.id === row.id ? { ...item, revealed: !item.revealed } : item))}>
            {row.revealed ? <EyeOffIcon className="w-3.5 h-3.5" /> : <EyeIcon className="w-3.5 h-3.5" />}
          </button>
          </div>
          <button
            type="button"
            onClick={() => removeRow(row.id)}
            className="arca-key-value-delete"
            title={t("mcpEditor.deleteRow", { row: index + 1 })}
            aria-label={t("mcpEditor.deleteRow", { row: index + 1 })}
            disabled={disabled}
          >
            <TrashIcon className="w-3.5 h-3.5" />
          </button>
        </div>
        {errors[index] && <ToastNotice kind="warning" delay={500} message={errors[index]} />}
        </div>
      ))}
      <button
        type="button"
        onClick={addRow}
        className="arca-key-value-add"
        disabled={disabled}
      >
        <PlusIcon className="w-3.5 h-3.5" />
        <span>{t(kind === "headers" ? "mcpEditor.addHeader" : "mcpEditor.addVariable")}</span>
      </button>
    </div>
  );
}
