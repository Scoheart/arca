import { ToastNotice } from "./Toast";
import { useState, type ReactNode } from "react";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { TrashIcon } from "./icons";

export function ReviewDialog({
  title,
  subtitle,
  children,
  confirmLabel,
  tone = "danger",
  onConfirm,
  onClose,
}: {
  title: string;
  subtitle?: ReactNode;
  children: ReactNode;
  confirmLabel: string;
  tone?: "danger" | "primary";
  onConfirm: () => Promise<unknown> | unknown;
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
    } catch (reason) {
      setError(formatError(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <DialogShell
      kind="review"
      width="min(408px, calc(100vw - 32px))"
      className={`arca-review-dialog arca-review-dialog-${tone}`}
      leading={tone === "danger" ? (
        <span className="arca-review-dialog-glyph" aria-hidden="true">
          <TrashIcon className="w-4 h-4" />
        </span>
      ) : undefined}
      title={title}
      subtitle={subtitle}
      status={error ? <ToastNotice message={error} /> : undefined}
      busy={busy}
      onClose={onClose}
      footerEnd={
        <>
          <button type="button" className="btn-ghost" disabled={busy} onClick={onClose}>取消</button>
          <button
            type="button"
            className={tone === "danger" ? "btn-danger-solid" : "btn-primary"}
            disabled={busy}
            onClick={() => void confirm()}
          >
            {busy ? "处理中…" : confirmLabel}
          </button>
        </>
      }
    >
      <div className="arca-review-content">{children}</div>
    </DialogShell>
  );
}
