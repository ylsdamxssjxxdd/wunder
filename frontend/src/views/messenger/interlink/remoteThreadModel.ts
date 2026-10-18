import { emptyChatThreadState, type ChatThreadFrame } from '@/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame, applyChatThreadSnapshot, composeItemText, type ChatThreadSnapshot } from '@/realtime/chat/chatThreadState';

export type RemoteThreadItem = { id: string; role: 'user' | 'assistant'; content: string };

function visibleItems(state: ReturnType<typeof emptyChatThreadState>): RemoteThreadItem[] {
  return [...state.items.values()]
    .filter(row => row.visibility !== 'admin' && row.visibility !== 'model_internal' && ['user_message', 'assistant_message'].includes(row.kind))
    .sort((a, b) => a.order - b.order)
    .slice(-100)
    .map(row => ({
      id: row.itemId,
      role: row.role === 'user' || row.kind === 'user_message' ? 'user' : 'assistant',
      content: composeItemText(state, row.itemId, 'content')
    }));
}

/** Device frames reuse the chat reducer, including replay deduplication and gaps. */
export function remoteThreadModel(thread: string) {
  const state = emptyChatThreadState(thread);
  return {
    apply(frame: Record<string, any>): boolean {
      const envelope = frame;
      if (envelope.thread_id !== thread) return false;
      const payload = envelope.payload || {};
      if (frame.type === 'snapshot') {
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
      return visibleItems(state)
        .map(row => `${row.role === 'user' ? '›' : '◆'} ${row.content}`)
        .join('\n\n').slice(-100_000);
    },
    items(): RemoteThreadItem[] { return visibleItems(state); }
  };
}

export function remoteHistoryItems(items: unknown): RemoteThreadItem[] {
  return (Array.isArray(items) ? items : []).slice(-100).map((value, index) => {
    const row = value && typeof value === 'object' ? value as Record<string, unknown> : {};
    const role: RemoteThreadItem['role'] = String(row.role || '').toLowerCase() === 'user' ? 'user' : 'assistant';
    return { id: String(row.item_id || row.id || `${role}:${index}`), role, content: typeof row.content === 'string' ? row.content : '' };
  }).filter(row => row.content.trim());
}
