import { createContext, useContext, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { RESOURCE_CATEGORIES, type ResourceCategoryId } from "./resourcePresentation";
import { DocumentIcon } from "./icons";

const ToolbarTarget = createContext<HTMLDivElement | null | undefined>(undefined);

export function AgentResourceActions({ children }: { children: ReactNode }) {
  const target = useContext(ToolbarTarget);
  if (target === undefined) return <>{children}</>;
  return target ? createPortal(children, target) : null;
}

export type AgentResourceTab = ResourceCategoryId | "instructions";

export function AgentResourcePanel({
  value,
  onChange,
  counts,
  instructions = false,
  children,
}: {
  value: AgentResourceTab;
  onChange: (value: AgentResourceTab) => void;
  counts: Record<ResourceCategoryId, number>;
  instructions?: boolean;
  children: ReactNode;
}) {
  const TABS: Array<{ id: AgentResourceTab; label: string; Icon: typeof DocumentIcon }> = [...RESOURCE_CATEGORIES,
    ...(instructions ? [{ id: "instructions" as const, label: "Instructions", Icon: DocumentIcon }] : [])];
  const id = useId();
  const refs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = TABS.findIndex((tab) => tab.id === value);
  const panelId = `${id}-panel`;
  const [toolbarTarget, setToolbarTarget] = useState<HTMLDivElement | null>(null);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    let nextIndex: number | null = null;
    if (event.key === "ArrowUp") nextIndex = (index - 1 + TABS.length) % TABS.length;
    if (event.key === "ArrowDown") nextIndex = (index + 1) % TABS.length;
    if (event.key === "Home") nextIndex = 0;
    if (event.key === "End") nextIndex = TABS.length - 1;
    if (nextIndex === null) return;
    event.preventDefault();
    onChange(TABS[nextIndex].id);
    refs.current[nextIndex]?.focus();
  };

  return (
    <ToolbarTarget.Provider value={toolbarTarget}><section className="arca-agent-resource-panel" aria-label="Agent 资源与配置">
      <div className="arca-agent-resource-navigation">
        <div className="arca-agent-resource-tabs" role="tablist" aria-label="Agent 资源" aria-orientation="vertical">
          {TABS.map((tab, index) => (
            <button
              key={tab.id}
              ref={(element) => { refs.current[index] = element; }}
              type="button"
              role="tab"
              id={`${id}-${tab.id}`}
              aria-controls={panelId}
              aria-selected={tab.id === value}
              tabIndex={tab.id === value ? 0 : -1}
              data-active={tab.id === value ? "true" : undefined}
              onClick={() => onChange(tab.id)}
              onKeyDown={(event) => handleKeyDown(event, index)}
            >
              <span className="arca-agent-resource-tab-icon" aria-hidden="true"><tab.Icon className="w-4 h-4" /></span>
              <span className="arca-agent-resource-tab-label">{tab.label}</span>
              {tab.id !== "instructions" && <span className="arca-agent-resource-tab-count">{counts[tab.id]}</span>}
            </button>
          ))}
        </div>
      </div>
      <div className="arca-agent-resource-main">
        <div className="arca-agent-resource-panel-head">
          <h3>{TABS[selectedIndex]?.label}</h3>
          <div className="arca-agent-toolbar-actions">
            <div className="arca-agent-toolbar-target" ref={setToolbarTarget} />
          </div>
        </div>
        <div
          className="arca-agent-resource-panel-body"
          id={panelId}
          role="tabpanel"
          aria-labelledby={`${id}-${TABS[selectedIndex]?.id ?? value}`}
        >
          {children}
        </div>
      </div>
    </section></ToolbarTarget.Provider>
  );
}
