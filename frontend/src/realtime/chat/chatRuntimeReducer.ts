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
  ChatRuntimeSessionProjection,
  ChatRuntimeUserTurnProjection,
  ChatRuntimeViolation,
  ChatSessionRuntimeStatus
} from './chatRuntimeTypes';

const DEBUG_EVENT_LIMIT = 300;
const QUARANTINE_LIMIT = 100;
const VIOLATION_LIMIT = 100;

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
  quarantinedEvents: []
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

  if (
    event.eventSeq !== null &&
    session.appliedSeq > 0 &&
    event.eventSeq > session.appliedSeq + 1 &&
    event.source !== 'legacy'
  ) {
    session.syncRequired = true;
    pushViolation(session, {
      code: 'event_seq_gap',
      message: 'event_seq advanced beyond the next expected value',
      eventSeq: event.eventSeq,
      eventType: event.type
    });
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
  appendDebugEvent(projection, session, event, beforeSummary, summarizeSession(session));
  return {
    applied: true,
    ignored: false,
    quarantined: false,
    sessionId: session.sessionId,
    eventSeq: event.eventSeq
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
      event.type === 'tool_call_failed'
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
  const message = ensureMessage(session, {
    id: event.messageId,
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
};

const applyToolActivity = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  completed: boolean
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(
    session,
    event,
    completed ? 'streaming' : 'tooling'
  );
  message.status = completed ? 'streaming' : 'tooling';
  modelTurn.status = completed ? 'streaming' : 'tool_running';
  if (!completed) {
    setSessionBusy(session, 'running', 'tool_running');
  }
};

const applyToolFailed = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent
): void => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const message = ensureAssistantMessageForModelTurn(session, event, 'failed');
  message.status = 'failed';
  message.failed = true;
  modelTurn.status = 'failed';
  session.runtimeStatus = 'failed';
  session.busyReason = null;
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
      modelTurn.finalMessageId = modelTurn.finalMessageId || message.id;
    } else if (terminal === 'failed') {
      message.status = 'failed';
      message.failed = true;
    } else if (terminal === 'cancelled') {
      message.status = 'cancelled';
      message.cancelled = true;
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
    : Array.isArray(event.payload.messages)
      ? event.payload.messages as ChatRuntimeRawMessage[]
      : [];
  mergeLegacyMessages(session, messages, {
    snapshotSeq,
    replaceExistingAtOrBelowSeq: true
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
    : Array.isArray(event.payload.messages)
      ? event.payload.messages as ChatRuntimeRawMessage[]
      : [];
  const reconcileSeq = event.eventSeq ?? nextLocalSeq(session);
  mergeLegacyMessages(session, messages, {
    snapshotSeq: reconcileSeq,
    replaceExistingAtOrBelowSeq: true
  });
  const loading = normalizeFlag(event.loading ?? event.payload.loading);
  const running = normalizeFlag(event.running ?? event.payload.running);
  if (loading || running) {
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
  options: { snapshotSeq: number; replaceExistingAtOrBelowSeq: boolean }
): void => {
  const plans = buildLegacyMessagePlans(session, messages, options.snapshotSeq);
  plans.forEach((plan) => {
    const { raw, role, id, status, userTurnId, modelTurnId, createdSeq } = plan;
    if (role === 'user') {
      const turn = ensureUserTurn(session, userTurnId, createdSeq);
      const existed = Boolean(session.messageById[id]);
      const message = ensureMessage(session, {
        id,
        role,
        createdSeq,
        createdAt: normalizeCreatedAt(raw.created_at ?? raw.createdAt),
        userTurnId: turn.id,
        modelTurnId: ''
      });
      if (existed === false || shouldReplaceSnapshotMessage(message, options.snapshotSeq, options.replaceExistingAtOrBelowSeq)) {
        patchMessageFromRaw(message, raw, status, options.snapshotSeq);
      }
      message.legacyKey = id;
      message.raw = raw;
      addUnique(turn.messageIds, message.id);
      addUnique(session.messages, message.id);
      return;
    }
    const modelTurn = ensureModelTurn(
      session,
      modelTurnId,
      userTurnId,
      createdSeq
    );
    const existed = Boolean(session.messageById[id]);
    const message = ensureMessage(session, {
      id,
      role,
      createdSeq,
      createdAt: normalizeCreatedAt(raw.created_at ?? raw.createdAt),
      userTurnId,
      modelTurnId: modelTurn.id
    });
    if (existed === false || shouldReplaceSnapshotMessage(message, options.snapshotSeq, options.replaceExistingAtOrBelowSeq)) {
      patchMessageFromRaw(message, raw, status, options.snapshotSeq);
    }
    message.legacyKey = id;
    message.raw = raw;
    addUnique(modelTurn.messageIds, message.id);
    if (status === 'final') {
      modelTurn.finalMessageId = modelTurn.finalMessageId || message.id;
    }
    addUnique(session.messages, message.id);
  });
};

type LegacyMessagePlan = {
  raw: ChatRuntimeRawMessage;
  index: number;
  role: 'user' | 'assistant';
  id: string;
  status: ChatRuntimeMessageStatus;
  streamRound: number | null;
  userTurnId: string;
  modelTurnId: string;
  createdAtMs: number | null;
  createdSeq: number;
  turnOrder: number;
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
    const userTurnId =
      explicitUserTurnId ||
      (role === 'user'
        ? streamRound !== null
          ? `legacy-user-turn:round:${streamRound}`
          : `legacy-user-turn:${id}`
        : '');
    const modelTurnId = role === 'assistant'
      ? explicitModelTurnId ||
        (streamRound !== null ? `legacy-model-turn:round:${streamRound}` : `legacy-model-turn:${id}`)
      : '';
    plans.push({
      raw,
      index,
      role,
      id,
      status,
      streamRound,
      userTurnId,
      modelTurnId,
      createdAtMs,
      createdSeq: snapshotSeq + index + 1,
      turnOrder: Number.MAX_SAFE_INTEGER
    });
  });

  const userTurnByRound = new Map<number, string>();
  const userPlans = plans.filter((plan) => plan.role === 'user');
  userPlans.forEach((plan) => {
    if (plan.role !== 'user') return;
    if (plan.streamRound !== null) {
      userTurnByRound.set(plan.streamRound, plan.userTurnId);
    }
  });

  plans.forEach((plan) => {
    if (plan.role !== 'assistant' || plan.userTurnId) return;
    plan.userTurnId =
      (plan.streamRound !== null ? userTurnByRound.get(plan.streamRound) : '') ||
      resolveLegacyUserTurnByTimestamp(userPlans, plan) ||
      resolveAdjacentLegacyUserTurn(userPlans, plan) ||
      resolveNearestLegacyUserTurnId(session, plan.index);
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
      plan.turnOrder = turnOrderByUserTurnId.get(plan.userTurnId) ?? orderedUserPlans.length + plan.index;
    }
  });

  const semanticOrder = [...plans].sort(compareLegacyMessagePlanOrder);
  semanticOrder.forEach((plan, semanticIndex) => {
    plan.createdSeq = snapshotSeq + semanticIndex + 1;
  });

  return semanticOrder;
};

const resolveLegacyUserTurnByTimestamp = (
  userPlans: LegacyMessagePlan[],
  assistantPlan: LegacyMessagePlan
): string => {
  if (assistantPlan.createdAtMs === null) return '';
  const precedingUser = userPlans
    .filter((plan) => plan.createdAtMs !== null && Number(plan.createdAtMs) <= Number(assistantPlan.createdAtMs))
    .sort((left, right) => {
      const timeDiff = Number(right.createdAtMs) - Number(left.createdAtMs);
      return timeDiff || right.index - left.index;
    })[0];
  if (precedingUser) return precedingUser.userTurnId;
  return userPlans
    .filter((plan) => plan.createdAtMs !== null)
    .sort((left, right) => {
      const timeDiff = Number(left.createdAtMs) - Number(right.createdAtMs);
      return timeDiff || left.index - right.index;
    })[0]?.userTurnId || '';
};

const resolveAdjacentLegacyUserTurn = (
  userPlans: LegacyMessagePlan[],
  assistantPlan: LegacyMessagePlan
): string => {
  const previousUser = userPlans
    .filter((plan) => plan.index < assistantPlan.index)
    .sort((left, right) => right.index - left.index)[0];
  if (previousUser) return previousUser.userTurnId;
  return userPlans
    .filter((plan) => plan.index > assistantPlan.index)
    .sort((left, right) => left.index - right.index)[0]?.userTurnId || '';
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
  message.updatedSeq = Math.max(message.updatedSeq, seq);
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

const ensureModelTurn = (
  session: ChatRuntimeSessionProjection,
  modelTurnId: string,
  userTurnId: string,
  seq: number | null
): ChatRuntimeModelTurnProjection => {
  const id = modelTurnId || `local-model-turn:${session.sessionId}:${session.modelTurns.length + 1}`;
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

const ensureAssistantMessageForModelTurn = (
  session: ChatRuntimeSessionProjection,
  event: NormalizedRuntimeEvent,
  status: ChatRuntimeMessageStatus
): ChatRuntimeMessageProjection => {
  const modelTurn = ensureModelTurn(session, event.modelTurnId, event.userTurnId, event.eventSeq);
  const messageId = event.messageId || modelTurn.finalMessageId || `local-assistant:${modelTurn.id}`;
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
  return message;
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
      cancelled: false
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
): string => {
  const lastUserTurn = [...session.userTurns].reverse().find((turnId) => {
    const turn = session.userTurnById[turnId];
    return turn?.messageIds?.length && turn.createdSeq <= session.snapshotSeq + index + 1;
  });
  return lastUserTurn || `legacy-user-turn:orphan:${index}`;
};

const resolveLegacyMessageStatus = (message: ChatRuntimeRawMessage): ChatRuntimeMessageStatus => {
  if (normalizeRole(message.role) !== 'assistant') return 'final';
  if (normalizeFlag(message.failed) || normalizeText(message.status) === 'failed') return 'failed';
  if (normalizeFlag(message.cancelled) || normalizeText(message.status) === 'cancelled') return 'cancelled';
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
  if (normalizeFlag(message.workflowStreaming) || hasActiveWorkflowItems(message.workflowItems)) {
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

const hasActiveWorkflowItems = (workflowItems: unknown): boolean => {
  if (!Array.isArray(workflowItems)) return false;
  return workflowItems.some((item) => {
    if (!item || typeof item !== 'object') return false;
    return ACTIVE_WORKFLOW_STATUSES.has(normalizeText((item as Record<string, unknown>).status));
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

const addUnique = (target: string[], value: string): void => {
  if (!value || target.includes(value)) return;
  target.push(value);
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
