import test from 'node:test';
import assert from 'node:assert/strict';
import { analyzeChatEvidence, type ChatEvidence } from '../../tests/support/chatEvidenceAnalysis';
import { redactEvidence } from '../../tests/support/chatEvidenceRedaction';

const valid = (): ChatEvidence => ({ mode: 'real-service',
  snapshot: { cursor: 3,
    turns: [{ turn_id: 'turn-a', user_turn_index: 1, status: 'completed' }],
    items: [
      { item_id: 'turn-a:user', turn_id: 'turn-a', kind: 'user_message', status: 'completed' },
      { item_id: 'turn-a:text-1', turn_id: 'turn-a', kind: 'assistant_message', model_round: 1,
        status: 'completed', meta: { message_stats: { interaction_duration_s: 1 } } }
    ], blocks: [] },
  changes: [
    { change_seq: 1, change_type: 'turn_upsert', turn_id: 'turn-a', payload: { status: 'running' } },
    { change_seq: 2, change_type: 'item_upsert', item_id: 'turn-a:text-1', revision: 1, payload: {} },
    { change_seq: 3, change_type: 'turn_status', turn_id: 'turn-a', payload: { status: 'completed' } }
  ], performance: { capture: { startedAt: 'fixture' }, summary: { responsiveness: { frameGaps: { p95UpperMs: 32 } } } },
  expectedUserTurns: 1, coverage: { tools: true }, requiredCoverage: ['tools'] });
const codes = (value: ChatEvidence) => analyzeChatEvidence(value).findings.map(item => item.code);

test('evidence analyzer accepts complete independent thread evidence', () => {
  assert.equal(analyzeChatEvidence(valid()).verdict, 'passed');
});
test('evidence analyzer catches duplicate rounds, unfinished tools and orphan blocks', () => {
  const value = valid();
  value.snapshot.turns.push({ turn_id: 'turn-b', user_turn_index: 1, status: 'completed' });
  value.snapshot.items.push({ item_id: 'tool-a', turn_id: 'turn-a', kind: 'tool_call', status: 'running' });
  value.snapshot.blocks.push({ item_id: 'absent', content: 'fixture' });
  for (const code of ['duplicate-user-round', 'unfinished-item-in-terminal-turn', 'orphan-text-block']) {
    assert.ok(codes(value).includes(code));
  }
});
test('missing coverage or logs cannot be reported as successful real interaction', () => {
  const value = valid();
  value.coverage = {};
  value.changes.splice(1, 1);
  value.performance = undefined;
  for (const code of ['durable-gap-or-reorder', 'unproven-coverage:tools', 'missing-performance-capture']) {
    assert.ok(codes(value).includes(code));
  }
});
test('cancellation followed by late terminal replacement is an observable failure', () => {
  const value = valid();
  value.changes[0].payload.status = 'cancelled';
  assert.ok(codes(value).includes('terminal-turn-revived'));
});
test('long tasks remain review findings even if the p95 passes', () => {
  const value = valid();
  value.performance!.summary.responsiveness.longTasks = { count: 1, maxMs: 80 };
  assert.equal(analyzeChatEvidence(value).verdict, 'needs-review');
  value.performance!.summary.responsiveness.longTasks.maxMs = 300;
  assert.equal(analyzeChatEvidence(value).verdict, 'failed');
});

test('evidence export preserves matching fingerprints but omits message text and credentials', () => {
  const source = { item_id: 'fixture-item', turn_id: 'fixture-item', content: 'fixture-content',
    nested: { access_token: 'fixture-credential', arguments: 'fixture-arguments' }, status: 'completed' };
  const safe = redactEvidence(source);
  assert.deepEqual(safe.item_id, safe.turn_id);
  assert.equal(safe.status, 'completed');
  assert.equal(safe.nested.access_token, '[redacted]');
  for (const value of ['fixture-content', 'fixture-credential', 'fixture-arguments']) {
    assert.equal(JSON.stringify(safe).includes(value), false);
  }
});
