// Keep live events and persisted workflow replay on the same recovery semantics.
const RECOVERY_STAGES = new Set(['invalid_tool_call_reroute', 'empty_final_answer_reroute']);

export const isChatRetryEventType = (value: unknown): boolean =>
  ['llm_stream_retry', 'bad_tool_call_retry', 'model_recovery'].includes(String(value || '').toLowerCase());

export const resolveChatRetryEvent = (
  eventType: string,
  source: Record<string, unknown>
): { reason: string; willRetry: boolean } | null => {
  const stage = String(source.stage || '');
  if (eventType === 'progress' && RECOVERY_STAGES.has(stage)) {
    return { reason: stage, willRetry: true };
  }
  if (!isChatRetryEventType(eventType)) return null;
  return {
    reason: String(source.retry_reason ?? source.retryReason ??
      (eventType === 'bad_tool_call_retry' ? 'invalid_tool_call_arguments' : '')),
    willRetry: source.will_retry !== false
  };
};

export const isModelRecoveryReason = (value: unknown): boolean =>
  value === 'invalid_tool_call_arguments' || RECOVERY_STAGES.has(String(value || ''));

export const continuesRecoveryOnModelRequest = (value: unknown): boolean =>
  RECOVERY_STAGES.has(String(value || ''));
