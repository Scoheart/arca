import i18n from "../i18n";
import { useTranslation } from "react-i18next";
import { ResourceIcon } from "./resourcePresentation";
import { type ReactNode, useMemo, useState } from "react";
import type { InstallState } from "../hooks/useInstallState";
import type { SourceView } from "../lib/types";
import { IconButton } from "./ui";
import { CloudIcon, FolderIcon, PlusIcon, RefreshIcon, TagIcon, TrashIcon, EditIcon } from "./icons";
import { useToast } from "./Toast";
import { formatError } from "../lib/format";
import { SidebarItem, SidebarSection, WorkspaceSidebar } from "./ResourceWorkspace";
import { ReviewDialog } from "./ReviewDialog";

export interface McpTagSummary {
  name: string;
  count: number;
}

/** Sort rank: remote (0) → user local (1) → managed manual/discovered (2). */
function rank(s: SourceView): number {
  if (s.kind === "remote") return 0;
  return s.managed ? 2 : 1;
}

function kindIconOf(s: SourceView) {
  if (s.kind === "remote") return <CloudIcon className="w-3.5 h-3.5" />;
  return <FolderIcon className="w-3.5 h-3.5" />;
}

/**
 * Left column that organizes the MCP catalog by source and user-defined tags.
 */
export function SourcesSidebar({
  state,
  selectedId,
  selectedTag = null,
  tags = [],
  onSelect,
  onSelectTag,
  onManageTags,
  onCreate,
  footer,
}: {
  state: InstallState;
  /** null = 全部 (all sources). */
  selectedId: string | null;
  selectedTag?: string | null;
  tags?: McpTagSummary[];
  onSelect: (id: string | null) => void;
  onSelectTag?: (tag: string | null) => void;
  onManageTags?: (tag?: string | null) => void;
  onCreate: () => void;
  footer?: ReactNode;
}) {
  useTranslation();
  const { sources, catalog } = state;
  const toast = useToast();
  const [busyId, setBusyId] = useState<string | null>(null);
  const [deleteReview, setDeleteReview] = useState<SourceView | null>(null);

  const visibleSources = useMemo(
    () =>
      [...sources]
        .filter((s) => !s.managed)
        .sort(
          (a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
        ),
    [sources]
  );

  const doRefresh = async (s: SourceView) => {
    setBusyId(s.id);
    try {
      await state.refreshOneSource(s.id);
      toast.show({ kind: "success", msg: i18n.t("desktop.sourcesSidebar.refreshed", { value0: s.name }) });
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
      actions={
        <button className="btn-primary" type="button" onClick={onCreate}>
          <PlusIcon className="w-4 h-4" />
          <span>{i18n.t("desktop.sourcesSidebar.addMCPs")}</span>
        </button>
      }
    >
      <SidebarSection title={i18n.t("desktop.sourcesSidebar.source")}>
        <SidebarItem
          active={selectedId === null && selectedTag === null}
          icon={<ResourceIcon domain="mcp" className="w-3.5 h-3.5" />}
          label={i18n.t("desktop.sourcesSidebar.allSources")}
          count={catalog.length}
          onClick={() => onSelect(null)}
        />
        {visibleSources.map((source) => (
          <SidebarItem
            key={source.id}
            active={selectedId === source.id}
            icon={kindIconOf(source)}
            label={source.name}
            count={source.server_count}
            onClick={() => onSelect(source.id)}
            actions={
              <>
                <IconButton
                  title={i18n.t("desktop.sourcesSidebar.refreshSource")}
                  onClick={() => doRefresh(source)}
                  disabled={busyId === source.id}
                >
                  <RefreshIcon className="w-3.5 h-3.5" style={busyId === source.id ? { animation: "spin 0.8s linear infinite" } : undefined} />
                </IconButton>
                <IconButton title={i18n.t("desktop.sourcesSidebar.deleteSource")} onClick={() => setDeleteReview(source)} disabled={busyId === source.id}>
                  <TrashIcon className="w-3.5 h-3.5" />
                </IconButton>
              </>
            }
          />
        ))}
      </SidebarSection>

      <SidebarSection
        title={i18n.t("desktop.sourcesSidebar.tags")}
        actions={
          onManageTags && (
            <IconButton
              title={i18n.t("desktop.sourcesSidebar.createTag")}
              onClick={() => onManageTags(null)}
            >
              <PlusIcon className="w-3.5 h-3.5" />
            </IconButton>
          )
        }
      >
        {tags.length === 0 ? (
          <div className="arca-sidebar-empty">{i18n.t("desktop.sourcesSidebar.noTags")}</div>
        ) : (
          tags.map((tag) => (
            <SidebarItem
              key={tag.name}
              active={selectedTag !== null && selectedTag.toLowerCase() === tag.name.toLowerCase()}
              icon={<TagIcon className="w-3.5 h-3.5" />}
              label={tag.name}
              count={tag.count}
              onClick={() => onSelectTag?.(tag.name)}
              actions={
                onManageTags && (
                  <IconButton
                    title={i18n.t("desktop.sourcesSidebar.editTag", { value0: tag.name })}
                    onClick={() => onManageTags(tag.name)}
                  >
                    <EditIcon className="w-3.5 h-3.5" />
                  </IconButton>
                )
              }
            />
          ))
        )}
      </SidebarSection>
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
