import { BrainIcon, HookMarkIcon, McpMarkIcon, SkillMarkIcon, SubagentMarkIcon } from "./icons";

/** One presentation contract for resource navigation, settings and empty states. */
export const RESOURCE_CATEGORIES = [
  { id: "models", domain: "model", view: "models", label: "Models", Icon: BrainIcon },
  { id: "mcps", domain: "mcp", view: "registry", label: "MCPs", Icon: McpMarkIcon },
  { id: "skills", domain: "skill", view: "skills", label: "Skills", Icon: SkillMarkIcon },
  { id: "subagents", domain: "subagent", view: "subagents", label: "Subagents", Icon: SubagentMarkIcon },
  { id: "hooks", domain: "hook", view: "hooks", label: "Hooks", Icon: HookMarkIcon },
] as const;

export type ResourceCategoryId = typeof RESOURCE_CATEGORIES[number]["id"];
export type ResourceDomain = typeof RESOURCE_CATEGORIES[number]["domain"];
export const RESOURCE_PRESENTATION = {
  model: RESOURCE_CATEGORIES[0],
  mcp: RESOURCE_CATEGORIES[1],
  skill: RESOURCE_CATEGORIES[2],
  subagent: RESOURCE_CATEGORIES[3],
  hook: RESOURCE_CATEGORIES[4],
} as const;

export function ResourceIcon({ domain, className = "w-4 h-4" }: {
  domain: ResourceDomain;
  className?: string;
}) {
  const Icon = RESOURCE_PRESENTATION[domain].Icon;
  return <Icon className={className} />;
}
