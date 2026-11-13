import { ResourceIcon } from "./resourcePresentation";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { AssetRef, ConfiguredMcpObservation, ConsumptionView, ConvergenceAction, HostMcpObservation, NativeFileSkillObservation, NativeSkillObservation } from "../lib/types";
import { ConfiguredMcpCard, HostMcpCard, NativeFileSkillCard, NativeSkillCard } from "./NativePluginsPanel";
import { assetIdentity } from "../lib/consumption";
import {
  DownloadIcon,
  LinkOffIcon,
  MoreHorizontalIcon,
  PlusIcon,
  RefreshIcon,
  TrashIcon,
} from "./icons";
import { ConsumptionStatus } from "./ConsumptionStatus";
import { SearchBar, Switch } from "./ui";
import { useTranslation } from "react-i18next";
import { AgentResourceActions } from "./AgentResourcePanel";

export interface ConsumptionAssetPresentation {
  name: string;
  description?: string;
  icon?: ReactNode;
  meta?: ReactNode;
}

function rowKey(item: ConsumptionView, external: boolean) {
  // One Skill can be observed at several physical targets for the same Agent.
  return JSON.stringify([item.agent_id, item.asset.domain, assetIdentity(item.asset), external,
    item.target?.target_id, item.target?.global_dir, item.observation_id]);
}

function ConvergenceActionIcon({ action }: { action: ConvergenceAction }) {
  if (action === "adopt-observed") return <DownloadIcon className="w-3.5 h-3.5" />;
  if (action === "restore-desired") return <RefreshIcon className="w-3.5 h-3.5" />;
  return <LinkOffIcon className="w-3.5 h-3.5" />;
}

function ConvergenceActions({
  name,
  actions,
  disabled,
  onAction,
}: {
  name: string;
  actions: ConvergenceAction[];
  disabled: boolean;
  onAction(action: ConvergenceAction): void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLSpanElement>(null);
  const label = (action: ConvergenceAction) => action === "adopt-observed"
    ? t("observations.actions.adopt")
    : action === "restore-desired"
      ? t("observations.actions.restore")
      : t("observations.actions.detach");
  const primary = actions.includes("restore-desired")
    ? "restore-desired"
    : actions.includes("adopt-observed")
      ? "adopt-observed"
      : actions[0];
  const secondary = actions.filter((action) => action !== primary);

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  if (!primary) return null;
  return (
    <span className="arca-convergence-controls" ref={rootRef}>
      <button
        type="button"
        className="arca-convergence-action"
        data-primary="true"
        data-action={primary}
        aria-label={`${label(primary)}：${name}`}
        title={label(primary)}
        disabled={disabled}
        onClick={() => onAction(primary)}
      >
        <ConvergenceActionIcon action={primary} />
      </button>
      {secondary.length > 0 && (
        <span className="arca-convergence-more">
          <button
            type="button"
            className="arca-convergence-action"
            aria-label={`${t("observations.actions.more")}：${name}`}
            title={t("observations.actions.more")}
            aria-haspopup="menu"
            aria-expanded={open}
            disabled={disabled}
            onClick={() => setOpen((current) => !current)}
          >
            <MoreHorizontalIcon className="w-4 h-4" />
          </button>
          {open && (
            <span className="arca-convergence-menu" role="menu">
              {secondary.map((action) => (
                <button
                  key={action}
                  type="button"
                  role="menuitem"
                  data-action={action}
                  aria-label={`${label(action)}：${name}`}
                  onClick={() => {
                    setOpen(false);
                    onAction(action);
                  }}
                >
                  <ConvergenceActionIcon action={action} />
                  <span>{label(action)}</span>
                </button>
              ))}
            </span>
          )}
        </span>
      )}
    </span>
  );
}

export function AgentConsumptionPanel({
  domain,
  title,
  description,
  manageLabel,
  rows,
  external,
  hostMcps = [],
  configuredMcps = [],
  nativeSkills = [],
  fileSkills = [],
  externalMode = "summary",
  externalTitle = "外部配置",
  externalDescription = "尚未由 Arca 管理",
  present,
  onManage,
  manageIcon = <PlusIcon className="w-3.5 h-3.5" />,
  onOpenAsset,
  onInspect,
  onEnabledChange,
  enabledChangeDisabled,
  toggleKind = "enabled",
  renderAction,
  onRemove,
  onConverge,
  convergenceDisabled = false,
  removeLabel,
  manageDisabled = false,
  bulkToggleLabel,
  bulkEnabled,
  bulkToggleDisabled = false,
  onBulkEnabledChange,
  bulkRemoveLabel,
  bulkRemoveTitle,
  bulkRemoveDisabled = false,
  onBulkRemove,
  removeDisabled = false,
  emptyTitle = "还没有添加资产",
  emptyDescription,
  emptyAction,
  columns = 2,
}: {
  domain: AssetRef["domain"];
  title: string;
  description?: ReactNode;
  manageLabel: string;
  rows: ConsumptionView[];
  external: ConsumptionView[];
  hostMcps?: HostMcpObservation[];
  configuredMcps?: ConfiguredMcpObservation[];
  nativeSkills?: NativeSkillObservation[];
  fileSkills?: NativeFileSkillObservation[];
  externalMode?: "summary" | "cards";
  externalTitle?: string;
  externalDescription?: string;
  present(asset: AssetRef): ConsumptionAssetPresentation;
  onManage(): void;
  manageIcon?: ReactNode;
  onOpenAsset?(asset: AssetRef): void;
  onInspect?(item: ConsumptionView): void;
  onEnabledChange?(item: ConsumptionView, enabled: boolean): void;
  enabledChangeDisabled?: boolean | ((item: ConsumptionView) => boolean);
  toggleKind?: "enabled" | "current";
  renderAction?(item: ConsumptionView): ReactNode;
  onRemove?(asset: AssetRef): void;
  onConverge?(item: ConsumptionView, action: ConvergenceAction): void;
  convergenceDisabled?: boolean;
  removeLabel?(name: string): string;
  manageDisabled?: boolean;
  bulkToggleLabel?: string;
  bulkEnabled?: boolean;
  bulkToggleDisabled?: boolean;
  onBulkEnabledChange?(enabled: boolean): void;
  bulkRemoveLabel?: string;
  bulkRemoveTitle?: string;
  bulkRemoveDisabled?: boolean;
  onBulkRemove?(): void;
  removeDisabled?: boolean;
  emptyTitle?: string;
  emptyDescription?: string;
  emptyAction?: ReactNode;
  columns?: 2 | 3;
}) {
  const [query, setQuery] = useState("");
  useEffect(() => setQuery(""), [domain]);
  const domainRows = rows.filter((item) => item.asset.domain === domain);
  const domainExternal = external.filter((item) => item.asset.domain === domain);
  const items = [
    ...domainRows.map((item) => ({ item, external: false })),
    ...(externalMode === "cards" ? domainExternal.map((item) => ({ item, external: true })) : []),
  ];

  const needle = query.trim().toLocaleLowerCase();
  const hosts = domain === "mcp" ? hostMcps : [];
  const declarations = domain === "mcp" ? configuredMcps : [];
  const skills = domain === "skill" ? nativeSkills : [];
  const skillFiles = domain === "skill" ? fileSkills : [];
  const filteredHosts = hosts.filter(item => !needle || `${item.name} ${item.server_id} ${item.provider}`.toLocaleLowerCase().includes(needle));
  const filteredDeclarations = declarations.filter(item => !needle || `${item.name} ${item.source_path}`.toLocaleLowerCase().includes(needle));
  const filteredSkills = skills.filter(item => !needle || `${item.name} ${item.plugin_name} ${item.plugin_id}`.toLocaleLowerCase().includes(needle));
  const filteredSkillFiles = skillFiles.filter(item => !needle || `${item.name} ${item.description} ${item.source_path}`.toLocaleLowerCase().includes(needle));
  const total = items.length + hosts.length + declarations.length + skills.length + skillFiles.length;
  const visible = filteredHosts.length + filteredDeclarations.length + filteredSkills.length + filteredSkillFiles.length;
  const filtered = items.filter(({ item }) => {
    const presentation = present(item.asset);
    return !needle || `${presentation.name} ${presentation.description ?? ""} ${assetIdentity(item.asset)}`.toLocaleLowerCase().includes(needle);
  });

  return (
    <section className="arca-agent-section arca-agent-resource-content arca-consumption-panel" aria-label={title}>
      <div className="arca-agent-section-head">
        <div>
          <h3>{title}</h3>
          {description && domain !== "model" && <p>{description}</p>}
        </div>
        <AgentResourceActions><div className="arca-agent-section-actions">
          {total > 0 && (
            <div className="arca-consumption-search">
              <SearchBar value={query} onChange={setQuery} placeholder={`搜索当前 ${title}`} />
              <span role="status" aria-live="polite">{query ? `${filtered.length + visible} / ${total} 项` : `${total} 项`}</span>
            </div>
          )}
          {domainRows.length > 0 && bulkToggleLabel && bulkEnabled !== undefined && onBulkEnabledChange && (
            <div className="arca-agent-bulk-toggle">
              <span>{bulkToggleLabel}</span>
              <Switch
                checked={bulkEnabled}
                compact
                disabled={bulkToggleDisabled}
                ariaLabel={bulkEnabled ? `停用全部 ${title}` : `启用全部 ${title}`}
                title={bulkEnabled ? `停用全部 ${title}` : `启用全部 ${title}`}
                onChange={onBulkEnabledChange}
              />
            </div>
          )}
          {items.length > 0 && bulkRemoveLabel && onBulkRemove && (
            <button
              type="button"
              className="btn-danger"
              disabled={bulkRemoveDisabled}
              title={bulkRemoveTitle ?? bulkRemoveLabel}
              aria-label={bulkRemoveLabel}
              onClick={onBulkRemove}
            >
              <TrashIcon className="w-3.5 h-3.5" />
              <span className="arca-consumption-bulk-label">{bulkRemoveLabel}</span>
            </button>
          )}
          <button
            type="button"
            className="btn-primary"
            aria-label={manageLabel}
            disabled={manageDisabled}
            onClick={onManage}
          >
            {manageIcon}
            <span className="arca-consumption-manage-label">{manageLabel}</span>
          </button>
        </div></AgentResourceActions>
      </div>
      {domain === "model" && description && <p className="arca-consumption-model-note">{description}</p>}
      {externalMode === "summary" && domainExternal.length > 0 && (
        <div className="arca-consumption-external" role="status">
          <div>
            <strong>{externalTitle} {domainExternal.length}</strong>
            <span>{externalDescription}</span>
          </div>
          <ul>
            {domainExternal.slice(0, 3).map((item) => {
              const shared = item.asset.domain === "skill" && item.affected_agent_ids.length > 1;
              return (
                <li key={rowKey(item, true)}>
                  {present(item.asset).name}
                  {shared && <small>外部 · 共用 {item.affected_agent_ids.length}</small>}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {total === 0 ? (
        <div className="arca-consumption-empty">
          <ResourceIcon domain={domain === "model-provider" ? "model" : domain} className="w-7 h-7" />
          <strong>{emptyTitle}</strong>
          {emptyDescription && <span>{emptyDescription}</span>}
          {emptyAction}
        </div>
      ) : filtered.length + visible === 0 ? <p role="status" className="arca-consumption-empty">没有匹配的 {title}</p> : (
        <ul className="arca-consumption-list" data-columns={columns}>
          {filtered.map(({ item, external: isExternal }) => {
            const presentation = present(item.asset);
            const presentationDescription = presentation.description?.trim();
            const enabled = typeof item.enabled === "boolean" ? item.enabled : null;
            const toggleDisabled = typeof enabledChangeDisabled === "function"
              ? enabledChangeDisabled(item)
              : enabledChangeDisabled;
            const toggleLabel = toggleKind === "current"
              ? enabled
                ? `${presentation.name} 当前正在使用；请选择其他 Model 切换`
                : `使用 ${presentation.name}`
              : enabled
                ? `停用 ${presentation.name}`
                : `启用 ${presentation.name}`;
            const canInspect = domain === "mcp" && Boolean(onInspect);
            const canOpen = !isExternal && Boolean(onOpenAsset);
            const canViewDetail = canInspect || canOpen || Boolean(onOpenAsset);
            const openDetail = () => {
              if (canInspect) onInspect?.(item);
              else if (onOpenAsset) onOpenAsset?.(item.asset);
            };
            const openDetailLabel = canInspect
              ? `查看 ${presentation.name} 配置`
              : `查看 ${presentation.name} 详情`;
            const hasTitleButton = canInspect || canOpen;
            const canRemoveDirect = !isExternal && Boolean(onRemove);
            const canDetachDirect = Boolean(onConverge) && item.available_actions.includes("detach");
            const canDelete = canRemoveDirect || canDetachDirect;
            const removeActionLabel = canDetachDirect && !canRemoveDirect
              ? `取消关联 ${presentation.name}`
              : (removeLabel?.(presentation.name) ?? `从 Agent 移除 ${presentation.name}`);
            const handleDelete = () => {
              if (canRemoveDirect) {
                onRemove?.(item.asset);
              } else if (canDetachDirect) {
                onConverge?.(item, "detach");
              }
            };
            const remainingConvergenceActions = canDetachDirect
              ? item.available_actions.filter((action) => action !== "detach")
              : item.available_actions;
            const hasConvergence = Boolean(onConverge) && remainingConvergenceActions.length > 0;
            const hasControls = hasConvergence
              || Boolean(renderAction)
              || (!isExternal && Boolean(onEnabledChange) && enabled !== null);
            return (
              <li
                key={rowKey(item, isExternal)}
                data-domain={domain}
                data-status={item.status}
                data-enabled={isExternal || enabled === false ? "false" : undefined}
                data-clickable={canViewDetail ? "true" : undefined}
                onClick={canViewDetail ? openDetail : undefined}
              >
                <span className="arca-consumption-icon">{presentation.icon}</span>
                <span className="arca-consumption-copy">
                  <span className="arca-consumption-title">
                    {hasTitleButton ? (
                      <button
                        type="button"
                        className="arca-consumption-inspect"
                        aria-label={openDetailLabel}
                        aria-haspopup={canInspect ? "dialog" : undefined}
                        onClick={(event) => {
                          event.stopPropagation();
                          event.currentTarget.focus({ preventScroll: true });
                          openDetail();
                        }}
                      >
                        <strong title={presentation.name}>{presentation.name}</strong>
                      </button>
                    ) : (
                      <strong title={presentation.name}>{presentation.name}</strong>
                    )}
                  </span>
                  {domain === "mcp" && <span className="arca-consumption-secondary">
                    {!isExternal && <ConsumptionStatus status={item.status} reason={item.reason} compact />}
                    {presentation.meta && <span className="arca-consumption-meta">{presentation.meta}</span>}
                    {presentationDescription && <small title={presentationDescription}>{presentationDescription}</small>}
                  </span>}
                </span>
                {canDelete && (
                  <button
                    type="button"
                    className="arca-consumption-open arca-consumption-remove"
                    disabled={removeDisabled || (canDetachDirect && !canRemoveDirect && convergenceDisabled)}
                    aria-label={removeActionLabel}
                    title={removeActionLabel}
                    onClick={(event) => {
                      event.stopPropagation();
                      handleDelete();
                    }}
                  >
                    <TrashIcon className="w-3.5 h-3.5" />
                  </button>
                )}
                {domain !== "mcp" && (presentationDescription || presentation.meta || (!isExternal && item.status !== "synced")) && (
                  <p className="arca-consumption-card-description" title={presentationDescription}>
                    {!isExternal && <ConsumptionStatus status={item.status} reason={item.reason} compact />}
                    {presentation.meta && <span className="arca-consumption-meta">{presentation.meta}</span>}
                    {presentationDescription && <span>{presentationDescription}</span>}
                  </p>
                )}
                <div className="arca-consumption-card-footer">
                  {domain !== "mcp" && <div className="arca-consumption-card-summary">
                    <ConsumptionStatus status={item.status} reason={item.reason} compact />
                    {presentation.meta && <span className="arca-consumption-meta">{presentation.meta}</span>}
                  </div>}
                {hasControls && (
                  <span
                    className="arca-consumption-controls"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <span className="arca-consumption-actions">
                      {hasConvergence && (
                        <ConvergenceActions
                          name={`${presentation.name}${item.asset.domain === "mcp" ? ` · ${item.asset.key.split("::").at(-1)}` : item.target?.target_id ? ` · ${item.target.target_id}` : ""}`}
                          actions={item.available_actions}
                          disabled={convergenceDisabled}
                          onAction={(action) => onConverge?.(item, action)}
                        />
                      )}
                      {renderAction?.(item)}
                      {!isExternal && onEnabledChange && enabled !== null && (
                        <Switch
                          checked={enabled}
                          compact
                          disabled={toggleDisabled}
                          ariaLabel={toggleLabel}
                          title={toggleLabel}
                          onChange={(next) => onEnabledChange(item, next)}
                        />
                      )}
                    </span>
                  </span>
                )}
                </div>
              </li>
            );
          })}
          {filteredDeclarations.map(item => <ConfiguredMcpCard key={JSON.stringify([item.agent_id, item.source_path, item.name])} item={item} />)}
          {filteredSkillFiles.map(item => <NativeFileSkillCard key={item.source_path} item={item} />)}
          {filteredHosts.map(item => <HostMcpCard key={`${item.agent_id}:${item.source_fingerprint}`} item={item} />)}
          {filteredSkills.map(item => <NativeSkillCard key={item.id} item={item} />)}
        </ul>
      )}
    </section>
  );
}
