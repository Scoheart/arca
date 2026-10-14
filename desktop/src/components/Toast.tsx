import { createContext, useContext, useState, useCallback, useEffect, useId, useMemo, useRef, type ReactNode } from "react";
import { CheckIcon, XIcon } from "./icons";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

export type ToastKind = "success" | "error" | "warning" | "info";

interface ToastMessage {
  kind: ToastKind;
  msg: string;
  action?: ReactNode;
  noticeKey?: string;
  persistent?: boolean;
}

interface ToastItem extends ToastMessage {
  id: number;
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
export function ToastNotice({ message, kind = "error", action, delay = 0, noticeKey, persistent }: {
  message?: string | null;
  kind?: ToastKind;
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
  useEffect(() => {
    if (!message) { clear(key); return; }
    const report = () => show({ kind, msg: message, action: actionRef.current, noticeKey: key, persistent });
    if (!delay) { report(); return; }
    const timer = setTimeout(report, delay);
    return () => clearTimeout(timer);
  }, [show, clear, kind, message, delay, key, persistent]);
  useEffect(() => () => clear(key), [clear, key]);
  return null;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const [leavingId, setLeavingId] = useState<number | null>(null);
  const [paused, setPaused] = useState(false);
  const nextId = useRef(0);
  const leavingRef = useRef(leavingId);
  leavingRef.current = leavingId;
  const dismissed = useRef(new Set<string>());
  const active = toasts[0];
  const dismiss = useCallback(() => {
    if (!active) return;
    if (active.noticeKey) dismissed.current.add(active.noticeKey);
    setLeavingId(active.id);
  }, [active]);

  const show = useCallback(({ kind, msg, action, noticeKey, persistent }: ToastMessage) => {
    if (!msg.trim()) return;
    if (noticeKey && dismissed.current.has(noticeKey)) return;
    const id = ++nextId.current;
    // Repeated failures should not turn into a long sequence of identical cards.
    setToasts((prev) => {
      const existing = prev.find((toast) => (noticeKey && toast.noticeKey === noticeKey) || toast.kind === kind && toast.msg === msg);
      if (existing) {
        if (!noticeKey) return prev;
        return prev.map((toast) => toast === existing
          ? { id: leavingRef.current === toast.id ? id : toast.id, kind, msg, action, noticeKey, persistent }
          : toast);
      }
      const next = { id, kind, msg, action, noticeKey, persistent };
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

  useEffect(() => {
    if (!active || leavingId === active.id || paused || active.persistent) return;
    const timer = setTimeout(dismiss, active.kind === "error" || active.kind === "warning" ? 9000 : 6000);
    return () => clearTimeout(timer);
  }, [active?.id, active?.kind, active?.persistent, leavingId, paused, dismiss]);

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
          <div key={active.id} className="arca-toast" data-kind={active.kind} data-leaving={leavingId === active.id || undefined}
            role={active.kind === "error" || active.kind === "warning" ? "alert" : "status"} aria-atomic="true">
            <div className="arca-toast-icon" data-kind={active.kind}>
              {active.kind === "success" ? <CheckIcon className="w-3 h-3 text-white" />
                : active.kind === "error" ? <XIcon className="w-3 h-3 text-white" />
                : <span aria-hidden="true">{active.kind === "warning" ? "!" : "i"}</span>}
            </div>
            <div className="arca-toast-message"><span>{active.msg}</span>
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
