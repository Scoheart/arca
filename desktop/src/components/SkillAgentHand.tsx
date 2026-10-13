import { useMemo, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { AgentGlyph, agentName, isAgentVisible } from "./brandIcons";
import { SkillAgentOrbit } from "./SkillAgentOrbit";

export function SkillAgentHand({ ids: allIds, names, skillName, onOpenAgent }: {
  ids: string[];
  names: ReadonlyMap<string, string>;
  skillName?: string;
  onOpenAgent?: (id: string) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const handRef = useRef<HTMLDivElement>(null);
  const ids = useMemo(() => allIds.filter(isAgentVisible), [allIds]);
  if (allIds.length > 0 && ids.length === 0) return null;
  if (!ids.length) return <span className="arca-skill-unassigned">{t("skillLibrary.unassigned")}</span>;
  const visible = ids.slice(0, 4);
  const remaining = ids.length - visible.length;
  const count = visible.length + (remaining ? 1 : 0);
  const cardStyle = (index: number): CSSProperties => ({
    "--card-index": index,
    "--card-rotation": `${(index - (count - 1) / 2) * 7}deg`,
    "--card-offset": `${Math.abs(index - (count - 1) / 2) * 1.5}px`,
  } as CSSProperties);
  const open = (id: string) => { setExpanded(false); onOpenAgent?.(id); };
  return <>
    <div ref={handRef} className="arca-skill-agent-hand" role="group" aria-label={t("skillLibrary.agents")}
      data-expanded={expanded || undefined}
      style={{ "--card-count": count } as CSSProperties}>
      {visible.map((id, index) => {
        const name = agentName(id, names.get(id));
        return <button key={id} type="button" className="arca-skill-agent-card"
          style={cardStyle(index)} data-agent-name={name} data-agent-id={id}
          aria-label={t("skillLibrary.openAgent", { name })} onClick={() => open(id)}>
          <span aria-hidden="true"><AgentGlyph id={id} name={name} size={26} /></span>
        </button>;
      })}
      {remaining > 0 && <button type="button" className="arca-skill-agent-card arca-skill-agent-more"
        style={cardStyle(visible.length)} data-agent-name={t("skillLibrary.allAgents")}
        aria-label={t("skillLibrary.moreAgents", { count: remaining })}
        aria-expanded={expanded} onClick={(event) => {
          // Warm the same module used by App's lazy AgentView while the cards are dealt.
          void import("./AgentView").catch(() => undefined);
          event.currentTarget.focus();
          setExpanded(true);
        }}>
        +{remaining}
      </button>}
    </div>
    {expanded && <SkillAgentOrbit ids={ids} names={names} skillName={skillName} sourceRef={handRef}
      onExit={(id) => { setExpanded(false); if (id) onOpenAgent?.(id); }} />}
  </>;
}
