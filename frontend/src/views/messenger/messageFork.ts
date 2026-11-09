export const resolveAgentMessageForkRound = (message: Record<string, unknown> | null | undefined): number | null => {
  const candidates = [
    message?.user_round,
    message?.userRound,
    message?.user_turn_index,
    message?.__runtime_user_round
  ];
  for (const candidate of candidates) {
    const round = Number(candidate);
    if (Number.isSafeInteger(round) && round > 0) return round;
  }
  return null;
};

export const canForkAgentMessage = (message: Record<string, unknown> | null | undefined): boolean => {
  if (String(message?.role || '') !== 'assistant') return false;
  if (
    message?.streaming === true ||
    message?.stream_incomplete === true ||
    message?.workflowStreaming === true ||
    message?.reasoningStreaming === true ||
    ['accepted', 'running', 'queued', 'waiting_input', 'waiting_approval'].includes(String(message?.status || ''))
  ) {
    return false;
  }
  return resolveAgentMessageForkRound(message) !== null;
};
