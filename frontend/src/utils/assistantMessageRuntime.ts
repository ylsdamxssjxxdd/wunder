import { isCompactionRunningFromWorkflowItems } from './chatCompactionWorkflow';

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
