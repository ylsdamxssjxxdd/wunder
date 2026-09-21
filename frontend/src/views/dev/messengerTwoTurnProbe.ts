import { nextTick } from 'vue';
import { useChatStore } from '@/stores/chat';
import { applyCanonicalClientMessageSubmittedRuntimeEvent, applyCanonicalStreamRuntimeEvent,
  syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';
import { selectVisibleMessageProjections } from '@/realtime/chat/chatRuntimeSelectors';

const frame = () => new Promise<void>(resolve => requestAnimationFrame(() => resolve()));

// Exercise the real reducer and DOM, including tool-model handoffs and refresh.
export const runMessengerTwoTurnProbe = async (sessionId: string) => {
  const chat = useChatStore();
  const gaps: number[] = [];
  let previous = performance.now();
  let maxNodes = 0;
  let peakNodeBreakdown: unknown = null;
  const sample = async () => {
    await frame();
    const now = performance.now();
    gaps.push(now - previous);
    previous = now;
    const nodes = document.querySelectorAll('*').length;
    if (nodes > maxNodes) {
      maxNodes = nodes;
      peakNodeBreakdown = {
        messages: document.querySelectorAll('.messenger-message').length,
        toolEntries: document.querySelectorAll('.tool-workflow-entry').length,
        messageNodes: document.querySelectorAll('.messenger-message-panel *').length,
        viewport: document.querySelector('[data-testid="messenger-message-list"]')?.clientHeight,
        heights: [...document.querySelectorAll<HTMLElement>('.messenger-message')].slice(0, 10).map(node => node.offsetHeight)
      };
    }
  };
  const emit = (eventType: string, payload: Record<string, unknown>) => {
    const seq = (chat.runtimeProjection.sessions[sessionId]?.appliedSeq || 0) + 1;
    applyCanonicalStreamRuntimeEvent(chat, sessionId, eventType,
      { ...payload, event_seq: seq }, String(seq), { phase: 'watch' });
  };
  const retainedWorkflowCounts: number[] = [];
  for (let turn = 1; turn <= 2; turn++) {
    const userTurnId = `user-turn:${sessionId}:round:${200 + turn}`;
    const modelTurnId = `model-turn:${sessionId}:user:${200 + turn}:model:1`;
    const messageId = `assistant:${userTurnId}`;
    const ids = { user_turn_id: userTurnId, model_turn_id: modelTurnId, assistant_message_id: messageId };
    applyCanonicalClientMessageSubmittedRuntimeEvent(chat, { sessionId,
      content: `input-${turn}`, clientMessageId: `client-${turn}`, userTurnId,
      modelTurnId, assistantMessageId: messageId });
    const list = document.querySelector<HTMLElement>('[data-testid="messenger-message-list"]');
    await nextTick();
    if (list) { list.scrollTop = list.scrollHeight; list.dispatchEvent(new Event('scroll')); }
    for (let tool = 0; tool < 80; tool++) {
      const call = { ...ids, tool: 'read_file', tool_call_id: `call-${turn}-${tool}` };
      emit('llm_output', { ...ids, finish_reason: 'tool_calls', tool_calls: [{ id: call.tool_call_id }] });
      emit('tool_call', { ...call, args: { item: tool } });
      emit('tool_output_delta', { ...call, delta: 'x'.repeat(512) });
      emit('tool_result', { ...call, result: 'x'.repeat(512), status: 'completed' });
      if (!chat.isSessionBusy(sessionId)) throw new Error('Tool round lost running status');
      await sample();
      if (tool === 0) {
        await nextTick();
        for (let attempt = 0; attempt < 12; attempt++) {
          if (document.querySelector(`[data-virtual-key="runtime:assistant:${messageId}"] .message-tool-workflow`)) break;
          await frame();
        }
        const workflow = document.querySelector<HTMLDetailsElement>(
          `[data-virtual-key="runtime:assistant:${messageId}"] .message-tool-workflow`
        );
        if (!workflow) throw new Error('Active workflow shell is missing');
        if (!workflow.open) workflow.querySelector<HTMLElement>('summary')?.click();
      }
    }
    const seed = 'stream-text '.repeat(5600);
    emit('llm_output_delta', { ...ids, delta: seed });
    for (let chunk = 0; chunk < 24; chunk++) {
      emit('llm_output_delta', { ...ids, delta: ` tail-${turn}-${chunk}` });
      const input = document.querySelector<HTMLTextAreaElement>('.messenger-agent-composer textarea');
      if (input) { input.value = `draft-${chunk}`; input.dispatchEvent(new Event('input', { bubbles: true })); }
      await sample();
    }
    await new Promise(resolve => setTimeout(resolve, 160));
    if (!list?.textContent?.includes(`tail-${turn}-23`)) throw new Error('Long stream failed to reach DOM');
    const text = chat.runtimeProjection.sessions[sessionId].messageById[messageId]?.content;
    if (!text) throw new Error('Missing streamed content');
    emit('final', { ...ids, answer: text });
    emit('thread_status', { status: 'idle' });
    if (chat.isSessionBusy(sessionId)) throw new Error('Completed turn remained busy');
    const before = selectVisibleMessageProjections(chat.runtimeProjection, sessionId);
    const transcript = before.map((message, index) => ({ role: message.role, content: message.content,
      message_id: message.id, user_turn_id: message.userTurnId, model_turn_id: message.modelTurnId,
      turn_index: index + 1, workflowItems: message.workflowItems }));
    syncChatRuntimeProjectionFromSnapshot(chat, sessionId, transcript,
      { immediate: true, running: false, loading: false, authoritative: true });
    const after = selectVisibleMessageProjections(chat.runtimeProjection, sessionId);
    if (JSON.stringify(after.map(message => message.id)) !== JSON.stringify(before.map(message => message.id))) {
      throw new Error('Refresh changed transcript order');
    }
    const retained = after.filter(message => message.id.startsWith('assistant:user-turn:') && message.workflowItems.length >= 80);
    retainedWorkflowCounts.push(retained.length);
    if (retained.length !== turn) throw new Error('Earlier workflow was discarded after refresh');
    previous = performance.now();
    for (let index = 0; index < 12; index++) {
      if (list) {
        list.scrollTop = index % 2 ? list.scrollHeight : list.scrollHeight / 2;
        list.dispatchEvent(new Event('scroll'));
      }
      await sample();
    }
  }
  return { turns: 2, toolCalls: 160, maxFrameGapMs: Math.max(...gaps), maxNodes, retainedWorkflowCounts, peakNodeBreakdown };
};
