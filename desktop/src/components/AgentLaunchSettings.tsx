import { ToastNotice } from "./Toast";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { configureAgentLaunchProfiles, getAgentLaunchInfo, type AgentLaunchInfo } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { AgentGlyph } from "./brandIcons";
import { AgentLaunchProfiles, launchProfilesChanged, launchProfilesPayload, launchProfilesValid, createLaunchProfilesDraft } from "./AgentLaunchProfiles";

export function AgentLaunchSettings({ info, onClose, onSaved, onLaunch }: {
  info: AgentLaunchInfo; onClose(): void; onSaved(): void; onLaunch(info: AgentLaunchInfo): Promise<boolean>;
}) {
  const { t } = useTranslation();
  const [savedInfo, setSavedInfo] = useState(info);
  const [draft, setDraft] = useState(() => createLaunchProfilesDraft(info));
  const [action, setAction] = useState<"save" | "run" | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [error, setError] = useState("");
  const submitting = useRef(false);
  const busy = action !== null || browsing;
  const valid = launchProfilesValid(draft);
  const changed = launchProfilesChanged(savedInfo, draft);
  const canRun = info.supported && Boolean(info.kind) && valid
    && (draft.activeProfileId !== "default" || Boolean(savedInfo.default_target));
  const submit = async (run: boolean) => {
    if (submitting.current || busy || !valid || run && !canRun) return;
    submitting.current = true;
    setAction(run ? "run" : "save"); setError("");
    try {
      let current = savedInfo;
      if (changed) {
        const payload = launchProfilesPayload(savedInfo, draft);
        current = await configureAgentLaunchProfiles(info.agent_id, payload.profiles, payload.activeProfileId, savedInfo.profiles_revision ?? "");
        setSavedInfo(current); setDraft(createLaunchProfilesDraft(current));
        onSaved();
      } else if (run) {
        current = await getAgentLaunchInfo(info.agent_id);
        if (current.profiles_revision !== savedInfo.profiles_revision) throw new Error(t("agentConfiguration.launchChanged"));
      }
      if (!run || await onLaunch(current)) onClose();
    } catch (failure) { setError(formatError(failure)); }
    finally { submitting.current = false; setAction(null); }
  };
  return <DialogShell className="arca-dialog-agent-launch" kind="editor" size="wide" title={info.name} subtitle={t("agentConfiguration.launchSettings")} busy={busy} onClose={onClose}
    leading={<AgentGlyph id={info.agent_id} name={info.name} size={32} />}
    footerEnd={<><button type="button" className="btn-ghost" onClick={onClose} disabled={busy}>{t("common.cancel")}</button>
      <button type="button" className="btn-secondary" disabled={busy || !valid || !changed} onClick={() => void submit(false)}>{action === "save" ? t("common.saving") : t("common.save")}</button>
      <button type="button" className="btn-primary" disabled={busy || !canRun} onClick={() => void submit(true)}>
        {action === "run" ? t("agentConfiguration.running") : t(changed ? "agentConfiguration.saveAndRun" : "agentConfiguration.run")}
      </button></>}>
    <AgentLaunchProfiles info={savedInfo} draft={draft} disabled={busy} onChange={setDraft} onBusyChange={setBrowsing} onError={setError} />
    <ToastNotice message={error} />
  </DialogShell>;
}
