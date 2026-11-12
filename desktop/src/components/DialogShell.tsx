import type { ReactNode } from "react";
import { CheckIcon, DocumentIcon, EditIcon, SearchIcon, XIcon } from "./icons";
import { Modal } from "./ui";
import { useTranslation } from "react-i18next";

export type DialogShellKind = "editor" | "picker" | "review" | "detail";
export type DialogShellSize = "sm" | "md" | "wide" | "lg" | "xl";

const SIZE_WIDTH: Record<DialogShellSize, number> = {
  sm: 440,
  md: 560,
  wide: 640,
  lg: 760,
  xl: 860,
};

const DEFAULT_LEADING: Record<DialogShellKind, ReactNode> = {
  editor: <EditIcon className="w-4 h-4" />,
  picker: <SearchIcon className="w-4 h-4" />,
  review: <CheckIcon className="w-4 h-4" />,
  detail: <DocumentIcon className="w-4 h-4" />,
};

type DialogFrameProps = {
  kind: DialogShellKind;
  size?: DialogShellSize;
  className?: string;
  leading?: ReactNode;
  title: string;
  subtitle?: ReactNode;
  status?: ReactNode;
  busy?: boolean;
  closeLabel?: string;
  label?: string;
  onClose: () => void;
  children: ReactNode;
  footerStart?: ReactNode;
  footerEnd?: ReactNode;
};

/** The shared frame also fits an inspector already hosted by a Modal. */
export function DialogFrame({
  kind,
  size,
  className,
  leading,
  title,
  subtitle,
  status,
  busy = false,
  closeLabel,
  label,
  onClose,
  children,
  footerStart,
  footerEnd,
}: DialogFrameProps) {
  const { t } = useTranslation();
  const closeName = closeLabel ?? t("common.close");
  const detail = kind === "detail";
  const Frame = detail ? "aside" : "section";
  const effectiveLeading = leading ?? (
    <span className="arca-dialog-shell-glyph" aria-hidden="true">
      {DEFAULT_LEADING[kind]}
    </span>
  );

  return (
    <Frame
      className={["arca-dialog-shell", className].filter(Boolean).join(" ")}
      data-dialog-kind={kind}
      data-dialog-size={size}
      data-modal-initial-focus={detail || undefined}
      tabIndex={detail ? -1 : undefined}
      aria-label={label}
      aria-busy={busy || undefined}
    >
      <header className="arca-dialog-shell-header">
        <div className="arca-dialog-shell-leading">{effectiveLeading}</div>
        <div className="arca-dialog-shell-heading">
          <h2 data-modal-title tabIndex={-1} title={title}>{title}</h2>
          {subtitle != null && <div className="arca-dialog-shell-subtitle">{subtitle}</div>}
        </div>
        <button
          type="button"
          className="arca-dialog-shell-close"
          onClick={onClose}
          disabled={busy}
          aria-label={closeName}
          title={closeName}
        >
          <XIcon className="w-4 h-4" />
        </button>
      </header>
      {status != null && <div className="arca-dialog-shell-status">{status}</div>}
      <div className="arca-dialog-shell-body" inert={busy || undefined}>{children}</div>
      {(footerStart != null || footerEnd != null) && (
        <footer className="arca-dialog-shell-footer">
          {footerStart != null && <div className="arca-dialog-shell-footer-start">{footerStart}</div>}
          {footerEnd != null && <div className="arca-dialog-shell-footer-end">{footerEnd}</div>}
        </footer>
      )}
    </Frame>
  );
}

export function DialogShell({
  size,
  layer,
  ariaLabel,
  ...frame
}: DialogFrameProps & { layer?: string; ariaLabel?: string }) {
  const resolvedSize = size ?? (frame.kind === "review" ? "sm" : frame.kind === "editor" ? "wide" : "md");
  return (
    <Modal
      width={SIZE_WIDTH[resolvedSize]}
      ariaLabel={ariaLabel ?? frame.title}
      busy={frame.busy}
      layer={layer ?? frame.kind}
      onClose={frame.onClose}
    >
      <DialogFrame {...frame} size={resolvedSize} />
    </Modal>
  );
}
