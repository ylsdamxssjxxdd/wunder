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
  message: AssistantMessageLike | null | undefined
): AssistantMessageRuntimeState => {
  if (!message || message.role !== 'assistant') return 'idle';
  const pendingQuestion = hasAssistantPendingQuestion(message);
  if (pendingQuestion) return 'pending';
  if (isAssistantMessageRunning(message)) return 'running';
  const normalized = normalizeAssistantMessageRuntimeState(message.state, pendingQuestion);
  return normalized === 'idle' ? 'done' : normalized;
};
