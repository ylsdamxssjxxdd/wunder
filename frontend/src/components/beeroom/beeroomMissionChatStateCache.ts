import type {
  DispatchRuntimeStatus,
  MissionChatMessage
} from '@/components/beeroom/beeroomCanvasChatModel';

type BeeroomMissionChatDispatchState = {
  sessionId: string;
  lastEventId: number;
  targetAgentId: string;
  targetName: string;
  targetTone: MissionChatMessage['tone'];
  runtimeStatus: DispatchRuntimeStatus;
};

export type BeeroomMissionChatState = {
  version: number;
  manualMessages: MissionChatMessage[];
  runtimeRelayMessages: MissionChatMessage[];
  dispatch: BeeroomMissionChatDispatchState | null;
};

const CHAT_STATE_VERSION = 2;
const CHAT_STATE_STORAGE_KEY = 'wunder:beeroom-mission-chat-state';
const MAX_CACHE_ENTRIES = 36;
const ALLOWED_TONES = new Set(['mother', 'worker', 'system', 'user']);
const ALLOWED_RUNTIME_STATUSES = new Set([
  'idle',
  'queued',
  'running',
  'awaiting_approval',
  'resuming',
  'stopped',
  'completed',
  'failed'
]);

const missionChatStateCache = new Map<string, BeeroomMissionChatState>();
let hydrated = false;

const normalizeScopeKey = (scopeKey: unknown): string => {
  const key = String(scopeKey || '').trim();
  return key || 'standby';
};

const resolveStorage = (): Storage | null => {
  if (typeof window === 'undefined') return null;
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
};

const normalizeTone = (value: unknown): MissionChatMessage['tone'] => {
  const normalized = String(value || '').trim().toLowerCase();
  if (ALLOWED_TONES.has(normalized)) {
    return normalized as MissionChatMessage['tone'];
  }
  return 'system';
};

const normalizeRuntimeStatus = (value: unknown): DispatchRuntimeStatus => {
  const normalized = String(value || '').trim().toLowerCase();
  if (ALLOWED_RUNTIME_STATUSES.has(normalized)) {
    return normalized as DispatchRuntimeStatus;
  }
  return 'idle';
};

const normalizeTime = (value: unknown): number => {
  const time = Number(value || 0);
  if (!Number.isFinite(time) || time <= 0) return 0;
  return time;
};

const normalizeSortOrder = (value: unknown): number => {
  const normalized = Number(value || 0);
  return Number.isFinite(normalized) ? normalized : 0;
};

const cloneManualMessages = (messages: MissionChatMessage[]): MissionChatMessage[] =>
  (Array.isArray(messages) ? messages : [])
    .map((message) => {
      const key = String(message?.key || '').trim();
      const body = String(message?.body || '').trim();
      const time = normalizeTime(message?.time);
      if (!key || !body || time <= 0) return null;
      return {
        key,
        senderName: String(message?.senderName || '').trim() || 'Wunder',
        senderAgentId: String(message?.senderAgentId || '').trim(),
        avatarImageUrl: String(message?.avatarImageUrl || '').trim(),
        mention: String(message?.mention || '').trim(),
        body,
        meta: String(message?.meta || '').trim(),
        time,
        timeLabel: String(message?.timeLabel || '').trim(),
        tone: normalizeTone(message?.tone),
        sortOrder: normalizeSortOrder(message?.sortOrder)
      } satisfies MissionChatMessage;
    })
    .filter((message: MissionChatMessage | null): message is MissionChatMessage => Boolean(message))
    .slice(-120);

const cloneDispatchState = (
  state: BeeroomMissionChatDispatchState | null | undefined
): BeeroomMissionChatDispatchState | null => {
  if (!state) return null;
  const sessionId = String(state.sessionId || '').trim();
  if (!sessionId) return null;
  return {
    sessionId,
    lastEventId: Math.max(0, Math.floor(Number(state.lastEventId || 0))),
    targetAgentId: String(state.targetAgentId || '').trim(),
    targetName: String(state.targetName || '').trim(),
    targetTone: normalizeTone(state.targetTone),
    runtimeStatus: normalizeRuntimeStatus(state.runtimeStatus)
  };
};

const normalizeState = (
  state: Partial<BeeroomMissionChatState> | null | undefined
): BeeroomMissionChatState => ({
  version: CHAT_STATE_VERSION,
  manualMessages: cloneManualMessages(state?.manualMessages || []),
  runtimeRelayMessages: cloneManualMessages(state?.runtimeRelayMessages || []),
  dispatch: cloneDispatchState(state?.dispatch || null)
});

const cloneState = (state: BeeroomMissionChatState): BeeroomMissionChatState => ({
  version: CHAT_STATE_VERSION,
  manualMessages: cloneManualMessages(state.manualMessages),
  runtimeRelayMessages: cloneManualMessages(state.runtimeRelayMessages),
  dispatch: cloneDispatchState(state.dispatch)
});

const persistCache = () => {
  const storage = resolveStorage();
  if (!storage) return;
  try {
    storage.setItem(
      CHAT_STATE_STORAGE_KEY,
      JSON.stringify({
        version: CHAT_STATE_VERSION,
        entries: Array.from(missionChatStateCache.entries()).map(([scopeKey, state]) => [
          scopeKey,
          cloneState(state)
        ])
      })
    );
  } catch {
    // Ignore privacy-mode and quota failures.
  }
};

const hydrateCache = () => {
  if (hydrated) return;
  hydrated = true;
  const storage = resolveStorage();
  if (!storage) return;
  try {
    const raw = storage.getItem(CHAT_STATE_STORAGE_KEY);
    if (!raw) return;
    const payload = JSON.parse(raw) as {
      entries?: Array<[unknown, Partial<BeeroomMissionChatState>]>;
    } | null;
    const entries = Array.isArray(payload?.entries) ? payload.entries : [];
    entries.forEach((entry) => {
      if (!Array.isArray(entry) || entry.length < 2) return;
      missionChatStateCache.set(normalizeScopeKey(entry[0]), normalizeState(entry[1]));
    });
    while (missionChatStateCache.size > MAX_CACHE_ENTRIES) {
      const oldest = missionChatStateCache.keys().next();
      if (oldest.done) break;
      missionChatStateCache.delete(oldest.value);
    }
  } catch {
    try {
      storage.removeItem(CHAT_STATE_STORAGE_KEY);
    } catch {
      // Ignore cleanup failures.
    }
  }
};

export const getBeeroomMissionChatState = (scopeKey: unknown): BeeroomMissionChatState | null => {
  hydrateCache();
  const key = normalizeScopeKey(scopeKey);
  const hit = missionChatStateCache.get(key);
  if (!hit) return null;
  missionChatStateCache.delete(key);
  missionChatStateCache.set(key, hit);
  persistCache();
  return cloneState(hit);
};

export const setBeeroomMissionChatState = (
  scopeKey: unknown,
  state: BeeroomMissionChatState | null | undefined
) => {
  hydrateCache();
  const key = normalizeScopeKey(scopeKey);
  const next = normalizeState(state || null);
  if (next.manualMessages.length === 0 && next.runtimeRelayMessages.length === 0 && !next.dispatch) {
    missionChatStateCache.delete(key);
  } else {
    missionChatStateCache.set(key, next);
  }
  while (missionChatStateCache.size > MAX_CACHE_ENTRIES) {
    const oldest = missionChatStateCache.keys().next();
    if (oldest.done) break;
    missionChatStateCache.delete(oldest.value);
  }
  persistCache();
};
