import { ResourceIcon } from "./resourcePresentation";
import type { SkillsState } from "../hooks/useSkillsState";
import type { SkillNavigationRequest } from "../lib/types";

/**
 * Compatibility shell for older embedders. Production Agent pages use
 * AgentConsumptionPanel; this component deliberately exposes no install or
 * assignment mutation surface.
 */
export function AgentSkillsSection({
  agentId,
  state,
}: {
  agentId: string;
  state: SkillsState;
  onOpenSkills?: (request: SkillNavigationRequest) => void;
}) {
  const agent = state.inventory?.agents.find((candidate) => candidate.id === agentId);
  return (
    <section className="arca-agent-section arca-agent-resource-content">
      <div className="arca-consumption-empty">
        <ResourceIcon domain="skill" className="w-7 h-7" />
        <strong>请使用中央 Skills 选择器</strong>
        <span>
          {agent
            ? `已核验目标：${agent.global_dir}`
            : "此处不再安装或分配 Skill；消费关系由 AgentConsumptionPanel 统一管理。"}
        </span>
      </div>
    </section>
  );
}
