import protocolDefinitions from "../../../data/model-protocols.json";
import type { ModelAgentView } from "../lib/types";
import { AgentResourceActions } from "./AgentResourcePanel";
import { ExternalLinkIcon } from "./icons";
import { Badge } from "./ui";

export function GuidedModelsPanel({ agent, onOpenDocs }: {
  agent: ModelAgentView;
  onOpenDocs(): void;
}) {
  return (
    <section className="arca-guided-models" aria-label={`${agent.name} 模型配置引导`}>
      <AgentResourceActions>
        <button type="button" className="btn-secondary" onClick={onOpenDocs}>
          <ExternalLinkIcon className="w-3.5 h-3.5" />查看配置步骤
        </button>
      </AgentResourceActions>
      <div className="arca-guided-models-heading">
        <h4>{agent.name} 自定义模型</h4>
        <Badge tone="neutral">在应用中配置</Badge>
      </div>
      <p>{agent.note}</p>
      {agent.supported_protocols.length > 0 && (
        <div className="arca-guided-models-protocols" aria-label="原生支持的协议">
          {agent.supported_protocols.map((protocol) => (
            <Badge key={protocol} tone="neutral">
              {protocolDefinitions.find((definition) => definition.id === protocol)?.label ?? protocol}
            </Badge>
          ))}
        </div>
      )}
    </section>
  );
}
