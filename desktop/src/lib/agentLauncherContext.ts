import { createContext, useContext } from "react";

export const AgentLauncherContext = createContext<{
  busyId: string | null;
  revision: number;
  launch(id: string, chooseDirectory?: boolean): Promise<void>;
  launchWithStartupAgent(id: string, startupAgent: string | null): Promise<void>;
  configure(id: string): Promise<void>;
  refresh(): void;
} | null>(null);

export function useAgentLauncher() {
  const value = useContext(AgentLauncherContext);
  if (!value) throw new Error("AgentLauncherProvider is missing");
  return value;
}
