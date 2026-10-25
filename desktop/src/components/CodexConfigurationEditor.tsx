import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { readCodexConfiguration, renderCodexConfiguration, saveCodexConfiguration, type CodexConfigurationSnapshot, type CodexConfigurationSource } from "../lib/codexConfiguration";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { FormSelect } from "./FormSelect";
import { ToastNotice, useToast } from "./Toast";
import { RefreshIcon } from "./icons";
import "./CodexConfigurationEditor.css";

interface Draft { snapshot: CodexConfigurationSnapshot; text: string; fields: Record<string, string>; patch: Record<string, string>; mode: "form" | "source" }
const OPTIONS: Record<string, string[]> = {
  model_reasoning_effort: ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
  service_tier: ["fast", "flex"], approval_policy: ["on-request", "never"],
  sandbox_mode: ["read-only", "workspace-write", "danger-full-access"], web_search: ["disabled", "cached", "live"],
};
const makeDraft = (snapshot: CodexConfigurationSnapshot): Draft => ({ snapshot, text: snapshot.text, fields: snapshot.fields, patch: {}, mode: snapshot.valid ? "form" : "source" });

export function CodexConfigurationEditor({ initialSource = "native", codexHome, onClose }: {
  initialSource?: CodexConfigurationSource; codexHome?: string; onClose(): void;
}) {
  const { t } = useTranslation();
  const toast = useToast();
  const id = useId();
  const [source, setSource] = useState(initialSource);
  const [drafts, setDrafts] = useState<Partial<Record<CodexConfigurationSource, Draft>>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const lock = useRef(false);
  const generation = useRef(0);
  const draft = drafts[source];
  const dirty = draft && (draft.text !== draft.snapshot.text || Object.keys(draft.patch).length > 0);
  useEffect(() => {
    if (drafts[source]) return;
    const epoch = ++generation.current;
    setBusy(true); setError(""); lock.current = true;
    void readCodexConfiguration(source, codexHome).then(snapshot => {
      if (generation.current === epoch) setDrafts(current => ({ ...current, [source]: makeDraft(snapshot) }));
    }).catch(failure => { if (generation.current === epoch) setError(formatError(failure)); })
      .finally(() => { if (generation.current === epoch) { setBusy(false); lock.current = false; } });
    return () => { ++generation.current; };
  }, [source, codexHome]);
  const update = (patch: Partial<Draft>) => setDrafts(current => current[source] ? ({ ...current, [source]: { ...current[source]!, ...patch } }) : current);
  const retry = async () => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const snapshot = await readCodexConfiguration(source, codexHome);
      setDrafts(current => ({ ...current, [source]: makeDraft(snapshot) }));
    } catch (failure) { setError(formatError(failure)); }
    finally { lock.current = false; setBusy(false); }
  };
  const changeMode = async (mode: Draft["mode"]) => {
    if (lock.current || !draft || draft.mode === mode) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const rendered = await renderCodexConfiguration(draft.text, draft.patch);
      update({ ...rendered, patch: {}, mode });
    } catch (failure) { setError(formatError(failure)); }
    finally { lock.current = false; setBusy(false); }
  };
  const save = async () => {
    if (lock.current || !draft) return;
    lock.current = true; setBusy(true); setError("");
    try {
      const snapshot = await saveCodexConfiguration(source, draft.snapshot, draft.text, draft.patch, codexHome);
      setDrafts(current => ({ ...current, [source]: { ...makeDraft(snapshot), mode: draft.mode } }));
      toast.show({ kind: "success", msg: t("agentConfiguration.codex.saved") });
    } catch (failure) { setError(formatError(failure)); }
    finally { lock.current = false; setBusy(false); }
  };
  return <DialogShell className="arca-dialog-codex-config" kind="editor" size="lg" title={t("agentConfiguration.codex.title")}
    busy={busy} onClose={onClose} footerEnd={<>
      <button type="button" className="btn-ghost" disabled={busy} onClick={onClose}>{t("common.close")}</button>
      <button type="button" className="btn-primary" disabled={busy || !draft || (!dirty && draft.snapshot.exists)} onClick={() => void save()}>{t(busy ? "common.saving" : "common.save")}</button>
    </>}>
    <div className="arca-codex-config-tabs" role="group" aria-label={t("agentConfiguration.codex.source")}>
      {(["native", "arca"] as const).map(value => <button type="button" key={value} aria-pressed={source === value} disabled={busy}
        onClick={() => { setSource(value); setError(""); }}>{t(`agentConfiguration.codex.${value}`)}</button>)}
    </div>
    <p className="arca-codex-config-description">{t(`agentConfiguration.codex.${source}Hint`)}</p>
    {draft ? <>
      <div className="arca-codex-config-location"><code>{draft.snapshot.path}</code>{!draft.snapshot.exists && <span>{t("agentConfiguration.codex.createOnSave")}</span>}</div>
      <div className="arca-codex-config-toolbar">
        <div className="arca-codex-config-tabs" role="group" aria-label={t("agentConfiguration.codex.editorMode")}>
          {(["form", "source"] as const).map(mode => <button type="button" key={mode} aria-pressed={draft.mode === mode} disabled={busy} onClick={() => void changeMode(mode)}>{t(`agentConfiguration.codex.${mode}Mode`)}</button>)}
        </div>
        <button type="button" className="btn-ghost" disabled={busy} onClick={() => void retry()}><RefreshIcon className="w-3.5 h-3.5" />{t(dirty ? "agentConfiguration.codex.discardReload" : "agentConfiguration.codex.reload")}</button>
      </div>
      {draft.mode === "form" ? <fieldset className="arca-codex-config-form" disabled={busy}>
        {["model", ...Object.keys(OPTIONS)].map(field => {
          const value = field in draft.patch ? draft.patch[field] : draft.fields[field] ?? "";
          const change = (value: string) => {
            const patch = { ...draft.patch };
            if (value === (draft.fields[field] ?? "")) delete patch[field]; else patch[field] = value;
            update({ patch });
          };
          return <label key={field} htmlFor={`${id}-${field}`}><span>{t(`agentConfiguration.codex.fields.${field}`)}</span>
            {!(field in draft.fields) ? <button type="button" className="btn-secondary" onClick={() => void changeMode("source")}>{t("agentConfiguration.codex.editComplex")}</button>
              : OPTIONS[field] ? <FormSelect ariaLabel={t(`agentConfiguration.codex.fields.${field}`)} value={value} disabled={busy}
              options={["", ...new Set([...OPTIONS[field], ...(value ? [value] : [])])].map(option => ({ value: option, label: option || t("agentConfiguration.codex.inherit") }))} onChange={change} />
              : <input id={`${id}-${field}`} className="arca-dialog-input" value={value} placeholder={t("agentConfiguration.codex.inherit")} onChange={event => change(event.target.value)} />}
          </label>;
        })}
      </fieldset> : <textarea className="arca-codex-config-text arca-dialog-input" aria-label={t("agentConfiguration.codex.sourceMode")} spellCheck={false} autoCapitalize="off" autoCorrect="off" disabled={busy}
        value={draft.text} maxLength={1024 * 1024} onChange={event => update({ text: event.target.value })}
        onKeyDown={event => { if ((event.metaKey || event.ctrlKey) && event.key === "s") { event.preventDefault(); void save(); } }} />}
    </> : <button type="button" className="btn-secondary" disabled={busy} onClick={() => void retry()}>{t(busy ? "agentConfiguration.reading" : "common.retry")}</button>}
    <ToastNotice message={error} />
  </DialogShell>;
}
