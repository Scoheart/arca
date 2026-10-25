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
import { DocumentIcon, ExternalLinkIcon, FolderIcon, KeyIcon, PlusIcon, RefreshIcon, TrashIcon } from "./icons";
import { ToastNotice, useToast } from "./Toast";
import { AgentGlyph } from "./brandIcons";
import { FormSelect } from "./FormSelect";
import "./AgentLaunch.css";
import { homeDir } from "@tauri-apps/api/path";
import { openPath } from "@tauri-apps/plugin-opener";
import { preferredFileEditor } from "./FileEditorSelect";
import { open as choosePath } from "@tauri-apps/plugin-dialog";
import { instructionGetPaths, instructionSetPaths, type InstructionPathConfiguration } from "../lib/instructions";

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
  onInstructionPathsSaved,
}: {
  agent: AgentInfo;
  modelAgent: ModelAgentView | null;
  onClose(): void;
  onSaved(): Promise<unknown> | unknown;
  onInstructionPathsSaved?(): void;
}) {
  const { t } = useTranslation();
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
  const [browsing, setBrowsing] = useState(false);
  const [instructionPaths, setInstructionPaths] = useState<InstructionPathConfiguration | null>(null);
  const [instructionEdits, setInstructionEdits] = useState<Record<string, string | null>>({});
  const [instructionLoading, setInstructionLoading] = useState(Boolean(agent.has_instructions));
  const [instructionError, setInstructionError] = useState("");
  const [instructionRequest, setInstructionRequest] = useState(0);
  const reviewedConfiguration = useRef<string | null>(null);
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
  const instructionsChanged = Object.keys(instructionEdits).length > 0;
  const instructionsValid = Object.values(instructionEdits).every(path => path === null || path.trim().length > 0);
  const configurationValid = (!hasMcp || (mcpPath.trim().length > 0 && mcpKey.trim().length > 0))
    && (!hasModel || (modelPaths.length > 0
      && modelPaths.every((path) => path.trim().length > 0)))
    && (!hasSkills || (skillsPaths.length > 0
      && skillsPaths.every((path) => path.trim().length > 0)));
  const canSubmit = !busy && !browsing && !instructionLoading && instructionsValid && (configurationChanged || deliveryChanged || instructionsChanged)
    && (!configurationChanged || configurationValid);

  useEffect(() => {
    if (!agent.has_instructions) return;
    let active = true;
    setInstructionLoading(true); setInstructionError("");
    void instructionGetPaths(agent.id).then(value => {
      if (active) { setInstructionPaths(value); setInstructionEdits({}); }
    }).catch(failure => { if (active) setInstructionError(formatError(failure)); })
      .finally(() => { if (active) setInstructionLoading(false); });
    return () => { active = false; };
  }, [agent.id, agent.has_instructions, instructionRequest]);

  const savePreferences = async () => {
    if (instructionsChanged && instructionPaths) {
      const paths = Object.entries(instructionEdits).map(([id, path]) => ({ id,
        path: path === null || path.trim() === instructionPaths.sources.find(source => source.id === id)?.default_path ? null : path.trim(),
      }));
      const saved = await instructionSetPaths(agent.id, instructionPaths.revision, paths);
      setInstructionPaths(saved); setInstructionEdits({});
      onInstructionPathsSaved?.();
    }
    if (deliveryChanged) {
      await setAgentCredentialDelivery(agent.id, delivery, plaintextApproved.current);
      setSavedDelivery(delivery);
    }
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
    let preferencesCommitted = false;
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
      preferencesCommitted = true;
      await onSaved();
      toast.show({ kind: "success", msg: t("agentConfiguration.updated", { name: agent.name }) });
      onClose();
    } catch (commitError) {
      setError(configurationCommitted
        ? t(preferencesCommitted ? "agentConfiguration.refreshFailed" : "agentConfiguration.preferencesIncomplete", { error: formatError(commitError) })
        : formatError(commitError));
      if (configurationCommitted && !preferencesCommitted) {
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

  const updateInstructionPath = (id: string, value: string | null) => {
    const source = instructionPaths?.sources.find(source => source.id === id);
    setInstructionEdits(current => {
      const next = { ...current };
      if (value === source?.path || value === null && !source?.overridden) delete next[id];
      else next[id] = value;
      return next;
    });
  };
  const browseInstructionPath = async (id: string, value: string) => {
    setBrowsing(true);
    try {
      const home = (await homeDir()).replace(/\/$/, "");
      const defaultPath = value === "~" ? home : value.startsWith("~/") ? `${home}/${value.slice(2)}` : value || undefined;
      const selected = await choosePath({ directory: true, multiple: false, defaultPath, title: t("agentConfiguration.selectInstructionsDirectory") });
      if (typeof selected === "string") updateInstructionPath(id, selected);
    } catch (failure) { toast.show({ kind: "error", msg: formatError(failure) }); }
    finally { setBrowsing(false); }
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
      subtitle={t("agentConfiguration.paths")}
      leading={<AgentGlyph id={agent.id} name={agent.name} size={32} />}
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
        {agent.has_instructions && <section className="arca-agent-instruction-paths" aria-label="Instructions">
          {instructionLoading ? <p className="arca-launch-hint" role="status">{t("agentConfiguration.reading")}</p>
            : instructionPaths?.sources.map((source, index) => {
              const value = source.id in instructionEdits ? instructionEdits[source.id] ?? source.default_path ?? "" : source.path;
              return <div key={source.id} className="arca-agent-instruction-path">
                <ConfigField icon={<DocumentIcon className="w-4 h-4" />} label={t("agentConfiguration.instructionsDirectory", { index: index + 1 })}
                  value={value} openKind="folder" hint={source.kind === "files" ? undefined : `${source.kind === 'mode-rules' ? 'rules-*/ · ' : ''}${source.patterns.join(' · ')}`}
                  onChange={path => updateInstructionPath(source.id, path)}
                  action={<>
                    <button type="button" className="arca-agent-config-remove" title={t("agentConfiguration.selectInstructionsDirectory")} aria-label={t("agentConfiguration.selectInstructionsDirectoryLabel", { index: index + 1 })}
                      onClick={event => { event.preventDefault(); void browseInstructionPath(source.id, value); }}><FolderIcon className="w-4 h-4" /></button>
                    <button type="button" className="arca-agent-config-remove" title={t("agentConfiguration.restoreDefault")} aria-label={t("agentConfiguration.resetInstructionsDirectory", { index: index + 1 })}
                      disabled={!source.overridden && !(source.id in instructionEdits)} onClick={event => { event.preventDefault(); updateInstructionPath(source.id, null); }}><RefreshIcon className="w-4 h-4" /></button>
                  </>} />
                {source.kind === "files" && source.patterns.map(filename => (
                  <ConfigField key={filename} icon={<DocumentIcon className="w-4 h-4" />} label={filename}
                    value={value.trim() ? `${value.trim().replace(/\/+$/, "")}/${filename}` : ""}
                    readOnly openKind="file" />
                ))}
              </div>;
            })}
          {instructionPaths && <p className="arca-agent-instruction-hint">{t("agentConfiguration.instructionsPathHint")}</p>}
          {instructionError && <ToastNotice message={instructionError} action={<button type="button" className="btn-ghost" onClick={() => setInstructionRequest(value => value + 1)}>{t("common.retry")}</button>} />}
        </section>}
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
      {error && <ToastNotice message={t("agentConfiguration.saveFailed", { error })} />}
    </DialogShell>
  );
}

function ConfigField({
  icon,
  label,
  value,
  disabled = false,
  readOnly = false,
  onChange,
  action,
  openKind,
  hint,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  disabled?: boolean;
  readOnly?: boolean;
  onChange?(value: string): void;
  action?: ReactNode;
  openKind?: "file" | "folder";
  hint?: string;
}) {
  const toast = useToast();
  const { t } = useTranslation();
  const hintId = useId();
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
        aria-label={label}
        value={value}
        disabled={disabled}
        readOnly={readOnly}
        spellCheck={false}
        aria-describedby={hint ? hintId : undefined}
        onChange={(event) => onChange?.(event.target.value)}
      />
      {(openKind || action) && <span className="arca-agent-config-field-actions">
        {openKind && <button type="button" className="arca-agent-config-remove" disabled={disabled || !value.trim()}
          title={t(openKind === "file" ? "agentConfiguration.openEditor" : "agentConfiguration.openFolder")} aria-label={t("agentConfiguration.openLabel", { label })}
          onClick={(event) => { event.preventDefault(); void openLocation(); }}><ExternalLinkIcon className="w-4 h-4" /></button>}
        {action}
      </span>}
      </span>
      {hint && <small className="arca-agent-path-patterns" id={hintId}>{hint}</small>}
    </label>
  );
}
