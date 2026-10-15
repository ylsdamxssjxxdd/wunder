import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  buildTrajectoryLayout,
  collapseTurnRecords,
  extractUsage,
  flattenRecords,
  buildFoldGroups,
  trajectoryRecordId,
} from '../../src/views/trajectory/trajectoryModel';

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

test('turn summaries preserve the first row and restore the hidden turn on expansion', () => {
  const turns = buildTrajectoryLayout([{
    user_turn_index: 4,
    items: [
      { kind: 'user_message', status: 'completed', payload: { event_type: 'user_message', content: 'question' } },
      { kind: 'assistant_message', status: 'completed', payload: { event_type: 'assistant_message', content: 'answer', model_round: 1 } },
      { kind: 'tool_call', status: 'completed', payload: { event_type: 'tool_call', tool: 'read_file', args: {}, model_round: 1 } },
    ],
  }], translate);
  const records = flattenRecords(turns);
  const collapsed = collapseTurnRecords(records, new Set([4]), new Set(), translate);

  assert.equal(collapsed.length, 2);
  assert.equal(collapsed[0]?.cell.kind, 'user');
  assert.equal(collapsed[1]?.collapsedSummaryKind, 'turn');
  assert.deepEqual(collapseTurnRecords(records, new Set(), new Set(), translate), records);
});

test('call summaries keep the assistant message and fold only tool rows', () => {
  const turns = buildTrajectoryLayout([{
    user_turn_index: 5,
    items: [
      { kind: 'user_message', status: 'completed', payload: { event_type: 'user_message', content: 'question' } },
      { kind: 'assistant_message', status: 'completed', payload: { event_type: 'assistant_message', content: 'working', model_round: 1 } },
      { kind: 'tool_call', status: 'completed', payload: { event_type: 'tool_call', tool: 'read_file', args: {}, model_round: 1 } },
      { kind: 'assistant_message', status: 'completed', payload: { event_type: 'assistant_message', content: 'done', model_round: 2 } },
    ],
  }], translate);
  const records = flattenRecords(turns);
  const key = trajectoryRecordId(records.find(record => record.cell.kind === 'message')!.cell);
  const collapsed = collapseTurnRecords(records, new Set(), new Set([key]), translate);
  const assistant = collapsed.findIndex(record => record.cell.text === 'working');

  assert.equal(collapsed[assistant]?.cell.kind, 'message');
  assert.equal(collapsed[assistant + 1]?.collapsedSummaryKind, 'assistant');
  assert.equal(collapsed.some(record => record.cell.kind === 'tool'), false);
  assert.deepEqual(collapseTurnRecords(records, new Set(), new Set(), translate), records);
});

const foldFixture = () => flattenRecords(buildTrajectoryLayout([{
  user_turn_index: 7,
  items: [
    { item_id: 'prefix', kind: 'system_message', payload: { content: 'instructions' } },
    { item_id: 'user', kind: 'user_message', payload: { content: 'question' } },
    { item_id: 'a1', kind: 'assistant_message', payload: { content: 'working', model_round: 1 } },
    { item_id: 't1', kind: 'tool_call', payload: { tool: 'read_file', model_round: 1 } },
    { item_id: 'a2', kind: 'assistant_message', payload: { content: 'working again', model_round: 1 } },
    { item_id: 't2', kind: 'tool_call', payload: { tool: 'write_file', model_round: 1 } },
    { item_id: 'ctx', kind: 'context', payload: { content: 'context' } },
    { item_id: 'orphan', kind: 'tool_call', payload: { tool: 'read_file', model_round: 1 } },
    { item_id: 'a3', kind: 'assistant_message', payload: { content: 'done', model_round: 2 } },
  ],
}], translate));

test('assistant folds use record IDs and preserve later assistants sharing a group', () => {
  const records = foldFixture();
  const groups = buildFoldGroups(records);
  assert.equal(groups.assistants.size, 2);
  const [first, second] = [...groups.assistants.keys()];
  assert.notEqual(first, second);
  const folded = collapseTurnRecords(records, new Set(), new Set([first!]), translate);
  assert.equal(folded.filter(record => record.collapsedSummary).length, 1);
  assert.equal(folded.some(record => record.cell.text === 'working again'), true);
  assert.equal(folded.some(record => record.cell.kind === 'context'), true);
  assert.equal(folded.filter(record => record.cell.kind === 'tool' && !record.collapsedSummary).length, 2);
  assert.ok(records.findIndex(record => record.cell.recordId === 'ctx') < records.findIndex(record => record.cell.recordId === 'orphan'));
  assert.equal(folded.find(record => record.collapsedSummary)?.collapsedSummary?.includes('read_file'), true);
});

test('turn folds preserve system headers, ignore null and singleton turns, and transfer turn end', () => {
  const records = foldFixture();
  const folded = collapseTurnRecords(records, new Set([7]), new Set(), translate);
  assert.deepEqual(folded.filter(record => !record.collapsedSummary).map(record => record.cell.kind), ['system', 'user']);
  assert.equal(folded.at(-1)?.turnEnd, true);
  assert.equal(folded.filter(record => record.turnEnd).length, 1);
  const singleton = records.filter(record => record.cell.kind === 'user');
  assert.deepEqual(collapseTurnRecords(singleton, new Set([7]), new Set(), translate), singleton);
  const unassigned = records.map(record => ({ ...record, turn: null }));
  assert.deepEqual(collapseTurnRecords(unassigned, new Set([null]), new Set(), translate), unassigned);
});

test('nested folds restore call folds after a turn expands and leave orphan tools visible', () => {
  const records = foldFixture();
  const calls = new Set(buildFoldGroups(records).assistants.keys());
  const turnFolded = collapseTurnRecords(records, new Set([7]), calls, translate);
  assert.equal(turnFolded.filter(record => record.collapsedSummary).length, 1);
  const turnExpanded = collapseTurnRecords(records, new Set(), calls, translate);
  assert.equal(turnExpanded.filter(record => record.collapsedSummaryKind === 'assistant').length, 2);
  const orphan = records.filter(record => record.cell.kind !== 'message');
  assert.equal(buildFoldGroups(orphan).assistants.size, 0);
  assert.deepEqual(collapseTurnRecords(orphan, new Set(), calls, translate), orphan);
});

test('bridge and hive project identical folds for the same ledger', async () => {
  const bridgeSource = readFileSync(resolve(__dirname, '../../web/modules/trajectory-model.js'), 'utf8');
  const bridge = await import(`data:text/javascript;base64,${Buffer.from(bridgeSource).toString('base64')}`);
  const records = foldFixture();
  const calls = new Set(buildFoldGroups(records).assistants.keys());
  for (const turns of [new Set<number>(), new Set([7])]) {
    for (const assistants of [new Set<string>(), calls]) {
      const hive = collapseTurnRecords(records, turns, assistants, translate);
      const admin = bridge.collapseTurnRecords(records, turns, assistants,
        (key: string, params: Record<string, string | number>) => translate(`trajectory.${key}`, params));
      assert.deepEqual(admin, hive);
    }
  }
});
