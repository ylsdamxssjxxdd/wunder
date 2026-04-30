import type { ChatRuntimeEvent, ChatRuntimeProjection } from './chatRuntimeTypes';
import { applyChatRuntimeEvent } from './chatRuntimeReducer';

export const replayChatRuntimeEvents = (
  projection: ChatRuntimeProjection,
  events: ChatRuntimeEvent[]
): ChatRuntimeProjection => {
  events.forEach((event) => {
    applyChatRuntimeEvent(projection, event);
  });
  return projection;
};

export const buildLegacyMessagesReconciledEvent = (payload: {
  sessionId: string;
  agentId?: string;
  messages: Record<string, unknown>[];
  loading?: boolean;
  running?: boolean;
  eventSeq?: number;
}): ChatRuntimeEvent => ({
  event_type: 'legacy_messages_reconciled',
  source: 'legacy',
  strict: false,
  session_id: payload.sessionId,
  agent_id: payload.agentId || '',
  event_seq: payload.eventSeq,
  messages: payload.messages,
  loading: payload.loading,
  running: payload.running
});
