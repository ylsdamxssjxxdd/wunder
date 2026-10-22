import { emptyChatThreadState, type ChatThreadFrame } from '@/realtime/chat/chatThreadTypes';
import { applyChatThreadFrame, applyChatThreadSnapshot, composeItemText, type ChatThreadSnapshot } from '@/realtime/chat/chatThreadState';
import type { ChatThreadState } from '@/realtime/chat/chatThreadTypes';

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

/** Device frames reuse the chat reducer, including sparse cursors and replay deduplication. */
export function remoteThreadModel(thread: string) {
  const state = emptyChatThreadState(thread);
  return {
    state(): ChatThreadState { return state; },
    replace(snapshot: unknown): boolean {
      if (!snapshot || typeof snapshot !== 'object') return false;
      const result = applyChatThreadSnapshot(state, snapshot as ChatThreadSnapshot, Date.now());
      return result.changed;
    },
    apply(frame: Record<string, any>): boolean {
      const envelope = frame;
      if (envelope.thread_id !== thread) return false;
      const payload = envelope.payload || {};
      if (frame.type === 'snapshot' || envelope.snapshot === true) {
        const snapshot = payload;
        return applyChatThreadSnapshot(state, snapshot as ChatThreadSnapshot, Date.now()).changed;
      } else if (payload.overflow) return true;
      else if (payload.event === 'thread_change') {
        const result = applyChatThreadFrame(state, { ...payload.data, event: 'thread_change', seq: envelope.seq } as ChatThreadFrame, Date.now());
        if (result.changed) return true;
      }
      // A fresh bounded snapshot recovers overflow and bounds long-running subscriptions.
      return state.items.size > 256;
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
