const STORAGE_KEY = 'e2e:workflow-history';

export const enableWorkflowHistoryFixture = () => sessionStorage.setItem(STORAGE_KEY, '1');

// Rehydrate the same transcript on a real browser reload, including every call's arguments.
export const readWorkflowHistoryFixture = (sessionId: string) => {
  if (sessionStorage.getItem(STORAGE_KEY) !== '1') return null;
  return Array.from({ length: 4 }, (_, index) => ({
    id: `workflow-message-${index}`,
    message_id: `workflow-message-${index}`,
    user_turn_id: `workflow-turn-${Math.floor(index / 2)}`,
    model_turn_id: index % 2 ? `workflow-model-${index}` : undefined,
    user_round: Math.floor(index / 2) + 1,
    turn_index: index + 1,
    role: index % 2 ? 'assistant' : 'user',
    content: `message-${index}`,
    created_at: new Date(Date.UTC(2026, 0, 1, 0, 0, index)).toISOString(),
    workflowItems: index % 2 ? Array.from({ length: 260 }, (_, item) => {
      const toolCallId = `${sessionId}-call-${index}-${item}`;
      const toolCallRawDetail = JSON.stringify({ args: { item } });
      return [
        { id: `${toolCallId}:call`, toolCallId, toolName: 'read_file', eventType: 'tool_call',
          status: 'completed', detail: toolCallRawDetail, toolCallRawDetail },
        { id: `${toolCallId}:result`, toolCallId, toolName: 'read_file', eventType: 'tool_result',
          status: 'completed', detail: item === 259
            ? JSON.stringify({ result: { data: { content: 'worker-preview-text\n'.repeat(20000) } } })
            : 'x'.repeat(512), toolCallRawDetail }
      ];
    }).flat() : []
  }));
};
