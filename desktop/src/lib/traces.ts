import { invoke } from "@tauri-apps/api/core";

export interface TraceSource { id: string; agent_ids: string[]; name: string; format: string; roots: string[] }
export interface TraceSession { id: string; source_id: string; agent_name: string; format: string; title: string; project: string | null; path: string; modified_at: string; bytes: number; imported: boolean }
export interface TraceIndex { sources: TraceSource[]; sessions: TraceSession[]; warnings: string[] }
export type TraceKind = "user" | "assistant" | "tool_call" | "tool_result" | "event";
export interface TraceEvent { id: string; kind: TraceKind; timestamp: string | null; title: string; preview: string; call_id: string | null; is_error: boolean; text?: string | null; text_truncated?: boolean }
export interface TracePage { session: TraceSession; events: TraceEvent[]; next_cursor: string | null; revision: string; warnings: string[] }
export interface TraceDetail { event: TraceEvent; raw: unknown; text: string; related: { label: string; raw: unknown; text?: string }[]; pairing_note: string | null; redacted: boolean }
export const traceIndex = () => invoke<TraceIndex>("trace_index");
export const tracePage = (sessionId: string, cursor: string | null = null) => invoke<TracePage>("trace_page", { sessionId, cursor });
export const traceDetail = (sessionId: string, eventId: string, revision: string) => invoke<TraceDetail>("trace_detail", { sessionId, eventId, revision });
export const traceImport = () => invoke<TraceSession | null>("trace_import");
export const traceExport = (sessionId: string, eventId: string, revision: string) => invoke<string | null>("trace_export", { sessionId, eventId, revision });
export function traceMatches(event: TraceEvent, query: string, role: string): boolean {
  return (role === "all" || (role === "tools" ? event.kind.startsWith("tool_") : event.kind === role))
    && [event.title, event.preview, event.text ?? "", event.call_id ?? ""].join("\n").toLocaleLowerCase().includes(query.trim().toLocaleLowerCase());
}
