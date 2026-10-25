import { ToastNotice } from "./Toast";
import { useState } from "react";
import { configureAgentLaunchProfiles, type AgentLaunchInfo } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { AgentGlyph } from "./brandIcons";
import { AgentLaunchProfiles, launchProfilesPayload, launchProfilesValid, createLaunchProfilesDraft } from "./AgentLaunchProfiles";

export function AgentLaunchSettings({ info, onClose, onSaved }: {
  info: AgentLaunchInfo; onClose(): void; onSaved(): void;
}) {
  const [draft, setDraft] = useState(() => createLaunchProfilesDraft(info));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const save = async () => {
    if (busy || !launchProfilesValid(draft)) return;
    setBusy(true); setError("");
    try {
      const payload = launchProfilesPayload(info, draft);
      await configureAgentLaunchProfiles(info.agent_id, payload.profiles, payload.activeProfileId, info.profiles_revision ?? "");
      onSaved(); onClose();
    } catch (failure) { setError(formatError(failure)); }
    finally { setBusy(false); }
  };
  return <DialogShell kind="editor" size="md" title="启动设置" subtitle={info.name} busy={busy} onClose={onClose}
    leading={<AgentGlyph id={info.agent_id} name={info.name} size={28} />}
    footerEnd={<><button type="button" className="btn-ghost" onClick={onClose} disabled={busy}>取消</button>
      <button type="button" className="btn-primary" disabled={busy || !launchProfilesValid(draft)} onClick={() => void save()}>保存</button></>}>
    <AgentLaunchProfiles info={info} draft={draft} disabled={busy} onChange={setDraft} onBusyChange={setBusy} onError={setError} />
    <ToastNotice message={error} />
  </DialogShell>;
}
