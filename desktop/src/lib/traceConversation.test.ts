import { describe, expect, it } from 'vitest';
import { conversationRows, retainConversationText } from './traceConversation';
import { traceMatches, type TraceEvent } from './traces';
const call: TraceEvent = { id: 'c', kind: 'tool_call', timestamp: null, title: 'Read', preview: '', call_id: 'native-a', is_error: false };
const result: TraceEvent = { ...call, id: 'r', kind: 'tool_result' };
describe('conversation history boundaries', () => {
  it('collapses only adjacent calls with the exact native result ID', () => {
    expect(conversationRows([call, result])).toEqual([{ event: call, result }]);
    const other = { ...call, id: 'other', call_id: 'native-b' };
    expect(conversationRows([call, other, result])).toHaveLength(3);
    expect(conversationRows([call, { ...result, call_id: 'different' }])).toHaveLength(2);
  });
  it('releases older inline bodies while keeping summaries and IDs accessible', () => {
    const first = { ...call, id: 'u', kind: 'user' as const, text: 'older' };
    const second = { ...first, id: 'a', kind: 'assistant' as const, text: 'newer' };
    const retained = retainConversationText([first, second], 10);
    expect(retained[0]).toMatchObject({ id: 'u', text: undefined, text_truncated: true });
    expect(retained[1].text).toBe('newer');
    expect(first.text).toBe('older');
    expect(traceMatches(second, 'NEWER', 'assistant')).toBe(true);
  });
});
