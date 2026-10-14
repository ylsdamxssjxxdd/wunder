import test from 'node:test';
import assert from 'node:assert/strict';

import { buildTrajectoryLayout, extractUsage, flattenRecords } from '../../src/views/trajectory/trajectoryModel';

const translate = (key: string, params?: Record<string, string | number>) =>
  params ? `${key}:${JSON.stringify(params)}` : key;

const buildToolCells = (items: unknown[]) => {
  const turns = buildTrajectoryLayout([{ user_turn_index: 1, items }], translate);
  return flattenRecords(turns).map(record => record.cell);
};

test('successful tool result with an empty error field stays successful', () => {
  const [tool] = buildToolCells([{
    kind: 'tool_result',
    status: 'completed',
    payload: { event_type: 'tool_result', tool: 'read_file', ok: true, error: '' },
  }]);

  assert.equal(tool.isError, false);
});

test('explicit failed status and unsuccessful result are marked as failures', () => {
  const tools = buildToolCells([
    {
      kind: 'tool_result',
      status: 'failed',
      payload: { event_type: 'tool_result', tool: 'read_file', error: '' },
    },
    {
      kind: 'tool_result',
      status: 'completed',
      payload: { event_type: 'tool_result', tool: 'write_file', ok: false, error: '' },
    },
  ]);

  assert.deepEqual(tools.map(tool => tool.isError), [true, true]);
});

test('merged tool call inherits failure state from its result item', () => {
  const [tool] = buildToolCells([
    {
      kind: 'tool_call',
      status: 'completed',
      created_seq: 1,
      payload: { event_type: 'tool_call', tool: 'write_file', tool_call_id: 'call-1', args: {} },
    },
    {
      kind: 'tool_result',
      status: 'completed',
      created_seq: 2,
      payload: { event_type: 'tool_result', tool: 'write_file', tool_call_id: 'call-1', ok: false, error: '' },
    },
  ]);

  assert.equal(tool.isError, true);
});

test('agent mailbox deliveries are readable context instead of tool rows', () => {
  const [context] = buildToolCells([{
    kind: 'subagent_message',
    status: 'completed',
    payload: {
      event_type: 'subagent_message',
      message: 'fixture completion report',
      delivery: 'applied',
    },
  }]);

  assert.equal(context.kind, 'context');
  assert.equal(context.text, 'fixture completion report');
});

test('usage accepts DSH camelCase and nested persisted shapes', () => {
  assert.deepEqual(extractUsage({
    usage: { inputTokens: '12', outputTokens: 8, reasoningTokens: 3, cacheReadTokens: 2 },
  }), { input: 12, output: 8, think: 3, cacheRead: 2 });
  assert.deepEqual(extractUsage({
    meta: { message_stats: { round_usage: { input_tokens: 20, output_tokens: 4 } } },
  }), { input: 20, output: 4 });
});
