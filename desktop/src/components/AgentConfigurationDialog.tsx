import { useTranslation } from "react-i18next";
import { RESOURCE_CATEGORIES, ResourceIcon } from "./resourcePresentation";
import { Fragment, useEffect, useId, useRef, useState, type ReactNode } from "react";
import type {
  AgentConfigurationPatch,
  AgentInfo,
  ApiKeyDelivery,
  AssetOperationPlan,
  ModelAgentView,
} from "../lib/types";
import {
  cancelOperation,
  commitOperation,
  planOperation,
  setAgentCredentialDelivery,
} from "../lib/api";
import { formatError } from "../lib/format";
import { DialogShell } from "./DialogShell";
import { AssetOperationReviewDialog } from "./AssetOperationReviewDialog";
import { ExternalLinkIcon, KeyIcon, PlusIcon, TrashIcon } from "./icons";
import { ToastNotice, useToast } from "./Toast";
import { configureAgentLaunch, getAgentLaunchInfo, type AgentLaunchInfo } from "../lib/agentLaunch";
import { AgentLaunchFields, agentLaunchDraftChanged, agentLaunchDraftTarget, agentLaunchDraftValid, createAgentLaunchDraft, type AgentLaunchDraft } from "./AgentLaunchFields";
import { AgentGlyph } from "./brandIcons";
import { FormSelect } from "./FormSelect";
import "./AgentLaunch.css";
import { homeDir } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import { preferredFileEditor } from "./FileEditorSelect";

function deliveryLabel(value: ApiKeyDelivery) {
  if (value === "env") return "agentConfiguration.deliveryEnv";
  if (value === "command") return "agentConfiguration.deliveryCommand";
  if (value === "agent-store") return "agentConfiguration.deliveryStore";
  if (value === "plaintext") return "agentConfiguration.deliveryPlaintext";
  return "agentConfiguration.deliveryAuto";
}

function resolvedAgentDelivery(agent: ModelAgentView): ApiKeyDelivery {
  const available = agent.available_deliveries ?? [];
  const stored = agent.default_delivery ?? "plaintext";
  if (available.includes(stored)) return stored;
  if (available.includes("plaintext")) return "plaintext";
  return available[0] ?? "auto";
}

export function AgentConfigurationDialog({
  agent,
  modelAgent,
  onClose,
  onSaved,
  onLaunchSaved,
  initialSection = "paths",
}: {
  agent: AgentInfo;
  modelAgent: ModelAgentView | null;
  onClose(): void;
  onSaved(): Promise<unknown> | unknown;
  onLaunchSaved?(): void;
  initialSection?: "paths" | "launch";
}) {
  const { t } = useTranslation();
  const [section, setSection] = useState<"launch" | "paths">(initialSection);
  const sectionId = useId();
  const sectionButtons = useRef<Array<HTMLButtonElement | null>>([]);
  const initialModelPaths = modelAgent?.config_paths?.length
    ? modelAgent.config_paths
    : modelAgent?.config_path
      ? [modelAgent.config_path]
      : [];
  const [mcpPath, setMcpPath] = useState(agent.global ?? "");
  const [mcpKey, setMcpKey] = useState(agent.key);
  const [modelPaths, setModelPaths] = useState(initialModelPaths);
  const [skillsPaths, setSkillsPaths] = useState(
    agent.skills_global_dirs?.length
      ? agent.skills_global_dirs
      : agent.skills_global_dir ? [agent.skills_global_dir] : [],
  );
  const hasDelivery = modelAgent?.mode === "managed" && (modelAgent.available_deliveries?.length ?? 0) > 0;
  const [delivery, setDelivery] = useState<ApiKeyDelivery>(() => modelAgent ? resolvedAgentDelivery(modelAgent) : "auto");
  const [savedDelivery, setSavedDelivery] = useState(delivery);
  const [confirmingPlaintext, setConfirmingPlaintext] = useState(false);
  const plaintextApproved = useRef(false);
  const deliveryChanged = hasDelivery && delivery !== savedDelivery;
  const [busy, setBusy] = useState(false);
  const [plan, setPlan] = useState<AssetOperationPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [launchInfo, setLaunchInfo] = useState<AgentLaunchInfo | null>(null);
  const [launchDraft, setLaunchDraft] = useState<AgentLaunchDraft | null>(null);
  const [launchLoading, setLaunchLoading] = useState(true);
  const [launchError, setLaunchError] = useState("");
  const [launchRequest, setLaunchRequest] = useState(0);
  const [browsing, setBrowsing] = useState(false);
  const reviewedConfiguration = useRef<string | null>(null);
  const launchSection = useRef<HTMLElement>(null);
  const didFocusLaunch = useRef(false);
  const toast = useToast();
  const hasMcp = agent.has_global;
  const hasModel = modelAgent !== null;
  const hasSkills = agent.skills_global_dir !== null;

  const configurationPatch = (): AgentConfigurationPatch => ({
    ...(hasMcp ? { mcp: { path: mcpPath.trim(), key: mcpKey.trim() } } : {}),
    ...(hasModel ? { model: { paths: modelPaths.map((path) => path.trim()) } } : {}),
    ...(hasSkills ? { skill: { global_dir: skillsPaths[0]?.trim() ?? "", alias_dirs: skillsPaths.slice(1).map((path) => path.trim()) } } : {}),
  });
  const [savedConfiguration, setSavedConfiguration] = useState(() => JSON.stringify(configurationPatch()));
  const configurationChanged = JSON.stringify(configurationPatch()) !== savedConfiguration;
  const launchChanged = Boolean(launchInfo && launchDraft && agentLaunchDraftChanged(launchInfo, launchDraft));
  const configurationValid = (!hasMcp || (mcpPath.trim().length > 0 && mcpKey.trim().length > 0))
    && (!hasModel || (modelPaths.length > 0
      && modelPaths.every((path) => path.trim().length > 0)))
    && (!hasSkills || (skillsPaths.length > 0
      && skillsPaths.every((path) => path.trim().length > 0)));
  const canSubmit = !busy && !browsing && !launchLoading && (configurationChanged || launchChanged || deliveryChanged)
    && (!configurationChanged || configurationValid) && (!launchChanged || launchDraft && agentLaunchDraftValid(launchDraft));

  useEffect(() => {
    let active = true;
    setLaunchLoading(true); setLaunchError("");
    void getAgentLaunchInfo(agent.id).then((info) => {
      if (!active) return;
      setLaunchInfo(info); setLaunchDraft(createAgentLaunchDraft(info));
    }).catch((failure) => { if (active) setLaunchError(formatError(failure)); })
      .finally(() => { if (active) setLaunchLoading(false); });
    return () => { active = false; };
  }, [agent.id, launchRequest]);

  useEffect(() => {
    if (initialSection !== "launch" || section !== "launch" || launchLoading || didFocusLaunch.current) return;
    didFocusLaunch.current = true;
    launchSection.current?.scrollIntoView({ block: "nearest" });
    launchSection.current?.querySelector<HTMLInputElement>("input:not(:disabled)")?.focus({ preventScroll: true });
  }, [initialSection, section, launchLoading]);

  const saveLaunch = async () => {
    if (!launchChanged || !launchDraft) return;
    const info = await configureAgentLaunch(agent.id, agentLaunchDraftTarget(launchDraft, launchInfo ?? undefined), launchDraft.directory.trim());
    setLaunchInfo(info); setLaunchDraft(createAgentLaunchDraft(info));
    onLaunchSaved?.();
  };

  const savePreferences = async () => {
    if (deliveryChanged) {
      await setAgentCredentialDelivery(agent.id, delivery, plaintextApproved.current);
      setSavedDelivery(delivery);
    }
    await saveLaunch();
  };

  const save = async () => {
    if (!canSubmit) return;
    if (deliveryChanged && delivery === "plaintext" && !plaintextApproved.current) {
      setConfirmingPlaintext(true);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (!configurationChanged) {
        await savePreferences();
        await onSaved();
        toast.show({ kind: "success", msg: t("agentConfiguration.updated", { name: agent.name }) });
        onClose();
        return;
      }
      const patch = configurationPatch();
      const result = await planOperation({
        operation: "update_agent_capabilities",
        request: { agent_id: agent.id, patch },
      });
      if (result.domain !== "asset") {
        throw new Error("Core returned a Skill plan for an Agent configuration request");
      }
      reviewedConfiguration.current = JSON.stringify(patch);
      setPlan(result.plan);
    } catch (error) {
      const message = formatError(error);
      setError(message);
      toast.show({ kind: "error", msg: t("agentConfiguration.saveFailed", { error: message }) });
    } finally {
      setBusy(false);
    }
  };

  const commit = async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    let configurationCommitted = false;
    let launchCommitted = false;
    try {
      const result = await commitOperation({
        domain: "asset",
        request: {
          operation_id: plan.operation_id,
          candidate_hash: plan.candidate_hash,
        },
      });
      if (result.domain !== "asset") {
        throw new Error("Core returned a Skill inventory for an Agent configuration commit");
      }
      if (!result.converged) {
        await onSaved();
        throw new Error(t("agentConfiguration.convergencePending"));
      }
      configurationCommitted = true;
      setSavedConfiguration(reviewedConfiguration.current ?? JSON.stringify(configurationPatch()));
      setPlan(null);
      await savePreferences();
      launchCommitted = true;
      await onSaved();
      toast.show({ kind: "success", msg: t("agentConfiguration.updated", { name: agent.name }) });
      onClose();
    } catch (commitError) {
      setError(configurationCommitted
        ? t(launchCommitted ? "agentConfiguration.refreshFailed" : "agentConfiguration.preferencesIncomplete", { error: formatError(commitError) })
        : formatError(commitError));
      if (configurationCommitted && !launchCommitted) {
        // Keep unsaved preferences so retry does not repeat committed changes.
        try { await onSaved(); } catch { /* The original save error stays visible. */ }
      }
    } finally {
      setBusy(false);
    }
  };

  const cancelPlan = async () => {
    if (!plan) return onClose();
    setBusy(true);
    try {
      await cancelOperation({
        domain: "asset",
        operation_id: plan.operation_id,
      });
      setPlan(null);
      plaintextApproved.current = false;
      setError(null);
    } catch (cancelError) {
      setError(formatError(cancelError));
    } finally {
      setBusy(false);
    }
  };

  const updateModelPath = (index: number, value: string) => {
    setModelPaths((current) => current.map((path, candidate) => (
      candidate === index ? value : path
    )));
  };

  const updateSkillsPath = (index: number, value: string) => {
    setSkillsPaths((current) => current.map((path, candidate) => (
      candidate === index ? value : path
    )));
  };

  if (confirmingPlaintext) {
    return (
      <DialogShell kind="review" size="sm" className="arca-plaintext-confirmation"
        title={t("agentConfiguration.plaintextTitle")} subtitle={agent.name}
        onClose={() => setConfirmingPlaintext(false)}
        footerEnd={<>
          <button type="button" className="btn-secondary" onClick={() => setConfirmingPlaintext(false)}>{t("agentConfiguration.backToEdit")}</button>
          <button type="button" className="btn-danger" onClick={() => {
            plaintextApproved.current = true;
            setConfirmingPlaintext(false);
            void save();
          }}>{t("agentConfiguration.confirmPlaintext")}</button>
        </>}
      >
        <div className="arca-plaintext-confirmation-body">
          <strong>{t("agentConfiguration.plaintextDescription", { name: agent.name })}</strong>
          <code>{modelPaths[0]?.trim() || modelAgent?.config_path}</code>
          <span>{t("agentConfiguration.plaintextScope")}</span>
        </div>
      </DialogShell>
    );
  }

  if (plan) {
    return (
      <AssetOperationReviewDialog
        plan={plan}
        busy={busy || browsing}
        error={error}
        agentName={agent.name}
        cancelLabel={t("agentConfiguration.backToEdit")}
        onCommit={commit}
        onCancel={cancelPlan}
      />
    );
  }

  return (
    <DialogShell
      className="arca-dialog-agent-config"
      kind="editor"
      size="wide"
      title={agent.name}
      leading={<AgentGlyph id={agent.id} name={agent.name} size={32} />}
      status={<div className="arca-agent-config-tabs" role="tablist" aria-label={t("agentConfiguration.tabs")}>
        {([['launch', t('agentConfiguration.launch')], ['paths', t('agentConfiguration.paths')]] as const).map(([value, label], index) => <button
          key={value} ref={(node) => { sectionButtons.current[index] = node; }} type="button" role="tab"
          id={`${sectionId}-${value}-tab`} aria-controls={`${sectionId}-${value}-panel`} aria-selected={section === value}
          tabIndex={section === value ? 0 : -1} disabled={busy || browsing} onClick={() => setSection(value)}
          onKeyDown={(event) => {
            if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === "Home" ? 0 : event.key === "End" ? 1 : 1 - index;
            setSection(next === 0 ? "launch" : "paths"); sectionButtons.current[next]?.focus();
          }}>
          {label}{(value === "launch" ? launchChanged : configurationChanged || deliveryChanged) && <i aria-label={t("agentConfiguration.unsaved")} />}
        </button>)}
      </div>}
      busy={busy || browsing}
      onClose={onClose}
      footerStart={configurationChanged ? <span className="arca-agent-config-hint">{t("agentConfiguration.pathChangeHint")}</span> : null}
      footerEnd={(
        <>
          <button type="button" className="btn-ghost" disabled={busy || browsing} onClick={onClose}>{t("common.cancel")}</button>
          <button type="button" className="btn-primary" disabled={!canSubmit} onClick={() => void save()}>
            {busy ? t("common.saving") : configurationChanged ? t("agentConfiguration.continue") : t("common.save")}
          </button>
        </>
      )}
    >
      <div hidden={section !== "paths"} role="tabpanel" id={`${sectionId}-paths-panel`} aria-labelledby={`${sectionId}-paths-tab`}>
      <fieldset className="arca-agent-config-form arca-agent-config-fields" disabled={busy || browsing}>
        {RESOURCE_CATEGORIES.map((resource) => <Fragment key={resource.id}>
          {resource.id === "models" && <>
        {modelPaths.length > 0 ? modelPaths.map((path, index) => (
          <ConfigField
            key={index}
            icon={<ResourceIcon domain="model" />}
            label={modelPaths.length > 1 ? `${resource.label} ${index + 1}` : resource.label}
            value={path}
            openKind="file"
            onChange={(value) => updateModelPath(index, value)}
          />
        )) : (
          <ConfigField
            icon={<ResourceIcon domain="model" />}
            label={resource.label}
            value={t("agentConfiguration.unavailable")}
            disabled
          />
        )}
          </>}
          {resource.id === "mcps" && <>
        {hasMcp ? (
          <div className="arca-agent-config-mcp">
            <ConfigField
              icon={<ResourceIcon domain="mcp" />}
              label={resource.label}
              openKind="file"
              value={mcpPath}
              onChange={setMcpPath}
            />
            <ConfigField
              icon={null}
              label={t("agentConfiguration.configKey")}
              value={mcpKey}
              onChange={setMcpKey}
            />
          </div>
        ) : (
          <ConfigField
            icon={<ResourceIcon domain="mcp" />}
            label={resource.label}
            value={t("agentConfiguration.unavailable")}
            disabled
          />
        )}
          </>}
          {resource.id === "skills" && <>
        {skillsPaths.length > 0 ? skillsPaths.map((path, index) => (
          <ConfigField
            key={index}
            icon={<ResourceIcon domain="skill" />}
            label={skillsPaths.length > 1 ? `${resource.label} ${index + 1}` : resource.label}
            value={path}
            openKind="folder"
            onChange={(value) => updateSkillsPath(index, value)}
            action={index > 0 ? (
              <button
                type="button"
                className="arca-agent-config-remove"
                aria-label={t("agentConfiguration.removeSkillsDirectory", { index: index + 1 })}
                onClick={() => setSkillsPaths((current) => current.filter((_, candidate) => candidate !== index))}
              >
                <TrashIcon className="w-4 h-4" />
              </button>
            ) : null}
          />
        )) : (
          <ConfigField
            icon={<ResourceIcon domain="skill" />}
            label={resource.label}
            value={t("agentConfiguration.unavailable")}
            disabled
          />
        )}
        {skillsPaths.length > 0 && skillsPaths.length < 16 && (
          <button
            type="button"
            className="arca-agent-config-add"
            onClick={() => setSkillsPaths((current) => [...current, ""])}
          >
            <PlusIcon className="w-3.5 h-3.5" />{t("agentConfiguration.addSkillsDirectory")}
          </button>
        )}
          </>}
        </Fragment>)}
      </fieldset>
      {hasDelivery && (
        <section className="arca-agent-config-credential" aria-label={t("agentConfiguration.credentials")}>
          <div className="arca-agent-config-credential-row">
            <span className="arca-agent-field-caption"><KeyIcon className="w-4 h-4" />{t("agentConfiguration.delivery")}</span>
            <FormSelect ariaLabel={t("agentConfiguration.delivery")} value={delivery} disabled={busy || browsing}
              options={(modelAgent?.available_deliveries ?? []).map((value) => ({ value, label: t(deliveryLabel(value)) }))}
              onChange={(value) => { setDelivery(value as ApiKeyDelivery); plaintextApproved.current = false; }} />
          </div>
        </section>
      )}
      </div>
      <div hidden={section !== "launch"} role="tabpanel" id={`${sectionId}-launch-panel`} aria-labelledby={`${sectionId}-launch-tab`}>
      <section ref={launchSection} className="arca-agent-config-launch" aria-label={t("agentConfiguration.launchSettings")}>
        {launchLoading ? <p className="arca-launch-hint" role="status">{t("agentConfiguration.reading")}</p>
          : launchInfo && launchDraft ? <AgentLaunchFields info={launchInfo} draft={launchDraft} disabled={busy || browsing}
            onChange={setLaunchDraft} onBusyChange={setBrowsing} onError={setLaunchError} /> : null}
        {launchError && <ToastNotice message={launchError} action={!launchInfo
          ? <button type="button" className="btn-ghost" onClick={() => setLaunchRequest((value) => value + 1)}>{t("common.retry")}</button> : undefined} />}
      </section>
      </div>
      {error && <ToastNotice message={t("agentConfiguration.saveFailed", { error })} />}
    </DialogShell>
  );
}

function ConfigField({
  icon,
  label,
  value,
  disabled = false,
  onChange,
  action,
  openKind,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  disabled?: boolean;
  onChange?(value: string): void;
  action?: ReactNode;
  openKind?: "file" | "folder";
}) {
  const toast = useToast();
  const { t } = useTranslation();
  const openLocation = async () => {
    try {
      const home = (await homeDir()).replace(/\/$/, "");
      const path = value.trim();
      const absolute = path === "~" ? home : path.startsWith("~/") ? `${home}/${path.slice(2)}` : path;
      const editor = openKind === "file" ? preferredFileEditor() : undefined;
      if (editor) await openPath(absolute, editor); else await openPath(absolute);
    } catch (error) { toast.show({ kind: "error", msg: t("agentConfiguration.openFailed", { error: formatError(error) }) }); }
  };
  if (disabled) return <div className="arca-agent-config-unavailable">
    <span className="arca-agent-field-caption">{icon}{label}</span><span>{t("agentConfiguration.unavailable")}</span>
  </div>;
  return (
    <label data-path-field={openKind || undefined} className="arca-agent-config-field" data-disabled={disabled || undefined}>
      <span className="arca-agent-field-caption">{icon}{label}</span>
      <span className="arca-agent-field-control">
      <input
        className="arca-model-field"
        value={value}
        disabled={disabled}
        spellCheck={false}
        onChange={(event) => onChange?.(event.target.value)}
      />
      {(openKind || action) && <span className="arca-agent-config-field-actions">
        {openKind && <button type="button" className="arca-agent-config-remove" disabled={disabled || !value.trim()}
          title={t(openKind === "file" ? "agentConfiguration.openEditor" : "agentConfiguration.openFolder")} aria-label={t("agentConfiguration.openLabel", { label })}
          onClick={(event) => { event.preventDefault(); void openLocation(); }}><ExternalLinkIcon className="w-4 h-4" /></button>}
        {action}
      </span>}
      </span>
    </label>
  );
}
