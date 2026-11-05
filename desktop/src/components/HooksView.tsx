import { useEffect, useMemo, useState } from "react";
import type { HookDefinition, ResourceNavigationIntent } from "../lib/types";
import * as api from "../lib/api";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";
import type { ConsumptionState } from "../hooks/useConsumptionState";
import { ResourceIcon } from "./resourcePresentation";
import {
  CopyIcon,
  EditIcon,
  HookMarkIcon,
  LayersIcon,
  PlusIcon,
  SparklesIcon,
  TrashIcon,
} from "./icons";
import { Badge } from "./ui";
import { HookEditorDialog } from "./HookEditorDialog";
import { ReviewDialog } from "./ReviewDialog";
import {
  InspectorField,
  InspectorMetric,
  InspectorMetrics,
  ResourceEmpty,
  ResourceGrid,
  ResourceInspector,
  ResourceWorkspace,
  SidebarItem,
  SidebarSection,
  WorkspaceSidebar,
} from "./ResourceWorkspace";

interface HooksViewProps {
  consumptionState: ConsumptionState;
  intent?: Extract<ResourceNavigationIntent, { domain: "hook" }>;
  onIntentConsumed?(id: number): void;
  onOpenAgent?(id: string): void;
}

export function HooksView({
  consumptionState: _consumptionState,
  intent,
  onIntentConsumed,
  onOpenAgent: _onOpenAgent,
}: HooksViewProps) {
  const toast = useToast();
  const [hooks, setHooks] = useState<HookDefinition[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [eventFilter, setEventFilter] = useState<string>("all");
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editingHook, setEditingHook] = useState<HookDefinition | null>(null);
  const [deletingHook, setDeletingHook] = useState<HookDefinition | null>(null);
  const [deleting, setDeleting] = useState(false);

  const loadHooks = async () => {
    setLoading(true);
    try {
      const list = await api.listHooks();
      setHooks(list);
    } catch (err) {
      toast.show({ kind: "error", msg: `读取 Hooks 失败: ${formatError(err)}` });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadHooks();
  }, []);

  // Handle navigation intents
  useEffect(() => {
    if (!intent) return;
    if (intent.kind === "create") {
      setEditingHook(null);
      setEditorOpen(true);
    } else if (intent.kind === "detail" && intent.name) {
      setSelectedName(intent.name);
    }
    onIntentConsumed?.(intent.id);
  }, [intent, onIntentConsumed]);

  const allEvents = useMemo(() => {
    const set = new Set<string>();
    for (const h of hooks) {
      if (h.event) set.add(h.event);
    }
    return Array.from(set).sort();
  }, [hooks]);

  const filteredHooks = useMemo(() => {
    const q = query.trim().toLowerCase();
    return hooks.filter((h) => {
      if (eventFilter !== "all" && h.event !== eventFilter) return false;
      if (!q) return true;
      return (
        h.name.toLowerCase().includes(q) ||
        h.display_name.toLowerCase().includes(q) ||
        h.description.toLowerCase().includes(q) ||
        h.event.toLowerCase().includes(q) ||
        h.command.toLowerCase().includes(q)
      );
    });
  }, [hooks, query, eventFilter]);

  const selectedHook = useMemo(
    () => hooks.find((h) => h.name === selectedName) ?? null,
    [hooks, selectedName],
  );

  const handleSaveHook = async (definition: HookDefinition) => {
    await api.saveHook(definition);
    toast.show({ kind: "success", msg: `已保存 Hook · ${definition.display_name}` });
    await loadHooks();
    setSelectedName(definition.name);
  };

  const handleDeleteHook = async () => {
    if (!deletingHook) return;
    setDeleting(true);
    try {
      await api.deleteHook(deletingHook.name);
      toast.show({ kind: "success", msg: `已删除 Hook · ${deletingHook.display_name}` });
      setDeletingHook(null);
      await loadHooks();
      if (selectedName === deletingHook.name) {
        setSelectedName(null);
      }
    } catch (err) {
      toast.show({ kind: "error", msg: `删除失败: ${formatError(err)}` });
    } finally {
      setDeleting(false);
    }
  };

  const copyDefinitionJson = (h: HookDefinition) => {
    void navigator.clipboard.writeText(JSON.stringify(h, null, 2)).then(() => {
      toast.show({ kind: "success", msg: "已复制 Hook JSON 定义" });
    });
  };

  return (
    <ResourceWorkspace
      sidebar={
        <WorkspaceSidebar
          title="Hooks"
          count={hooks.length}
          actions={
            <button
              className="btn-primary"
              type="button"
              onClick={() => {
                setEditingHook(null);
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
              active={eventFilter === "all"}
              icon={<ResourceIcon domain="hook" className="w-3.5 h-3.5" />}
              label="全部 Hooks"
              count={hooks.length}
              onClick={() => setEventFilter("all")}
            />
            {allEvents.map((ev) => (
              <SidebarItem
                key={ev}
                active={eventFilter === ev}
                icon={<HookMarkIcon className="w-3.5 h-3.5" />}
                label={ev}
                count={hooks.filter((h) => h.event === ev).length}
                onClick={() => setEventFilter(ev)}
              />
            ))}
          </SidebarSection>
        </WorkspaceSidebar>
      }
      query={query}
      onQueryChange={setQuery}
      searchPlaceholder="搜索 Hooks…"
      listLabel="Hooks"
      resultCount={filteredHooks.length}
      inspector={
        selectedHook ? (
          <ResourceInspector
            avatar={
              <div className="w-10 h-10 rounded-xl bg-blue-500/10 text-blue-500 flex items-center justify-center shrink-0">
                <HookMarkIcon className="w-6 h-6" />
              </div>
            }
            title={selectedHook.display_name || selectedHook.name}
            subtitle={selectedHook.name}
            onClose={() => setSelectedName(null)}
            footer={
              <div className="flex items-center justify-between w-full">
                <button
                  type="button"
                  className="btn-secondary text-xs text-rose-500 hover:text-rose-600 flex items-center gap-1.5"
                  onClick={() => setDeletingHook(selectedHook)}
                >
                  <TrashIcon className="w-3.5 h-3.5" />
                  <span>删除</span>
                </button>
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1"
                    onClick={() => copyDefinitionJson(selectedHook)}
                  >
                    <CopyIcon className="w-3.5 h-3.5" />
                    <span>复制 JSON</span>
                  </button>
                  <button
                    type="button"
                    className="btn-secondary text-xs flex items-center gap-1.5"
                    onClick={() => {
                      setEditingHook(selectedHook);
                      setEditorOpen(true);
                    }}
                  >
                    <EditIcon className="w-3.5 h-3.5" />
                    <span>编辑</span>
                  </button>
                </div>
              </div>
            }
          >
            <InspectorMetrics>
              <InspectorMetric
                icon={<HookMarkIcon className="w-4 h-4" />}
                label="触发事件"
                value={selectedHook.event}
              />
              <InspectorMetric
                icon={<SparklesIcon className="w-4 h-4" />}
                label="运行状态"
                value={selectedHook.enabled ? "已启用" : "已停用"}
              />
              <InspectorMetric
                icon={<LayersIcon className="w-4 h-4" />}
                label="超时限制"
                value={`${selectedHook.timeout}s`}
              />
            </InspectorMetrics>

            <InspectorField label="描述说明">
              <p className="text-sm text-foreground leading-relaxed">
                {selectedHook.description || "无详细描述"}
              </p>
            </InspectorField>

            <InspectorField label="匹配条件 (Matcher)">
              <span className="arca-badge font-mono text-xs">
                {selectedHook.matcher || "全局 (*)"}
              </span>
            </InspectorField>

            <InspectorField label="执行命令 (Command)">
              <pre className="p-3 bg-subtle rounded-md text-xs font-mono whitespace-pre-wrap break-words max-h-96 overflow-y-auto leading-relaxed border border-subtle">
                {selectedHook.command}
              </pre>
            </InspectorField>

            {selectedHook.command_windows && (
              <InspectorField label="Windows 专用命令">
                <pre className="p-3 bg-subtle rounded-md text-xs font-mono whitespace-pre-wrap break-words max-h-96 overflow-y-auto leading-relaxed border border-subtle">
                  {selectedHook.command_windows}
                </pre>
              </InspectorField>
            )}
          </ResourceInspector>
        ) : undefined
      }
      onInspectorClose={() => setSelectedName(null)}
    >
      {loading && hooks.length === 0 ? (
        <div className="p-8 text-center text-sm text-muted">正在加载 Hooks…</div>
      ) : filteredHooks.length === 0 ? (
        <ResourceEmpty
          icon={<HookMarkIcon className="w-8 h-8" />}
          title={query ? "没有匹配的 Hook" : "暂无 Hook 资产"}
          detail={query ? "尝试调整搜索关键词" : "点击右上角新建第一个事件钩子"}
          action={
            <button
              type="button"
              className="btn-primary"
              onClick={() => {
                setEditingHook(null);
                setEditorOpen(true);
              }}
            >
              <PlusIcon className="w-4 h-4" />
              <span>新建 Hook</span>
            </button>
          }
        />
      ) : (
        <ResourceGrid>
          {filteredHooks.map((item) => (
            <div
              key={item.name}
              role="button"
              tabIndex={0}
              aria-label={`查看 ${item.display_name || item.name} 详情`}
              className="arca-card cursor-pointer group hover:border-primary transition-colors p-4 flex flex-col justify-between"
              data-active={selectedHook?.name === item.name ? "true" : undefined}
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
                    <div className="w-8 h-8 rounded-lg bg-blue-500/10 text-blue-500 flex items-center justify-center shrink-0">
                      <HookMarkIcon className="w-4 h-4" />
                    </div>
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
                    <Badge tone={item.enabled ? "success" : "neutral"}>
                      {item.event}
                    </Badge>
                    <button
                      type="button"
                      className="arca-model-card-action arca-model-card-delete"
                      aria-label={`删除 ${item.display_name || item.name}`}
                      title={`删除 ${item.display_name || item.name}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        setDeletingHook(item);
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
                <div className="mt-2 text-xs font-mono text-neutral-400 bg-neutral-900/50 p-1.5 rounded truncate">
                  {item.command}
                </div>
              </div>
            </div>
          ))}
        </ResourceGrid>
      )}

      {editorOpen && (
        <HookEditorDialog
          hook={editingHook}
          existingNames={new Set(hooks.map((h) => h.name))}
          onSave={handleSaveHook}
          onClose={() => setEditorOpen(false)}
        />
      )}

      {deletingHook && (
        <ReviewDialog
          title="删除 Hook"
          confirmLabel={deleting ? "删除中…" : "确认删除"}
          tone="danger"
          onConfirm={handleDeleteHook}
          onClose={() => setDeletingHook(null)}
        >
          <p>
            确定要删除 Hook <strong>{deletingHook.display_name || deletingHook.name}</strong> 吗？
          </p>
          <p className="text-xs text-neutral-500 mt-2">
            删除后，已投影到各 Agent 的本地原生配置不会被立刻抹除，但该 Hook 将不再作为中央受控资产。
          </p>
        </ReviewDialog>
      )}
    </ResourceWorkspace>
  );
}
