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
  viewport: BeeroomCanvasViewportState | null;
};

const MAX_CACHE_ENTRIES = 48;
const MISSION_CANVAS_STATE_VERSION = 2;
const missionCanvasStateCache = new Map<string, BeeroomMissionCanvasState>();

const normalizeScopeKey = (scopeKey: unknown) => {
  const key = String(scopeKey || '').trim();
  return key || 'standby';
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

const cloneState = (state: BeeroomMissionCanvasState): BeeroomMissionCanvasState => ({
  version: MISSION_CANVAS_STATE_VERSION,
  nodePositionOverrides: cloneNodePositionOverrides(state.nodePositionOverrides),
  activeNodeId: String(state.activeNodeId || '').trim(),
  chatCollapsed: !!state.chatCollapsed,
  viewport: cloneViewport(state.viewport)
});

const normalizeState = (state: Partial<BeeroomMissionCanvasState> | null | undefined): BeeroomMissionCanvasState => ({
  version: MISSION_CANVAS_STATE_VERSION,
  nodePositionOverrides: cloneNodePositionOverrides(state?.nodePositionOverrides || {}),
  activeNodeId: String(state?.activeNodeId || '').trim(),
  chatCollapsed: !!state?.chatCollapsed,
  viewport: cloneViewport(state?.viewport || null)
});

export const getBeeroomMissionCanvasState = (scopeKey: unknown): BeeroomMissionCanvasState | null => {
  const key = normalizeScopeKey(scopeKey);
  const hit = missionCanvasStateCache.get(key);
  if (!hit) return null;
  // Keep recently-used scopes hot when multiple missions are toggled quickly.
  missionCanvasStateCache.delete(key);
  missionCanvasStateCache.set(key, hit);
  return cloneState(hit);
};

export const setBeeroomMissionCanvasState = (scopeKey: unknown, state: BeeroomMissionCanvasState) => {
  const key = normalizeScopeKey(scopeKey);
  missionCanvasStateCache.set(key, cloneState(state));
  while (missionCanvasStateCache.size > MAX_CACHE_ENTRIES) {
    const oldest = missionCanvasStateCache.keys().next();
    if (oldest.done) break;
    missionCanvasStateCache.delete(oldest.value);
  }
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
    viewport:
      nextPatch.viewport !== undefined ? cloneViewport(nextPatch.viewport || null) : current.viewport
  });
};
