import { emptyChatThreadState, type ChatThreadFrame } from '@/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame, applyChatThreadSnapshot, composeItemText, type ChatThreadSnapshot } from '@/realtime/chat/chatThreadState';

/** Device frames reuse the chat reducer, including replay deduplication and gaps. */
export function remoteThreadModel(thread: string) {
  const state = emptyChatThreadState(thread);
  return {
    apply(frame: Record<string, any>): boolean {
      const envelope = frame.payload || {};
      if (envelope.thread_id !== thread) return false;
      const payload = envelope.payload || {};
      if (envelope.snapshot) {
        applyChatThreadSnapshot(state, payload as ChatThreadSnapshot, Date.now());
      } else if (payload.overflow) return true;
      else if (payload.event === 'thread_change') {
        const result = applyChatThreadFrame(state, { ...payload.data, event: 'thread_change', seq: envelope.seq } as ChatThreadFrame, Date.now());
        if (result.needResume) return true;
      }
      // A fresh bounded snapshot heals gaps and bounds long-running subscriptions.
      return state.items.size > 256 || state.blocks.size > 256 || [...state.blocks.values()].some(fields => [...fields.values()].some(blocks => blocks.size > 256));
    },
    text(): string {
      return [...state.items.values()].filter(row => row.visibility !== 'admin' && row.visibility !== 'model_internal' && ['user_message','assistant_message'].includes(row.kind))
        .sort((a,b) => a.order - b.order).slice(-100)
        .map(row => `${row.role === 'user' || row.kind === 'user_message' ? '›' : '◆'} ${composeItemText(state, row.itemId, 'content')}`)
        .join('\n\n').slice(-100_000);
    }
  };
}

export function remoteHistoryText(items: unknown): string {
  return (Array.isArray(items) ? items : []).slice(-100)
    .map(row => `${row.role === 'user' ? '›' : '◆'} ${typeof row.content === 'string' ? row.content : ''}`)
    .join('\n\n').slice(-100_000);
}
