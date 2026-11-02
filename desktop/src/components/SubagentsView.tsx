import { useEffect, useMemo, useState } from "react";
import type { SubagentDefinition, ResourceNavigationIntent } from "../lib/types";
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
  LayersIcon,
  PlusIcon,
  SparklesIcon,
  SubagentMarkIcon,
  TrashIcon,
} from "./icons";
import { Avatar, Badge } from "./ui";
import { SubagentEditorDialog } from "./SubagentEditorDialog";
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

interface SubagentsViewProps {
  consumptionState: ConsumptionState;
  intent?: Extract<ResourceNavigationIntent, { domain: "subagent" }>;
  onIntentConsumed?(id: number): void;
  onOpenAgent?(id: string): void;
}

export function SubagentsView({
  consumptionState,
  intent,
  onIntentConsumed,
  onOpenAgent,
}: SubagentsViewProps) {
  const toast = useToast();
  const [subagents, setSubagents] = useState<SubagentDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<"all" | "readonly">("all");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingSubagent, setEditingSubagent] = useState<SubagentDefinition | null>(null);
  const [deletingSubagent, setDeletingSubagent] = useState<SubagentDefinition | null>(null);
  const [deleting, setDeleting] = useState(false);

  const loadSubagents = async () => {
    setLoading(true);
    try {
      const list = await api.listSubagents();
      setSubagents(list);
      if (list.length > 0 && !selectedName) {
        setSelectedName(list[0].name);
      }
    } catch (err) {
      toast.show({ kind: "error", msg: `读取 Subagents 失败: ${formatError(err)}` });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadSubagents();
  }, []);

  // Handle navigation intents
  useEffect(() => {
    if (!intent) return;
    if (intent.kind === "create") {
      setEditingSubagent(null);
      setEditorOpen(true);
    } else if (intent.kind === "detail" && intent.name) {
      setSelectedName(intent.name);
    }
    onIntentConsumed?.(intent.id);
  }, [intent, onIntentConsumed]);

  const filteredSubagents = useMemo(() => {
    const q = query.trim().toLowerCase();
    return subagents.filter((s) => {
      if (filter === "readonly" && !s.readonly) return false;
      if (!q) return true;
      return (
        s.name.toLowerCase().includes(q) ||
        s.display_name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q) ||
        s.system_prompt.toLowerCase().includes(q)
      );
    });
  }, [subagents, query, filter]);

  const selectedSubagent = useMemo(() => {
    if (!selectedName) return null;
    return subagents.find((s) => s.name === selectedName) ?? null;
  }, [subagents, selectedName]);

  const existingNames = useMemo(() => new Set(subagents.map((s) => s.name)), [subagents]);

  const handleSave = async (definition: SubagentDefinition) => {
    await api.saveSubagent(definition);
    toast.show({
      kind: "success",
      msg: `Subagent "${definition.display_name || definition.name}" 已保存`,
    });
    await loadSubagents();
    setSelectedName(definition.name);
    void consumptionState.refresh();
  };

  const handleDelete = async () => {
    if (!deletingSubagent) return;
    setDeleting(true);
    try {
      await api.deleteSubagent(deletingSubagent.name);
      toast.show({
        kind: "success",
        msg: `Subagent "${deletingSubagent.display_name || deletingSubagent.name}" 已删除`,
      });
      setDeletingSubagent(null);
      if (selectedName === deletingSubagent.name) {
        setSelectedName(null);
      }
      await loadSubagents();
      void consumptionState.refresh();
    } catch (err) {
      toast.show({ kind: "error", msg: `删除失败: ${formatError(err)}` });
    } finally {
      setDeleting(false);
    }
  };

  const handleCopyPrompt = async (prompt: string) => {
    try {
      await navigator.clipboard.writeText(prompt);
      toast.show({ kind: "success", msg: "系统提示词已复制到剪贴板" });
    } catch {
      toast.show({ kind: "error", msg: "复制失败" });
    }
  };

  const activeConsumers = useMemo(() => {
    if (!selectedSubagent) return [];
    return consumersForAsset(consumptionState.inventory, {
      domain: "subagent",
      name: selectedSubagent.name,
    });
  }, [consumptionState.inventory, selectedSubagent]);

  return (
    <>
      <ResourceWorkspace
        overview={
          <ResourceOverview
            eyebrow="子代理资产库"
            title="Subagents"
            description="跨 Agent 集中管理的独立子代理角色与能力定义，支持物理投射至 Claude Code、Google Antigravity 等宿主环境。"
            icon={<ResourceIcon domain="subagent" className="w-6 h-6" />}
            actions={
              <button
                type="button"
                className="btn-primary"
                onClick={() => {
                  setEditingSubagent(null);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-4 h-4" />
                <span>新建 Subagent</span>
              </button>
            }
          />
        }
        sidebar={
          <WorkspaceSidebar
            title="Subagents"
            count={subagents.length}
            actions={
              <button
                className="btn-primary"
                type="button"
                onClick={() => {
                  setEditingSubagent(null);
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
                icon={<ResourceIcon domain="subagent" className="w-3.5 h-3.5" />}
                label="全部 Subagents"
                count={subagents.length}
                onClick={() => setFilter("all")}
              />
              <SidebarItem
                active={filter === "readonly"}
                icon={<SparklesIcon className="w-3.5 h-3.5" />}
                label="只读沙盒"
                count={subagents.filter((s) => s.readonly).length}
                onClick={() => setFilter("readonly")}
              />
            </SidebarSection>
          </WorkspaceSidebar>
        }
        query={query}
        onQueryChange={setQuery}
        searchPlaceholder="搜索 Subagents…"
        listLabel="Subagents"
        resultCount={filteredSubagents.length}
        inspector={
          selectedSubagent ? (
            <ResourceInspector
              avatar={<Avatar seed={selectedSubagent.name} kind="subagent" size={40} />}
              title={selectedSubagent.display_name || selectedSubagent.name}
              subtitle={selectedSubagent.name}
              onClose={() => setSelectedName(null)}
              footer={
                <div className="flex items-center justify-between w-full">
                  <button
                    type="button"
                    className="btn-secondary text-xs text-rose-500 hover:text-rose-600 flex items-center gap-1.5"
                    onClick={() => setDeletingSubagent(selectedSubagent)}
                  >
                    <TrashIcon className="w-3.5 h-3.5" />
                    <span>删除</span>
                  </button>
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1.5"
                    onClick={() => {
                      setEditingSubagent(selectedSubagent);
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
                  label="偏好模型"
                  value={selectedSubagent.model || "inherit"}
                />
                <InspectorMetric
                  icon={<SparklesIcon className="w-4 h-4" />}
                  label="运行权限"
                  value={selectedSubagent.readonly ? "只读沙盒" : "标准读写"}
                />
                <InspectorMetric
                  icon={<LayersIcon className="w-4 h-4" />}
                  label="衍生权限"
                  value={selectedSubagent.allow_subagents ? "允许" : "禁止"}
                />
                <InspectorMetric
                  icon={<SubagentMarkIcon className="w-4 h-4" />}
                  label="宿主投射"
                  value={`${activeConsumers.length} 个`}
                />
              </InspectorMetrics>

              {selectedSubagent.description && (
                <InspectorField label="角色描述">
                  <p className="text-sm text-foreground leading-relaxed">
                    {selectedSubagent.description}
                  </p>
                </InspectorField>
              )}

              <InspectorField label="宿主 Agent 投射状态">
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
                    尚未分配给任何 Agent。进入 Claude Code 或 Antigravity 详情页的 Subagents 选项卡即可一键添加。
                  </p>
                )}
              </InspectorField>

              {selectedSubagent.tools && selectedSubagent.tools.length > 0 && (
                <InspectorField label="允许调用的工具 (Tool Whitelist)">
                  <div className="flex flex-wrap gap-1.5">
                    {selectedSubagent.tools.map((tool) => (
                      <span key={tool} className="arca-badge font-mono text-xs">
                        {tool}
                      </span>
                    ))}
                  </div>
                </InspectorField>
              )}

              {selectedSubagent.mcp_dependencies && selectedSubagent.mcp_dependencies.length > 0 && (
                <InspectorField label="依赖的 MCP 服务">
                  <div className="flex flex-wrap gap-1.5">
                    {selectedSubagent.mcp_dependencies.map((dep) => (
                      <span key={dep} className="arca-badge text-xs bg-indigo-500/10 text-indigo-400">
                        {dep}
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
                    onClick={() => handleCopyPrompt(selectedSubagent.system_prompt)}
                  >
                    <CopyIcon className="w-3 h-3" />
                    <span>复制</span>
                  </button>
                }
              >
                <pre className="p-3 bg-subtle rounded-md text-xs font-mono whitespace-pre-wrap break-words max-h-96 overflow-y-auto leading-relaxed border border-subtle">
                  {selectedSubagent.system_prompt}
                </pre>
              </InspectorField>
            </ResourceInspector>
          ) : undefined
        }
        onInspectorClose={() => setSelectedName(null)}
      >
        {loading && subagents.length === 0 ? (
          <div className="p-8 text-center text-sm text-muted">正在加载 Subagents…</div>
        ) : filteredSubagents.length === 0 ? (
          <ResourceEmpty
            icon={<SubagentMarkIcon className="w-8 h-8" />}
            title={query ? "没有匹配的 Subagent" : "暂无 Subagent 资产"}
            detail={query ? "尝试调整搜索关键词" : "点击右上角新建第一个跨 Agent 子代理"}
            action={
              <button
                type="button"
                className="btn-primary"
                onClick={() => {
                  setEditingSubagent(null);
                  setEditorOpen(true);
                }}
              >
                <PlusIcon className="w-4 h-4" />
                <span>新建 Subagent</span>
              </button>
            }
          />
        ) : (
          <ResourceGrid>
            {filteredSubagents.map((item) => (
              <div
                key={item.name}
                className="arca-card cursor-pointer group hover:border-primary transition-colors p-4 flex flex-col justify-between"
                data-active={selectedSubagent?.name === item.name ? "true" : undefined}
                onClick={() => setSelectedName(item.name)}
              >
                <div>
                  <div className="flex items-start justify-between gap-3 mb-2">
                    <div className="flex items-center gap-2.5 min-w-0">
                      <Avatar seed={item.name} kind="subagent" size={32} />
                      <div className="min-w-0">
                        <h4 className="font-semibold text-sm truncate">
                          {item.display_name || item.name}
                        </h4>
                        <span className="text-xs text-muted font-mono truncate block">
                          {item.name}
                        </span>
                      </div>
                    </div>
                    {item.model && (
                      <Badge tone="neutral">
                        {item.model}
                      </Badge>
                    )}
                  </div>
                  {item.description && (
                    <p className="text-xs text-muted line-clamp-2 mt-2 leading-relaxed">
                      {item.description}
                    </p>
                  )}
                </div>

                <div className="mt-4 pt-3 border-t border-subtle flex items-center justify-between text-xs text-muted">
                  <div className="flex items-center gap-1.5">
                    {item.readonly && <Badge tone="warning">只读沙盒</Badge>}
                    {item.allow_subagents && <Badge tone="neutral">允许衍生</Badge>}
                  </div>
                  <span className="text-[11px]">
                    {consumersForAsset(consumptionState.inventory, {
                      domain: "subagent",
                      name: item.name,
                    }).length}{" "}
                    个 Agent 投射
                  </span>
                </div>
              </div>
            ))}
          </ResourceGrid>
        )}
      </ResourceWorkspace>

      {editorOpen && (
        <SubagentEditorDialog
          subagent={editingSubagent}
          existingNames={existingNames}
          onSave={handleSave}
          onClose={() => {
            setEditorOpen(false);
            setEditingSubagent(null);
          }}
        />
      )}

      {deletingSubagent && (
        <ReviewDialog
          title={`删除 Subagent · ${deletingSubagent.display_name || deletingSubagent.name}`}
          subtitle={`确定要彻底删除 "${deletingSubagent.name}" 吗？`}
          confirmLabel={deleting ? "删除中…" : "确认删除"}
          onConfirm={handleDelete}
          onClose={() => setDeletingSubagent(null)}
        >
          <div className="p-4 bg-rose-500/10 border border-rose-500/20 rounded-md text-sm text-rose-400 space-y-2">
            <p className="font-semibold">⚠️ 该操作将删除 Arca 中央存储，并同步卸载所有关联 Agent 上的实体文件：</p>
            <ul className="list-disc list-inside text-xs space-y-1">
              <li>中央资产库: ~/.arca/assets/subagents/{deletingSubagent.name}.json</li>
              <li>Claude Code: ~/.claude/agents/{deletingSubagent.name}.md (如已投射)</li>
              <li>Antigravity: ~/.gemini/config/subagents/{deletingSubagent.name}.json (如已投射)</li>
            </ul>
          </div>
        </ReviewDialog>
      )}
    </>
  );
}
