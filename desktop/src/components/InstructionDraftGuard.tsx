import { useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useTranslation } from 'react-i18next';
import { instructionDrafts } from '../lib/instructionDrafts';
import { formatError } from '../lib/format';
import { DialogShell } from './DialogShell';
import { useToast } from './Toast';

/** Keep unsaved native-file drafts protected even after leaving the Agent tab. */
export function InstructionDraftGuard() {
  const { t } = useTranslation(); const toast = useToast();
  const [closing, setClosing] = useState(false), [busy, setBusy] = useState(false);
  const approved = useRef(false);
  useEffect(() => {
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (instructionDrafts.size && !approved.current) { event.preventDefault(); event.returnValue = ''; }
    };
    window.addEventListener('beforeunload', beforeUnload);
    let disposed = false, unlisten: (() => void) | undefined;
    try {
      void getCurrentWindow().onCloseRequested(event => {
        if (instructionDrafts.size && !approved.current) { event.preventDefault(); setClosing(true); }
      }).then(stop => { if (disposed) stop(); else unlisten = stop; }).catch(() => undefined);
    } catch { /* Browser-only component previews have no native window. */ }
    return () => { disposed = true; unlisten?.(); window.removeEventListener('beforeunload', beforeUnload); };
  }, []);
  if (!closing) return null;
  return <DialogShell kind="review" title={t('instructions.closeTitle')} busy={busy} onClose={() => setClosing(false)} footerEnd={<>
    <button type="button" className="btn-secondary" disabled={busy} onClick={() => setClosing(false)}>{t('instructions.keep')}</button>
    <button type="button" className="btn-primary" disabled={busy} onClick={() => {
      approved.current = true; setBusy(true);
      void getCurrentWindow().close().catch(error => { approved.current = false; setBusy(false); toast.show({ kind: 'error', msg: formatError(error) }); });
    }}>{t('instructions.closeDiscard')}</button>
  </>}><p>{t('instructions.closeBody', { count: instructionDrafts.size })}</p></DialogShell>;
}
