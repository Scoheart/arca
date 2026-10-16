import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { openPath, openUrl } from '@tauri-apps/plugin-opener';
import { instructionCommit, instructionList, instructionPrepareCreate, instructionPrepareSave, instructionRead, type InstructionInventory, type InstructionSnapshot } from '../lib/instructions';
import { formatError } from '../lib/format';
import { ToastNotice, useToast } from './Toast';
import { DialogShell } from './DialogShell';
import { TraceMarkdown } from './TraceMarkdown';
import { preferredFileEditor } from './FileEditorSelect';
import { DocumentIcon, ExternalLinkIcon, RefreshIcon } from './icons';
import './TraceView.css';
import './InstructionsEditor.css';

import { instructionDrafts as drafts } from '../lib/instructionDrafts';
const equalText = (a: string, b: string) => a.replace(/\r\n/g, '\n') === b.replace(/\r\n/g, '\n');

export function InstructionsEditor({ agentId }: { agentId: string }) {
  const { t } = useTranslation();
  const toast = useToast();
  const [inventory, setInventory] = useState<InstructionInventory | null>(null);
  const [snapshot, setSnapshot] = useState<InstructionSnapshot | null>(null);
  const [text, setText] = useState('');
  const [creating, setCreating] = useState(false);
  const [preview, setPreview] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [discardAction, setDiscardAction] = useState<(() => void) | null>(null);
  const generation = useRef(0), lock = useRef(false), selected = useRef<string | null>(null);
  const dirty = creating ? Boolean(text) : snapshot != null && !equalText(snapshot.text, text);
  const live = useRef({ inventory, snapshot, text, creating, dirty }); live.current = { inventory, snapshot, text, creating, dirty };

  const remember = useCallback((value: string, doc = live.current.snapshot, isCreating = live.current.creating) => {
    const current = live.current;
    if (current.inventory && (isCreating || doc && !equalText(doc.text, value))) {
      if (!drafts.has(agentId) && drafts.size >= 16) { toast.show({ kind: 'warning', msg: t('instructions.draftsFull') }); return false; }
      drafts.set(agentId, { snapshot: doc, text: value, creating: isCreating, inventory: current.inventory });
    } else drafts.delete(agentId);
    return true;
  }, [agentId, toast, t]);

  const open = useCallback(async (fileId: string) => {
    const epoch = ++generation.current;
    lock.current = true; setBusy(true); setError(null); selected.current = fileId; setPreview(false); setSnapshot(null); setText('');
    try {
      const value = await instructionRead(agentId, fileId);
      if (generation.current === epoch) { setSnapshot(value); setText(value.text); setCreating(false); }
    } catch (e) { if (generation.current === epoch) setError(formatError(e)); }
    finally { if (generation.current === epoch) { lock.current = false; setBusy(false); } }
  }, [agentId]);

  const refresh = useCallback(async (restoreDraft = false) => {
    const epoch = ++generation.current;
    lock.current = true; setBusy(true); setError(null);
    try {
      const value = await instructionList(agentId);
      if (generation.current !== epoch) return;
      setInventory(value);
      const draft = restoreDraft ? drafts.get(agentId) : null;
      if (draft) {
        setSnapshot(draft.snapshot); setText(draft.text); setCreating(draft.creating);
        selected.current = draft.snapshot?.file.id ?? null;
        // Preserve the creation revision used by an older draft. Saving must
        // reject newly appeared native files instead of treating them as absent.
        if (draft.creating) setInventory(draft.inventory);
        toast.show({ kind: 'info', msg: t('instructions.restored') });
      } else {
        drafts.delete(agentId);
        setSnapshot(null); setText(''); setCreating(false);
        const file = value.files.find(file => file.id === selected.current) ?? value.files.find(file => file.loading === 'preferred') ?? value.files[0];
        if (file) await open(file.id);
        else selected.current = null;
      }
    } catch (e) { if (generation.current === epoch) setError(formatError(e)); }
    finally { if (generation.current === epoch) { lock.current = false; setBusy(false); } }
  }, [agentId, open, toast, t]);
  const refreshRef = useRef(refresh); refreshRef.current = refresh;
  useEffect(() => { void refreshRef.current(true); return () => { ++generation.current; }; }, [agentId]);
  useEffect(() => {
    const onFocus = () => {
      const current = live.current;
      if (lock.current || !current.snapshot || current.creating) return;
      const epoch = generation.current;
      void instructionRead(agentId, current.snapshot.file.id).then(value => {
        if (generation.current !== epoch || value.revision === live.current.snapshot?.revision) return;
        if (live.current.dirty) toast.show({ kind: 'warning', msg: t('instructions.fileChanged') });
        else { setSnapshot(value); setText(value.text); }
      }).catch(e => { if (generation.current === epoch) toast.show({ kind: 'error', msg: formatError(e) }); });
    };
    window.addEventListener('focus', onFocus); return () => window.removeEventListener('focus', onFocus);
  }, [agentId, toast, t]);

  const guard = (action: () => void) => {
    if (lock.current) return;
    if (dirty || creating) setDiscardAction(() => action); else action();
  };
  const save = async () => {
    if (lock.current || !inventory || !creating && !snapshot) return;
    const epoch = generation.current, submitted = text;
    lock.current = true; setBusy(true); setError(null);
    try {
      const plan = creating ? await instructionPrepareCreate(agentId, inventory.revision, text)
        : await instructionPrepareSave(agentId, snapshot!.file.id, snapshot!.revision, text);
      const saved = await instructionCommit(plan);
      const cached = drafts.get(agentId);
      if (cached?.text === submitted) drafts.delete(agentId);
      if (generation.current !== epoch) return;
      setSnapshot(saved); setText(saved.text); selected.current = saved.file.id; setCreating(false);
      toast.show({ kind: 'success', msg: `${t(plan.creating ? 'instructions.created' : 'instructions.saved')} ${t(`instructions.reload.${saved.reload}`)}` });
      // A failed follow-up listing must not turn a successful disk save into
      // an apparent failure or retain a stale creation inventory.
      try { const value = await instructionList(agentId); if (generation.current === epoch) setInventory(value); }
      catch (e) { if (generation.current === epoch) toast.show({ kind: 'warning', msg: formatError(e) }); }
    } catch (e) { if (generation.current === epoch) setError(formatError(e)); }
    finally { if (generation.current === epoch) { lock.current = false; setBusy(false); } }
  };
  const path = creating ? inventory?.create?.path : snapshot?.file.path ?? inventory?.files.find(file => file.id === selected.current)?.path;
  const file = snapshot?.file;
  return <section className="arca-instructions" aria-label={t('instructions.title')}>
    <header className="arca-instructions-heading"><div><h3>{t('instructions.title')}</h3><p>{t('instructions.subtitle')}</p></div>
      <button type="button" className="btn-secondary" disabled={busy} onClick={() => guard(() => void refresh())}><RefreshIcon className="w-3.5 h-3.5" data-spinning={busy || undefined} />{t('instructions.refresh')}</button></header>
    <ToastNotice message={error} action={<button type="button" className="btn-ghost" onClick={() => guard(() => void refresh())}>{t('instructions.refresh')}</button>} />
    <ToastNotice kind="warning" noticeKey={`instructions-scan:${agentId}`} message={inventory && !inventory.complete ? t('instructions.incomplete') : null} />
    <div className="arca-instructions-workspace" data-single={(inventory?.files.length ?? 0) <= 1}>
      {(inventory?.files.length ?? 0) > 1 && <nav className="arca-instructions-files" aria-label={t('instructions.source')}>
        {inventory?.files.map(file => <button type="button" key={file.id} aria-current={file.id === selected.current ? 'true' : undefined} disabled={busy} title={file.path}
          onClick={() => file.id !== selected.current && guard(() => { drafts.delete(agentId); void open(file.id); })}>
          <DocumentIcon className="w-3.5 h-3.5" /><span><strong>{file.name}</strong><small>{file.path}</small></span>
        </button>)}
      </nav>}
      <div className="arca-instructions-document">
        {busy && !snapshot && !creating ? <div className="arca-instructions-empty">{t('instructions.loading')}</div>
          : snapshot || creating ? <>
            <div className="arca-instructions-file-heading"><div><strong>{creating ? path?.split('/').pop() : file?.name}</strong><code title={path}>{path}</code></div>
              <div className="arca-instructions-file-actions">{file && file.readers.length > 1 && <small title={file.readers.map(reader => reader.name).join(', ')}>{t('instructions.shared')}</small>}{dirty && <small>{t('instructions.dirty')}</small>}{file && !file.editable && <small>{t('instructions.readOnly')}</small>}
                <div className="arca-instructions-view-toggle" role="group" aria-label={t('instructions.preview')}><button type="button" aria-pressed={!preview} onClick={() => setPreview(false)}>{t('instructions.source')}</button><button type="button" aria-pressed={preview} onClick={() => setPreview(true)}>{t('instructions.preview')}</button></div>
              </div>
            </div>
            {preview ? <div className="arca-instructions-preview"><TraceMarkdown text={text} /></div>
              : <textarea className="arca-instructions-text" aria-label={t('instructions.source')} value={text} readOnly={busy || !creating && !file?.editable} spellCheck={false} autoCapitalize="off" autoCorrect="off" maxLength={1024 * 1024}
                onChange={event => { if (remember(event.target.value)) setText(event.target.value); }}
                onKeyDown={event => { if ((event.metaKey || event.ctrlKey) && event.key === 's') { event.preventDefault(); if (dirty || creating) void save(); } }} />}
            <footer className="arca-instructions-editor-footer"><span>{t(`instructions.reload.${inventory?.reload ?? 'unknown'}`)}</span>
              <button type="button" className="btn-secondary" disabled={busy || !dirty && !creating} onClick={() => { drafts.delete(agentId); setText(snapshot?.text ?? ''); if (creating) { setCreating(false); setSnapshot(null); } }}>{t('instructions.cancel')}</button>
              <button type="button" className="btn-primary" disabled={busy || !creating && (!dirty || !file?.editable)} onClick={() => void save()}>{t(busy ? 'instructions.saving' : creating ? 'instructions.create' : 'instructions.save')}</button>
            </footer>
          </> : <div className="arca-instructions-empty"><DocumentIcon className="w-7 h-7" /><strong>{t(inventory?.files.length ? 'instructions.unreadable' : inventory?.supported ? 'instructions.empty' : 'instructions.unsupported')}</strong>
            <p>{t(inventory?.files.length ? 'instructions.chooseFile' : inventory?.create ? 'instructions.createHint' : 'instructions.guided')}</p>
            {inventory?.create && <><code>{inventory.create.path}</code><button type="button" className="btn-secondary" disabled={busy} onClick={() => { setCreating(true); setText(inventory.create!.initial_text); remember(inventory.create!.initial_text, null, true); }}>{t('instructions.create')}</button></>}
          </div>}
      </div>
    </div>
    <div className="arca-instructions-footnote"><span>{t('instructions.untouched')}</span>
      {path && <button type="button" onClick={() => void openPath(path, preferredFileEditor()).catch(e => toast.show({ kind: 'error', msg: formatError(e) }))}><ExternalLinkIcon className="w-3 h-3" />{t('instructions.openEditor')}</button>}
    </div>
    {inventory && <details className="arca-instructions-details"><summary>{t('instructions.nativeDetails')}{file ? ` · ${t(`instructions.loadingState.${file.loading}`)} · ${t(`instructions.purpose.${file.purpose}`)}` : ''}</summary>
      {file && file.readers.length > 1 && <p><strong>{t('instructions.shared')}: </strong>{file.readers.map(reader => reader.name).join(', ')}<br />{t('instructions.readers')}</p>}
      <p>{inventory.native_notes}</p>{inventory.docs[0] && <button type="button" onClick={() => void openUrl(inventory.docs[0]).catch(e => toast.show({ kind: 'error', msg: formatError(e) }))}>{t('instructions.nativeDocs')} ↗</button>}
    </details>}
    {discardAction && <DialogShell kind="review" title={t('instructions.discardTitle')} onClose={() => setDiscardAction(null)} footerEnd={<><button type="button" className="btn-secondary" onClick={() => setDiscardAction(null)}>{t('instructions.keep')}</button><button type="button" className="btn-primary" onClick={() => { const action = discardAction; setDiscardAction(null); drafts.delete(agentId); setText(snapshot?.text ?? ''); action(); }}>{t('instructions.discard')}</button></>}><p>{t('instructions.discardBody')}</p></DialogShell>}
  </section>;
}
