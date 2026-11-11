import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  AgentStartupAgentsStateView,
  AgentStartupAgentItemView,
  StartupAgentDefinition,
} from "../lib/types";
import * as api from "../lib/api";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";
import {
  AgentIcon,
  CheckIcon,
  DownloadIcon,
  PlusIcon,
  RefreshIcon,
  TrashIcon,
} from "./icons";
import { Avatar, Badge } from "./ui";
import { StartupAgentEditorDialog } from "./StartupAgentEditorDialog";
import { DialogShell } from "./DialogShell";

interface AgentStartupAgentsPanelProps {
  agentId: string;
  agentName: string;
  onOpenCentralAgents?: () => void;
}

export function AgentStartupAgentsPanel({
  agentId,
  agentName,
  onOpenCentralAgents,
}: AgentStartupAgentsPanelProps) {
  const toast = useToast();
  const [state, setState] = useState<AgentStartupAgentsStateView | null>(null);
  const [centralAgents, setCentralAgents] = useState<StartupAgentDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [editorOpen, setEditorOpen] = useState(false);

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const [agentState, centralList] = await Promise.all([
        api.getAgentStartupAgentsState(agentId),
        api.listStartupAgents(),
      ]);
      setState(agentState);
      setCentralAgents(centralList);
    } catch (err) {
      toast.show({ kind: "error", msg: `读取 ${agentName} Agents 失败: ${formatError(err)}` });
    } finally {
      setLoading(false);
    }
  }, [agentId, agentName, toast]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  const handleSetActive = async (agentNameOrId: string | null) => {
    setSaving(true);
    try {
      await api.setActiveStartupAgent(agentId, agentNameOrId);
      await loadData();
      toast.show({
        kind: "success",
        msg: agentNameOrId
          ? `已将当前启动 Agent 切换为 "${agentNameOrId}"`
          : "已恢复默认启动身份",
      });
    } catch (err) {
      toast.show({ kind: "error", msg: `切换失败: ${formatError(err)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleAdopt = async (item: AgentStartupAgentItemView) => {
    setSaving(true);
    try {
      const adopted = await api.adoptStartupAgent(agentId, item.id);
      toast.show({
        kind: "success",
        msg: `已将 "${adopted.display_name || adopted.name}" 采纳为中央 Agent 资产`,
      });
      await loadData();
    } catch (err) {
      toast.show({ kind: "error", msg: `采纳失败: ${formatError(err)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async (item: AgentStartupAgentItemView) => {
    setSaving(true);
    try {
      await api.removeStartupAgentFromAgent(agentId, item.id);
      toast.show({
        kind: "success",
        msg: `已从 ${agentName} 移除 "${item.name}"`,
      });
      await loadData();
    } catch (err) {
      toast.show({ kind: "error", msg: `移除失败: ${formatError(err)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleProjectCentral = async (definition: StartupAgentDefinition) => {
    setSaving(true);
    try {
      await api.projectStartupAgentToAgent(agentId, definition);
      toast.show({
        kind: "success",
        msg: `已将 "${definition.display_name || definition.name}" 添加至 ${agentName}`,
      });
      setPickerOpen(false);
      await loadData();
    } catch (err) {
      toast.show({ kind: "error", msg: `添加失败: ${formatError(err)}` });
    } finally {
      setSaving(false);
    }
  };

  const handleSaveNewAgent = async (definition: StartupAgentDefinition) => {
    await api.saveStartupAgent(definition);
    await api.projectStartupAgentToAgent(agentId, definition);
    toast.show({
      kind: "success",
      msg: `Agent "${definition.display_name || definition.name}" 已创建并添加到 ${agentName}`,
    });
    setEditorOpen(false);
    await loadData();
  };

  const availableCentralAgents = useMemo(() => {
    if (!state) return [];
    const existingIds = new Set(state.items.map((i) => i.id));
    return centralAgents.filter((c) => !existingIds.has(c.name));
  }, [state, centralAgents]);

  const existingNames = useMemo(
    () => new Set(centralAgents.map((c) => c.name)),
    [centralAgents],
  );

  if (loading && !state) {
    return (
      <div className="p-8 text-center text-sm text-muted flex items-center justify-center gap-2">
        <RefreshIcon data-spinning="true" className="w-4 h-4" />
        <span>正在读取 Agents 配置…</span>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Status banner */}
      <div className="arca-card p-4 flex flex-col md:flex-row md:items-center justify-between gap-4 bg-subtle/50">
        <div className="flex items-start gap-3">
          <div className="w-9 h-9 rounded-lg bg-primary/10 flex items-center justify-center text-primary shrink-0 mt-0.5">
            <AgentIcon className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h4 className="font-semibold text-sm">
                当前启动身份:{" "}
                <span className="font-mono text-primary font-bold">
                  {state?.active_agent ?? state?.default_agent ?? "默认 (Default)"}
                </span>
              </h4>
              {state?.active_agent && (
                <Badge tone="success">
                  已激活
                </Badge>
              )}
            </div>
            <p className="text-xs text-muted mt-1">
              {state?.supports_config_persistence
                ? "支持持久化到本地配置文件作为全局默认 Agent"
                : `启动时将自动注入命令行参数: ${state?.cli_flag ?? "--agent"} <name>`}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {state?.active_agent && (
            <button
              type="button"
              className="btn-secondary text-xs"
              disabled={saving}
              onClick={() => handleSetActive(null)}
            >
              恢复默认身份
            </button>
          )}
          {onOpenCentralAgents && (
            <button
              type="button"
              className="btn-secondary text-xs"
              onClick={onOpenCentralAgents}
            >
              打开中央资产库
            </button>
          )}
          <button
            type="button"
            className="btn-primary text-xs flex items-center gap-1.5"
            disabled={saving}
            onClick={() => setPickerOpen(true)}
          >
            <PlusIcon className="w-3.5 h-3.5" />
            <span>添加 Agent</span>
          </button>
        </div>
      </div>

      {/* Agents grid */}
      {(!state?.items || state.items.length === 0) ? (
        <div className="arca-card p-8 text-center space-y-3">
          <div className="w-12 h-12 rounded-full bg-subtle flex items-center justify-center text-muted mx-auto">
            <AgentIcon className="w-6 h-6" />
          </div>
          <h5 className="font-semibold text-sm">暂无可切换的 Agent</h5>
          <p className="text-xs text-muted max-w-md mx-auto">
            该工具尚未添加任何可切换的启动 Agent。您可以从中央资产库中选择已有的 Agent，或新建一个专属于该工具的工作身份。
          </p>
          <button
            type="button"
            className="btn-primary text-xs mx-auto"
            onClick={() => setPickerOpen(true)}
          >
            <PlusIcon className="w-3.5 h-3.5" />
            <span>添加第一个 Agent</span>
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
          {state.items.map((item) => (
            <div
              key={item.id}
              className="arca-card p-4 flex flex-col justify-between space-y-3 border transition-colors hover:border-primary/50"
              data-active={item.is_active ? "true" : undefined}
            >
              <div>
                <div className="flex items-start justify-between gap-2 mb-1.5">
                  <div className="flex items-center gap-2 min-w-0">
                    <Avatar seed={item.name} kind="startup_agent" size={28} />
                    <div className="min-w-0">
                      <div className="font-semibold text-xs truncate">
                        {item.name}
                      </div>
                      <span className="text-[11px] text-muted font-mono truncate block">
                        {item.id}
                      </span>
                    </div>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    {item.source === "builtin" && (
                      <Badge tone="neutral">
                        内置
                      </Badge>
                    )}
                    {item.source === "central" && (
                      <Badge tone="neutral">
                        中央库
                      </Badge>
                    )}
                    {item.source === "external" && (
                      <Badge tone="warning">
                        本地发现
                      </Badge>
                    )}
                  </div>
                </div>

                {item.description && (
                  <p className="text-xs text-muted line-clamp-2 leading-relaxed">
                    {item.description}
                  </p>
                )}
              </div>

              <div className="pt-2 border-t border-subtle flex items-center justify-between gap-2">
                <div className="flex items-center gap-1.5">
                  {item.is_active ? (
                    <span className="flex items-center gap-1 text-emerald-500 text-xs font-semibold">
                      <CheckIcon className="w-3.5 h-3.5" />
                      <span>当前使用</span>
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="btn-secondary text-xs py-1 px-2.5"
                      disabled={saving}
                      onClick={() => handleSetActive(item.id)}
                    >
                      设为当前启动
                    </button>
                  )}
                </div>

                <div className="flex items-center gap-1">
                  {item.source === "external" && (
                    <button
                      type="button"
                      className="btn-ghost text-xs text-primary p-1 hover:bg-subtle rounded"
                      title="采纳为中央资产库"
                      disabled={saving}
                      onClick={() => handleAdopt(item)}
                    >
                      <DownloadIcon className="w-3.5 h-3.5" />
                    </button>
                  )}
                  {item.source === "central" && (
                    <button
                      type="button"
                      className="btn-ghost text-xs text-rose-500 p-1 hover:bg-subtle rounded"
                      title="移除此 Agent"
                      disabled={saving}
                      onClick={() => handleRemove(item)}
                    >
                      <TrashIcon className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Picker Dialog */}
      {pickerOpen && (
        <DialogShell
          kind="editor"
          size="md"
          title={`添加 Agent · ${agentName}`}
          subtitle="从中央资产库中选择要投射的 Agent，或创建全新定义"
          leading={<AgentIcon className="w-5 h-5" />}
          onClose={() => setPickerOpen(false)}
          footerEnd={
            <div className="flex items-center gap-2">
              <button
                type="button"
                className="btn-secondary"
                onClick={() => setPickerOpen(false)}
              >
                关闭
              </button>
              <button
                type="button"
                className="btn-primary flex items-center gap-1.5"
                onClick={() => {
                  setPickerOpen(false);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-3.5 h-3.5" />
                <span>新建全新 Agent</span>
              </button>
            </div>
          }
        >
          <div className="space-y-3">
            {availableCentralAgents.length === 0 ? (
              <div className="p-6 text-center text-sm text-muted space-y-2">
                <p>中央资产库中没有更多可添加的 Agent。</p>
                <p className="text-xs">您可以点击下方“新建全新 Agent”创建一个新 Agent。</p>
              </div>
            ) : (
              <div className="space-y-2 max-h-80 overflow-y-auto pr-1">
                {availableCentralAgents.map((agent) => (
                  <div
                    key={agent.name}
                    className="p-3 border border-subtle rounded-lg flex items-center justify-between gap-3 hover:bg-subtle/50 transition-colors"
                  >
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Avatar seed={agent.name} kind="startup_agent" size={32} />
                      <div className="min-w-0">
                        <div className="font-semibold text-xs truncate">
                          {agent.display_name || agent.name}
                        </div>
                        <div className="text-[11px] text-muted font-mono truncate">
                          {agent.name}
                        </div>
                      </div>
                    </div>

                    <button
                      type="button"
                      className="btn-secondary text-xs shrink-0 flex items-center gap-1"
                      disabled={saving}
                      onClick={() => handleProjectCentral(agent)}
                    >
                      <PlusIcon className="w-3 h-3" />
                      <span>添加到 {agentName}</span>
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </DialogShell>
      )}

      {/* Editor Dialog */}
      {editorOpen && (
        <StartupAgentEditorDialog
          existingNames={existingNames}
          onSave={handleSaveNewAgent}
          onClose={() => setEditorOpen(false)}
        />
      )}
    </div>
  );
}
