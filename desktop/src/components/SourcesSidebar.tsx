import i18n from "../i18n";
import { useTranslation } from "react-i18next";
import { ResourceIcon } from "./resourcePresentation";
import { type ReactNode, useMemo, useState } from "react";
import type { InstallState } from "../hooks/useInstallState";
import type { SourceView } from "../lib/types";
import { IconButton } from "./ui";
import { CloudIcon, FolderIcon, PlusIcon, RefreshIcon, TrashIcon, EditIcon, SearchIcon } from "./icons";
import { SubscribeDialog } from "./SubscribeDialog";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";
import { SidebarItem, SidebarSection, WorkspaceSidebar } from "./ResourceWorkspace";
import { ReviewDialog } from "./ReviewDialog";

/** Sort rank: remote (0) → user local (1) → managed manual/discovered (2). */
function rank(s: SourceView): number {
  if (s.kind === "remote") return 0;
  return s.managed ? 2 : 1;
}

function kindIconOf(s: SourceView) {
  if (s.kind === "remote") return <CloudIcon className="w-3.5 h-3.5" />;      // 订阅（远程 URL）
  if (s.managed) {
    // 手动添加（用户手建/编辑）vs 自动探索（扫描各 agent 发现）
    return s.id === "discovered" ? <SearchIcon className="w-3.5 h-3.5" /> : <EditIcon className="w-3.5 h-3.5" />;
  }
  return <FolderIcon className="w-3.5 h-3.5" />;                              // 本地文件
}

/**
 * Left column that both *organizes* the catalog (click a source to filter the
 * grid) and *manages* sources (subscribe / import, plus per-source
 * enable / refresh / remove). Replaces the old standalone 来源 page.
 */
export function SourcesSidebar({
  state,
  selectedId,
  onSelect,
  onCreate,
  footer,
}: {
  state: InstallState;
  /** null = 全部 (all sources). */
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCreate: () => void;
  footer?: ReactNode;
}) {
  useTranslation();
  const { sources, catalog } = state;
  const toast = useToast();
  const [subscribeOpen, setSubscribeOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [deleteReview, setDeleteReview] = useState<SourceView | null>(null);

  const sorted = useMemo(
    () =>
      [...sources].sort(
        (a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
      ),
    [sources]
  );

  const pickLocal = async () => {
    try {
      const v = await state.pickLocalSource();
      if (v) toast.show({ kind: "success", msg: i18n.t("desktop.sourcesSidebar.importedItems", { value0: v.name, value1: v.server_count }) });
    } catch (e) {
      toast.show({ kind: "error", msg: i18n.t("desktop.sourcesSidebar.importFailed") + formatError(e) });
    }
  };

  const doRefresh = async (s: SourceView) => {
    setBusyId(s.id);
    try {
      if (s.id === "discovered") {
        await state.rescanDiscovered();
        toast.show({ kind: "success", msg: i18n.t("desktop.sourcesSidebar.agentConfigurationsRediscovered") });
      } else {
        await state.refreshOneSource(s.id);
        toast.show({ kind: "success", msg: i18n.t("desktop.sourcesSidebar.refreshed", { value0: s.name }) });
      }
    } catch (e) {
      toast.show({ kind: "error", msg: i18n.t("desktop.sourcesSidebar.refreshFailed") + formatError(e) });
    } finally {
      setBusyId(null);
    }
  };

  const doRemove = async (s: SourceView) => {
    setBusyId(s.id);
    try {
      await state.deleteSource(s.id);
      if (selectedId === s.id) onSelect(null);
      setDeleteReview(null);
      toast.show({ kind: "success", msg: i18n.t("desktop.sourcesSidebar.sourceDeleted", { value0: s.name }) });
    } catch (e) {
      toast.show({ kind: "error", msg: i18n.t("desktop.sourcesSidebar.deleteFailed") + formatError(e) });
      throw e;
    } finally {
      setBusyId(null);
    }
  };

  return (
    <>
    <WorkspaceSidebar title="MCPs" count={catalog.length} footer={footer}
      actions={<>
        <button className="btn-primary" type="button" onClick={onCreate}>
          <PlusIcon className="w-4 h-4" />
          <span>{i18n.t("desktop.sourcesSidebar.addMCPs")}</span></button>
        <div className="arca-sidebar-action-pair">
          <button className="btn-secondary" type="button" onClick={() => setSubscribeOpen(true)}>
            <CloudIcon className="w-4 h-4" />
            <span>{i18n.t("desktop.sourcesSidebar.addSubscription")}</span></button>
          <button className="btn-secondary" type="button" onClick={pickLocal}>
            <FolderIcon className="w-4 h-4" />
            <span>{i18n.t("desktop.sourcesSidebar.importConfiguration")}</span></button>
        </div>
      </>}
    >
      <SidebarSection title={i18n.t("desktop.sourcesSidebar.source")}>
        <SidebarItem
          active={selectedId === null}
          icon={<ResourceIcon domain="mcp" className="w-3.5 h-3.5" />}
          label={i18n.t("desktop.sourcesSidebar.allSources")}
          count={catalog.length}
          onClick={() => onSelect(null)}
        />
        {sorted.length === 0 ? (
          <div className="arca-sidebar-empty">{i18n.t("desktop.sourcesSidebar.noSources")}</div>
        ) : (
          sorted.map((source) => (
            <SidebarItem
              key={source.id}
              active={selectedId === source.id}
              icon={kindIconOf(source)}
              label={source.name}
              count={source.server_count}
              onClick={() => onSelect(source.id)}
              actions={
                <>
                  {(source.kind === "remote" || !source.managed || source.id === "discovered") && (
                    <IconButton
                      title={source.id === "discovered" ? i18n.t("desktop.sourcesSidebar.rediscover") : i18n.t("desktop.sourcesSidebar.refreshSource")}
                      onClick={() => doRefresh(source)}
                      disabled={busyId === source.id}
                    >
                      <RefreshIcon className="w-3.5 h-3.5" style={busyId === source.id ? { animation: "spin 0.8s linear infinite" } : undefined} />
                    </IconButton>
                  )}
                  {!source.managed && (
                    <IconButton title={i18n.t("desktop.sourcesSidebar.deleteSource")} onClick={() => setDeleteReview(source)} disabled={busyId === source.id}>
                      <TrashIcon className="w-3.5 h-3.5" />
                    </IconButton>
                  )}
                </>
              }
            />
          ))
        )}
      </SidebarSection>

      {subscribeOpen && (
        <SubscribeDialog
          state={state}
          onClose={() => setSubscribeOpen(false)}
        />
      )}
    </WorkspaceSidebar>
    {deleteReview && (
      <ReviewDialog
        title={i18n.t("desktop.sourcesSidebar.deleteSource")}
        subtitle={deleteReview.name}
        confirmLabel={i18n.t("desktop.sourcesSidebar.deleteSource")}
        onClose={() => setDeleteReview(null)}
        onConfirm={() => doRemove(deleteReview)}
      >
        <p>{i18n.t("desktop.sourcesSidebar.deletesTheSourceRecordAndLocalCacheMCP")}</p>
      </ReviewDialog>
    )}
    </>
  );
}
