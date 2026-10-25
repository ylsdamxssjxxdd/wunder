import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  buildTrajectoryLayout,
  buildRequestNumbers,
  deriveTimeline,
  filterRecords,
  trajectoryDiagnosticRows,
  collapseTurnRecords,
  extractUsage,
  flattenRecords,
  buildFoldGroups,
  trajectoryRecordId,
  mergeTrajectoryPage,
  trajectoryPageTurns,
} from '../../src/views/trajectory/trajectoryModel';
import type { TrajectoryPageRow } from '../../src/views/trajectory/trajectoryModel';

const translate = (key: string, params?: Record<string, string | number>) =>
  params ? `${key}:${JSON.stringify(params)}` : key;

const buildToolCells = (items: unknown[]) => {
  const turns = buildTrajectoryLayout([{ user_turn_index: 1, items }], translate);
  return flattenRecords(turns).map(record => record.cell);
};

const trajectoryRow = (ordinal: number, kind = 'assistant_message'): TrajectoryPageRow => ({
  item_id: `fixture-item-${ordinal}`, root_turn_id: 'fixture-root', turn_id: 'fixture-execution',
  user_round: 1, item_index: ordinal, created_seq: ordinal + 1, model_round: ordinal + 1,
  kind, status: 'completed', summary: `fixture record ${ordinal}`,
  started_at: 1800000000 + ordinal, completed_at: 1800000000 + ordinal + .25,
});

test('individual trajectory pages retain every model/tool row and its timing and diagnostics', () => {
  const rows = [trajectoryRow(4), { ...trajectoryRow(8, 'tool_call'), tool_name: 'fixture_tool' },
    { ...trajectoryRow(12), diagnostics: { retry: { attempt: 2, will_retry: false } },
      stats: { round_usage: { input_tokens: 32, output_tokens: 8 } } }];
  const input = JSON.stringify(rows);
  const turns = trajectoryPageTurns(rows) as { items: any[] }[];
  const records = flattenRecords(buildTrajectoryLayout(turns, translate));
  assert.equal(turns.length, 1);
  assert.deepEqual(turns[0].items.map(item => item.created_seq), [5, 9, 13]);
  assert.deepEqual(records.map(record => record.cell.recordId), rows.map(row => row.item_id));
  assert.deepEqual(records.map(record => record.cell.kind), ['message', 'tool', 'message']);
  assert.deepEqual(records.map(record => record.cell.timeSeconds), [.25, .25, .25]);
  assert.equal(records[2].cell.usage?.output, 8);
  assert.equal(records[2].cell.retry?.attempt, 2);
  assert.equal(records[0].collapsedSummary, undefined);
  assert.equal(JSON.stringify(rows), input);
});

test('trajectory paging uses explicit ordinal cursors and preserves the opposite retained edge', () => {
  const edges = { before: null, after: null, hasOlder: false, hasNewer: false };
  const initial = mergeTrajectoryPage([], { trajectory: [trajectoryRow(30), trajectoryRow(50)],
    next_before: 29, next_after: 51, has_more: true }, null, 4, edges);
  assert.equal(initial.before, 29);
  assert.equal(initial.after, 51);
  const older = mergeTrajectoryPage(initial.rows, { trajectory: [trajectoryRow(10), trajectoryRow(20)],
    next_before: 9, next_after: 21, has_more: true, has_newer: true }, 'older', 4,
    { ...initial, hasOlder: initial.hasOlder });
  assert.equal(older.before, 9);
  assert.equal(older.after, 51);
  assert.equal(older.hasNewer, false, 'server newer rows already retained must not create a paging loop');
  const zero = mergeTrajectoryPage([], { trajectory: [trajectoryRow(0)], next_before: 0, next_after: 0 }, null, 4, edges);
  assert.equal(zero.before, 0);
  assert.equal(zero.after, 0);
});

test('bounded trajectory windows can traverse sparse records both ways without gaps or duplication', () => {
  const fixture = Array.from({ length: 1001 }, (_, index) => trajectoryRow(index * 3));
  const seen = new Set<string>();
  const pageFor = (selected: TrajectoryPageRow[], older: boolean, newer: boolean) => ({
    trajectory: selected, next_before: selected[0]?.item_index ?? null,
    next_after: selected.at(-1)?.item_index ?? null, has_more: older, has_newer: newer,
  });
  let state = mergeTrajectoryPage([], pageFor(fixture.slice(-80), true, false), null, 600,
    { before: null, after: null, hasOlder: false, hasNewer: false });
  for (const row of state.rows) seen.add(row.item_id);
  for (let request = 0; state.hasOlder && request < 20; request++) {
    const older = fixture.filter(row => row.item_index < state.before!).slice(-80);
    state = mergeTrajectoryPage(state.rows, pageFor(older, older[0]?.item_index !== 0, true),
      'older', 600, state);
    assert.ok(state.rows.length <= 600);
    assert.equal(new Set(state.rows.map(row => row.item_id)).size, state.rows.length);
    for (const row of state.rows) seen.add(row.item_id);
  }
  assert.equal(state.hasOlder, false);
  assert.equal(state.rows[0].item_index, 0);
  assert.equal(state.hasNewer, true);
  assert.equal(seen.size, fixture.length);
  for (let request = 0; state.hasNewer && request < 20; request++) {
    const newer = fixture.filter(row => row.item_index > state.after!).slice(0, 80);
    state = mergeTrajectoryPage(state.rows, pageFor(newer, true, newer.at(-1)?.item_id !== fixture.at(-1)!.item_id),
      'newer', 600, state);
    assert.ok(state.rows.length <= 600);
  }
  assert.equal(state.hasNewer, false);
  assert.equal(state.rows.at(-1)?.item_id, fixture.at(-1)?.item_id);
  assert.equal(state.hasOlder, true);
});

test('overlapping trajectory pages update one record without duplicating durable identities', () => {
  const original = trajectoryRow(4);
  const updated = { ...original, summary: 'fixture completed response', revision: 2 };
  const merged = mergeTrajectoryPage([original, trajectoryRow(8)], {
    trajectory: [trajectoryRow(0), updated], next_before: 0, next_after: 4,
  }, 'older', 600, { before: 4, after: 8, hasOlder: true, hasNewer: false });
  assert.deepEqual(merged.rows.map(row => row.item_index), [0, 4, 8]);
  assert.equal(merged.rows[1].summary, updated.summary);
  assert.equal(merged.after, 8);
});

test('both trajectory clients preserve canonical timing, explicit zero and unknown durations', async () => {
  const bridge = await bridgeModel();
  const committed = (id: string, kind: string, payload: Record<string, unknown> = {}) => ({
    item_id: id, kind, status: 'completed', created_at_ms: 1800000000000,
    updated_at_ms: 1800000000000, payload: { content: 'fixture output', ...payload },
  });
  const fixture = [{ user_turn_index: 1, items: [
    committed('fixture-canonical', 'assistant_message', { item_id: 'fixture-inner',
      kind: 'assistant_message', payload: { content: 'fixture canonical output',
        started_at: 1800000000, completed_at: 1800000002,
        message_stats: { interaction_duration_s: 90 } } }),
    { ...committed('fixture-top-level', 'tool_call', { tool: 'fixture_tool' }),
      started_at: 1800000000, completed_at: 1800000001.5 },
    committed('fixture-model-duration', 'assistant_message', {
      stats: { model_duration_s: 1.25, interaction_duration_s: 90 } }),
    committed('fixture-stream-timing', 'assistant_message', { stream_timing: { prefill_ms: 100, decode_ms: 500 } }),
    committed('fixture-tool-milliseconds', 'tool_call', { tool: 'fixture_tool', duration_ms: 75 }),
    committed('fixture-tool-elapsed', 'tool_call', { tool: 'fixture_tool', elapsed_s: .125 }),
    committed('fixture-explicit-zero', 'tool_call', { tool: 'fixture_tool', duration_ms: 0 }),
    committed('fixture-interval-zero', 'assistant_message', { started_at: 1800000000, completed_at: 1800000000 }),
    committed('fixture-unknown-tool', 'tool_call', { tool: 'fixture_tool' }),
    committed('fixture-unknown-model', 'assistant_message', { message_stats: { interaction_duration_s: 90 } }),
  ] }];
  const project = (model: { buildTrajectoryLayout: typeof buildTrajectoryLayout; flattenRecords: typeof flattenRecords }) =>
    model.flattenRecords(model.buildTrajectoryLayout(fixture, translate)).map(record => record.cell);
  const hive = project({ buildTrajectoryLayout, flattenRecords });
  const admin = project(bridge);
  assert.deepEqual(admin, hive);
  assert.deepEqual(hive.map(cell => cell.timeSeconds), [2, 1.5, 1.25, .6, .075, .125, 0, 0, null, null]);
  assert.equal(hive[0].outputDetail, 'fixture canonical output');
  assert.equal(hive[0].startedAt, 1800000000000);
  assert.equal(hive[1].startedAt, 1800000000000);
  assert.equal(hive[2].startedAt, 1799999998750);
  assert.equal(hive.at(-1)?.metrics?.timingRecorded, false);
});

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

test('assistant overview keeps response in output and never duplicates it as input', async () => {
  const turns = buildTrajectoryLayout([{ user_turn_index: 2, items: [
    { kind: 'assistant_message', payload: {
      event_type: 'assistant_message', content: 'model response',
      reasoning_content: 'model reasoning'
    } }
  ] }], translate);
  const [assistant] = flattenRecords(turns).map(record => record.cell);
  assert.equal(assistant?.inputDetail, undefined);
  assert.equal(assistant?.outputDetail, 'model response');
  assert.equal(assistant?.thinkingDetail, 'model reasoning');

  const bridgeSource = readFileSync(resolve(__dirname, '../../web/modules/trajectory-model.js'), 'utf8');
  const bridge = await import(`data:text/javascript;base64,${Buffer.from(bridgeSource).toString('base64')}`);
  const bridgeCell = bridge.flattenRecords(bridge.buildTrajectoryLayout([
    { user_turn_index: 2, items: [{ kind: 'assistant_message', payload: {
      event_type: 'assistant_message', content: 'model response', reasoning_content: 'model reasoning'
    } }] }
  ], translate)).map((record: any) => record.cell)[0];
  assert.equal(bridgeCell.inputDetail, undefined);
  assert.equal(bridgeCell.outputDetail, 'model response');
});

test('subagent runs keep child task/report and own wall time in a separate subtool span', async () => {
  const items = [{ kind: 'subagent_run', item_id: 'child-run', created_time: '2026-10-16T11:23:34.000Z', payload: {
    event_type: 'subagent_run',
    runtime: {
      label: 'directory worker',
      user_message: 'inspect the workspace',
      summary: 'workspace contains two files',
      status: 'success',
      started_time: '2026-10-16T11:23:34.000Z',
      finished_time: '2026-10-16T11:23:38.000Z',
    },
  } }]
  const [hiveCell] = flattenRecords(buildTrajectoryLayout([{ user_turn_index: 1, items }], translate))
    .map(record => record.cell)
  assert.equal(hiveCell?.kind, 'subtool')
  assert.equal(hiveCell?.text, 'directory worker')
  assert.equal(hiveCell?.inputDetail, 'inspect the workspace')
  assert.equal(hiveCell?.outputDetail, 'workspace contains two files')
  assert.equal(hiveCell?.timeSeconds, 4)

  const bridgeSource = readFileSync(resolve(__dirname, '../../web/modules/trajectory-model.js'), 'utf8')
  const bridge = await import(`data:text/javascript;base64,${Buffer.from(bridgeSource).toString('base64')}`)
  const [bridgeCell] = bridge.flattenRecords(bridge.buildTrajectoryLayout([
    { user_turn_index: 1, items },
  ], translate)).map((record: any) => record.cell)
  assert.equal(bridgeCell.kind, 'subtool')
  assert.equal(bridgeCell.text, 'directory worker')
  assert.equal(bridgeCell.inputDetail, 'inspect the workspace')
  assert.equal(bridgeCell.outputDetail, 'workspace contains two files')
  assert.equal(bridgeCell.timeSeconds, 4)
})

test('assistant timeline uses explicit request duration instead of cumulative user duration', () => {
  const [assistant] = flattenRecords(buildTrajectoryLayout([{ user_turn_index: 1, items: [{
    kind: 'assistant_message', created_time: '2026-10-16T11:23:33.000Z',
    updated_time: '2026-10-16T11:23:53.000Z', payload: {
      content: 'done', meta: {
        // This is cumulative from the user turn and must not set the one
        // request span. `request_duration_s` is the explicit own-step value.
        message_stats: { interaction_duration_s: 400, request_duration_s: 2.5 },
      },
    },
  }] }], translate)).map(record => record.cell)
  assert.equal(assistant?.timeSeconds, 2.5)
  assert.equal(assistant?.startedAt, Date.parse('2026-10-16T11:23:50.500Z'))
})

test('cumulative user duration never stretches assistant spans in either client', async () => {
  const fixture = [{ user_turn_index: 1, items: [{
    kind: 'assistant_message', status: 'completed', created_time: 1800000000,
    updated_time: 1800000002, payload: {
      content: 'fixture response', message_stats: { interaction_duration_s: 400 },
    },
  }] }];
  for (const model of [{ buildTrajectoryLayout, flattenRecords, deriveTimeline }, await bridgeModel()]) {
    const turns = model.buildTrajectoryLayout(fixture, translate);
    const [cell] = model.flattenRecords(turns).map((record: any) => record.cell);
    assert.equal(cell.startedAt, 1800000000000);
    assert.equal(cell.timeSeconds, 2);
    const timeline = model.deriveTimeline(turns, 'duration');
    assert.equal(timeline.end - timeline.start, 2000);
  }
});

test('compaction uses summary_text instead of the history placeholder and ignores marker rows', async () => {
  const bridgeSource = readFileSync(resolve(__dirname, '../../web/modules/trajectory-model.js'), 'utf8');
  const bridge = await import(`data:text/javascript;base64,${Buffer.from(bridgeSource).toString('base64')}`);
  const turns = bridge.buildTrajectoryLayout([{ user_turn_index: 1, items: [
    { kind: 'assistant_message', payload: {
      content: 'summary marker', meta: { type: 'manual_compaction_marker', manual_compaction: true }
    } },
    { kind: 'compaction', payload: {
      event_type: 'compaction', content: 'history', summary_text: 'real durable summary'
    } }
  ] }], translate);
  const cells = bridge.flattenRecords(turns).map((record: any) => record.cell);
  assert.equal(cells.some((cell: any) => cell.text === 'summary marker'), false);
  const compacted = cells.find((cell: any) => cell.kind === 'compacted');
  assert.equal(compacted?.text, 'real durable summary');
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

const bridgeModel = async () => {
  const source = readFileSync(resolve(__dirname, '../../web/modules/trajectory-model.js'), 'utf8');
  return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);
};

const assistantItem = (step: number, status = 'completed', execution = 'execution-a') => ({
  kind: 'assistant_message', turn_id: execution, item_id: `${execution}:text-${step}`,
  status, created_seq: step, created_time: 1800000000, updated_time: 1800000002,
  payload: { model_round: step, content: 'partial response' },
});

test('bridge snapshots group continuations once and retain failure execution identity', async () => {
  const bridge = await bridgeModel();
  const first = assistantItem(1);
  const continued = assistantItem(1, 'running', 'execution-b');
  const snapshot = { turns: [
    { turn_id: 'execution-b', root_turn_id: 'execution-a', trigger_kind: 'continuation', user_turn_index: 1, items: [continued] },
    { turn_id: 'execution-a', root_turn_id: 'execution-a', trigger_kind: 'user', user_turn_index: 1, items: [first] },
  ], items: [first, continued, { kind: 'terminal', item_id: 'failure', turn_id: 'execution-b',
    status: 'failed', payload: { model_round: 1, error: { code: 'UPSTREAM_TIMEOUT', message: 'fixture timeout' } } }] };
  const before = JSON.stringify(snapshot);
  const turns = bridge.attachTurnItems(snapshot);
  assert.equal(turns.length, 1);
  assert.equal(turns[0].trigger_kind, 'user');
  assert.equal(turns[0].items.length, 3);
  const cells = bridge.flattenRecords(bridge.buildTrajectoryLayout(turns, translate)).map((r: any) => r.cell);
  assert.equal(cells.length, 2);
  assert.equal(cells.find((cell: any) => cell.recordId === first.item_id).isError, false);
  assert.equal(cells.find((cell: any) => cell.recordId === continued.item_id).failure.code, 'UPSTREAM_TIMEOUT');
  assert.equal(JSON.stringify(snapshot), before);
  const continuationOnly = bridge.attachTurnItems({ turns: [snapshot.turns[0]], items: [continued] });
  assert.equal(continuationOnly.length, 1);
  assert.equal(continuationOnly[0].turn_id, 'execution-a');
  assert.equal(continuationOnly[0].items.length, 1);
});

test('bridge failure details hydrate durable blocks without duplication or replacing sealed text', async () => {
  const bridge = await bridgeModel();
  const active = { ...assistantItem(1, 'failed'), payload: { model_round: 1 } };
  const sealed = { ...assistantItem(2), payload: { model_round: 2, content: 'sealed output', reasoning_content: 'sealed reasoning' } };
  const chunk = (item: string, field: string, index: number, offset: number, text: string) => ({
    item_id: item, field, block_index: index, data: { [field]: text, [`${field}_offset`]: offset },
  });
  const snapshot = { turns: [{ turn_id: 'execution-a', user_turn_index: 1 }], items: [active, sealed], blocks: [
    chunk(active.item_id, 'content', 1, 2, ' done'),
    chunk(active.item_id, 'content', 0, 0, '😀'),
    chunk(active.item_id, 'content', 0, 0, '😀'),
    chunk(active.item_id, 'reasoning', 0, 0, 'partial thought'),
    chunk(sealed.item_id, 'content', 0, 0, 'stale output'),
    chunk(sealed.item_id, 'reasoning', 0, 0, 'stale reasoning'),
  ] };
  const before = JSON.stringify(snapshot);
  const cells = bridge.flattenRecords(bridge.buildTrajectoryLayout(bridge.attachTurnItems(snapshot), translate)).map((r: any) => r.cell);
  assert.equal(cells[0].outputDetail, '😀 done');
  assert.equal(cells[0].thinkingDetail, 'partial thought');
  assert.equal(cells[1].outputDetail, 'sealed output');
  assert.equal(cells[1].thinkingDetail, 'sealed reasoning');
  assert.equal(JSON.stringify(snapshot), before);
  const gapped = bridge.attachTurnItems({ ...snapshot, blocks: [chunk(active.item_id, 'content', 1, 100000000, 'tail')] });
  assert.equal(gapped[0].items[0].payload.content, '\n…\ntail');
});

test('legacy timeout settles the matching request and timeline without an extra context row', async () => {
  const fixture = [{ user_turn_index: 1, items: [
    assistantItem(1), assistantItem(2, 'running'),
    { kind: 'terminal', turn_id: 'execution-a', status: 'failed', created_seq: 3,
      updated_time: 1800000005, payload: { model_round: 2, error: {
        code: 'UPSTREAM_TIMEOUT', message: 'fixture provider timed out',
        error_meta: { source_stage: 'llm', retryable: true, recovery_action: 'retry_later' },
      }, retry_after_ms: 1000 } },
  ] }];
  const bridge = await bridgeModel();
  for (const model of [{ buildTrajectoryLayout, flattenRecords, buildRequestNumbers, deriveTimeline, filterRecords }, bridge]) {
    const turns = model.buildTrajectoryLayout(fixture, translate);
    const cells = model.flattenRecords(turns).map((r: any) => r.cell);
    assert.equal(cells.length, 2);
    assert.equal(cells[0].isError, false);
    assert.equal(cells[1].isError, true);
    assert.equal(cells[1].failure.code, 'UPSTREAM_TIMEOUT');
    assert.equal(cells[1].failure.recoveryAction, 'retry_later');
    assert.equal(cells[1].failure.retryAfterMs, 1000);
    const requests = model.buildRequestNumbers(turns);
    assert.equal(requests[1].status, 'error');
    assert.equal(requests[1].completedAt, 1800000005000);
    assert.equal(model.deriveTimeline(turns).spans[1].isError, true);
    assert.equal(model.filterRecords(model.flattenRecords(turns), 'UPSTREAM_TIMEOUT').length, 1);
  }
});

test('successful retries retain last failure while running retries stay pending', async () => {
  const fixture = [{ user_turn_index: 1, items: [
    { ...assistantItem(1), payload: { model_round: 1, content: 'response', trajectory_retry: {
      attempt: 2, max_attempts: 3, delay_s: 0.5, message: 'temporary provider error', will_retry: false,
    } } },
    { ...assistantItem(2, 'running'), payload: { model_round: 2, trajectory_retry: {
      attempt: 1, max_attempts: 3, error: 'another temporary failure', will_retry: true,
    } } },
  ] }];
  const bridge = await bridgeModel();
  for (const model of [{ buildTrajectoryLayout, flattenRecords, buildRequestNumbers, trajectoryDiagnosticRows }, bridge]) {
    const turns = model.buildTrajectoryLayout(fixture, translate);
    const requests = model.buildRequestNumbers(turns);
    assert.equal(requests[0].status, 'complete');
    assert.equal(requests[0].retry.attempt, 2);
    assert.equal(requests[0].retry.willRetry, false);
    assert.equal(requests[1].status, 'running');
    assert.equal(requests[1].completedAt, null);
    const cells = model.flattenRecords(turns).map((r: any) => r.cell);
    assert.equal(cells[1].isError, false);
    assert.equal(cells[1].retry.willRetry, true);
    assert.ok(model.trajectoryDiagnosticRows(cells[0], translate).some((r: any) => r.value === 'temporary provider error'));
  }
});

test('tool error objects are preserved and unfinished calls become interrupted', async () => {
  const fixture = [{ user_turn_index: 1, items: [
    assistantItem(1),
    { kind: 'tool_call', turn_id: 'execution-a', status: 'running', created_seq: 2, payload: { tool: 'fixture_tool', model_round: 1, tool_call_id: 'tool-a' } },
    { kind: 'tool_result', turn_id: 'execution-a', status: 'completed', created_seq: 3, payload: { tool_call_id: 'tool-a', ok: false, error: { code: 'TOOL_INVALID', message: 'fixture argument rejected' } } },
    { kind: 'tool_call', turn_id: 'execution-a', status: 'cancelled', created_seq: 4, payload: { tool: 'fixture_tool', tool_call_id: 'tool-b', model_round: 1 } },
    { kind: 'terminal', turn_id: 'execution-a', status: 'cancelled', created_seq: 5, payload: { model_round: 1, error: { code: 'CANCELLED', message: 'cancelled' } } },
  ] }];
  const bridge = await bridgeModel();
  for (const model of [{ buildTrajectoryLayout, flattenRecords }, bridge]) {
    const cells = model.flattenRecords(model.buildTrajectoryLayout(fixture, translate)).map((r: any) => r.cell);
    assert.equal(cells.length, 3);
    assert.equal(cells[0].status, 'cancelled');
    assert.equal(cells[1].failure.code, 'TOOL_INVALID');
    assert.equal(cells[1].failure.message, 'fixture argument rejected');
    assert.equal(cells[2].failure.code, 'INTERRUPTED');
    assert.equal(cells[2].isError, true);
  }
});

test('errors never cross execution identity and admission failures remain visible', async () => {
  const fixture = [{ user_turn_index: 1, items: [
    assistantItem(1), assistantItem(2), assistantItem(1, 'completed', 'execution-b'),
    { kind: 'terminal', turn_id: 'execution-a', status: 'failed', created_seq: 4, payload: { error: { code: 'MODEL_ERROR', message: 'fixture failure' } } },
    { kind: 'terminal', turn_id: 'execution-c', status: 'rejected', created_seq: 5, payload: { error: { code: 'INPUT_INVALID', message: 'fixture rejected' } } },
  ] }];
  const bridge = await bridgeModel();
  for (const model of [{ buildTrajectoryLayout, flattenRecords }, bridge]) {
    const cells = model.flattenRecords(model.buildTrajectoryLayout(fixture, translate)).map((r: any) => r.cell);
    assert.equal(cells.length, 4);
    assert.equal(cells.find((c: any) => c.recordId === 'execution-a:text-2').failure.code, 'MODEL_ERROR');
    assert.equal(cells.find((c: any) => c.recordId === 'execution-b:text-1').isError, false);
    assert.equal(cells[3].failure.code, 'INPUT_INVALID');
  }
});

test('bounded paged diagnostics and exported retry events have identical client projections', async () => {
  const fixture = [{ user_turn_index: 1, items: [
    { ...assistantItem(1), payload: { model_round: 1, diagnostics: { failure: {
      status: 'failed', code: 'MODEL_ERROR', message: 'x'.repeat(10000), source_stage: 'llm', recovery_action: 'retry_later', retryable: true,
    }, retry: { attempt: 1, max_attempts: 3, message: 'fixture retry', will_retry: false } } } },
    { ...assistantItem(2), payload: { model_round: 2, content: 'done' } },
    { kind: 'llm_stream_retry', turn_id: 'execution-a', payload: { model_round: 2, attempt: 1, max_attempts: 3, error: 'fixture transient failure', will_retry: true } },
  ] }];
  const bridge = await bridgeModel();
  const hive = flattenRecords(buildTrajectoryLayout(fixture, translate)).map(r => r.cell);
  const admin = bridge.flattenRecords(bridge.buildTrajectoryLayout(fixture, translate)).map((r: any) => r.cell);
  assert.deepEqual(admin, hive);
  assert.equal(hive[0].failure?.message?.length, 2048);
  assert.equal(hive[0].failure?.retryable, true);
  assert.equal(hive[1].retry?.willRetry, false);
  assert.equal(hive[1].isError, false);
});
