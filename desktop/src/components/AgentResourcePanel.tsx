import { Tabs } from "@base-ui/react/tabs";
import { createContext, useContext, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { RESOURCE_CATEGORIES, type ResourceCategoryId } from "./resourcePresentation";
import { DocumentIcon } from "./icons";

const ToolbarTarget = createContext<HTMLDivElement | null | undefined>(undefined);

export function AgentResourceActions({ children }: { children: ReactNode }) {
  const target = useContext(ToolbarTarget);
  if (target === undefined) return <>{children}</>;
  return target ? createPortal(children, target) : null;
}

export type AgentResourceTab = ResourceCategoryId | "instructions" | "plugins";

export function AgentResourcePanel({
  value,
  onChange,
  counts,
  countLabels,
  instructions = false,
  plugins,
  subagents: _subagents = false,
  children,
}: {
  value: AgentResourceTab;
  onChange: (value: AgentResourceTab) => void;
  counts: Partial<Record<ResourceCategoryId, number>>;
  countLabels?: Partial<Record<ResourceCategoryId, string>>;
  instructions?: boolean;
  plugins?: number;
  subagents?: boolean;
  children: ReactNode;
}) {
  const TABS: Array<{ id: AgentResourceTab; label: string; Icon: typeof DocumentIcon }> = [
    ...RESOURCE_CATEGORIES,
    ...(instructions ? [{ id: "instructions" as const, label: "Instructions", Icon: DocumentIcon }] : []),
    ...(plugins !== undefined ? [{ id: "plugins" as const, label: "Plugins", Icon: DocumentIcon }] : [])];
  const selected = TABS.find((tab) => tab.id === value);
  const [toolbarTarget, setToolbarTarget] = useState<HTMLDivElement | null>(null);

  return (
    <ToolbarTarget.Provider value={toolbarTarget}><Tabs.Root
      className="arca-agent-resource-panel"
      render={<section aria-label="Agent 资源与配置" />}
      orientation="vertical"
      value={value}
      onValueChange={(next) => {
        const tab = TABS.find(({ id }) => id === next);
        if (tab) onChange(tab.id);
      }}
    >
      <div className="arca-agent-resource-navigation">
        <Tabs.List className="arca-agent-resource-tabs" aria-label="Agent 资源" activateOnFocus>
          {TABS.map((tab) => (
            <Tabs.Tab key={tab.id} value={tab.id}>
              <span className="arca-agent-resource-tab-icon" aria-hidden="true"><tab.Icon className="w-4 h-4" /></span>
              <span className="arca-agent-resource-tab-label">{tab.label}</span>
              {tab.id !== "instructions" && <span className="arca-agent-resource-tab-count">{tab.id === "plugins" ? plugins : (countLabels?.[tab.id] ?? counts[tab.id] ?? 0)}</span>}
            </Tabs.Tab>
          ))}
        </Tabs.List>
      </div>
      <div className="arca-agent-resource-main">
        <div className="arca-agent-resource-panel-head">
          <h3>{selected?.label}</h3>
          <div className="arca-agent-toolbar-actions">
            <div className="arca-agent-toolbar-target" ref={setToolbarTarget} />
          </div>
        </div>
        <Tabs.Panel className="arca-agent-resource-panel-body" value={value} tabIndex={-1}>
          {children}
        </Tabs.Panel>
      </div>
    </Tabs.Root></ToolbarTarget.Provider>
  );
}
