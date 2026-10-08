// Dev-harness bridge: feed the durable thread runtime (v2).
//
// MessengerView renders exclusively from `buildChatThreadMaterializedSlots`
// (thread v2). Probes that only wrote the legacy projection (`chat.messages` +
// `applyCanonicalStreamRuntimeEvent`) produced an empty page, so every dev
// probe installs its fixture through the same entry points the application
// uses: the reload snapshot (`hydrateChatThreadRuntime`) and server frames
// (`applyChatThreadServerEvent`).
import { useChatStore } from '@/stores/chat';
import { applyChatThreadEffects } from '@/stores/chatThreadEffects';
import {
  applyChatThreadServerEvent,
  getChatThreadState,
  hydrateChatThreadRuntime,
  resetChatThreadRuntime
} from '@/realtime/chat/chatThreadRuntime';
import { composeItemText } from '@/realtime/chat/chatThreadState';
import type {
  ChatThreadSnapshot,
  ChatThreadSnapshotTurn
} from '@/realtime/chat/chatThreadState';
import type { ThreadBlockUpsertPayload, ThreadItemUpsertPayload } from '@/realtime/chat/chatThreadTypes';

type FixtureMessage = Record<string, any>;

/** Fixture event types that render as workflow entries inside the turn. */
const WORKFLOW_KIND_BY_EVENT: Record<string, string> = {
  tool_call: 'tool_call',
  tool_call_delta: 'tool_call',
  tool_output: 'tool_call',
  tool_output_delta: 'tool_call',
  tool_result: 'tool_call',
  approval_request: 'approval',
  approval_result: 'approval',
  approval_resolved: 'approval',
  plan_update: 'plan',
  goal_update: 'plan',
  goal_completed: 'plan',
  compaction: 'compaction',
  compaction_progress: 'compaction',
  queue_enter: 'queue',
  queue_update: 'queue',
  queue_start: 'queue'
};

const workflowItemsOf = (message: FixtureMessage): FixtureMessage[] =>
  Array.isArray(message.workflowItems) ? message.workflowItems : [];

/**
 * Build the reload snapshot for the plain message fixtures the dev probes use.
 * Messages pair up by `user_turn_id`: the user row owns the slot, the assistant
 * row is its single reply body, `workflowItems` become durable tool rows.
 */
export const buildThreadSnapshotFromMessages = (
  sessionId: string,
  messages: FixtureMessage[]
): ChatThreadSnapshot => {
  const turns: ChatThreadSnapshotTurn[] = [];
  const items: ThreadItemUpsertPayload[] = [];
  const blocks: ThreadBlockUpsertPayload[] = [];
  const roundByTurn = new Map<string, number>();
  let round = 0;
  messages.forEach((message, index) => {
    const turnId = String(message.user_turn_id || `${sessionId}:turn-${Math.floor(index / 2) + 1}`);
    const isUser = String(message.role || '') === 'user';
    if (!roundByTurn.has(turnId)) {
      round += 1;
      roundByTurn.set(turnId, round);
      turns.push({
        turn_id: turnId,
        root_turn_id: String(message.root_turn_id || turnId),
        user_round: Number(message.user_round ?? round),
        status: isUser ? 'completed' : 'running',
        content: ''
      });
    }
    const userRound = roundByTurn.get(turnId);
    if (isUser) {
      const turn = turns.find((entry) => entry.turn_id === turnId);
      if (turn) {
        turn.content = String(message.content ?? '');
        turn.status = 'completed';
      }
      items.push({
        item_id: `${turnId}:user`,
        turn_id: turnId,
        kind: 'user_message',
        role: 'user',
        status: 'completed',
        visibility: 'user',
        revision: 1,
        user_round: userRound,
        content: String(message.content ?? ''),
        timestamp: message.created_at
      });
      return;
    }
    const turn = turns.find((entry) => entry.turn_id === turnId);
    if (turn) turn.status = message.stream_incomplete ? 'running' : 'completed';
    const modelRound = Number(message.model_round ?? 1) || 1;
    const textItemId = `${turnId}:text-${modelRound}`;
    const content = String(message.content ?? '');
    items.push({
      ...(message.stats ? { stats: message.stats } : {}),
      ...(message.meta ? { meta: message.meta } : {}),
      item_id: textItemId,
      turn_id: turnId,
      kind: 'assistant_message',
      role: 'assistant',
      model_round: modelRound,
      status: message.stream_incomplete ? 'running' : 'completed',
      visibility: 'user',
      revision: 2,
      user_round: userRound,
      content,
      ...(message.reasoning ? { reasoning: String(message.reasoning) } : {}),
      ...(typeof message.model_turn_id === 'string' ? { model_turn_id: message.model_turn_id } : {})
    });
    if (content) {
      // 块身份由 `item_id` + `field` + `block_index` 决定，`turn_id` 不在
      // `ThreadBlockUpsertPayload` 里（块的应用路径从不读它），这里不再多带。
      blocks.push({
        item_id: textItemId,
        field: 'content',
        block_index: 0,
        content_offset: 0,
        content
      });
    }
    workflowItemsOf(message).forEach((entry, itemIndex) => {
      const eventType = String(entry.eventType || entry.event_type || 'tool_result');
      items.push({
        ...entry,
        item_id: `${turnId}:item-${itemIndex}`,
        turn_id: turnId,
        kind: WORKFLOW_KIND_BY_EVENT[eventType] || 'tool_call',
        status: String(entry.status || 'completed'),
        visibility: 'user',
        revision: itemIndex + 1,
        model_round: modelRound,
        event_type: eventType
      });
    });
  });
  return { cursor: Math.max(1, items.length), turns, items, blocks };
};

/** Replace the session's durable state with the fixture (same as a reload). */
export const installChatThreadFixture = (
  store: ReturnType<typeof useChatStore>,
  sessionId: string,
  messages: FixtureMessage[]
): void => {
  resetChatThreadRuntime(sessionId);
  hydrateChatThreadRuntime(store, sessionId, buildThreadSnapshotFromMessages(sessionId, messages));
};

/** Next durable cursor; frames must arrive contiguously (state.lastSeq + 1). */
export const nextThreadCursor = (sessionId: string): number =>
  (getChatThreadState(sessionId)?.lastSeq ?? 0) + 1;

/** Apply one server frame through the production adapter. */
export const applyThreadFrame = (
  store: ReturnType<typeof useChatStore>,
  sessionId: string,
  eventType: string,
  payload: Record<string, unknown>
): boolean => applyChatThreadServerEvent(store, sessionId, eventType, payload, {
  onChangesApplied: (changes) => applyChatThreadEffects(store, sessionId, changes)
});

/** One durable change frame with the next cursor. */
export const applyThreadChange = (
  store: ReturnType<typeof useChatStore>,
  sessionId: string,
  changeType: string,
  payload: Record<string, unknown>
): boolean => applyThreadFrame(store, sessionId, 'thread_change', {
  cursor: nextThreadCursor(sessionId),
  change_type: changeType,
  turn_id: payload.turn_id,
  item_id: payload.item_id,
  revision: payload.revision,
  payload
});

/** Latest assistant text item of the session, with its composed text. */
export const resolveThreadTextItem = (
  sessionId: string
): { itemId: string; turnId: string; item: Record<string, unknown>; content: string } | null => {
  const state = getChatThreadState(sessionId);
  if (!state) return null;
  let latest: { itemId: string; turnId: string; item: Record<string, unknown>; order: number } | null = null;
  state.items.forEach((item) => {
    if (item.kind !== 'assistant_message') return;
    if (!latest || item.order > latest.order) {
      latest = { itemId: item.itemId, turnId: item.turnId, item: item.raw, order: item.order };
    }
  });
  if (!latest) return null;
  const resolved = latest as { itemId: string; turnId: string; item: Record<string, unknown> };
  return { ...resolved, content: composeItemText(state, resolved.itemId, 'content') };
};

/**
 * The newest assistant row that the virtual window currently has mounted.
 * Streaming probes must target a rendered row: the window mounts only a few of
 * the fixture's 160 turns, so the newest turn is usually off-screen.
 */
export const resolveRenderedThreadTextItem = (
  sessionId: string
): { itemId: string; turnId: string; item: Record<string, unknown>; content: string } | null => {
  const state = getChatThreadState(sessionId);
  if (!state) return null;
  const rows = Array.from(document.querySelectorAll<HTMLElement>(
    '.messenger-turn-assistant [data-virtual-key^="runtime:assistant:tturn:"]'
  ));
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const turnId = String(rows[index].getAttribute('data-virtual-key') || '')
      .replace('runtime:assistant:tturn:', '').replace(/:assistant$/, '');
    if (!turnId) continue;
    let found: { itemId: string; turnId: string; item: Record<string, unknown>; order: number } | null = null;
    state.items.forEach((item) => {
      if (item.turnId !== turnId || item.kind !== 'assistant_message') return;
      if (!found || item.order > found.order) {
        found = { itemId: item.itemId, turnId: item.turnId, item: item.raw, order: item.order };
      }
    });
    if (found) {
      const resolved = found as { itemId: string; turnId: string; item: Record<string, unknown> };
      return { ...resolved, content: composeItemText(state, resolved.itemId, 'content') };
    }
  }
  return resolveThreadTextItem(sessionId);
};

/** Append streamed text through the ephemeral tail frame (arrival-order). */
export const applyThreadTail = (
  store: ReturnType<typeof useChatStore>,
  sessionId: string,
  itemId: string,
  text: string,
  field: 'content' | 'reasoning' = 'content'
): boolean => applyThreadFrame(store, sessionId, 'thread_item_tail', {
  item_id: itemId,
  field,
  offset: -1,
  text
});

/** Settle an item's text as a durable block snapshot (heals the tail). */
export const applyThreadTextBlock = (
  store: ReturnType<typeof useChatStore>,
  sessionId: string,
  itemId: string,
  turnId: string,
  content: string,
  field: 'content' | 'reasoning' = 'content'
): boolean => applyThreadFrame(store, sessionId, 'thread_change', {
  cursor: nextThreadCursor(sessionId),
  change_type: 'text_block',
  item_id: itemId,
  turn_id: turnId,
  payload: { item_id: itemId, turn_id: turnId, field, block_index: 0, content_offset: 0, [field]: content }
});
