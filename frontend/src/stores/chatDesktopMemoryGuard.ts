import { isDesktopModeEnabled } from '@/config/desktop';
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';

const DESKTOP_RETAINED_SESSION_LIMIT = 3;
const DESKTOP_BACKGROUND_MESSAGE_LIMIT = 80;

const sessionAccessAt = new Map<string, number>();

type SessionMap = Map<string, unknown>;

type DesktopChatMemoryCacheOptions = {
  activeSessionId?: string | null;
  sessionMessages: Map<string, unknown[]>;
  sessionDetailSnapshotCache: SessionMap;
  sessionEventsSnapshotCache: SessionMap;
  sessionHydratedMessageVersion: SessionMap;
  sessionDetailWarmState: SessionMap;
  sessionHistoryState: SessionMap;
  sessionWorkflowState: SessionMap;
  sessionProtectedRealtimeMessages: SessionMap;
  sessionSubagentsCache: SessionMap;
  sessionRuntimeShadowState: SessionMap;
  runtimeProjection?: ChatRuntimeProjection | null;
  isHotSession?: (sessionId: string) => boolean;
};

export const shouldUseDesktopChatMemoryGuard = (): boolean => isDesktopModeEnabled();

export const normalizeDesktopSessionKey = (sessionId: unknown): string =>
  String(sessionId || '').trim();

export const touchDesktopChatSession = (sessionId: unknown): void => {
  if (!shouldUseDesktopChatMemoryGuard()) return;
  const key = normalizeDesktopSessionKey(sessionId);
  if (!key) return;
  sessionAccessAt.set(key, Date.now());
};

const oldestHistoryIdFromMessages = (messages: unknown[]): number | null => {
  for (const message of messages) {
    const record = message && typeof message === 'object'
      ? message as Record<string, unknown>
      : null;
    const historyId = Number.parseInt(String(record?.history_id ?? ''), 10);
    if (Number.isFinite(historyId) && historyId > 0) {
      return historyId;
    }
  }
  return null;
};

const trimDesktopBackgroundMessages = (
  sessionId: string,
  activeSessionId: string,
  messages: unknown[],
  historyState: SessionMap
): void => {
  if (!sessionId || sessionId === activeSessionId) return;
  if (!Array.isArray(messages) || messages.length <= DESKTOP_BACKGROUND_MESSAGE_LIMIT) return;
  const overflow = messages.length - DESKTOP_BACKGROUND_MESSAGE_LIMIT;
  messages.splice(0, overflow);
  const beforeId = oldestHistoryIdFromMessages(messages);
  const state = historyState.get(sessionId);
  if (state && typeof state === 'object') {
    Object.assign(state as Record<string, unknown>, {
      beforeId,
      hasMore: Boolean(beforeId)
    });
  }
};

const deleteEventsForSession = (
  cache: SessionMap,
  sessionId: string
): void => {
  for (const key of Array.from(cache.keys())) {
    if (key === sessionId || key.startsWith(`${sessionId}|`)) {
      cache.delete(key);
    }
  }
};

const collectSessionKeys = (options: DesktopChatMemoryCacheOptions): Set<string> => {
  const keys = new Set<string>();
  const addMapKeys = (map: SessionMap | Map<string, unknown[]>): void => {
    for (const key of map.keys()) {
      const baseKey = String(key || '').split('|')[0]?.trim();
      if (baseKey) keys.add(baseKey);
    }
  };
  addMapKeys(options.sessionMessages);
  addMapKeys(options.sessionDetailSnapshotCache);
  addMapKeys(options.sessionEventsSnapshotCache);
  addMapKeys(options.sessionHydratedMessageVersion);
  addMapKeys(options.sessionDetailWarmState);
  addMapKeys(options.sessionHistoryState);
  addMapKeys(options.sessionWorkflowState);
  addMapKeys(options.sessionProtectedRealtimeMessages);
  addMapKeys(options.sessionSubagentsCache);
  addMapKeys(options.sessionRuntimeShadowState);
  Object.keys(options.runtimeProjection?.sessions || {}).forEach((key) => {
    const normalized = normalizeDesktopSessionKey(key);
    if (normalized) keys.add(normalized);
  });
  return keys;
};

export const pruneDesktopChatMemoryCaches = (
  options: DesktopChatMemoryCacheOptions
): void => {
  if (!shouldUseDesktopChatMemoryGuard()) return;
  const activeSessionId = normalizeDesktopSessionKey(options.activeSessionId);
  if (activeSessionId) {
    touchDesktopChatSession(activeSessionId);
  }

  for (const [sessionId, messages] of options.sessionMessages.entries()) {
    trimDesktopBackgroundMessages(
      sessionId,
      activeSessionId,
      messages,
      options.sessionHistoryState
    );
  }

  const hotSessionIds = new Set<string>();
  const allSessionIds = collectSessionKeys(options);
  for (const sessionId of allSessionIds) {
    if (sessionId === activeSessionId || options.isHotSession?.(sessionId)) {
      hotSessionIds.add(sessionId);
    }
  }

  const retained = new Set<string>(hotSessionIds);
  const candidates = Array.from(allSessionIds)
    .filter((sessionId) => !retained.has(sessionId))
    .sort((left, right) =>
      (sessionAccessAt.get(right) || 0) - (sessionAccessAt.get(left) || 0)
    );
  for (const sessionId of candidates) {
    if (retained.size >= DESKTOP_RETAINED_SESSION_LIMIT) break;
    retained.add(sessionId);
  }

  for (const sessionId of allSessionIds) {
    if (retained.has(sessionId)) continue;
    options.sessionMessages.delete(sessionId);
    options.sessionDetailSnapshotCache.delete(sessionId);
    options.sessionHydratedMessageVersion.delete(sessionId);
    options.sessionDetailWarmState.delete(sessionId);
    options.sessionHistoryState.delete(sessionId);
    options.sessionWorkflowState.delete(sessionId);
    options.sessionProtectedRealtimeMessages.delete(sessionId);
    options.sessionSubagentsCache.delete(sessionId);
    options.sessionRuntimeShadowState.delete(sessionId);
    deleteEventsForSession(options.sessionEventsSnapshotCache, sessionId);
    delete options.runtimeProjection?.sessions?.[sessionId];
    sessionAccessAt.delete(sessionId);
  }
};
