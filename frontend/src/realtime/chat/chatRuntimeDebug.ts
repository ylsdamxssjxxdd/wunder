import type { ChatRuntimeProjection } from './chatRuntimeTypes';

export const exportChatRuntimeDebugSnapshot = (
  projection: ChatRuntimeProjection,
  sessionId: unknown
) => {
  const key = String(sessionId ?? '').trim();
  const session = key ? projection.sessions[key] : null;
  return {
    activeSessionId: projection.activeSessionId,
    session,
    recentEvents: projection.debugEvents.filter((event) => !key || event.sessionId === key)
  };
};
