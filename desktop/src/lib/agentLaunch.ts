import { invoke } from "@tauri-apps/api/core";
import type { StartupAgentQuickOption } from "./types";

export type LaunchTarget = { kind: "app"; path: string; args?: string[]; new_instance?: boolean; env?: Record<string, string> }
  | { kind: "cli"; command: string; args: string[]; env?: Record<string, string> }
  | { kind: "web"; url: string };
export interface LaunchProfile {
  id: string;
  name: string;
  target: LaunchTarget | null;
  default_directory: string | null;
}
export interface AgentLaunchInfo {
  agent_id: string;
  name: string;
  category: string;
  kind: "app" | "cli" | "web" | null;
  available: boolean;
  installed?: boolean | null;
  supported: boolean;
  host_name: string | null;
  install_url: string | null;
  directory: string | null;
  default_directory?: string | null;
  directory_exists: boolean;
  configured_target: LaunchTarget | null;
  resolved_target: LaunchTarget | null;
  default_target?: LaunchTarget | null;
  profiles?: LaunchProfile[];
  active_profile_id?: string;
  profiles_revision?: string;
  active_startup_agent?: string | null;
  startup_agents?: StartupAgentQuickOption[];
}
export const getAgentLaunchInfo = (agentId: string) => invoke<AgentLaunchInfo>("get_agent_launch_info", { agentId });
export const getAgentRuntimeVersion = (agentId: string) => invoke<string | null>("get_agent_runtime_version", { agentId });
export const configureAgentLaunch = (agentId: string, target: LaunchTarget | null, defaultDirectory?: string) => invoke<AgentLaunchInfo>("configure_agent_launch", { agentId, target, defaultDirectory });
export const configureAgentLaunchProfiles = (agentId: string, profiles: LaunchProfile[], activeProfileId: string, expectedRevision: string) =>
  invoke<AgentLaunchInfo>("configure_agent_launch_profiles", { agentId, profiles, activeProfileId, expectedRevision });
export const launchAgent = (agentId: string, directory: string | null) => invoke<{ directory_saved: boolean }>("launch_agent", { agentId, directory });
export const launchAgentWithStartupAgent = (agentId: string, directory: string | null, startupAgent: string | null) =>
  invoke<{ directory_saved: boolean }>("launch_agent_with_startup_agent", { agentId, directory, startupAgent });

export function splitLaunchArguments(text: string): string[] {
  const result: string[] = [];
  const lines = text.split(/\r?\n/);
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    let current = "";
    let inSingleQuote = false;
    let inDoubleQuote = false;
    let hasToken = false;

    for (let i = 0; i < trimmed.length; i++) {
      const ch = trimmed[i];
      if (ch === "'" && !inDoubleQuote) {
        inSingleQuote = !inSingleQuote;
        hasToken = true;
      } else if (ch === '"' && !inSingleQuote) {
        inDoubleQuote = !inDoubleQuote;
        hasToken = true;
      } else if (ch === "\\" && inDoubleQuote) {
        const next = trimmed[i + 1];
        if (next === '"' || next === "\\" || next === "$" || next === "`") {
          current += next;
          i++;
        } else {
          current += "\\";
        }
        hasToken = true;
      } else if (ch === "\\" && !inSingleQuote) {
        const next = trimmed[i + 1];
        if (next !== undefined) {
          current += next;
          i++;
          hasToken = true;
        }
      } else if ((ch === " " || ch === "\t") && !inSingleQuote && !inDoubleQuote) {
        if (hasToken || current.length > 0) {
          result.push(current);
          current = "";
          hasToken = false;
        }
      } else {
        current += ch;
        hasToken = true;
      }
    }
    if (hasToken || current.length > 0) {
      result.push(current);
    }
  }
  return result;
}
