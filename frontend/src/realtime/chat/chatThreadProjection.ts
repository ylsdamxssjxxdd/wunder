// Thread streaming pipeline v2 — deterministic projection.
// Spec: docs/聊天流式管线根治方案.md §3.1 (I4: identity is rendering) and
// §3.4 (deterministic projection replaces coalesce/score).
//
// buildChatThreadRenderableMessages is a pure function of ChatThreadState:
// the same state always produces a deep-equal projection array. Ordering is
// defined, never guessed: turns by userRound, bubbles by modelRound, tool
// records by registration order (change_seq arrival). No scoring, no string
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

// Bubble ids are namespaced so they can never collide with legacy v1 message
// ids while both pipelines exist side by side (plan §6 gradual rollout).
const USER_BUBBLE_ID_PREFIX = 'tturn:';
const ASSISTANT_BUBBLE_ID_PREFIX = 'titem:';

// Backend thread item kinds that render as workflow cards inside the
// assistant bubble of their model round. Everything else (terminal,
// model_call, system_message, ...) is lifecycle bookkeeping, not a card.
const WORKFLOW_ITEM_KINDS = new Set(['tool_call', 'approval', 'plan', 'compaction']);

// User-facing visibility. 'admin' is excluded per the v2 contract;
// 'model_internal' matches the legacy snapshot behaviour (never rendered).
const HIDDEN_ITEM_VISIBILITIES = new Set(['admin', 'model_internal']);

type WorkflowItemRecord = ChatRuntimeWorkflowItemProjection;

export const buildChatThreadRenderableMessages = (
  state: ChatThreadState | null | undefined
): ChatRuntimeMessageProjection[] => {
  if (!state) return [];
  const itemsByTurn = bucketVisibleItemsByTurn(state);
  const messages: ChatRuntimeMessageProjection[] = [];
  const turns = [...state.turns.values()].sort(compareTurns);
  for (const turn of turns) {
    const items = itemsByTurn.get(turn.turnId) ?? [];
    appendTurnMessages(messages, state, turn, items);
  }
  return messages;
};

// ---------------------------------------------------------------------------
// Turn assembly
// ---------------------------------------------------------------------------

const appendTurnMessages = (
  messages: ChatRuntimeMessageProjection[],
  state: ChatThreadState,
  turn: ThreadTurnState,
  items: ThreadItemState[]
): void => {
  const userContent = turn.userContent ?? '';
  if (userContent.length > 0) {
    messages.push(buildUserBubble(turn, state));
  }

  const workflowsByRound = new Map<number, ThreadItemState[]>();
  const orphanWorkflows: ThreadItemState[] = [];
  const rounds = new Set<number>();
  for (const item of items) {
    if (item.kind === 'assistant_message') {
      rounds.add(item.modelRound);
      continue;
    }
    if (!WORKFLOW_ITEM_KINDS.has(item.kind)) continue;
    const round = resolveWorkflowItemRound(item);
    if (round === null) {
      orphanWorkflows.push(item);
    } else {
      rounds.add(round);
      const bucket = workflowsByRound.get(round);
      if (bucket) bucket.push(item);
      else workflowsByRound.set(round, [item]);
    }
  }

  let lastBubble: ChatRuntimeMessageProjection | null = null;
  for (const round of [...rounds].sort((left, right) => left - right)) {
    const textItem = state.items.get(textItemId(turn.turnId, round)) ?? null;
    const workflows = workflowsByRound.get(round) ?? [];
    if (!textItem && workflows.length === 0) continue;
    const bubble = buildAssistantBubble(state, turn, round, textItem, workflows);
    messages.push(bubble);
    lastBubble = bubble;
  }

  if (orphanWorkflows.length === 0) return;
  if (lastBubble) {
    // Items without a payload model_round belong to the turn's latest bubble.
    const target = lastBubble;
    const existing = Array.isArray(target.workflowItems) ? target.workflowItems : [];
    target.workflowItems = existing.concat(
      orphanWorkflows
        .sort((left, right) => left.order - right.order)
        .map((item) => buildWorkflowRecord(item, target.modelTurnId))
    );
    target.updatedSeq = Math.max(target.updatedSeq, maxItemOrder(orphanWorkflows));
    return;
  }
  // No assistant bubble exists at all: keep the legacy standalone-workflow
  // convention (a placeholder assistant bubble carrying the workflow cards).
  const standaloneRound = resolveStandaloneWorkflowRound(items);
  messages.push(buildAssistantBubble(state, turn, standaloneRound, null, orphanWorkflows));
};

const bucketVisibleItemsByTurn = (
  state: ChatThreadState
): Map<string, ThreadItemState[]> => {
  const byTurn = new Map<string, ThreadItemState[]>();
  for (const item of state.items.values()) {
    if (item.visibility !== null && HIDDEN_ITEM_VISIBILITIES.has(item.visibility)) continue;
    const bucket = byTurn.get(item.turnId);
    if (bucket) bucket.push(item);
    else byTurn.set(item.turnId, [item]);
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
    createdAt: '',
    createdSeq: seq,
    updatedSeq: seq,
    userTurnId: turn.turnId,
    modelTurnId: '',
    final: true,
    failed: false,
    cancelled: false,
    raw: {
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
  round: number,
  textItem: ThreadItemState | null,
  workflows: ThreadItemState[]
): ChatRuntimeMessageProjection => {
  const turnId = turn.turnId;
  const itemId = textItemId(turnId, round);
  const modelTurnId = `${turnId}:round-${round}`;
  const content = textItem ? composeItemText(state, itemId, 'content') : '';
  const reasoning = textItem ? composeItemText(state, itemId, 'reasoning') : '';
  const records = workflows
    .slice()
    .sort((left, right) => left.order - right.order)
    .map((item) => buildWorkflowRecord(item, modelTurnId));
  const createdSeq = textItem?.order ?? (workflows.length > 0 ? minItemOrder(workflows) : 0);
  const updatedSeq = Math.max(
    textItem?.order ?? 0,
    workflows.length > 0 ? maxItemOrder(workflows) : 0
  );
  const resolved = resolveBubbleStatus(turn, textItem, workflows);
  const message: ChatRuntimeMessageProjection = {
    id: `${ASSISTANT_BUBBLE_ID_PREFIX}${itemId}`,
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
    subagents: [],
    raw: buildAssistantBubbleRaw(turn, round, textItem)
  };
  if (turn.userRound !== null) {
    message.display = { user_round: turn.userRound };
  }
  return message;
};

const buildAssistantBubbleRaw = (
  turn: ThreadTurnState,
  round: number,
  textItem: ThreadItemState | null
): Record<string, unknown> => {
  const base: Record<string, unknown> = {
    turn_id: turn.turnId,
    model_round: round
  };
  if (!textItem) return base;
  return {
    ...textItem.raw,
    item_id: textItem.itemId,
    turn_id: turn.turnId,
    model_round: round,
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
  textItem: ThreadItemState | null,
  workflows: ThreadItemState[]
): ResolvedBubbleStatus => {
  const turnStatus = normalizeStatus(turn.status);
  if (turnStatus === 'completed') return { status: 'final', final: true, failed: false, cancelled: false };
  if (turnStatus === 'failed') return { status: 'failed', final: false, failed: true, cancelled: false };
  if (turnStatus === 'cancelled') return { status: 'cancelled', final: false, failed: false, cancelled: true };
  if (textItem?.status === 'failed') return { status: 'failed', final: false, failed: true, cancelled: false };
  if (textItem?.status === 'cancelled') return { status: 'cancelled', final: false, failed: false, cancelled: true };
  if (turnStatus === 'queued') return { status: 'queued', final: false, failed: false, cancelled: false };
  if (workflows.some((item) => isActiveItemStatus(item.status))) {
    return { status: 'tooling', final: false, failed: false, cancelled: false };
  }
  if (textItem && isActiveItemStatus(textItem.status)) {
    return { status: 'streaming', final: false, failed: false, cancelled: false };
  }
  if (textItem && normalizeStatus(textItem.status) === 'completed') {
    return { status: 'final', final: true, failed: false, cancelled: false };
  }
  return { status: 'streaming', final: false, failed: false, cancelled: false };
};

// ---------------------------------------------------------------------------
// Workflow records (MessageToolWorkflow-compatible)
// ---------------------------------------------------------------------------

const buildWorkflowRecord = (
  item: ThreadItemState,
  modelTurnId: string
): WorkflowItemRecord => {
  const payload = item.raw ?? {};
  const eventType = resolveWorkflowEventType(item);
  const toolName = firstText(payload.tool, payload.tool_name, payload.name, payload.toolName);
  const record: WorkflowItemRecord = {
    id: item.itemId,
    eventType,
    status: mapWorkflowStatus(item.status),
    isTool: true,
    title: firstText(payload.title, deriveWorkflowTitle(eventType, toolName)),
    detail: resolveWorkflowDetail(payload),
    modelTurnId,
    model_turn_id: modelTurnId,
    updatedSeq: item.revision
  };
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

const resolveWorkflowItemRound = (item: ThreadItemState): number | null => {
  const raw = item.raw?.model_round;
  return typeof raw === 'number' && Number.isFinite(raw) ? raw : null;
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
  const data = payload.data;
  if (isPlainRecord(data)) return stringifyWorkflowDetail(data);
  return '';
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

const resolveStandaloneWorkflowRound = (items: ThreadItemState[]): number => {
  let max = 0;
  for (const item of items) {
    if (Number.isFinite(item.modelRound) && item.modelRound > max) max = item.modelRound;
  }
  return max;
};

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
