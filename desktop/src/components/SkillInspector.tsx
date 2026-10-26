import { ToastNotice } from "./Toast";
import type { PlanRepairRequest, SkillCommandError, SkillDetail, SkillInventoryItem } from "../lib/types";
import { ResourceInspector } from "./ResourceWorkspace";
import { skillSourceText } from "./SkillCard";
import { Avatar, Badge } from "./ui";
import { LinkIcon } from "./icons";

export type SkillLifecycleIntent =
  | {
      kind: "import";
      identity: string;
      replaceConflicts: boolean;
    }
  | { kind: "update"; skillName: string; replaceLocalChanges: boolean }
  | { kind: "remove"; skillName: string }
  | {
      kind: "repair";
      skillName: string;
      repair: PlanRepairRequest["repair"];
    };

function sourceKindLabel(item: SkillInventoryItem) {
  if (!item.source) return "外部副本";
  if (item.source.kind === "github") return "GitHub";
  if (item.source.kind === "local") return "本地";
  if (item.source.kind === "archive") return "压缩包";
  return "Imported";
}

function shortDate(value: string | null) {
  if (!value) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString();
}

// Metadata is already supplied by Core; the reading area shows only the body.
function skillBody(text: string) {
  return text.replace(/^\uFEFF?---[ \t]*\r?\n[\s\S]*?\r?\n---[ \t]*(?:\r?\n|$)/, "").trimStart();
}

export function SkillInspector({
  item, detail, loading, error, onClose,
}: {
  item: SkillInventoryItem;
  copies?: SkillInventoryItem[];
  onSelectCopy?: (identity: string) => void;
  detail: SkillDetail | null;
  loading: boolean;
  error: SkillCommandError | null;
  onClose: () => void;
  onPlan?: (intent: SkillLifecycleIntent) => void;
  planning?: boolean;
  readOnly?: boolean;
}) {
  return (
    <ResourceInspector
      title={item.name}
      avatar={<Avatar seed={item.name} kind="skill" size={40} />}
      subtitle={<Badge tone={item.source?.kind === "github" ? "info" : "neutral"}>{sourceKindLabel(item)}</Badge>}
      onClose={onClose}
    >
      <article className="arca-skill-reading" key={item.identity}>
        {item.description && <p className="arca-skill-reading-description">{item.description}</p>}
        <div className="arca-skill-detail-source">
          <LinkIcon className="w-3.5 h-3.5" />
          <span>{item.source?.kind === "github" ? `${item.source.owner}/${item.source.repo}` : skillSourceText(item.source)}</span>
          {item.updated_at && <small>更新于 {shortDate(item.updated_at)}</small>}
        </div>
        <section className="arca-skill-reading-body" aria-label="Skill 正文">
          {loading ? <p role="status">正在读取正文…</p>
            : error ? <ToastNotice message={`读取失败：${error.message}`} />
            : detail ? <>
              {detail.skill_md_truncated && <p className="arca-skill-inspector-truncation">正文预览已截断</p>}
              <pre>{skillBody(detail.skill_md) || "暂无正文。"}</pre>
            </> : <p>尚未加载正文。</p>}
        </section>
      </article>
    </ResourceInspector>
  );
}
