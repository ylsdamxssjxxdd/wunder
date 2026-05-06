import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';
import { normalizeChatTimestampMs } from './chatTiming';

type AssistantMessageLike = Record<string, unknown>;

export type AssistantMessageRuntimeState = 'idle' | 'running' | 'pending' | 'done' | 'error';

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};

const normalizeRuntimeText = (value: unknown): string => String(value || '').trim().toLowerCase();

const ACTIVE_WORKFLOW_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);

const hasVisibleAssistantOutput = (message: AssistantMessageLike | null | undefined): boolean =>
  Boolean(String(message?.content || '').trim()) || Boolean(String(message?.reasoning || '').trim());

const normalizeTimestampMs = (value: unknown): number | null => {
  const millis = normalizeChatTimestampMs(value);
  return Number.isFinite(millis) ? Number(millis) : null;
};

const resolveLatestWorkflowStatus = (workflowItems: unknown): string => {
  const items = Array.isArray(workflowItems) ? workflowItems : [];
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (!item || typeof item !== 'object') continue;
    const status = normalizeRuntimeText((item as AssistantMessageLike).status);
    if (status) {
      return status;
    }
  }
  return '';
};

const resolveMessageIndex = (
  message: AssistantMessageLike,
  allMessages: AssistantMessageLike[]
): number => {
  const directIndex = allMessages.lastIndexOf(message);
  if (directIndex >= 0) return directIndex;
  const role = String(message?.role || '');
  const createdAt = String(message?.created_at || '');
  const streamEventId = String(message?.stream_event_id || '');
  const streamRound = String(message?.stream_round || '');
  for (let index = allMessages.length - 1; index >= 0; index -= 1) {
    const item = allMessages[index];
    if (!item || typeof item !== 'object') continue;
    if (
      String(item?.role || '') === role &&
      String(item?.created_at || '') === createdAt &&
      String(item?.stream_event_id || '') === streamEventId &&
      String(item?.stream_round || '') === streamRound
    ) {
      return index;
    }
  }
  return -1;
};

const hasAssistantTerminalSignals = (message: AssistantMessageLike): boolean => {
  const explicitState = normalizeAssistantMessageRuntimeState(message.state, false);
  if (explicitState === 'done' || explicitState === 'error' || explicitState === 'pending') {
    return true;
  }
  if (normalizeFlag(message.resume_available) || hasAssistantPendingQuestion(message)) {
    return true;
  }
  if (String(message.stop_reason ?? message.stopReason ?? '').trim()) {
    return true;
  }
  const stats = message.stats && typeof message.stats === 'object'
    ? (message.stats as AssistantMessageLike)
    : null;
  const interactionEndMs = normalizeTimestampMs(
    stats?.interaction_end_ms ??
      stats?.interactionEndMs ??
      stats?.interaction_end ??
      stats?.ended_at
  );
  if (interactionEndMs !== null && interactionEndMs > 0) {
    return true;
  }
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return items.some((item) => {
    if (!item || typeof item !== 'object') return false;
    const record = item as AssistantMessageLike;
    const status = normalizeRuntimeText(record.status);
    const eventType = normalizeRuntimeText(record.eventType ?? record.event);
    return (
      eventType === 'final' ||
      eventType === 'turn_terminal' ||
      eventType === 'error' ||
      eventType === 'request_failed' ||
      status === 'failed' ||
      status === 'error'
    );
  });
};

export const isLatestAssistantPlaceholderWaiting = (
  message: AssistantMessageLike | null | undefined,
  allMessages?: AssistantMessageLike[] | null
): boolean => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  if (!Array.isArray(allMessages) || allMessages.length === 0) return false;
  const currentIndex = resolveMessageIndex(message, allMessages);
  if (currentIndex < 0) return false;
  for (let index = currentIndex + 1; index < allMessages.length; index += 1) {
    const item = allMessages[index];
    if (!item || item.isGreeting || item.hiddenInternal) continue;
    const role = String(item.role || '').trim().toLowerCase();
    if (role === 'assistant' || role === 'user') return false;
  }
  let previousVisibleRole = '';
  for (let index = currentIndex - 1; index >= 0; index -= 1) {
    const item = allMessages[index];
    if (!item || item.isGreeting || item.hiddenInternal) continue;
    previousVisibleRole = String(item.role || '').trim().toLowerCase();
    break;
  }
  if (previousVisibleRole !== 'user') return false;
  if (hasVisibleAssistantOutput(message)) return false;
  if (isAssistantMessageRunning(message) || hasAssistantWaitingForCurrentOutput(message)) return false;
  if (hasAssistantTerminalSignals(message)) return false;
  const workflowItems = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  return workflowItems.length === 0 || workflowItems.every((item) => {
    if (!item || typeof item !== 'object') return true;
    const record = item as AssistantMessageLike;
    const eventType = normalizeRuntimeText(record.eventType ?? record.event);
    const status = normalizeRuntimeText(record.status);
    return !eventType && (!status || status === 'completed');
  });
};

export const hasAssistantPendingQuestion = (
  message: AssistantMessageLike | null | undefined
): boolean => {
  if (!message || message.role !== 'assistant') return false;
  const panelStatus = normalizeRuntimeText((message.questionPanel as AssistantMessageLike | null)?.status);
  return (
    panelStatus === 'pending' ||
    normalizeFlag(message.pendingQuestion) ||
    normalizeFlag(message.pending_question) ||
    normalizeFlag(message.awaiting_confirmation) ||
    normalizeFlag(message.requires_confirmation)
  );
};

export const hasAssistantWaitingForCurrentOutput = (
  message: AssistantMessageLike | null | undefined
): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (hasAssistantPendingQuestion(message) || hasVisibleAssistantOutput(message)) return false;

  const waitingUpdatedAtMs = normalizeTimestampMs(
    message.waiting_updated_at_ms ??
      message.waitingUpdatedAtMs ??
      (message.stats as AssistantMessageLike | null)?.interaction_start_ms ??
      (message.stats as AssistantMessageLike | null)?.interactionStartMs
  );
  if (waitingUpdatedAtMs === null || waitingUpdatedAtMs <= 0) {
    return false;
  }

  const waitingFirstOutputAtMs = normalizeTimestampMs(
    message.waiting_phase_first_output_at_ms ??
      message.waitingPhaseFirstOutputAtMs ??
      message.waiting_first_output_at_ms ??
      message.waitingFirstOutputAtMs
  );
  if (waitingFirstOutputAtMs !== null && waitingFirstOutputAtMs > 0) {
    return false;
  }

  const interactionEndMs = normalizeTimestampMs(
    (message.stats as AssistantMessageLike | null)?.interaction_end_ms ??
      (message.stats as AssistantMessageLike | null)?.interactionEndMs ??
      (message.stats as AssistantMessageLike | null)?.interaction_end ??
      (message.stats as AssistantMessageLike | null)?.ended_at
  );
  if (!isAssistantMessageRunning(message) && interactionEndMs !== null && interactionEndMs >= waitingUpdatedAtMs) {
    return false;
  }

  const explicitState = normalizeAssistantMessageRuntimeState(message.state, false);
  if (explicitState === 'done' || explicitState === 'error' || explicitState === 'pending') {
    return false;
  }

  const latestWorkflowStatus = resolveLatestWorkflowStatus(message.workflowItems);
  if (
    latestWorkflowStatus &&
    !ACTIVE_WORKFLOW_STATUSES.has(latestWorkflowStatus) &&
    !isAssistantMessageRunning(message)
  ) {
    return false;
  }

  return true;
};

export const normalizeAssistantMessageRuntimeState = (
  state: unknown,
  pendingQuestion = false
): AssistantMessageRuntimeState => {
  const raw = normalizeRuntimeText(state);
  if (
    pendingQuestion ||
    raw === 'pending_question' ||
    raw === 'pending-question' ||
    raw === 'pending_confirm' ||
    raw === 'pending-confirm' ||
    raw === 'pending_confirmation' ||
    raw === 'awaiting_confirmation' ||
    raw === 'awaiting-confirmation' ||
    raw === 'awaiting_approval' ||
    raw === 'awaiting-approval' ||
    raw === 'approval_pending' ||
    raw === 'approval-pending' ||
    raw === 'pending' ||
    raw === 'waiting' ||
    raw === 'queued' ||
    raw === 'await_confirm' ||
    raw === 'question' ||
    raw === 'questioning' ||
    raw === 'asking'
  ) {
    return 'pending';
  }
  if (raw === 'running' || raw === 'executing' || raw === 'processing' || raw === 'cancelling') {
    return 'running';
  }
  if (
    raw === 'done' ||
    raw === 'completed' ||
    raw === 'complete' ||
    raw === 'finish' ||
    raw === 'finished' ||
    raw === 'success' ||
    raw === 'succeeded'
  ) {
    return 'done';
  }
  if (
    raw === 'error' ||
    raw === 'failed' ||
    raw === 'timeout' ||
    raw === 'aborted' ||
    raw === 'terminated' ||
    raw === 'cancelled' ||
    raw === 'canceled'
  ) {
    return 'error';
  }
  return 'idle';
};

export const isAssistantMessageRunning = (
  message: AssistantMessageLike | null | undefined
): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (
    normalizeFlag(message.stream_incomplete) ||
    normalizeFlag(message.workflowStreaming) ||
    normalizeFlag(message.reasoningStreaming)
  ) {
    return true;
  }
  return isCompactionRunningFromWorkflowItems(message.workflowItems);
};

export const resolveAssistantMessageRuntimeState = (
  message: AssistantMessageLike | null | undefined,
  allMessages?: AssistantMessageLike[] | null
): AssistantMessageRuntimeState => {
  if (!message || message.role !== 'assistant') return 'idle';
  const pendingQuestion = hasAssistantPendingQuestion(message);
  if (pendingQuestion) return 'pending';
  if (isAssistantMessageRunning(message)) return 'running';
  if (isLatestAssistantPlaceholderWaiting(message, allMessages)) return 'running';
  const normalized = normalizeAssistantMessageRuntimeState(message.state, pendingQuestion);
  return normalized === 'idle' ? 'done' : normalized;
};
