import type { ChatRuntimeEvent } from './chatRuntimeTypes';

type CanonicalBuildOptions = {
  sessionId: string;
  eventType: string;
  payload?: Record<string, unknown> | null;
  eventId?: string | number | null;
  requestId?: string | null;
  clientMessageId?: string | null;
  userTurnId?: string | null;
  modelTurnId?: string | null;
  assistantMessageId?: string | null;
  phase?: string | null;
  source?: string | null;
};

const TERMINAL_FAILED_STATUSES = new Set([
  'failed',
  'error',
  'system_error',
  'aborted',
  'cancelled',
  'canceled',
  'rejected'
]);

const TERMINAL_CANCELLED_STATUSES = new Set(['cancelled', 'canceled', 'aborted']);

const WORKFLOW_EVENT_PREFIXES = ['subagent_', 'team_'];
const COMMAND_SESSION_EVENT_TYPES = new Set([
  'command_session_delta',
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary'
]);
const GENERIC_WORKFLOW_EVENT_TYPES = new Set([
  'progress',
  'llm_request',
  'llm_stream_retry',
  'knowledge_request',
  'thread_control',
  'plan_update',
  'question_panel',
  'slow_client',
  'compaction',
  'compaction_progress',
  'compaction_notice'
]);
const USAGE_EVENT_TYPES = new Set([
  'token_usage',
  'round_usage',
  'context_usage',
  'quota_usage'
]);

const normalizeId = (value: unknown): string => String(value ?? '').trim();

const normalizeEventType = (value: unknown): string => normalizeId(value).toLowerCase();

const normalizeSeq = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};

const readData = (payload: Record<string, unknown>): Record<string, unknown> => {
  const nested = asRecord(payload.data);
  return Object.keys(nested).length > 0 ? nested : payload;
};

const readSegments = (source: Record<string, unknown>): Record<string, unknown>[] => {
  if (Array.isArray(source.segments)) {
    return source.segments.map(asRecord).filter((item) => Object.keys(item).length > 0);
  }
  const nested = asRecord(source.data);
  if (Array.isArray(nested.segments)) {
    return nested.segments.map(asRecord).filter((item) => Object.keys(item).length > 0);
  }
  return [];
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value) {
      return value;
    }
    if (value !== null && value !== undefined && typeof value !== 'object') {
      const text = String(value);
      if (text) return text;
    }
  }
  return '';
};

const firstId = (...values: unknown[]): string => {
  for (const value of values) {
    const text = normalizeId(value);
    if (text) return text;
  }
  return '';
};

const resolveUserRound = (payload: Record<string, unknown>, data: Record<string, unknown>): string => {
  const direct = firstId(
    data.user_round,
    data.userRound,
    payload.user_round,
    payload.userRound,
    data.round,
    payload.round
  );
  if (direct) return direct;
  for (const segment of readSegments(data).concat(readSegments(payload))) {
    const segmentRound = firstId(segment.user_round, segment.userRound, segment.round);
    if (segmentRound) return segmentRound;
  }
  return '';
};

const resolveModelRound = (payload: Record<string, unknown>, data: Record<string, unknown>): string => {
  const direct = firstId(
    data.model_round,
    data.modelRound,
    payload.model_round,
    payload.modelRound,
    data.round,
    payload.round
  );
  if (direct) return direct;
  for (const segment of readSegments(data).concat(readSegments(payload))) {
    const segmentRound = firstId(segment.model_round, segment.modelRound, segment.round);
    if (segmentRound) return segmentRound;
  }
  return '';
};

const resolveUserTurnId = (
  sessionId: string,
  payload: Record<string, unknown>,
  data: Record<string, unknown>,
  requestId: string,
  userRound: string,
  clientMessageId: string
): string =>
  firstId(
    data.user_turn_id,
    data.userTurnId,
    payload.user_turn_id,
    payload.userTurnId,
    data.turn_id,
    data.turnId,
    payload.turn_id,
    payload.turnId,
    userRound ? `user-turn:${sessionId}:round:${userRound}` : '',
    clientMessageId,
    requestId ? `user-turn:${sessionId}:request:${requestId}` : '',
    `user-turn:${sessionId}:unknown`
  );

const resolveModelTurnId = (
  sessionId: string,
  payload: Record<string, unknown>,
  data: Record<string, unknown>,
  requestId: string,
  userRound: string,
  modelRound: string,
  userTurnId: string
): string =>
  firstId(
    data.model_turn_id,
    data.modelTurnId,
    payload.model_turn_id,
    payload.modelTurnId,
    data.assistant_turn_id,
    data.assistantTurnId,
    payload.assistant_turn_id,
    payload.assistantTurnId,
    userRound && modelRound ? `model-turn:${sessionId}:user:${userRound}:model:${modelRound}` : '',
    userRound ? `model-turn:${sessionId}:user:${userRound}` : '',
    modelRound ? `model-turn:${sessionId}:model:${modelRound}` : '',
    requestId ? `model-turn:${sessionId}:request:${requestId}` : '',
    `model-turn:${userTurnId}`
  );

const resolveMessageId = (
  sessionId: string,
  role: 'user' | 'assistant',
  payload: Record<string, unknown>,
  data: Record<string, unknown>,
  modelTurnId: string,
  userTurnId: string,
  clientMessageId: string,
  assistantMessageId = ''
): string => {
  if (role === 'user') {
    return firstId(
      data.message_id,
      data.messageId,
      payload.message_id,
      payload.messageId,
      clientMessageId,
      `user-message:${userTurnId}`
    );
  }
  return firstId(
    data.message_id,
    data.messageId,
    payload.message_id,
    payload.messageId,
    assistantMessageId,
    data.assistant_message_id,
    data.assistantMessageId,
    payload.assistant_message_id,
    payload.assistantMessageId,
    `assistant-message:${modelTurnId || sessionId}`
  );
};

const buildEventId = (
  sessionId: string,
  eventType: string,
  eventId: string,
  requestId: string,
  suffix = ''
): string => {
  if (eventId) return suffix ? `${eventId}:${suffix}` : eventId;
  const requestPart = requestId || 'no-request';
  return `synthetic:${sessionId}:${requestPart}:${eventType}${suffix ? `:${suffix}` : ''}`;
};

const buildBaseEvent = (
  options: CanonicalBuildOptions,
  runtimeType: string,
  extra: Partial<ChatRuntimeEvent> = {},
  suffix = ''
): ChatRuntimeEvent => {
  const payload = asRecord(options.payload);
  const data = readData(payload);
  const sessionId = normalizeId(
    options.sessionId || data.session_id || data.sessionId || payload.session_id || payload.sessionId
  );
  const requestId = firstId(
    options.requestId,
    data.request_id,
    data.requestId,
    payload.request_id,
    payload.requestId
  );
  const eventSeq = normalizeSeq(
    extra.event_seq ??
      data.event_seq ??
      data.eventSeq ??
      payload.event_seq ??
      payload.eventSeq ??
      options.eventId
  );
  const eventId = buildEventId(
    sessionId,
    runtimeType,
    normalizeId(extra.event_id ?? data.event_id ?? data.eventId ?? payload.event_id ?? payload.eventId ?? options.eventId),
    requestId,
    suffix
  );
  const userRound = resolveUserRound(payload, data);
  const modelRound = resolveModelRound(payload, data);
  const clientMessageId = firstId(
    options.clientMessageId,
    data.client_message_id,
    data.clientMessageId,
    payload.client_message_id,
    payload.clientMessageId
  );
  const userTurnId = firstId(
    options.userTurnId,
    resolveUserTurnId(sessionId, payload, data, requestId, userRound, clientMessageId)
  );
  const modelTurnId = firstId(
    options.modelTurnId,
    resolveModelTurnId(sessionId, payload, data, requestId, userRound, modelRound, userTurnId)
  );
  const assistantMessageIdHint = firstId(options.assistantMessageId);
  const assistantMessageId = resolveMessageId(
    sessionId,
    'assistant',
    payload,
    data,
    modelTurnId,
    userTurnId,
    clientMessageId,
    assistantMessageIdHint
  );
  const needsAssistantMessage =
    runtimeType.startsWith('assistant_') ||
    runtimeType.startsWith('tool_call_') ||
    runtimeType === 'workflow_event' ||
    runtimeType === 'usage_stats';
  return {
    event_type: runtimeType,
    source: options.source || 'ws',
    strict: extra.strict ?? eventSeq !== null,
    session_id: sessionId,
    event_id: eventId,
    event_seq: eventSeq,
    user_turn_id: userTurnId,
    model_turn_id: modelTurnId,
    ...(runtimeType === 'user_message_created' ? {
      message_id: resolveMessageId(
        sessionId,
        'user',
        payload,
        data,
        modelTurnId,
        userTurnId,
        clientMessageId
      )
    } : {}),
    ...(needsAssistantMessage ? { message_id: assistantMessageId } : {}),
    payload: {
      ...payload,
      data,
      request_id: requestId,
      client_message_id: clientMessageId || undefined,
      user_turn_id: userTurnId || undefined,
      model_turn_id: modelTurnId || undefined,
      assistant_message_id: assistantMessageId || undefined,
      source_event_type: options.eventType,
      source_phase: options.phase || undefined
    },
    ...extra
  };
};

const extractDelta = (
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): { delta: string; reasoningDelta: string } => {
  const segments = readSegments(data);
  if (segments.length > 0) {
    let delta = '';
    let reasoningDelta = '';
    segments.forEach((segment) => {
      delta += firstText(segment.delta, segment.content);
      reasoningDelta += firstText(
        segment.reasoning_delta,
        segment.reasoningDelta,
        segment.think_delta,
        segment.thinkDelta
      );
    });
    return { delta, reasoningDelta };
  }
  return {
    delta: firstText(data.delta, payload.delta, data.content, payload.content, data.message, payload.message),
    reasoningDelta: firstText(
      data.reasoning_delta,
      data.reasoningDelta,
      payload.reasoning_delta,
      payload.reasoningDelta,
      data.think_delta,
      data.thinkDelta,
      payload.think_delta,
      payload.thinkDelta
    )
  };
};

const extractFinalContent = (
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): { content: string; reasoning: string } => ({
  content: firstText(data.answer, payload.answer, data.content, payload.content, data.message, payload.message),
  reasoning: firstText(
    data.reasoning,
    payload.reasoning,
    data.reasoning_content,
    data.reasoningContent,
    payload.reasoning_content,
    payload.reasoningContent,
    data.think_content,
    data.thinkContent,
    payload.think_content,
    payload.thinkContent
  )
});

const isTerminalLlmOutput = (
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): boolean => {
  const stopReason = firstText(
    data.stop_reason,
    data.stopReason,
    data.finish_reason,
    data.finishReason,
    payload.stop_reason,
    payload.stopReason,
    payload.finish_reason,
    payload.finishReason
  );
  return Boolean(
    stopReason ||
      data.done === true ||
      data.final === true ||
      data.is_final === true ||
      payload.done === true
  );
};

export const buildCanonicalChatRuntimeEvents = (
  options: CanonicalBuildOptions
): ChatRuntimeEvent[] => {
  const eventType = normalizeEventType(options.eventType);
  if (!options.sessionId || eventType === 'heartbeat' || eventType === 'ping') {
    return [];
  }
  const payload = asRecord(options.payload);
  const data = readData(payload);

  if (
    eventType === 'llm_output_delta' ||
    eventType === 'delta' ||
    eventType === 'message' ||
    eventType === 'think_delta' ||
    eventType === 'reasoning_delta'
  ) {
    const { delta, reasoningDelta } = extractDelta(payload, data);
    if (!delta && !reasoningDelta) return [];
    return [
      buildBaseEvent(options, 'assistant_delta', {
        delta,
        reasoning_delta: reasoningDelta
      })
    ];
  }

  if (eventType === 'round_start' || eventType === 'received') {
    const content = firstText(
      data.question,
      payload.question,
      data.user_message,
      payload.user_message,
      data.user_content,
      payload.user_content,
      data.input,
      payload.input,
      data.prompt,
      payload.prompt,
      data.message,
      payload.message
    );
    if (!content) return [];
    return [
      buildBaseEvent(options, 'user_message_created', {
        role: 'user',
        content
      })
    ];
  }

  if (eventType === 'channel_message') {
    const role = normalizeEventType(data.role ?? payload.role);
    const content = firstText(data.content, payload.content, data.message, payload.message);
    if (!content || (role !== 'user' && role !== 'assistant')) return [];
    if (role === 'user') {
      return [
        buildBaseEvent(options, 'user_message_created', {
          role: 'user',
          content
        })
      ];
    }
    return [
      buildBaseEvent(options, 'assistant_final', {
        role: 'assistant',
        content
      }),
      buildBaseEvent(options, 'turn_completed', {}, 'terminal')
    ];
  }

  if (eventType === 'final') {
    const finalContent = extractFinalContent(payload, data);
    return [
      buildBaseEvent(options, 'assistant_final', {
        content: finalContent.content,
        reasoning: finalContent.reasoning
      }),
      buildBaseEvent(options, 'turn_completed', {}, 'terminal')
    ];
  }

  if (eventType === 'turn_terminal') {
    const status = normalizeEventType(data.status ?? payload.status);
    const finalOk = data.final_ok ?? payload.final_ok;
    const terminalType =
      TERMINAL_CANCELLED_STATUSES.has(status)
        ? 'turn_cancelled'
        : TERMINAL_FAILED_STATUSES.has(status) || finalOk === false
          ? 'turn_failed'
          : 'turn_completed';
    return [buildBaseEvent(options, terminalType)];
  }

  if (eventType === 'queue_finish') {
    return [buildBaseEvent(options, 'turn_completed')];
  }

  if (eventType === 'error' || eventType === 'queue_fail') {
    return [
      buildBaseEvent(options, 'turn_failed', {
        content: firstText(data.message, payload.message, data.error, payload.error)
      })
    ];
  }

  if (eventType === 'thread_closed') {
    return [buildBaseEvent(options, 'session_idle')];
  }

  if (eventType === 'thread_status') {
    const status = normalizeEventType(
      data.thread_status ??
        data.threadStatus ??
        data.runtime_status ??
        data.runtimeStatus ??
        data.status ??
        payload.status
    );
    return [
      buildBaseEvent(options, status === 'idle' ? 'session_idle' : 'session_runtime', {
        runtime_status: status || 'running'
      })
    ];
  }

  if (eventType === 'queued' || eventType === 'queue_enter' || eventType === 'queue_update') {
    return [
      buildBaseEvent(options, 'session_runtime', {
        runtime_status: 'queued'
      }),
      buildBaseEvent(options, 'queue_status', {
        strict: false,
        event_seq: null
      }, 'queue')
    ];
  }

  if (eventType === 'queue_start') {
    return [
      buildBaseEvent(options, 'session_runtime', {
        runtime_status: 'running'
      })
    ];
  }

  if (eventType === 'tool_call_delta' || eventType === 'tool_output' || eventType === 'tool_output_delta') {
    return [buildBaseEvent(options, 'tool_call_delta')];
  }

  if (COMMAND_SESSION_EVENT_TYPES.has(eventType)) {
    const exitCode = Number(data.exit_code ?? data.exitCode ?? payload.exit_code ?? payload.exitCode);
    const failed =
      data.success === false ||
      payload.success === false ||
      (Number.isFinite(exitCode) && exitCode !== 0) ||
      TERMINAL_FAILED_STATUSES.has(normalizeEventType(data.status ?? payload.status));
    if (eventType === 'command_session_exit' || eventType === 'command_session_summary') {
      return [buildBaseEvent(options, failed ? 'tool_call_failed' : 'tool_call_completed')];
    }
    return [buildBaseEvent(options, 'tool_call_delta')];
  }

  if (eventType === 'tool_call' || eventType === 'approval_request') {
    return [buildBaseEvent(options, 'tool_call_started')];
  }

  if (eventType === 'tool_result' || eventType === 'approval_result' || eventType === 'approval_resolved') {
    const failed =
      data.success === false ||
      payload.success === false ||
      TERMINAL_FAILED_STATUSES.has(normalizeEventType(data.status ?? payload.status));
    return [buildBaseEvent(options, failed ? 'tool_call_failed' : 'tool_call_completed')];
  }

  if (GENERIC_WORKFLOW_EVENT_TYPES.has(eventType) || WORKFLOW_EVENT_PREFIXES.some((prefix) => eventType.startsWith(prefix))) {
    return [buildBaseEvent(options, 'workflow_event')];
  }

  if (USAGE_EVENT_TYPES.has(eventType)) {
    return [buildBaseEvent(options, 'usage_stats')];
  }

  if (eventType === 'llm_output') {
    const finalContent = extractFinalContent(payload, data);
    if (!isTerminalLlmOutput(payload, data)) {
      if (!finalContent.content && !finalContent.reasoning) return [];
      return [
        buildBaseEvent(options, 'assistant_output_snapshot', {
          content: finalContent.content,
          reasoning: finalContent.reasoning
        })
      ];
    }
    return [
      buildBaseEvent(options, 'assistant_final', {
        content: finalContent.content,
        reasoning: finalContent.reasoning
      })
    ];
  }

  return [buildBaseEvent(options, 'workflow_event')];
};

export const buildCanonicalClientMessageSubmittedEvent = (payload: {
  sessionId: string;
  agentId?: string;
  content: string;
  clientMessageId: string;
  createdAt?: unknown;
  userTurnId?: string;
  attachments?: unknown[];
}): ChatRuntimeEvent => ({
  event_type: 'client_message_submitted',
  source: 'local',
  strict: false,
  session_id: payload.sessionId,
  agent_id: payload.agentId || '',
  event_id: `local:${payload.sessionId}:${payload.clientMessageId}:submitted`,
  user_turn_id: payload.userTurnId || `user-turn:${payload.sessionId}:${payload.clientMessageId}`,
  message_id: payload.clientMessageId,
  role: 'user',
  content: payload.content,
  created_at: payload.createdAt,
  payload: {
    client_message_id: payload.clientMessageId,
    ...(Array.isArray(payload.attachments) && payload.attachments.length > 0
      ? { attachments: payload.attachments }
      : {})
  }
});
