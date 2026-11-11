import type {
  ResourceNavigationIntent,
  ResourceNavigationRequest,
  View,
} from "./types";

export function createResourceNavigationIntent(
  id: number,
  request: ResourceNavigationRequest,
): ResourceNavigationIntent {
  return { ...request, id } as ResourceNavigationIntent;
}

export function viewForResourceIntent(intent: ResourceNavigationIntent): View {
  if (intent.domain === "mcp") return { kind: "registry", intent };
  if (intent.domain === "model") return { kind: "models", intent };
  if (intent.domain === "startup_agent") return { kind: "startup_agents", intent };
  if (intent.domain === "subagent") return { kind: "subagents", intent };
  if (intent.domain === "hook") return { kind: "hooks", intent };
  return { kind: "skills", intent };
}

export function viewHasResourceIntent(view: View, id: number): boolean {
  return "intent" in view && view.intent?.id === id;
}

export function clearResourceIntent(view: View, id: number): View {
  if (!viewHasResourceIntent(view, id)) return view;
  if (view.kind === "registry") return { kind: "registry" };
  if (view.kind === "models") return { kind: "models" };
  if (view.kind === "startup_agents") return { kind: "startup_agents" };
  if (view.kind === "skills") return { kind: "skills" };
  if (view.kind === "subagents") return { kind: "subagents" };
  if (view.kind === "hooks") return { kind: "hooks" };
  return view;
}
