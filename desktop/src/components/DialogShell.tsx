import type { CSSProperties, ReactNode } from "react";
import { CheckIcon, EditIcon, SearchIcon, XIcon } from "./icons";
import { Modal } from "./ui";
import { useTranslation } from "react-i18next";

export type DialogShellKind = "editor" | "picker" | "review";
export type DialogShellSize = "sm" | "md" | "wide" | "lg";

const SIZE_WIDTH: Record<DialogShellSize, number> = {
  sm: 440,
  md: 560,
  wide: 640,
  lg: 760,
};

const DEFAULT_LEADING: Record<DialogShellKind, ReactNode> = {
  editor: <EditIcon className="w-4 h-4" />,
  picker: <SearchIcon className="w-4 h-4" />,
  review: <CheckIcon className="w-4 h-4" />,
};

export function DialogShell({
  kind,
  size = kind === "review" ? "sm" : kind === "picker" ? "md" : "wide",
  width,
  className,
  borderRadius,
  leading,
  title,
  subtitle,
  status,
  busy = false,
  closeLabel,
  onClose,
  children,
  footerStart,
  footerEnd,
}: {
  kind: DialogShellKind;
  size?: DialogShellSize;
  width?: CSSProperties["width"];
  className?: string;
  borderRadius?: CSSProperties["borderRadius"];
  leading?: ReactNode;
  title: string;
  subtitle?: ReactNode;
  status?: ReactNode;
  busy?: boolean;
  closeLabel?: string;
  onClose: () => void;
  children: ReactNode;
  footerStart?: ReactNode;
  footerEnd?: ReactNode;
}) {
  const { t } = useTranslation();
  const requestClose = () => {
    if (!busy) onClose();
  };
  const effectiveLeading = leading ?? (
    <span className="arca-dialog-shell-glyph" aria-hidden="true">
      {DEFAULT_LEADING[kind]}
    </span>
  );

  return (
    <Modal
      width={width ?? `min(${SIZE_WIDTH[size]}px, calc(100vw - 32px))`}
      maxHeight="calc(100dvh - 32px)"
      borderRadius={borderRadius}
      ariaLabel={title}
      busy={busy}
      layer={kind}
      onClose={requestClose}
    >
      <section
        className={["arca-dialog-shell", className].filter(Boolean).join(" ")}
        data-dialog-kind={kind}
        data-dialog-size={size}
        aria-busy={busy || undefined}
      >
        <header className="arca-dialog-shell-header">
          <div className="arca-dialog-shell-leading">{effectiveLeading}</div>
          <div className="arca-dialog-shell-heading">
            <h2 data-modal-title tabIndex={-1}>{title}</h2>
            {subtitle != null && <div className="arca-dialog-shell-subtitle">{subtitle}</div>}
          </div>
          <button
            type="button"
            className="arca-dialog-shell-close"
            onClick={requestClose}
            disabled={busy}
            aria-label={closeLabel ?? t("common.close")}
            title={closeLabel ?? t("common.close")}
          >
            <XIcon className="w-4 h-4" />
          </button>
        </header>
        {status != null && <div className="arca-dialog-shell-status">{status}</div>}
        <div className="arca-dialog-shell-body" inert={busy || undefined}>{children}</div>
        {(footerStart != null || footerEnd != null) && (
          <footer className="arca-dialog-shell-footer">
            <div className="arca-dialog-shell-footer-start">{footerStart}</div>
            <div className="arca-dialog-shell-footer-end">{footerEnd}</div>
          </footer>
        )}
      </section>
    </Modal>
  );
}
