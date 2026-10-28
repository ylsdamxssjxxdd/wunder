import './mocks/browserStorage';
import assert from 'node:assert/strict';
import { applyChatThreadServerEvent } from '../../src/realtime/chat/chatThreadRuntime';
import { beginThreadView, commitThreadView, applyThreadViewTail, applyThreadViewProcessItem, applyThreadViewChanges, threadViewSlots, clearThreadView } from '../../src/realtime/chat/threadViewStore';

const key = 'fixture-order-session';
const row = (revision: number, round: number) => ({
  root_turn_id: 'fixture-order-turn', execution_turn_id: 'fixture-order-turn', user_round: 1, revision, status: 'running',
  user: { item_id: 'fixture-order-user', content: 'Fixture input' },
  assistant: { item_id: `fixture-order-turn:text-${round}`, content: '', reasoning: '', reasoning_offset: 0 },
  workflow_count: 1, failed_count: 0
});
const generation = beginThreadView(key);
commitThreadView(key, { session_id: key, epoch: 1, view_seq: 1, projection_state: 'ready', thread_empty: false,
  latest_round: 1, chat: [row(1, 1)], trajectory: [], tiles: [], tile_level: 0, next_before: 1, has_more: false }, generation);
const tail = (round: number, offset: number, text: string) => applyThreadViewTail(key, {
  item_id: `fixture-order-turn:text-${round}`, field: 'reasoning', offset, text
});
tail(2, 0, 'Fixture current');
tail(1, 'Fixture current'.length, ' stale');
assert.equal(threadViewSlots(key)![0].assistant.reasoning, 'Fixture current');
assert.equal(threadViewSlots(key)![0].assistant.reasoning_item_id, 'fixture-order-turn:text-2');
applyThreadViewChanges(key, { view_seq: 2, rows: [row(2, 1)] });
assert.equal(threadViewSlots(key)![0].assistant.reasoning, 'Fixture current');
assert.equal(threadViewSlots(key)![0].assistant.reasoning_item_id, 'fixture-order-turn:text-2');
tail(2, 'Fixture current'.length, ' continued');
assert.equal(threadViewSlots(key)![0].assistant.reasoning, 'Fixture current continued');
const tool = { item_id: 'fixture-order-turn:tool-first', turn_id: 'fixture-order-turn', kind: 'tool_call', status: 'running', payload: { tool: 'fixture_tool', process_seq: 3 } };
assert.equal(applyChatThreadServerEvent({}, key, 'thread_process_item', { data: tool }), true);
assert.equal(threadViewSlots(key)![0].assistant.process_items[0].status, 'running');
applyThreadViewProcessItem(key, { ...tool, status: 'completed' });
applyThreadViewProcessItem(key, tool);
assert.equal(threadViewSlots(key)![0].assistant.process_items[0].status, 'completed');
applyThreadViewChanges(key, { view_seq: 3, rows: [row(3, 2)] });
assert.equal(threadViewSlots(key)![0].assistant.process_items.length, 1);
for (let index=0;index<120;index++) applyThreadViewProcessItem(key, { ...tool, item_id: 'fixture-order-turn:tool-'+index });
assert.equal(threadViewSlots(key)![0].assistant.process_items.length, 100);
clearThreadView(key);
console.log('process order: rejected stale tails and preserved newer live identity across durable refresh');
