import { readLocalSetting, writeLocalSetting } from "../lib/localSettings";
import {
  createContext,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { AgentGlyph, agentName, isAgentVisible } from "./brandIcons";
import { MODAL_DIALOG_SELECTOR, Modal, SearchBar, wasHandledByLayer } from "./ui";
import { XIcon } from "./icons";
import {
  MAX_SIDEBAR_WIDTH,
  MIN_SIDEBAR_WIDTH,
  SIDEBAR_WIDTH_STORAGE_KEY,
  clampSidebarWidth,
  parseSidebarWidth,
} from "../lib/resourceWorkspace";

interface SidebarResizeSession {
  previousCursor: string;
  previousUserSelect: string;
  onPointerMove: (event: PointerEvent) => void;
  onPointerUp: () => void;
}

interface WorkspaceResizeContextValue {
  isResizing: boolean;
  sidebarWidth: number;
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLDivElement>) => void;
}

interface ResourcePanelContextValue {
  panelId: string;
  setActiveTabId: (tabId: string | undefined) => void;
}

const WorkspaceResizeContext = createContext<WorkspaceResizeContextValue | null>(null);
const ResourcePanelContext = createContext<ResourcePanelContextValue | null>(null);

export interface ResourceTabOption<T extends string> {
  value: T;
  label: string;
  count: number;
}

export function ResourceTabs<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: ResourceTabOption<T>[];
  onChange: (value: T) => void;
}) {
  const tabListId = useId();
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const panel = useContext(ResourcePanelContext);
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const selectedTabId = `${tabListId}-${selectedIndex}`;

  useEffect(() => {
    panel?.setActiveTabId(selectedTabId);
  }, [panel, selectedTabId]);

  useEffect(() => () => panel?.setActiveTabId(undefined), [panel]);

  const moveFocus = (index: number) => {
    const option = options[index];
    if (!option) return;
    onChange(option.value);
    tabRefs.current[index]?.focus();
  };

  return (
    <div className="arca-resource-tabs" role="tablist" aria-label={label}>
      {options.map((option, index) => {
        const tabId = `${tabListId}-${index}`;
        return (
          <button
            key={option.value}
            ref={(element) => {
              tabRefs.current[index] = element;
            }}
            type="button"
            className="arca-resource-tab"
            role="tab"
            id={tabId}
            aria-controls={panel?.panelId}
            aria-selected={option.value === value}
            tabIndex={option.value === value ? 0 : -1}
            data-active={option.value === value ? "true" : undefined}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => {
              if (options.length === 0) return;

              let nextIndex: number | null = null;
              if (event.key === "ArrowLeft") {
                nextIndex = (selectedIndex - 1 + options.length) % options.length;
              } else if (event.key === "ArrowRight") {
                nextIndex = (selectedIndex + 1) % options.length;
              } else if (event.key === "Home") {
                nextIndex = 0;
              } else if (event.key === "End") {
                nextIndex = options.length - 1;
              }

              if (nextIndex === null) return;
              event.preventDefault();
              moveFocus(nextIndex);
            }}
          >
            <span>{option.label}</span>
            <span className="arca-resource-tab-count">{option.count}</span>
          </button>
        );
      })}
    </div>
  );
}

export function ResourceWorkspace({
  sidebar,
  title,
  description,
  overview,
  query,
  onQueryChange,
  searchPlaceholder,
  listLabel,
  resultCount,
  toolbarActions,
  children,
  filters,
  inspector,
  onInspectorClose,
}: {
  sidebar?: ReactNode;
  title?: string;
  description?: string;
  overview?: ReactNode;
  query: string;
  onQueryChange: (value: string) => void;
  searchPlaceholder: string;
  listLabel?: string;
  resultCount?: number;
  toolbarActions?: ReactNode;
  children: ReactNode;
  filters?: ReactNode;
  inspector?: ReactNode;
  onInspectorClose?: () => void;
}) {
  const hasSidebar = sidebar !== null && sidebar !== undefined;
  const [sidebarWidth, setSidebarWidth] = useState(() =>
    hasSidebar
      ? parseSidebarWidth(readLocalSetting(SIDEBAR_WIDTH_STORAGE_KEY))
      : MIN_SIDEBAR_WIDTH
  );
  const sidebarWidthRef = useRef(sidebarWidth);
  const resizeSessionRef = useRef<SidebarResizeSession | null>(null);
  const [isResizing, setIsResizing] = useState(false);
  const workspaceId = useId();
  const panelId = `arca-resource-workspace-panel-${workspaceId}`;
  const [activeTabId, setActiveTabId] = useState<string>();
  const resourcePanelContext = useMemo(
    () => ({ panelId, setActiveTabId }),
    [panelId],
  );
  const isInspectorOpen = Boolean(inspector);

  const persistSidebarWidth = useCallback((width: number) => {
    if (!hasSidebar) return;
    writeLocalSetting(SIDEBAR_WIDTH_STORAGE_KEY, String(width));
  }, [hasSidebar]);

  const updateSidebarWidth = useCallback((width: number) => {
    const nextWidth = clampSidebarWidth(width);
    sidebarWidthRef.current = nextWidth;
    setSidebarWidth(nextWidth);
    return nextWidth;
  }, []);

  const finishSidebarResize = useCallback(
    (persist: boolean) => {
      const session = resizeSessionRef.current;
      if (!session) return;

      resizeSessionRef.current = null;
      window.removeEventListener("pointermove", session.onPointerMove);
      window.removeEventListener("pointerup", session.onPointerUp);
      window.removeEventListener("pointercancel", session.onPointerUp);
      document.body.style.userSelect = session.previousUserSelect;
      document.body.style.cursor = session.previousCursor;
      setIsResizing(false);

      if (persist) persistSidebarWidth(sidebarWidthRef.current);
    },
    [persistSidebarWidth]
  );

  const onResizePointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;

      event.preventDefault();
      finishSidebarResize(true);
      const startWidth = sidebarWidthRef.current;
      const startX = event.clientX;
      const previousUserSelect = document.body.style.userSelect;
      const previousCursor = document.body.style.cursor;
      document.body.style.userSelect = "none";
      document.body.style.cursor = "col-resize";
      setIsResizing(true);

      const session: SidebarResizeSession = {
        previousCursor,
        previousUserSelect,
        onPointerMove: (moveEvent) => {
          updateSidebarWidth(startWidth + moveEvent.clientX - startX);
        },
        onPointerUp: () => finishSidebarResize(true),
      };

      resizeSessionRef.current = session;
      window.addEventListener("pointermove", session.onPointerMove);
      window.addEventListener("pointerup", session.onPointerUp);
      window.addEventListener("pointercancel", session.onPointerUp);
    },
    [finishSidebarResize, updateSidebarWidth]
  );

  const onResizeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      let nextWidth: number | null = null;
      if (event.key === "ArrowLeft") nextWidth = sidebarWidthRef.current - 8;
      if (event.key === "ArrowRight") nextWidth = sidebarWidthRef.current + 8;
      if (event.key === "Home") nextWidth = MIN_SIDEBAR_WIDTH;
      if (event.key === "End") nextWidth = MAX_SIDEBAR_WIDTH;
      if (nextWidth === null) return;

      event.preventDefault();
      persistSidebarWidth(updateSidebarWidth(nextWidth));
    },
    [persistSidebarWidth, updateSidebarWidth]
  );

  useEffect(() => () => finishSidebarResize(true), [finishSidebarResize]);

  return (
    <ResourcePanelContext.Provider value={resourcePanelContext}>
      <WorkspaceResizeContext.Provider
        value={
          hasSidebar
            ? {
                isResizing,
                sidebarWidth,
                onPointerDown: onResizePointerDown,
                onKeyDown: onResizeKeyDown,
              }
            : null
        }
      >
        <div
          className="arca-workspace"
          data-sidebar={hasSidebar ? "true" : "false"}
          style={
            hasSidebar
              ? ({ "--arca-workspace-sidebar-width": `${sidebarWidth}px` } as CSSProperties)
              : undefined
          }
        >
          {sidebar}
          <section className="arca-workspace-stage">
            {overview ?? (title && (
              <header className="arca-workspace-intro">
                <div>
                  <h1>{title}</h1>
                  {description && <p>{description}</p>}
                </div>
              </header>
            ))}
            <div className="arca-workspace-toolbar">
              {listLabel && <h2 className="arca-workspace-list-heading">
                {listLabel}
                {typeof resultCount === "number" && <span>{resultCount}</span>}
              </h2>}
              <SearchBar value={query} onChange={onQueryChange} placeholder={searchPlaceholder} />
              {toolbarActions && <div className="arca-workspace-actions">{toolbarActions}</div>}
            </div>
            {filters && <div className="arca-workspace-filters">{filters}</div>}
            <div className="arca-workspace-content">
              <div
                id={panelId}
                className="arca-workspace-scroll"
                role={activeTabId ? "tabpanel" : "region"}
                aria-label={searchPlaceholder}
                aria-labelledby={activeTabId}
                aria-hidden={isInspectorOpen || undefined}
                inert={isInspectorOpen || undefined}
              >
                {children}
              </div>
              {inspector && (
                <Modal
                  width="min(720px, calc(100vw - 32px))"
                  maxHeight="calc(100vh - 32px)"
                  ariaLabel="资源详情"
                  layer="detail"
                  onClose={() => onInspectorClose?.()}
                >
                  <div className="arca-workspace-inspector-surface">{inspector}</div>
                </Modal>
              )}
            </div>
          </section>
        </div>
      </WorkspaceResizeContext.Provider>
    </ResourcePanelContext.Provider>
  );
}

export function WorkspaceSidebar({
  title,
  count,
  actions,
  footer,
  children,
}: {
  title: string;
  count: number;
  actions?: ReactNode;
  footer?: ReactNode;
  children: ReactNode;
}) {
  const resize = useContext(WorkspaceResizeContext);

  return (
    <aside className="arca-workspace-sidebar">
      <div className="arca-workspace-sidebar-head">
        <strong>{title}</strong>
        <span>{count} 项</span>
      </div>
      {actions && <div className="arca-workspace-sidebar-actions">{actions}</div>}
      <div className="arca-workspace-sidebar-scroll">{children}</div>
      {footer && <div className="arca-workspace-sidebar-footer">{footer}</div>}
      {resize && (
        <div
          className="arca-workspace-sidebar-resize"
          role="separator"
          aria-orientation="vertical"
          aria-label="调整侧边栏宽度"
          aria-valuemin={MIN_SIDEBAR_WIDTH}
          aria-valuemax={MAX_SIDEBAR_WIDTH}
          aria-valuenow={resize.sidebarWidth}
          tabIndex={0}
          data-resizing={resize.isResizing ? "true" : undefined}
          onPointerDown={resize.onPointerDown}
          onKeyDown={resize.onKeyDown}
        />
      )}
    </aside>
  );
}

/** The selected Provider/source stays visible while its resource cards scroll. */
export function ResourceOverview({
  eyebrow,
  title,
  description,
  icon,
  actions,
  children,
}: {
  eyebrow: string;
  title: string;
  description?: string;
  icon: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="arca-resource-overview">
      <div className="arca-resource-overview-heading">
        <span className="arca-resource-overview-icon">{icon}</span>
        <div className="arca-resource-overview-identity">
          <span className="arca-resource-overview-eyebrow">{eyebrow}</span>
          <h1 title={title}>{title}</h1>
          {description && <p>{description}</p>}
        </div>
        {actions && <div className="arca-resource-overview-actions">{actions}</div>}
      </div>
      {children && <div className="arca-resource-overview-details">{children}</div>}
    </header>
  );
}

export function SidebarSection({
  title,
  actions,
  children,
}: {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="arca-sidebar-section">
      <div className="arca-sidebar-section-head">
        <span>{title}</span>
        {actions && <div className="arca-sidebar-section-actions">{actions}</div>}
      </div>
      <div className="arca-sidebar-section-list">{children}</div>
    </section>
  );
}

export function SidebarItem({
  active,
  icon,
  label,
  title,
  count,
  actions,
  onClick,
}: {
  active: boolean;
  icon: ReactNode;
  label: string;
  title?: string;
  count?: number;
  actions?: ReactNode;
  onClick: () => void;
}) {
  return (
    <div
      className="arca-sidebar-item group"
      data-active={active ? "true" : undefined}
      title={title ?? label}
      role="button"
      tabIndex={0}
      aria-pressed={active}
      onClick={onClick}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onClick();
        }
      }}
    >
      <span className="arca-sidebar-item-icon">{icon}</span>
      <span className="arca-sidebar-item-label">{label}</span>
      {typeof count === "number" && (
        <span className={`arca-sidebar-item-count ${actions ? "group-hover:hidden" : ""}`}>
          {count}
        </span>
      )}
      {actions && (
        <span
          className="arca-sidebar-item-actions hidden group-hover:flex"
          onClick={(event) => event.stopPropagation()}
        >
          {actions}
        </span>
      )}
    </div>
  );
}

export function ResourceGrid({ children }: { children: ReactNode }) {
  return <div className="arca-resource-grid">{children}</div>;
}

export function ResourceEmpty({
  icon,
  title,
  detail,
  action,
}: {
  icon?: ReactNode;
  title: string;
  detail?: string;
  action?: ReactNode;
}) {
  return (
    <div className="arca-resource-empty">
      {icon && <span className="arca-resource-empty-icon">{icon}</span>}
      <strong>{title}</strong>
      {detail && <span>{detail}</span>}
      {action}
    </div>
  );
}

export function ResourceInspector({
  title,
  subtitle,
  avatar,
  onClose,
  children,
  footer,
}: {
  title: string;
  subtitle?: ReactNode;
  avatar: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
}) {
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (wasHandledByLayer(event) || document.querySelector(MODAL_DIALOG_SELECTOR)) return;
      onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  return (
    <aside
      className="arca-resource-inspector"
      data-resource-inspector
      data-modal-initial-focus
      tabIndex={-1}
      aria-label={`${title} 详情`}
    >
      <header className="arca-resource-inspector-head">
        {avatar}
        <div className="min-w-0 flex-1">
          <h2 title={title}>{title}</h2>
          {subtitle && <div className="arca-resource-inspector-subtitle">{subtitle}</div>}
        </div>
        <button type="button" className="arca-inspector-close" onClick={onClose} title="关闭" aria-label="关闭详情">
          <XIcon className="w-4 h-4" />
        </button>
      </header>
      <div className="arca-resource-inspector-body">{children}</div>
      {footer && <footer className="arca-resource-inspector-footer">{footer}</footer>}
    </aside>
  );
}

export function InspectorSection({
  title,
  icon,
  children,
}: {
  title: string;
  icon?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="arca-inspector-section">
      <h3>
        {icon && <span aria-hidden="true">{icon}</span>}
        {title}
      </h3>
      {children}
    </section>
  );
}

export function InspectorMetrics({ children }: { children: ReactNode }) {
  return <div className="arca-inspector-metrics">{children}</div>;
}

export function InspectorMetric({
  icon,
  label,
  value,
}: {
  icon: ReactNode;
  label: string;
  value: ReactNode;
}) {
  return (
    <div className="arca-inspector-metric" title={label}>
      <span className="arca-inspector-metric-icon" aria-hidden="true">{icon}</span>
      <span>
        <strong>{value}</strong>
        <small>{label}</small>
      </span>
    </div>
  );
}

export function InspectorField({
  label,
  icon,
  children,
  mono,
  action,
  wide,
}: {
  label: string;
  icon?: ReactNode;
  children: ReactNode;
  mono?: boolean;
  action?: ReactNode;
  wide?: boolean;
}) {
  return (
    <div className="arca-inspector-field" data-wide={wide ? "true" : undefined}>
      <span className="arca-inspector-field-icon" data-empty={!icon ? "true" : undefined} aria-hidden="true">{icon}</span>
      <span className="arca-inspector-field-label">{label}</span>
      <div className={["arca-inspector-field-value", mono && "arca-inspector-field-mono"].filter(Boolean).join(" ")}>{children}</div>
      {action && <div className="arca-inspector-field-action">{action}</div>}
    </div>
  );
}

export function AgentStack({ ids: allIds, max = 4 }: { ids: string[]; max?: number }) {
  const ids = allIds.filter(isAgentVisible);
  if (allIds.length > 0 && ids.length === 0) return null;
  if (ids.length === 0) return <span className="arca-resource-usage-empty">未连接</span>;
  const visible = ids.slice(0, max);
  return (
    <span className="arca-agent-stack" title={ids.map((id) => agentName(id)).join("、")}>
      <span className="arca-agent-stack-icons">
        {visible.map((id) => (
          <span className="arca-agent-stack-icon" key={id}>
            <AgentGlyph id={id} size={20} />
          </span>
        ))}
      </span>
      <span>{ids.length} 个 Agent</span>
    </span>
  );
}
