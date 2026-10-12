import { invoke } from '@tauri-apps/api/core';
export interface CaptureTarget { id: string; name: string; pids: number[]; agent_ids: string[] }
export interface AgentCaptureSupport { agent_id: string; available: boolean; kind: string | null; target_ids: string[]; ca_environment: string[]; chromium: boolean; ca_scope: 'process' | 'mixed' | 'system' }
export interface CaptureEnvironment { supported: boolean; engine: string | null; extension_enabled: boolean; certificate_present: boolean; certificate_trusted: boolean; other_extensions: string[]; targets: CaptureTarget[]; agents: AgentCaptureSupport[] }
export interface CaptureSession { id: string; agent_id: string; agent_name: string; target_id: string; target_name: string; pids: number[]; proxy_url: string | null; started_at: string }
export interface CaptureStatus { state: string; message: string; pids: number[]; flow_count: number }
export interface FlowSummary { id: string; method: string; url: string; status: number | null; started_at: number; duration_ms: number | null; error: string | null;
  phase?: 'pending' | 'receiving' | 'streaming' | 'complete' | 'failed' | 'cancelled' | null;
  updated_at?: number | null; request_bytes?: number | null; response_bytes?: number | null;
  time_to_first_byte_ms?: number | null; content_type?: string | null; stream_type?: 'sse' | 'websocket' | null }
export interface CapturedMessage { headers: [string, string][]; body: string; bytes: number; truncated: boolean; encoding: string }
export interface CapturedFlow extends FlowSummary { request: CapturedMessage | null; response: CapturedMessage | null; request_http_version: string; response_http_version: string | null; proxy_url: string | null }
export interface CaptureSnapshot { session: CaptureSession; status: CaptureStatus; flows: FlowSummary[] }
export interface CaptureDelta { session: CaptureSession; status: CaptureStatus; revision: string; reset: boolean; upserts: FlowSummary[]; removed: string[] }

/** Deltas remove stale IDs before upserting: an ID can be deleted and recreated
 * between polls. Keep unchanged rows and arrays stable on idle refreshes. */
export function mergeCaptureDelta(current: CaptureSnapshot | null, delta: CaptureDelta): CaptureSnapshot {
  if (!delta.reset && current?.session.id === delta.session.id && !delta.upserts.length && !delta.removed.length) {
    if (JSON.stringify(current.status) === JSON.stringify(delta.status)
      && JSON.stringify(current.session) === JSON.stringify(delta.session)) return current;
    return { session: delta.session, status: delta.status, flows: current.flows };
  }
  const flows = new Map<string, FlowSummary>(!delta.reset && current?.session.id === delta.session.id
    ? current.flows.map(flow => [flow.id, flow]) : []);
  for (const id of delta.removed) flows.delete(id);
  for (const flow of delta.upserts) flows.set(flow.id, flow);
  return { session: delta.session, status: delta.status,
    flows: [...flows.values()].sort((left, right) => right.started_at - left.started_at || left.id.localeCompare(right.id)) };
}
export const captureEnvironment = () => invoke<CaptureEnvironment>('capture_environment');
export const captureLaunchAgent = (agentId: string, directory?: string) => invoke<void>('capture_launch_agent', { agentId, directory: directory ?? null });
export const captureSessions = () => invoke<CaptureSession[]>('capture_sessions');
export const captureSnapshot = (sessionId: string) => invoke<CaptureSnapshot>('capture_snapshot', { sessionId });
export const captureDelta = (sessionId: string, revision: string | null = null) => invoke<CaptureDelta>('capture_delta', { sessionId, revision });
export const captureDetail = (sessionId: string, flowId: string) => invoke<CapturedFlow>('capture_detail', { sessionId, flowId });
export const captureStart = (request: { agent_id: string; target_id: string; egress: string; proxy_url: string | null }) => invoke<CaptureSnapshot>('capture_start', { request });
export const captureStop = (sessionId: string) => invoke<CaptureSnapshot>('capture_stop', { sessionId });
export const captureExport = (sessionId: string, flowId?: string) => invoke<string | null>('capture_export', { sessionId, flowId: flowId ?? null });
