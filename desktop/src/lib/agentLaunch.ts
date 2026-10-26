import { invoke } from "@tauri-apps/api/core";

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
}
export const getAgentLaunchInfo = (agentId: string) => invoke<AgentLaunchInfo>("get_agent_launch_info", { agentId });
export const getAgentRuntimeVersion = (agentId: string) => invoke<string | null>("get_agent_runtime_version", { agentId });
export const configureAgentLaunch = (agentId: string, target: LaunchTarget | null, defaultDirectory?: string) => invoke<AgentLaunchInfo>("configure_agent_launch", { agentId, target, defaultDirectory });
export const configureAgentLaunchProfiles = (agentId: string, profiles: LaunchProfile[], activeProfileId: string, expectedRevision: string) =>
  invoke<AgentLaunchInfo>("configure_agent_launch_profiles", { agentId, profiles, activeProfileId, expectedRevision });
export const launchAgent = (agentId: string, directory: string | null) => invoke<{ directory_saved: boolean }>("launch_agent", { agentId, directory });
