import type { TraceEvent, TraceSession } from './traces';

export interface TraceRow { event: TraceEvent; result?: TraceEvent }

/** Collapse only adjacent native call/result IDs. Interleaved calls, branches
 * and unmatched returns keep their original chronological positions. */
export function conversationRows(events: TraceEvent[]): TraceRow[] {
  const rows: TraceRow[] = [];
  for (let index = 0; index < events.length; index++) {
    const event = events[index], next = events[index + 1];
    if (event.kind === 'tool_call' && event.call_id && next?.kind === 'tool_result' && next.call_id === event.call_id) {
      rows.push({ event, result: next }); index++;
    } else rows.push({ event });
  }
  return rows;
}

/** Keep summaries/cursors while releasing older inline bodies. Full records
 * remain available through Core's revision-checked, on-demand reader. */
export function retainConversationText(events: TraceEvent[], budget = 2 * 1024 * 1024): TraceEvent[] {
  const retained = [...events];
  for (let index = retained.length - 1; index >= 0; index--) {
    const event = retained[index];
    if (event.text == null) continue;
    const bytes = event.text.length * 2;
    if (bytes <= budget) budget -= bytes;
    else retained[index] = { ...event, text: undefined, text_truncated: true };
  }
  return retained;
}

export function traceSessionLabel(session: TraceSession): string {
  const generated = /^\d{4}-\d{2}-\d{2}[T_]/.test(session.title)
    || /^[0-9a-f]{8}-[0-9a-f-]{27,}$/i.test(session.title);
  if (!generated) return session.title;
  return session.project?.split(/[\\/]/).filter(Boolean).pop() || session.agent_name;
}

export function traceJson(value: unknown): string {
  const compact = JSON.stringify(value) ?? '';
  return compact.length > 1024 * 1024 ? compact : JSON.stringify(value, null, 2) ?? '';
}
