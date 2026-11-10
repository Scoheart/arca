import { ToastNotice } from "./Toast";
import { useEffect, useState } from "react";
import type { PlanRepairRequest, SkillCommandError, SkillDetail, SkillInventoryItem } from "../lib/types";
import { ResourceInspector } from "./ResourceWorkspace";
import { skillSourceText } from "./SkillCard";
import { Avatar, Badge } from "./ui";
import { LinkIcon } from "./icons";
import { FormSelect } from "./FormSelect";

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
  item, copies = [item], onSelectCopy, detail, loading, error, onClose,
  onPlan, planning = false, readOnly = false,
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
  const [replaceConflicts, setReplaceConflicts] = useState(false);
  const [replaceLocalChanges, setReplaceLocalChanges] = useState(false);
  useEffect(() => {
    setReplaceConflicts(false);
    setReplaceLocalChanges(false);
  }, [item.identity]);
  const central = item.location.kind === "central";
  const external = !central && item.states.includes("external");
  const needsRepair = item.states.some((state) =>
    state === "missing" || state === "broken_link" || state === "conflicting_link");
  const canUpdate = central && item.source && item.source.kind !== "imported";
  const blocked = readOnly || planning || loading;
  const submit = (intent: SkillLifecycleIntent) => {
    if (!blocked) onPlan?.(intent);
  };
  const visibleCopies = copies.filter((copy) => copy.name === item.name);

  return (
    <ResourceInspector
      title={item.name}
      avatar={<Avatar seed={item.name} kind="skill" size={40} />}
      subtitle={<Badge tone={item.source?.kind === "github" ? "info" : "neutral"}>{sourceKindLabel(item)}</Badge>}
      onClose={onClose}
      footer={onPlan ? <div className="flex gap-2">
        {external ? <button type="button" className="btn-primary" disabled={blocked}
          onClick={() => submit({ kind: "import", identity: item.identity, replaceConflicts })}>导入</button> : null}
        {canUpdate ? <button type="button" className="btn-secondary" disabled={blocked}
          onClick={() => submit({ kind: "update", skillName: item.name, replaceLocalChanges })}>更新</button> : null}
        {needsRepair && !external ? <button type="button" className="btn-secondary" disabled={blocked}
          onClick={() => submit({ kind: "repair", skillName: item.name,
            repair: item.location.kind === "central" ? { kind: "central" }
              : { kind: "target", target_id: item.location.target_id } })}>修复</button> : null}
        {central ? <button type="button" className="btn-danger" disabled={blocked}
          onClick={() => submit({ kind: "remove", skillName: item.name })}>删除</button> : null}
      </div> : undefined}
    >
      <article className="arca-skill-reading" key={item.identity}>
        {onSelectCopy && visibleCopies.length > 1 && <FormSelect ariaLabel="Skill 位置" value={item.identity}
          disabled={planning} options={visibleCopies.map((copy) => ({ value: copy.identity,
            label: copy.location.kind === "central" ? "中央副本" : copy.location.global_dir }))}
          onChange={(identity) => {
            if (!planning && visibleCopies.some((copy) => copy.identity === identity)) onSelectCopy(identity);
          }} />}
        {onPlan && external && <label>
          <input type="checkbox" checked={replaceConflicts} disabled={blocked}
            onChange={(event) => setReplaceConflicts(event.currentTarget.checked)} />备份并替换同名中央副本
        </label>}
        {onPlan && canUpdate && item.states.includes("locally_modified") && <label>
          <input type="checkbox" checked={replaceLocalChanges} disabled={blocked}
            onChange={(event) => setReplaceLocalChanges(event.currentTarget.checked)} />备份并替换本地修改
        </label>}
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
