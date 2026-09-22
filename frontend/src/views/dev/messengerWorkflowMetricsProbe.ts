import { useChatStore } from '@/stores/chat';
import { applyCanonicalStreamRuntimeEvent, syncChatRuntimeProjectionFromSnapshot } from '@/stores/chatRuntimeState';

export const createMessengerWorkflowMetricsProbe = (sessionId: string) => {
  const chat = useChatStore();
  const ids = {user_turn_id:'metrics_turn',model_turn_id:'metrics_model',assistant_message_id:'metrics_message'};
  const emit = (type: string, data: Record<string, unknown>) => {
    const seq = (chat.runtimeProjection.sessions[sessionId]?.appliedSeq || 0) + 1;
    applyCanonicalStreamRuntimeEvent(chat, sessionId, type, {
      ...ids, ...data, event_seq:seq, timestamp:new Date().toISOString()
    }, String(seq), {phase:'watch'});
  };
  return {
    start() {
      syncChatRuntimeProjectionFromSnapshot(chat, sessionId, [
        {role:'user',message_id:'metrics_user',user_turn_id:ids.user_turn_id,content:'input'},
        {role:'assistant',message_id:ids.assistant_message_id,user_turn_id:ids.user_turn_id,
          model_turn_id:ids.model_turn_id,content:'',stream_incomplete:true}
      ], {immediate:true,authoritative:true,running:true});
      emit('llm_request', {});
      emit('context_usage', {context_tokens:500,max_context:1000});
      emit('tool_call', {tool:'read_file',tool_call_id:'metrics_call',request_context_tokens:500,args:{}});
      emit('tool_result', {tool:'read_file',tool_call_id:'metrics_call',ok:false,meta:{duration_ms:0},
        data:{count:999,duration_ms:777,context_tokens:888}});
    },
    compact() {
      emit('compaction', {status:'done',compaction_id:'metrics_compaction',final_context_tokens:0});
      emit('context_usage', {context_tokens:0});
    },
    observe() { emit('context_usage', {context_tokens:120,max_context:1000}); },
    delta() { emit('llm_output_delta', {delta:'text'}); }
  };
};
