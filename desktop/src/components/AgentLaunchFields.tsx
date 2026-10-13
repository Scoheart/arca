import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentLaunchInfo, LaunchTarget } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { formatLaunchEnvironment, parseLaunchEnvironment } from "../lib/launchEnvironment";
import { Switch } from "./ui";
import { FolderIcon } from "./icons";

export interface AgentLaunchDraft {
  kind: LaunchTarget["kind"];
  app: string;
  appArgs: string;
  newInstance: boolean;
  command: string;
  args: string;
  url: string;
  resetDefault: boolean;
  directory: string;
  environment: string;
}

function fixedKind(info: AgentLaunchInfo): "app" | "cli" {
  return info.kind === "cli" ? "cli" : "app";
}

export function createAgentLaunchDraft(info: AgentLaunchInfo): AgentLaunchDraft {
  const kind = fixedKind(info);
  const target = info.configured_target?.kind === kind ? info.configured_target : info.resolved_target?.kind === kind ? info.resolved_target : null;
  return { kind, app: target?.kind === "app" ? target.path : "",
    appArgs: target?.kind === "app" ? (target.args ?? []).join("\n") : "", newInstance: target?.kind === "app" && Boolean(target.new_instance),
    command: target?.kind === "cli" ? target.command : "", args: target?.kind === "cli" ? target.args.join("\n") : "",
    url: "", resetDefault: false, directory: info.default_directory ?? "",
    environment: formatLaunchEnvironment(target?.env) };
}

export function agentLaunchDraftTarget(draft: AgentLaunchDraft, info?: AgentLaunchInfo): LaunchTarget | null {
  if (info && !draft.resetDefault && JSON.stringify(agentLaunchDraftTarget(draft)) === JSON.stringify(agentLaunchDraftTarget(createAgentLaunchDraft(info)))) return info.configured_target;

  if (draft.resetDefault) return null;
  const env = parseLaunchEnvironment(draft.environment).env;
  if (draft.kind === "app") return { kind: "app", path: draft.app.trim(), args: draft.appArgs.split(/\r?\n/).filter(Boolean), new_instance: draft.newInstance, env };
  return { kind: "cli", command: draft.command.trim(), args: draft.args.split("\n").filter(Boolean), env };
}

function environmentError(draft: AgentLaunchDraft): string | null {
  if (draft.resetDefault) return null;
  return parseLaunchEnvironment(draft.environment).error;
}

export function agentLaunchDraftValid(draft: AgentLaunchDraft) {
  if (environmentError(draft)) return false;
  return draft.resetDefault || Boolean(draft.kind === "app" ? draft.app.trim() : draft.command.trim());
}

export function agentLaunchDraftChanged(info: AgentLaunchInfo, draft: AgentLaunchDraft) {
  if (environmentError(draft)) return true;
  if (draft.directory.trim() !== (info.default_directory ?? "")) return true;
  if (draft.resetDefault) return info.configured_target !== null;
  return JSON.stringify(agentLaunchDraftTarget(draft)) !== JSON.stringify(agentLaunchDraftTarget(createAgentLaunchDraft(info)));
}

export function AgentLaunchFields({ info, draft, disabled, onChange, onBusyChange, onError }: {
  info: AgentLaunchInfo; draft: AgentLaunchDraft; disabled: boolean;
  onChange(draft: AgentLaunchDraft): void; onBusyChange(busy: boolean): void; onError(message: string): void;
}) {
  const { t } = useTranslation();
  const validation = draft.resetDefault ? null : parseLaunchEnvironment(draft.environment);
  const change = (patch: Partial<AgentLaunchDraft>) => onChange({ ...draft, resetDefault: false, ...patch });
  const browse = async () => {
    if (disabled) return;
    onBusyChange(true); onError("");
    try {
      const value = await open({ title: draft.kind === "app" ? t("agentConfiguration.selectApp") : t("agentConfiguration.selectExecutable"), multiple: false, directory: false,
        ...(draft.kind === "app" ? { defaultPath: "/Applications", filters: [{ name: t("agentConfiguration.applications"), extensions: ["app"] }] } : {}) });
      if (value) change(draft.kind === "app" ? { app: value } : { command: value });
    } catch (error) { onError(formatError(error)); }
    finally { onBusyChange(false); }
  };
  const browseDirectory = async () => {
    if (disabled) return;
    onBusyChange(true); onError("");
    try {
      const value = await open({ title: t("agentConfiguration.defaultDirectory"), directory: true, multiple: false,
        ...(draft.directory || info.directory ? { defaultPath: draft.directory || info.directory! } : {}) });
      if (value) change({ directory: value });
    } catch (error) { onError(formatError(error)); }
    finally { onBusyChange(false); }
  };
  const appLabel = info.category === "plugin" ? t("agentConfiguration.hostApp") : info.category === "ide" ? "IDE" : t("agentConfiguration.app");
  return <fieldset className="arca-launch-settings arca-launch-fields" disabled={disabled}>
    <div className="arca-launch-field-modes">
      {(info.configured_target || agentLaunchDraftChanged(info, draft)) && <button type="button" className="arca-launch-reset"
        onClick={() => onChange({ ...createAgentLaunchDraft(info), resetDefault: true, directory: "" })}>{t("agentConfiguration.restoreDefault")}</button>}
    </div>
    {draft.resetDefault ? <p className="arca-launch-hint">{t("agentConfiguration.defaultAfterSave")}</p> : <>
      <label>{draft.kind === "app" ? appLabel : t("agentConfiguration.executable")}<div className="arca-launch-path-input">
          <input className="arca-dialog-input" value={draft.kind === "app" ? draft.app : draft.command} placeholder={draft.kind === "app" ? t("agentConfiguration.appPlaceholder") : t("agentConfiguration.commandPlaceholder")}
            onChange={(event) => change(draft.kind === "app" ? { app: event.target.value } : { command: event.target.value })} />
          <button type="button" className="btn-secondary" title={t("agentConfiguration.selectFile")} aria-label={t("agentConfiguration.selectFile")} onClick={() => void browse()}><FolderIcon className="w-4 h-4" /></button>
        </div></label>
      {draft.kind === "cli" && <label>{t("agentConfiguration.defaultDirectory")}<div className="arca-launch-path-input">
        <input className="arca-dialog-input" value={draft.directory} placeholder={t("agentConfiguration.lastDirectory")} onChange={(event) => change({ directory: event.target.value })} />
        <button type="button" className="btn-secondary" aria-label={t("agentConfiguration.selectDefaultDirectory")} title={t("agentConfiguration.selectFolder")} onClick={() => void browseDirectory()}><FolderIcon className="w-4 h-4" /></button>
      </div></label>}
      <label><span className="arca-launch-label-line">{t("agentConfiguration.arguments")}<span className="arca-launch-hint">{t("agentConfiguration.onePerLine")}</span></span>
        <textarea className="arca-dialog-input arca-launch-arguments" rows={2} value={draft.kind === "app" ? draft.appArgs : draft.args}
          onChange={(event) => change(draft.kind === "app" ? { appArgs: event.target.value } : { args: event.target.value })} />
      </label>
      <label><span className="arca-launch-label-line">{t("agentConfiguration.environment")}<span className="arca-launch-hint">{t("agentConfiguration.environmentHint")}</span></span>
        <textarea className="arca-dialog-input arca-launch-arguments arca-launch-environment-block" rows={6} spellCheck={false} aria-label={t("agentConfiguration.environment")}
          placeholder={"HTTP_PROXY=http://127.0.0.1:6789\nNO_PROXY=localhost,127.0.0.1"} value={draft.environment}
          onChange={(event) => change({ environment: event.target.value })} />
        {validation?.errorCode && <p className="arca-launch-error" role="alert">{t(`agentConfiguration.environmentErrors.${validation.errorCode}`, { line: validation.line })}</p>}
      </label>
      {draft.kind === "app" && <div className="arca-launch-instance-option">
        <div><span>{t("agentConfiguration.newInstance")}</span><p className="arca-launch-hint">{t("agentConfiguration.newInstanceHint")}</p></div>
        <Switch ariaLabel={t("agentConfiguration.newInstance")} checked={draft.newInstance} disabled={disabled} onChange={(value) => change({ newInstance: value })} />
      </div>}
    </>}
  </fieldset>;
}
