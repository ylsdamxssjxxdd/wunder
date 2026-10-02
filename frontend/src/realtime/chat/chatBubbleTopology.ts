// Final chat-surface topology guard.
//
// ThreadLog projection is the authoritative source and already emits one user
// and one assistant message per durable turn. This guard sits immediately
// before MessengerView's virtual list as a defensive boundary for optimistic
// rows and transient history hydration: no upstream array can make the page
// render a second bubble for the same user turn.

export type ChatBubbleRenderable = {
  key: string;
  sourceIndex: number;
  message: Record<string, unknown>;
};

type BubbleSlot = {
  user: ChatBubbleRenderable | null;
  assistant: ChatBubbleRenderable | null;
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value !== 'string') continue;
    const text = value.trim();
    if (text) return text;
  }
  return '';
};

const userTurnIdOf = (message: Record<string, unknown>): string => firstText(
  message.__runtime_user_turn_id,
  message.user_turn_id,
  message.userTurnId,
  message.turn_id,
  message.turnId
);

const isGreeting = (message: Record<string, unknown>): boolean =>
  message.isGreeting === true || message.is_greeting === true;

const isTerminal = (message: Record<string, unknown>): boolean => {
  if (message.final === true || message.failed === true || message.cancelled === true) return true;
  const status = firstText(message.runtime_status, message.status, message.state).toLowerCase();
  return status === 'final' || status === 'completed' || status === 'done' ||
    status === 'failed' || status === 'error' || status === 'cancelled';
};

const mergeText = (left: unknown, right: unknown): string => {
  const first = typeof left === 'string' ? left : '';
  const second = typeof right === 'string' ? right : '';
  if (!first) return second;
  if (!second || first === second) return first;
  if (first.includes(second)) return first;
  if (second.includes(first)) return second;
  return `${first}\n\n${second}`;
};

const stableListIdentity = (value: unknown, index: number): string => {
  const item = value && typeof value === 'object' ? value as Record<string, unknown> : {};
  return firstText(item.id, item.item_id, item.itemId, item.tool_call_id, item.toolCallId) || `index:${index}`;
};

const mergeUniqueList = (left: unknown, right: unknown): unknown[] => {
  const result: unknown[] = [];
  const indexes = new Map<string, number>();
  for (const list of [left, right]) {
    if (!Array.isArray(list)) continue;
    list.forEach((item, index) => {
      const id = stableListIdentity(item, index);
      const priorIndex = indexes.get(id);
      if (priorIndex === undefined) {
        indexes.set(id, result.length);
        result.push(item);
      } else {
        // The second list is the chosen primary bubble and therefore owns the
        // latest durable version of a workflow or subagent record.
        result[priorIndex] = item;
      }
    });
  }
  return result;
};

const mergeStats = (left: unknown, right: unknown): Record<string, unknown> | undefined => {
  const first = left && typeof left === 'object' && !Array.isArray(left)
    ? left as Record<string, unknown>
    : null;
  const second = right && typeof right === 'object' && !Array.isArray(right)
    ? right as Record<string, unknown>
    : null;
  if (!first && !second) return undefined;
  return { ...(first ?? {}), ...(second ?? {}) };
};

const modelTurnIdOf = (message: Record<string, unknown>): string => firstText(
  message.__runtime_model_turn_id,
  message.model_turn_id,
  message.modelTurnId,
  message.model_round,
  message.modelRound
);

const preferIncoming = (current: Record<string, unknown>, incoming: Record<string, unknown>): boolean => {
  const currentDurable = current.__runtime_projected === true;
  const incomingDurable = incoming.__runtime_projected === true;
  if (currentDurable !== incomingDurable) return incomingDurable;
  const currentModelTurn = modelTurnIdOf(current);
  const incomingModelTurn = modelTurnIdOf(incoming);
  // Multiple model rounds remain sections of one assistant bubble. A later
  // round owns its live lifecycle, while updates for one model round retain
  // terminal precedence against stale streaming placeholders.
  if (currentModelTurn && incomingModelTurn && currentModelTurn !== incomingModelTurn) return true;
  const currentTerminal = isTerminal(current);
  const incomingTerminal = isTerminal(incoming);
  if (currentTerminal !== incomingTerminal) return incomingTerminal;
  return false;
};

const mergeBubble = (
  current: ChatBubbleRenderable,
  incoming: ChatBubbleRenderable,
  role: 'user' | 'assistant'
): ChatBubbleRenderable => {
  const currentMessage = current.message;
  const incomingMessage = incoming.message;
  const useIncoming = preferIncoming(currentMessage, incomingMessage);
  const primary = useIncoming ? incoming : current;
  const secondary = useIncoming ? current : incoming;
  const primaryMessage = primary.message;
  const secondaryMessage = secondary.message;
  const terminalPrimary = isTerminal(primaryMessage);
  const terminalSecondary = isTerminal(secondaryMessage);
  const sameModelTurn = modelTurnIdOf(primaryMessage) === modelTurnIdOf(secondaryMessage);
  const message: Record<string, unknown> = {
    ...secondaryMessage,
    ...primaryMessage,
    content: mergeText(secondaryMessage.content, primaryMessage.content),
    attachments: mergeUniqueList(secondaryMessage.attachments, primaryMessage.attachments)
  };
  if (role === 'assistant') {
    message.reasoning = mergeText(secondaryMessage.reasoning, primaryMessage.reasoning);
    message.workflowItems = mergeUniqueList(secondaryMessage.workflowItems, primaryMessage.workflowItems);
    message.subagents = mergeUniqueList(secondaryMessage.subagents, primaryMessage.subagents);
    const stats = mergeStats(secondaryMessage.stats, primaryMessage.stats);
    if (stats) message.stats = stats;
    // A stale optimistic running row is never allowed to overwrite an already
    // durable terminal bubble for the same turn.
    if (terminalPrimary) {
      // Terminal state is a hard boundary: none of the merged placeholders
      // may retain a streaming flag and visually reopen this bubble.
      message.stream_incomplete = false;
      message.workflowStreaming = false;
      message.reasoningStreaming = false;
    } else if (terminalSecondary && sameModelTurn) {
      for (const key of [
        'status', 'runtime_status', 'state', 'final', 'failed', 'cancelled',
        'stream_incomplete', 'workflowStreaming', 'reasoningStreaming'
      ]) {
        if (secondaryMessage[key] !== undefined) message[key] = secondaryMessage[key];
      }
    }
  }
  return { ...primary, message };
};

/**
 * Returns a display list with the hard chat contract:
 * - one user bubble per user turn;
 * - one assistant bubble per user turn;
 * - at most one assistant-only greeting before the first user turn.
 *
 * Items with no durable turn id use their preceding user row as a temporary
 * optimistic turn. They are replaced naturally when the durable turn arrives.
 */
export const enforceOneBubblePerChatTurn = (
  source: readonly ChatBubbleRenderable[]
): ChatBubbleRenderable[] => {
  const slots = new Map<string, BubbleSlot>();
  const order: string[] = [];
  let activeTurnKey = '';
  let anonymousTurn = 0;
  let greeting: ChatBubbleRenderable | null = null;

  const slotFor = (turnKey: string): BubbleSlot => {
    let slot = slots.get(turnKey);
    if (!slot) {
      slot = { user: null, assistant: null };
      slots.set(turnKey, slot);
      order.push(turnKey);
    }
    return slot;
  };

  source.forEach((candidate) => {
    const message = candidate?.message;
    if (!message || typeof message !== 'object') return;
    const role = firstText(message.role).toLowerCase();
    if (role !== 'user' && role !== 'assistant') return;
    if (role === 'assistant' && isGreeting(message)) {
      greeting = greeting ? mergeBubble(greeting, candidate, 'assistant') : candidate;
      return;
    }

    let turnKey = userTurnIdOf(message);
    if (role === 'user') {
      if (!turnKey) turnKey = `optimistic:${++anonymousTurn}`;
      activeTurnKey = turnKey;
    } else if (!turnKey) {
      turnKey = activeTurnKey || `orphan:${++anonymousTurn}`;
    }
    const slot = slotFor(turnKey);
    if (role === 'user') {
      slot.user = slot.user ? mergeBubble(slot.user, candidate, 'user') : candidate;
    } else {
      slot.assistant = slot.assistant ? mergeBubble(slot.assistant, candidate, 'assistant') : candidate;
    }
  });

  const result: ChatBubbleRenderable[] = [];
  if (greeting) result.push(greeting);
  for (const turnKey of order) {
    const slot = slots.get(turnKey);
    if (!slot) continue;
    if (slot.user) result.push(slot.user);
    if (slot.assistant) result.push(slot.assistant);
  }
  return result;
};
