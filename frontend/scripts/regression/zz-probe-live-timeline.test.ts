import './mocks/browserStorage';
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  applyChatThreadServerEvent,
  ensureChatThreadRuntime,
  buildChatThreadMaterializedSlots,
  resetChatThreadRuntime,
  getChatThreadState
} from '../../src/realtime/chat/chatThreadRuntime';

const SESSION = 'probe-live-timeline';

class FakeStore {
  public runtimeProjectionVersionBySession: Record<string, number> = {};
  public runtimeProjectionContentVersionByMessage: Record<string, number> = {};
  public runtimeProjectionReasoningVersionByMessage: Record<string, number> = {};
  public activeSessionId = SESSION;
}

const TURN = 'T1';
let cursor = 0;

/** Full committed item row shape (feeder `payload`). */
const itemRow = (row: Record<string, unknown>, payload: Record<string, unknown>) => ({
  item_id: row.item_id,
  item_index: row.item_index ?? 1,
  kind: row.kind,
  status: row.status,
  revision: row.revision,
  payload,
  created_time: 1,
  updated_time: 1,
  turn_id: row.turn_id ?? TURN,
  visibility: row.visibility ?? 'user',
  root_turn_id: TURN,
  created_seq: row.created_seq ?? cursor + 1
});

const change = (store: FakeStore, data: Record<string, unknown>) =>
  applyChatThreadServerEvent(store, SESSION, 'thread_change', {
    event: 'thread_change',
    data: { ...data, cursor: ++cursor }
  });

const tail = (store: FakeStore, data: Record<string, unknown>) =>
  applyChatThreadServerEvent(store, SESSION, 'thread_item_tail', {
    event: 'thread_item_tail',
    data
  });

const assistantRow = () => {
  const slots = buildChatThreadMaterializedSlots(SESSION);
  return slots?.find((slot) => slot.assistant?.role === 'assistant')?.assistant ?? null;
};

const shape = (row: any): string[] =>
  (row?.timeline ?? []).map((block: any) =>
    block.kind === 'body'
      ? `body[${block.text}]`
      : `activity[${block.rows.map((r: any) => (r.type === 'reasoning' ? `r:${r.text}` : `t:${r.itemId}`)).join(',')}]`
  );

test('probe: live multi-round tool turn timeline', () => {
  resetChatThreadRuntime(SESSION);
  ensureChatThreadRuntime(SESSION);
  const store = new FakeStore();

  // 1. turn accepted (feeder turn_upsert payload shape)
  change(store, {
    change_type: 'turn_upsert',
    turn_id: TURN,
    payload: { turn_id: TURN, status: 'running', root_turn_id: TURN, trigger_kind: 'user', user_round: 1 }
  });
  // 2. user message item
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:user`,
    revision: 1,
    payload: itemRow(
      { item_id: `${TURN}:user`, kind: 'user_message', status: 'completed', revision: 1 },
      { item_id: `${TURN}:user`, turn_id: TURN, kind: 'user_message', status: 'completed', visibility: 'user', role: 'user', content: 'hello', client_message_id: 'cm1', user_round: 1 }
    )
  });
  // 3. llm_request registers text item round 1 (running)
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:text-1`,
    revision: 1,
    payload: itemRow(
      { item_id: `${TURN}:text-1`, kind: 'assistant_message', status: 'running', revision: 1 },
      { turn_id: TURN, model_round: 1, user_round: 1, item_id: `${TURN}:text-1`, kind: 'assistant_message', status: 'running', visibility: 'user', role: 'assistant', content: '', reasoning: '' }
    )
  });
  // 4. reasoning tail
  tail(store, { item_id: `${TURN}:text-1`, field: 'reasoning', offset: -1, base_seq: cursor, text: 'let me think' });
  // 5. content tail round 1
  tail(store, { item_id: `${TURN}:text-1`, field: 'content', offset: -1, base_seq: cursor, text: 'round one body ' });

  const afterRound1Text = assistantRow();
  console.log('AFTER ROUND1 TEXT:', JSON.stringify(shape(afterRound1Text)), 'content=', JSON.stringify(afterRound1Text?.content));

  // 6. tool_call item (running)
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:tool-call-1`,
    revision: 1,
    payload: itemRow(
      { item_id: `${TURN}:tool-call-1`, kind: 'tool_call', status: 'running', revision: 1 },
      { turn_id: TURN, model_round: 1, item_id: `${TURN}:tool-call-1`, kind: 'tool_call', status: 'running', visibility: 'user', event_type: 'tool_call', tool_call_id: 'call-1', tool: 'lookup', args: { q: 'x' } }
    )
  });
  const afterToolCall = assistantRow();
  console.log('AFTER TOOL_CALL:', JSON.stringify(shape(afterToolCall)), 'workflowItems=', afterToolCall?.workflowItems?.length);

  // 7. tool_result item (completed, revision 2)
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:tool-call-1`,
    revision: 2,
    payload: itemRow(
      { item_id: `${TURN}:tool-call-1`, kind: 'tool_call', status: 'completed', revision: 2 },
      { turn_id: TURN, model_round: 1, item_id: `${TURN}:tool-call-1`, kind: 'tool_call', status: 'completed', visibility: 'user', event_type: 'tool_result', tool_call_id: 'call-1', tool: 'lookup', ok: true, content: 'result-data' }
    )
  });
  // 8. completed item round 1
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:text-1`,
    revision: 2,
    payload: itemRow(
      { item_id: `${TURN}:text-1`, kind: 'assistant_message', status: 'completed', revision: 2 },
      { turn_id: TURN, model_round: 1, user_round: 1, role: 'assistant', content: 'round one body ', reasoning: 'let me think' }
    )
  });
  // 9. llm_request round 2 registers text-2
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:text-2`,
    revision: 1,
    payload: itemRow(
      { item_id: `${TURN}:text-2`, kind: 'assistant_message', status: 'running', revision: 1 },
      { turn_id: TURN, model_round: 2, user_round: 1, item_id: `${TURN}:text-2`, kind: 'assistant_message', status: 'running', visibility: 'user', role: 'assistant', content: '', reasoning: '' }
    )
  });
  // 10. content tail round 2
  tail(store, { item_id: `${TURN}:text-2`, field: 'content', offset: -1, base_seq: cursor, text: 'final answer' });
  // 11. completed item round 2
  change(store, {
    change_type: 'item_upsert',
    turn_id: TURN,
    item_id: `${TURN}:text-2`,
    revision: 2,
    payload: itemRow(
      { item_id: `${TURN}:text-2`, kind: 'assistant_message', status: 'completed', revision: 2 },
      { turn_id: TURN, model_round: 2, user_round: 1, role: 'assistant', content: 'final answer', reasoning: '' }
    )
  });
  // 12. turn completes
  change(store, {
    change_type: 'turn_status',
    turn_id: TURN,
    payload: { turn_id: TURN, status: 'completed' }
  });

  const row = assistantRow();
  console.log('FINAL SHAPE:', JSON.stringify(shape(row), null, 1));
  console.log('FINAL content=', JSON.stringify(row?.content), 'reasoning=', JSON.stringify(row?.reasoning));
  console.log('FINAL workflowItems=', row?.workflowItems?.length, JSON.stringify(row?.workflowItems?.map((i: any) => [i.id, i.status, i.toolName ?? i.tool])));
  const state = getChatThreadState(SESSION)!;
  console.log('ITEMS:', [...state.items.keys()]);
  console.log('ITEM KINDS:', [...state.items.values()].map((i) => `${i.itemId}:${i.kind}:${i.status}:r${i.revision}`));

  // Expectations: two body blocks + tool entry + reasoning entry
  assert.equal(row?.workflowItems?.length, 1, 'workflow items should contain the tool call');
  const blocks = row?.timeline ?? [];
  const bodyBlocks = blocks.filter((b: any) => b.kind === 'body');
  assert.equal(bodyBlocks.length, 2, 'two body blocks (intermediate + final)');
});
