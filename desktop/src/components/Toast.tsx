import { createContext, useContext, useState, useCallback, useEffect, useId, useMemo, useRef, type ReactNode } from "react";
import { CheckIcon, XIcon } from "./icons";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { copyToClipboard } from "../lib/api";
import { formatErrorDetails } from "../lib/format";

export type ToastKind = "success" | "error" | "warning" | "info";

interface ToastMessage {
  kind: ToastKind;
  msg: string;
  details?: unknown;
  action?: ReactNode;
  noticeKey?: string;
  persistent?: boolean;
}

interface ToastItem extends Omit<ToastMessage, "details"> {
  id: number;
  details?: string;
}

interface ToastContextValue {
  show: (toast: ToastMessage) => void;
  clear: (noticeKey: string) => void;
}

const ToastContext = createContext<ToastContextValue>({ show: () => {}, clear: () => {} });

export function useToast() {
  return useContext(ToastContext);
}

/** Report state-driven feedback through the same global notification surface. */
export function ToastNotice({ message, kind = "error", details, action, delay = 0, noticeKey, persistent }: {
  message?: string | null;
  kind?: ToastKind;
  details?: unknown;
  action?: ReactNode;
  delay?: number;
  noticeKey?: string;
  persistent?: boolean;
}) {
  const { show, clear } = useToast();
  const instanceId = useId();
  const key = noticeKey ?? `notice:${instanceId}`;
  const actionRef = useRef(action);
  actionRef.current = action;
  const diagnostic = details == null ? undefined : formatErrorDetails(details);
  useEffect(() => {
    if (!message) { clear(key); return; }
    const report = () => show({ kind, msg: message, details: diagnostic, action: actionRef.current, noticeKey: key, persistent });
    if (!delay) { report(); return; }
    const timer = setTimeout(report, delay);
    return () => clearTimeout(timer);
  }, [show, clear, kind, message, diagnostic, delay, key, persistent]);
  useEffect(() => () => clear(key), [clear, key]);
  return null;
}

export function ErrorDetails({ summary, text, onExpandedChange }: {
  summary: string;
  text: string;
  onExpandedChange?: (expanded: boolean) => void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [expanded, setExpanded] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"done" | "failed" | null>(null);

  const copy = async () => {
    try {
      await copyToClipboard([summary, text].join("\n\n"));
      setCopyStatus("done");
    } catch {
      setCopyStatus("failed");
    }
  };

  return <div className="arca-error-details">
    <button type="button" className="btn-ghost" aria-expanded={expanded} aria-controls={id} onClick={() => {
      setExpanded(!expanded);
      setCopyStatus(null);
      onExpandedChange?.(!expanded);
    }}>{t("notifications.details")}</button>
    <div id={id} hidden={!expanded}>
      {expanded && <>
        <pre tabIndex={0} aria-label={t("notifications.details")}>{text}</pre>
        <div className="arca-error-details-copy">
          <button type="button" className="btn-ghost" onClick={() => void copy()}>{t("notifications.copyDetails")}</button>
          {copyStatus && <span aria-live="polite">{t(copyStatus === "done" ? "notifications.copied" : "notifications.copyFailed")}</span>}
        </div>
      </>}
    </div>
  </div>;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [leavingId, setLeavingId] = useState<number | null>(null);
  const [paused, setPaused] = useState(false);
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const nextId = useRef(0);
  const leavingRef = useRef(leavingId);
  leavingRef.current = leavingId;
  const dismissed = useRef(new Map<string, Pick<ToastItem, "kind" | "msg" | "details">>());
  const active = toasts[0];
  const expanded = expandedId !== null && expandedId === active?.id;
  const dismiss = useCallback(() => {
    if (!active) return;
    if (active.noticeKey) {
      dismissed.current.set(active.noticeKey, { kind: active.kind, msg: active.msg, details: active.details });
    }
    leavingRef.current = active.id;
    setLeavingId(active.id);
  }, [active]);

  const show = useCallback(({ kind, msg, details, action, noticeKey, persistent }: ToastMessage) => {
    if (!msg.trim()) return;
    const summary = kind === "error" || kind === "warning" ? formatErrorDetails(msg) : msg;
    const diagnostic = details == null ? undefined : formatErrorDetails(details).trim() || undefined;
    const previous = noticeKey ? dismissed.current.get(noticeKey) : undefined;
    if (previous?.kind === kind && ((kind === "info" || kind === "success")
      || (previous.msg === summary && previous.details === diagnostic))) return;
    const id = ++nextId.current;
    const next = { id, kind, msg: summary, details: diagnostic, action, noticeKey, persistent };
    // Repeated failures should not turn into a long sequence of identical cards.
    setToasts((prev) => {
      const existing = prev.find((toast) => noticeKey ? toast.noticeKey === noticeKey
        : !toast.noticeKey && toast.kind === kind && toast.msg === summary && toast.details === diagnostic);
      if (existing) {
        if (!noticeKey) return prev;
        return prev.map((toast) => toast === existing
          ? { ...next, id: leavingRef.current === toast.id ? id : toast.id }
          : toast);
      }
      // A background progress notice must not hold later operation feedback.
      const priority = (kind === "error" || kind === "warning")
        && (prev[0]?.kind === "success" || prev[0]?.kind === "info");
      return prev[0]?.persistent || priority ? [next, ...prev] : [...prev, next];
    });
  }, []);
  const clear = useCallback((noticeKey: string) => {
    dismissed.current.delete(noticeKey);
    setToasts((prev) => prev.some((toast) => toast.noticeKey === noticeKey)
      ? prev.filter((toast) => toast.noticeKey !== noticeKey) : prev);
  }, []);
  const context = useMemo(() => ({ show, clear }), [show, clear]);

  useEffect(() => { setExpandedId(null); }, [active?.id]);

  useEffect(() => {
    if (!active || leavingId === active.id || paused || expanded || active.persistent) return;
    const duration = active.details ? 12000 : active.kind === "error" || active.kind === "warning" ? 9000 : 6000;
    const timer = setTimeout(dismiss, duration);
    return () => clearTimeout(timer);
  }, [active, leavingId, paused, expanded, dismiss]);

  useEffect(() => {
    if (leavingId === null) return;
    const timer = setTimeout(() => {
      setToasts((current) => current.filter((toast) => toast.id !== leavingId));
      setLeavingId(null);
      setPaused(false);
    }, 180);
    return () => clearTimeout(timer);
  }, [leavingId]);

  return (
    <ToastContext.Provider value={context}>
      {children}
      {active && createPortal(
        <div className="arca-toast-viewport" data-toast-viewport
          onPointerEnter={() => setPaused(true)}
          onPointerLeave={(event) => { if (!event.currentTarget.contains(document.activeElement)) setPaused(false); }}
          onFocusCapture={() => setPaused(true)}
          onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setPaused(false); }}>
          <div key={active.id} className="arca-toast" data-kind={active.kind} data-expanded={expanded || undefined}
            data-leaving={leavingId === active.id || undefined}
            role={active.kind === "error" || active.kind === "warning" ? "alert" : "status"} aria-atomic="true">
            <div className="arca-toast-icon" data-kind={active.kind}>
              {active.kind === "success" ? <CheckIcon className="w-3 h-3 text-white" />
                : active.kind === "error" ? <XIcon className="w-3 h-3 text-white" />
                : <span aria-hidden="true">{active.kind === "warning" ? "!" : "i"}</span>}
            </div>
            <div className="arca-toast-message"><span className="arca-toast-summary">{active.msg}</span>
              {active.details && <ErrorDetails summary={active.msg} text={active.details}
                onExpandedChange={(open) => setExpandedId(open ? active.id : null)} />}
              {active.action != null && <div className="arca-toast-actions">{active.action}</div>}
            </div>
            <button type="button" className="arca-icon-btn arca-toast-close"
              aria-label={t(active.kind === "error" || active.kind === "warning" ? "notifications.closeError" : "notifications.closeSuccess")}
              title={t("notifications.close")} onClick={dismiss}><XIcon className="w-3.5 h-3.5" /></button>
          </div>
        </div>, document.body,
      )}
    </ToastContext.Provider>
  );
}
