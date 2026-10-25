import { useId, useRef, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { AgentLaunchInfo, LaunchProfile } from "../lib/agentLaunch";
import { AgentLaunchFields, agentLaunchDraftTarget, agentLaunchDraftValid, createAgentLaunchDraft, type AgentLaunchDraft } from "./AgentLaunchFields";
import { PlusIcon, TrashIcon } from "./icons";
import { CodexLaunchConfiguration } from "./CodexLaunchConfiguration";
import { parseLaunchEnvironment } from "../lib/launchEnvironment";
import type { CodexConfigurationSource } from "../lib/codexConfiguration";

interface ProfileDraft { id: string; name: string; launch: AgentLaunchDraft }
export interface LaunchProfilesDraft { profiles: ProfileDraft[]; activeProfileId: string }

function savedProfiles(info: AgentLaunchInfo): LaunchProfile[] {
  if (info.profiles?.length) return info.profiles;
  const profiles: LaunchProfile[] = [{ id: "default", name: "默认配置", target: null, default_directory: null }];
  if (info.configured_target || info.default_directory) profiles.push({ id: "custom", name: "配置 1", target: info.configured_target, default_directory: info.default_directory ?? null });
  return profiles;
}

function profileInfo(info: AgentLaunchInfo, profile: LaunchProfile): AgentLaunchInfo {
  return { ...info, configured_target: profile.target, default_directory: profile.default_directory,
    resolved_target: profile.target ?? (info.default_target !== undefined ? info.default_target : info.resolved_target) };
}

export function createLaunchProfilesDraft(info: AgentLaunchInfo): LaunchProfilesDraft {
  const profiles = savedProfiles(info);
  return { activeProfileId: info.active_profile_id ?? profiles[profiles.length - 1].id,
    profiles: profiles.map(profile => ({ id: profile.id, name: profile.name,
      launch: createAgentLaunchDraft(profile.id === "default" ? { ...info, configured_target: null, resolved_target: null, default_directory: null } : profileInfo(info, profile)) })) };
}

export function launchProfilesPayload(info: AgentLaunchInfo, draft: LaunchProfilesDraft) {
  return { activeProfileId: draft.activeProfileId, profiles: draft.profiles.map(profile => {
    if (profile.id === "default") return { id: "default", name: "默认配置", target: null, default_directory: null };
    const original = savedProfiles(info).find(saved => saved.id === profile.id);
    return { id: profile.id, name: profile.name.trim(), target: agentLaunchDraftTarget(profile.launch, original ? profileInfo(info, original) : undefined),
      default_directory: profile.launch.directory.trim() || null };
  }) };
}

export function launchProfilesValid(draft: LaunchProfilesDraft) {
  const fixed = draft.profiles[0];
  return fixed?.id === "default" && fixed.name === "默认配置"
    && !fixed.launch.app && !fixed.launch.command && !fixed.launch.appArgs && !fixed.launch.args && !fixed.launch.environment && !fixed.launch.directory && !fixed.launch.newInstance
    && draft.profiles.length <= 32 && draft.profiles.some(profile => profile.id === draft.activeProfileId)
    && draft.profiles.every(profile => profile.id === "default" || (profile.name.trim().length > 0 && Array.from(profile.name.trim()).length <= 80
      && !/[\u0000-\u001f\u007f-\u009f]/.test(profile.name) && agentLaunchDraftValid(profile.launch)));
}

export function launchProfilesChanged(info: AgentLaunchInfo, draft: LaunchProfilesDraft) {
  // Invalid environment text must remain dirty without being parsed into a payload.
  if (!launchProfilesValid(draft)) return true;
  return JSON.stringify(launchProfilesPayload(info, draft)) !== JSON.stringify(launchProfilesPayload(info, createLaunchProfilesDraft(info)));
}

export function AgentLaunchProfiles({ info, draft, disabled, onChange, onBusyChange, onError, onEditConfiguration }: {
  info: AgentLaunchInfo; draft: LaunchProfilesDraft; disabled: boolean;
  onChange(draft: LaunchProfilesDraft): void; onBusyChange(busy: boolean): void; onError(message: string): void;
  onEditConfiguration?(source: CodexConfigurationSource, codexHome?: string): void;
}) {
  const { t } = useTranslation();
  const id = useId();
  const tabRefs = useRef(new Map<string, HTMLButtonElement>());
  const selected = draft.profiles.find(profile => profile.id === draft.activeProfileId) ?? draft.profiles[0];
  const isDefault = selected.id === "default";
  const original = savedProfiles(info).find(profile => profile.id === selected.id);
  const selectedInfo = profileInfo(info, original ?? { id: selected.id, name: selected.name, target: null, default_directory: null });
  const profileLabel = (profile: ProfileDraft) => profile.id === "default" && profile.name === "默认配置"
    ? t("agentConfiguration.defaultProfile") : profile.name || t("agentConfiguration.unnamedProfile");
  const select = (activeProfileId: string) => {
    if (!disabled) onChange({ ...draft, activeProfileId });
  };
  const navigate = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (disabled) return;
    let next: number;
    switch (event.key) {
      case "ArrowUp": next = (index + draft.profiles.length - 1) % draft.profiles.length; break;
      case "ArrowDown": next = (index + 1) % draft.profiles.length; break;
      case "Home": next = 0; break;
      case "End": next = draft.profiles.length - 1; break;
      default: return;
    }
    event.preventDefault();
    select(draft.profiles[next].id);
    tabRefs.current.get(draft.profiles[next].id)?.focus();
  };
  const update = (patch: Partial<ProfileDraft>) => { if (!isDefault) onChange({ ...draft, profiles: draft.profiles.map(profile => profile.id === selected.id ? { ...profile, ...patch } : profile) }); };
  const add = () => {
    const id = crypto.randomUUID();
    let number = draft.profiles.length;
    while (draft.profiles.some(profile => profile.name === t("agentConfiguration.profileNumber", { number }))) number += 1;
    const launch = isDefault ? createAgentLaunchDraft({ ...info, configured_target: null, default_directory: null,
      resolved_target: info.default_target !== undefined ? info.default_target : info.resolved_target }) : { ...selected.launch };
    onChange({ activeProfileId: id, profiles: [...draft.profiles, { id, name: t("agentConfiguration.profileNumber", { number }), launch }] });
  };
  const remove = () => {
    if (isDefault) return;
    const profiles = draft.profiles.filter(profile => profile.id !== selected.id);
    if (profiles.length) {
      onChange({ activeProfileId: profiles[0].id, profiles });
      tabRefs.current.get(profiles[0].id)?.focus();
    }
  };
  return <div className="arca-launch-profiles">
    <div className="arca-launch-profile-sidebar">
      <span id={`${id}-label`} className="arca-launch-profile-caption">{t("agentConfiguration.launchProfile")}</span>
      <div className="arca-launch-profile-tabs" role="tablist" aria-orientation="vertical" aria-labelledby={`${id}-label`}>
        {draft.profiles.map((profile, index) => <button key={profile.id} type="button" role="tab" id={`${id}-tab-${profile.id}`}
          ref={element => { if (element) tabRefs.current.set(profile.id, element); else tabRefs.current.delete(profile.id); }}
          aria-controls={`${id}-panel`} aria-selected={selected.id === profile.id} tabIndex={selected.id === profile.id ? 0 : -1}
          disabled={disabled} title={profileLabel(profile)} onClick={() => select(profile.id)} onKeyDown={event => navigate(event, index)}>
          <span>{profileLabel(profile)}</span>
        </button>)}
      </div>
      <button type="button" className="arca-launch-profile-add btn-secondary" disabled={disabled || draft.profiles.length >= 32} onClick={add}
        aria-label={t("agentConfiguration.addProfile")}><PlusIcon className="w-4 h-4" />{t("agentConfiguration.addProfileShort")}</button>
    </div>
    <div key={selected.id} className="arca-launch-profile-panel" role="tabpanel" id={`${id}-panel`} aria-labelledby={`${id}-tab-${selected.id}`} tabIndex={0}>
      <fieldset className="arca-launch-profile-editor" disabled={disabled}>
        <div className="arca-launch-profile-toolbar">
          <label className="arca-launch-profile-name">{t("agentConfiguration.profileName")}
            <input className="arca-dialog-input" value={isDefault ? t("agentConfiguration.defaultProfile") : selected.name} disabled={disabled || isDefault} maxLength={80} onChange={event => update({ name: event.target.value })} />
          </label>
          <button type="button" className="btn-ghost" disabled={disabled || isDefault} onClick={remove}
            aria-label={t("agentConfiguration.deleteProfile")} title={t("agentConfiguration.deleteProfile")}><TrashIcon className="w-4 h-4" /></button>
        </div>
      </fieldset>
      {info.agent_id === "codex" && onEditConfiguration && <CodexLaunchConfiguration args={selected.launch.args} disabled={disabled || isDefault}
        onChange={args => update({ launch: { ...selected.launch, args } })}
        onEdit={source => onEditConfiguration(source, parseLaunchEnvironment(selected.launch.environment).env.CODEX_HOME)} />}
      <AgentLaunchFields key={selected.id} info={selectedInfo} draft={selected.launch} disabled={disabled} readOnlyDefault={isDefault} onChange={launch => update({ launch })}
        onBusyChange={onBusyChange} onError={onError} />
    </div>
  </div>;
}
