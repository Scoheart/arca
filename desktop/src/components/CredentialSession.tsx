import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CREDENTIAL_SESSION_CHANGED, CREDENTIAL_SESSION_REQUIRED, getModelCredentialSessionStatus, unlockModelCredentials } from "../lib/api";
import { invalidateModelObservation } from "../lib/modelObservation";
import { ToastNotice } from "./Toast";

/** Startup unlock lives at application scope, never in individual Provider views. */
export function CredentialSession() {
  const { t } = useTranslation();
  const [locked, setLocked] = useState(false);
  const [deferred, setDeferred] = useState(false);
  const [busy, setBusy] = useState(false);
  const [noticeVersion, setNoticeVersion] = useState(0);
  const pending = useRef(false);
  const mounted = useRef(true);
  const requireUnlock = useCallback(() => {
    setLocked(true);
    // Dismissing a notification must not permanently hide its recovery action.
    setNoticeVersion((version) => version + 1);
  }, []);
  const unlock = useCallback(async (retry: boolean) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      const status = await unlockModelCredentials(retry);
      if (!mounted.current) return;
      setLocked(!status.unlocked);
      setDeferred(status.sync_deferred);
      // Retry requests that raced startup or previously failed authorization.
      if (status.unlocked) invalidateModelObservation();
    } catch {
      if (mounted.current) requireUnlock();
    } finally {
      pending.current = false;
      if (mounted.current) setBusy(false);
    }
  }, [requireUnlock]);
  useEffect(() => {
    mounted.current = true;
    const refreshStatus = () => {
      void getModelCredentialSessionStatus().then((status) => {
        if (mounted.current) setDeferred(status.sync_deferred);
      }).catch(() => undefined);
    };
    window.addEventListener(CREDENTIAL_SESSION_REQUIRED, requireUnlock);
    window.addEventListener(CREDENTIAL_SESSION_CHANGED, refreshStatus);
    void unlock(false);
    return () => {
      mounted.current = false;
      window.removeEventListener(CREDENTIAL_SESSION_REQUIRED, requireUnlock);
      window.removeEventListener(CREDENTIAL_SESSION_CHANGED, refreshStatus);
    };
  }, [unlock, requireUnlock]);
  return <>
    <ToastNotice kind="warning" noticeKey={`credential-session:${noticeVersion}`} persistent
      message={locked ? t("credentialSession.locked") : null}
      action={<button type="button" className="btn-ghost" disabled={busy}
        onClick={() => void unlock(true)}>{t(busy ? "credentialSession.unlocking" : "credentialSession.retry")}</button>} />
    <ToastNotice kind="warning" noticeKey="credential-session-sync"
      message={deferred ? t("credentialSession.deferred") : null} />
  </>;
}
