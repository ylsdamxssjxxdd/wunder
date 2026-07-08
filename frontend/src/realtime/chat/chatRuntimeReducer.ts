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
const SEQUENTIAL_GAP_REPLAY_DEADLINE_MS = 800;

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
const PROJECTED_TOOL_RESULT_EVENT_TYPES = new Set(['tool_result']);
const isTerminalWorkflowStatus = (status: string): boolean =>
  Boolean(status) && !ACTIVE_WORKFLOW_STATUSES.has(status);
const WORKFLOW_CONTEXT_SNAPSHOT_KEY = '__workflowContextSnapshot';
const EPHEMERAL_TEXT_BEFORE_TOOL_KEY = '__ephemeralTextBeforeTool';
const EPHEMERAL_REASONING_BEFORE_TOOL_KEY = '__ephemeralReasoningBeforeTool';
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
const COMMAND_SESSION_WORKFLOW_EVENT_TYPES = new Set([
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary',
  'command_session_delta'
]);
const COMMAND_SESSION_TERMINAL_EVENT_TYPES = new Set([
  'command_session_exit',
  'command_session_summary'
]);
const QUESTION_PANEL_WORKFLOW_EVENT_TYPES = new Set(['question_panel']);
const PLAN_WORKFLOW_EVENT_TYPES = new Set(['plan_update']);
const COMPACTION_PROGRESS_STAGES = new Set([
  'compacting',
  'context_overflow_recovery',
  'context_guard'
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

const isCommandSessionRuntimeEvent = (event: { payload?: Record<string, unknown> | null }): boolean => {
  const payload = asRecord(event.payload);
  const sourceType = normalizeText(payload.source_event_type);
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) return true;
  const data = asRecord(payload.data);
  return Boolean(
    firstText(
      data.command_session_id,
      data.commandSessionId,
      payload.command_session_id,
      payload.commandSessionId
    )
  );
};

const CONTENT_ONLY_EVENT_TYPES = new Set([
  'assistant_delta',
  'assistant_reasoning_delta'
]);

const isContentOnlyRuntimeEvent = (event: { type?: string }): boolean =>
  CONTENT_ONLY_EVENT_TYPES.has(normalizeText(event.type));

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
  lastAppliedEventId: 0,
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
  const commandSessionRuntimeEvent = isCommandSessionRuntimeEvent(event);

  if (duplicateEventId) {
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: true,
    quarantined: false,
    sessionId: session.sessionId,
    messageId: event.messageId,
    eventSeq: event.eventSeq,
    reason: 'duplicate_event_id'
  };
  }

  if (
    !commandSessionRuntimeEvent &&
    event.eventSeq !== null &&
    event.eventSeq <= session.appliedSeq
  ) {
    removePendingSequentialEvent(session, event);
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: true,
    quarantined: false,
    sessionId: session.sessionId,
    messageId: event.messageId,
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
      messageId: event.messageId,
      eventSeq: event.eventSeq,
      reason: quarantineReason
    };
    }
  }

  const expiredGapReason = expireSequentialGapIfNeeded(session, event);
  const pendingReason = expiredGapReason ? '' : queueSequentialEventIfNeeded(session, event);
  if (pendingReason) {
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: false,
      ignored: pendingReason === 'duplicate_pending_event_id',
      quarantined: false,
      pending: pendingReason !== 'duplicate_pending_event_id',
      sessionId: session.sessionId,
      messageId: event.messageId,
      eventSeq: event.eventSeq,
      reason: pendingReason
    };
  }

  const hardGapReason = shouldApplySequentialGapImmediately(session, event) ? 'event_seq_gap' : '';
  const contentOnlyMessageId = resolveContentOnlyRuntimeMessageId(session, event);
  if (
    contentOnlyMessageId &&
    !expiredGapReason &&
    !hardGapReason &&
    session.pendingSequentialEvents.length === 0 &&
    !shouldIgnoreEventForCancelledTurn(session, event)
  ) {
    applyContentOnlyRuntimeEvent(session, event, contentOnlyMessageId);
    appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
    return {
      applied: true,
      ignored: false,
      quarantined: false,
      contentOnly: true,
      drained: 0,
      sessionId: session.sessionId,
      messageId: contentOnlyMessageId,
      eventSeq: event.eventSeq
    };
  }
  applyNormalizedRuntimeEvent(session, event);
  const drained = drainPendingSequentialEvents(session);
  appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
  return {
    applied: true,
    ignored: false,
    quarantined: false,
    contentOnly: drained === 0 && Boolean(contentOnlyMessageId),
    drained,
    sessionId: session.sessionId,
    messageId: contentOnlyMessageId || event.messageId,
    eventSeq: event.eventSeq,
    reason: expiredGapReason || hardGapReason || undefined
  };
};

const resolveContentOnlyRuntimeMessageId = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): string => {
  if (!isContentOnlyRuntimeEvent(event)) return '';
  const modelTurn = session.modelTurnById[event.modelTurnId];
  const messageId = modelTurn
    ? resolveAssistantMessageIdForModelTurn(session, modelTurn, event.messageId)
    : event.messageId;
  const message = messageId ? session.messageById[messageId] : null;
  if (!message || message.role !== 'assistant') return '';
  if (message.status !== 'streaming') return '';
  if (event.type === 'assistant_delta') {
    if ((event.reasoningDelta || event.reasoning) && !message.reasoning) return '';
    return message.content ? message.id : '';
  }
  if (event.type === 'assistant_reasoning_delta') {
    return message.reasoning ? message.id : '';
  }
  return '';
};

const applyContentOnlyRuntimeEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  messageId: string
): void => {
  if (event.eventId) {
    session.eventIdIndex[event.eventId] = true;
  }
  markAppliedStreamEventId(session, event.eventId);
  if (event.agentId) {
    session.agentId = event.agentId;
  }
  const modelTurn = session.modelTurnById[event.modelTurnId];
  const message = session.messageById[messageId];
  if (modelTurn) {
    modelTurn.status = 'streaming';
  }
  if (message?.role === 'assistant') {
    if (event.type === 'assistant_delta') {
      clearEphemeralAssistantTextBeforeNextOutput(message);
      message.content += event.delta || event.content;
      if (event.reasoningDelta || event.reasoning) {
        message.reasoning += event.reasoningDelta || event.reasoning;
      }
    } else if (event.type === 'assistant_reasoning_delta') {
      message.reasoning += event.reasoningDelta || event.reasoning;
    }
    if (isPlainRecord(message.display)) {
      clearProjectedRetryDisplay(message.display);
    }
    message.status = 'streaming';
    message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  }
  if (
    !isCommandSessionRuntimeEvent(event) &&
    event.eventSeq !== null &&
    event.eventSeq > session.appliedSeq
  ) {
    session.appliedSeq = event.eventSeq;
  }
  setSessionBusy(session, 'running', 'streaming');
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
  bindEventToOptimisticUserTurn(session, event);
  if (shouldIgnoreEventForCancelledTurn(session, event)) {
    if (event.eventId) {
      session.eventIdIndex[event.eventId] = true;
    }
    markAppliedStreamEventId(session, event.eventId);
    if (
      !isCommandSessionRuntimeEvent(event) &&
      event.eventSeq !== null &&
      event.eventSeq > session.appliedSeq
    ) {
      session.appliedSeq = event.eventSeq;
    }
    return;
  }
  if (event.eventId) {
    session.eventIdIndex[event.eventId] = true;
  }
  markAppliedStreamEventId(session, event.eventId);
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
    case 'assistant_output_snapshot':
      applyAssistantOutputSnapshot(session, event);
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
    case 'usage_stats':
      applyUsageStats(session, event);
      break;
    case 'queue_status':
      applyQueueStatus(session, event);
      break;
    case 'turn_completed':
      applyTurnTerminal(session, event, 'completed');
      break;
    case 'turn_failed':
      applyTurnFailed(session, event);
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
  if (isServerAuthoritativeRuntimeEvent(event)) {
    pruneLocalTerminalArtifactsForAuthoritativeEvent(session, event);
  }

  if (
    !isCommandSessionRuntimeEvent(event) &&
    event.eventSeq !== null &&
    event.eventSeq > session.appliedSeq
  ) {
    session.appliedSeq = event.eventSeq;
  }
  deriveSessionRuntime(session);
  validateSessionInvariants(session, event);
};

const ensureRuntimeSessionCollections = (session: ChatRuntimeSessionProjection): void => {
  if (!Array.isArray(session.pendingSequentialEvents)) {
    session.pendingSequentialEvents = [];
  }
  if (!Number.isFinite(session.lastAppliedEventId)) {
    session.lastAppliedEventId = 0;
  }
};

const markAppliedStreamEventId = (
  session: ChatRuntimeSessionProjection,
  eventId: unknown
): void => {
  const numericEventId = normalizePureNumericEventId(eventId);
  if (numericEventId === null || numericEventId <= session.lastAppliedEventId) return;
  session.lastAppliedEventId = numericEventId;
};

const normalizePureNumericEventId = (value: unknown): number | null => {
  const text = String(value ?? '').trim();
  if (!/^\d+$/.test(text)) return null;
  const parsed = Number.parseInt(text, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const shouldBufferSequentialEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): boolean => {
  if (!event.strict || event.eventSeq === null) return false;
  if (isCommandSessionRuntimeEvent(event)) return false;
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
  const receivedAt = Date.now();
  session.pendingSequentialEvents.push({
    eventSeq: event.eventSeq,
    eventId: event.eventId,
    eventType: event.type,
    receivedAt,
    deadlineAt: receivedAt + SEQUENTIAL_GAP_REPLAY_DEADLINE_MS,
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

const expireSequentialGapIfNeeded = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): string => {
  ensureRuntimeSessionCollections(session);
  if (!event.strict || event.eventSeq === null || session.pendingSequentialEvents.length === 0) {
    return '';
  }
  const now = Date.now();
  const expired = session.pendingSequentialEvents.find((pending) => pending.deadlineAt <= now);
  if (!expired) return '';
  const expectedSeq = session.appliedSeq + 1;
  session.pendingSequentialEvents.splice(0);
  session.syncRequired = true;
  pushViolation(session, {
    code: 'event_seq_gap_timeout',
    message: 'pending sequential runtime events reached the replay deadline',
    eventSeq: expired.eventSeq,
    eventType: expired.eventType
  });
  pushViolation(session, {
    code: 'event_seq_gap',
    message: `runtime event sequence ${expectedSeq} did not arrive before replay deadline`,
    eventSeq: event.eventSeq,
    eventType: event.type
  });
  return 'event_seq_gap_timeout';
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
    strict: event.strict === false ? false : event.strict === true || source === 'ws' || source === 'test',
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

const bindEventToOptimisticUserTurn = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  if (!event.userTurnId) return;
  const clientMessageId = resolveEventClientMessageId(event);
  if (!clientMessageId) return;
  const clientMessage = session.messageById[clientMessageId];
  if (!clientMessage || clientMessage.role !== 'user') return;
  const localUserTurn = session.userTurnById[clientMessage.userTurnId];
  if (!localUserTurn) return;
  const incomingUserTurn = session.userTurnById[event.userTurnId];
  if (incomingUserTurn && incomingUserTurn.id !== localUserTurn.id) {
    mergeUserTurnInto(session, incomingUserTurn.id, localUserTurn.id);
  }
  event.userTurnId = localUserTurn.id;
};

const resolveEventClientMessageId = (event: NormalizedRuntimeEvent): string =>
  firstText(
    event.payload.client_message_id,
    event.payload.clientMessageId,
    asRecord(event.payload.data).client_message_id,
    asRecord(event.payload.data).clientMessageId
  );

const mergeUserTurnInto = (
  session: ChatRuntimeSessionProjection,
  sourceTurnId: string,
  targetTurnId: string
): void => {
  if (!sourceTurnId || !targetTurnId || sourceTurnId === targetTurnId) return;
  const sourceTurn = session.userTurnById[sourceTurnId];
  const targetTurn = session.userTurnById[targetTurnId] || ensureUserTurn(session, targetTurnId, sourceTurn?.createdSeq ?? null);
  if (!sourceTurn) return;
  sourceTurn.messageIds.forEach((messageId) => {
    const message = session.messageById[messageId];
    if (!message) return;
    message.userTurnId = targetTurn.id;
    addUnique(targetTurn.messageIds, messageId);
  });
  sourceTurn.modelTurnIds.forEach((modelTurnId) => {
    const modelTurn = session.modelTurnById[modelTurnId];
    if (!modelTurn) return;
    modelTurn.userTurnId = targetTurn.id;
    modelTurn.messageIds.forEach((messageId) => {
      const message = session.messageById[messageId];
      if (message) {
        message.userTurnId = targetTurn.id;
      }
    });
    addUnique(targetTurn.modelTurnIds, modelTurnId);
  });
  targetTurn.createdSeq = Math.min(targetTurn.createdSeq, sourceTurn.createdSeq);
  targetTurn.status = mergeUserTurnStatus(targetTurn.status, sourceTurn.status);
  delete session.userTurnById[sourceTurnId];
  session.userTurns = session.userTurns.filter((turnId) => turnId !== sourceTurnId);
};

const mergeUserTurnStatus = (
  current: ChatRuntimeUserTurnProjection['status'],
  incoming: ChatRuntimeUserTurnProjection['status']
): ChatRuntimeUserTurnProjection['status'] => {
  if (current === incoming) return current;
  if (current === 'completed' || incoming === 'completed') return 'completed';
  if (current === 'failed' || incoming === 'failed') return 'failed';
  if (current === 'cancelled' || incoming === 'cancelled') return 'cancelled';
  if (current === 'waiting_user_input' || incoming === 'waiting_user_input') return 'waiting_user_input';
  if (current === 'model_running' || incoming === 'model_running') return 'model_running';
  if (current === 'dispatched' || incoming === 'dispatched') return 'dispatched';
  if (current === 'accepted' || incoming === 'accepted') return 'accepted';
  return 'created';
};

const isServerAuthoritativeRuntimeEvent = (event: NormalizedRuntimeEvent): boolean =>
  event.source !== 'local' &&
  event.source !== 'legacy' &&
  event.source !== 'snapshot';

const pruneLocalTerminalArtifactsForAuthoritativeEvent = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  if (!event.userTurnId || !isAssistantRuntimeEventType(event.type) && !isTerminalSafeEventType(event.type)) {
    return;
  }
  const canonicalTurn = session.modelTurnById[event.modelTurnId];
  if (!canonicalTurn) return;
  if (!canonicalTurn.messageIds.some((messageId) => session.messageById[messageId]?.role === 'assistant')) {
    return;
  }
  const staleTurnIds = session.userTurnById[event.userTurnId]?.modelTurnIds.filter((modelTurnId) => {
    if (modelTurnId === canonicalTurn.id) return false;
    const modelTurn = session.modelTurnById[modelTurnId];
    if (!modelTurn) return false;
    if (modelTurn.userTurnId !== event.userTurnId) return false;
    if (modelTurn.status !== 'failed' && modelTurn.status !== 'cancelled') return false;
    return modelTurn.messageIds.some((messageId) => isLocalTerminalArtifactMessage(session.messageById[messageId]));
  }) || [];
  staleTurnIds.forEach((modelTurnId) => removeModelTurnProjection(session, modelTurnId));
};

const isLocalTerminalArtifactMessage = (
  message: ChatRuntimeMessageProjection | null | undefined
): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (message.status !== 'failed' && message.status !== 'cancelled') return false;
  if (!message.id.startsWith('local-assistant:')) return false;
  return message.updatedSeq === message.createdSeq || message.updatedSeq <= 0;
};

const removeModelTurnProjection = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string
): void => {
  const modelTurn = session.modelTurnById[modelTurnId];
  if (!modelTurn) return;
  const staleMessageIds = new Set(modelTurn.messageIds);
  staleMessageIds.forEach((messageId) => {
    delete session.messageById[messageId];
  });
  session.messages = session.messages.filter((messageId) => !staleMessageIds.has(messageId));
  const userTurn = session.userTurnById[modelTurn.userTurnId];
  if (userTurn) {
    userTurn.modelTurnIds = userTurn.modelTurnIds.filter((id) => id !== modelTurnId);
    userTurn.messageIds = userTurn.messageIds.filter((messageId) => !staleMessageIds.has(messageId));
  }
  delete session.modelTurnById[modelTurnId];
  session.modelTurns = session.modelTurns.filter((id) => id !== modelTurnId);
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
      event.type === 'workflow_event' ||
      event.type === 'usage_stats'
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
  if (Array.isArray(event.payload.attachments)) {
    ensureMessageDisplayProjection(message).attachments = event.payload.attachments
      .filter((item): item is Record<string, unknown> => isPlainRecord(item))
      .map((item) => cloneProjectedDisplayValue(item));
  }
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
    clearEphemeralAssistantTextBeforeNextOutput(message);
    message.content += event.delta || event.content;
  } else {
    message.reasoning += event.reasoningDelta || event.reasoning;
  }
  if (isPlainRecord(message.display)) {
    clearProjectedRetryDisplay(message.display);
  }
  message.status = 'streaming';
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  setSessionBusy(session, 'running', 'streaming');
};

const applyAssistantOutputSnapshot = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  modelTurn.status = 'streaming';
  const message = ensureAssistantMessageForModelTurn(session, event, 'streaming');
  if (event.content) {
    message.content = mergeRuntimeSnapshotText(message.content, event.content, event);
  }
  if (event.reasoning) {
    message.reasoning = mergeRuntimeSnapshotText(message.reasoning, event.reasoning, event);
  }
  if (isPlainRecord(message.display)) {
    clearProjectedRetryDisplay(message.display);
  }
  applyProjectedUsageStatsDisplay(message, event, 'token_usage');
  updateWorkflowContextSnapshot(message, event, modelTurn);
  message.status = 'streaming';
  message.final = false;
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
  if (normalizeText(event.payload.source_event_type) === 'final') {
    clearEphemeralAssistantTextBeforeNextOutput(message);
  }
  if (event.content) {
    message.content = mergeRuntimeSnapshotText(message.content, event.content, event);
  }
  if (event.reasoning) {
    message.reasoning = mergeRuntimeSnapshotText(message.reasoning, event.reasoning, event);
  }
  if (isPlainRecord(message.display)) {
    clearProjectedRetryDisplay(message.display);
  }
  applyProjectedUsageStatsDisplay(message, event, 'round_usage');
  updateWorkflowContextSnapshot(message, event, modelTurn);
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
  if (!completed) {
    clearAssistantTextAtToolBoundary(message);
  }
  upsertToolWorkflowItem(message, event, completed ? 'completed' : 'loading', modelTurn);
  syncProjectedToolCallStats(message);
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
  upsertToolWorkflowItem(message, event, 'failed', modelTurn);
  syncProjectedToolCallStats(message);
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
  if (shouldClearAssistantTextAtWorkflowBoundary(sourceType, status)) {
    clearAssistantTextAtToolBoundary(message);
  }
  upsertProjectedWorkflowEventItem(message, event, sourceType, status, modelTurn);
  applyProjectedWorkflowDisplay(message, event, sourceType, status);
  if (SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    upsertProjectedSubagents(message, event, sourceType, status);
  }
  syncProjectedToolCallStats(message);
  if (sourceType === 'slow_client') {
    settleProjectedRetryWorkflowItems(message);
  }
  message.status = sourceType === 'slow_client'
    ? 'final'
    : status === 'failed'
      ? 'failed'
      : status === 'completed'
        ? 'streaming'
        : 'tooling';
  message.failed = message.failed || (status === 'failed' && sourceType !== 'slow_client');
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = sourceType === 'slow_client'
    ? 'finalizing'
    : status === 'failed'
      ? 'failed'
      : status === 'completed'
        ? 'streaming'
        : 'tool_running';
  if (sourceType === 'slow_client') {
    session.runtimeStatus = 'idle';
    session.busyReason = null;
  } else if (status === 'failed') {
    session.runtimeStatus = 'failed';
    session.busyReason = null;
  } else if (status !== 'completed') {
    setSessionBusy(session, 'running', 'tool_running');
  }
};

const applyUsageStats = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const sourceType = normalizeText(event.payload.source_event_type);
  const modelTurn = event.modelTurnId || event.userTurnId
    ? ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq)
    : null;
  const existing = resolveUsageStatsTargetMessage(session, event);
  if (!existing) {
    if (modelTurn) {
      updateWorkflowContextSnapshotRecord(modelTurn as unknown as Record<string, unknown>, event);
    }
    return;
  }
  const message = existing;
  applyProjectedUsageStatsDisplay(message, event, sourceType);
  updateWorkflowContextSnapshot(message, event, modelTurn);
  if (modelTurn) {
    updateWorkflowContextSnapshotRecord(modelTurn as unknown as Record<string, unknown>, event);
  }
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
};

const applyTurnTerminal = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  terminal: 'completed' | 'failed' | 'cancelled'
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  if (
    terminal !== 'completed' &&
    !modelTurn.messageIds.some((messageId) => session.messageById[messageId]?.role === 'assistant') &&
    (event.content || event.reasoning || event.messageId)
  ) {
    ensureAssistantMessageForModelTurn(session, event, terminal);
  }
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
      if (event.content && !message.content) {
        message.content = event.content;
      }
      if (event.reasoning && !message.reasoning) {
        message.reasoning = event.reasoning;
      }
      message.status = 'failed';
      message.failed = true;
      settleProjectedWorkflowItems(message, 'failed');
    } else if (terminal === 'cancelled') {
      if (event.content && !message.content) {
        message.content = event.content;
      }
      if (event.reasoning && !message.reasoning) {
        message.reasoning = event.reasoning;
      }
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

const applyTurnFailed = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const hasAssistantMessage = modelTurn.messageIds.some((messageId) =>
    session.messageById[messageId]?.role === 'assistant'
  );
  if (event.content || event.reasoning) {
    const message = ensureAssistantMessageForModelTurn(session, event, 'failed');
    if (event.content && !message.content) {
      message.content = event.content;
    }
    if (event.reasoning && !message.reasoning) {
      message.reasoning = event.reasoning;
    }
    applyTurnTerminal(session, event, 'failed');
    return;
  }
  if (hasAssistantMessage) {
    applyTurnTerminal(session, event, 'failed');
    return;
  }
  modelTurn.status = 'failed';
  const userTurn = session.userTurnById[modelTurn.userTurnId];
  if (userTurn) {
    userTurn.status = 'failed';
  }
  session.runtimeStatus = 'failed';
  session.busyReason = null;
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

const applyQueueStatus = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(session, event, 'tooling');
  upsertProjectedQueueWorkflowItem(message, event);
  message.status = 'tooling';
  message.updatedSeq = event.eventSeq ?? message.updatedSeq;
  modelTurn.status = 'tool_running';
  setSessionBusy(session, 'queued', 'queued');
};

const applySessionSnapshot = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const snapshotSeq = event.snapshotSeq ?? event.eventSeq ?? nextLocalSeq(session);
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
    if (!isSnapshotRuntimeActive(event, messages)) {
      settleLegacyActiveMessages(session);
      session.runtimeStatus = 'idle';
      session.busyReason = null;
      deriveSessionRuntime(session);
    }
    return;
  }
  const running = isSnapshotRuntimeActive(event, messages);
  mergeLegacyMessages(session, messages, {
    snapshotSeq,
    replaceExistingAtOrBelowSeq: true,
    authoritative:
      normalizeFlag(event.authoritative ?? event.payload.authoritative ?? event.prune_missing ?? event.payload.prune_missing) &&
      !running
  });
  if (running) {
    setSessionBusy(session, 'running', resolveLegacyBusyReason(messages));
  } else {
    settleLegacyActiveMessages(session);
    session.runtimeStatus = 'idle';
    session.busyReason = null;
    deriveSessionRuntime(session);
  }
};

const isSnapshotRuntimeActive = (
  event: NormalizedRuntimeEvent,
  messages: ChatRuntimeRawMessage[]
): boolean => {
  const loading = normalizeFlag(event.loading ?? event.payload.loading);
  const running = normalizeFlag(event.running ?? event.payload.running);
  const status = normalizeChatRuntimeStatus(event.payload.runtime_status ?? event.payload.status);
  return loading || running || isChatRuntimeBusyStatus(status) || hasActiveLegacyRuntime(messages);
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
      modelTurn.status = 'completed';
    } else if (status === 'failed') {
      modelTurn.status = 'failed';
    } else if (status === 'cancelled') {
      modelTurn.status = 'cancelled';
    } else if (status === 'tooling') {
      modelTurn.status = 'tool_running';
    } else if (status === 'streaming') {
      modelTurn.status = 'streaming';
    } else if (status === 'waiting_first_output' || status === 'placeholder') {
      modelTurn.status = 'waiting_first_output';
    }
    const userTurn = session.userTurnById[modelTurn.userTurnId];
    if (userTurn && (status === 'final' || status === 'failed' || status === 'cancelled')) {
      userTurn.status = status === 'final' ? 'completed' : status;
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
  const canonicalAssistantCountsByUserTurn = plans.reduce<Map<string, number>>((acc, plan) => {
    if (plan.role !== 'assistant') return acc;
    acc.set(plan.userTurnId, (acc.get(plan.userTurnId) || 0) + 1);
    return acc;
  }, new Map());

  plans.forEach((plan) => {
    const userTurn = ensureUserTurn(session, plan.userTurnId, plan.createdSeq);
    userTurn.createdSeq = keepUserTurnIds.has(userTurn.id)
      ? Math.min(userTurn.createdSeq, plan.createdSeq)
      : plan.createdSeq;
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
      message.createdSeq = plan.createdSeq;
      message.userTurnId = userTurn.id;
      message.modelTurnId = '';
      addUnique(userTurn.messageIds, message.id);
      keepMessageIds.add(message.id);
      return;
    }

    const modelTurn = ensureCanonicalModelTurn(session, plan.modelTurnId, userTurn.id, plan.createdSeq);
    modelTurn.createdSeq = keepModelTurnIds.has(modelTurn.id)
      ? Math.min(modelTurn.createdSeq, plan.createdSeq)
      : plan.createdSeq;
    keepModelTurnIds.add(modelTurn.id);
    const message = ensureMessage(session, {
      id: plan.id,
      role: 'assistant',
      createdSeq: plan.createdSeq,
      createdAt: resolveCanonicalCreatedAt(plan.raw),
      userTurnId: userTurn.id,
      modelTurnId: modelTurn.id
    });
    const metadataSourceIds = collectCanonicalAssistantMetadataSourceIds(
      session,
      userTurn.id,
      modelTurn.id,
      message.id,
      (canonicalAssistantCountsByUserTurn.get(userTurn.id) || 0) <= 1
    );
    patchMessageFromRaw(message, plan.raw, plan.status, plan.createdSeq);
    message.createdSeq = plan.createdSeq;
    metadataSourceIds.forEach((sourceMessageId) => {
      mergeAssistantMessageProjectionMetadata(session, sourceMessageId, message.id);
    });
    syncProjectedToolCallStats(message);
    if (plan.status === 'final') {
      settleProjectedWorkflowItems(message, 'completed');
    }
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

const collectCanonicalAssistantMetadataSourceIds = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string,
  modelTurnId: string,
  targetMessageId: string,
  includeSameUserTurnStrongSources = false
): string[] => {
  const sourceIds: string[] = [];
  const push = (messageId: string): void => {
    if (!messageId || messageId === targetMessageId || sourceIds.includes(messageId)) return;
    const message = session.messageById[messageId];
    if (!message || message.role !== 'assistant') return;
    if (!isCanonicalAssistantMetadataSource(
      session,
      message,
      userTurnId,
      modelTurnId,
      includeSameUserTurnStrongSources
    )) return;
    if (!hasAssistantProjectionMetadata(message)) return;
    sourceIds.push(messageId);
  };
  const turnIds = new Set<string>();
  if (modelTurnId) {
    turnIds.add(modelTurnId);
  }
  const userTurn = session.userTurnById[userTurnId];
  (userTurn?.modelTurnIds || []).forEach((turnId) => turnIds.add(turnId));
  session.modelTurns.forEach((turnId) => {
    const turn = session.modelTurnById[turnId];
    if (turn?.userTurnId === userTurnId) {
      turnIds.add(turnId);
    }
  });
  turnIds.forEach((turnId) => {
    const turn = session.modelTurnById[turnId];
    (turn?.messageIds || []).forEach(push);
  });
  Object.values(session.messageById).forEach((message) => {
    if (message.userTurnId === userTurnId || message.modelTurnId === modelTurnId) {
      push(message.id);
    }
  });
  return sourceIds;
};

const isCanonicalAssistantMetadataSource = (
  session: ChatRuntimeSessionProjection,
  message: ChatRuntimeMessageProjection,
  userTurnId: string,
  modelTurnId: string,
  includeSameUserTurnStrongSources = false
): boolean => {
  if (message.modelTurnId === modelTurnId) return true;
  if (message.userTurnId !== userTurnId) return false;
  if (includeSameUserTurnStrongSources) return true;
  const turn = session.modelTurnById[message.modelTurnId];
  if (!turn) return !message.modelTurnId;
  return turn.id.startsWith('legacy-model-turn:') ||
    isWeakGeneratedModelTurnId(session, turn.id, turn.userTurnId);
};

const hasAssistantProjectionMetadata = (
  message: ChatRuntimeMessageProjection
): boolean =>
  Boolean(
    (isPlainRecord(message.display) && Object.keys(message.display).length > 0) ||
      (Array.isArray(message.workflowItems) && message.workflowItems.length > 0) ||
      (Array.isArray(message.subagents) && message.subagents.length > 0)
  );

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
    if (isSyntheticGreetingRawMessage(raw)) return;
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
  message.display = patchMessageDisplayProjectionFromRaw(message.display, raw);
  message.content = String(raw.content ?? '');
  message.reasoning = String(raw.reasoning ?? '');
  message.status = status;
  message.final = status === 'final';
  message.failed = status === 'failed';
  message.cancelled = status === 'cancelled';
  patchProjectionRecordsFromRaw(message, 'workflowItems', raw.workflowItems);
  patchProjectionRecordsFromRaw(message, 'subagents', raw.subagents);
  message.updatedSeq = Math.max(message.updatedSeq, seq);
};

const mergeMessageFromRaw = (
  message: ChatRuntimeMessageProjection,
  raw: ChatRuntimeRawMessage,
  status: ChatRuntimeMessageStatus,
  seq: number
): void => {
  message.display = patchMessageDisplayProjectionFromRaw(message.display, raw);
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

const shouldMergeRuntimeSnapshotText = (event: NormalizedRuntimeEvent): boolean => {
  const sourceType = normalizeText(event.payload.source_event_type);
  return sourceType === 'llm_output' || sourceType === 'final';
};

const shouldClearAssistantTextAtWorkflowBoundary = (
  sourceType: string,
  status: string
): boolean => {
  if (status === 'completed') return false;
  if (sourceType === 'command_session_start') return true;
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) return false;
  if (SUBAGENT_WORKFLOW_EVENT_TYPES.has(sourceType)) return false;
  if (TEAM_WORKFLOW_EVENT_TYPES.has(sourceType)) return false;
  if (QUESTION_PANEL_WORKFLOW_EVENT_TYPES.has(sourceType)) return false;
  if (PLAN_WORKFLOW_EVENT_TYPES.has(sourceType)) return false;
  return false;
};

const mergeRuntimeSnapshotText = (
  current: string,
  incoming: string,
  event: NormalizedRuntimeEvent
): string => {
  const currentText = String(current || '');
  const incomingText = String(incoming || '');
  const sourceType = normalizeText(event.payload.source_event_type);
  if (event.type === 'assistant_final' && sourceType === 'final') {
    return incomingText || currentText;
  }
  if (!shouldMergeRuntimeSnapshotText(event)) {
    return incomingText || currentText;
  }
  if (!incomingText) return currentText;
  if (!currentText) return incomingText;
  if (incomingText === currentText) return currentText;
  if (incomingText.startsWith(currentText)) return incomingText;
  if (currentText.startsWith(incomingText)) return currentText;
  if (currentText.includes(incomingText)) return currentText;
  if (incomingText.length < currentText.length) return currentText;
  return incomingText;
};

const clearAssistantTextAtToolBoundary = (
  message: ChatRuntimeMessageProjection
): void => {
  if (!message || message.role !== 'assistant') return;
  const display = ensureMessageDisplayProjection(message);
  delete display[EPHEMERAL_TEXT_BEFORE_TOOL_KEY];
  delete display[EPHEMERAL_REASONING_BEFORE_TOOL_KEY];
  message.content = '';
  message.reasoning = '';
};

const clearEphemeralAssistantTextBeforeNextOutput = (
  message: ChatRuntimeMessageProjection
): void => {
  if (!message || message.role !== 'assistant' || !isPlainRecord(message.display)) return;
  const contentLength = Number(message.display[EPHEMERAL_TEXT_BEFORE_TOOL_KEY] ?? 0);
  const reasoningLength = Number(message.display[EPHEMERAL_REASONING_BEFORE_TOOL_KEY] ?? 0);
  if (Number.isFinite(contentLength) && contentLength > 0) {
    message.content = String(message.content || '').slice(contentLength);
  }
  if (Number.isFinite(reasoningLength) && reasoningLength > 0) {
    message.reasoning = String(message.reasoning || '').slice(reasoningLength);
  }
  delete message.display[EPHEMERAL_TEXT_BEFORE_TOOL_KEY];
  delete message.display[EPHEMERAL_REASONING_BEFORE_TOOL_KEY];
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

const patchProjectionRecordsFromRaw = (
  message: ChatRuntimeMessageProjection,
  field: 'workflowItems' | 'subagents',
  value: unknown
): void => {
  if (!Array.isArray(value)) return;
  const incoming = value.filter(isPlainRecord).map((item) => ({ ...item }));
  const existing = Array.isArray(message[field]) ? message[field] || [] : [];
  // Empty legacy snapshots mean "metadata not included", not "clear canonical projection".
  if (incoming.length === 0 && existing.length > 0) return;
  if (field === 'workflowItems') {
    message.workflowItems = incoming as ChatRuntimeWorkflowItemProjection[];
  } else {
    message.subagents = incoming as ChatRuntimeSubagentProjection[];
  }
};

const MESSAGE_DISPLAY_OWNED_FIELDS = new Set([
  'id',
  'message_id',
  'messageId',
  'role',
  'content',
  'reasoning',
  'created_at',
  'createdAt',
  'state',
  'runtime_status',
  'runtimeStatus',
  'stream_incomplete',
  'streamIncomplete',
  'workflowStreaming',
  'reasoningStreaming',
  'failed',
  'cancelled',
  'workflowItems',
  'subagents'
]);

const PROJECTED_STATS_DISPLAY_FIELDS = [
  'usage',
  'tokenUsage',
  'token_usage',
  'roundUsage',
  'round_usage',
  'round_usage_total',
  'quotaConsumed',
  'quota_consumed',
  'partialQuotaConsumed',
  'partial_quota_consumed',
  'toolCalls',
  'tool_calls',
  'quotaSnapshot',
  'quota',
  'quota_usage',
  'quotaUsage',
  'contextTokens',
  'context_tokens',
  'context_occupancy_tokens',
  'contextOccupancyTokens',
  'contextTotalTokens',
  'context_total_tokens',
  'context_max_tokens',
  'max_context',
  'context_usage',
  'prefill_duration_s',
  'decode_duration_s',
  'prefill_duration_total_s',
  'decode_duration_total_s',
  'avg_model_round_speed_tps',
  'avg_model_round_decode_speed_tps',
  'avg_model_round_speed_rounds',
  'interaction_start_ms',
  'interaction_end_ms',
  'interaction_duration_s'
];

const buildMessageDisplayProjection = (
  raw: ChatRuntimeRawMessage
): Record<string, unknown> => {
  const display: Record<string, unknown> = {};
  Object.entries(raw || {}).forEach(([key, value]) => {
    if (MESSAGE_DISPLAY_OWNED_FIELDS.has(key)) return;
    if (value === undefined) return;
    display[key] = cloneProjectionDisplayValue(value);
  });
  return display;
};

const patchMessageDisplayProjectionFromRaw = (
  existing: unknown,
  raw: ChatRuntimeRawMessage
): Record<string, unknown> => {
  const next = buildMessageDisplayProjection(raw);
  if (!isPlainRecord(existing)) return next;
  const existingStats = asRecord(existing.stats);
  if (Object.keys(existingStats).length > 0) {
    const mergedStats = mergeProjectedDisplayMetadata(asRecord(next.stats), existingStats);
    if (mergedStats) {
      next.stats = mergedStats;
      mirrorProjectedStatsDisplay(next, mergedStats);
    }
  }
  PROJECTED_STATS_DISPLAY_FIELDS.forEach((key) => {
    if (next[key] !== undefined || existing[key] === undefined) return;
    next[key] = cloneProjectionDisplayValue(existing[key]);
  });
  return next;
};

const cloneProjectionDisplayValue = (value: unknown): unknown => {
  if (Array.isArray(value)) {
    return value.map(cloneProjectionDisplayValue);
  }
  if (isPlainRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, inner]) => [key, cloneProjectionDisplayValue(inner)])
    );
  }
  return value;
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
  const id = resolveModelTurnIdentity(session, modelTurnId, userTurnId, seq) ||
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
  userTurnId: string,
  seq: number | null = null
): string => {
  if (!modelTurnId) return '';
  const existing = session.modelTurnById[modelTurnId];
  if (existing) {
    if (
      existing.id.startsWith('legacy-model-turn:') ||
      isWeakGeneratedModelTurnId(session, existing.id, existing.userTurnId)
    ) {
      const reusableForExisting = resolveReusableModelTurnForUserTurn(session, existing.userTurnId);
      if (
        reusableForExisting &&
        reusableForExisting.id !== existing.id &&
        shouldFoldModelTurnIntoExisting(existing.id, reusableForExisting, existing.userTurnId)
      ) {
        mergeModelTurnInto(session, existing.id, reusableForExisting.id, seq);
        return reusableForExisting.id;
      }
    }
    mergeWeakSiblingModelTurnsInto(session, existing, seq);
    return modelTurnId;
  }
  if (shouldUseActiveModelTurnForWeakRuntimeTurn(session, modelTurnId, userTurnId)) {
    const activeTurn = resolveLatestActiveAssistantModelTurn(session);
    if (activeTurn) return activeTurn.id;
  }
  const existingForUserTurn = resolveReusableModelTurnForUserTurn(session, userTurnId);
  if (
    existingForUserTurn &&
    shouldFoldModelTurnIntoExisting(modelTurnId, existingForUserTurn, userTurnId)
  ) {
    return existingForUserTurn.id;
  }
  const terminalForUserTurn = resolveReusableModelTurnForUserTurn(session, userTurnId, true, [
    'failed',
    'cancelled'
  ]);
  if (
    terminalForUserTurn &&
    shouldFoldModelTurnIntoExisting(modelTurnId, terminalForUserTurn, userTurnId)
  ) {
    return terminalForUserTurn.id;
  }
  return modelTurnId;
};

const shouldUseActiveModelTurnForWeakRuntimeTurn = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string
): boolean =>
  Boolean(resolveLatestActiveAssistantModelTurn(session)) &&
  (isWeakGeneratedUserTurnId(session, userTurnId) ||
    isWeakGeneratedModelTurnId(session, modelTurnId, userTurnId));

const isWeakGeneratedUserTurnId = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string
): boolean => {
  if (!userTurnId) return true;
  return (
    userTurnId === `user-turn:${session.sessionId}:unknown` ||
    userTurnId.startsWith(`user-turn:${session.sessionId}:request:`) ||
    userTurnId.startsWith('orphan-user-turn:')
  );
};

const isWeakGeneratedModelTurnId = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string
): boolean => {
  if (!modelTurnId) return true;
  const round = resolveRoundFromUserTurnId(session, userTurnId);
  return (
    modelTurnId === `model-turn:${userTurnId}` ||
    (
      round !== null &&
      modelTurnId === `model-turn:${session.sessionId}:user:${round}`
    ) ||
    modelTurnId === `model-turn:user-turn:${session.sessionId}:unknown` ||
    modelTurnId.startsWith(`model-turn:${session.sessionId}:request:`) ||
    modelTurnId.startsWith(`model-turn:${session.sessionId}:model:`) ||
    modelTurnId.startsWith(`model-turn:${userTurnId}:`) ||
    (
      isWeakGeneratedUserTurnId(session, userTurnId) &&
      modelTurnId.startsWith('model-turn:')
    )
  );
};

const resolveRoundFromUserTurnId = (
  session: ChatRuntimeSessionProjection,
  userTurnId: string
): number | null => {
  const prefix = `user-turn:${session.sessionId}:round:`;
  if (!userTurnId.startsWith(prefix)) return null;
  const round = Number.parseInt(userTurnId.slice(prefix.length), 10);
  return Number.isFinite(round) && round > 0 ? round : null;
};

const mergeModelTurnInto = (
  session: ChatRuntimeSessionProjection,
  sourceTurnId: string,
  targetTurnId: string,
  seq: number | null = null
): void => {
  if (!sourceTurnId || !targetTurnId || sourceTurnId === targetTurnId) return;
  const sourceTurn = session.modelTurnById[sourceTurnId];
  const targetTurn = session.modelTurnById[targetTurnId];
  if (!sourceTurn || !targetTurn) return;
  sourceTurn.messageIds.forEach((messageId) => {
    const message = session.messageById[messageId];
    if (!message || message.role !== 'assistant') return;
    message.modelTurnId = targetTurn.id;
    message.userTurnId = targetTurn.userTurnId;
    addUnique(targetTurn.messageIds, messageId);
  });
  if (!targetTurn.finalMessageId && sourceTurn.finalMessageId) {
    targetTurn.finalMessageId = sourceTurn.finalMessageId;
  }
  targetTurn.createdSeq = Math.min(targetTurn.createdSeq, sourceTurn.createdSeq);
  targetTurn.status = mergeModelTurnStatus(targetTurn.status, sourceTurn.status);
  const sourceUserTurn = session.userTurnById[sourceTurn.userTurnId];
  if (sourceUserTurn) {
    sourceUserTurn.modelTurnIds = sourceUserTurn.modelTurnIds.filter((id) => id !== sourceTurn.id);
  }
  const targetUserTurn = ensureUserTurn(session, targetTurn.userTurnId, seq);
  addUnique(targetUserTurn.modelTurnIds, targetTurn.id);
  delete session.modelTurnById[sourceTurn.id];
  session.modelTurns = session.modelTurns.filter((id) => id !== sourceTurn.id);
  const preferredMessageId = targetTurn.finalMessageId ||
    targetTurn.messageIds.find((messageId) => session.messageById[messageId]?.role === 'assistant') ||
    '';
  if (preferredMessageId) {
    targetTurn.messageIds.forEach((messageId) => {
      if (messageId === preferredMessageId) return;
      mergeAssistantMessageProjectionMetadata(session, messageId, preferredMessageId);
    });
    pruneModelTurnAssistantMessages(session, targetTurn, preferredMessageId);
  }
};

const mergeWeakSiblingModelTurnsInto = (
  session: ChatRuntimeSessionProjection,
  targetTurn: ChatRuntimeModelTurnProjection,
  seq: number | null = null
): void => {
  const userTurn = session.userTurnById[targetTurn.userTurnId];
  const turnIds = userTurn?.modelTurnIds?.length
    ? [...userTurn.modelTurnIds]
    : session.modelTurns.filter((turnId) => session.modelTurnById[turnId]?.userTurnId === targetTurn.userTurnId);
  turnIds.forEach((turnId) => {
    if (turnId === targetTurn.id) return;
    const sibling = session.modelTurnById[turnId];
    if (!sibling) return;
    const weakSibling =
      sibling.id.startsWith('legacy-model-turn:') ||
      isWeakGeneratedModelTurnId(session, sibling.id, sibling.userTurnId);
    if (!weakSibling) return;
    if (!shouldFoldModelTurnIntoExisting(sibling.id, targetTurn, targetTurn.userTurnId)) return;
    mergeModelTurnInto(session, sibling.id, targetTurn.id, seq);
  });
};

const mergeAssistantMessageProjectionMetadata = (
  session: ChatRuntimeSessionProjection,
  sourceMessageId: string,
  targetMessageId: string
): void => {
  if (!sourceMessageId || !targetMessageId || sourceMessageId === targetMessageId) return;
  const source = session.messageById[sourceMessageId];
  const target = session.messageById[targetMessageId];
  if (!source || !target || source.role !== 'assistant' || target.role !== 'assistant') return;
  if (source.content && !target.content) {
    target.content = source.content;
  }
  if (source.reasoning && !target.reasoning) {
    target.reasoning = source.reasoning;
  }
  target.display = mergeProjectedDisplayMetadata(target.display, source.display);
  mergeAssistantProjectionRecords(target, 'workflowItems', source.workflowItems);
  mergeAssistantProjectionRecords(target, 'subagents', source.subagents);
  target.failed = target.failed || source.failed;
  target.cancelled = target.cancelled || source.cancelled;
  if (!target.final && source.final) {
    target.final = true;
  }
  if (isActiveMessageStatus(target.status)) {
    target.status = pickMergedMessageStatus(target.status, source.status);
  }
  target.updatedSeq = Math.max(target.updatedSeq, source.updatedSeq);
};

const mergeProjectedDisplayMetadata = (
  target: unknown,
  source: unknown
): Record<string, unknown> | undefined => {
  if (!isPlainRecord(source)) {
    return isPlainRecord(target) ? target : undefined;
  }
  const base = isPlainRecord(target) ? target : {};
  const merged: Record<string, unknown> = { ...base };
  Object.entries(source).forEach(([key, value]) => {
    if (value === undefined) return;
    const current = merged[key];
    if (isPlainRecord(current) && isPlainRecord(value)) {
      merged[key] = mergeProjectedDisplayMetadata(current, value);
      return;
    }
    if (Array.isArray(current) && Array.isArray(value)) {
      merged[key] = value.length > current.length
        ? cloneProjectionDisplayValue(value)
        : cloneProjectionDisplayValue(current);
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
      merged[key] = cloneProjectionDisplayValue(value);
    }
  });
  return merged;
};

const mergeAssistantProjectionRecords = (
  message: ChatRuntimeMessageProjection,
  field: 'workflowItems' | 'subagents',
  value: unknown
): void => {
  if (!Array.isArray(value) || value.length === 0) return;
  const existing = Array.isArray(message[field]) ? message[field] || [] : [];
  const incoming = value.filter(isPlainRecord).map((item) => ({ ...item }));
  if (incoming.length === 0) return;
  const merged = new Map<string, ChatRuntimeWorkflowItemProjection | ChatRuntimeSubagentProjection>();
  [...existing, ...incoming].forEach((item, index) => {
    const key = resolveProjectionRecordIdentity(item, index);
    const previous = merged.get(key);
    if (!previous || normalizeProjectedCount(item.updatedSeq) >= normalizeProjectedCount(previous.updatedSeq)) {
      merged.set(key, { ...item });
    }
  });
  if (field === 'workflowItems') {
    message.workflowItems = Array.from(merged.values()) as ChatRuntimeWorkflowItemProjection[];
  } else {
    message.subagents = Array.from(merged.values()) as ChatRuntimeSubagentProjection[];
  }
};

const mergeModelTurnStatus = (
  current: ChatRuntimeModelTurnProjection['status'],
  incoming: ChatRuntimeModelTurnProjection['status']
): ChatRuntimeModelTurnProjection['status'] => {
  if (current === incoming) return current;
  if (current === 'failed' || incoming === 'failed') return 'failed';
  if (current === 'cancelled' || incoming === 'cancelled') return 'cancelled';
  if (current === 'completed' || incoming === 'completed') return 'completed';
  if (current === 'tool_running' || incoming === 'tool_running') return 'tool_running';
  if (current === 'streaming' || incoming === 'streaming') return 'streaming';
  if (current === 'waiting_first_output' || incoming === 'waiting_first_output') {
    return 'waiting_first_output';
  }
  if (current === 'finalizing' || incoming === 'finalizing') return 'finalizing';
  return 'created';
};

const resolveLatestActiveAssistantModelTurn = (
  session: ChatRuntimeSessionProjection
): ChatRuntimeModelTurnProjection | null => {
  const activeTurns = session.modelTurns
    .map((turnId) => session.modelTurnById[turnId])
    .filter((turn): turn is ChatRuntimeModelTurnProjection =>
      Boolean(turn) &&
      (
        turn.status === 'created' ||
        turn.status === 'waiting_first_output' ||
        turn.status === 'streaming' ||
        turn.status === 'tool_running' ||
        turn.status === 'finalizing'
      ) &&
      turn.messageIds.some((messageId) => session.messageById[messageId]?.role === 'assistant')
    );
  if (activeTurns.length === 0) return null;
  return activeTurns.sort((left, right) =>
    resolveModelTurnLatestMessageSeq(session, right) - resolveModelTurnLatestMessageSeq(session, left) ||
    right.createdSeq - left.createdSeq
  )[0] || null;
};

const resolveModelTurnLatestMessageSeq = (
  session: ChatRuntimeSessionProjection,
  turn: ChatRuntimeModelTurnProjection
): number => {
  const seqs = turn.messageIds
    .map((messageId) => session.messageById[messageId]?.updatedSeq)
    .filter((value): value is number => Number.isFinite(value));
  return seqs.length > 0 ? Math.max(...seqs) : turn.createdSeq;
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
  const messageRecord = message as unknown as Record<string, unknown>;
  const modelTurnSnapshot = asRecord(
    (modelTurn as unknown as Record<string, unknown>)[WORKFLOW_CONTEXT_SNAPSHOT_KEY]
  );
  if (!isPlainRecord(messageRecord[WORKFLOW_CONTEXT_SNAPSHOT_KEY]) && Object.keys(modelTurnSnapshot).length > 0) {
    messageRecord[WORKFLOW_CONTEXT_SNAPSHOT_KEY] = {
      ...modelTurnSnapshot,
      ...(isPlainRecord(modelTurnSnapshot.context_usage)
        ? { context_usage: { ...modelTurnSnapshot.context_usage } }
        : {})
    };
  }
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
  const eventMessage = eventMessageId ? session.messageById[eventMessageId] : null;
  if (eventMessage?.role === 'assistant') {
    return eventMessage.id;
  }
  const existingAssistantId = modelTurn.messageIds.find((messageId) => {
    const message = session.messageById[messageId];
    return message?.role === 'assistant';
  });
  if (existingAssistantId) return existingAssistantId;
  const reusablePendingAssistant = Object.values(session.messageById)
    .filter((message) =>
      message.role === 'assistant' &&
      message.modelTurnId === modelTurn.id &&
      isActiveMessageStatus(message.status)
    )
    .sort((left, right) => right.createdSeq - left.createdSeq)[0];
  if (reusablePendingAssistant) {
    return reusablePendingAssistant.id;
  }
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
  status: 'loading' | 'completed' | 'failed',
  modelTurn?: ChatRuntimeModelTurnProjection | null
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedWorkflowItems(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const eventType = resolveProjectedWorkflowEventType(event, status);
  const sourceType = normalizeText(event.payload.source_event_type);
  const toolName = firstText(
    data.tool,
    data.name,
    data.tool_name,
    data.toolName,
    payload.tool,
    payload.name,
    payload.tool_name,
    payload.toolName,
    COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType) ? 'execute_command' : ''
  );
  const toolDisplayName = firstText(
    data.tool_display_name,
    data.toolDisplayName,
    data.display_name,
    data.displayName,
    payload.tool_display_name,
    payload.toolDisplayName,
    payload.display_name,
    payload.displayName
  );
  const toolRuntimeName = firstText(
    data.tool_runtime_name,
    data.toolRuntimeName,
    data.runtime_name,
    data.runtimeName,
    payload.tool_runtime_name,
    payload.toolRuntimeName,
    payload.runtime_name,
    payload.runtimeName
  );
  const toolFunctionName = firstText(
    data.tool_function_name,
    data.toolFunctionName,
    data.function_name,
    data.functionName,
    payload.tool_function_name,
    payload.toolFunctionName,
    payload.function_name,
    payload.functionName
  );
  const isCommandSessionEvent = COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType);
  const toolCallId = isCommandSessionEvent
    ? resolveExplicitToolWorkflowRef(payload, data)
    : resolveToolWorkflowRef(event, payload, data);
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
  const itemId = resolveProjectedWorkflowItemId(
    event,
    eventType,
    toolCallId,
    commandSessionId,
    approvalId,
    isCommandSessionEvent
  );
  const workflowRef = eventType === 'approval_request' || eventType === 'approval_result'
    ? approvalId || toolCallId
    : isCommandSessionEvent
      ? toolCallId || commandSessionId
      : commandSessionId || toolCallId;
  const existing = findProjectedWorkflowItem(
    items,
    itemId,
    workflowRef,
    isCommandSessionEvent && toolCallId ? '' : commandSessionId,
    approvalId
  );
  const detailSource = Object.keys(data).length > 0 ? data : payload;
  const title = resolveProjectedWorkflowTitle(eventType, toolName);
  const existingEventType = normalizeText(existing?.eventType ?? existing?.event ?? existing?.event_type);
  const keepExistingTerminalResult =
    Boolean(existing) &&
    eventType === 'tool_call' &&
    existingEventType === 'tool_result' &&
    isTerminalWorkflowStatus(normalizeText(existing?.status));
  const next: ChatRuntimeWorkflowItemProjection = {
    ...(existing || {}),
    id: itemId,
    title: keepExistingTerminalResult ? firstText(existing?.title, title) : title,
    detail: keepExistingTerminalResult
      ? firstText(existing?.detail, stringifyWorkflowDetail(detailSource))
      : stringifyWorkflowDetail(detailSource),
    status: keepExistingTerminalResult ? firstText(existing?.status, status) : status,
    isTool: true,
    eventType: keepExistingTerminalResult ? existingEventType : eventType,
    sourceEventType: event.type,
    modelTurnId: modelTurn?.id || event.modelTurnId || message.modelTurnId,
    model_turn_id: modelTurn?.id || event.modelTurnId || message.modelTurnId,
    updatedSeq: event.eventSeq ?? message.updatedSeq
  };
  if (toolName) {
    next.toolName = toolName;
    next.tool = toolName;
  }
  if (toolDisplayName) {
    next.toolDisplayName = toolDisplayName;
    next.tool_display_name = toolDisplayName;
    next.displayName = toolDisplayName;
    next.display_name = toolDisplayName;
  }
  if (toolRuntimeName) {
    next.toolRuntimeName = toolRuntimeName;
    next.tool_runtime_name = toolRuntimeName;
    next.runtimeName = toolRuntimeName;
    next.runtime_name = toolRuntimeName;
  }
  if (toolFunctionName) {
    next.toolFunctionName = toolFunctionName;
    next.tool_function_name = toolFunctionName;
    next.functionName = toolFunctionName;
    next.function_name = toolFunctionName;
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
  const rawCallDetail = eventType === 'tool_call'
    ? buildProjectedToolCallRawDetail(detailSource, toolFunctionName || toolRuntimeName || toolName)
    : sourceType === 'command_session_start'
      ? buildProjectedCommandSessionStartRawDetail(
          detailSource,
          toolFunctionName || toolRuntimeName || toolName || 'execute_command'
        )
      : '';
  if (rawCallDetail) {
    next.toolCallRawDetail = rawCallDetail;
    next.tool_call_raw_detail = rawCallDetail;
  }
  const rawResultDetail = buildProjectedToolResultRawDetail(detailSource);
  if (rawResultDetail && eventType === 'tool_result') {
    next.toolResultRawDetail = rawResultDetail;
    next.tool_result_raw_detail = rawResultDetail;
  }
  attachWorkflowContextSnapshot(next, message, detailSource, modelTurn);

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
  if (sourceType === 'llm_stream_retry') return 'loading';
  if (sourceType === 'slow_client') return 'failed';
  if (sourceType === 'llm_request' || sourceType === 'knowledge_request') return 'completed';
  if (sourceType === 'plan_update') return 'completed';
  if (sourceType === 'thread_control') return 'completed';
  if (sourceType === 'question_panel') return 'loading';
  if (sourceType === 'compaction') {
    const data = asRecord(payload.data);
    const status = normalizeText(data.status ?? payload.status);
    return FAILED_WORKFLOW_STATUSES.has(status) ? 'failed' : 'completed';
  }
  if (sourceType === 'team_error') return 'failed';
  if (sourceType === 'team_finish') return 'completed';
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    const data = asRecord(payload.data);
    const exitCode = parseNonNegativeInt(
      data.exit_code ??
        data.exitCode ??
        data.returncode ??
        data.return_code ??
        data.returnCode ??
        payload.exit_code ??
        payload.exitCode
    );
    const timedOut = normalizeFlag(data.timed_out ?? data.timedOut ?? payload.timed_out ?? payload.timedOut);
    const errorText = firstText(
      data.error,
      data.error_message,
      data.errorMessage,
      payload.error,
      payload.error_message,
      payload.errorMessage
    );
    if (timedOut || errorText || (exitCode !== null && exitCode !== 0)) {
      return 'failed';
    }
    if (COMMAND_SESSION_TERMINAL_EVENT_TYPES.has(sourceType) && exitCode === 0) {
      return 'completed';
    }
  }
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

const applyProjectedWorkflowDisplay = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  sourceType: string,
  status: 'loading' | 'completed' | 'failed'
): void => {
  if (message.role !== 'assistant') return;
  const display = ensureMessageDisplayProjection(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const source = Object.keys(data).length > 0 ? data : payload;
  const eventType = resolveProjectedGenericWorkflowEventType(sourceType, source);
  if (sourceType === 'llm_stream_retry') {
    const attempt = parsePositiveInt(source.attempt);
    const maxAttempts = parsePositiveInt(source.max_attempts ?? source.maxAttempts);
    const delayS = parsePositiveNumber(source.delay_s ?? source.delayS);
    const startedAtMs = normalizeCreatedAtMs(source.timestamp ?? payload.timestamp) ?? Date.now();
    display.retry_state = 'retrying';
    display.retry_attempt = attempt;
    display.retry_max_attempts = maxAttempts;
    display.retry_delay_s = delayS;
    display.retry_started_at_ms = startedAtMs;
    display.retry_next_attempt_at_ms = delayS !== null ? startedAtMs + delayS * 1000 : null;
    display.retry_reason = firstText(source.retry_reason, source.retryReason);
    display.retry_error = firstText(source.error, source.message);
    return;
  }
  if (sourceType === 'llm_request') {
    clearProjectedRetryDisplay(display);
  }
  if (sourceType === 'slow_client') {
    display.slow_client = true;
    display.resume_available = true;
    return;
  }
  if (sourceType === 'plan_update') {
    const plan = normalizeProjectedPlanPayload(source);
    if (plan) {
      display.plan = plan;
      display.planVisible = normalizeFlag(display.planVisible) || status === 'loading';
    }
    return;
  }
  if (sourceType === 'question_panel') {
    const panel = normalizeProjectedQuestionPanel(source);
    if (panel) {
      display.questionPanel = panel;
    }
    return;
  }
  if (eventType === 'compaction' || eventType === 'compaction_progress' || eventType === 'compaction_notice') {
    display.manual_compaction_marker = true;
    if (status !== 'loading') {
      display.resume_available = false;
    }
  }
};

const applyProjectedUsageStatsDisplay = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  sourceType: string
): void => {
  if (message.role !== 'assistant') return;
  const display = ensureMessageDisplayProjection(message);
  const stats = ensureProjectedStatsDisplay(display);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const source = Object.keys(data).length > 0 ? data : payload;
  const normalizedUsage = normalizeProjectedUsagePayload(source.usage ?? source);
  const normalizedRoundUsage = normalizeProjectedUsagePayload(source.round_usage ?? source.roundUsage ?? source);
  if (sourceType === 'token_usage') {
    if (normalizedUsage) {
      stats.usage = normalizedUsage;
      stats.tokenUsage = normalizedUsage;
      stats.token_usage = normalizedUsage;
      const consumed = normalizeProjectedUsageConsumedTokens(normalizedUsage);
      if (consumed > 0) {
        stats.partialQuotaConsumed = Math.max(normalizeProjectedCount(stats.partialQuotaConsumed), consumed);
        stats.partial_quota_consumed = stats.partialQuotaConsumed;
      }
    }
    applyProjectedTimingStats(stats, source);
    applyProjectedContextUsageStats(stats, source, normalizedUsage);
  } else if (sourceType === 'round_usage') {
    if (normalizedRoundUsage) {
      stats.roundUsage = normalizedRoundUsage;
      stats.round_usage = normalizedRoundUsage;
      stats.round_usage_total = normalizedRoundUsage;
    }
    applyProjectedConsumedStats(stats, source, normalizedRoundUsage);
    applyProjectedTimingStats(stats, source);
    applyProjectedContextUsageStats(stats, source);
  } else if (sourceType === 'context_usage') {
    applyProjectedContextUsageStats(stats, source);
  } else if (sourceType === 'quota_usage') {
    applyProjectedQuotaStats(stats, source);
  }
  mirrorProjectedStatsDisplay(display, stats);
};

const updateWorkflowContextSnapshot = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  modelTurn?: ChatRuntimeModelTurnProjection | null
): void => {
  if (message.role !== 'assistant') return;
  updateWorkflowContextSnapshotRecord(message as unknown as Record<string, unknown>, event);
  if (modelTurn) {
    updateWorkflowContextSnapshotRecord(modelTurn as unknown as Record<string, unknown>, event);
  }
  backfillWorkflowItemsContextSnapshot(message, modelTurn);
};

const updateWorkflowContextSnapshotRecord = (
  target: Record<string, unknown>,
  event: NormalizedRuntimeEvent
): void => {
  const payload = event.payload;
  const data = asRecord(payload.data);
  const source = Object.keys(data).length > 0 ? data : payload;
  const contextTokens = resolveProjectedContextTokensForEvent(source, event, payload);
  const contextTotalTokens = resolveProjectedContextTotalTokens(source);
  if ((contextTokens === null || contextTokens <= 0) && contextTotalTokens === null) return;
  const snapshot: Record<string, unknown> = {
    sourceEventType: normalizeText(payload.source_event_type) || event.type,
    modelTurnId: event.modelTurnId || undefined,
    model_turn_id: event.modelTurnId || undefined,
    eventSeq: event.eventSeq ?? null
  };
  if (contextTokens !== null && contextTokens > 0) {
    snapshot.contextTokens = contextTokens;
    snapshot.context_tokens = contextTokens;
    snapshot.context_occupancy_tokens = contextTokens;
    snapshot.contextOccupancyTokens = contextTokens;
  }
  if (contextTotalTokens !== null) {
    snapshot.contextTotalTokens = contextTotalTokens;
    snapshot.context_total_tokens = contextTotalTokens;
    snapshot.context_max_tokens = contextTotalTokens;
    snapshot.max_context = contextTotalTokens;
  }
  snapshot.context_usage = {
    ...(contextTokens !== null && contextTokens > 0
      ? {
          context_tokens: contextTokens,
          contextTokens,
          context_occupancy_tokens: contextTokens,
          contextOccupancyTokens: contextTokens
        }
      : {}),
    ...(contextTotalTokens !== null
      ? {
          max_context: contextTotalTokens,
          maxContext: contextTotalTokens,
          context_max_tokens: contextTotalTokens,
          contextMaxTokens: contextTotalTokens
        }
      : {})
  };
  target[WORKFLOW_CONTEXT_SNAPSHOT_KEY] = snapshot;
};

const resolveWorkflowContextSnapshot = (
  source: Record<string, unknown>
): { contextTokens: number | null; contextTotalTokens: number | null } | null => {
  const contextTokens = resolveProjectedContextTokens(source);
  const contextTotalTokens = resolveProjectedContextTotalTokens(source);
  if ((contextTokens === null || contextTokens <= 0) && contextTotalTokens === null) return null;
  return { contextTokens, contextTotalTokens };
};

const writeWorkflowContextFields = (
  item: ChatRuntimeWorkflowItemProjection,
  contextTokens: number | null,
  contextTotalTokens: number | null
): void => {
  if (contextTokens !== null && contextTokens > 0) {
    item.contextTokens = contextTokens;
    item.context_tokens = contextTokens;
    item.context_occupancy_tokens = contextTokens;
    item.contextOccupancyTokens = contextTokens;
  }
  if (contextTotalTokens !== null) {
    item.contextTotalTokens = contextTotalTokens;
    item.context_total_tokens = contextTotalTokens;
    item.context_max_tokens = contextTotalTokens;
    item.max_context = contextTotalTokens;
  }
  const existingUsage = asRecord(item.context_usage);
  item.context_usage = {
    ...existingUsage,
    ...(contextTokens !== null && contextTokens > 0
      ? {
          context_tokens: contextTokens,
          contextTokens,
          context_occupancy_tokens: contextTokens,
          contextOccupancyTokens: contextTokens
        }
      : {}),
    ...(contextTotalTokens !== null
      ? {
          max_context: contextTotalTokens,
          maxContext: contextTotalTokens,
          context_max_tokens: contextTotalTokens,
          contextMaxTokens: contextTotalTokens
        }
      : {})
  };
};

const attachWorkflowContextSnapshot = (
  item: ChatRuntimeWorkflowItemProjection,
  message: ChatRuntimeMessageProjection,
  explicitSource?: Record<string, unknown>,
  modelTurn?: ChatRuntimeModelTurnProjection | null
): void => {
  const explicit = explicitSource ? resolveWorkflowContextSnapshot(explicitSource) : null;
  if (explicit) {
    writeWorkflowContextFields(item, explicit.contextTokens, explicit.contextTotalTokens);
    return;
  }

  const existingContextTokens = resolveProjectedContextTokens(item);
  const existingContextTotalTokens = resolveProjectedContextTotalTokens(item);
  const itemModelTurnId = firstText(item.modelTurnId, item.model_turn_id);
  const modelTurnSnapshot = resolveWorkflowContextSnapshot(
    asRecord((modelTurn as unknown as Record<string, unknown> | null | undefined)?.[WORKFLOW_CONTEXT_SNAPSHOT_KEY])
  );
  const messageSnapshotRecord = asRecord((message as Record<string, unknown>)[WORKFLOW_CONTEXT_SNAPSHOT_KEY]);
  const messageSnapshotModelTurnId = firstText(messageSnapshotRecord.modelTurnId, messageSnapshotRecord.model_turn_id);
  const canUseMessageSnapshot =
    !itemModelTurnId ||
    !messageSnapshotModelTurnId ||
    itemModelTurnId === messageSnapshotModelTurnId;
  const messageSnapshot = canUseMessageSnapshot
    ? resolveWorkflowContextSnapshot(messageSnapshotRecord)
    : null;
  if (existingContextTokens !== null && existingContextTokens > 0) {
    if (existingContextTotalTokens === null) {
      const snapshotTotal =
        modelTurnSnapshot?.contextTotalTokens ?? messageSnapshot?.contextTotalTokens ?? null;
      if (snapshotTotal !== null) {
        writeWorkflowContextFields(item, existingContextTokens, snapshotTotal);
      }
    }
    return;
  }

  const snapshot = modelTurnSnapshot || messageSnapshot;
  if (!snapshot) return;
  writeWorkflowContextFields(item, snapshot.contextTokens, snapshot.contextTotalTokens);
};

const backfillWorkflowItemsContextSnapshot = (
  message: ChatRuntimeMessageProjection,
  modelTurn?: ChatRuntimeModelTurnProjection | null
): void => {
  if (message.role !== 'assistant' || !Array.isArray(message.workflowItems)) return;
  message.workflowItems.forEach((item) => {
    if (!isPlainRecord(item)) return;
    const itemModelTurnId = firstText(item.modelTurnId, item.model_turn_id);
    if (itemModelTurnId && modelTurn?.id && itemModelTurnId !== modelTurn.id) return;
    attachWorkflowContextSnapshot(item, message, undefined, modelTurn);
  });
};

const ensureProjectedStatsDisplay = (
  display: Record<string, unknown>
): Record<string, unknown> => {
  if (!isPlainRecord(display.stats)) {
    display.stats = {};
  }
  return display.stats as Record<string, unknown>;
};

const syncProjectedToolCallStats = (
  message: ChatRuntimeMessageProjection
): void => {
  if (message.role !== 'assistant') return;
  const count = countProjectedToolCalls(message.workflowItems);
  if (count <= 0) return;
  const display = ensureMessageDisplayProjection(message);
  const stats = ensureProjectedStatsDisplay(display);
  stats.toolCalls = Math.max(normalizeProjectedCount(stats.toolCalls), count);
  stats.tool_calls = stats.toolCalls;
  mirrorProjectedStatsDisplay(display, stats);
};

const countProjectedToolCalls = (items: unknown): number => {
  if (!Array.isArray(items)) return 0;
  const keys = new Set<string>();
  items.forEach((item, index) => {
    if (!isPlainRecord(item)) return;
    if (!isProjectedToolWorkflowItem(item)) return;
    const key = firstText(
      item.toolCallId,
      item.tool_call_id,
      item.callId,
      item.call_id,
      item.id
    ) || `tool:${index}`;
    keys.add(key);
  });
  return keys.size;
};

const isProjectedToolWorkflowItem = (
  item: Record<string, unknown>
): boolean => {
  if (item.isTool === true || item.is_tool === true) return true;
  const eventType = normalizeText(item.eventType ?? item.event ?? item.event_type);
  return eventType === 'tool_call' ||
    eventType === 'tool_result' ||
    eventType === 'tool_output' ||
    eventType === 'tool_output_delta' ||
    eventType === 'tool_call_started' ||
    eventType === 'tool_call_completed' ||
    eventType === 'tool_call_failed';
};

const mirrorProjectedStatsDisplay = (
  display: Record<string, unknown>,
  stats: Record<string, unknown>
): void => {
  display.stats = stats;
  const aliases = [
    'usage',
    'tokenUsage',
    'token_usage',
    'roundUsage',
    'round_usage',
    'round_usage_total',
    'quotaConsumed',
    'quota_consumed',
    'partialQuotaConsumed',
    'partial_quota_consumed',
    'toolCalls',
    'tool_calls',
    'quotaSnapshot',
    'quota',
    'quota_usage',
    'quotaUsage',
    'contextTokens',
    'context_tokens',
    'context_occupancy_tokens',
    'contextOccupancyTokens',
    'contextTotalTokens',
    'context_total_tokens',
    'context_max_tokens',
    'max_context',
    'context_usage',
    'prefill_duration_s',
    'decode_duration_s',
    'prefill_duration_total_s',
    'decode_duration_total_s',
    'avg_model_round_speed_tps',
    'avg_model_round_decode_speed_tps',
    'avg_model_round_speed_rounds',
    'interaction_start_ms',
    'interaction_end_ms',
    'interaction_duration_s'
  ];
  aliases.forEach((key) => {
    if (stats[key] !== undefined) {
      display[key] = cloneProjectedDisplayValue(stats[key]);
    }
  });
};

const applyProjectedConsumedStats = (
  stats: Record<string, unknown>,
  source: Record<string, unknown>,
  usage: { input: number; output: number; total: number } | null
): void => {
  const consumed =
    parsePositiveInt(
      source.request_consumed_tokens ??
        source.requestConsumedTokens ??
        source.consumed_tokens ??
        source.consumedTokens ??
        source.consumed ??
        source.used ??
        source.count
    ) ?? normalizeProjectedUsageConsumedTokens(usage);
  if (consumed <= 0) return;
  stats.quotaConsumed = Math.max(normalizeProjectedCount(stats.quotaConsumed), consumed);
  stats.quota_consumed = stats.quotaConsumed;
  stats.request_consumed_tokens = stats.quotaConsumed;
  stats.requestConsumedTokens = stats.quotaConsumed;
  stats.consumed_tokens = stats.quotaConsumed;
  stats.consumedTokens = stats.quotaConsumed;
};

const applyProjectedQuotaStats = (
  stats: Record<string, unknown>,
  source: Record<string, unknown>
): void => {
  const consumed = parsePositiveInt(
    source.request_consumed_tokens ??
      source.requestConsumedTokens ??
      source.consumed_tokens ??
      source.consumedTokens ??
      source.consumed ??
      source.count ??
      source.used
  );
  if (consumed !== null) {
    stats.quotaConsumed = Math.max(normalizeProjectedCount(stats.quotaConsumed), consumed);
    stats.quota_consumed = stats.quotaConsumed;
    stats.request_consumed_tokens = stats.quotaConsumed;
    stats.requestConsumedTokens = stats.quotaConsumed;
  }
  const snapshot = normalizeProjectedQuotaSnapshot(source);
  if (snapshot) {
    stats.quotaSnapshot = snapshot;
    stats.quota = snapshot;
    stats.quota_usage = snapshot;
    stats.quotaUsage = snapshot;
  }
};

const applyProjectedContextUsageStats = (
  stats: Record<string, unknown>,
  source: Record<string, unknown>,
  usageFallback?: { input: number; output: number; total: number } | null
): void => {
  const contextTokens = resolveProjectedContextTokens(source) ??
    (usageFallback && usageFallback.total > 0 ? usageFallback.total : null);
  const contextTotalTokens = resolveProjectedContextTotalTokens(source);
  if (contextTokens !== null && contextTokens > 0) {
    stats.contextTokens = contextTokens;
    stats.context_tokens = contextTokens;
    stats.context_occupancy_tokens = contextTokens;
    stats.contextOccupancyTokens = contextTokens;
    const contextUsage = isPlainRecord(stats.context_usage) ? stats.context_usage : {};
    stats.context_usage = {
      ...contextUsage,
      context_tokens: contextTokens,
      contextTokens,
      context_occupancy_tokens: contextTokens,
      contextOccupancyTokens: contextTokens
    };
  }
  if (contextTotalTokens !== null) {
    stats.contextTotalTokens = contextTotalTokens;
    stats.context_total_tokens = contextTotalTokens;
    stats.context_max_tokens = contextTotalTokens;
    stats.max_context = contextTotalTokens;
    const contextUsage = isPlainRecord(stats.context_usage) ? stats.context_usage : {};
    stats.context_usage = {
      ...contextUsage,
      max_context: contextTotalTokens,
      maxContext: contextTotalTokens,
      context_max_tokens: contextTotalTokens,
      contextMaxTokens: contextTotalTokens
    };
  }
};

const applyProjectedTimingStats = (
  stats: Record<string, unknown>,
  source: Record<string, unknown>
): void => {
  copyProjectedPositiveNumber(stats, 'prefill_duration_s', source.prefill_duration_s ?? source.prefillDurationS ?? source.prefillDuration);
  copyProjectedPositiveNumber(stats, 'decode_duration_s', source.decode_duration_s ?? source.decodeDurationS ?? source.decodeDuration);
  copyProjectedPositiveNumber(stats, 'prefill_duration_total_s', source.prefill_duration_total_s ?? source.prefillDurationTotalS);
  copyProjectedPositiveNumber(stats, 'decode_duration_total_s', source.decode_duration_total_s ?? source.decodeDurationTotalS);
  const speed = parsePositiveNumber(
    source.avg_model_round_speed_tps ??
      source.avg_model_round_decode_speed_tps ??
      source.avgModelRoundDecodeSpeedTps ??
      source.avgModelRoundSpeedTps ??
      source.average_speed_tps ??
      source.averageSpeedTps
  );
  if (speed !== null) {
    stats.avg_model_round_speed_tps = speed;
    stats.avg_model_round_decode_speed_tps = speed;
  }
  const speedRounds = parsePositiveInt(
    source.avg_model_round_speed_rounds ??
      source.avgModelRoundSpeedRounds ??
      source.average_speed_rounds ??
      source.averageSpeedRounds
  );
  if (speedRounds !== null) {
    stats.avg_model_round_speed_rounds = speedRounds;
  }
  const startedAtMs = resolveProjectedTimestampMs(
    source.interaction_start_ms,
    source.interactionStartMs,
    source.interaction_start,
    source.started_at,
    source.startedAt
  );
  const endedAtMs = resolveProjectedTimestampMs(
    source.interaction_end_ms,
    source.interactionEndMs,
    source.interaction_end,
    source.ended_at,
    source.endedAt
  );
  if (startedAtMs !== null) {
    stats.interaction_start_ms = startedAtMs;
  }
  if (endedAtMs !== null) {
    stats.interaction_end_ms = endedAtMs;
  }
  const duration = parsePositiveNumber(
    source.interaction_duration_s ??
      source.interactionDurationS ??
      source.interactionDuration ??
      source.duration_s ??
      source.elapsed_s
  );
  if (duration !== null) {
    stats.interaction_duration_s = duration;
  }
};

const resolveUsageStatsTargetMessage = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): ChatRuntimeMessageProjection | null => {
  if (event.messageId) {
    const explicit = session.messageById[event.messageId];
    if (explicit?.role === 'assistant' && !isSyntheticRuntimeMessage(explicit)) return explicit;
  }
  if (event.modelTurnId) {
    const turn = session.modelTurnById[event.modelTurnId] ||
      resolveReusableModelTurnForUserTurn(session, event.userTurnId, true, [
        'created',
        'waiting_first_output',
        'streaming',
        'tool_running',
        'finalizing',
        'completed'
      ]);
    const message = resolveLatestAssistantMessageForModelTurn(session, turn);
    if (message) return message;
  }
  const activeTurn = resolveLatestActiveAssistantModelTurn(session);
  const activeMessage = resolveLatestAssistantMessageForModelTurn(session, activeTurn);
  if (activeMessage) return activeMessage;
  return Object.values(session.messageById)
    .filter((message) => message.role === 'assistant' && !isSyntheticRuntimeMessage(message))
    .sort((left, right) => right.updatedSeq - left.updatedSeq || right.createdSeq - left.createdSeq)[0] || null;
};

const resolveLatestAssistantMessageForModelTurn = (
  session: ChatRuntimeSessionProjection,
  turn: ChatRuntimeModelTurnProjection | null
): ChatRuntimeMessageProjection | null => {
  if (!turn) return null;
  return turn.messageIds
    .map((messageId) => session.messageById[messageId])
    .filter((message): message is ChatRuntimeMessageProjection =>
      message?.role === 'assistant' && !isSyntheticRuntimeMessage(message)
    )
    .sort((left, right) => right.updatedSeq - left.updatedSeq || right.createdSeq - left.createdSeq)[0] || null;
};

const isSyntheticRuntimeMessage = (
  message: ChatRuntimeMessageProjection | null | undefined
): boolean => isPlainRecord(message?.display) &&
  (message.display.isGreeting === true || message.display.is_greeting === true);

const normalizeProjectedUsagePayload = (
  value: unknown
): { input: number; output: number; total: number } | null => {
  const source = parseProjectedUsageRecord(value);
  if (!source) return null;
  const input = parseNonNegativeInt(
    source.input_tokens ??
      source.prompt_tokens ??
      source.inputTokens ??
      source.promptTokens ??
      source.input ??
      source.prompt
  );
  const output = parseNonNegativeInt(
    source.output_tokens ??
      source.completion_tokens ??
      source.outputTokens ??
      source.completionTokens ??
      source.output ??
      source.completion
  );
  const total = parseNonNegativeInt(source.total_tokens ?? source.totalTokens ?? source.total);
  if (input === null && output === null && total === null) return null;
  const normalizedInput = input ?? 0;
  let normalizedOutput = output ?? 0;
  const normalizedTotal = total ?? normalizedInput + normalizedOutput;
  if (normalizedOutput <= 0 && normalizedTotal > normalizedInput) {
    normalizedOutput = normalizedTotal - normalizedInput;
  }
  return {
    input: normalizedInput,
    output: normalizedOutput,
    total: normalizedTotal
  };
};

const parseProjectedUsageRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as Record<string, unknown>;
};

const normalizeProjectedUsageConsumedTokens = (
  usage: { input: number; output: number; total: number } | null
): number => usage ? Math.max(0, usage.total || usage.input || 0) : 0;

const normalizeProjectedQuotaSnapshot = (
  value: Record<string, unknown>
): Record<string, unknown> | null => {
  const daily = parseNonNegativeInt(value.daily_quota ?? value.dailyQuota ?? value.daily ?? value.quota ?? value.total);
  const used = parseNonNegativeInt(value.used ?? value.consumed ?? value.count ?? value.usage);
  const remaining = parseNonNegativeInt(value.remaining ?? value.left ?? value.quota_remaining ?? value.remain);
  const date = firstText(value.date, value.quota_date, value.quotaDate);
  if (daily === null && used === null && remaining === null && !date) return null;
  return {
    daily,
    used,
    remaining,
    date
  };
};

const resolveProjectedContextTokens = (
  source: Record<string, unknown>
): number | null => {
  const contextUsage = asRecord(source.context_usage ?? source.contextUsage);
  return parseNonNegativeInt(
    source.context_occupancy_tokens ??
      source.contextOccupancyTokens ??
      contextUsage.context_occupancy_tokens ??
      contextUsage.contextOccupancyTokens ??
      source.context_tokens ??
      source.contextTokens ??
      contextUsage.context_tokens ??
      contextUsage.contextTokens ??
      source.context
  );
};

const resolveProjectedContextTokensForEvent = (
  source: Record<string, unknown>,
  event: NormalizedRuntimeEvent,
  payload: Record<string, unknown>
): number | null => {
  const explicit = resolveProjectedContextTokens(source);
  if (explicit !== null) return explicit;
  const sourceType = normalizeText(payload.source_event_type) || event.type;
  if (sourceType !== 'llm_output' && sourceType !== 'token_usage') return null;
  const usage = normalizeProjectedUsagePayload(source.usage ?? source);
  return usage && usage.total > 0 ? usage.total : null;
};

const resolveProjectedContextTotalTokens = (
  source: Record<string, unknown>
): number | null => {
  const contextUsage = asRecord(source.context_usage ?? source.contextUsage);
  const total = parseNonNegativeInt(
    source.max_context ??
      source.maxContext ??
      source.context_total_tokens ??
      source.contextTotalTokens ??
      source.context_window ??
      source.context_max_tokens ??
      contextUsage.max_context ??
      contextUsage.maxContext ??
      contextUsage.context_max_tokens ??
      contextUsage.contextMaxTokens
  );
  return total !== null && total > 0 ? total : null;
};

const copyProjectedPositiveNumber = (
  target: Record<string, unknown>,
  key: string,
  value: unknown
): void => {
  const normalized = parsePositiveNumber(value);
  if (normalized !== null) {
    target[key] = normalized;
  }
};

const normalizeProjectedCount = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

const parseNonNegativeInt = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const cloneProjectedDisplayValue = (value: unknown): unknown => {
  if (Array.isArray(value)) {
    return value.map(cloneProjectedDisplayValue);
  }
  if (isPlainRecord(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, inner]) => [key, cloneProjectedDisplayValue(inner)])
    );
  }
  return value;
};

const upsertProjectedWorkflowEventItem = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent,
  sourceType: string,
  status: 'loading' | 'completed' | 'failed',
  modelTurn?: ChatRuntimeModelTurnProjection | null
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedWorkflowItems(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const detailSource = Object.keys(data).length > 0 ? data : payload;
  const refs = resolveWorkflowEventRefs(event, payload, data);
  const eventType = resolveProjectedGenericWorkflowEventType(sourceType, detailSource);
  if ((eventType === 'compaction' || eventType === 'compaction_progress') && !refs.toolCallId) {
    refs.toolCallId = resolveCompactionWorkflowRef(event, detailSource);
  }
  const itemId = resolveProjectedGenericWorkflowItemId(event, eventType, refs);
  const existing = findProjectedWorkflowItem(
    items,
    itemId,
    refs.toolCallId || refs.runId || refs.sessionId,
    refs.commandSessionId || refs.dispatchId,
    refs.approvalId || refs.taskId
  );
  const title = resolveProjectedGenericWorkflowTitle(eventType, detailSource, status);
  const next: ChatRuntimeWorkflowItemProjection = {
    ...(existing || {}),
    id: itemId,
    title,
    detail: stringifyWorkflowDetail(detailSource),
    status,
    eventType: eventType || 'workflow_event',
    sourceEventType: sourceType || event.type,
    updatedSeq: event.eventSeq ?? message.updatedSeq
  };
  if (sourceType === 'llm_stream_retry') {
    const attempt = parsePositiveInt(detailSource.attempt);
    const maxAttempts = parsePositiveInt(detailSource.max_attempts ?? detailSource.maxAttempts);
    const delayS = parsePositiveNumber(detailSource.delay_s ?? detailSource.delayS);
    if (attempt !== null) next.attempt = attempt;
    if (maxAttempts !== null) next.maxAttempts = maxAttempts;
    if (delayS !== null) next.delayS = delayS;
    next.retryReason = firstText(detailSource.retry_reason, detailSource.retryReason);
    next.error = firstText(detailSource.error, detailSource.message);
  }
  if (refs.toolCallId) {
    next.toolCallId = refs.toolCallId;
    next.tool_call_id = refs.toolCallId;
  }
  if (refs.commandSessionId) {
    next.commandSessionId = refs.commandSessionId;
    next.command_session_id = refs.commandSessionId;
  }
  if (refs.approvalId) {
    next.approvalId = refs.approvalId;
    next.approval_id = refs.approvalId;
  }
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
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    next.isTool = true;
    next.kind = 'command';
    next.toolName = firstText(detailSource.tool, detailSource.name, detailSource.tool_name, 'execute_command');
    next.tool = next.toolName;
  }
  if (eventType === 'compaction' || eventType === 'compaction_progress') {
    next.isTool = true;
    next.kind = 'compaction';
    next.toolName = 'context_compaction';
    next.tool = 'context_compaction';
    next.toolCallId = refs.toolCallId || resolveCompactionWorkflowRef(event, detailSource);
    next.tool_call_id = next.toolCallId;
  }
  attachWorkflowContextSnapshot(next, message, detailSource, modelTurn);

  if (existing) {
    Object.assign(existing, next);
  } else {
    items.push({
      ...next,
      createdSeq: event.eventSeq ?? message.updatedSeq
    });
  }
};

const upsertProjectedQueueWorkflowItem = (
  message: ChatRuntimeMessageProjection,
  event: NormalizedRuntimeEvent
): void => {
  if (message.role !== 'assistant') return;
  const items = ensureProjectedWorkflowItems(message);
  const payload = event.payload;
  const data = asRecord(payload.data);
  const detailSource = Object.keys(data).length > 0 ? data : payload;
  const sourceType = normalizeText(payload.source_event_type) || 'queue_update';
  const existing = findProjectedWorkflowItem(items, 'queue:status', 'queue:status', '', '');
  const next: ChatRuntimeWorkflowItemProjection = {
    ...(existing || {}),
    id: 'queue:status',
    title: resolveProjectedQueueTitle(sourceType),
    detail: stringifyWorkflowDetail(detailSource),
    status: 'pending',
    eventType: sourceType,
    sourceEventType: sourceType,
    updatedSeq: event.eventSeq ?? message.updatedSeq
  };
  if (existing) {
    Object.assign(existing, next);
  } else {
    items.push({
      ...next,
      createdSeq: event.eventSeq ?? message.updatedSeq
    });
  }
};

const resolveProjectedQueueTitle = (sourceType: string): string => {
  if (sourceType === 'queue_enter' || sourceType === 'queued') return 'Queued';
  if (sourceType === 'queue_update') return 'Queue update';
  return 'Queued';
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
  toolCallId: resolveExplicitToolWorkflowRef(payload, data),
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
  if (PLAN_WORKFLOW_EVENT_TYPES.has(sourceType) || QUESTION_PANEL_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    return `runtime-workflow:${event.modelTurnId || event.messageId}:${sourceType}`;
  }
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

const resolveProjectedGenericWorkflowEventType = (
  sourceType: string,
  source: Record<string, unknown>
): string => {
  if (sourceType === 'progress' && isProjectedCompactionProgress(source)) {
    return 'compaction_progress';
  }
  return sourceType || 'workflow_event';
};

const isProjectedCompactionProgress = (source: Record<string, unknown>): boolean => {
  const stage = normalizeText(source.stage);
  if (COMPACTION_PROGRESS_STAGES.has(stage)) return true;
  const purpose = normalizeText(source.purpose);
  if (purpose === 'compaction_summary') return true;
  return Boolean(firstText(source.compaction_id, source.compactionId));
};

const resolveCompactionWorkflowRef = (
  event: NormalizedRuntimeEvent,
  source: Record<string, unknown>
): string => firstText(
  source.compaction_id,
  source.compactionId,
  source.workflow_ref,
  source.workflowRef,
  source.round ? `compaction:${source.round}` : '',
  event.modelTurnId ? `compaction:${event.modelTurnId}` : event.eventId
);

const normalizeProjectedPlanPayload = (
  payload: Record<string, unknown>
): Record<string, unknown> | null => {
  const rawPlan = Array.isArray(payload.plan)
    ? payload.plan
    : Array.isArray(payload.steps)
      ? payload.steps
      : [];
  if (rawPlan.length === 0) return null;
  const steps: Array<Record<string, unknown>> = [];
  let hasInProgress = false;
  rawPlan.forEach((item) => {
    const record = asRecord(item);
    const step = firstText(record.step, record.title, item);
    if (!step) return;
    let status = normalizeProjectedPlanStatus(record.status);
    if (status === 'in_progress') {
      if (hasInProgress) {
        status = 'pending';
      } else {
        hasInProgress = true;
      }
    }
    steps.push({ step, status });
  });
  if (steps.length === 0) return null;
  return {
    explanation: firstText(payload.explanation),
    steps
  };
};

const normalizeProjectedPlanStatus = (value: unknown): string => {
  const normalized = normalizeText(value).replace(/[-\s]+/g, '_');
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') return 'completed';
  if (normalized === 'in_progress' || normalized === 'inprogress') return 'in_progress';
  return 'pending';
};

const normalizeProjectedQuestionPanel = (
  payload: Record<string, unknown>
): Record<string, unknown> | null => {
  const routes =
    normalizeProjectedInquiryRoutes(payload.routes).length > 0
      ? normalizeProjectedInquiryRoutes(payload.routes)
      : normalizeProjectedInquiryRoutes(payload.options).length > 0
        ? normalizeProjectedInquiryRoutes(payload.options)
        : normalizeProjectedInquiryRoutes(payload.choices);
  if (routes.length === 0) return null;
  const keepOpenRaw = payload.keep_open ?? payload.keepOpen ?? payload.awaiting;
  return {
    question: firstText(payload.question, payload.prompt, payload.title, payload.header) || 'Please choose one option',
    routes,
    multiple: payload.multiple === true || payload.allow_multiple === true || payload.multi === true,
    keepOpen: keepOpenRaw === undefined ? true : keepOpenRaw === true,
    status: normalizeProjectedInquiryStatus(payload.status),
    selected: Array.isArray(payload.selected)
      ? payload.selected.map((item) => firstText(item)).filter(Boolean)
      : []
  };
};

const normalizeProjectedInquiryRoutes = (routes: unknown): Array<Record<string, unknown>> =>
  (Array.isArray(routes) ? routes : [])
    .map((item): Record<string, unknown> | null => {
      if (typeof item === 'string') {
        const label = item.trim();
        return label ? { label, description: '', recommended: isProjectedRecommendedLabel(label) } : null;
      }
      const record = asRecord(item);
      const label = firstText(record.label, record.title, record.name);
      if (!label) return null;
      const description = firstText(record.description, record.detail, record.desc, record.summary);
      return {
        label,
        description,
        recommended: normalizeFlag(record.recommended ?? record.preferred) || isProjectedRecommendedLabel(label)
      };
    })
    .filter((item): item is Record<string, unknown> => Boolean(item));

const normalizeProjectedInquiryStatus = (value: unknown): string => {
  const normalized = normalizeText(value);
  if (normalized === 'answered') return 'answered';
  if (normalized === 'dismissed') return 'dismissed';
  return 'pending';
};

const isProjectedRecommendedLabel = (value: unknown): boolean => {
  const normalized = String(value || '').trim().toLowerCase();
  return normalized.includes('recommended') || normalized.includes('推荐');
};

const resolveProjectedGenericWorkflowTitle = (
  sourceType: string,
  source: Record<string, unknown>,
  status: 'loading' | 'completed' | 'failed'
): string => {
  if (sourceType === 'llm_request') return 'Model request';
  if (sourceType === 'llm_stream_retry') return 'Model retry';
  if (sourceType === 'knowledge_request') return 'Knowledge request';
  if (sourceType === 'plan_update') return 'Plan update';
  if (sourceType === 'question_panel') return 'Question panel';
  if (sourceType === 'slow_client') return 'Stream resume required';
  if (sourceType === 'compaction_progress') return 'Context compaction';
  if (sourceType === 'compaction') return status === 'failed' ? 'Context compaction failed' : 'Context compaction';
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) return 'Command session';
  if (sourceType === 'progress') {
    const stage = firstText(source.stage);
    const summary = firstText(source.summary);
    return summary || (stage ? `Stage: ${stage}` : 'Progress update');
  }
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

const parsePositiveInt = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const parsePositiveNumber = (value: unknown): number | null => {
  const parsed = Number(value);
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

const settleProjectedRetryWorkflowItems = (
  message: ChatRuntimeMessageProjection
): void => {
  if (!Array.isArray(message.workflowItems)) return;
  message.workflowItems.forEach((item) => {
    const eventType = normalizeText(item.eventType ?? item.event ?? item.event_type);
    const status = normalizeText(item.status);
    if (eventType === 'llm_stream_retry' && ACTIVE_WORKFLOW_STATUSES.has(status)) {
      item.status = 'completed';
    }
  });
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
    const status = normalizeText(item.status);
    const canMergeTerminalToolCall =
      eventType === 'tool_result' &&
      isTerminalWorkflowStatus(status);
    return !PROJECTED_TOOL_RESULT_EVENT_TYPES.has(eventType) || canMergeTerminalToolCall;
  }) || null;
};

const resolveProjectedWorkflowItemId = (
  event: NormalizedRuntimeEvent,
  eventType: string,
  toolCallId: string,
  commandSessionId: string,
  approvalId: string,
  isCommandSessionEvent = false
): string => {
  const ref = eventType === 'approval_request' || eventType === 'approval_result'
    ? approvalId || toolCallId || commandSessionId
    : isCommandSessionEvent
      ? toolCallId || commandSessionId || approvalId
      : commandSessionId || toolCallId || approvalId;
  if (ref) return `runtime-workflow:${event.modelTurnId || event.messageId}:${ref}`;
  if (event.eventId) return `runtime-workflow:${event.modelTurnId || event.messageId}:event:${event.eventId}`;
  return `runtime-workflow:${event.modelTurnId || event.messageId}:${eventType}:${event.eventSeq ?? 'local'}`;
};

const resolveProjectedWorkflowEventType = (
  event: NormalizedRuntimeEvent,
  status: 'loading' | 'completed' | 'failed'
): string => {
  const sourceType = normalizeText(event.payload.source_event_type);
  if (COMMAND_SESSION_WORKFLOW_EVENT_TYPES.has(sourceType)) {
    return COMMAND_SESSION_TERMINAL_EVENT_TYPES.has(sourceType) || status !== 'loading'
      ? 'tool_result'
      : 'tool_output_delta';
  }
  if (
    status === 'loading' &&
    (
      sourceType === 'tool_call_delta' ||
      sourceType === 'tool_output' ||
      sourceType === 'tool_output_delta'
    )
  ) {
    return sourceType;
  }
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

const resolveExplicitToolWorkflowRef = (
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
  payload.toolRunId
);

const buildProjectedToolCallRawDetail = (
  source: Record<string, unknown>,
  toolName: string
): string => {
  const nestedFunction = asRecord(source.function);
  const args = parseProjectedToolCallArgs(
    source.args ??
      source.arguments ??
      source.input ??
      nestedFunction.arguments
  );
  if (!args) return '';
  return stringifyWorkflowDetail({
    tool: toolName || firstText(source.tool, source.name, nestedFunction.name),
    arguments: args
  });
};

const buildProjectedCommandSessionStartRawDetail = (
  source: Record<string, unknown>,
  toolName: string
): string => {
  const command = firstText(
    source.content,
    source.command,
    source.cmd,
    source.input,
    source.raw,
    source.script
  );
  if (!command) return '';
  const args: Record<string, unknown> = {
    content: command
  };
  const workdir = firstText(source.workdir, source.cwd);
  if (workdir) {
    args.workdir = workdir;
  }
  const timeout = source.timeout_s ?? source.timeoutS ?? source.timeout;
  if (timeout !== undefined && timeout !== null && timeout !== '') {
    args.timeout_s = timeout;
  }
  return stringifyWorkflowDetail({
    tool: toolName || 'execute_command',
    arguments: args
  });
};

const parseProjectedToolCallArgs = (value: unknown): unknown => {
  if (value === null || value === undefined || value === '') return null;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return null;
    if (trimmed[0] === '{' || trimmed[0] === '[') {
      try {
        return JSON.parse(trimmed);
      } catch {
        return { content: trimmed };
      }
    }
    return { content: trimmed };
  }
  if (typeof value === 'object') return value;
  return { content: String(value) };
};

const buildProjectedToolResultRawDetail = (
  source: Record<string, unknown>
): string => {
  const data = source.data !== undefined ? source.data : source.result;
  if (data !== undefined) {
    return stringifyWorkflowDetail(data);
  }
  const observation = firstText(source.model_observation, source.modelObservation);
  if (observation) return observation;
  return '';
};

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

const ensureMessageDisplayProjection = (
  message: ChatRuntimeMessageProjection
): Record<string, unknown> => {
  if (!isPlainRecord(message.display)) {
    message.display = {};
  }
  return message.display;
};

const clearProjectedRetryDisplay = (display: Record<string, unknown>): void => {
  delete display.retry_state;
  delete display.retry_attempt;
  delete display.retry_max_attempts;
  delete display.retry_delay_s;
  delete display.retry_started_at_ms;
  delete display.retry_next_attempt_at_ms;
  delete display.retry_reason;
  delete display.retry_error;
};

const deriveSessionRuntime = (session: ChatRuntimeSessionProjection): void => {
  if (session.connectionState === 'reconnecting') {
    setSessionBusy(session, 'reconnecting', 'reconnecting');
    return;
  }
  if (
    session.runtimeStatus === 'waiting_approval' ||
    session.runtimeStatus === 'waiting_user_input' ||
    session.runtimeStatus === 'finalizing' ||
    session.runtimeStatus === 'queued'
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
  eventType === 'assistant_output_snapshot' ||
  eventType === 'assistant_final' ||
  eventType === 'tool_call_started' ||
  eventType === 'tool_call_delta' ||
  eventType === 'tool_call_completed' ||
  eventType === 'tool_call_failed' ||
  eventType === 'workflow_event' ||
  eventType === 'usage_stats';

const isTerminalSafeEventType = (eventType: string): boolean =>
  eventType === 'turn_cancelled' ||
  eventType === 'turn_failed' ||
  eventType === 'turn_completed' ||
  eventType === 'session_idle' ||
  eventType === 'session_runtime' ||
  eventType === 'session_snapshot';

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
  const baseSeq = Math.max(session.appliedSeq, session.snapshotSeq, resolveMaxProjectionSeq(session));
  session.localSeq = Math.max(session.localSeq + 1, baseSeq - session.appliedSeq + 1);
  return session.appliedSeq + session.localSeq;
};

const resolveMaxProjectionSeq = (session: ChatRuntimeSessionProjection): number => {
  const seqs = [
    ...Object.values(session.messageById || {}).flatMap((message) => [
      message.createdSeq,
      message.updatedSeq
    ]),
    ...Object.values(session.userTurnById || {}).map((turn) => turn.createdSeq),
    ...Object.values(session.modelTurnById || {}).map((turn) => turn.createdSeq)
  ].filter((value): value is number => Number.isFinite(value));
  return seqs.length > 0 ? Math.max(...seqs) : 0;
};

const hashText = (value: string): string => {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash.toString(36);
};
