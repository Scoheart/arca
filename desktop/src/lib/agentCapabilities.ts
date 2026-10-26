import type { AgentCapabilityView, AgentInfo } from "./types";

function supportedTransports(
  values: string[] | undefined,
): Array<"stdio" | "http"> {
  return (values ?? []).filter(
    (value): value is "stdio" | "http" =>
      value === "stdio" || value === "http",
  );
}

function projectedAgentInfo(
  projection: AgentCapabilityView,
  legacy: AgentInfo | undefined,
): AgentInfo {
  const { identity, capabilities } = projection;
  const mcp = capabilities.mcp ?? null;
  const skill = capabilities.skill ?? null;
  const skillsGlobalDirs = skill
    ? [skill.global_dir, ...skill.alias_dirs]
    : [];

  return {
    id: identity.id,
    name: identity.name,
    format: mcp?.format ?? "",
    key: mcp?.key ?? "",
    has_global: Boolean(mcp?.config_path),
    has_project: legacy?.has_project ?? false,
    has_model: capabilities.model != null,
    has_instructions: capabilities.instructions ?? false,
    enabled: identity.enabled,
    supported_transports: supportedTransports(mcp?.supported_transports),
    global: mcp?.config_path ?? null,
    project: legacy?.project ?? null,
    skills_global_dir: skill?.global_dir ?? null,
    skills_global_dirs: skillsGlobalDirs,
    docs: identity.docs ?? null,
    note: identity.note ?? null,
    category: identity.category,
    evidence: identity.evidence,
    verified_at: identity.verified_at ?? null,
    builtin: identity.builtin,
  };
}

/**
 * Keep the stable legacy ordering while making the cross-domain workspace
 * projection authoritative for identity and capabilities. Projection-only
 * Model and Skill Agents are appended so navigation never depends on an
 * MCP-shaped row existing first.
 */
export function mergeAgentInfos(
  legacyAgents: AgentInfo[],
  projectedAgents: AgentCapabilityView[],
): AgentInfo[] {
  const projectedById = new Map(
    projectedAgents.map((agent) => [agent.identity.id, agent]),
  );
  const seen = new Set<string>();
  const merged = legacyAgents.map((legacy) => {
    const projection = projectedById.get(legacy.id);
    if (!projection) return legacy;
    seen.add(legacy.id);
    return projectedAgentInfo(projection, legacy);
  });
  for (const projection of projectedAgents) {
    if (seen.has(projection.identity.id)) continue;
    merged.push(projectedAgentInfo(projection, undefined));
  }
  return merged;
}
