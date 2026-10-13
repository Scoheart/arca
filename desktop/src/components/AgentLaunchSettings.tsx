import { useState } from "react";
import { configureAgentLaunch, type AgentLaunchInfo } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { AgentGlyph } from "./brandIcons";
import { AgentLaunchFields, agentLaunchDraftTarget, agentLaunchDraftValid, createAgentLaunchDraft } from "./AgentLaunchFields";

export function AgentLaunchSettings({ info, onClose, onSaved }: {
  info: AgentLaunchInfo; onClose(): void; onSaved(): void;
}) {
  const [draft, setDraft] = useState(() => createAgentLaunchDraft(info));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const save = async () => {
    if (busy || !agentLaunchDraftValid(draft)) return;
    setBusy(true); setError("");
    try {
      await configureAgentLaunch(info.agent_id, agentLaunchDraftTarget(draft, info), draft.directory.trim());
      onSaved(); onClose();
    } catch (failure) { setError(formatError(failure)); }
    finally { setBusy(false); }
  };
  return <DialogShell kind="editor" size="md" title="启动设置" subtitle={info.name} busy={busy} onClose={onClose}
    leading={<AgentGlyph id={info.agent_id} name={info.name} size={28} />}
    footerEnd={<><button type="button" className="btn-ghost" onClick={onClose} disabled={busy}>取消</button>
      <button type="button" className="btn-primary" disabled={busy || !agentLaunchDraftValid(draft)} onClick={() => void save()}>保存</button></>}>
    <AgentLaunchFields info={info} draft={draft} disabled={busy} onChange={setDraft} onBusyChange={setBusy} onError={setError} />
    {error && <p role="alert" className="arca-launch-error">{error}</p>}
  </DialogShell>;
}
