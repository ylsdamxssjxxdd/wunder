export type BeeroomCanvasPositionOverride = {
  x: number;
  y: number;
};

export type BeeroomCanvasViewportState = {
  scale: number;
  offsetX: number;
  offsetY: number;
};

export type BeeroomMissionCanvasState = {
  version: number;
  nodePositionOverrides: Record<string, BeeroomCanvasPositionOverride>;
  activeNodeId: string;
  chatCollapsed: boolean;
  chatWidth: number;
  timelineCollapsed: boolean;
  chatClearedAfter: number;
  viewport: BeeroomCanvasViewportState | null;
};

const MAX_CACHE_ENTRIES = 48;
const MISSION_CANVAS_STATE_VERSION = 5;
const MISSION_CANVAS_STATE_STORAGE_KEY = 'wunder:beeroom-mission-canvas-state';
const missionCanvasStateCache = new Map<string, BeeroomMissionCanvasState>();
let missionCanvasStateHydrated = false;

const normalizeScopeKey = (scopeKey: unknown) => {
  const key = String(scopeKey || '').trim();
  return key || 'standby';
};

const resolveStateStorage = (): Storage | null => {
  if (typeof window === 'undefined') return null;
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
};

const cloneNodePositionOverrides = (source: Record<string, BeeroomCanvasPositionOverride>) => {
  const result: Record<string, BeeroomCanvasPositionOverride> = {};
  Object.entries(source || {}).forEach(([nodeId, override]) => {
    const id = String(nodeId || '').trim();
    if (!id) return;
    const x = Number(override?.x);
    const y = Number(override?.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    result[id] = { x, y };
  });
  return result;
};

const cloneViewport = (viewport: BeeroomCanvasViewportState | null) => {
  if (!viewport) return null;
  const scale = Number((viewport as Partial<BeeroomCanvasViewportState>).scale);
  const offsetX = Number((viewport as Partial<BeeroomCanvasViewportState>).offsetX);
  const offsetY = Number((viewport as Partial<BeeroomCanvasViewportState>).offsetY);
  if (!Number.isFinite(scale) || !Number.isFinite(offsetX) || !Number.isFinite(offsetY)) {
    return null;
  }
  return {
    scale,
    offsetX,
    offsetY
  };
};

const normalizeChatClearedAfter = (value: unknown) => {
  const timestamp = Number(value || 0);
  if (!Number.isFinite(timestamp) || timestamp <= 0) return 0;
  return timestamp;
};

const normalizeChatWidth = (value: unknown) => {
  const width = Math.round(Number(value || 0));
  if (!Number.isFinite(width) || width <= 0) return 0;
  return width;
};

const cloneState = (state: BeeroomMissionCanvasState): BeeroomMissionCanvasState => ({
  version: MISSION_CANVAS_STATE_VERSION,
  nodePositionOverrides: cloneNodePositionOverrides(state.nodePositionOverrides),
  activeNodeId: String(state.activeNodeId || '').trim(),
  chatCollapsed: !!state.chatCollapsed,
  chatWidth: normalizeChatWidth(state.chatWidth),
  timelineCollapsed: !!state.timelineCollapsed,
  chatClearedAfter: normalizeChatClearedAfter(state.chatClearedAfter),
  viewport: cloneViewport(state.viewport)
});

const normalizeState = (state: Partial<BeeroomMissionCanvasState> | null | undefined): BeeroomMissionCanvasState => ({
  version: MISSION_CANVAS_STATE_VERSION,
  nodePositionOverrides: cloneNodePositionOverrides(state?.nodePositionOverrides || {}),
  activeNodeId: String(state?.activeNodeId || '').trim(),
  chatCollapsed: !!state?.chatCollapsed,
  chatWidth: normalizeChatWidth(state?.chatWidth),
  timelineCollapsed: !!state?.timelineCollapsed,
  chatClearedAfter: normalizeChatClearedAfter(state?.chatClearedAfter),
  viewport: cloneViewport(state?.viewport || null)
});

const persistMissionCanvasStateCache = () => {
  const storage = resolveStateStorage();
  if (!storage) return;
  try {
    storage.setItem(
      MISSION_CANVAS_STATE_STORAGE_KEY,
      JSON.stringify({
        version: MISSION_CANVAS_STATE_VERSION,
        entries: Array.from(missionCanvasStateCache.entries()).map(([scopeKey, state]) => [
          scopeKey,
          cloneState(state)
        ])
      })
    );
  } catch {
    // Ignore storage quota and privacy-mode failures.
  }
};

const hydrateMissionCanvasStateCache = () => {
  if (missionCanvasStateHydrated) return;
  missionCanvasStateHydrated = true;
  const storage = resolveStateStorage();
  if (!storage) return;
  try {
    const raw = storage.getItem(MISSION_CANVAS_STATE_STORAGE_KEY);
    if (!raw) return;
    const payload = JSON.parse(raw) as {
      version?: unknown;
      entries?: Array<[unknown, Partial<BeeroomMissionCanvasState>]>;
    } | null;
    const entries = Array.isArray(payload?.entries) ? payload?.entries : [];
    entries.forEach((entry) => {
      if (!Array.isArray(entry) || entry.length < 2) return;
      const key = normalizeScopeKey(entry[0]);
      missionCanvasStateCache.set(key, normalizeState(entry[1]));
    });
    while (missionCanvasStateCache.size > MAX_CACHE_ENTRIES) {
      const oldest = missionCanvasStateCache.keys().next();
      if (oldest.done) break;
      missionCanvasStateCache.delete(oldest.value);
    }
  } catch {
    try {
      storage.removeItem(MISSION_CANVAS_STATE_STORAGE_KEY);
    } catch {
      // Ignore follow-up cleanup failures.
    }
  }
};

export const getBeeroomMissionCanvasState = (scopeKey: unknown): BeeroomMissionCanvasState | null => {
  hydrateMissionCanvasStateCache();
  const key = normalizeScopeKey(scopeKey);
  const hit = missionCanvasStateCache.get(key);
  if (!hit) return null;
  // Keep recently-used scopes hot when multiple missions are toggled quickly.
  missionCanvasStateCache.delete(key);
  missionCanvasStateCache.set(key, hit);
  persistMissionCanvasStateCache();
  return cloneState(hit);
};

export const setBeeroomMissionCanvasState = (scopeKey: unknown, state: BeeroomMissionCanvasState) => {
  hydrateMissionCanvasStateCache();
  const key = normalizeScopeKey(scopeKey);
  missionCanvasStateCache.set(key, cloneState(state));
  while (missionCanvasStateCache.size > MAX_CACHE_ENTRIES) {
    const oldest = missionCanvasStateCache.keys().next();
    if (oldest.done) break;
    missionCanvasStateCache.delete(oldest.value);
  }
  persistMissionCanvasStateCache();
};

export const mergeBeeroomMissionCanvasState = (
  scopeKey: unknown,
  patch:
    | Partial<BeeroomMissionCanvasState>
    | ((current: BeeroomMissionCanvasState) => Partial<BeeroomMissionCanvasState>)
) => {
  const key = normalizeScopeKey(scopeKey);
  const current = normalizeState(getBeeroomMissionCanvasState(key));
  const nextPatch = typeof patch === 'function' ? patch(current) : patch;
  setBeeroomMissionCanvasState(key, {
    ...current,
    ...nextPatch,
    version: MISSION_CANVAS_STATE_VERSION,
    nodePositionOverrides:
      nextPatch.nodePositionOverrides !== undefined
        ? cloneNodePositionOverrides(nextPatch.nodePositionOverrides)
        : current.nodePositionOverrides,
    activeNodeId:
      nextPatch.activeNodeId !== undefined
        ? String(nextPatch.activeNodeId || '').trim()
        : current.activeNodeId,
    chatCollapsed:
      nextPatch.chatCollapsed !== undefined ? !!nextPatch.chatCollapsed : current.chatCollapsed,
    chatWidth:
      nextPatch.chatWidth !== undefined ? normalizeChatWidth(nextPatch.chatWidth) : current.chatWidth,
    timelineCollapsed:
      nextPatch.timelineCollapsed !== undefined ? !!nextPatch.timelineCollapsed : current.timelineCollapsed,
    chatClearedAfter:
      nextPatch.chatClearedAfter !== undefined
        ? normalizeChatClearedAfter(nextPatch.chatClearedAfter)
        : current.chatClearedAfter,
    viewport:
      nextPatch.viewport !== undefined ? cloneViewport(nextPatch.viewport || null) : current.viewport
  });
};

export const clearBeeroomMissionCanvasState = (scopeKey: unknown) => {
  hydrateMissionCanvasStateCache();
  const key = normalizeScopeKey(scopeKey);
  if (!missionCanvasStateCache.delete(key)) {
    return;
  }
  persistMissionCanvasStateCache();
};
