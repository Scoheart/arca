import { invoke } from "@tauri-apps/api/core";

export type CodexConfigurationSource = "native" | "arca";
export interface CodexConfigurationSnapshot {
  path: string; exists: boolean; revision: string; text: string;
  fields: Record<string, string>; valid: boolean;
}
export const readCodexConfiguration = (source: CodexConfigurationSource, codexHome?: string) =>
  invoke<CodexConfigurationSnapshot>("codex_configuration_read", { source, codexHome });
export const renderCodexConfiguration = (text: string, patch: Record<string, string>) =>
  invoke<{ text: string; fields: Record<string, string> }>("codex_configuration_render", { text, patch });
export const saveCodexConfiguration = (source: CodexConfigurationSource, snapshot: CodexConfigurationSnapshot, text: string, patch: Record<string, string>, codexHome?: string) =>
  invoke<CodexConfigurationSnapshot>("codex_configuration_save", { source, codexHome, expectedRevision: snapshot.revision, text, patch });

export function codexProfileArguments(text: string) {
  const args = text.split("\n").filter(Boolean);
  const remaining: string[] = [];
  let profile: string | null = null;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--") { remaining.push(...args.slice(index)); break; }
    if (arg === "--profile" || arg === "-p") { profile = args[++index] ?? ""; continue; }
    if (arg.startsWith("--profile=")) { profile = arg.slice("--profile=".length); continue; }
    if (arg.startsWith("-p") && !arg.startsWith("--") && arg.length > 2) { profile = arg.slice(2); continue; }
    remaining.push(arg);
  }
  return { profile, remaining };
}
