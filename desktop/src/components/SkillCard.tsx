import type {
  RiskLevel,
  SkillInventoryItem,
  SkillSource,
} from "../lib/types";
import { useTranslation } from "react-i18next";
import { SkillAgentHand } from "./SkillAgentHand";
import { Badge } from "./ui";

function appendSubpath(base: string, subpath: string) {
  return subpath ? `${base} / ${subpath}` : base;
}

export function skillSourceText(source: SkillSource | null) {
  if (!source) return "外部副本 · 来源未知";
  if (source.kind === "github") {
    return appendSubpath(`GitHub · ${source.owner}/${source.repo}`, source.subpath);
  }
  if (source.kind === "local") {
    return appendSubpath(`本地 · ${source.path}`, source.subpath);
  }
  if (source.kind === "archive") {
    return appendSubpath(`压缩包 · ${source.path}`, source.subpath);
  }
  return `导入副本 · ${source.original_path}`;
}

export function skillRiskLabel(level: RiskLevel | null) {
  if (level === "high") return "高风险";
  if (level === "medium") return "中风险";
  if (level === "low") return "未发现高风险模式";
  return "尚未检查";
}

export function SkillRiskBadge({
  level,
  label,
}: {
  level: RiskLevel | null;
  label?: string;
}) {
  return (
    <span
      className="arca-skill-risk-badge"
      data-level={level ?? "unreviewed"}
    >
      {label ?? skillRiskLabel(level)}
    </span>
  );
}

const attentionStates = new Set([
  "locally_modified",
  "broken_link",
  "conflicting_link",
  "missing",
]);

function skillAssetState(item: SkillInventoryItem) {
  if (
    item.update.available ||
    item.risk?.level === "high" ||
    item.states.some((state) => attentionStates.has(state))
  ) {
    return { labelKey: "needsAttention" as const, tone: "warning" as const };
  }
  if (item.states.includes("external")) return { labelKey: "external" as const, tone: "info" as const };
  return { labelKey: "normal" as const, tone: "success" as const };
}

export function SkillCard({ item, selected, onOpen, agentIds = [], agentNames = new Map(), onOpenAgent }: {
  item: SkillInventoryItem;
  selected: boolean;
  onOpen: () => void;
  agentIds?: string[];
  agentNames?: ReadonlyMap<string, string>;
  onOpenAgent?: (id: string) => void;
}) {
  const { t } = useTranslation();
  const assetState = skillAssetState(item);
  const status = item.update.error
    ? { labelKey: "updateFailed" as const, tone: "warning" as const }
    : item.update.available
      ? { labelKey: "updateAvailable" as const, tone: "info" as const }
      : assetState.labelKey !== "normal" ? assetState : null;
  return (
    <article className="arca-skill-card" data-selected={selected ? "true" : undefined}>
      <button type="button" className="arca-skill-card-main"
        aria-label={t("centralAssets.openSkillDetails", { name: item.name })}
        aria-pressed={selected} onClick={onOpen}>
        <h2 title={item.name}>{item.name}</h2>
        <span className="arca-skill-card-description" title={item.description}>
          {item.description || t("centralAssets.noDescription")}
        </span>
      </button>
      <div className="arca-skill-card-footer">
        <SkillAgentHand ids={agentIds} names={agentNames} skillName={item.name} onOpenAgent={onOpenAgent} />
        {status && <Badge tone={status.tone}>{t(`centralAssets.${status.labelKey}`)}</Badge>}
      </div>
    </article>
  );
}
