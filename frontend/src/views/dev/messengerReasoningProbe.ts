import { nextTick } from 'vue';
import { useChatStore } from '@/stores/chat';
import { applyCanonicalStreamRuntimeEvent, syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';
import { chatPerf } from '@/utils/chatPerf';

const settle = (ms = 200) => new Promise(resolve => setTimeout(resolve, ms));

// Use the real message panel, raw projection and throttled clocks across hydration.
export const runMessengerReasoningProbe = async (sessionId: string) => {
  const chat = useChatStore();
  const ids = { user_turn_id: 'reasoning-turn', model_turn_id: 'reasoning-model',
    assistant_message_id: 'reasoning-message' };
  const emit = (type: string, data: Record<string, unknown>) => {
    const seq = (chat.runtimeProjection.sessions[sessionId]?.appliedSeq || 0) + 1;
    applyCanonicalStreamRuntimeEvent(chat, sessionId, type, { ...ids, ...data, event_seq: seq },
      String(seq), { phase: 'watch' });
  };
  syncChatRuntimeProjectionFromSnapshot(chat, sessionId, [
    { role: 'user', message_id: 'reasoning-user', user_turn_id: ids.user_turn_id, content: 'input' },
    { role: 'assistant', message_id: ids.assistant_message_id, user_turn_id: ids.user_turn_id,
      model_turn_id: ids.model_turn_id, content: 'output', stream_incomplete: true }
  ], { immediate: true, authoritative: true, running: true });
  emit('llm_output_delta', { delta: ' start' });
  await settle();
  // The first reasoning chunk after visible content must mount the thinking row.
  emit('llm_output_delta', { reasoning_delta: 'reasoning '.repeat(12000) });
  await settle();
  if (!document.querySelector('.message-thinking-track')) throw new Error('Thinking row failed to mount');
  chatPerf.start();
  for (let index = 0; index < 100; index++) {
    emit('llm_output_delta', { reasoning_delta: ` chunk-${index}` });
    await settle(5);
  }
  await settle();
  const preview = document.querySelector('.message-thinking-track')?.textContent || '';
  const report = chatPerf.snapshot();
  chatPerf.stop();
  if (!preview.includes('chunk-99')) throw new Error('Reasoning clock failed to update DOM');
  emit('final', { answer: 'completed', visible_decode_speed_tps: 70 });
  emit('thread_status', { status: 'idle' });
  await settle();
  await nextTick();
  return { previewLength: preview.length,
    panelUpdates: report.summary.rendering.messagePanelUpdates,
    contentFlushes: report.durations.chat_stream_plain_text_flush?.count || 0,
    busy: chat.isSessionBusy(sessionId) };
};
