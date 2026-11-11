import { useEffect, useMemo, useState } from "react";
import type { StartupAgentDefinition, ResourceNavigationIntent } from "../lib/types";
import * as api from "../lib/api";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";
import { consumersForAsset } from "../lib/consumption";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import { ResourceIcon } from "./resourcePresentation";
import {
  BrainIcon,
  CopyIcon,
  EditIcon,
  PlusIcon,
  SparklesIcon,
  AgentIcon,
  TrashIcon,
  TerminalIcon,
} from "./icons";
import { Avatar, Badge } from "./ui";
import { StartupAgentEditorDialog } from "./StartupAgentEditorDialog";
import { ReviewDialog } from "./ReviewDialog";
import {
  InspectorField,
  InspectorMetric,
  InspectorMetrics,
  ResourceEmpty,
  ResourceGrid,
  ResourceInspector,
  ResourceOverview,
  ResourceWorkspace,
  SidebarItem,
  SidebarSection,
  WorkspaceSidebar,
} from "./ResourceWorkspace";

interface StartupAgentsViewProps {
  consumptionState: ConsumptionState;
  intent?: Extract<ResourceNavigationIntent, { domain: "startup_agent" }>;
  onIntentConsumed?(id: number): void;
  onOpenAgent?(id: string): void;
}

export function StartupAgentsView({
  consumptionState,
  intent,
  onIntentConsumed,
  onOpenAgent,
}: StartupAgentsViewProps) {
  const toast = useToast();
  const [startupAgents, setStartupAgents] = useState<StartupAgentDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "readonly">("all");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingAgent, setEditingAgent] = useState<StartupAgentDefinition | null>(null);
  const [deletingAgent, setDeletingAgent] = useState<StartupAgentDefinition | null>(null);
  const [deleting, setDeleting] = useState(false);

  const loadStartupAgents = async () => {
    setLoading(true);
    try {
      const list = await api.listStartupAgents();
      setStartupAgents(list);
    } catch (err) {
      toast.show({ kind: "error", msg: `读取 Agents 失败: ${formatError(err)}` });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadStartupAgents();
  }, []);

  // Handle navigation intents
  useEffect(() => {
    if (!intent) return;
    if (intent.kind === "create") {
      setEditingAgent(null);
      setEditorOpen(true);
    } else if (intent.kind === "detail" && intent.name) {
      setSelectedName(intent.name);
    }
    onIntentConsumed?.(intent.id);
  }, [intent, onIntentConsumed]);

  const filteredAgents = useMemo(() => {
    const q = query.trim().toLowerCase();
    return startupAgents.filter((s) => {
      if (filter === "readonly" && !s.readonly) return false;
      if (!q) return true;
      return (
        s.name.toLowerCase().includes(q) ||
        s.display_name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.system_prompt.toLowerCase().includes(q)
      );
    });
  }, [startupAgents, query, filter]);

  const selectedAgent = useMemo(() => {
    if (!selectedName) return null;
    return startupAgents.find((s) => s.name === selectedName) ?? null;
  }, [startupAgents, selectedName]);

  const existingNames = useMemo(() => new Set(startupAgents.map((s) => s.name)), [startupAgents]);

  const handleSave = async (definition: StartupAgentDefinition) => {
    await api.saveStartupAgent(definition);
    toast.show({
      kind: "success",
      msg: `Agent "${definition.display_name || definition.name}" 已保存`,
    });
    await loadStartupAgents();
    setSelectedName(definition.name);
    void consumptionState.refresh();
  };

  const handleDelete = async () => {
    if (!deletingAgent) return;
    setDeleting(true);
    try {
      await api.deleteStartupAgent(deletingAgent.name);
      toast.show({
        kind: "success",
        msg: `Agent "${deletingAgent.display_name || deletingAgent.name}" 已删除`,
      });
      setDeletingAgent(null);
      if (selectedName === deletingAgent.name) {
        setSelectedName(null);
      }
      await loadStartupAgents();
      void consumptionState.refresh();
    } catch (err) {
      toast.show({ kind: "error", msg: `删除失败: ${formatError(err)}` });
    } finally {
      setDeleting(false);
    }
  };

  const handleCopy = async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.show({ kind: "success", msg: `${label}已复制到剪贴板` });
    } catch {
      toast.show({ kind: "error", msg: "复制失败" });
    }
  };

  const activeConsumers = useMemo(() => {
    if (!selectedAgent) return [];
    return consumersForAsset(consumptionState.inventory, {
      domain: "startup_agent",
      name: selectedAgent.name,
    });
  }, [consumptionState.inventory, selectedAgent]);

  return (
    <>
      <ResourceWorkspace
        overview={
          <ResourceOverview
            eyebrow="启动代理资产库"
            title="Agents"
            description="集中管理可用于启动时指定的 Agent 角色与规范定义，支持注入启动参数 (--agent) 或直接写入工具原生配置。"
            icon={<ResourceIcon domain="startup_agent" className="w-6 h-6" />}
            actions={
              <button
                type="button"
                className="btn-primary"
                onClick={() => {
                  setEditingAgent(null);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-4 h-4" />
                <span>新建 Agent</span>
              </button>
            }
          />
        }
        sidebar={
          <WorkspaceSidebar
            title="Agents"
            count={startupAgents.length}
            actions={
              <button
                className="btn-primary"
                type="button"
                onClick={() => {
                  setEditingAgent(null);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-4 h-4" />
                <span>新建</span>
              </button>
            }
          >
            <SidebarSection title="分类">
              <SidebarItem
                active={filter === "all"}
                icon={<ResourceIcon domain="startup_agent" className="w-3.5 h-3.5" />}
                label="全部 Agents"
                count={startupAgents.length}
                onClick={() => setFilter("all")}
              />
              <SidebarItem
                active={filter === "readonly"}
                icon={<SparklesIcon className="w-3.5 h-3.5" />}
                label="只读模式"
                count={startupAgents.filter((s) => s.readonly).length}
                onClick={() => setFilter("readonly")}
              />
            </SidebarSection>
          </WorkspaceSidebar>
        }
        query={query}
        onQueryChange={setQuery}
        searchPlaceholder="搜索 Agents…"
        listLabel="Agents"
        resultCount={filteredAgents.length}
        inspector={
          selectedAgent ? (
            <ResourceInspector
              avatar={<Avatar seed={selectedAgent.name} kind="startup_agent" size={40} />}
              title={selectedAgent.display_name || selectedAgent.name}
              subtitle={selectedAgent.name}
              onClose={() => setSelectedName(null)}
              footer={
                <div className="flex items-center justify-between w-full">
                  <button
                    type="button"
                    className="btn-secondary text-xs text-rose-500 hover:text-rose-600 flex items-center gap-1.5"
                    onClick={() => setDeletingAgent(selectedAgent)}
                  >
                    <TrashIcon className="w-3.5 h-3.5" />
                    <span>删除</span>
                  </button>
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1.5"
                    onClick={() => {
                      setEditingAgent(selectedAgent);
                      setEditorOpen(true);
                    }}
                  >
                    <EditIcon className="w-3.5 h-3.5" />
                    <span>编辑</span>
                  </button>
                </div>
              }
            >
              <InspectorMetrics>
                <InspectorMetric
                  icon={<BrainIcon className="w-4 h-4" />}
                  label="推荐模型"
                  value={selectedAgent.model || "未指定"}
                />
                <InspectorMetric
                  icon={<TerminalIcon className="w-4 h-4" />}
                  label="模式标识"
                  value={selectedAgent.mode || "default"}
                />
                <InspectorMetric
                  icon={<SparklesIcon className="w-4 h-4" />}
                  label="权限"
                  value={selectedAgent.readonly ? "只读沙盒" : "标准"}
                />
                <InspectorMetric
                  icon={<AgentIcon className="w-4 h-4" />}
                  label="工具投射"
                  value={`${activeConsumers.length} 个`}
                />
              </InspectorMetrics>

              {selectedAgent.description && (
                <InspectorField label="角色描述">
                  <p className="text-sm text-foreground leading-relaxed">
                    {selectedAgent.description}
                  </p>
                </InspectorField>
              )}

              <InspectorField label="工具投射状态">
                {activeConsumers.length > 0 ? (
                  <div className="flex flex-wrap gap-2">
                    {activeConsumers.map((c) => (
                      <button
                        key={c.agent_id}
                        type="button"
                        className="arca-badge cursor-pointer hover:bg-subtle flex items-center gap-1.5 py-1 px-2.5"
                        onClick={() => onOpenAgent?.(c.agent_id)}
                        title={`在 ${c.agent_id} 中查看`}
                      >
                        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500" />
                        <span className="font-medium text-xs">{c.agent_id}</span>
                        <span className="text-[10px] text-muted capitalize">({c.status})</span>
                      </button>
                    ))}
                  </div>
                ) : (
                  <p className="text-xs text-muted">
                    尚未投射到任何工具。在工具详情页的 Agents 标签页中可直接启用或设为默认启动身份。
                  </p>
                )}
              </InspectorField>

              <InspectorField
                label="CLI 启动示例"
                action={
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1 py-0.5 px-2"
                    onClick={() => handleCopy(`--agent ${selectedAgent.name}`, "启动参数")}
                  >
                    <CopyIcon className="w-3 h-3" />
                    <span>复制参数</span>
                  </button>
                }
              >
                <div className="p-2.5 bg-subtle rounded-md text-xs font-mono border border-subtle text-foreground/90 space-y-1">
                  <div><span className="text-muted"># OpenCode 示例:</span> opencode --agent {selectedAgent.name}</div>
                  <div><span className="text-muted"># Grok CLI 示例:</span> grok --agent {selectedAgent.name}</div>
                  <div><span className="text-muted"># Vibe CLI 示例:</span> vibe --agent {selectedAgent.name}</div>
                </div>
              </InspectorField>

              {selectedAgent.tools && selectedAgent.tools.length > 0 && (
                <InspectorField label="工具白名单 (Tool Whitelist)">
                  <div className="flex flex-wrap gap-1.5">
                    {selectedAgent.tools.map((tool) => (
                      <span key={tool} className="arca-badge font-mono text-xs">
                        {tool}
                      </span>
                    ))}
                  </div>
                </InspectorField>
              )}

              <InspectorField
                label="系统提示词 (System Prompt Instructions)"
                action={
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1 py-0.5 px-2"
                    onClick={() => handleCopy(selectedAgent.system_prompt, "系统提示词")}
                  >
                    <CopyIcon className="w-3 h-3" />
                    <span>复制</span>
                  </button>
                }
              >
                <pre className="p-3 bg-subtle rounded-md text-xs font-mono whitespace-pre-wrap break-words max-h-96 overflow-y-auto leading-relaxed border border-subtle">
                  {selectedAgent.system_prompt}
                </pre>
              </InspectorField>
            </ResourceInspector>
          ) : undefined
        }
        onInspectorClose={() => setSelectedName(null)}
      >
        {loading && startupAgents.length === 0 ? (
          <div className="p-8 text-center text-sm text-muted">正在加载 Agents…</div>
        ) : filteredAgents.length === 0 ? (
          <ResourceEmpty
            icon={<AgentIcon className="w-8 h-8" />}
            title={query ? "没有匹配的 Agent" : "暂无 Agent 资产"}
            detail={query ? "尝试调整搜索关键词" : "点击右上角新建第一个可在启动时使用的 Agent"}
            action={
              <button
                type="button"
                className="btn-primary"
                onClick={() => {
                  setEditingAgent(null);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-4 h-4" />
                <span>新建 Agent</span>
              </button>
            }
          />
        ) : (
          <ResourceGrid>
            {filteredAgents.map((item) => (
              <div
                key={item.name}
                role="button"
                tabIndex={0}
                aria-label={`查看 ${item.display_name || item.name} 详情`}
                className="arca-card cursor-pointer group hover:border-primary transition-colors p-4 flex flex-col justify-between"
                data-active={selectedAgent?.name === item.name ? "true" : undefined}
                onClick={() => setSelectedName(item.name)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    setSelectedName(item.name);
                  }
                }}
              >
                <div>
                  <div className="flex items-start justify-between gap-3 mb-2">
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Avatar seed={item.name} kind="startup_agent" size={32} />
                      <div className="min-w-0">
                        <h4 className="font-semibold text-sm truncate">
                          {item.display_name || item.name}
                        </h4>
                        <span className="text-xs text-muted font-mono truncate block">
                          {item.name}
                        </span>
                      </div>
                    </div>
                    <div className="flex items-center gap-1.5 shrink-0">
                      {item.model && (
                        <Badge tone="neutral">
                          {item.model}
                        </Badge>
                      )}
                      <button
                        type="button"
                        className="arca-model-card-action arca-model-card-delete"
                        aria-label={`删除 ${item.display_name || item.name}`}
                        title={`删除 ${item.display_name || item.name}`}
                        onClick={(event) => {
                          event.stopPropagation();
                          setDeletingAgent(item);
                        }}
                      >
                        <TrashIcon className="w-4 h-4" />
                      </button>
                    </div>
                  </div>
                  {item.description && (
                    <p className="text-xs text-muted line-clamp-2 mt-2 leading-relaxed">
                      {item.description}
                    </p>
                  )}
                </div>

                <div className="mt-4 pt-3 border-t border-subtle flex items-center justify-between text-xs text-muted">
                  <div className="flex items-center gap-1.5">
                    {item.readonly && <Badge tone="warning">只读</Badge>}
                    {item.mode && <Badge tone="neutral">{item.mode}</Badge>}
                  </div>
                  <span className="text-[11px]">
                    {consumersForAsset(consumptionState.inventory, {
                      domain: "startup_agent",
                      name: item.name,
                    }).length}{" "}
                    个工具投射
                  </span>
                </div>
              </div>
            ))}
          </ResourceGrid>
        )}
      </ResourceWorkspace>

      {editorOpen && (
        <StartupAgentEditorDialog
          startupAgent={editingAgent}
          existingNames={existingNames}
          onSave={handleSave}
          onClose={() => {
            setEditorOpen(false);
            setEditingAgent(null);
          }}
        />
      )}

      {deletingAgent && (
        <ReviewDialog
          title={`删除 Agent · ${deletingAgent.display_name || deletingAgent.name}`}
          subtitle={`确定要彻底删除 "${deletingAgent.name}" 吗？`}
          confirmLabel={deleting ? "删除中…" : "确认删除"}
          onConfirm={handleDelete}
          onClose={() => setDeletingAgent(null)}
        >
          <div className="p-4 bg-rose-500/10 border border-rose-500/20 rounded-md text-sm text-rose-400 space-y-2">
            <p className="font-semibold">⚠️ 该操作将删除中央 Agent 资产库中的文件：</p>
            <ul className="list-disc list-inside text-xs space-y-1">
              <li>中央资产库: ~/.arca/assets/agents/{deletingAgent.name}.json</li>
            </ul>
          </div>
        </ReviewDialog>
      )}
    </>
  );
}
