import { useId, useState, type ReactNode } from "react";
import { ToastNotice } from "./Toast";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentLaunchInfo, LaunchTarget } from "../lib/agentLaunch";
import { formatError } from "../lib/format";
import { formatLaunchEnvironment, parseLaunchEnvironment } from "../lib/launchEnvironment";
import { Switch } from "./ui";
import { CheckIcon, EditIcon, FolderIcon } from "./icons";

export interface AgentLaunchDraft {
  kind: LaunchTarget["kind"];
  app: string;
  appArgs: string;
  newInstance: boolean;
  command: string;
  args: string;
  url: string;
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
    url: "", directory: info.default_directory ?? "",
    environment: formatLaunchEnvironment(target?.env) };
}

export function agentLaunchDraftTarget(draft: AgentLaunchDraft, info?: AgentLaunchInfo): LaunchTarget | null {
  if (info && JSON.stringify(agentLaunchDraftTarget(draft)) === JSON.stringify(agentLaunchDraftTarget(createAgentLaunchDraft(info)))) return info.configured_target;

  const env = parseLaunchEnvironment(draft.environment).env;
  if (draft.kind === "app") return { kind: "app", path: draft.app.trim(), args: draft.appArgs.split(/\r?\n/).filter(Boolean), new_instance: draft.newInstance, env };
  return { kind: "cli", command: draft.command.trim(), args: draft.args.split("\n").filter(Boolean), env };
}

function environmentError(draft: AgentLaunchDraft): string | null {
  return parseLaunchEnvironment(draft.environment).error;
}

export function agentLaunchDraftValid(draft: AgentLaunchDraft) {
  if (environmentError(draft)) return false;
  return Boolean(draft.kind === "app" ? draft.app.trim() : draft.command.trim());
}

function LaunchTextField({ label, hint, value, count, rows, placeholder, invalid = false, disabled, readOnly, onChange, children }: {
  label: string; hint: string; value: string; count: number; rows: number; placeholder?: string;
  invalid?: boolean; disabled: boolean; readOnly: boolean; onChange(value: string): void; children: ReactNode;
}) {
  const { t } = useTranslation();
  const id = useId();
  const [isEditing, setEditing] = useState(false);
  // Keep invalid text visible instead of presenting a partially parsed preview.
  const editing = isEditing || invalid;
  return <section className="arca-launch-text-field" aria-labelledby={`${id}-label`}>
    <div className="arca-launch-text-heading">
      <span id={`${id}-label`} className="arca-launch-text-label">{label}
        {!editing && count > 0 && <span className="arca-launch-value-count">{count}</span>}
      </span>
      {!readOnly && <button type="button" className="arca-launch-text-edit" disabled={disabled || (editing && invalid)}
        aria-label={t(editing ? "agentConfiguration.finishEditingField" : "agentConfiguration.editField", { field: label })}
        aria-expanded={editing} aria-controls={id} onClick={() => setEditing(!editing)}>
        {editing ? <CheckIcon className="w-3.5 h-3.5" /> : <EditIcon className="w-3.5 h-3.5" />}
        {t(editing ? "agentConfiguration.doneEditing" : "common.edit")}
      </button>}
    </div>
    {editing ? <div className="arca-launch-text-editor" id={id}>
      <textarea className="arca-dialog-input arca-launch-arguments" rows={rows} spellCheck={false} autoFocus
        aria-labelledby={`${id}-label`} aria-describedby={`${id}-hint`} aria-invalid={invalid || undefined}
        autoCapitalize="off" autoCorrect="off" value={value} placeholder={placeholder}
        onChange={event => onChange(event.target.value)} />
      <span id={`${id}-hint`} className="arca-launch-hint">{hint}</span>
    </div> : <div className="arca-launch-value-preview" id={id}>{children}</div>}
  </section>;
}

export function AgentLaunchFields({ info, draft, disabled, readOnlyDefault = false, onChange, onBusyChange, onError }: {
  info: AgentLaunchInfo; draft: AgentLaunchDraft; disabled: boolean; readOnlyDefault?: boolean;
  onChange(draft: AgentLaunchDraft): void; onBusyChange(busy: boolean): void; onError(message: string): void;
}) {
  const { t } = useTranslation();
  const validation = parseLaunchEnvironment(draft.environment);
  const argumentsText = draft.kind === "app" ? draft.appArgs : draft.args;
  const argumentsList = argumentsText.split(draft.kind === "app" ? /\r?\n/ : "\n").filter(Boolean);
  const environmentEntries = Object.entries(validation.env);
  const change = (patch: Partial<AgentLaunchDraft>) => { if (!readOnlyDefault) onChange({ ...draft, ...patch }); };
  const browse = async () => {
    if (disabled || readOnlyDefault) return;
    onBusyChange(true); onError("");
    try {
      const value = await open({ title: draft.kind === "app" ? t("agentConfiguration.selectApp") : t("agentConfiguration.selectExecutable"), multiple: false, directory: false,
        ...(draft.kind === "app" ? { defaultPath: "/Applications", filters: [{ name: t("agentConfiguration.applications"), extensions: ["app"] }] } : {}) });
      if (value) change(draft.kind === "app" ? { app: value } : { command: value });
    } catch (error) { onError(formatError(error)); }
    finally { onBusyChange(false); }
  };
  const browseDirectory = async () => {
    if (disabled || readOnlyDefault) return;
    onBusyChange(true); onError("");
    try {
      const value = await open({ title: t("agentConfiguration.defaultDirectory"), directory: true, multiple: false,
        ...(draft.directory || info.directory ? { defaultPath: draft.directory || info.directory! } : {}) });
      if (value) change({ directory: value });
    } catch (error) { onError(formatError(error)); }
    finally { onBusyChange(false); }
  };
  const appLabel = info.category === "plugin" ? t("agentConfiguration.hostApp") : info.category === "ide" ? "IDE" : t("agentConfiguration.app");
  return <fieldset className="arca-launch-settings arca-launch-fields" disabled={disabled || readOnlyDefault}>
      <label>{draft.kind === "app" ? appLabel : t("agentConfiguration.executable")}<div className="arca-launch-path-input">
          <input className="arca-dialog-input" value={draft.kind === "app" ? draft.app : draft.command} placeholder={readOnlyDefault ? "" : draft.kind === "app" ? t("agentConfiguration.appPlaceholder") : t("agentConfiguration.commandPlaceholder")}
            onChange={(event) => change(draft.kind === "app" ? { app: event.target.value } : { command: event.target.value })} />
          <button type="button" className="btn-secondary" title={t("agentConfiguration.selectFile")} aria-label={t("agentConfiguration.selectFile")} onClick={() => void browse()}><FolderIcon className="w-4 h-4" /></button>
        </div></label>
      {draft.kind === "cli" && <label>{t("agentConfiguration.defaultDirectory")}<div className="arca-launch-path-input">
        <input className="arca-dialog-input" value={draft.directory} placeholder={readOnlyDefault ? "" : t("agentConfiguration.lastDirectory")} onChange={(event) => change({ directory: event.target.value })} />
        <button type="button" className="btn-secondary" aria-label={t("agentConfiguration.selectDefaultDirectory")} title={t("agentConfiguration.selectFolder")} onClick={() => void browseDirectory()}><FolderIcon className="w-4 h-4" /></button>
      </div></label>}
      <LaunchTextField label={t("agentConfiguration.arguments")} hint={t("agentConfiguration.onePerLine")}
        value={argumentsText} count={argumentsList.length} rows={3} disabled={disabled} readOnly={readOnlyDefault}
        onChange={value => change(draft.kind === "app" ? { appArgs: value } : { args: value })}>
        {argumentsList.length ? <ol className="arca-launch-argument-list">
          {argumentsList.map((argument, index) => <li key={index}><code>{argument}</code></li>)}
        </ol> : <span className="arca-launch-value-empty">{t("agentConfiguration.noArguments")}</span>}
      </LaunchTextField>
      <LaunchTextField label={t("agentConfiguration.environment")} hint={t("agentConfiguration.environmentHint")}
        value={draft.environment} count={environmentEntries.length} rows={6} disabled={disabled} readOnly={readOnlyDefault}
        invalid={Boolean(validation.errorCode)} placeholder={"HTTP_PROXY=http://127.0.0.1:6789\nNO_PROXY=localhost,127.0.0.1"}
        onChange={environment => change({ environment })}>
        {environmentEntries.length ? <dl className="arca-launch-environment-list">
          {environmentEntries.map(([name, value]) => <div className="arca-launch-environment-row" key={name}>
            <dt><code>{name}</code></dt><dd><code>{value || <span className="arca-launch-value-empty">{t("agentConfiguration.emptyValue")}</span>}</code></dd>
          </div>)}
        </dl> : <span className="arca-launch-value-empty">{t("agentConfiguration.noEnvironment")}</span>}
      </LaunchTextField>
      {validation.errorCode && <ToastNotice kind="warning" delay={500} message={t(`agentConfiguration.environmentErrors.${validation.errorCode}`, { line: validation.line })} />}
      {draft.kind === "app" && <div className="arca-launch-instance-option">
        <div><span>{t("agentConfiguration.newInstance")}</span><p className="arca-launch-hint">{t("agentConfiguration.newInstanceHint")}</p></div>
        <Switch ariaLabel={t("agentConfiguration.newInstance")} checked={draft.newInstance} disabled={disabled || readOnlyDefault} onChange={(value) => change({ newInstance: value })} />
      </div>}
  </fieldset>;
}
