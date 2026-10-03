// Thread streaming pipeline v2 — deterministic projection.
// Spec: docs/聊天流式管线根治方案.md §3.1 (I4: identity is rendering) and
// §3.4 (deterministic projection replaces coalesce/score).
//
// buildChatThreadRenderableMessages is a pure function of ChatThreadState:
// the same state always produces a deep-equal projection array. Ordering is
// defined, never guessed: turns by userRound and records by registration order
// (change_seq arrival). A user turn has exactly one user slot and one
// assistant slot; model rounds are sections inside that assistant bubble.
// No scoring, no string
// pattern matching, no localeCompare, no cross-call caching.

import type {
  ChatThreadState,
  ThreadItemState,
  ThreadTurnState
} from './chatThreadTypes';
import { composeItemText } from './chatThreadState';
import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageStatus,
  ChatRuntimeWorkflowItemProjection
} from './chatRuntimeTypes';

// Bubble ids are namespaced so they can never collide with legacy message ids.
const USER_BUBBLE_ID_PREFIX = 'tturn:';
const ASSISTANT_BUBBLE_ID_PREFIX = 'tturn:';

// Backend thread item kinds that render as workflow cards inside the
// assistant bubble of their user turn. Everything else (terminal,
// model_call, system_message, ...) is lifecycle bookkeeping, not a card.
const WORKFLOW_ITEM_KINDS = new Set(['tool_call', 'approval', 'plan', 'compaction', 'queue']);

// User-facing visibility. 'admin' is excluded per the v2 contract;
// 'model_internal' matches the legacy snapshot behaviour (never rendered).
const HIDDEN_ITEM_VISIBILITIES = new Set(['admin', 'model_internal']);

type WorkflowItemRecord = ChatRuntimeWorkflowItemProjection;

/** Fixed page schema: executions/items cannot add another assistant field. */
export interface ChatThreadTurnSlot {
  rootTurnId: string;
  key: string;
  user: ChatRuntimeMessageProjection;
  assistant: ChatRuntimeMessageProjection;
}

export const buildChatThreadTurnSlots = (
  state: ChatThreadState | null | undefined
): ChatThreadTurnSlot[] => {
  if (!state) return [];
  // Execution turns (goal continuations) retain their own item identities,
  // but belong to the initiating user's single assistant bubble.
  const roots = new Map<string, string>();
  for (const turn of state.turns.values()) roots.set(turn.turnId, turn.rootTurnId || turn.turnId);
  for (const item of state.items.values()) {
    if (typeof item.raw.root_turn_id === 'string' && item.raw.root_turn_id) {
      roots.set(item.turnId, item.raw.root_turn_id);
    }
  }
  const itemsByTurn = bucketVisibleItemsByTurn(state, roots);
  const executions = new Map<string, ThreadTurnState[]>();
  for (const turn of state.turns.values()) {
    const root = roots.get(turn.turnId) || turn.turnId;
    const group = executions.get(root) ?? [];
    group.push(turn);
    executions.set(root, group);
  }
  const slots: ChatThreadTurnSlot[] = [];
  const turns = [...state.turns.values()].sort(compareTurns);
  for (const turn of turns) {
    if (roots.get(turn.turnId) !== turn.turnId) continue;
    const items = itemsByTurn.get(turn.turnId) ?? [];
    const group = executions.get(turn.turnId) ?? [turn];
    // Map insertion follows durable acceptance order, including after reload.
    const latest = group[group.length - 1];
    const active = group.find(entry => entry.status && !['completed', 'failed', 'cancelled', 'interrupted', 'rejected', 'stopped'].includes(entry.status));
    // Only an accepted user input owns a page slot. Orphan execution data is
    // retained by the reducer until its root input arrives; never invent a row.
    if (turn.userContent === null) continue;
    slots.push(buildTurnSlot(state, { ...turn, status: (active ?? latest).status }, items));
  }
  return slots;
};

/** Compatibility view for non-page consumers; the page consumes slots directly. */
export const buildChatThreadRenderableMessages = (
  state: ChatThreadState | null | undefined
): ChatRuntimeMessageProjection[] => buildChatThreadTurnSlots(state).flatMap(slot => [slot.user, slot.assistant]);

// ---------------------------------------------------------------------------
// Turn assembly
// ---------------------------------------------------------------------------

const buildTurnSlot = (
  state: ChatThreadState,
  turn: ThreadTurnState,
  items: ThreadItemState[]
): ChatThreadTurnSlot => {
  const textItems: ThreadItemState[] = [];
  const workflows: ThreadItemState[] = [];
  for (const item of items) {
    // EventEmitter owns visible output with this stable identity. Execution
    // history snapshots use random ids and must never become duplicate bubbles.
    if (item.kind === 'assistant_message' && item.itemId === textItemId(item.turnId, item.modelRound)) {
      textItems.push(item);
    } else if (WORKFLOW_ITEM_KINDS.has(item.kind)) {
      workflows.push(item);
    }
  }
  return {
    rootTurnId: turn.turnId,
    key: turn.clientMessageId ? `turn:client:${turn.clientMessageId}` : `turn:${turn.turnId}`,
    user: buildUserBubble(turn, state),
    assistant: buildAssistantBubble(state, turn, items, textItems, workflows)
  };
};

const bucketVisibleItemsByTurn = (
  state: ChatThreadState,
  roots: Map<string, string>
): Map<string, ThreadItemState[]> => {
  const byTurn = new Map<string, ThreadItemState[]>();
  for (const item of state.items.values()) {
    if (item.visibility !== null && HIDDEN_ITEM_VISIBILITIES.has(item.visibility)) continue;
    const root = roots.get(item.turnId) || item.turnId;
    const bucket = byTurn.get(root);
    if (bucket) bucket.push(item);
    else byTurn.set(root, [item]);
  }
  return byTurn;
};

const compareTurns = (left: ThreadTurnState, right: ThreadTurnState): number => {
  if (left.userRound === null && right.userRound !== null) return 1;
  if (left.userRound !== null && right.userRound === null) return -1;
  if (left.userRound !== null && right.userRound !== null && left.userRound !== right.userRound) {
    return left.userRound - right.userRound;
  }
  // Stable lexicographic fallback on UTF-16 code units (never localeCompare).
  return left.turnId < right.turnId ? -1 : left.turnId > right.turnId ? 1 : 0;
};

const textItemId = (turnId: string, round: number): string => `${turnId}:text-${round}`;

// ---------------------------------------------------------------------------
// Bubbles
// ---------------------------------------------------------------------------

const buildUserBubble = (
  turn: ThreadTurnState,
  state: ChatThreadState
): ChatRuntimeMessageProjection => {
  const userItem = state.items.get(`${turn.turnId}:user`) ?? null;
  const seq = userItem?.order ?? 0;
  return {
    id: `${USER_BUBBLE_ID_PREFIX}${turn.turnId}:user`,
    role: 'user',
    content: turn.userContent ?? '',
    reasoning: '',
    status: 'final',
    createdAt: String(userItem?.raw.timestamp ?? ''),
    display: { ...(userItem?.raw.attachments ? { attachments: userItem.raw.attachments } : {}), user_round: turn.userRound },
    createdSeq: seq,
    updatedSeq: seq,
    userTurnId: turn.turnId,
    modelTurnId: '',
    final: true,
    failed: false,
    cancelled: false,
    raw: {
      client_message_id: turn.clientMessageId,
      turn_id: turn.turnId,
      item_id: `${turn.turnId}:user`,
      kind: 'user_message',
      revision: userItem?.revision ?? 0,
      content: turn.userContent ?? ''
    }
  };
};

const buildAssistantBubble = (
  state: ChatThreadState,
  turn: ThreadTurnState,
  turnItems: ThreadItemState[],
  textItems: ThreadItemState[],
  workflows: ThreadItemState[]
): ChatRuntimeMessageProjection => {
  const turnId = turn.turnId;
  const executionOrder = new Map<string, number>();
  for (const item of turnItems) {
    const order = Number(item.raw.created_seq) || item.order;
    executionOrder.set(item.turnId, Math.min(executionOrder.get(item.turnId) ?? order, order));
  }
  const compareExecutions = (a: ThreadItemState, b: ThreadItemState): number =>
    a.turnId === b.turnId ? 0 : (executionOrder.get(a.turnId) ?? 0) - (executionOrder.get(b.turnId) ?? 0);
  const orderedTextItems = textItems.slice().sort((a, b) => compareExecutions(a, b) || compareModelItems(a, b));
  const latestTextItem = orderedTextItems[orderedTextItems.length - 1] ?? null;
  // `append_chat` still records an immutable conversation-history snapshot
  // after a model round. It has no stable thread item id, so it is never a
  // bubble source. Its terminal message_stats are nevertheless authoritative
  // presentation metadata for the matching stable output item.
  const stats = resolveTurnAssistantStats(turnItems, orderedTextItems);
  const modelTurnId = `${turnId}:assistant`;
  // A user turn has one reply body. Earlier model rounds are execution
  // commentary, retained in ThreadLog rather than concatenated into the answer.
  // Do not fall back to an earlier round when the new reply has no text yet.
  const content = latestTextItem && !hasToolCalls(latestTextItem)
    ? composeItemText(state, latestTextItem.itemId, 'content')
    : '';
  const reasoning = composeTurnText(state, orderedTextItems, 'reasoning');
  const records = workflows
    .slice()
    .sort((a, b) => compareExecutions(a, b) || compareWorkflowItems(a, b))
    .map((item) => buildWorkflowRecord(item, modelTurnId));
  const createdSeq = orderedTextItems[0]?.order ?? (workflows.length > 0 ? minItemOrder(workflows) : 0);
  const updatedSeq = Math.max(
    latestTextItem?.order ?? 0,
    workflows.length > 0 ? maxItemOrder(workflows) : 0
  );
  const resolved = resolveBubbleStatus(turn, orderedTextItems, workflows);
  const message: ChatRuntimeMessageProjection = {
    id: `${ASSISTANT_BUBBLE_ID_PREFIX}${turnId}:assistant`,
    role: 'assistant',
    content,
    reasoning,
    status: resolved.status,
    createdAt: '',
    createdSeq,
    updatedSeq,
    userTurnId: turnId,
    modelTurnId,
    final: resolved.final,
    failed: resolved.failed,
    cancelled: resolved.cancelled,
    workflowItems: records,
    subagents: turnItems.filter(item => item.kind === 'subagent_run' && isPlainRecord(item.raw.runtime))
      .map(item => {
        const runtime = item.raw.runtime as Record<string, unknown>;
        return { ...runtime, key: item.itemId, updatedSeq: item.revision, durable: true, detail: runtime,
          canTerminate: runtime.can_terminate === true,
          updated_at: typeof runtime.updated_time === 'number' ? new Date(runtime.updated_time * 1000).toISOString() : '' };
      }),
    raw: buildAssistantBubbleRaw(turn, latestTextItem)
  };
  if (turn.userRound !== null || stats) {
    message.display = {
      ...(turn.userRound !== null ? { user_round: turn.userRound } : {}),
      ...(stats ? { stats } : {})
    };
  }
  if (resolved.cancelled) {
    // Cancellation settles this existing bubble. Its visible partial answer,
    // tool history and performance record remain attached to the same turn.
    message.display = {
      ...(message.display ?? {}),
      stopped: true,
      stop_reason: 'user_stop'
    };
  }
  if (resolved.failed) {
    const failure = [...turnItems].reverse().find(item => item.kind === 'terminal' || item.kind === 'error');
    if (failure) {
      const error = failure.raw.error;
      const detail = typeof error === 'string' ? error : isPlainRecord(error) ? firstText(error.message, error.code) : '';
      message.display = { ...(message.display ?? {}), failureDetail: detail || firstText(failure.raw.message, failure.raw.code) };
    }
  }
  return message;
};

/**
 * The visible assistant text has one stable identity per (turn, model round).
 * Conversation-history rows use random ids, so treating them as messages
 * creates duplicate bubbles. We retain only their aggregate stats, tied to the
 * newest stable round, and merge them with the stable item's live diagnostics.
 */
const resolveTurnAssistantStats = (
  items: ThreadItemState[],
  stableTextItems: ThreadItemState[]
): Record<string, unknown> | null => {
  const latestStable = stableTextItems[stableTextItems.length - 1];
  if (!latestStable) return null;
  const direct = extractItemStats(latestStable.raw);
  let persisted: Record<string, unknown> | null = null;
  let fallbackPersisted: Record<string, unknown> | null = null;
  for (const item of items) {
    if (item === latestStable || item.kind !== 'assistant_message' || item.role !== 'assistant') continue;
    if (item.turnId !== latestStable.turnId) continue;
    const stats = extractPersistedMessageStats(item.raw);
    if (!stats) continue;
    // append_chat history rows from tool/child-agent rounds can omit
    // model_round even though they carry the authoritative terminal timing.
    // Keep that row as a fallback for generation speed, but prefer an exact
    // stable-round match when available.
    if (item.modelRound === latestStable.modelRound) persisted = stats;
    if (hasGenerationSpeed(stats)) fallbackPersisted = stats;
  }
  persisted = persisted ?? fallbackPersisted;
  if (!direct && !persisted) return null;
  return { ...(direct ?? {}), ...(persisted ?? {}) };
};

const hasGenerationSpeed = (stats: Record<string, unknown>): boolean =>
  ['visible_decode_speed_tps', 'decode_speed_tps', 'avg_model_round_speed_tps',
    'avg_model_round_decode_speed_tps'].some((key) => {
      if (stats[key] === null || stats[key] === undefined || stats[key] === '') return false;
      const value = Number(stats[key]);
      return Number.isFinite(value) && value >= 0;
    });

const extractPersistedMessageStats = (payload: Record<string, unknown>): Record<string, unknown> | null => {
  const meta = isPlainRecord(payload.meta) ? payload.meta : null;
  return isPlainRecord(meta?.message_stats) ? meta.message_stats :
    isPlainRecord(payload.message_stats) ? payload.message_stats : null;
};

const extractItemStats = (payload: Record<string, unknown>): Record<string, unknown> | null => {
  const nested = isPlainRecord(payload.stats) ? payload.stats : null;
  const persisted = extractPersistedMessageStats(payload);
  // `llm_output` owns these per-round values. Copy only known diagnostics so
  // content and lifecycle data cannot leak into the presentation stats object.
  const directKeys = [
    'usage', 'round_usage', 'decode_output_tokens', 'decode_tokens',
    'decode_duration_s', 'decode_speed_tps', 'prefill_duration_s',
    'prefill_speed_tps', 'stream_timing', 'ttft_ms', 'tool_calls'
  ];
  const direct = Object.fromEntries(
    directKeys
      .filter((key) => payload[key] !== undefined)
      .map((key) => [key, payload[key]])
  );
  return nested || persisted || Object.keys(direct).length > 0
    ? { ...direct, ...(nested ?? {}), ...(persisted ?? {}) }
    : null;
};

const buildAssistantBubbleRaw = (
  turn: ThreadTurnState,
  textItem: ThreadItemState | null
): Record<string, unknown> => {
  const base: Record<string, unknown> = {
    client_message_id: turn.clientMessageId,
    turn_id: turn.turnId,
    model_round: textItem?.modelRound ?? 0
  };
  if (!textItem) return base;
  return {
    ...textItem.raw,
    ...base,
    item_id: textItem.itemId,
    turn_id: turn.turnId,
    model_round: textItem.modelRound,
    kind: textItem.kind,
    revision: textItem.revision
  };
};

type ResolvedBubbleStatus = {
  status: ChatRuntimeMessageStatus;
  final: boolean;
  failed: boolean;
  cancelled: boolean;
};

// Turn status is the authority for terminal flags (plan §3.4); inside a live
// turn the bubble mirrors the legacy semantics: tools running -> tooling,
// text streaming -> streaming, settled text -> final.
const resolveBubbleStatus = (
  turn: ThreadTurnState,
  textItems: ThreadItemState[],
  workflows: ThreadItemState[]
): ResolvedBubbleStatus => {
  const turnStatus = normalizeStatus(turn.status);
  if (turnStatus === 'completed') return { status: 'final', final: true, failed: false, cancelled: false };
  if (turnStatus === 'failed' || turnStatus === 'rejected') return { status: 'failed', final: false, failed: true, cancelled: false };
  if (['cancelled', 'interrupted', 'stopped'].includes(turnStatus)) return { status: 'cancelled', final: false, failed: false, cancelled: true };
  if (textItems.some((item) => item.status === 'failed')) return { status: 'failed', final: false, failed: true, cancelled: false };
  if (textItems.some((item) => item.status === 'cancelled')) return { status: 'cancelled', final: false, failed: false, cancelled: true };
  if (turnStatus === 'queued') return { status: 'queued', final: false, failed: false, cancelled: false };
  if (turnStatus === 'waiting' || turnStatus === 'waiting_input' || turnStatus === 'waiting_user_input' || turnStatus === 'waiting_approval') {
    return { status: 'queued', final: false, failed: false, cancelled: false };
  }
  if (workflows.some((item) => item.kind !== 'queue' && isActiveItemStatus(item.status))) {
    return { status: 'tooling', final: false, failed: false, cancelled: false };
  }
  if (textItems.some((item) => isActiveItemStatus(item.status))) {
    return { status: 'streaming', final: false, failed: false, cancelled: false };
  }
  // Model output and tools settle independently inside a user turn. Only
  // the durable turn terminal above may mark the assistant bubble complete.
  return { status: 'streaming', final: false, failed: false, cancelled: false };
};

// ---------------------------------------------------------------------------
// Workflow records (MessageToolWorkflow-compatible)
// ---------------------------------------------------------------------------

const workflowRecordCache = new WeakMap<ThreadItemState, { revision: number; modelTurnId: string; record: WorkflowItemRecord }>();

export const buildWorkflowRecord = (
  item: ThreadItemState,
  modelTurnId: string
): WorkflowItemRecord => {
  const cached = workflowRecordCache.get(item);
  if (cached?.revision === item.revision && cached.modelTurnId === modelTurnId) return cached.record;
  const payload = item.raw ?? {};
  const eventType = resolveWorkflowEventType(item);
  // Compaction is a workflow activity, not a model tool call. Its payload
  // need not contain a tool name; never infer its identity from a generic title.
  const toolName = item.kind === 'compaction'
    ? 'context_compaction'
    : firstText(payload.tool, payload.tool_name, payload.name, payload.toolName);
  const record: WorkflowItemRecord = {
    ...payload,
    ...(payload.request_usage ? { usage: payload.request_usage } : {}),
    id: item.itemId,
    eventType,
    status: mapWorkflowStatus(item.status),
    isTool: item.kind !== 'queue',
    title: firstText(payload.title, deriveWorkflowTitle(eventType, toolName)),
    detail: resolveWorkflowDetail(payload),
    modelTurnId,
    model_turn_id: modelTurnId,
    updatedSeq: item.revision
  };
  const args = payload.args ?? payload.arguments ?? payload.input;
  if (args !== undefined) {
    record.toolCallRawDetail = stringifyWorkflowDetail({ tool: toolName, arguments: args });
  }
  if (eventType === 'tool_result') record.toolResultRawDetail = stringifyWorkflowDetail(payload);
  if (toolName) {
    record.toolName = toolName;
    record.tool = toolName;
  }
  const toolCallId = firstText(payload.tool_call_id, payload.toolCallId, payload.call_id, payload.callId);
  if (toolCallId) record.toolCallId = toolCallId;
  copyWorkflowAliases(record, payload, 'tool_display_name', 'toolDisplayName', 'display_name', 'displayName', 'toolDisplayName', 'tool_display_name', 'displayName', 'display_name');
  copyWorkflowAliases(record, payload, 'tool_runtime_name', 'toolRuntimeName', 'runtime_name', 'runtimeName', 'toolRuntimeName', 'tool_runtime_name', 'runtimeName', 'runtime_name');
  copyWorkflowAliases(record, payload, 'tool_function_name', 'toolFunctionName', 'function_name', 'functionName', 'toolFunctionName', 'tool_function_name', 'functionName', 'function_name');
  copyWorkflowAliases(record, payload, 'command_session_id', 'commandSessionId', 'command_session_id', 'commandSessionId');
  workflowRecordCache.set(item, { revision: item.revision, modelTurnId, record });
  return record;
};

const copyWorkflowAliases = (
  record: WorkflowItemRecord,
  payload: Record<string, unknown>,
  ...keys: string[]
): void => {
  const camelKeys = keys.slice(0, keys.length / 2);
  const outputKeys = keys.slice(keys.length / 2);
  const value = firstText(...camelKeys.map((key) => payload[key]));
  if (!value) return;
  outputKeys.forEach((key) => {
    record[key] = value;
  });
};

const resolveWorkflowEventType = (item: ThreadItemState): string => {
  const rawType = normalizeStatus(firstText(item.raw?.event_type, item.raw?.eventType));
  if (item.kind === 'tool_call') {
    if (rawType === 'tool_call' || rawType === 'tool_result' ||
      rawType === 'tool_call_delta' || rawType === 'tool_output_delta') {
      return rawType;
    }
    return isActiveItemStatus(item.status) ? 'tool_call' : 'tool_result';
  }
  if (item.kind === 'approval') {
    if (rawType === 'approval_request' || rawType === 'approval_result' || rawType === 'approval_resolved') {
      return rawType === 'approval_request' ? 'approval_request' : 'approval_result';
    }
    return isActiveItemStatus(item.status) ? 'approval_request' : 'approval_result';
  }
  if (item.kind === 'plan') return rawType || 'plan_update';
  if (item.kind === 'queue') return rawType || (item.status === 'queued' ? 'queue_update' : 'queue_start');
  return rawType || 'compaction';
};

const mapWorkflowStatus = (status: string): 'loading' | 'completed' | 'failed' => {
  const normalized = normalizeStatus(status);
  if (isActiveItemStatus(normalized)) return 'loading';
  if (normalized === 'failed' || normalized === 'cancelled') return 'failed';
  return 'completed';
};

const deriveWorkflowTitle = (eventType: string, toolName: string): string => {
  if (eventType === 'approval_request') return toolName ? `Approval required: ${toolName}` : 'Approval required';
  if (eventType === 'approval_result') return toolName ? `Approval result: ${toolName}` : 'Approval result';
  if (eventType === 'tool_result') return toolName ? `Tool result: ${toolName}` : 'Tool result';
  return toolName ? `Tool call: ${toolName}` : 'Tool call';
};

const resolveWorkflowDetail = (payload: Record<string, unknown>): string => {
  if (typeof payload.detail === 'string' && payload.detail) return payload.detail;
  if (typeof payload.content === 'string' && payload.content) return payload.content;
  return stringifyWorkflowDetail(payload);
};

const stringifyWorkflowDetail = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (!isPlainRecord(value)) return String(value ?? '');
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return '';
  }
};

// ---------------------------------------------------------------------------
// Text composition
// ---------------------------------------------------------------------------

// Bubble text is composed by the state layer's composeItemText: durable blocks
// in offset order plus the uncovered tail suffix, falling back to the item
// payload when a field has neither. The projection adds no composition rules.

// ---------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------

const compareModelItems = (left: ThreadItemState, right: ThreadItemState): number =>
  left.modelRound !== right.modelRound ? left.modelRound - right.modelRound : left.order - right.order;

const compareWorkflowItems = (left: ThreadItemState, right: ThreadItemState): number => {
  const leftRound = explicitWorkflowRound(left);
  const rightRound = explicitWorkflowRound(right);
  if (leftRound === null && rightRound !== null) return 1;
  if (leftRound !== null && rightRound === null) return -1;
  if (leftRound !== null && rightRound !== null && leftRound !== rightRound) {
    return leftRound - rightRound;
  }
  // Durable identity, rather than arrival timing, makes a snapshot and its
  // replay produce the same workflow ordering.
  return left.itemId < right.itemId ? -1 : left.itemId > right.itemId ? 1 : 0;
};

const explicitWorkflowRound = (item: ThreadItemState): number | null => {
  const round = item.raw?.model_round;
  return typeof round === 'number' && Number.isFinite(round) ? round : null;
};

const composeTurnText = (
  state: ChatThreadState,
  items: ThreadItemState[],
  field: 'content' | 'reasoning'
): string => items
  .map((item) => composeItemText(state, item.itemId, field))
  .filter((text) => text.length > 0)
  .join('\n\n');

const hasToolCalls = (item: ThreadItemState): boolean =>
  Array.isArray(item.raw.tool_calls) && item.raw.tool_calls.length > 0;

const minItemOrder = (items: ThreadItemState[]): number =>
  items.reduce((min, item) => (item.order < min ? item.order : min), items[0]?.order ?? 0);

const maxItemOrder = (items: ThreadItemState[]): number =>
  items.reduce((max, item) => (item.order > max ? item.order : max), 0);

const isActiveItemStatus = (status: string): boolean => {
  const normalized = normalizeStatus(status);
  return normalized === 'running' || normalized === 'pending' || normalized === 'queued';
};

const normalizeStatus = (value: unknown): string =>
  String(value ?? '').trim().toLowerCase();

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};

const isPlainRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));
