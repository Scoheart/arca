import { RESOURCE_PRESENTATION } from "./resourcePresentation";
import type { AssetCommandError, AssetOperationPlan, AssetRef, ConvergenceAction } from "../lib/types";
import { assetIdentity } from "../lib/consumption";
import { AgentGlyph, agentName as resolveAgentName } from "./brandIcons";
import { TrashIcon } from "./icons";
import { DialogShell } from "./DialogShell";
import { AssetSyncReviewDialog } from "./AssetSyncReviewDialog";
import { DialogDisclosure } from "./DialogDisclosure";

function assetKey(asset: AssetRef) {
  return `${asset.domain}:${assetIdentity(asset)}`;
}

function readableIdentity(value: string) {
  return value
    .replace(/::(?:stdio|http)$/, "")
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toLocaleUpperCase() + part.slice(1))
    .join(" ");
}

function displayAssetName(asset: AssetRef, names: Record<string, string>) {
  return names[assetKey(asset)] ?? readableIdentity(assetIdentity(asset));
}

function displayAgentName(
  id: string,
  currentId: string | undefined,
  currentName: string | undefined,
  names: Record<string, string>,
) {
  if (id === currentId && currentName) return currentName;
  return names[id] ?? resolveAgentName(id);
}

function assetLabel(asset: AssetRef, names: Record<string, string>) {
  const domain = asset.domain === "model-provider"
    ? "Provider" : RESOURCE_PRESENTATION[asset.domain].label;
  return `${domain} · ${displayAssetName(asset, names)}`;
}

export function assetReviewErrorMessage(
  error: AssetCommandError | string,
  plan: AssetOperationPlan,
) {
  const code = typeof error === "string" ? "" : error.code;
  const message = typeof error === "string" ? error : error.message;
  const domain = plan.domain_plan.domain;
  const normalized = `${code} ${message}`.toLocaleLowerCase();
  if (
    domain === "skill"
    && /only an exact managed skill link can be disabled/i.test(message)
  ) {
    const target = plan.target_files.find((path) => path.trim().length > 0) ?? "路径未知";
    return `无法移除：这不是可安全移除的托管 Skill 链接（${target}）。`;
  }
  if (
    code === "model_consumption_missing"
    || /(?:requested )?model.*(?:not assigned|not added)/i.test(message)
  ) {
    return "该 Model 未添加到此 Agent，无法移除。";
  }
  if (
    code === "mcp_consumption_missing"
    || /mcp.*(?:not assigned|not added)/i.test(message)
  ) {
    return "该 MCP 未分配给此 Agent，无法移除。";
  }
  if (
    code === "skill_consumption_missing"
    || /skill.*(?:not assigned|not added)/i.test(message)
  ) {
    return "该 Skill 未分配给此 Agent，无法移除。";
  }
  const hasRemove = plan.relationship_changes.some((change) => change.action === "remove");
  if (
    hasRemove
    && (
      normalized.includes("conflict")
      || normalized.includes("not assigned")
      || normalized.includes("not added")
      || /^[\[{].*[\]}]$/s.test(message.trim())
    )
  ) {
    const asset = domain === "agent-capabilities" ? "配置" : RESOURCE_PRESENTATION[domain].label;
    return `当前 ${asset} 状态已变化，无法按此计划移除。请关闭后刷新再试。`;
  }
  return message;
}

function agentActionCopy(plan: AssetOperationPlan) {
  const domain = plan.domain_plan.domain;
  const asset = domain === "agent-capabilities" ? "配置" : RESOURCE_PRESENTATION[domain].label;
  const hasAdd = plan.relationship_changes.some((change) => change.action === "add");
  const hasRemove = plan.relationship_changes.some((change) => change.action === "remove");
  const stateChanges = plan.consumption_state_changes ?? [];
  if (domain === "model") {
    const states = plan.model_state_changes;
    if (states.some((change) => change.reason === "model_activated") && !hasAdd && !hasRemove) {
      return { title: "确认切换当前 Model", commit: "切换当前 Model", busy: "切换中…" };
    }
    if (hasAdd && !hasRemove) {
      const addsCurrent = states.some(
        (change) => change.reason === "model_added" && change.after.active,
      );
      return addsCurrent
        ? { title: "确认添加当前 Model", commit: "添加 Model", busy: "添加中…" }
        : { title: "确认添加备选 Model", commit: "添加备选 Model", busy: "添加中…" };
    }
    if (states.some((change) => change.reason === "model_disabled")) {
      return { title: "确认停用 Model", commit: "停用 Model", busy: "停用中…" };
    }
    if (states.some((change) => change.reason === "model_enabled")) {
      return { title: "确认启用 Model", commit: "启用 Model", busy: "启用中…" };
    }
  }
  if (!hasAdd && !hasRemove && stateChanges.length > 0) {
    if (stateChanges.every((change) => change.after_enabled)) {
      return { title: `确认启用 ${asset}`, commit: `启用 ${asset}`, busy: "启用中…" };
    }
    if (stateChanges.every((change) => !change.after_enabled)) {
      return { title: `确认停用 ${asset}`, commit: `停用 ${asset}`, busy: "停用中…" };
    }
  }
  if (hasAdd && !hasRemove) {
    return { title: `确认添加 ${asset}`, commit: `添加 ${asset}`, busy: "添加中…" };
  }
  if (hasRemove && !hasAdd) {
    return { title: `确认移除 ${asset}`, commit: `移除 ${asset}`, busy: "移除中…" };
  }
  return { title: `确认更新 ${asset}`, commit: `更新 ${asset}`, busy: "更新中…" };
}

function warningCopy(warning: string) {
  const separator = warning.lastIndexOf(":");
  const agent = warning.slice(0, separator).split(" / ")[0];
  const reason = warning.slice(separator + 1).trim();
  const labels: Record<string, string> = {
    mcp_config_drift: "MCP 配置与 Arca 不一致",
    mcp_enabled_state_drift: "MCP 启用状态与 Arca 不一致",
    mcp_target_missing: "Agent 中缺少这份 MCP 配置",
    model_active_state_drift: "当前 Model 与 Arca 记录不一致，请先刷新或重新选择当前 Model",
    model_external_current: "当前 Model 由 Agent 外部配置管理，切换前需要先让 Arca 接管",
    model_active_conflicted: "当前 Model 配置存在歧义，请先修复配置",
    model_owned_fields_drift: "该 Model 的托管字段已被外部修改",
    model_target_missing: "该 Model 的 Agent 配置缺失",
    model_credential_export_plaintext:
      `将把所选 Provider 的 API Key 写入 ${resolveAgentName("claude-desktop")} 的私有配置文件（权限 0600）`,
  };
  return labels[reason] ? `${resolveAgentName(agent)}：${labels[reason]}` : warning;
}

function modelStateLabel(state: { added: boolean; enabled: boolean; active: boolean }) {
  if (!state.added) return "未添加";
  if (!state.enabled) return "已添加 · 已停用";
  if (state.active) return "已启用 · 当前";
  return "已启用 · 非当前";
}

function enabledStateLabel(enabled: boolean) {
  return enabled ? "已启用" : "已停用";
}

function affectedAgentsLabel(
  agentIds: string[],
  currentId: string | undefined,
  currentName: string | undefined,
  names: Record<string, string>,
) {
  return agentIds
    .map((id) => displayAgentName(id, currentId, currentName, names))
    .join("、");
}

interface ConfigurationValues {
  mcpPath: string | null;
  mcpKey: string | null;
  modelPaths: string[];
  skillsGlobalDir: string | null;
  skillsAliasDirs: string[];
}

function configurationPlanValues(plan: AssetOperationPlan): {
  agentId: string;
  before: ConfigurationValues;
  after: ConfigurationValues;
  migratedSkillNames: string[];
} | null {
  const domainPlan = plan.domain_plan;
  if (domainPlan.domain === "agent-capabilities") {
    const project = (value: typeof domainPlan.before): ConfigurationValues => ({
      mcpPath: value.mcp?.path ?? null,
      mcpKey: value.mcp?.key ?? null,
      modelPaths: value.model?.paths ?? [],
      skillsGlobalDir: value.skill?.global_dir ?? null,
      skillsAliasDirs: value.skill?.alias_dirs ?? [],
    });
    return {
      agentId: domainPlan.agent_id,
      before: project(domainPlan.before),
      after: project(domainPlan.after),
      migratedSkillNames: domainPlan.migrated_skill_names,
    };
  }
  return null;
}

function configurationChanges(plan: AssetOperationPlan) {
  const configuration = configurationPlanValues(plan);
  if (!configuration) return [];
  const { before, after } = configuration;
  const rows: Array<{ label: string; before: string; after: string }> = [];
  const beforeModels = before.modelPaths.join(" · ");
  const afterModels = after.modelPaths.join(" · ");
  if (beforeModels !== afterModels) {
    rows.push({ label: RESOURCE_PRESENTATION.model.label, before: beforeModels, after: afterModels });
  }
  if (before.mcpPath !== after.mcpPath) {
    rows.push({
      label: `${RESOURCE_PRESENTATION.mcp.label} 配置文件`,
      before: before.mcpPath ?? "未接入",
      after: after.mcpPath ?? "未接入",
    });
  }
  const beforeMcpKey = before.mcpKey ?? "";
  const afterMcpKey = after.mcpKey ?? "";
  if (beforeMcpKey !== afterMcpKey) {
    rows.push({ label: `${RESOURCE_PRESENTATION.mcp.label} 配置键`, before: beforeMcpKey, after: afterMcpKey });
  }
  const beforeSkills = [before.skillsGlobalDir, ...before.skillsAliasDirs]
    .filter(Boolean).join(" · ");
  const afterSkills = [after.skillsGlobalDir, ...after.skillsAliasDirs]
    .filter(Boolean).join(" · ");
  if (beforeSkills !== afterSkills) {
    rows.push({
      label: RESOURCE_PRESENTATION.skill.label,
      before: beforeSkills || "未接入",
      after: afterSkills || "未接入",
    });
  }
  return rows;
}

export function AssetOperationReviewDialog({
  plan,
  busy,
  error,
  agentId,
  agentName,
  convergenceAction,
  agentDisplayNames = {},
  assetDisplayNames = {},
  cancelLabel = "取消",
  onCommit,
  onCancel,
}: {
  plan: AssetOperationPlan;
  busy: boolean;
  error?: AssetCommandError | string | null;
  agentId?: string;
  agentName?: string;
  convergenceAction?: ConvergenceAction;
  agentDisplayNames?: Record<string, string>;
  assetDisplayNames?: Record<string, string>;
  cancelLabel?: string;
  onCommit(): Promise<unknown> | unknown;
  onCancel(): Promise<unknown> | unknown;
}) {
  if (plan.kind === "clear-mcp" || plan.kind === "clear-models") return null;
  const isConfiguration = plan.kind === "update-configuration";
  const isAgentSkillPlan = Boolean(
    agentId && agentName && plan.kind === "set-consumption" && plan.domain_plan.domain === "skill",
  );
  const compatibleAgentCount = isAgentSkillPlan
    ? plan.affected_agent_ids.filter((id) => id !== agentId).length
    : 0;
  const configurationPlan = configurationPlanValues(plan);
  const skillsPathChanged = configurationPlan !== null
    && (configurationPlan.before.skillsGlobalDir !== configurationPlan.after.skillsGlobalDir
      || JSON.stringify(configurationPlan.before.skillsAliasDirs)
        !== JSON.stringify(configurationPlan.after.skillsAliasDirs));
  const sharedConfigurationReaderCount = isConfiguration
    && configurationPlan
    && skillsPathChanged
    ? plan.affected_agent_ids.filter((id) => id !== configurationPlan.agentId).length
    : 0;
  const configChanges = configurationChanges(plan);
  const mcpLocationChanged = configurationPlan !== null
    && (configurationPlan.before.mcpPath !== configurationPlan.after.mcpPath
      || configurationPlan.before.mcpKey !== configurationPlan.after.mcpKey);
  const modelLocationChanged = configurationPlan !== null
    && (configurationPlan.before.modelPaths.length !== configurationPlan.after.modelPaths.length
      || configurationPlan.before.modelPaths.some(
        (path, index) => path !== configurationPlan.after.modelPaths[index],
      ));
  const consumptionStateChanges = plan.consumption_state_changes ?? [];
  const genericConsumptionStateChanges = consumptionStateChanges.filter(
    (change) => change.asset.domain !== "model",
  );
  const hasAdd = plan.relationship_changes.some((change) => change.action === "add");
  const hasRemove = plan.relationship_changes.some((change) => change.action === "remove");
  const isRemoveOnly = hasRemove && !hasAdd;
  const isDanger = isRemoveOnly || plan.kind === "delete-asset";
  const reviewError = error ? assetReviewErrorMessage(error, plan) : null;
  const agentCopy = agentName && plan.kind === "set-consumption"
    ? agentActionCopy(plan)
    : null;
  const title = isConfiguration ? "确认修改配置" : agentCopy?.title ?? (plan.kind === "update-asset"
    ? "确认更改"
    : plan.kind === "delete-asset"
      ? "确认删除"
      : "确认更新配置");
  const commitLabel = isConfiguration ? "保存配置" : agentCopy?.commit ?? (plan.kind === "delete-asset" ? "删除" : "应用更改");
  const subtitle = agentName
    ? compatibleAgentCount > 0
      ? `${agentName} · 同一目录也被 ${compatibleAgentCount} 个 Agent 读取`
      : sharedConfigurationReaderCount > 0
        ? `${agentName} · Skills 目录变更涉及 ${sharedConfigurationReaderCount} 个其他 Agent`
      : !isConfiguration && plan.affected_agent_ids.length > 1
      ? `${agentName} · 另影响 ${plan.affected_agent_ids.length - 1} 个 Agent`
      : agentName
    : `${plan.affected_agent_ids.length} 个 Agent · ${plan.target_files.length} 个目标`;
  // Only the explicit restore action uses this copy; an ordinary central edit
  // can have the same before/after selections and must not be called a resync.
  if (convergenceAction === "restore-desired"
    && plan.kind === "update-asset" && plan.domain_plan.domain === "mcp"
    && plan.central_changes.length > 0
    && plan.central_changes.every((change) => change.asset.domain === "mcp" && change.action === "update")
    && plan.relationship_changes.length === 0
    && consumptionStateChanges.length === 0 && plan.model_state_changes.length === 0) {
    return <AssetSyncReviewDialog
      plan={plan} busy={busy} error={reviewError} cancelLabel={cancelLabel}
      assets={plan.central_changes.map((change) => ({
        key: assetKey(change.asset), name: displayAssetName(change.asset, assetDisplayNames),
      }))}
      agents={[...new Set(plan.affected_agent_ids)].map((id) => ({
        id, name: displayAgentName(id, agentId, agentName, agentDisplayNames),
      }))}
      onCommit={onCommit} onCancel={onCancel}
    />;
  }
  const providerChange = plan.central_changes.find((change) => change.asset.domain === "model-provider" && change.action === "update");
  if (plan.kind === "delete-asset" && plan.domain_plan.domain === "mcp") {
    const deletedAssets = plan.central_changes.filter((change) => change.asset.domain === "mcp");
    const affectedAgents = [...new Set(plan.affected_agent_ids)];
    return (
      <DialogShell
        kind="review"
        size="sm"
        className="arca-mcp-delete-dialog"
        title="删除 MCPs"
        leading={<span className="arca-dialog-shell-glyph" aria-hidden="true"><TrashIcon /></span>}
        busy={busy}
        onClose={() => void onCancel()}
        footerEnd={<>
          <button type="button" className="btn-secondary" disabled={busy} onClick={() => void onCancel()}>{cancelLabel}</button>
          <button type="button" className="btn-danger-solid" disabled={busy || !plan.can_commit} onClick={() => void onCommit()}>
            {busy ? "删除中…" : reviewError ? "重试删除" : "删除"}
          </button>
        </>}
      >
        <div className="arca-mcp-delete-content">
          {deletedAssets.map((change) => (
            <strong className="arca-mcp-delete-name" key={assetKey(change.asset)}>
              {displayAssetName(change.asset, assetDisplayNames)}
            </strong>
          ))}
          <p className="arca-mcp-delete-caption">
            {deletedAssets.flatMap((change) => change.summary.slice(1, 2)).join("；")
              || (affectedAgents.length ? `更新 ${affectedAgents.length} 个 Agent 的 MCP 配置` : "删除这份 MCP 配置")}
          </p>
          {affectedAgents.length > 0 && <div className="arca-provider-update-agents">
            {affectedAgents.map((id) => {
              const label = displayAgentName(id, agentId, agentName, agentDisplayNames);
              return <span key={id} title={label} role="img" aria-label={label} tabIndex={0}>
                <AgentGlyph id={id} name={label} size={32} />
              </span>;
            })}
          </div>}
          {reviewError && <p role="alert" className="arca-review-error">{reviewError}</p>}
          {!plan.can_commit && <p role="alert" className="arca-review-error">暂时无法删除，请查看原因。</p>}
          {plan.warnings.length > 0 && <section className="arca-provider-update-issues">
            {plan.warnings.map((warning) => <p key={warning}>{warningCopy(warning)}</p>)}
          </section>}
        </div>
      </DialogShell>
    );
  }
  if (providerChange) {
    const name = displayAssetName(providerChange.asset, assetDisplayNames);
    return <DialogShell kind="review" size="sm" title={`更新 ${name}`}
      subtitle={plan.affected_agent_ids.length ? `同步到 ${plan.affected_agent_ids.length} 个 Agent` : "保存 Provider 设置"}
      busy={busy} onClose={() => void onCancel()}
      footerEnd={<><button type="button" className="btn-secondary" disabled={busy} onClick={() => void onCancel()}>取消</button>
        <button type="button" className="btn-primary" disabled={busy || !plan.can_commit} onClick={() => void onCommit()}>{busy ? "更新中…" : "保存并同步"}</button></>}>
      <div className="arca-provider-update-summary">
        {plan.affected_agent_ids.length > 0 && <div className="arca-provider-update-agents">
          {plan.affected_agent_ids.map((id) => {
            const label = displayAgentName(id, agentId, agentName, agentDisplayNames);
            return <span key={id} title={label} role="img" aria-label={label} tabIndex={0}>
              <AgentGlyph id={id} name={label} size={36} />
            </span>;
          })}
        </div>}
        {reviewError && <p role="alert" className="arca-review-error">{reviewError}</p>}
        {!plan.can_commit && <p role="alert" className="arca-review-error">部分配置暂时无法更新，请查看原因。</p>}
        {plan.warnings.length > 0 && <section className="arca-provider-update-issues">
          {plan.warnings.map((warning) => <p key={warning}>{warningCopy(warning)}</p>)}
        </section>}
      </div>
    </DialogShell>;
  }
  return (
    <DialogShell
      kind="review"
      size="md"
      title={title}
      subtitle={subtitle}
      busy={busy}
      onClose={() => void onCancel()}
      status={!plan.can_commit ? (
        <div className="arca-review-error" role="alert">存在冲突，暂不可继续。</div>
      ) : reviewError ? (
        <div className="arca-asset-review-error" role="alert">
          <strong>操作未完成</strong>
          <span>{reviewError}</span>
        </div>
      ) : undefined}
      footerEnd={(
        <>
          <button type="button" className="btn-ghost" disabled={busy} onClick={() => void onCancel()}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={isDanger ? "btn-danger" : "btn-primary"}
            disabled={busy || !plan.can_commit}
            onClick={() => void onCommit()}
          >
            {!busy && isDanger && <TrashIcon className="w-4 h-4" />}
            {busy
              ? (isConfiguration ? "保存中…" : agentCopy?.busy ?? "处理中…")
              : reviewError && isRemoveOnly
                ? `重试${commitLabel}`
                : commitLabel}
          </button>
        </>
      )}
    >
      <div className="arca-review-content arca-asset-review">
        {isRemoveOnly && (
          <section className="arca-asset-review-summary">
            <h3>影响摘要</h3>
            {plan.relationship_changes
              .filter((change) => change.action === "remove")
              .map((change, index) => (
                <p key={`${change.agent_id}:${assetIdentity(change.asset)}:${index}`}>
                  {isAgentSkillPlan && change.agent_id !== agentId ? (
                    <>
                      共用目录将同步影响
                      <strong>
                        {displayAgentName(
                          change.agent_id,
                          agentId,
                          agentName,
                          agentDisplayNames,
                        )}
                      </strong>
                      的 Skill 可见性。
                    </>
                  ) : (
                    <>
                      将从
                      <strong>
                        {displayAgentName(
                          change.agent_id,
                          agentId,
                          agentName,
                          agentDisplayNames,
                        )}
                      </strong>
                      移除 {assetLabel(change.asset, assetDisplayNames)}。
                    </>
                  )}
                </p>
              ))}
          </section>
        )}
        {isConfiguration && configChanges.length > 0 && (
          <section className="arca-config-review">
            <h3>配置位置</h3>
            <ul>
              {configChanges.map((change) => (
                <li key={change.label}>
                  <strong>{change.label}</strong>
                  <code>{change.before}</code>
                  <span>→</span>
                  <code>{change.after}</code>
                </li>
              ))}
            </ul>
            {mcpLocationChanged && (
              <p className="arca-asset-review-note">
                只更新 Arca 后续使用的 MCP 配置位置；旧文件不会删除，现有 MCP 配置不会复制到新位置。
              </p>
            )}
            {modelLocationChanged && (
              <p className="arca-asset-review-note">
                只更新 Arca 后续使用的 Model 配置位置；旧文件不会删除，现有 Model 配置不会复制到新位置。
              </p>
            )}
          </section>
        )}
        {plan.central_changes.length > 0 && (
          <section>
            <h3>资源变化</h3>
            <ul>
              {plan.central_changes.map((change) => (
                <li key={`${change.action}:${assetIdentity(change.asset)}`}>
                  <span data-action={change.action}>
                    {change.action === "create" ? "创建" : change.action === "update" ? "更新" : "删除"}
                  </span>
                  <code>{assetLabel(change.asset, assetDisplayNames)}</code>
                  {change.summary.length > 0 && (
                    <span className="arca-central-change-summary">
                      {change.summary.map((item, index) => (
                        <small key={`${index}:${item}`}>{item}</small>
                      ))}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          </section>
        )}
        {plan.model_state_changes.length > 0 && (
          <section>
            <h3>Models 状态变化</h3>
            <ul>
              {plan.model_state_changes.map((change) => (
                <li key={`${change.agent_id}:${change.profile_id}`}>
                  <strong>
                    {displayAgentName(change.agent_id, agentId, agentName, agentDisplayNames)}
                  </strong>
                  <code>
                    {displayAssetName(
                      { domain: "model", profile_id: change.profile_id },
                      assetDisplayNames,
                    )}
                  </code>
                  <small>
                    {modelStateLabel(change.before)} → {modelStateLabel(change.after)}
                    {change.fallback_profile_id ? `；回退到 ${change.fallback_profile_id}` : ""}
                  </small>
                </li>
              ))}
            </ul>
          </section>
        )}
        {genericConsumptionStateChanges.length > 0 && (
          <section>
            <h3>启用状态变化</h3>
            <ul>
              {genericConsumptionStateChanges.map((change, index) => {
                const affectedAgentIds = change.affected_agent_ids.length > 0
                  ? change.affected_agent_ids
                  : [change.agent_id];
                return (
                  <li
                    className="arca-asset-review-relationship"
                    key={`${change.agent_id}:${assetIdentity(change.asset)}:${index}`}
                  >
                    <span data-action={change.after_enabled ? "enable" : "disable"}>
                      {change.after_enabled ? "启用" : "停用"}
                    </span>
                    <span className="arca-asset-review-relationship-copy">
                      <strong>{assetLabel(change.asset, assetDisplayNames)}</strong>
                      <small>
                        {displayAgentName(
                          change.agent_id,
                          agentId,
                          agentName,
                          agentDisplayNames,
                        )}：{enabledStateLabel(change.before_enabled)} → {enabledStateLabel(change.after_enabled)}
                      </small>
                      {affectedAgentIds.length > 1 && (
                        <small>
                          同步影响 {affectedAgentsLabel(
                            affectedAgentIds,
                            agentId,
                            agentName,
                            agentDisplayNames,
                          )}
                        </small>
                      )}
                      {change.target && <small>共享目标 {change.target.global_dir}</small>}
                    </span>
                  </li>
                );
              })}
            </ul>
          </section>
        )}
        {plan.relationship_changes.length > 0 && <section>
          <h3>{isConfiguration ? "Skills 影响" : isAgentSkillPlan ? "生效范围" : agentName ? "Agent 变更" : "关系变化"}</h3>
          {isAgentSkillPlan && compatibleAgentCount > 0 && (
            <p className="arca-asset-review-note">
              只写入一个目录；兼容 Agent 会读取同一份 Skill，不会重复安装。
            </p>
          )}
          {(
            <ul className={isAgentSkillPlan ? "arca-skill-impact-list" : undefined}>
              {plan.relationship_changes.map((change, index) => {
                const isDirect = !isAgentSkillPlan || change.agent_id === agentId;
                const action = change.action === "add"
                  ? isDirect ? "直接添加" : "兼容可见"
                  : isDirect ? "直接移除" : "同步不可见";
                return (
                <li
                  className="arca-asset-review-relationship"
                  key={`${change.agent_id}:${assetIdentity(change.asset)}:${index}`}
                >
                  <span data-action={change.action} data-impact={isDirect ? "direct" : "compatible"}>
                    {isAgentSkillPlan ? action : change.action === "add" ? "添加" : "移除"}
                  </span>
                  <span className="arca-asset-review-relationship-copy">
                    <strong>
                      {displayAgentName(change.agent_id, agentId, agentName, agentDisplayNames)}
                    </strong>
                    <small>{assetLabel(change.asset, assetDisplayNames)}</small>
                  </span>
                </li>
                );
              })}
            </ul>
          )}
        </section>}
        {configurationPlan
          && configurationPlan.migratedSkillNames.length > 0 && (
            <section>
              <h3>迁移 Skills</h3>
              <p>{configurationPlan.migratedSkillNames.join("、")}</p>
            </section>
          )}
        {plan.target_files.length > 0 && (
          <DialogDisclosure title="写入位置" summary={`${plan.target_files.length} 个文件`}>
            <ul>{plan.target_files.map((path) => <li key={path}><code>{path}</code></li>)}</ul>
          </DialogDisclosure>
        )}
        {plan.warnings.length > 0 && (
          <section className="arca-asset-review-warnings">
            <h3>需要处理</h3>
            <ul>{plan.warnings.map((warning) => <li key={warning}>{warningCopy(warning)}</li>)}</ul>
          </section>
        )}
      </div>
    </DialogShell>
  );
}
