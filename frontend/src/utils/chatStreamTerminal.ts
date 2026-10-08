// A model round ending in tool calls is not the end of the user request.
// Keep this rule shared by the socket, projection and workflow consumers.
export const isTerminalLlmOutputPayload = (payload: any, data: any = null): boolean => {
  const source = data || payload?.data || payload || {};
  const calls = source.tool_calls ?? source.toolCalls ?? payload?.tool_calls ?? payload?.toolCalls;
  const reason = String(source.stop_reason ?? source.stopReason ?? source.finish_reason ??
    source.finishReason ?? payload?.stop_reason ?? payload?.stopReason ??
    payload?.finish_reason ?? payload?.finishReason ?? '').trim().toLowerCase();
  if ((Array.isArray(calls) && calls.length > 0) ||
      reason === 'tool_calls' || reason === 'function_call' || reason === 'tool_use') return false;
  return Boolean(reason) || [source.done, source.is_final, source.isFinal, source.final,
    payload?.done, payload?.is_final, payload?.isFinal, payload?.final].some(flag => flag === true);
};
