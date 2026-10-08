import { useChatStore } from '@/stores/chat';
import { applyCanonicalStreamRuntimeEvent, syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';

// Drive the production store and message panel; no model requests are needed.
export const createMessengerToolRetryProbe = (sessionId: string) => {
  const chat = useChatStore();
  const ids = { user_turn_id: 'retry-turn', model_turn_id: 'retry-model',
    assistant_message_id: 'retry-message' };
  const emit = (type: string, data: Record<string, unknown>) => {
    const seq = (chat.runtimeProjection.sessions[sessionId]?.appliedSeq || 0) + 1;
    applyCanonicalStreamRuntimeEvent(chat, sessionId, type, {
      ...ids, ...data, event_seq: seq, timestamp: new Date().toISOString()
    }, String(seq), { phase: 'watch' });
  };
  return {
    start() {
      syncChatRuntimeProjectionFromSnapshot(chat, sessionId, [
        { role: 'user', message_id: 'retry-user', user_turn_id: ids.user_turn_id, content: 'input' },
        { role: 'assistant', message_id: ids.assistant_message_id, user_turn_id: ids.user_turn_id,
          model_turn_id: ids.model_turn_id, content: '', stream_incomplete: true }
      ], { immediate: true, authoritative: true, running: true });
      emit('llm_request', {});
      emit('llm_output_delta', { reasoning_delta: 'Working' });
    },
    retry() {
      emit('bad_tool_call_retry', { attempt: 1, max_attempts: 6, delay_s: 1.2,
        will_retry: true, retry_reason: 'invalid_tool_call_arguments' });
    },
    recover() {
      emit('llm_output_delta', { delta: 'Recovered' });
    },
    finish(status: 'failed' | 'cancelled' | 'completed') {
      if (status === 'failed') emit('error', { message: 'Automatic recovery failed.' });
      emit('turn_terminal', { status });
      emit('thread_status', { status: 'idle' });
      emit('thread_closed', { status: 'not_loaded' });
    },
    busy: () => chat.isSessionBusy(sessionId)
  };
};
