import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageStatus,
  ChatRuntimeProjection,
  ChatRuntimeSessionProjection,
  ChatSessionRuntimeStatus
} from './chatRuntimeTypes';
import { isChatRuntimeBusyStatus, normalizeChatRuntimeStatus } from './chatRuntimeReducer';

type ChatMessageLike = Record<string, unknown>;

export const selectChatRuntimeSession = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatRuntimeSessionProjection | null => {
  const key = String(sessionId ?? '').trim();
  if (!key) return null;
  return projection?.sessions?.[key] || null;
};

export const selectChatRuntimeMessage = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  messageId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const key = String(messageId ?? '').trim();
  if (!session || !key) return null;
  return session.messageById[key] || null;
};

export const selectSessionBusy = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): boolean => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return false;
  return Boolean(session.busyReason) && isChatRuntimeBusyStatus(session.runtimeStatus);
};

export const selectSessionBusyReason = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
) => selectChatRuntimeSession(projection, sessionId)?.busyReason ?? null;

export const selectSessionRuntimeStatus = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatSessionRuntimeStatus => {
  const session = selectChatRuntimeSession(projection, sessionId);
  return normalizeChatRuntimeStatus(session?.runtimeStatus);
};

export const selectRuntimeLastAppliedEventId = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): number => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return 0;
  const direct = normalizePositiveEventId(session.lastAppliedEventId);
  if (direct > 0) return direct;
  return Object.keys(session.eventIdIndex || {}).reduce((maxEventId, eventId) => {
    const normalized = normalizePositiveEventId(eventId);
    return normalized > maxEventId ? normalized : maxEventId;
  }, 0);
};

const normalizePositiveEventId = (value: unknown): number => {
  const text = String(value ?? '').trim();
  if (!/^\d+$/.test(text)) return 0;
  const parsed = Number.parseInt(text, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

export const selectCanSend = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): boolean => !selectSessionBusy(projection, sessionId);

export const selectVisibleMessageProjections = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown
): ChatRuntimeMessageProjection[] => {
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return [];
  const ordered: ChatRuntimeMessageProjection[] = [];
  const seen = new Set<string>();
  const pushMessage = (messageId: string): void => {
    if (!messageId || seen.has(messageId)) return;
    const message = session.messageById[messageId];
    if (!message) return;
    if (isSyntheticGreetingProjection(message)) return;
    ordered.push(message);
    seen.add(messageId);
  };
  session.userTurns.forEach((turnId) => {
    const userTurn = session.userTurnById[turnId];
    if (!userTurn) return;
    userTurn.messageIds.forEach(pushMessage);
    userTurn.modelTurnIds.forEach((modelTurnId) => {
      const modelTurn = session.modelTurnById[modelTurnId];
      modelTurn?.messageIds?.forEach(pushMessage);
    });
  });
  session.messages.forEach(pushMessage);
  const turnOrder = buildMessageTurnOrder(session);
  const sorted = ordered.sort((left, right) => {
    const leftTurnOrder = resolveMessageTurnOrder(turnOrder, left);
    const rightTurnOrder = resolveMessageTurnOrder(turnOrder, right);
    if (leftTurnOrder !== rightTurnOrder) {
      return leftTurnOrder - rightTurnOrder;
    }
    if (left.role !== right.role) {
      return left.role === 'user' ? -1 : 1;
    }
    if (left.createdSeq !== right.createdSeq) return left.createdSeq - right.createdSeq;
    return left.id.localeCompare(right.id);
  });
  return coalesceVisibleMessagesByTurn(session, sorted);
};

const isSyntheticGreetingProjection = (
  message: ChatRuntimeMessageProjection | null | undefined
): boolean => Boolean(message?.display?.isGreeting === true || message?.display?.is_greeting === true);

const coalesceVisibleMessagesByTurn = (
  session: ChatRuntimeSessionProjection,
  sorted: ChatRuntimeMessageProjection[]
): ChatRuntimeMessageProjection[] => {
  const result: ChatRuntimeMessageProjection[] = [];
  const userIndexByTurn = new Map<string, number>();
  const assistantIndexByTurn = new Map<string, number>();
  sorted.forEach((message) => {
    if (!shouldCoalesceMessageByTurn(message)) {
      result.push(message);
      return;
    }
    const turnKey = resolveCoalescingTurnKey(message);
    if (!turnKey) {
      result.push(message);
      return;
    }
    const indexByTurn = message.role === 'user' ? userIndexByTurn : assistantIndexByTurn;
    const existingIndex = indexByTurn.get(turnKey);
    if (existingIndex === undefined) {
      const fallbackIndex = message.role === 'assistant'
        ? result.findIndex((candidate) => shouldMergeCrossKeyTransientAssistantDuplicate(
          session,
          candidate,
          message
        ))
        : -1;
      if (fallbackIndex >= 0) {
        result[fallbackIndex] = mergeVisibleMessageProjection(
          session,
          result[fallbackIndex],
          message
        );
        indexByTurn.set(turnKey, fallbackIndex);
        return;
      }
      indexByTurn.set(turnKey, result.length);
      result.push(message);
      return;
    }
    if (!shouldMergeVisibleDuplicate(session, result[existingIndex], message)) {
      result.push(message);
      return;
    }
    result[existingIndex] = mergeVisibleMessageProjection(
      session,
      result[existingIndex],
      message
    );
  });
  return result;
};

const shouldCoalesceMessageByTurn = (
  message: ChatRuntimeMessageProjection
): boolean => {
  if (message.role === 'user') return true;
  if (message.role !== 'assistant') return false;
  if (isSpecialAssistantProjection(message)) return false;
  return true;
};

const shouldMergeVisibleDuplicate = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  if (left.role === 'user' && right.role === 'user') return true;
  if (left.role !== 'assistant' || right.role !== 'assistant') return false;
  if (isSpecialAssistantProjection(left) || isSpecialAssistantProjection(right)) return false;
  if (shouldMergeTransientAssistantDuplicate(session, left, right)) return true;
  if (shouldMergeSameRoundSupplementalAssistantDuplicate(left, right)) return true;
  const hasLeftPayload = hasVisibleAssistantPayload(left);
  const hasRightPayload = hasVisibleAssistantPayload(right);
  if (!hasLeftPayload || !hasRightPayload) return true;
  if (isQueueOnlyAssistantProjection(left) || isQueueOnlyAssistantProjection(right)) return true;
  return false;
};

const shouldMergeCrossKeyTransientAssistantDuplicate = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  if (left.role !== 'assistant' || right.role !== 'assistant') return false;
  if (isSpecialAssistantProjection(left) || isSpecialAssistantProjection(right)) return false;
  return shouldMergeTransientAssistantDuplicate(session, left, right) ||
    shouldMergeSameRoundSupplementalAssistantDuplicate(left, right);
};

const hasVisibleAssistantPayload = (
  message: ChatRuntimeMessageProjection
): boolean =>
  Boolean(
    String(message.content || '').trim() ||
      String(message.reasoning || '').trim() ||
      (Array.isArray(message.workflowItems) && message.workflowItems.length > 0) ||
      (Array.isArray(message.subagents) && message.subagents.length > 0)
  );

const isQueueOnlyAssistantProjection = (
  message: ChatRuntimeMessageProjection
): boolean =>
  message.status !== 'failed' &&
  message.status !== 'cancelled' &&
  !String(message.content || '').trim() &&
  !String(message.reasoning || '').trim() &&
  Array.isArray(message.workflowItems) &&
  message.workflowItems.length > 0 &&
  message.workflowItems.every((item) => {
    if (!isPlainRecord(item)) return false;
    const eventType = normalizeText(item.eventType ?? item.event_type ?? item.event ?? item.sourceEventType ?? item.source_event_type);
    return isQueueWorkflowEventType(eventType);
  });

const resolveCoalescingTurnKey = (
  message: ChatRuntimeMessageProjection
): string => {
  if (message.role === 'assistant') {
    const semanticRound = resolveAssistantSemanticUserRound(message);
    if (semanticRound !== null && isTransientAssistantProjectionForKey(message)) {
      return `${message.role}:round:${semanticRound}`;
    }
  }
  const turnId = String(message.userTurnId || '').trim();
  if (turnId) return `${message.role}:${turnId}`;
  return '';
};

const shouldMergeTransientAssistantDuplicate = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  const leftRound = resolveAssistantSemanticUserRound(left);
  const rightRound = resolveAssistantSemanticUserRound(right);
  if (leftRound === null || rightRound === null) {
    return shouldMergeUnknownRoundQueueStartAssistant(left, right);
  }
  if (leftRound !== rightRound) return false;
  const leftTransient = isTransientAssistantProjection(session, left);
  const rightTransient = isTransientAssistantProjection(session, right);
  if (!leftTransient && !rightTransient) return false;
  if (hasQueueWorkflowProjection(left) || hasQueueWorkflowProjection(right)) return true;
  const leftModelRound = resolveAssistantModelRound(left);
  const rightModelRound = resolveAssistantModelRound(right);
  return leftModelRound === null || rightModelRound === null;
};

const shouldMergeSameRoundSupplementalAssistantDuplicate = (
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  const leftRound = resolveAssistantSemanticUserRound(left);
  const rightRound = resolveAssistantSemanticUserRound(right);
  if (leftRound === null || rightRound === null || leftRound !== rightRound) return false;
  if (hasSupplementalAssistantPayload(left) || hasSupplementalAssistantPayload(right)) {
    return true;
  }
  return areAssistantTextsCompatible(left, right) &&
    (isHydratedHistoryProjection(left) || isHydratedHistoryProjection(right));
};

const hasSupplementalAssistantPayload = (
  message: ChatRuntimeMessageProjection
): boolean =>
  !hasAssistantText(message) &&
  (
    (Array.isArray(message.workflowItems) && message.workflowItems.length > 0) ||
    (Array.isArray(message.subagents) && message.subagents.length > 0)
  );

const hasAssistantText = (message: ChatRuntimeMessageProjection): boolean =>
  Boolean(String(message.content || '').trim() || String(message.reasoning || '').trim());

const areAssistantTextsCompatible = (
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  const leftText = `${String(left.reasoning || '').trim()}\n${String(left.content || '').trim()}`.trim();
  const rightText = `${String(right.reasoning || '').trim()}\n${String(right.content || '').trim()}`.trim();
  if (!leftText || !rightText) return true;
  return leftText === rightText || leftText.startsWith(rightText) || rightText.startsWith(leftText);
};

const isHydratedHistoryProjection = (
  message: ChatRuntimeMessageProjection
): boolean => {
  const id = String(message.id || '').trim();
  if (id.startsWith('history:')) return true;
  const raw = isPlainRecord(message.raw) ? message.raw : {};
  return normalizeText(raw.source ?? raw.source_type ?? raw.sourceType) === 'history';
};

const shouldMergeUnknownRoundQueueStartAssistant = (
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): boolean => {
  const leftRound = resolveAssistantSemanticUserRound(left);
  const rightRound = resolveAssistantSemanticUserRound(right);
  if (leftRound === null && rightRound === null) return false;
  const unknown = leftRound === null ? left : right;
  const known = leftRound === null ? right : left;
  if (!hasQueueStartWorkflowProjection(unknown)) return false;
  if (String(unknown.content || '').trim() || String(unknown.reasoning || '').trim()) return false;
  if (isSpecialAssistantProjection(known)) return false;
  return known.role === 'assistant' && (
    isRuntimeMessageActive(known.status) ||
    Boolean(String(known.content || '').trim() || String(known.reasoning || '').trim())
  );
};

const isTransientAssistantProjection = (
  session: ChatRuntimeSessionProjection,
  message: ChatRuntimeMessageProjection
): boolean => {
  if (isQueueOnlyAssistantProjection(message) || hasQueueWorkflowProjection(message)) return true;
  const userTurnId = String(message.userTurnId || '').trim();
  if (userTurnId.startsWith('queue-turn:')) return true;
  const modelTurnId = String(message.modelTurnId || '').trim();
  if (modelTurnId === `model-turn:${userTurnId}`) return true;
  if (modelTurnId.startsWith(`model-turn:${session.sessionId}:request:`)) return true;
  return resolveAssistantSemanticUserRound(message) !== null &&
    resolveAssistantModelRound(message) === null &&
    modelTurnId.startsWith(`model-turn:${session.sessionId}:user:`);
};

const isTransientAssistantProjectionForKey = (
  message: ChatRuntimeMessageProjection
): boolean => {
  if (isQueueOnlyAssistantProjection(message) || hasQueueWorkflowProjection(message)) return true;
  const userTurnId = String(message.userTurnId || '').trim();
  if (userTurnId.startsWith('queue-turn:')) return true;
  const modelTurnId = String(message.modelTurnId || '').trim();
  return resolveAssistantModelRound(message) === null &&
    (modelTurnId.startsWith('model-turn:') || modelTurnId.startsWith('queue:'));
};

const hasQueueWorkflowProjection = (
  message: ChatRuntimeMessageProjection
): boolean => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return items.some((item) => {
    if (!isPlainRecord(item)) return false;
    const eventType = normalizeText(item.eventType ?? item.event_type ?? item.event ?? item.sourceEventType ?? item.source_event_type);
    return isQueueWorkflowEventType(eventType);
  });
};

const hasQueueStartWorkflowProjection = (
  message: ChatRuntimeMessageProjection
): boolean => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return items.some((item) => {
    if (!isPlainRecord(item)) return false;
    const eventType = normalizeText(item.eventType ?? item.event_type ?? item.event ?? item.sourceEventType ?? item.source_event_type);
    return eventType === 'queue_start';
  });
};

const isQueueWorkflowEventType = (eventType: string): boolean =>
  eventType === 'queued' ||
  eventType === 'queue_enter' ||
  eventType === 'queue_update' ||
  eventType === 'queue_start' ||
  eventType === 'queue_finish' ||
  eventType === 'queue_fail';

const resolveAssistantSemanticUserRound = (
  message: ChatRuntimeMessageProjection
): number | null => {
  const turnId = String(message.userTurnId || '').trim();
  const turnRound = turnId.match(/(?:^|:)round:(\d+)(?::|$)/i);
  if (turnRound) return normalizePositiveInteger(turnRound[1]);
  const modelTurnId = String(message.modelTurnId || '').trim();
  const modelRound = modelTurnId.match(/(?:^|:)user:(\d+)(?::|$)/i);
  if (modelRound) return normalizePositiveInteger(modelRound[1]);
  const display = isPlainRecord(message.display) ? message.display : {};
  const raw = isPlainRecord(message.raw) ? message.raw : {};
  for (const value of [
    display.user_round,
    display.userRound,
    display.stream_round,
    display.streamRound,
    raw.user_round,
    raw.userRound,
    raw.stream_round,
    raw.streamRound
  ]) {
    const round = normalizePositiveInteger(value);
    if (round !== null) return round;
  }
  return null;
};

const resolveAssistantModelRound = (
  message: ChatRuntimeMessageProjection
): number | null => {
  const modelTurnId = String(message.modelTurnId || '').trim();
  const match = modelTurnId.match(/(?:^|:)model:(\d+)(?::|$)/i);
  if (!match) return null;
  return normalizePositiveInteger(match[1]);
};

const isSpecialAssistantProjection = (
  message: ChatRuntimeMessageProjection
): boolean => {
  const display = isPlainRecord(message.display) ? message.display : {};
  const raw = isPlainRecord(message.raw) ? message.raw : {};
  return Boolean(
    display.manual_compaction_marker === true ||
      display.manualCompactionMarker === true ||
      raw.manual_compaction_marker === true ||
      raw.manualCompactionMarker === true ||
      display.manual_goal_marker === true ||
      display.manualGoalMarker === true ||
      raw.manual_goal_marker === true ||
      raw.manualGoalMarker === true
  );
};

const mergeVisibleMessageProjection = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): ChatRuntimeMessageProjection => {
  const target = chooseVisibleMergeTarget(session, left, right);
  const source = target === left ? right : left;
  const merged: ChatRuntimeMessageProjection = {
    ...target,
    content: chooseMergedText(target.content, source.content),
    reasoning: chooseMergedText(target.reasoning, source.reasoning),
    status: mergeVisibleMessageStatus(session, target.status, source.status),
    createdSeq: Math.min(target.createdSeq, source.createdSeq),
    updatedSeq: Math.max(target.updatedSeq, source.updatedSeq),
    final: target.final || source.final,
    failed: target.failed || source.failed,
    cancelled: target.cancelled || source.cancelled,
    display: mergeVisibleRecord(target.display, source.display),
    workflowItems: mergeVisibleRecordArray(target.workflowItems, source.workflowItems),
    subagents: mergeVisibleRecordArray(target.subagents, source.subagents),
    raw: target.raw || source.raw
  };
  return merged;
};

const chooseVisibleMergeTarget = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageProjection,
  right: ChatRuntimeMessageProjection
): ChatRuntimeMessageProjection => {
  const leftScore = scoreVisibleMergeTarget(session, left);
  const rightScore = scoreVisibleMergeTarget(session, right);
  if (leftScore !== rightScore) return leftScore > rightScore ? left : right;
  return left.createdSeq <= right.createdSeq ? left : right;
};

const scoreVisibleMergeTarget = (
  session: ChatRuntimeSessionProjection,
  message: ChatRuntimeMessageProjection
): number => {
  let score = 0;
  if (message.content || message.reasoning) score += 8;
  if (resolveAssistantModelRound(message) !== null) score += 5;
  if (hasQueueWorkflowProjection(message) && !message.content && !message.reasoning) score -= 4;
  if (String(message.userTurnId || '').startsWith('queue-turn:')) score -= 3;
  if (isRuntimeMessageActive(message.status)) score += isRuntimeStatusHot(session.runtimeStatus) ? 6 : 1;
  if (message.status === 'final') score += isRuntimeStatusHot(session.runtimeStatus) ? 1 : 6;
  if (message.status === 'failed' || message.status === 'cancelled') score += 4;
  if (message.id.startsWith('local-')) score -= 1;
  return score;
};

const mergeVisibleMessageStatus = (
  session: ChatRuntimeSessionProjection,
  left: ChatRuntimeMessageStatus,
  right: ChatRuntimeMessageStatus
): ChatRuntimeMessageStatus => {
  const hot = isRuntimeStatusHot(session.runtimeStatus);
  const statuses = [left, right];
  if (hot) {
    for (const status of ['tooling', 'streaming', 'waiting_first_output', 'queued', 'placeholder'] as ChatRuntimeMessageStatus[]) {
      if (statuses.includes(status)) return status;
    }
  }
  if (statuses.includes('failed')) return 'failed';
  if (statuses.includes('cancelled')) return 'cancelled';
  if (statuses.includes('final')) return 'final';
  for (const status of ['streaming', 'tooling', 'waiting_first_output', 'queued', 'placeholder'] as ChatRuntimeMessageStatus[]) {
    if (statuses.includes(status)) return status;
  }
  return left;
};

const isRuntimeStatusHot = (status: unknown): boolean =>
  isChatRuntimeBusyStatus(status) || normalizeChatRuntimeStatus(status) === 'queued';

const chooseMergedText = (left: string, right: string): string => {
  if (!left) return right || '';
  if (!right) return left;
  if (right.startsWith(left)) return right;
  if (left.startsWith(right)) return left;
  return right.length > left.length ? right : left;
};

const mergeVisibleRecord = (
  left: unknown,
  right: unknown
): Record<string, unknown> | undefined => {
  if (!isPlainRecord(left)) return isPlainRecord(right) ? { ...right } : undefined;
  if (!isPlainRecord(right)) return { ...left };
  const merged: Record<string, unknown> = { ...left };
  Object.entries(right).forEach(([key, value]) => {
    if (value === undefined) return;
    const current = merged[key];
    if (isPlainRecord(current) && isPlainRecord(value)) {
      merged[key] = mergeVisibleRecord(current, value);
      return;
    }
    if (Array.isArray(current) && Array.isArray(value)) {
      merged[key] = mergeVisibleRecordArray(current, value) || value.slice();
      return;
    }
    if (typeof current === 'number' && typeof value === 'number') {
      merged[key] = Math.max(current, value);
      return;
    }
    if (typeof current === 'boolean' && typeof value === 'boolean') {
      merged[key] = current || value;
      return;
    }
    if (current === undefined || current === null || current === '') {
      merged[key] = value;
      return;
    }
    if (value !== null && value !== '') {
      merged[key] = value;
    }
  });
  return merged;
};

const mergeVisibleRecordArray = (
  left: unknown,
  right: unknown
): Record<string, unknown>[] | undefined => {
  const records = [
    ...(Array.isArray(left) ? left : []).map((record, index) => ({ record, index })),
    ...(Array.isArray(right) ? right : []).map((record, index) => ({ record, index }))
  ].filter((item): item is { record: Record<string, unknown>; index: number } => isPlainRecord(item.record));
  if (records.length === 0) return undefined;
  const merged = new Map<string, Record<string, unknown>>();
  records.forEach(({ record, index }) => {
    const key = resolveVisibleRecordKey(record, index);
    const previous = merged.get(key);
    if (!previous) {
      merged.set(key, { ...record });
      return;
    }
    const incomingIsNewer = normalizeRecordSeq(record) >= normalizeRecordSeq(previous);
    const next = incomingIsNewer
      ? mergeVisibleRecord(previous, record)
      : mergeVisibleRecord(record, previous);
    const status = pickMergedVisibleRecordStatus(previous.status, record.status);
    if (status && next) {
      next.status = status;
    }
    merged.set(key, next || { ...previous, ...record });
  });
  return Array.from(merged.values());
};

const resolveVisibleRecordKey = (
  record: Record<string, unknown>,
  index: number
): string => {
  for (const key of [
    'id',
    'toolCallId',
    'tool_call_id',
    'workflowRef',
    'workflow_ref',
    'runId',
    'run_id',
    'sessionId',
    'session_id',
    'eventId',
    'event_id'
  ]) {
    const value = String(record[key] ?? '').trim();
    if (value) return `${key}:${value}`;
  }
  const semantic = resolveVisibleRecordSemanticKey(record, index);
  if (semantic) return semantic;
  const eventType = String(record.eventType ?? record.event_type ?? record.event ?? '').trim();
  return eventType ? `event:${eventType}:${index}` : `index:${index}`;
};

const resolveVisibleRecordSemanticKey = (
  record: Record<string, unknown>,
  index: number
): string => {
  const eventType = normalizeVisibleRecordIdentityEventType(
    record.eventType ?? record.event_type ?? record.event ?? record.sourceEventType ?? record.source_event_type
  );
  const modelTurnId = String(record.modelTurnId ?? record.model_turn_id ?? '').trim();
  const toolName = normalizeText(
    record.toolName ??
      record.tool_name ??
      record.tool ??
      record.name ??
      record.functionName ??
      record.function_name ??
      record.runtimeName ??
      record.runtime_name
  );
  const title = normalizeText(record.title);
  const kind = normalizeText(record.kind ?? record.type);
  const label = normalizeText(record.label ?? record.name);
  const semanticParts = [
    modelTurnId,
    eventType,
    toolName || title || label || kind,
    String(index)
  ].filter(Boolean);
  return semanticParts.length >= 2 ? `semantic:${semanticParts.join(':')}` : '';
};

const normalizeVisibleRecordIdentityEventType = (value: unknown): string => {
  const eventType = normalizeText(value);
  if (
    eventType === 'tool_call' ||
    eventType === 'tool_result' ||
    eventType === 'tool_call_started' ||
    eventType === 'tool_call_completed' ||
    eventType === 'tool_call_failed'
  ) {
    return 'tool';
  }
  return eventType;
};

const normalizeRecordSeq = (record: Record<string, unknown>): number => {
  const parsed = Number.parseInt(String(record.updatedSeq ?? record.createdSeq ?? ''), 10);
  return Number.isFinite(parsed) ? parsed : 0;
};

const ACTIVE_VISIBLE_RECORD_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);
const SUCCESS_VISIBLE_RECORD_STATUSES = new Set([
  'complete',
  'completed',
  'done',
  'finished',
  'idle',
  'success',
  'succeeded'
]);
const FAILED_VISIBLE_RECORD_STATUSES = new Set([
  'aborted',
  'cancelled',
  'canceled',
  'closed',
  'error',
  'failed',
  'not_found',
  'partial',
  'rejected',
  'timeout'
]);

const pickMergedVisibleRecordStatus = (
  previous: unknown,
  incoming: unknown
): string => {
  const left = normalizeText(previous);
  const right = normalizeText(incoming);
  if (!left) return right;
  if (!right) return left;
  if (FAILED_VISIBLE_RECORD_STATUSES.has(left) || FAILED_VISIBLE_RECORD_STATUSES.has(right)) {
    return FAILED_VISIBLE_RECORD_STATUSES.has(right) ? right : left;
  }
  if (SUCCESS_VISIBLE_RECORD_STATUSES.has(left) || SUCCESS_VISIBLE_RECORD_STATUSES.has(right)) {
    return SUCCESS_VISIBLE_RECORD_STATUSES.has(right) ? right : left;
  }
  if (!ACTIVE_VISIBLE_RECORD_STATUSES.has(left) && ACTIVE_VISIBLE_RECORD_STATUSES.has(right)) return left;
  if (!ACTIVE_VISIBLE_RECORD_STATUSES.has(right) && ACTIVE_VISIBLE_RECORD_STATUSES.has(left)) return right;
  return right;
};

const isPlainRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

const normalizeText = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const buildMessageTurnOrder = (
  session: ChatRuntimeSessionProjection
): Map<string, number> => {
  const orderedTurns = session.userTurns
    .map((turnId, index) => ({
      turnId,
      index,
      semanticOrder: resolveTurnSemanticOrder(turnId),
      createdSeq: resolveTurnCreatedSeq(session, turnId)
    }))
    .sort((left, right) => {
      if (
        left.semanticOrder !== null &&
        right.semanticOrder !== null &&
        left.semanticOrder !== right.semanticOrder
      ) {
        return left.semanticOrder - right.semanticOrder;
      }
      return left.createdSeq - right.createdSeq || left.index - right.index;
    });
  return new Map(orderedTurns.map((item, index) => [item.turnId, index]));
};

const resolveMessageTurnOrder = (
  turnOrder: Map<string, number>,
  message: ChatRuntimeMessageProjection
): number =>
  turnOrder.get(message.userTurnId) ?? Number.MAX_SAFE_INTEGER;

const resolveTurnCreatedSeq = (
  session: ChatRuntimeSessionProjection,
  turnId: string
): number => {
  const turn = session.userTurnById[turnId];
  if (!turn) return Number.MAX_SAFE_INTEGER;
  const seqs = [
    turn.createdSeq,
    ...turn.messageIds.map((messageId) => session.messageById[messageId]?.createdSeq),
    ...turn.modelTurnIds.flatMap((modelTurnId) => {
      const modelTurn = session.modelTurnById[modelTurnId];
      return [
        modelTurn?.createdSeq,
        ...(modelTurn?.messageIds || []).map((messageId) => session.messageById[messageId]?.createdSeq)
      ];
    })
  ].filter((value): value is number => Number.isFinite(value));
  return seqs.length > 0 ? Math.min(...seqs) : Number.MAX_SAFE_INTEGER;
};

const resolveTurnSemanticOrder = (turnId: string): number | null => {
  const text = String(turnId || '').trim();
  if (!text) return null;
  const roundMatch = text.match(/(?:^|:)round:(\d+)(?::|$)/i);
  if (roundMatch) {
    return normalizePositiveInteger(roundMatch[1]);
  }
  const userMatch = text.match(/(?:^|:)user:(\d+)(?::|$)/i);
  if (userMatch) {
    return normalizePositiveInteger(userMatch[1]);
  }
  return null;
};

const normalizePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

export const selectLatestAssistantForTurn = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  userTurnId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const turnId = String(userTurnId ?? '').trim();
  if (!session || !turnId) return null;
  const userTurn = session.userTurnById[turnId];
  if (!userTurn) return null;
  for (let turnIndex = userTurn.modelTurnIds.length - 1; turnIndex >= 0; turnIndex -= 1) {
    const modelTurn = session.modelTurnById[userTurn.modelTurnIds[turnIndex]];
    if (!modelTurn) continue;
    for (let messageIndex = modelTurn.messageIds.length - 1; messageIndex >= 0; messageIndex -= 1) {
      const message = session.messageById[modelTurn.messageIds[messageIndex]];
      if (message?.role === 'assistant') return message;
    }
  }
  return null;
};

export const selectMessageRuntime = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  messageId: unknown
): ChatRuntimeMessageProjection | null => {
  const session = selectChatRuntimeSession(projection, sessionId);
  const key = String(messageId ?? '').trim();
  if (!session || !key) return null;
  return session.messageById[key] || null;
};

export const selectLegacyMessageRuntime = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  message: ChatMessageLike | null | undefined
): ChatRuntimeMessageProjection | null => {
  if (!message) return null;
  const session = selectChatRuntimeSession(projection, sessionId);
  if (!session) return null;
  const explicit = String(message.message_id ?? message.messageId ?? message.id ?? '').trim();
  if (explicit && session.messageById[explicit]) {
    return session.messageById[explicit];
  }
  const eventId = Number.parseInt(String(message.stream_event_id ?? message.streamEventId ?? ''), 10);
  if (Number.isFinite(eventId) && eventId > 0) {
    const byEvent = session.messageById[`legacy-event:${eventId}`];
    if (byEvent) return byEvent;
  }
  return Object.values(session.messageById).find((item) => item.raw === message) || null;
};

export const selectLegacyMessageStatus = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: unknown,
  message: ChatMessageLike | null | undefined
): ChatRuntimeMessageStatus | null =>
  selectLegacyMessageRuntime(projection, sessionId, message)?.status ?? null;

export const isRuntimeMessageActive = (status: ChatRuntimeMessageStatus | null | undefined): boolean =>
  status === 'placeholder' ||
  status === 'queued' ||
  status === 'waiting_first_output' ||
  status === 'streaming' ||
  status === 'tooling';
