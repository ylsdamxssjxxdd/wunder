import type {
  ChatRuntimeApplyResult,
  ChatRuntimeBusyReason,
  ChatRuntimeConnectionState,
  ChatRuntimeEvent,
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageRole,
  ChatRuntimeMessageStatus,
  ChatRuntimeModelTurnProjection,
  ChatRuntimeProjection,
  ChatRuntimeRawMessage,
  ChatRuntimeSubagentProjection,
  ChatRuntimeSessionProjection,
  ChatRuntimeUserTurnProjection,
  ChatRuntimeViolation,
  ChatRuntimeWorkflowItemProjection,
  ChatSessionRuntimeStatus
} from './chatRuntimeTypes';

const DEBUG_EVENT_LIMIT = 300;
const QUARANTINE_LIMIT = 100;
const VIOLATION_LIMIT = 100;
const PENDING_SEQUENTIAL_EVENT_LIMIT = 200;
const SMALL_SEQUENTIAL_GAP_LIMIT = 4;

const BUSY_STATUSES = new Set<ChatSessionRuntimeStatus>([
  'queued',
  'running',
  'waiting_approval',
  'waiting_user_input',
  'finalizing',
  'reconnecting'
]);

const TERMINAL_EVENT_TYPES = new Set([
  'assistant_final',
  'turn_completed',
  'turn_failed',
  'turn_cancelled',
  'session_idle'
]);

const ACTIVE_WORKFLOW_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);
const FAILED_WORKFLOW_STATUSES = new Set([
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
const ACTIVE_SUBAGENT_STATUSES = new Set([
  'accepted',
  'cancelling',
  'in_progress',
  'inprogress',
  'loading',
  'pending',
  'processing',
  'queued',
  'running',
  'started',
  'waiting'
]);
const TOOL_RESULT_EVENT_TYPES = new Set(['tool_call_completed', 'tool_call_failed']);
const SUBAGENT_WORKFLOW_EVENT_TYPES = new Set([
  'subagent_dispatch_start',
  'subagent_dispatch_item_update',
  'subagent_dispatch_finish',
  'subagent_status',
  'subagent_interrupt',
  'subagent_close',
  'subagent_resume',
  'subagent_announce'
]);
const TEAM_WORKFLOW_EVENT_TYPES = new Set([
  'team_start',
  'team_task_dispatch',
  'team_task_update',
  'team_task_result',
  'team_merge',
  'team_finish',
  'team_error'
]);
const SUCCESS_WORKFLOW_STATUSES = new Set([
  'complete',
  'completed',
  'done',
  'finished',
  'idle',
  'success',
  'succeeded'
]);

export const createChatRuntimeProjection = (): ChatRuntimeProjection => ({
  activeSessionId: null,
  sessions: {},
  debugEvents: []
});

export const createChatRuntimeSessionProjection = (
  sessionId: string
): ChatRuntimeSessionProjection => ({
  sessionId,
  agentId: '',
  appliedSeq: 0,
  snapshotSeq: 0,
  localSeq: 0,
  syncRequired: false,
  connectionState: 'connected',
  runtimeStatus: 'not_loaded',
  busyReason: null,
  eventIdIndex: {},
  userTurns: [],
  modelTurns: [],
  messages: [],
  messageById: {},
  userTurnById: {},
  modelTurnById: {},
  invariantViolations: [],
  quarantinedEvents: [],
  pendingSequentialEvents: []
});

export const resolveChatRuntimeSession = (
  projection: ChatRuntimeProjection,
  sessionId: unknown
): ChatRuntimeSessionProjection => {
  const key = normalizeId(sessionId) || '__unknown__';
  if (!projection.sessions[key]) {
    projection.sessions[key] = createChatRuntimeSessionProjection(key);
  }
  return projection.sessions[key];
};

export const normalizeChatRuntimeStatus = (value: unknown): ChatSessionRuntimeStatus => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'idle') return 'idle';
  if (normalized === 'queued' || normalized === 'pending') return 'queued';
  if (normalized === 'running' || normalized === 'resuming') return 'running';
  if (normalized === 'waiting_approval' || normalized === 'awaiting_approval') {
    return 'waiting_approval';
  }
  if (normalized === 'waiting_user_input' || normalized === 'awaiting_user_input') {
    return 'waiting_user_input';
  }
  if (normalized === 'finalizing') return 'finalizing';
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') {
    return 'completed';
  }
  if (normalized === 'failed' || normalized === 'error' || normalized === 'system_error') {
    return normalized === 'system_error' ? 'system_error' : 'failed';
  }
  if (normalized === 'cancelled' || normalized === 'canceled') return 'cancelled';
  if (normalized === 'reconnecting') return 'reconnecting';
  if (normalized === 'offline') return 'offline';
  return 'not_loaded';
};

export const isChatRuntimeBusyStatus = (status: unknown): boolean =>
  BUSY_STATUSES.has(normalizeChatRuntimeStatus(status));

export const applyChatRuntimeEvent = (
  projection: ChatRuntimeProjection,
  rawEvent: ChatRuntimeEvent
): ChatRuntimeApplyResult => {
  const event = normalizeRuntimeEvent(rawEvent);
  const session = resolveChatRuntimeSession(projection, event.sessionId);
  ensureRuntimeSessionCollections(session);
  const beforeSummary = summarizeSession(session);
  const duplicateEventId = event.eventId && session.eventIdIndex[event.eventId];

  if (duplicateEventId) {
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: true,
      quarantined: false,
      sessionId: session.sessionId,
      eventSeq: event.eventSeq,
      reason: 'duplicate_event_id'
    };
  }

  if (event.eventSeq !== null && event.eventSeq <= session.appliedSeq) {
    removePendingSequentialEvent(session, event);
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: true,
      quarantined: false,
      sessionId: session.sessionId,
      eventSeq: event.eventSeq,
      reason: 'stale_event_seq'
    };
  }

  if (event.strict) {
    const quarantineReason = resolveStrictEventQuarantineReason(event);
    if (quarantineReason) {
      quarantineEvent(session, event, quarantineReason);
      appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
      return {
        applied: false,
        ignored: false,
        quarantined: true,
        sessionId: session.sessionId,
        eventSeq: event.eventSeq,
        reason: quarantineReason
      };
    }
  }

  const pendingReason = queueSequentialEventIfNeeded(session, event);
  if (pendingReason) {
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: pendingReason === 'duplicate_pending_event_id',
      quarantined: false,
      pending: pendingReason !== 'duplicate_pending_event_id',
      sessionId: session.sessionId,
      eventSeq: event.eventSeq,
      reason: pendingReason
    };
  }

  const hardGapReason = shouldApplySequentialGapImmediately(session, event) ? 'event_seq_gap' : '';
  applyNormalizedRuntimeEvent(session, event);
  const drained = drainPendingSequentialEvents(session);
  appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
  return {
    applied: true,
    ignored: false,
    quarantined: false,
    drained,
    sessionId: session.sessionId,
    eventSeq: event.eventSeq,
    reason: hardGapReason || undefined
  };
};

type NormalizedRuntimeEvent = ChatRuntimeEvent & {
  type: string;
  source: string;
  strict: boolean;
  sessionId: string;
  agentId: string;
  eventId: string;
  eventSeq: number | null;
  snapshotSeq: number | null;
  userTurnId: string;
  modelTurnId: string;
  messageId: string;
  role: ChatRuntimeMessageRole;
  content: string;
  reasoning: string;
  delta: string;
  reasoningDelta: string;
  runtimeStatus: ChatSessionRuntimeStatus;
  createdAt: string;
  payload: Record<string, unknown>;
};

const applyNormalizedRuntimeEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  if (shouldIgnoreEventForCancelledTurn(session, event)) {
    if (event.eventId) {
      session.eventIdIndex[event.eventId] = true;
    }
    if (event.eventSeq !== null && event.eventSeq > session.appliedSeq) {
      session.appliedSeq = event.eventSeq;
    }
    return;
  }
  if (event.eventId) {
    session.eventIdIndex[event.eventId] = true;
  }
  if (event.agentId) {
    session.agentId = event.agentId;
  }

  switch (event.type) {
    case 'connection_state':
      applyConnectionState(session, event);
      break;
    case 'client_message_submitted':
    case 'user_message_created':
      applyUserMessageCreated(session, event);
      break;
    case 'assistant_message_created':
      applyAssistantMessageCreated(session, event);
      break;
    case 'assistant_delta':
      applyAssistantDelta(session, event, 'content');
      if (event.reasoningDelta || event.reasoning) {
        applyAssistantDelta(session, event, 'reasoning');
      }
      break;
    case 'assistant_reasoning_delta':
      applyAssistantDelta(session, event, 'reasoning');
      break;
    case 'assistant_final':
      applyAssistantFinal(session, event);
      break;
    case 'tool_call_started':
    case 'tool_call_delta':
      applyToolActivity(session, event, false);
      break;
    case 'tool_call_completed':
      applyToolActivity(session, event, true);
      break;
    case 'tool_call_failed':
      applyToolFailed(session, event);
      break;
    case 'workflow_event':
      applyWorkflowEvent(session, event);
      break;
    case 'turn_completed':
      applyTurnTerminal(session, event, 'completed');
      break;
    case 'turn_failed':
      applyTurnTerminal(session, event, 'failed');
      break;
    case 'turn_cancelled':
      applyTurnTerminal(session, event, 'cancelled');
      break;
    case 'session_idle':
      applySessionIdle(session, event);
      break;
    case 'session_runtime':
      applySessionRuntime(session, event);
      break;
    case 'session_snapshot':
      applySessionSnapshot(session, event);
      break;
    case 'legacy_messages_reconciled':
      applyLegacyMessagesReconciled(session, event);
      break;
    case 'sync_required':
      session.syncRequired = true;
      setSessionBusy(session, 'reconnecting', 'syncing');
      break;
    default:
      if (TERMINAL_EVENT_TYPES.has(event.type)) {
        applySessionIdle(session, event);
      }
      break;
  }

  if (event.eventSeq !== null && event.eventSeq > session.appliedSeq) {
    session.appliedSeq = event.eventSeq;
  }
  deriveSessionRuntime(session);
  validateSessionInvariants(session, event);
};

const ensureRuntimeSessionCollections = (session: ChatRuntimeSessionProjection): void => {
  if (!Array.isArray(session.pendingSequentialEvents)) {
    session.pendingSequentialEvents = [];
  }
};

const shouldBufferSequentialEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): boolean => {
  if (!event.strict || event.eventSeq === null) return false;
  if (event.source === 'legacy' || event.source === 'snapshot') return false;
  if (session.appliedSeq <= 0) return false;
  return event.eventSeq > session.appliedSeq + 1;
};

const shouldApplySequentialGapImmediately = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): boolean => {
  if (!shouldBufferSequentialEvent(session, event)) return false;
  return Number(event.eventSeq) - session.appliedSeq - 1 > SMALL_SEQUENTIAL_GAP_LIMIT;
};

const queueSequentialEventIfNeeded = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): string => {
  if (!shouldBufferSequentialEvent(session, event)) return '';
  ensureRuntimeSessionCollections(session);
  if (shouldApplySequentialGapImmediately(session, event)) {
    session.syncRequired = true;
    pushViolation(session, {
      code: 'event_seq_gap',
      message: 'event_seq advanced beyond the bounded reorder buffer',
      eventSeq: event.eventSeq,
      eventType: event.type
    });
    return '';
  }
  if (
    event.eventId &&
    session.pendingSequentialEvents.some((pending) => pending.eventId === event.eventId)
  ) {
    return 'duplicate_pending_event_id';
  }
  if (session.pendingSequentialEvents.length >= PENDING_SEQUENTIAL_EVENT_LIMIT) {
    session.pendingSequentialEvents.splice(0);
    session.syncRequired = true;
    pushViolation(session, {
      code: 'event_seq_buffer_overflow',
      message: 'pending sequential runtime events exceeded the bounded buffer',
      eventSeq: event.eventSeq,
      eventType: event.type
    });
    return '';
  }
  session.pendingSequentialEvents.push({
    eventSeq: event.eventSeq,
    eventId: event.eventId,
    eventType: event.type,
    receivedAt: Date.now(),
    event
  });
  session.pendingSequentialEvents.sort((left, right) =>
    left.eventSeq - right.eventSeq || left.receivedAt - right.receivedAt
  );
  session.syncRequired = true;
  pushViolation(session, {
    code: 'event_seq_gap_buffered',
    message: 'runtime event arrived ahead of the next expected sequence and was buffered',
    eventSeq: event.eventSeq,
    eventType: event.type
  });
  return 'pending_event_seq_gap';
};

const drainPendingSequentialEvents = (session: ChatRuntimeSessionProjection): number => {
  ensureRuntimeSessionCollections(session);
  let drained = 0;
  while (session.pendingSequentialEvents.length > 0) {
    session.pendingSequentialEvents.sort((left, right) =>
      left.eventSeq - right.eventSeq || left.receivedAt - right.receivedAt
    );
    const staleIndex = session.pendingSequentialEvents.findIndex(
      (pending) => pending.eventSeq <= session.appliedSeq
    );
    if (staleIndex >= 0) {
      session.pendingSequentialEvents.splice(staleIndex, 1);
      continue;
    }
    const nextIndex = session.pendingSequentialEvents.findIndex(
      (pending) => pending.eventSeq === session.appliedSeq + 1
    );
    if (nextIndex < 0) break;
    const pending = session.pendingSequentialEvents.splice(nextIndex, 1)[0];
    const event = normalizeRuntimeEvent(pending.event);
    if (event.eventId && session.eventIdIndex[event.eventId]) {
      continue;
    }
    applyNormalizedRuntimeEvent(session, event);
    drained += 1;
  }
  if (
    session.pendingSequentialEvents.length === 0 &&
    !session.invariantViolations.some((violation) =>
      violation.code === 'event_seq_gap' ||
      violation.code === 'event_seq_buffer_overflow'
    )
  ) {
    session.syncRequired = false;
  }
  return drained;
};

const removePendingSequentialEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): boolean => {
  ensureRuntimeSessionCollections(session);
  const index = session.pendingSequentialEvents.findIndex((pending) =>
    (event.eventId && pending.eventId === event.eventId) ||
    pending.eventSeq === event.eventSeq
  );
  if (index < 0) return false;
  session.pendingSequentialEvents.splice(index, 1);
  return true;
};

const normalizeRuntimeEvent = (event: ChatRuntimeEvent): NormalizedRuntimeEvent => {
  const payload = asRecord(event.payload);
  const eventType = normalizeText(
    event.event_type || payload.event_type || payload.event || payload.type
  );
  const source = normalizeText(event.source || payload.source || 'ws') || 'ws';
  const sessionId = normalizeId(event.session_id ?? payload.session_id ?? payload.sessionId);
  const snapshotSeq = normalizeSeq(event.snapshot_seq ?? payload.snapshot_seq ?? payload.snapshotSeq);
  return {
    ...event,
    type: eventType,
    source,
    strict: event.strict === true || source === 'ws' || source === 'test',
    sessionId,
    agentId: normalizeId(event.agent_id ?? payload.agent_id ?? payload.agentId),
    eventId: normalizeId(event.event_id ?? payload.event_id ?? payload.id),
    eventSeq: normalizeSeq(event.event_seq ?? payload.event_seq ?? payload.eventSeq),
    snapshotSeq,
    userTurnId: normalizeId(event.user_turn_id ?? payload.user_turn_id ?? payload.userTurnId),
    modelTurnId: normalizeId(event.model_turn_id ?? payload.model_turn_id ?? payload.modelTurnId),
    messageId: normalizeId(event.message_id ?? payload.message_id ?? payload.messageId),
    role: normalizeRole(event.role ?? payload.role),
    content: String(event.content ?? payload.content ?? payload.message ?? ''),
    reasoning: String(event.reasoning ?? payload.reasoning ?? ''),
    delta: String(event.delta ?? payload.delta ?? payload.content_delta ?? ''),
    reasoningDelta: String(
      event.reasoning_delta ?? payload.reasoning_delta ?? payload.think_delta ?? ''
    ),
    runtimeStatus: normalizeChatRuntimeStatus(
      event.runtime_status ?? payload.runtime_status ?? payload.thread_status ?? payload.status
    ),
    createdAt: normalizeCreatedAt(event.created_at ?? payload.created_at ?? payload.createdAt),
    payload
  };
};

const resolveStrictEventQuarantineReason = (event: NormalizedRuntimeEvent): string => {
  if (!event.sessionId) return 'missing_session_id';
  if (!event.eventId) return 'missing_event_id';
  if (event.eventSeq === null) return 'missing_event_seq';
  if (
    (event.type === 'user_message_created' || event.type === 'client_message_submitted') &&
    (!event.userTurnId || !event.messageId)
  ) {
    return 'missing_user_turn_or_message_id';
  }
  if (
    (
      event.type === 'assistant_message_created' ||
      event.type === 'assistant_delta' ||
      event.type === 'assistant_reasoning_delta' ||
      event.type === 'assistant_final' ||
      event.type === 'tool_call_started' ||
      event.type === 'tool_call_delta' ||
      event.type === 'tool_call_completed' ||
      event.type === 'tool_call_failed' ||
      event.type === 'workflow_event'
    ) &&
    (!event.modelTurnId || !event.messageId)
  ) {
    return 'missing_model_turn_or_message_id';
  }
  if (
    (event.type === 'turn_completed' || event.type === 'turn_failed' || event.type === 'turn_cancelled') &&
    !event.modelTurnId
  ) {
    return 'missing_model_turn_id';
  }
  return '';
};

const applyConnectionState = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const state = normalizeConnectionState(event.payload.state ?? event.payload.connection_state);
  session.connectionState = state;
  if (state === 'reconnecting') {
    setSessionBusy(session, 'reconnecting', 'reconnecting');
  } else if (state === 'offline') {
    session.runtimeStatus = 'offline';
    session.busyReason = null;
  } else if (session.runtimeStatus === 'reconnecting' || session.runtimeStatus === 'offline') {
    session.runtimeStatus = hasActiveMessage(session) ? 'running' : 'idle';
  }
};

const applyUserMessageCreated = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const turn = ensureUserTurn(session, event.userTurnId, event.eventSeq);
  turn.status = event.type === 'client_message_submitted' ? 'created' : 'accepted';
  const messageId = resolveUserMessageIdForTurn(session, turn, event.messageId, event);
  const message = ensureMessage(session, {
    id: messageId,
    role: 'user',
    createdSeq: event.eventSeq,
    createdAt: event.createdAt,
    userTurnId: turn.id,
    modelTurnId: ''
  });
  message.content = event.content || message.content;
  message.status = 'final';
  addUnique(turn.messageIds, message.id);
  addUnique(session.messages, message.id);
  pruneUserTurnUserMessages(session, turn, message.id);
  setSessionBusy(session, 'running', 'waiting_first_output');
};

const applyAssistantMessageCreated = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  modelTurn.status = 'waiting_first_output';
  const message = ensureAssistantMessageForModelTurn(session, event, 'waiting_first_output');
  addUnique(modelTurn.messageIds, message.id);
  setSessionBusy(session, 'running', 'waiting_first_output');
};

const applyAssistantDelta = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  target: 'content' | 'reasoning'
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  modelTurn.status = 'streaming';
  const message = ensureAssistantMessageForModelTurn(session, event, 'streaming');
  if (target === 'content') {
    message.content += event.delta || event.content;
  } else {
    message.reasoning += event.reasoningDelta || event.reasoning;
  }
  message.status = 'streaming';
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  setSessionBusy(session, 'running', 'streaming');
};

const applyAssistantFinal = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const existingFinalId = modelTurn.finalMessageId;
  const finalEvent = existingFinalId && existingFinalId !== event.messageId
    ? { ...event, messageId: existingFinalId }
    : event;
  const message = ensureAssistantMessageForModelTurn(session, finalEvent, 'final');
  if (event.content) {
    message.content = event.content;
  }
  if (event.reasoning) {
    message.reasoning = event.reasoning;
  }
  message.status = 'final';
  message.final = true;
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = 'completed';
  modelTurn.finalMessageId = message.id;
  const userTurn = session.userTurnById[modelTurn.userTurnId];
  if (userTurn) {
    userTurn.status = 'completed';
  }
  session.runtimeStatus = 'completed';
  session.busyReason = null;
};

const applyToolActivity = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  completed: boolean
): void => {
  const sourceType = normalizeText(event.payload.source_event_type);
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(
    session,
    event,
    completed ? 'streaming' : 'tooling'
  );
  upsertToolWorkflowItem(message, event, completed ? 'completed' : 'loading');
  message.status = completed ? 'streaming' : 'tooling';
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = completed ? 'streaming' : 'tool_running';
  if (sourceType === 'approval_request') {
    setSessionBusy(session, 'waiting_approval', 'waiting_approval');
  } else if (completed && (
    sourceType === 'approval_result' ||
    sourceType === 'approval_resolved'
  )) {
    setSessionBusy(session, 'running', 'streaming');
  } else if (!completed) {
    setSessionBusy(session, 'running', 'tool_running');
  }
};

const applyToolFailed = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(session, event, 'failed');
  upsertToolWorkflowItem(message, event, 'failed');
  message.status = 'failed';
  message.failed = true;
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = 'failed';
  session.runtimeStatus = 'failed';
  session.busyReason = null;
};

const applyWorkflowEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const sourceType = normalizeText(event.payload.source_event_type);
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(session, event, 'tooling');
  const status = resolveProjectedWorkflowStatus(sourceType, event.payload);
  upsertProjectedWorkflowEventItem(message, event, sourceType, status);
  if (SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    upsertProjectedSubagents(message, event, sourceType, status);
  }
  message.status = status === 'failed' ? 'failed' : status === 'completed' ? 'streaming' : 'tooling';
  message.failed = message.failed || status === 'failed';
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = status === 'failed' ? 'failed' : status === 'completed' ? 'streaming' : 'tool_running';
  if (status === 'failed') {
    session.runtimeStatus = 'failed';
    session.busyReason = null;
  } else if (status !== 'completed') {
    setSessionBusy(session, 'running', 'tool_running');
  }
};

const applyTurnTerminal = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  terminal: 'completed' | 'failed' | 'cancelled'
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  modelTurn.status = terminal;
  modelTurn.messageIds.forEach((messageId) => {
    const message = session.messageById[messageId];
    if (!message) return;
    if (terminal === 'completed' && message.status !== 'failed' && message.status !== 'cancelled') {
      message.status = 'final';
      message.final = true;
      settleProjectedWorkflowItems(message, 'completed');
      modelTurn.finalMessageId = modelTurn.finalMessageId || message.id;
    } else if (terminal === 'failed') {
      message.status = 'failed';
      message.failed = true;
      settleProjectedWorkflowItems(message, 'failed');
    } else if (terminal === 'cancelled') {
      message.status = 'cancelled';
      message.cancelled = true;
      settleProjectedWorkflowItems(message, 'failed');
    }
  });
  const userTurn = session.userTurnById[modelTurn.userTurnId];
  if (userTurn) {
    userTurn.status = terminal === 'completed' ? 'completed' : terminal;
  }
  if (!hasActiveMessage(session)) {
    session.runtimeStatus = terminal === 'completed' ? 'completed' : terminal;
    session.busyReason = null;
  }
};

const applySessionIdle = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  Object.values(session.modelTurnById).forEach((turn) => {
    if (turn.status === 'created' || turn.status === 'waiting_first_output' || turn.status === 'streaming' || turn.status === 'tool_running' || turn.status === 'finalizing') {
      turn.status = 'completed';
    }
  });
  Object.values(session.messageById).forEach((message) => {
    if (message.role !== 'assistant') return;
    if (message.status === 'placeholder' || message.status === 'waiting_first_output' || message.status === 'streaming' || message.status === 'tooling') {
      message.status = 'final';
      message.final = true;
      settleProjectedWorkflowItems(message, 'completed');
      message.updatedSeq = event.eventSeq ?? message.updatedSeq;
    }
  });
  session.runtimeStatus = 'idle';
  session.busyReason = null;
};

const applySessionRuntime = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const explicitStatus = event.runtimeStatus;
  if (explicitStatus === 'not_loaded') {
    deriveSessionRuntime(session);
    return;
  }
  if (explicitStatus === 'completed') {
    session.runtimeStatus = hasActiveMessage(session) ? 'running' : 'idle';
    session.busyReason = hasActiveMessage(session) ? (session.busyReason || 'streaming') : null;
    return;
  }
  if (explicitStatus === 'failed' || explicitStatus === 'cancelled' || explicitStatus === 'system_error') {
    session.runtimeStatus = explicitStatus;
    session.busyReason = null;
    return;
  }
  if (isChatRuntimeBusyStatus(explicitStatus)) {
    setSessionBusy(session, explicitStatus, resolveBusyReasonForStatus(explicitStatus));
    return;
  }
  if (explicitStatus === 'idle') {
    session.runtimeStatus = hasActiveMessage(session) ? 'running' : 'idle';
    session.busyReason = hasActiveMessage(session) ? (session.busyReason || 'streaming') : null;
  }
};

const applySessionSnapshot = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const snapshotSeq = event.snapshotSeq ?? event.eventSeq ?? 0;
  if (snapshotSeq > session.snapshotSeq) {
    session.snapshotSeq = snapshotSeq;
  }
  const messages = Array.isArray(event.messages)
    ? event.messages
    : Array.isArray(event.payload.transcript)
      ? event.payload.transcript as ChatRuntimeRawMessage[]
    : Array.isArray(event.payload.messages)
      ? event.payload.messages as ChatRuntimeRawMessage[]
      : [];
  if (isCanonicalTranscript(messages)) {
    applyCanonicalTranscriptSnapshot(session, messages, snapshotSeq);
    applySessionRuntime(session, {
      ...event,
      runtimeStatus: normalizeChatRuntimeStatus(event.payload.runtime_status ?? event.payload.status)
    });
    return;
  }
  mergeLegacyMessages(session, messages, {
    snapshotSeq,
    replaceExistingAtOrBelowSeq: true,
    authoritative: normalizeFlag(event.authoritative ?? event.payload.authoritative ?? event.prune_missing ?? event.payload.prune_missing)
  });
  applySessionRuntime(session, {
    ...event,
    runtimeStatus: normalizeChatRuntimeStatus(event.payload.runtime_status ?? event.payload.status)
  });
};

const applyLegacyMessagesReconciled = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const messages = Array.isArray(event.messages)
    ? event.messages
    : Array.isArray(event.payload.transcript)
      ? event.payload.transcript as ChatRuntimeRawMessage[]
    : Array.isArray(event.payload.messages)
      ? event.payload.messages as ChatRuntimeRawMessage[]
      : [];
  const reconcileSeq = event.eventSeq ?? nextLocalSeq(session);
  const loading = normalizeFlag(event.loading ?? event.payload.loading);
  const running = normalizeFlag(event.running ?? event.payload.running);
  if (isCanonicalTranscript(messages)) {
    applyCanonicalTranscriptSnapshot(session, messages, reconcileSeq);
    if (loading || running || hasActiveLegacyRuntime(messages)) {
      setSessionBusy(session, 'running', resolveLegacyBusyReason(messages));
    } else {
      settleLegacyActiveMessages(session);
      session.runtimeStatus = 'idle';
      session.busyReason = null;
      deriveSessionRuntime(session);
    }
    return;
  }
  const authoritative =
    normalizeFlag(event.authoritative ?? event.payload.authoritative ?? event.prune_missing ?? event.payload.prune_missing) &&
    !loading &&
    !running &&
    !hasActiveLegacyRuntime(messages);
  mergeLegacyMessages(session, messages, {
    snapshotSeq: reconcileSeq,
    replaceExistingAtOrBelowSeq: true,
    authoritative
  });
  if (loading || running || hasActiveLegacyRuntime(messages)) {
    setSessionBusy(session, 'running', resolveLegacyBusyReason(messages));
  } else {
    settleLegacyActiveMessages(session);
    session.runtimeStatus = 'idle';
    session.busyReason = null;
    deriveSessionRuntime(session);
  }
};

const settleLegacyActiveMessages = (session: ChatRuntimeSessionProjection): void => {
  Object.values(session.messageById).forEach((message) => {
    if (message.role !== 'assistant') return;
    if (!isActiveMessageStatus(message.status)) return;
    message.status = 'final';
    message.final = true;
    settleProjectedWorkflowItems(message, 'completed');
  });
  Object.values(session.modelTurnById).forEach((turn) => {
    if (
      turn.status === 'created' ||
      turn.status === 'waiting_first_output' ||
      turn.status === 'streaming' ||
      turn.status === 'tool_running' ||
      turn.status === 'finalizing'
    ) {
      turn.status = 'completed';
    }
  });
};

const mergeLegacyMessages = (
  session: ChatRuntimeSessionProjection,
  messages: ChatRuntimeRawMessage[],
  options: { snapshotSeq: number; replaceExistingAtOrBelowSeq: boolean; authoritative?: boolean }
): void => {
  const plans = buildLegacyMessagePlans(session, messages, options.snapshotSeq);
  const assistantBatchMessageIds = new Set<string>();
  const authoritativeMessageIds = new Set<string>();
  const authoritativeMessageOrder: string[] = [];
  plans.forEach((plan) => {
    const { raw, role, id, status, userTurnId, modelTurnId, createdSeq } = plan;
    if (role === 'user') {
      const turn = ensureUserTurn(session, userTurnId, createdSeq);
      const messageId = resolveUserMessageIdForTurn(session, turn, id, raw);
      const existed = Boolean(session.messageById[messageId]);
      const message = ensureMessage(session, {
        id: messageId,
        role,
        createdSeq,
        createdAt: normalizeCreatedAt(raw.created_at ?? raw.createdAt),
        userTurnId: turn.id,
        modelTurnId: ''
      });
      if (existed === false || message.id !== id || shouldReplaceSnapshotMessage(message, options.snapshotSeq, options.replaceExistingAtOrBelowSeq)) {
        patchMessageFromRaw(message, raw, status, options.snapshotSeq);
      }
      message.legacyKey = id;
      message.raw = raw;
      addUnique(turn.messageIds, message.id);
      addUnique(session.messages, message.id);
      pruneUserTurnUserMessages(session, turn, message.id);
      authoritativeMessageIds.add(message.id);
      authoritativeMessageOrder.push(message.id);
      return;
    }
    const modelTurn = ensureModelTurn(
      session,
      modelTurnId,
      userTurnId,
      createdSeq
    );
    const messageId = resolveAssistantMessageIdForModelTurn(session, modelTurn, id);
    const seenInBatch = assistantBatchMessageIds.has(messageId);
    const existingMessage = session.messageById[messageId];
    const existed = Boolean(existingMessage);
    const foldedIntoExisting = existed && messageId !== id;
    const canReplaceSnapshot = existingMessage
      ? shouldReplaceSnapshotMessage(
          existingMessage,
          options.snapshotSeq,
          options.replaceExistingAtOrBelowSeq
        )
      : false;
    const messageUserTurnId = modelTurn.userTurnId || userTurnId;
    const message = ensureMessage(session, {
      id: messageId,
      role,
      createdSeq,
      createdAt: normalizeCreatedAt(raw.created_at ?? raw.createdAt),
      userTurnId: messageUserTurnId,
      modelTurnId: modelTurn.id
    });
    if (seenInBatch || (foldedIntoExisting && canReplaceSnapshot && shouldMergeFoldedLegacyMessage(raw, status))) {
      mergeMessageFromRaw(message, raw, status, options.snapshotSeq);
    } else if (existed === false || canReplaceSnapshot || (foldedIntoExisting && canReplaceSnapshot)) {
      patchMessageFromRaw(message, raw, status, options.snapshotSeq);
    }
    message.legacyKey = id;
    message.raw = raw;
    addUnique(modelTurn.messageIds, message.id);
    if (status === 'final') {
      modelTurn.finalMessageId = modelTurn.finalMessageId || message.id;
    }
    addUnique(session.messages, message.id);
    pruneModelTurnAssistantMessages(session, modelTurn, message.id);
    assistantBatchMessageIds.add(message.id);
    authoritativeMessageIds.add(message.id);
    authoritativeMessageOrder.push(message.id);
  });
  if (options.authoritative === true) {
    pruneProjectionToAuthoritativeMessages(session, authoritativeMessageIds, authoritativeMessageOrder);
  }
};

const pruneProjectionToAuthoritativeMessages = (
  session: ChatRuntimeSessionProjection,
  keepMessageIds: Set<string>,
  orderedMessageIds: string[]
): void => {
  const staleMessageIds = Object.keys(session.messageById)
    .filter((messageId) => !keepMessageIds.has(messageId));
  const staleSet = new Set(staleMessageIds);
  staleMessageIds.forEach((messageId) => {
    delete session.messageById[messageId];
  });
  session.messages = session.messages.filter((messageId) =>
    keepMessageIds.has(messageId) && Boolean(session.messageById[messageId])
  );
  const orderIndex = new Map(orderedMessageIds.map((messageId, index) => [messageId, index]));
  session.messages = [...new Set(session.messages)].sort((left, right) =>
    (orderIndex.get(left) ?? Number.MAX_SAFE_INTEGER) -
      (orderIndex.get(right) ?? Number.MAX_SAFE_INTEGER)
  );
  Object.values(session.userTurnById).forEach((turn) => {
    turn.messageIds = turn.messageIds.filter((messageId) => keepMessageIds.has(messageId));
    turn.modelTurnIds = turn.modelTurnIds.filter((modelTurnId) => {
      const modelTurn = session.modelTurnById[modelTurnId];
      if (!modelTurn) return false;
      modelTurn.messageIds = modelTurn.messageIds.filter((messageId) => keepMessageIds.has(messageId));
      if (modelTurn.finalMessageId && staleSet.has(modelTurn.finalMessageId)) {
        modelTurn.finalMessageId = modelTurn.messageIds.find((messageId) =>
          session.messageById[messageId]?.role === 'assistant' &&
          session.messageById[messageId]?.final
        ) || '';
      }
      return modelTurn.messageIds.length > 0;
    });
  });
  session.modelTurns = session.modelTurns.filter((modelTurnId) => {
    const modelTurn = session.modelTurnById[modelTurnId];
    if (!modelTurn || modelTurn.messageIds.length === 0) {
      delete session.modelTurnById[modelTurnId];
      return false;
    }
    return true;
  });
  session.userTurns = session.userTurns.filter((turnId) => {
    const turn = session.userTurnById[turnId];
    if (!turn || (turn.messageIds.length === 0 && turn.modelTurnIds.length === 0)) {
      delete session.userTurnById[turnId];
      return false;
    }
    return true;
  });
  session.userTurns = [...session.userTurns]
    .filter((turnId) => Boolean(session.userTurnById[turnId]))
    .sort((leftId, rightId) =>
      resolveUserTurnAuthoritativeOrder(session, leftId, orderIndex) -
      resolveUserTurnAuthoritativeOrder(session, rightId, orderIndex)
    );
  session.modelTurns = [...session.modelTurns]
    .filter((turnId) => Boolean(session.modelTurnById[turnId]))
    .sort((leftId, rightId) =>
      resolveModelTurnAuthoritativeOrder(session, leftId, orderIndex) -
      resolveModelTurnAuthoritativeOrder(session, rightId, orderIndex)
    );
};

const resolveUserTurnAuthoritativeOrder = (
  session: ChatRuntimeSessionProjection,
  turnId: string,
  orderIndex: Map<string, number>
): number => {
  const turn = session.userTurnById[turnId];
  if (!turn) return Number.MAX_SAFE_INTEGER;
  const ids = [
    ...turn.messageIds,
    ...turn.modelTurnIds.flatMap((modelTurnId) => session.modelTurnById[modelTurnId]?.messageIds || [])
  ];
  const indexes = ids
    .map((messageId) => orderIndex.get(messageId) ?? Number.MAX_SAFE_INTEGER)
    .filter((index) => index >= 0);
  return indexes.length > 0 ? Math.min(...indexes) : Number.MAX_SAFE_INTEGER;
};

const resolveModelTurnAuthoritativeOrder = (
  session: ChatRuntimeSessionProjection,
  turnId: string,
  orderIndex: Map<string, number>
): number => {
  const turn = session.modelTurnById[turnId];
  if (!turn) return Number.MAX_SAFE_INTEGER;
  const indexes = turn.messageIds
    .map((messageId) => orderIndex.get(messageId) ?? Number.MAX_SAFE_INTEGER)
    .filter((index) => index >= 0);
  if (indexes.length > 0) return Math.min(...indexes);
  const fallback = orderIndex.get(turn.finalMessageId);
  return fallback ?? Number.MAX_SAFE_INTEGER;
};

type LegacyMessagePlan = {
  raw: ChatRuntimeRawMessage;
  index: number;
  role: 'user' | 'assistant';
  id: string;
  status: ChatRuntimeMessageStatus;
  streamRound: number | null;
  userTurnId: string;
  userTurnBinding: LegacyUserTurnBindingStrength;
  userTurnBindingSource: LegacyUserTurnBindingSource;
  modelTurnId: string;
  createdAtMs: number | null;
  createdSeq: number;
  turnOrder: number;
};

type LegacyUserTurnBindingStrength = 'none' | 'weak' | 'strong';

type LegacyUserTurnBindingSource =
  | 'none'
  | 'explicit'
  | 'stream_round'
  | 'message'
  | 'timestamp'
  | 'adjacent_previous'
  | 'nearest_existing';

type LegacyUserTurnResolution = {
  userTurnId: string;
  strength: LegacyUserTurnBindingStrength;
  source: LegacyUserTurnBindingSource;
};

const isCanonicalTranscript = (messages: ChatRuntimeRawMessage[]): boolean =>
  Array.isArray(messages) &&
  messages.filter((message) => !isSyntheticGreetingRawMessage(message)).length > 0 &&
  messages.every((message) => isSyntheticGreetingRawMessage(message) || isCanonicalTranscriptMessage(message));

const isSyntheticGreetingRawMessage = (message: ChatRuntimeRawMessage): boolean =>
  Boolean(
    message &&
    typeof message === 'object' &&
    (message.isGreeting === true || message.is_greeting === true)
  );

const isCanonicalTranscriptMessage = (message: ChatRuntimeRawMessage): boolean => {
  if (!message || typeof message !== 'object') return false;
  const role = normalizeRole(message.role);
  if (role !== 'user' && role !== 'assistant') return false;
  return Boolean(
    normalizeId(message.message_id ?? message.messageId ?? message.id) &&
    normalizeId(message.user_turn_id ?? message.userTurnId) &&
    normalizeSeq(message.turn_index ?? message.turnIndex) !== null
  );
};

const applyCanonicalTranscriptSnapshot = (
  session: ChatRuntimeSessionProjection,
  messages: ChatRuntimeRawMessage[],
  snapshotSeq: number
): void => {
  const plans = messages
    .filter((raw) => !isSyntheticGreetingRawMessage(raw))
    .map((raw, index) => buildCanonicalTranscriptPlan(raw, index, snapshotSeq))
    .filter((plan): plan is LegacyMessagePlan => Boolean(plan))
    .sort((left, right) => left.createdSeq - right.createdSeq || left.index - right.index);
  const keepMessageIds = new Set<string>();
  const keepUserTurnIds = new Set<string>();
  const keepModelTurnIds = new Set<string>();

  plans.forEach((plan) => {
    const userTurn = ensureUserTurn(session, plan.userTurnId, plan.createdSeq);
    keepUserTurnIds.add(userTurn.id);
    if (plan.role === 'user') {
      const message = ensureMessage(session, {
        id: plan.id,
        role: 'user',
        createdSeq: plan.createdSeq,
        createdAt: resolveCanonicalCreatedAt(plan.raw),
        userTurnId: userTurn.id,
        modelTurnId: ''
      });
      patchMessageFromRaw(message, plan.raw, plan.status, plan.createdSeq);
      message.userTurnId = userTurn.id;
      message.modelTurnId = '';
      addUnique(userTurn.messageIds, message.id);
      keepMessageIds.add(message.id);
      return;
    }

    const modelTurn = ensureCanonicalModelTurn(session, plan.modelTurnId, userTurn.id, plan.createdSeq);
    keepModelTurnIds.add(modelTurn.id);
    const message = ensureMessage(session, {
      id: plan.id,
      role: 'assistant',
      createdSeq: plan.createdSeq,
      createdAt: resolveCanonicalCreatedAt(plan.raw),
      userTurnId: userTurn.id,
      modelTurnId: modelTurn.id
    });
    patchMessageFromRaw(message, plan.raw, plan.status, plan.createdSeq);
    message.userTurnId = userTurn.id;
    message.modelTurnId = modelTurn.id;
    addUnique(modelTurn.messageIds, message.id);
    modelTurn.finalMessageId = message.id;
    modelTurn.status = resolveCanonicalModelTurnStatus(plan.status);
    addUnique(userTurn.modelTurnIds, modelTurn.id);
    keepMessageIds.add(message.id);
  });

  Object.keys(session.messageById).forEach((messageId) => {
    if (!keepMessageIds.has(messageId)) {
      delete session.messageById[messageId];
    }
  });
  Object.keys(session.modelTurnById).forEach((modelTurnId) => {
    if (!keepModelTurnIds.has(modelTurnId)) {
      delete session.modelTurnById[modelTurnId];
    }
  });
  Object.keys(session.userTurnById).forEach((userTurnId) => {
    if (!keepUserTurnIds.has(userTurnId)) {
      delete session.userTurnById[userTurnId];
    }
  });

  session.messages = plans.map((plan) => plan.id).filter((id) => keepMessageIds.has(id));
  Object.values(session.userTurnById).forEach((turn) => {
    turn.messageIds = turn.messageIds.filter((messageId) => keepMessageIds.has(messageId));
    turn.modelTurnIds = turn.modelTurnIds.filter((modelTurnId) => keepModelTurnIds.has(modelTurnId));
    turn.status = resolveCanonicalUserTurnStatus(session, turn);
  });
  Object.values(session.modelTurnById).forEach((turn) => {
    turn.messageIds = turn.messageIds.filter((messageId) => keepMessageIds.has(messageId));
  });
  const orderIndex = new Map(session.messages.map((messageId, index) => [messageId, index]));
  session.userTurns = Object.keys(session.userTurnById).sort((left, right) =>
    resolveUserTurnAuthoritativeOrder(session, left, orderIndex) -
    resolveUserTurnAuthoritativeOrder(session, right, orderIndex)
  );
  session.modelTurns = Object.keys(session.modelTurnById).sort((left, right) =>
    resolveModelTurnAuthoritativeOrder(session, left, orderIndex) -
    resolveModelTurnAuthoritativeOrder(session, right, orderIndex)
  );
  session.snapshotSeq = Math.max(session.snapshotSeq, snapshotSeq);
  session.syncRequired = false;
};

const buildCanonicalTranscriptPlan = (
  raw: ChatRuntimeRawMessage,
  index: number,
  snapshotSeq: number
): LegacyMessagePlan | null => {
  const role = normalizeRole(raw.role);
  if (role !== 'user' && role !== 'assistant') return null;
  const userTurnId = normalizeId(raw.user_turn_id ?? raw.userTurnId);
  const id = normalizeId(raw.message_id ?? raw.messageId ?? raw.id);
  if (!userTurnId || !id) return null;
  const explicitModelTurnId = normalizeId(raw.model_turn_id ?? raw.modelTurnId);
  const modelTurnId = role === 'assistant'
    ? explicitModelTurnId || `model-turn:${userTurnId}:message:${id}`
    : '';
  return {
    raw,
    index,
    role,
    id,
    status: resolveLegacyMessageStatus(raw),
    streamRound: normalizeSeq(raw.stream_round ?? raw.streamRound),
    userTurnId,
    userTurnBinding: 'strong',
    userTurnBindingSource: 'explicit',
    modelTurnId,
    createdAtMs: normalizeCreatedAtMs(raw.created_at ?? raw.createdAt),
    createdSeq: resolveCanonicalTranscriptSeq(raw, index, snapshotSeq),
    turnOrder: Number.MAX_SAFE_INTEGER
  };
};

const resolveCanonicalTranscriptSeq = (
  raw: ChatRuntimeRawMessage,
  index: number,
  snapshotSeq: number
): number => {
  const turnIndex = normalizeSeq(raw.turn_index ?? raw.turnIndex);
  return snapshotSeq + (turnIndex ?? index + 1);
};

const resolveCanonicalCreatedAt = (raw: ChatRuntimeRawMessage): string =>
  firstText(raw.created_at, raw.createdAt);

const resolveCanonicalModelTurnStatus = (
  status: ChatRuntimeMessageStatus
): ChatRuntimeModelTurnProjection['status'] => {
  if (status === 'failed') return 'failed';
  if (status === 'cancelled') return 'cancelled';
  if (status === 'tooling') return 'tool_running';
  if (status === 'streaming') return 'streaming';
  if (status === 'waiting_first_output' || status === 'placeholder') {
    return 'waiting_first_output';
  }
  return 'completed';
};

const resolveCanonicalUserTurnStatus = (
  session: ChatRuntimeSessionProjection,
  turn: ChatRuntimeUserTurnProjection
): ChatRuntimeUserTurnProjection['status'] => {
  const modelStatuses = turn.modelTurnIds
    .map((modelTurnId) => session.modelTurnById[modelTurnId]?.status)
    .filter(Boolean);
  if (modelStatuses.includes('failed')) return 'failed';
  if (modelStatuses.includes('cancelled')) return 'cancelled';
  if (
    modelStatuses.some((status) =>
      status === 'created' ||
      status === 'waiting_first_output' ||
      status === 'streaming' ||
      status === 'tool_running' ||
      status === 'finalizing'
    )
  ) {
    return 'model_running';
  }
  return modelStatuses.length > 0 ? 'completed' : 'accepted';
};

const ensureCanonicalModelTurn = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string,
  seq: number | null
): ChatRuntimeModelTurnProjection => {
  const id = modelTurnId || `canonical-model-turn:${session.sessionId}:${session.modelTurns.length + 1}`;
  const resolvedUserTurnId = userTurnId || `orphan-user-turn:${id}`;
  if (!session.modelTurnById[id]) {
    const userTurn = ensureUserTurn(session, resolvedUserTurnId, seq);
    session.modelTurnById[id] = {
      id,
      userTurnId: userTurn.id,
      createdSeq: seq ?? nextLocalSeq(session),
      messageIds: [],
      finalMessageId: '',
      status: 'created'
    };
    addUnique(session.modelTurns, id);
    addUnique(userTurn.modelTurnIds, id);
  } else if (userTurnId) {
    const turn = session.modelTurnById[id];
    turn.userTurnId = userTurnId;
    const userTurn = ensureUserTurn(session, userTurnId, seq);
    addUnique(userTurn.modelTurnIds, id);
  }
  return session.modelTurnById[id];
};

const buildLegacyMessagePlans = (
  session: ChatRuntimeSessionProjection,
  messages: ChatRuntimeRawMessage[],
  snapshotSeq: number
): LegacyMessagePlan[] => {
  const plans: LegacyMessagePlan[] = [];
  messages.forEach((raw, index) => {
    if (!raw || typeof raw !== 'object') return;
    const role = normalizeRole(raw.role);
    if (role !== 'user' && role !== 'assistant') return;
    const id = resolveLegacyMessageId(raw, index);
    const status = resolveLegacyMessageStatus(raw);
    const streamRound = normalizeSeq(raw.stream_round ?? raw.streamRound);
    const explicitUserTurnId = normalizeId(raw.user_turn_id ?? raw.userTurnId);
    const explicitModelTurnId = normalizeId(raw.model_turn_id ?? raw.modelTurnId);
    const createdAtMs = normalizeCreatedAtMs(raw.created_at ?? raw.createdAt);
    const resolvedUserTurn = role === 'user' && !explicitUserTurnId
      ? streamRound !== null
        ? resolveLegacyUserTurnIdForRound(session, streamRound)
        : resolveLegacyUserTurnIdForMessage(session, raw, id, createdAtMs)
      : '';
    const userTurnId =
      explicitUserTurnId ||
      resolvedUserTurn;
    const modelTurnId = role === 'assistant' ? explicitModelTurnId : '';
    plans.push({
      raw,
      index,
      role,
      id,
      status,
      streamRound,
      userTurnId,
      userTurnBinding: userTurnId ? 'strong' : 'none',
      userTurnBindingSource: explicitUserTurnId
        ? 'explicit'
        : resolvedUserTurn
          ? streamRound !== null
            ? 'stream_round'
            : 'message'
          : 'none',
      modelTurnId,
      createdAtMs,
      createdSeq: snapshotSeq + index + 1,
      turnOrder: Number.MAX_SAFE_INTEGER
    });
  });

  const userPlansByRound = new Map<number, LegacyMessagePlan[]>();
  const userPlans = plans.filter((plan) => plan.role === 'user');
  userPlans.forEach((plan) => {
    if (plan.role !== 'user') return;
    if (plan.streamRound !== null) {
      const bucket = userPlansByRound.get(plan.streamRound) || [];
      bucket.push(plan);
      userPlansByRound.set(plan.streamRound, bucket);
    }
  });

  plans.forEach((plan) => {
    if (plan.role !== 'assistant' || plan.userTurnId) return;
    const resolution =
      resolveLegacyUserTurnByRound(userPlans, userPlansByRound, plan) ||
      resolveAdjacentLegacyUserTurn(userPlans, plan) ||
      resolveLegacyUserTurnByTimestamp(userPlans, plan) ||
      (userPlans.length === 0 ? resolveNearestLegacyUserTurnId(session, plan.index) : null);
    if (!resolution) return;
    plan.userTurnId = resolution.userTurnId;
    plan.userTurnBinding = resolution.strength;
    plan.userTurnBindingSource = resolution.source;
  });
  plans.forEach((plan) => {
    if (plan.role !== 'assistant' || plan.modelTurnId) return;
    plan.modelTurnId = resolveLegacyModelTurnId(
      session,
      plan.userTurnId,
      plan.userTurnBinding,
      plan.userTurnBindingSource,
      plan.streamRound,
      plan.id,
      plan.status
    );
  });

  const orderedUserPlans = [...userPlans].sort(compareLegacyUserPlanOrder);
  orderedUserPlans.forEach((plan, index) => {
    plan.turnOrder = index;
  });
  const turnOrderByUserTurnId = new Map(
    orderedUserPlans.map((plan, index) => [plan.userTurnId, index])
  );
  plans.forEach((plan) => {
    if (plan.role === 'assistant') {
      plan.turnOrder = turnOrderByUserTurnId.get(plan.userTurnId) ??
        resolveOrphanLegacyAssistantTurnOrder(orderedUserPlans, plan);
    }
  });

  const semanticOrder = [...plans].sort(compareLegacyMessagePlanOrder);
  semanticOrder.forEach((plan, semanticIndex) => {
    plan.createdSeq = snapshotSeq + semanticIndex + 1;
  });

  return semanticOrder;
};

const resolveLegacyUserTurnByRound = (
  userPlans: LegacyMessagePlan[],
  userPlansByRound: Map<number, LegacyMessagePlan[]>,
  assistantPlan: LegacyMessagePlan
): LegacyUserTurnResolution | null => {
  if (assistantPlan.streamRound === null) return null;
  const sameRoundUsers = userPlansByRound.get(assistantPlan.streamRound) || [];
  if (sameRoundUsers.length !== 1) return null;
  const matchedUser = sameRoundUsers[0];
  const hasInterveningUser = userPlans.some((plan) =>
    plan.index > matchedUser.index && plan.index < assistantPlan.index
  );
  if (hasInterveningUser) return null;
  return {
    userTurnId: matchedUser.userTurnId,
    strength: 'strong',
    source: 'stream_round'
  };
};

const resolveLegacyUserTurnByTimestamp = (
  userPlans: LegacyMessagePlan[],
  assistantPlan: LegacyMessagePlan
): LegacyUserTurnResolution | null => {
  if (assistantPlan.createdAtMs === null) return null;
  const precedingUser = userPlans
    .filter((plan) => plan.createdAtMs !== null && Number(plan.createdAtMs) <= Number(assistantPlan.createdAtMs))
    .sort((left, right) => {
      const timeDiff = Number(right.createdAtMs) - Number(left.createdAtMs);
      return timeDiff || right.index - left.index;
    })[0];
  return precedingUser
    ? {
        userTurnId: precedingUser.userTurnId,
        strength: 'strong',
        source: 'timestamp'
      }
    : null;
};

const resolveAdjacentLegacyUserTurn = (
  userPlans: LegacyMessagePlan[],
  assistantPlan: LegacyMessagePlan
): LegacyUserTurnResolution | null => {
  const previousUser = userPlans
    .filter((plan) => plan.index < assistantPlan.index)
    .sort((left, right) => right.index - left.index)[0];
  if (
    previousUser &&
    previousUser.createdAtMs !== null &&
    assistantPlan.createdAtMs !== null &&
    Number(assistantPlan.createdAtMs) < Number(previousUser.createdAtMs)
  ) {
    return null;
  }
  return previousUser
    ? {
        userTurnId: previousUser.userTurnId,
        strength: 'strong',
        source: 'adjacent_previous'
      }
    : null;
};

const resolveLegacyUserTurnIdForRound = (
  session: ChatRuntimeSessionProjection,
  streamRound: number
): string => {
  const canonical = `user-turn:${session.sessionId}:round:${streamRound}`;
  if (session.userTurnById[canonical]) return canonical;
  const legacy = `legacy-user-turn:round:${streamRound}`;
  if (session.userTurnById[legacy]) return legacy;
  const suffix = `:round:${streamRound}`;
  const existing = session.userTurns.find((turnId) => turnId.endsWith(suffix));
  return existing || canonical;
};

const resolveLegacyUserTurnIdForMessage = (
  session: ChatRuntimeSessionProjection,
  raw: ChatRuntimeRawMessage,
  legacyMessageId: string,
  createdAtMs: number | null
): string => {
  const clientMessageId = firstText(
    raw.client_message_id,
    raw.clientMessageId,
    asRecord(raw.payload).client_message_id,
    asRecord(raw.payload).clientMessageId
  );
  if (clientMessageId) {
    const clientMessage = session.messageById[clientMessageId];
    if (clientMessage?.role === 'user') {
      return clientMessage.userTurnId;
    }
  }
  const explicitMessageId = firstText(raw.message_id, raw.messageId, raw.id);
  if (explicitMessageId) {
    const existingMessage = session.messageById[explicitMessageId];
    if (existingMessage?.role === 'user') {
      return existingMessage.userTurnId;
    }
  }
  const matchedMessage = findLegacyUserMessageByContent(session, raw, createdAtMs);
  if (matchedMessage) {
    return matchedMessage.userTurnId;
  }
  return `legacy-user-turn:${legacyMessageId}`;
};

const resolveLegacyModelTurnId = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string,
  userTurnBinding: LegacyUserTurnBindingStrength,
  userTurnBindingSource: LegacyUserTurnBindingSource,
  streamRound: number | null,
  legacyMessageId: string,
  status: ChatRuntimeMessageStatus
): string => {
  if (userTurnId && userTurnBinding === 'strong') {
    const reusable = resolveReusableModelTurnForUserTurn(
      session,
      userTurnId,
      true,
      status === 'cancelled' ? ['cancelled'] : ['completed', 'failed']
    );
    if (reusable) return reusable.id;
  } else if (userTurnId && status !== 'final') {
    const activeReusable = resolveReusableModelTurnForUserTurn(session, userTurnId);
    if (activeReusable) return activeReusable.id;
  }
  if (streamRound !== null && userTurnBindingSource === 'stream_round') {
    const canonical = `model-turn:${session.sessionId}:user:${streamRound}:model:1`;
    if (session.modelTurnById[canonical]) return canonical;
    const suffix = `:user:${streamRound}:model:1`;
    const existingByRound = session.modelTurns.find((turnId) => turnId.endsWith(suffix));
    if (existingByRound) return existingByRound;
    return canonical;
  }
  return `legacy-model-turn:${legacyMessageId}`;
};

const resolveOrphanLegacyAssistantTurnOrder = (
  orderedUserPlans: LegacyMessagePlan[],
  assistantPlan: LegacyMessagePlan
): number => {
  const precedingUserCount = orderedUserPlans.filter((plan) => plan.index < assistantPlan.index).length;
  return precedingUserCount - 0.5;
};

const compareLegacyUserPlanOrder = (
  left: LegacyMessagePlan,
  right: LegacyMessagePlan
): number => {
  if (left.streamRound !== null && right.streamRound !== null && left.streamRound !== right.streamRound) {
    return left.streamRound - right.streamRound;
  }
  const leftHasTime = left.createdAtMs !== null;
  const rightHasTime = right.createdAtMs !== null;
  if (leftHasTime && rightHasTime && left.createdAtMs !== right.createdAtMs) {
    return Number(left.createdAtMs) - Number(right.createdAtMs);
  }
  if (leftHasTime !== rightHasTime) {
    return leftHasTime ? -1 : 1;
  }
  if (left.streamRound !== null && right.streamRound !== null && left.streamRound !== right.streamRound) {
    return left.streamRound - right.streamRound;
  }
  return left.index - right.index;
};

const compareLegacyMessagePlanOrder = (
  left: LegacyMessagePlan,
  right: LegacyMessagePlan
): number => {
  if (left.turnOrder !== right.turnOrder) {
    return left.turnOrder - right.turnOrder;
  }
  if (left.role !== right.role) {
    return left.role === 'user' ? -1 : 1;
  }
  if (left.streamRound !== null && right.streamRound !== null && left.streamRound !== right.streamRound) {
    return left.streamRound - right.streamRound;
  }
  const leftHasTime = left.createdAtMs !== null;
  const rightHasTime = right.createdAtMs !== null;
  if (leftHasTime && rightHasTime && left.createdAtMs !== right.createdAtMs) {
    return Number(left.createdAtMs) - Number(right.createdAtMs);
  }
  if (leftHasTime !== rightHasTime) {
    return leftHasTime ? -1 : 1;
  }
  return left.index - right.index;
};

const shouldReplaceSnapshotMessage = (
  message: ChatRuntimeMessageProjection,
  snapshotSeq: number,
  replaceExistingAtOrBelowSeq: boolean
): boolean => {
  if (!replaceExistingAtOrBelowSeq) return false;
  return message.updatedSeq <= snapshotSeq;
};

const patchMessageFromRaw = (
  message: ChatRuntimeMessageProjection,
  raw: ChatRuntimeRawMessage,
  status: ChatRuntimeMessageStatus,
  seq: number
): void => {
  message.content = String(raw.content ?? '');
  message.reasoning = String(raw.reasoning ?? '');
  message.status = status;
  message.final = status === 'final';
  message.failed = status === 'failed';
  message.cancelled = status === 'cancelled';
  if (Array.isArray(raw.workflowItems)) {
    message.workflowItems = raw.workflowItems
      .filter(isPlainRecord)
      .map((item) => ({ ...item }));
  }
  if (Array.isArray(raw.subagents)) {
    message.subagents = raw.subagents
      .filter(isPlainRecord)
      .map((item) => ({ ...item }));
  }
  message.updatedSeq = Math.max(message.updatedSeq, seq);
};

const mergeMessageFromRaw = (
  message: ChatRuntimeMessageProjection,
  raw: ChatRuntimeRawMessage,
  status: ChatRuntimeMessageStatus,
  seq: number
): void => {
  const nextContent = String(raw.content ?? '');
  const nextReasoning = String(raw.reasoning ?? '');
  if (nextContent) {
    message.content = mergeLegacyText(message.content, nextContent);
  }
  if (nextReasoning) {
    message.reasoning = mergeLegacyText(message.reasoning, nextReasoning);
  }
  message.status = pickMergedMessageStatus(message.status, status);
  message.final = message.status === 'final';
  message.failed = message.failed || status === 'failed';
  message.cancelled = message.cancelled || status === 'cancelled';
  mergeProjectionRecords(message, 'workflowItems', raw.workflowItems);
  mergeProjectionRecords(message, 'subagents', raw.subagents);
  message.updatedSeq = Math.max(message.updatedSeq, seq);
};

const shouldMergeFoldedLegacyMessage = (
  raw: ChatRuntimeRawMessage,
  status: ChatRuntimeMessageStatus
): boolean => {
  if (!isActiveMessageStatus(status)) return false;
  const streamEventId = normalizeSeq(raw.stream_event_id ?? raw.streamEventId);
  return streamEventId !== null;
};

const mergeLegacyText = (current: string, incoming: string): string => {
  if (!incoming) return current;
  if (!current) return incoming;
  if (incoming === current) return current;
  if (incoming.startsWith(current)) return incoming;
  if (current.startsWith(incoming)) return current;
  const overlap = resolveTextOverlapLength(current, incoming);
  return overlap > 0 ? `${current}${incoming.slice(overlap)}` : `${current}${incoming}`;
};

const resolveTextOverlapLength = (current: string, incoming: string): number => {
  const limit = Math.min(current.length, incoming.length, 1024);
  for (let size = limit; size > 0; size -= 1) {
    if (current.endsWith(incoming.slice(0, size))) return size;
  }
  return 0;
};

const pickMergedMessageStatus = (
  current: ChatRuntimeMessageStatus,
  incoming: ChatRuntimeMessageStatus
): ChatRuntimeMessageStatus => {
  if (incoming === 'failed' || current === 'failed') return 'failed';
  if (incoming === 'cancelled' || current === 'cancelled') return 'cancelled';
  if (incoming === 'tooling' || current === 'tooling') return 'tooling';
  if (incoming === 'streaming' || current === 'streaming') return 'streaming';
  if (incoming === 'waiting_first_output' || current === 'waiting_first_output') {
    return 'waiting_first_output';
  }
  if (incoming === 'placeholder' || current === 'placeholder') return 'placeholder';
  return 'final';
};

const mergeProjectionRecords = (
  message: ChatRuntimeMessageProjection,
  field: 'workflowItems' | 'subagents',
  value: unknown
): void => {
  if (!Array.isArray(value)) return;
  const incoming = value.filter(isPlainRecord).map((item) => ({ ...item }));
  if (incoming.length === 0) return;
  const existing = Array.isArray(message[field]) ? message[field] || [] : [];
  message[field] = incoming.length >= existing.length
    ? incoming
    : dedupeProjectionRecords([...existing, ...incoming]);
};

const dedupeProjectionRecords = (
  records: ChatRuntimeWorkflowItemProjection[] | ChatRuntimeSubagentProjection[]
): ChatRuntimeWorkflowItemProjection[] | ChatRuntimeSubagentProjection[] => {
  const seen = new Set<string>();
  return records.filter((record, index) => {
    const key = resolveProjectionRecordIdentity(record, index);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
};

const resolveProjectionRecordIdentity = (
  record: ChatRuntimeWorkflowItemProjection | ChatRuntimeSubagentProjection,
  index: number
): string => {
  const stable = firstText(
    record.id,
    record.key,
    record.taskId,
    record.task_id,
    record.toolCallId,
    record.tool_call_id,
    record.callId,
    record.call_id,
    record.commandSessionId,
    record.command_session_id,
    record.approvalId,
    record.approval_id,
    record.runId,
    record.run_id,
    record.sessionId,
    record.session_id,
    record.eventId,
    record.event_id
  );
  if (stable) return stable;
  try {
    return JSON.stringify(record);
  } catch {
    return `record:${index}`;
  }
};

const ensureUserTurn = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string,
  seq: number | null
): ChatRuntimeUserTurnProjection => {
  const id = userTurnId || `local-user-turn:${session.sessionId}:${session.userTurns.length + 1}`;
  if (!session.userTurnById[id]) {
    session.userTurnById[id] = {
      id,
      createdSeq: seq ?? nextLocalSeq(session),
      messageIds: [],
      modelTurnIds: [],
      status: 'created'
    };
    addUnique(session.userTurns, id);
  }
  return session.userTurnById[id];
};

const resolveUserMessageIdForTurn = (
  session: ChatRuntimeSessionProjection,
  turn: ChatRuntimeUserTurnProjection,
  eventMessageId: string,
  source: Record<string, unknown>
): string => {
  const clientMessageId = firstText(
    source.client_message_id,
    source.clientMessageId,
    asRecord(source.payload).client_message_id,
    asRecord(source.payload).clientMessageId
  );
  if (clientMessageId && session.messageById[clientMessageId]?.role === 'user') {
    return clientMessageId;
  }
  const exactExisting = eventMessageId ? session.messageById[eventMessageId] : null;
  if (exactExisting?.role === 'user') {
    return exactExisting.id;
  }
  const byTurn = turn.messageIds.find((messageId) => session.messageById[messageId]?.role === 'user');
  if (byTurn) return byTurn;
  const content = String(source.content ?? asRecord(source.payload).content ?? '');
  const byContent = findRecentUserMessageByContent(session, turn.id, content);
  if (byContent) return byContent.id;
  return eventMessageId || `local-user:${turn.id}`;
};

const findRecentUserMessageByContent = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string,
  content: string
): ChatRuntimeMessageProjection | null => {
  if (!content) return null;
  const sameTurn = Object.values(session.messageById)
    .filter((message) =>
      message.role === 'user' &&
      message.userTurnId === userTurnId &&
      message.content === content
    )
    .sort((left, right) => left.createdSeq - right.createdSeq)[0];
  if (sameTurn) return sameTurn;
  return Object.values(session.messageById)
    .filter((message) =>
      message.role === 'user' &&
      message.content === content &&
      isLocalOptimisticUserTurn(message.userTurnId) &&
      !hasTerminalModelTurnForUserTurn(session, message.userTurnId)
    )
    .sort((left, right) => right.createdSeq - left.createdSeq)[0] || null;
};

const hasTerminalModelTurnForUserTurn = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string
): boolean =>
  Boolean(session.userTurnById[userTurnId]?.modelTurnIds?.some((turnId) => {
    const status = session.modelTurnById[turnId]?.status;
    return status === 'completed' || status === 'failed' || status === 'cancelled';
  }));

const findLegacyUserMessageByContent = (
  session: ChatRuntimeSessionProjection,
  raw: ChatRuntimeRawMessage,
  createdAtMs: number | null
): ChatRuntimeMessageProjection | null => {
  const content = String(raw.content ?? asRecord(raw.payload).content ?? '');
  if (!content) return null;
  const candidates = Object.values(session.messageById)
    .filter((message) => message.role === 'user' && message.content === content);
  if (candidates.length === 0) return null;
  return candidates.sort((left, right) => {
    if (createdAtMs !== null) {
      const leftMs = normalizeCreatedAtMs(left.createdAt);
      const rightMs = normalizeCreatedAtMs(right.createdAt);
      if (leftMs !== null && rightMs !== null) {
        const delta = Math.abs(leftMs - createdAtMs) - Math.abs(rightMs - createdAtMs);
        if (delta !== 0) return delta;
      }
    }
    const leftOptimistic = isLocalOptimisticUserTurn(left.userTurnId) ? 1 : 0;
    const rightOptimistic = isLocalOptimisticUserTurn(right.userTurnId) ? 1 : 0;
    if (leftOptimistic !== rightOptimistic) {
      return rightOptimistic - leftOptimistic;
    }
    return right.createdSeq - left.createdSeq;
  })[0] || null;
};

const pruneUserTurnUserMessages = (
  session: ChatRuntimeSessionProjection,
  turn: ChatRuntimeUserTurnProjection,
  keepMessageId: string
): void => {
  const staleIds = turn.messageIds.filter((messageId) => {
    if (messageId === keepMessageId) return false;
    return session.messageById[messageId]?.role === 'user';
  });
  if (staleIds.length === 0) return;
  turn.messageIds = turn.messageIds.filter((messageId) => !staleIds.includes(messageId));
  session.messages = session.messages.filter((messageId) => !staleIds.includes(messageId));
  staleIds.forEach((messageId) => {
    delete session.messageById[messageId];
  });
};

const isLocalOptimisticUserTurn = (userTurnId: string): boolean =>
  userTurnId.startsWith('user-turn:') || userTurnId.startsWith('local-user-turn:');

const ensureModelTurn = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string,
  seq: number | null
): ChatRuntimeModelTurnProjection => {
  const id = resolveModelTurnIdentity(session, modelTurnId, userTurnId) ||
    `local-model-turn:${session.sessionId}:${session.modelTurns.length + 1}`;
  if (!session.modelTurnById[id]) {
    const resolvedUserTurnId = userTurnId || `orphan-user-turn:${id}`;
    const userTurn = ensureUserTurn(session, resolvedUserTurnId, seq);
    session.modelTurnById[id] = {
      id,
      userTurnId: userTurn.id,
      createdSeq: seq ?? nextLocalSeq(session),
      messageIds: [],
      finalMessageId: '',
      status: 'created'
    };
    addUnique(session.modelTurns, id);
    addUnique(userTurn.modelTurnIds, id);
  } else if (userTurnId) {
    const turn = session.modelTurnById[id];
    if (!turn.userTurnId || turn.userTurnId.startsWith('orphan-user-turn:')) {
      turn.userTurnId = userTurnId;
    }
    const userTurn = ensureUserTurn(session, turn.userTurnId, seq);
    addUnique(userTurn.modelTurnIds, id);
  }
  return session.modelTurnById[id];
};

const resolveModelTurnIdentity = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string
): string => {
  if (!modelTurnId) return '';
  const existing = session.modelTurnById[modelTurnId];
  if (existing) return modelTurnId;
  const existingForUserTurn = resolveReusableModelTurnForUserTurn(session, userTurnId);
  if (
    existingForUserTurn &&
    shouldFoldModelTurnIntoExisting(modelTurnId, existingForUserTurn, userTurnId)
  ) {
    return existingForUserTurn.id;
  }
  return modelTurnId;
};

const resolveReusableModelTurnForUserTurn = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string,
  includeTerminal = false,
  terminalStatuses: Array<ChatRuntimeModelTurnProjection['status']> = [
    'completed',
    'failed',
    'cancelled'
  ]
): ChatRuntimeModelTurnProjection | null => {
  if (!userTurnId) return null;
  const userTurn = session.userTurnById[userTurnId];
  const turnIds = userTurn?.modelTurnIds?.length
    ? userTurn.modelTurnIds
    : session.modelTurns.filter((turnId) => session.modelTurnById[turnId]?.userTurnId === userTurnId);
  for (let index = turnIds.length - 1; index >= 0; index -= 1) {
    const turn = session.modelTurnById[turnIds[index]];
    if (!turn) {
      continue;
    }
    if (
      !includeTerminal &&
      (turn.status === 'completed' || turn.status === 'failed' || turn.status === 'cancelled')
    ) {
      continue;
    }
    if (
      includeTerminal &&
      (turn.status === 'completed' || turn.status === 'failed' || turn.status === 'cancelled') &&
      !terminalStatuses.includes(turn.status)
    ) {
      continue;
    }
    if (turn.messageIds.some((messageId) => session.messageById[messageId]?.role === 'assistant')) {
      return turn;
    }
  }
  return null;
};

const shouldFoldModelTurnIntoExisting = (
  incomingModelTurnId: string,
  existing: ChatRuntimeModelTurnProjection,
  userTurnId: string
): boolean => {
  if (!incomingModelTurnId || !existing || !userTurnId) return false;
  if (incomingModelTurnId.startsWith('legacy-model-turn:')) return true;
  if (incomingModelTurnId.startsWith(`model-turn:${userTurnId}`)) return true;
  if (incomingModelTurnId.startsWith(`model-turn:${existing.userTurnId}`)) return true;
  if (incomingModelTurnId.includes(`:${userTurnId}:`)) return true;
  if (incomingModelTurnId.includes(':model:')) return true;
  return incomingModelTurnId.startsWith('model-turn:');
};

const ensureAssistantMessageForModelTurn = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  status: ChatRuntimeMessageStatus
): ChatRuntimeMessageProjection => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const messageId = resolveAssistantMessageIdForModelTurn(session, modelTurn, event.messageId);
  const message = ensureMessage(session, {
    id: messageId,
    role: 'assistant',
    createdSeq: event.eventSeq,
    createdAt: event.createdAt,
    userTurnId: modelTurn.userTurnId,
    modelTurnId: modelTurn.id
  });
  message.status = status;
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  addUnique(modelTurn.messageIds, message.id);
  addUnique(session.messages, message.id);
  pruneModelTurnAssistantMessages(session, modelTurn, message.id);
  return message;
};

const resolveAssistantMessageIdForModelTurn = (
  session: ChatRuntimeSessionProjection,
  modelTurn: ChatRuntimeModelTurnProjection,
  eventMessageId: string
): string => {
  if (modelTurn.finalMessageId) return modelTurn.finalMessageId;
  const existingAssistantId = modelTurn.messageIds.find((messageId) => {
    const message = session.messageById[messageId];
    return message?.role === 'assistant';
  });
  if (existingAssistantId) return existingAssistantId;
  return eventMessageId || `local-assistant:${modelTurn.id}`;
};

const pruneModelTurnAssistantMessages = (
  session: ChatRuntimeSessionProjection,
  modelTurn: ChatRuntimeModelTurnProjection,
  keepMessageId: string
): void => {
  const staleIds = modelTurn.messageIds.filter((messageId) => {
    if (messageId === keepMessageId) return false;
    return session.messageById[messageId]?.role === 'assistant';
  });
  if (staleIds.length === 0) return;
  modelTurn.messageIds = modelTurn.messageIds.filter((messageId) => !staleIds.includes(messageId));
  session.messages = session.messages.filter((messageId) => !staleIds.includes(messageId));
  staleIds.forEach((messageId) => {
    delete session.messageById[messageId];
  });
};

const upsertToolWorkflowItem = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  status: 'loading' | 'completed' | 'failed'
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedWorkflowItems(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const eventType = resolveProjectedWorkflowEventType(event, status);
  const toolName = firstText(
    data.tool,
    data.name,
    data.tool_name,
    data.toolName,
    payload.tool,
    payload.name,
    payload.tool_name,
    payload.toolName
  );
  const toolCallId = resolveToolWorkflowRef(event, payload, data);
  const commandSessionId = firstText(
    data.command_session_id,
    data.commandSessionId,
    payload.command_session_id,
    payload.commandSessionId
  );
  const approvalId = firstText(
    data.approval_id,
    data.approvalId,
    payload.approval_id,
    payload.approvalId
  );
  const itemId = resolveProjectedWorkflowItemId(event, eventType, toolCallId, commandSessionId, approvalId);
  const workflowRef = eventType === 'approval_request' || eventType === 'approval_result'
    ? approvalId || toolCallId
    : toolCallId;
  const existing = findProjectedWorkflowItem(items, itemId, workflowRef, commandSessionId, approvalId);
  const detailSource = Object.keys(data).length > 0 ? data : payload;
  const title = resolveProjectedWorkflowTitle(eventType, toolName);
  const next: ChatRuntimeWorkflowItemProjection = {
    ...(existing || {}),
    id: itemId,
    title,
    detail: stringifyWorkflowDetail(detailSource),
    status,
    isTool: true,
    eventType,
    sourceEventType: event.type,
    updatedSeq: event.eventSeq ?? message.updatedSeq
  };
  if (toolName) {
    next.toolName = toolName;
    next.tool = toolName;
  }
  if (toolCallId) {
    next.toolCallId = toolCallId;
    next.tool_call_id = toolCallId;
  }
  if (commandSessionId) {
    next.commandSessionId = commandSessionId;
    next.command_session_id = commandSessionId;
  }
  if (approvalId) {
    next.approvalId = approvalId;
    next.approval_id = approvalId;
  }

  if (existing) {
    Object.assign(existing, next);
  } else {
    items.push({
      ...next,
      createdSeq: event.eventSeq ?? message.updatedSeq
    });
  }
};

const resolveProjectedWorkflowStatus = (
  sourceType: string,
  payload: Record<string, unknown>
): 'loading' | 'completed' | 'failed' => {
  if (sourceType === 'team_error') return 'failed';
  if (sourceType === 'team_finish') return 'completed';
  const data = asRecord(payload.data);
  const status = normalizeText(
    data.status ??
      payload.status ??
      data.thread_status ??
      data.threadStatus ??
      payload.thread_status ??
      payload.threadStatus
  );
  if (FAILED_WORKFLOW_STATUSES.has(status)) return 'failed';
  if (SUCCESS_WORKFLOW_STATUSES.has(status)) return 'completed';
  if (!status && (
    sourceType === 'subagent_interrupt' ||
    sourceType === 'subagent_close' ||
    sourceType === 'subagent_announce'
  )) {
    return 'completed';
  }
  if (
    sourceType === 'subagent_dispatch_finish' ||
    sourceType === 'team_task_result' ||
    sourceType === 'team_merge'
  ) {
    return 'completed';
  }
  return 'loading';
};

const upsertProjectedWorkflowEventItem = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  sourceType: string,
  status: 'loading' | 'completed' | 'failed'
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedWorkflowItems(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const detailSource = Object.keys(data).length > 0 ? data : payload;
  const refs = resolveWorkflowEventRefs(event, payload, data);
  const itemId = resolveProjectedGenericWorkflowItemId(event, sourceType, refs);
  const existing = findProjectedWorkflowItem(
    items,
    itemId,
    refs.toolCallId || refs.runId || refs.sessionId,
    refs.commandSessionId || refs.dispatchId,
    refs.approvalId || refs.taskId
  );
  const title = resolveProjectedGenericWorkflowTitle(sourceType, detailSource, status);
  const next: ChatRuntimeWorkflowItemProjection = {
    ...(existing || {}),
    id: itemId,
    title,
    detail: stringifyWorkflowDetail(detailSource),
    status,
    eventType: sourceType || 'workflow_event',
    sourceEventType: sourceType || event.type,
    updatedSeq: event.eventSeq ?? message.updatedSeq
  };
  if (refs.dispatchId) {
    next.dispatchId = refs.dispatchId;
    next.dispatch_id = refs.dispatchId;
  }
  if (refs.sessionId) {
    next.sessionId = refs.sessionId;
    next.session_id = refs.sessionId;
  }
  if (refs.runId) {
    next.runId = refs.runId;
    next.run_id = refs.runId;
  }
  if (refs.taskId) {
    next.taskId = refs.taskId;
    next.task_id = refs.taskId;
  }
  if (refs.agentId) {
    next.agentId = refs.agentId;
    next.agent_id = refs.agentId;
  }
  if (TEAM_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    next.kind = 'team';
  }
  if (SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    next.kind = 'subagent';
  }

  if (existing) {
    Object.assign(existing, next);
  } else {
    items.push({
      ...next,
      createdSeq: event.eventSeq ?? message.updatedSeq
    });
  }
};

const upsertProjectedSubagents = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  sourceType: string,
  status: 'loading' | 'completed' | 'failed'
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedSubagents(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const source = Object.keys(data).length > 0 ? data : payload;
  const payloads = collectProjectedSubagentPayloads(source);
  const candidates = payloads.length > 0 ? payloads : [source];
  candidates.forEach((candidate) => {
    const next = buildProjectedSubagent(candidate, event, sourceType, status);
    if (!next) return;
    const existing = findProjectedSubagent(items, next);
    if (existing) {
      Object.assign(existing, next);
    } else {
      items.push(next);
    }
  });
  message.subagents = sortProjectedSubagents(items);
};

type WorkflowEventRefs = {
  dispatchId: string;
  sessionId: string;
  runId: string;
  taskId: string;
  agentId: string;
  toolCallId: string;
  commandSessionId: string;
  approvalId: string;
};

const resolveWorkflowEventRefs = (
  event: NormalizedRuntimeEvent,
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): WorkflowEventRefs => ({
  dispatchId: firstText(
    data.dispatch_id,
    data.dispatchId,
    payload.dispatch_id,
    payload.dispatchId
  ),
  sessionId: firstText(
    data.session_id,
    data.sessionId,
    data.target_session_id,
    data.targetSessionId,
    data.spawned_session_id,
    data.spawnedSessionId,
    payload.session_id,
    payload.sessionId,
    payload.target_session_id,
    payload.targetSessionId,
    payload.spawned_session_id,
    payload.spawnedSessionId
  ),
  runId: firstText(
    data.run_id,
    data.runId,
    data.session_run_id,
    data.sessionRunId,
    payload.run_id,
    payload.runId,
    payload.session_run_id,
    payload.sessionRunId
  ),
  taskId: firstText(
    data.task_id,
    data.taskId,
    payload.task_id,
    payload.taskId
  ),
  agentId: firstText(
    data.agent_id,
    data.agentId,
    payload.agent_id,
    payload.agentId
  ),
  toolCallId: resolveToolWorkflowRef(event, payload, data),
  commandSessionId: firstText(
    data.command_session_id,
    data.commandSessionId,
    payload.command_session_id,
    payload.commandSessionId
  ),
  approvalId: firstText(
    data.approval_id,
    data.approvalId,
    payload.approval_id,
    payload.approvalId
  )
});

const resolveProjectedGenericWorkflowItemId = (
  event: NormalizedRuntimeEvent,
  sourceType: string,
  refs: WorkflowEventRefs
): string => {
  const workflowKind = SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)
    ? 'subagent'
    : TEAM_WORKFLOW_EVENT_TYPES.has(sourceType)
      ? 'team'
      : sourceType || 'workflow';
  const workflowRef =
    refs.runId ||
    refs.sessionId ||
    refs.dispatchId ||
    refs.taskId ||
    refs.toolCallId ||
    refs.commandSessionId ||
    refs.approvalId;
  if (workflowRef) {
    return `runtime-workflow:${event.modelTurnId || event.messageId}:${workflowKind}:${workflowRef}`;
  }
  if (event.eventId) {
    return `runtime-workflow:${event.modelTurnId || event.messageId}:event:${event.eventId}`;
  }
  return `runtime-workflow:${event.modelTurnId || event.messageId}:${workflowKind}:${event.eventSeq ?? 'local'}`;
};

const resolveProjectedGenericWorkflowTitle = (
  sourceType: string,
  source: Record<string, unknown>,
  status: 'loading' | 'completed' | 'failed'
): string => {
  const label = firstText(
    source.label,
    source.spawn_label,
    source.spawnLabel,
    source.title,
    source.name,
    source.task_title,
    source.taskTitle,
    source.task
  );
  if (SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    if (sourceType === 'subagent_dispatch_start') {
      return label ? `Subagent dispatch: ${label}` : 'Subagent dispatch';
    }
    if (sourceType === 'subagent_dispatch_finish') {
      return label ? `Subagent result: ${label}` : 'Subagent result';
    }
    return label ? `Subagent: ${label}` : 'Subagent event';
  }
  if (TEAM_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    return label ? `Team event: ${label}` : `Team event: ${sourceType}`;
  }
  if (status === 'failed') return 'Workflow failed';
  if (status === 'loading') return 'Workflow running';
  return 'Workflow completed';
};

const ensureProjectedSubagents = (
  message: ChatRuntimeMessageProjection
): ChatRuntimeSubagentProjection[] => {
  if (!Array.isArray(message.subagents)) {
    message.subagents = [];
  }
  return message.subagents;
};

const buildProjectedSubagent = (
  source: Record<string, unknown>,
  event: NormalizedRuntimeEvent,
  sourceType: string,
  workflowStatus: 'loading' | 'completed' | 'failed'
): ChatRuntimeSubagentProjection | null => {
  const agentState = asRecord(source.agent_state ?? source.agentState);
  const detail = buildProjectedSubagentDetail(source, agentState);
  const sessionId = firstText(
    source.session_id,
    source.sessionId,
    source.target_session_id,
    source.targetSessionId,
    source.spawned_session_id,
    source.spawnedSessionId,
    detail.session_id,
    detail.sessionId
  );
  const runId = firstText(
    source.run_id,
    source.runId,
    source.session_run_id,
    source.sessionRunId,
    detail.run_id,
    detail.runId
  );
  const dispatchId = firstText(source.dispatch_id, source.dispatchId, detail.dispatch_id, detail.dispatchId);
  const key = runId || sessionId || dispatchId;
  if (!key) return null;
  const sourceStatus = normalizeText(source.status ?? agentState.status ?? detail.status);
  const status = resolveProjectedSubagentStatus(sourceStatus, workflowStatus);
  const label = firstText(
    source.label,
    source.spawn_label,
    source.spawnLabel,
    source.title,
    detail.label,
    detail.spawn_label,
    detail.spawnLabel,
    detail.title
  );
  const title = label || sessionId || runId || 'Subagent';
  const summary = firstText(
    source.summary,
    detail.summary,
    source.assistant_message,
    source.assistantMessage,
    detail.assistant_message,
    detail.assistantMessage,
    source.result,
    detail.result,
    agentState.message,
    source.error,
    source.error_message,
    source.errorMessage,
    detail.error,
    detail.error_message,
    detail.errorMessage
  );
  const updatedAtMs = resolveProjectedTimestampMs(
    source.updated_time,
    source.updatedTime,
    source.finished_time,
    source.finishedTime,
    source.started_time,
    source.startedTime,
    source.queued_time,
    source.queuedTime,
    source.created_at,
    source.createdAt,
    event.createdAt
  );
  const terminal =
    normalizeFlag(source.terminal) ||
    workflowStatus === 'completed' ||
    workflowStatus === 'failed' ||
    SUCCESS_WORKFLOW_STATUSES.has(status) ||
    FAILED_WORKFLOW_STATUSES.has(status);
  const failed = normalizeFlag(source.failed) || workflowStatus === 'failed' || FAILED_WORKFLOW_STATUSES.has(status);
  return {
    key,
    session_id: sessionId,
    run_id: runId,
    dispatch_id: dispatchId,
    title,
    label,
    status,
    summary,
    terminal,
    failed,
    canTerminate: normalizeFlag(source.can_terminate ?? source.canTerminate ?? (!terminal && !failed)),
    updated_at: updatedAtMs === null ? '' : new Date(updatedAtMs).toISOString(),
    updated_at_ms: updatedAtMs,
    parent_user_round: normalizeOptionalRound(source.parent_user_round ?? source.parentUserRound),
    parent_model_round: normalizeOptionalRound(source.parent_model_round ?? source.parentModelRound),
    detail,
    agent_state: {
      status: firstText(agentState.status, status),
      message: firstText(agentState.message, summary)
    },
    eventType: sourceType,
    sourceEventType: sourceType,
    updatedSeq: event.eventSeq ?? 0
  };
};

const buildProjectedSubagentDetail = (
  source: Record<string, unknown>,
  agentState: Record<string, unknown>
): Record<string, unknown> => {
  const detail = asRecord(source.detail);
  const output: Record<string, unknown> = {};
  const copy = (key: string, value: unknown): void => {
    if (value !== undefined && value !== null && value !== '') {
      output[key] = value;
    }
  };
  copy('agent_id', firstText(detail.agent_id, detail.agentId, source.agent_id, source.agentId));
  copy('session_id', firstText(detail.session_id, detail.sessionId, source.session_id, source.sessionId));
  copy('run_id', firstText(detail.run_id, detail.runId, source.run_id, source.runId));
  copy('status', firstText(agentState.status, detail.status, source.status));
  copy('assistant_message', firstText(detail.assistant_message, detail.assistantMessage, source.assistant_message, source.assistantMessage, source.result, agentState.message));
  copy('error', firstText(detail.error, detail.error_message, detail.errorMessage, source.error, source.error_message, source.errorMessage));
  copy('model_name', firstText(detail.model_name, detail.modelName, source.model_name, source.modelName));
  copy('requested_by', firstText(detail.requested_by, detail.requestedBy, source.requested_by, source.requestedBy));
  copy('spawned_by', firstText(detail.spawned_by, detail.spawnedBy, source.spawned_by, source.spawnedBy));
  copy('queued_at', firstText(detail.queued_at, detail.queuedAt, source.queued_at, source.queuedAt));
  copy('started_at', firstText(detail.started_at, detail.startedAt, source.started_at, source.startedAt));
  copy('finished_at', firstText(detail.finished_at, detail.finishedAt, source.finished_at, source.finishedAt));
  copy('updated_at', firstText(detail.updated_at, detail.updatedAt, source.updated_at, source.updatedAt));
  return Object.keys(output).length > 0 ? output : { ...source };
};

const resolveProjectedSubagentStatus = (
  sourceStatus: string,
  workflowStatus: 'loading' | 'completed' | 'failed'
): string => {
  if (workflowStatus === 'failed' && (!sourceStatus || ACTIVE_SUBAGENT_STATUSES.has(sourceStatus))) {
    return 'failed';
  }
  if (workflowStatus === 'completed' && (!sourceStatus || ACTIVE_SUBAGENT_STATUSES.has(sourceStatus))) {
    return 'completed';
  }
  if (sourceStatus) return sourceStatus;
  if (workflowStatus === 'failed') return 'failed';
  if (workflowStatus === 'completed') return 'completed';
  return 'running';
};

const findProjectedSubagent = (
  items: ChatRuntimeSubagentProjection[],
  incoming: ChatRuntimeSubagentProjection
): ChatRuntimeSubagentProjection | null => {
  const key = firstText(incoming.key, incoming.run_id, incoming.runId, incoming.session_id, incoming.sessionId);
  if (!key) return null;
  return items.find((item) => {
    const candidate = firstText(item.key, item.run_id, item.runId, item.session_id, item.sessionId);
    return candidate === key;
  }) || null;
};

const sortProjectedSubagents = (
  items: ChatRuntimeSubagentProjection[]
): ChatRuntimeSubagentProjection[] => [...items].sort((left, right) => {
  const leftTime = Number(left.updated_at_ms ?? left.updatedAtMs ?? left.updatedSeq ?? 0);
  const rightTime = Number(right.updated_at_ms ?? right.updatedAtMs ?? right.updatedSeq ?? 0);
  return rightTime - leftTime;
});

const collectProjectedSubagentPayloads = (
  source: unknown
): Record<string, unknown>[] => {
  const output: Record<string, unknown>[] = [];
  const append = (item: unknown): void => {
    if (!isPlainRecord(item)) return;
    if (hasProjectedSubagentIdentity(item)) {
      output.push(item);
    }
    resolveProjectedSubagentPayloadItems(item).forEach(append);
  };
  append(source);
  return output;
};

const resolveProjectedSubagentPayloadItems = (
  source: Record<string, unknown>
): unknown[] => {
  const nested = asRecord(source.data);
  return [
    source.item,
    source.selected_item,
    source.selectedItem,
    source.winner_item,
    source.winnerItem,
    ...(Array.isArray(source.items) ? source.items : []),
    ...(Array.isArray(source.selected_items) ? source.selected_items : []),
    ...(Array.isArray(source.selectedItems) ? source.selectedItems : []),
    ...(Array.isArray(source.settled_items) ? source.settled_items : []),
    ...(Array.isArray(source.settledItems) ? source.settledItems : []),
    ...(Object.keys(nested).length > 0 ? resolveProjectedSubagentPayloadItems(nested) : [])
  ];
};

const hasProjectedSubagentIdentity = (
  source: Record<string, unknown>
): boolean => Boolean(firstText(
  source.session_id,
  source.sessionId,
  source.target_session_id,
  source.targetSessionId,
  source.spawned_session_id,
  source.spawnedSessionId,
  source.run_id,
  source.runId,
  source.session_run_id,
  source.sessionRunId,
  source.dispatch_id,
  source.dispatchId
));

const resolveProjectedTimestampMs = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (value === null || value === undefined || value === '') continue;
    if (typeof value === 'number' && Number.isFinite(value)) {
      return value > 0 && value < 10_000_000_000 ? value * 1000 : value;
    }
    const text = String(value).trim();
    if (!text) continue;
    const numeric = Number(text);
    if (Number.isFinite(numeric)) {
      return numeric > 0 && numeric < 10_000_000_000 ? numeric * 1000 : numeric;
    }
    const parsed = Date.parse(text);
    if (Number.isFinite(parsed)) return parsed;
  }
  return null;
};

const normalizeOptionalRound = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const ensureProjectedWorkflowItems = (
  message: ChatRuntimeMessageProjection
): ChatRuntimeWorkflowItemProjection[] => {
  if (!Array.isArray(message.workflowItems)) {
    message.workflowItems = [];
  }
  return message.workflowItems;
};

const settleProjectedWorkflowItems = (
  message: ChatRuntimeMessageProjection,
  terminalStatus: 'completed' | 'failed'
): void => {
  if (Array.isArray(message.workflowItems)) {
    message.workflowItems.forEach((item) => {
      const status = normalizeText(item.status);
      if (ACTIVE_WORKFLOW_STATUSES.has(status)) {
        item.status = terminalStatus;
      }
    });
  }
  settleProjectedSubagents(message, terminalStatus);
};

const settleProjectedSubagents = (
  message: ChatRuntimeMessageProjection,
  terminalStatus: 'completed' | 'failed'
): void => {
  if (!Array.isArray(message.subagents)) return;
  message.subagents.forEach((item) => {
    const status = normalizeText(item.status ?? (asRecord(item.agent_state).status));
    if (!status || ACTIVE_SUBAGENT_STATUSES.has(status)) {
      item.status = terminalStatus;
      item.terminal = true;
      item.failed = terminalStatus === 'failed';
      item.canTerminate = false;
      const agentState = asRecord(item.agent_state);
      item.agent_state = {
        ...agentState,
        status: terminalStatus
      };
    }
  });
};

const findProjectedWorkflowItem = (
  items: ChatRuntimeWorkflowItemProjection[],
  itemId: string,
  toolCallId: string,
  commandSessionId: string,
  approvalId: string
): ChatRuntimeWorkflowItemProjection | null => {
  const exact = items.find((item) => normalizeId(item.id ?? item.itemId ?? item.item_id) === itemId);
  if (exact) return exact;
  const ref = toolCallId || commandSessionId || approvalId;
  if (!ref) return null;
  return items.find((item) => {
    const candidate = firstText(
      item.toolCallId,
      item.tool_call_id,
      item.callId,
      item.call_id,
      item.commandSessionId,
      item.command_session_id,
      item.approvalId,
      item.approval_id
    );
    if (candidate !== ref) return false;
    const eventType = normalizeText(item.eventType ?? item.event ?? item.event_type);
    return !TOOL_RESULT_EVENT_TYPES.has(eventType);
  }) || null;
};

const resolveProjectedWorkflowItemId = (
  event: NormalizedRuntimeEvent,
  eventType: string,
  toolCallId: string,
  commandSessionId: string,
  approvalId: string
): string => {
  const ref = eventType === 'approval_request' || eventType === 'approval_result'
    ? approvalId || toolCallId || commandSessionId
    : toolCallId || commandSessionId || approvalId;
  if (ref) return `runtime-workflow:${event.modelTurnId || event.messageId}:${ref}`;
  if (event.eventId) return `runtime-workflow:${event.modelTurnId || event.messageId}:event:${event.eventId}`;
  return `runtime-workflow:${event.modelTurnId || event.messageId}:${eventType}:${event.eventSeq ?? 'local'}`;
};

const resolveProjectedWorkflowEventType = (
  event: NormalizedRuntimeEvent,
  status: 'loading' | 'completed' | 'failed'
): string => {
  const sourceType = normalizeText(event.payload.source_event_type);
  if (sourceType === 'approval_request' || sourceType === 'approval_result' || sourceType === 'approval_resolved') {
    return status === 'loading' ? 'approval_request' : 'approval_result';
  }
  return status === 'loading' ? 'tool_call' : 'tool_result';
};

const resolveProjectedWorkflowTitle = (
  eventType: string,
  toolName: string
): string => {
  if (eventType === 'approval_request') return toolName ? `Approval required: ${toolName}` : 'Approval required';
  if (eventType === 'approval_result') return toolName ? `Approval result: ${toolName}` : 'Approval result';
  if (eventType === 'tool_result') return toolName ? `Tool result: ${toolName}` : 'Tool result';
  return toolName ? `Tool call: ${toolName}` : 'Tool call';
};

const resolveToolWorkflowRef = (
  event: NormalizedRuntimeEvent,
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): string => firstText(
  data.tool_call_id,
  data.toolCallId,
  data.call_id,
  data.callId,
  data.id,
  payload.tool_call_id,
  payload.toolCallId,
  payload.call_id,
  payload.callId,
  payload.tool_run_id,
  payload.toolRunId,
  event.eventId
);

const stringifyWorkflowDetail = (value: unknown): string => {
  if (typeof value === 'string') return value;
  if (!value || typeof value !== 'object') return String(value ?? '');
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

const ensureMessage = (
  session: ChatRuntimeSessionProjection,
  options: {
    id: string;
    role: ChatRuntimeMessageRole;
    createdSeq: number | null;
    createdAt: string;
    userTurnId: string;
    modelTurnId: string;
  }
): ChatRuntimeMessageProjection => {
  const id = options.id || `local-message:${session.sessionId}:${session.messages.length + 1}`;
  if (!session.messageById[id]) {
    const seq = options.createdSeq ?? nextLocalSeq(session);
    session.messageById[id] = {
      id,
      role: options.role,
      content: '',
      reasoning: '',
      status: options.role === 'assistant' ? 'placeholder' : 'final',
      createdAt: options.createdAt || new Date().toISOString(),
      createdSeq: seq,
      updatedSeq: seq,
      userTurnId: options.userTurnId,
      modelTurnId: options.modelTurnId,
      final: options.role !== 'assistant',
      failed: false,
      cancelled: false,
      ...(options.role === 'assistant' ? { workflowItems: [], subagents: [] } : {})
    };
  }
  const message = session.messageById[id];
  if (!message.userTurnId && options.userTurnId) {
    message.userTurnId = options.userTurnId;
  }
  if (!message.modelTurnId && options.modelTurnId) {
    message.modelTurnId = options.modelTurnId;
  }
  return message;
};

const deriveSessionRuntime = (session: ChatRuntimeSessionProjection): void => {
  if (session.connectionState === 'reconnecting') {
    setSessionBusy(session, 'reconnecting', 'reconnecting');
    return;
  }
  if (
    session.runtimeStatus === 'waiting_approval' ||
    session.runtimeStatus === 'waiting_user_input' ||
    session.runtimeStatus === 'finalizing'
  ) {
    session.busyReason = session.busyReason || resolveBusyReasonForStatus(session.runtimeStatus);
    return;
  }
  const activeMessage = Object.values(session.messageById)
    .filter((message) => message.role === 'assistant')
    .sort((left, right) => right.updatedSeq - left.updatedSeq)
    .find((message) => isActiveMessageStatus(message.status));
  if (activeMessage) {
    const reason = activeMessage.status === 'tooling'
      ? 'tool_running'
      : activeMessage.status === 'waiting_first_output' || activeMessage.status === 'placeholder'
        ? 'waiting_first_output'
        : 'streaming';
    setSessionBusy(session, 'running', reason);
    return;
  }
  if (isChatRuntimeBusyStatus(session.runtimeStatus) && session.busyReason) {
    return;
  }
  if (session.runtimeStatus === 'failed' || session.runtimeStatus === 'cancelled' || session.runtimeStatus === 'system_error') {
    session.busyReason = null;
    return;
  }
  if (session.runtimeStatus === 'completed') {
    session.runtimeStatus = 'idle';
    session.busyReason = null;
    return;
  }
  session.runtimeStatus = 'idle';
  session.busyReason = null;
};

const setSessionBusy = (
  session: ChatRuntimeSessionProjection,
  status: ChatSessionRuntimeStatus,
  reason: ChatRuntimeBusyReason
): void => {
  session.runtimeStatus = status;
  session.busyReason = reason;
};

const hasActiveMessage = (session: ChatRuntimeSessionProjection): boolean =>
  Object.values(session.messageById).some((message) => isActiveMessageStatus(message.status));

const isActiveMessageStatus = (status: ChatRuntimeMessageStatus): boolean =>
  status === 'placeholder' ||
  status === 'waiting_first_output' ||
  status === 'streaming' ||
  status === 'tooling';

const shouldIgnoreEventForCancelledTurn = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): boolean => {
  if (!event.userTurnId || isTerminalSafeEventType(event.type)) return false;
  const userTurn = session.userTurnById[event.userTurnId];
  if (userTurn?.status !== 'cancelled') return false;
  return isAssistantRuntimeEventType(event.type);
};

const isAssistantRuntimeEventType = (eventType: string): boolean =>
  eventType === 'assistant_message_created' ||
  eventType === 'assistant_delta' ||
  eventType === 'assistant_reasoning_delta' ||
  eventType === 'assistant_final' ||
  eventType === 'tool_call_started' ||
  eventType === 'tool_call_delta' ||
  eventType === 'tool_call_completed' ||
  eventType === 'tool_call_failed' ||
  eventType === 'workflow_event';

const isTerminalSafeEventType = (eventType: string): boolean =>
  eventType === 'turn_cancelled' ||
  eventType === 'turn_failed' ||
  eventType === 'turn_completed' ||
  eventType === 'session_idle' ||
  eventType === 'session_runtime' ||
  eventType === 'session_snapshot' ||
  eventType === 'legacy_messages_reconciled';

const validateSessionInvariants = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  Object.values(session.modelTurnById).forEach((turn) => {
    const finalMessages = turn.messageIds
      .map((id) => session.messageById[id])
      .filter((message) => message?.final);
    if (finalMessages.length > 1) {
      pushViolation(session, {
        code: 'multiple_final_assistants',
        message: 'a model turn produced more than one final assistant message',
        eventSeq: event.eventSeq,
        eventType: event.type,
        modelTurnId: turn.id
      });
    }
  });
  Object.values(session.modelTurnById).forEach((turn) => {
    if (!turn.userTurnId) return;
    const userTurnIndex = session.userTurns.indexOf(turn.userTurnId);
    const modelTurnIndex = session.modelTurns.indexOf(turn.id);
    if (userTurnIndex < 0 || modelTurnIndex < 0) return;
    const userTurn = session.userTurnById[turn.userTurnId];
    const hasUserMessage = userTurn?.messageIds?.some((id) => session.messageById[id]?.role === 'user');
    if (!hasUserMessage) return;
    const firstUserSeq = Math.min(...userTurn.messageIds.map((id) => session.messageById[id]?.createdSeq || 0));
    turn.messageIds.forEach((messageId) => {
      const assistant = session.messageById[messageId];
      if (!assistant || assistant.role !== 'assistant') return;
      if (assistant.createdSeq < firstUserSeq && event.type !== 'user_message_created') {
        return;
      }
    });
  });
  if (!session.busyReason && isChatRuntimeBusyStatus(session.runtimeStatus)) {
    pushViolation(session, {
      code: 'busy_without_reason',
      message: 'busy runtime status must expose a busy reason',
      eventSeq: event.eventSeq,
      eventType: event.type
    });
  }
};

const appendDebugEvent = (
  projection: ChatRuntimeProjection,
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  beforeSummary: string,
  afterSummary: string
): void => {
  projection.debugEvents.push({
    receivedAt: Date.now(),
    sessionId: session.sessionId,
    eventType: event.type,
    eventSeq: event.eventSeq,
    eventId: event.eventId,
    beforeSummary,
    afterSummary,
    violationCount: session.invariantViolations.length
  });
  if (projection.debugEvents.length > DEBUG_EVENT_LIMIT) {
    projection.debugEvents.splice(0, projection.debugEvents.length - DEBUG_EVENT_LIMIT);
  }
};

const quarantineEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  reason: string
): void => {
  session.quarantinedEvents.push({
    reason,
    eventType: event.type,
    eventSeq: event.eventSeq,
    eventId: event.eventId,
    receivedAt: Date.now(),
    event
  });
  if (session.quarantinedEvents.length > QUARANTINE_LIMIT) {
    session.quarantinedEvents.splice(0, session.quarantinedEvents.length - QUARANTINE_LIMIT);
  }
};

const pushViolation = (
  session: ChatRuntimeSessionProjection,
  violation: ChatRuntimeViolation
): void => {
  session.invariantViolations.push(violation);
  if (session.invariantViolations.length > VIOLATION_LIMIT) {
    session.invariantViolations.splice(0, session.invariantViolations.length - VIOLATION_LIMIT);
  }
};

const summarizeSession = (session: ChatRuntimeSessionProjection): string =>
  JSON.stringify({
    seq: session.appliedSeq,
    status: session.runtimeStatus,
    busy: session.busyReason,
    messages: session.messages.length,
    quarantine: session.quarantinedEvents.length,
    violations: session.invariantViolations.length
  });

const resolveLegacyMessageId = (message: ChatRuntimeRawMessage, index: number): string => {
  const explicit = normalizeId(message.message_id ?? message.messageId ?? message.id);
  if (explicit) return explicit;
  const eventId = normalizeSeq(message.stream_event_id ?? message.streamEventId);
  if (eventId !== null) return `legacy-event:${eventId}`;
  const role = normalizeRole(message.role);
  const createdAt = normalizeCreatedAtKey(message.created_at ?? message.createdAt);
  const contentHash = hashText(String(message.content ?? ''));
  return `legacy:${role}:${createdAt}:${index}:${contentHash}`;
};

const resolveNearestLegacyUserTurnId = (
  session: ChatRuntimeSessionProjection,
  index: number
): LegacyUserTurnResolution => {
  const lastUserTurn = [...session.userTurns].reverse().find((turnId) => {
    const turn = session.userTurnById[turnId];
    return turn?.messageIds?.length && turn.createdSeq <= session.snapshotSeq + index + 1;
  });
  return {
    userTurnId: lastUserTurn || `legacy-user-turn:orphan:${index}`,
    strength: 'weak',
    source: 'nearest_existing'
  };
};

const resolveLegacyMessageStatus = (message: ChatRuntimeRawMessage): ChatRuntimeMessageStatus => {
  if (normalizeRole(message.role) !== 'assistant') return 'final';
  const normalizedStatus = normalizeText(message.status);
  if (normalizeFlag(message.failed) || normalizedStatus === 'failed') return 'failed';
  if (
    normalizeFlag(message.cancelled) ||
    normalizedStatus === 'cancelled' ||
    normalizedStatus === 'canceled' ||
    isLegacyCancelledMessage(message)
  ) {
    return 'cancelled';
  }
  const hasWaitingTimestamp = Number(
    message.waiting_updated_at_ms ??
      message.waitingUpdatedAtMs ??
      (asRecord(message.stats).interaction_start_ms) ??
      0
  ) > 0;
  const hasFirstOutput = Number(
    message.waiting_first_output_at_ms ??
      message.waitingFirstOutputAtMs ??
      message.waiting_phase_first_output_at_ms ??
      message.waitingPhaseFirstOutputAtMs ??
      0
  ) > 0;
  const hasVisibleOutput = Boolean(String(message.content ?? '').trim() || String(message.reasoning ?? '').trim());
  if (
    normalizeFlag(message.workflowStreaming) ||
    hasActiveWorkflowItems(message.workflowItems) ||
    hasActiveSubagentItems(message.subagents)
  ) {
    return 'tooling';
  }
  if (normalizeFlag(message.stream_incomplete) || normalizeFlag(message.reasoningStreaming)) {
    return hasVisibleOutput ? 'streaming' : 'waiting_first_output';
  }
  if (hasWaitingTimestamp && !hasFirstOutput && !hasVisibleOutput) {
    return 'waiting_first_output';
  }
  return 'final';
};

const isLegacyCancelledMessage = (message: ChatRuntimeRawMessage): boolean => {
  const stopReason = normalizeText(message.stop_reason ?? message.stopReason);
  if (
    stopReason === 'user_stop' ||
    stopReason === 'cancelled' ||
    stopReason === 'canceled' ||
    stopReason === 'aborted'
  ) {
    return true;
  }
  const meta = asRecord(message.meta);
  const metaType = normalizeText(meta.type);
  return metaType === 'session_cancelled' || normalizeFlag(meta.cancelled);
};

const resolveLegacyBusyReason = (messages: ChatRuntimeRawMessage[]): ChatRuntimeBusyReason => {
  const active = [...messages]
    .reverse()
    .find((message) => normalizeRole(message?.role) === 'assistant' && isActiveMessageStatus(resolveLegacyMessageStatus(message)));
  if (!active) return 'streaming';
  const status = resolveLegacyMessageStatus(active);
  if (status === 'tooling') return 'tool_running';
  if (status === 'waiting_first_output' || status === 'placeholder') return 'waiting_first_output';
  return 'streaming';
};

const hasActiveLegacyRuntime = (messages: ChatRuntimeRawMessage[]): boolean =>
  Array.isArray(messages) &&
  messages.some((message) =>
    normalizeRole(message?.role) === 'assistant' &&
    isActiveMessageStatus(resolveLegacyMessageStatus(message))
  );

const hasActiveWorkflowItems = (workflowItems: unknown): boolean => {
  if (!Array.isArray(workflowItems)) return false;
  return workflowItems.some((item) => {
    if (!item || typeof item !== 'object') return false;
    return ACTIVE_WORKFLOW_STATUSES.has(normalizeText((item as Record<string, unknown>).status));
  });
};

const hasActiveSubagentItems = (subagents: unknown): boolean => {
  if (!Array.isArray(subagents)) return false;
  return subagents.some((item) => {
    if (!item || typeof item !== 'object') return false;
    const record = item as Record<string, unknown>;
    const status = normalizeText(record.status ?? asRecord(record.agent_state).status);
    if (ACTIVE_SUBAGENT_STATUSES.has(status)) return true;
    if (SUCCESS_WORKFLOW_STATUSES.has(status) || FAILED_WORKFLOW_STATUSES.has(status)) return false;
    if (normalizeFlag(record.terminal) || normalizeFlag(record.failed)) return false;
    if (normalizeFlag(record.reply_pending ?? record.replyPending)) return true;
    return Boolean(firstText(record.session_id, record.sessionId, record.run_id, record.runId));
  });
};

const resolveBusyReasonForStatus = (status: ChatSessionRuntimeStatus): ChatRuntimeBusyReason => {
  if (status === 'queued') return 'queued';
  if (status === 'waiting_approval') return 'waiting_approval';
  if (status === 'waiting_user_input') return 'waiting_user_input';
  if (status === 'finalizing') return 'finalizing';
  if (status === 'reconnecting') return 'reconnecting';
  return 'streaming';
};

const normalizeConnectionState = (value: unknown): ChatRuntimeConnectionState => {
  const normalized = normalizeText(value);
  if (normalized === 'offline') return 'offline';
  if (normalized === 'reconnecting' || normalized === 'connecting') return 'reconnecting';
  return 'connected';
};

const normalizeRole = (value: unknown): ChatRuntimeMessageRole => {
  const normalized = normalizeText(value);
  if (normalized === 'user') return 'user';
  if (normalized === 'assistant') return 'assistant';
  return 'system';
};

const normalizeSeq = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizeId = (value: unknown): string => String(value ?? '').trim();

const normalizeText = (value: unknown): string => String(value ?? '').trim().toLowerCase();

const normalizeCreatedAt = (value: unknown): string => {
  const text = String(value ?? '').trim();
  if (!text) return new Date().toISOString();
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? new Date(parsed).toISOString() : text;
};

const normalizeCreatedAtMs = (value: unknown): number | null => {
  const text = String(value ?? '').trim();
  if (!text) return null;
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeCreatedAtKey = (value: unknown): string => {
  const text = String(value ?? '').trim();
  if (!text) return 'missing-created-at';
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? new Date(parsed).toISOString() : text;
};

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const normalized = value.trim().toLowerCase();
    if (!normalized) return false;
    return normalized !== 'false' && normalized !== '0' && normalized !== 'no';
  }
  return Boolean(value);
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};

const isPlainRecord = (value: unknown): value is Record<string, unknown> =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

const addUnique = (target: string[], value: string): void => {
  if (!value || target.includes(value)) return;
  target.push(value);
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};

const nextLocalSeq = (session: ChatRuntimeSessionProjection): number => {
  session.localSeq += 1;
  return session.appliedSeq + session.localSeq;
};

const hashText = (value: string): string => {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash.toString(36);
};
