export type BeeroomCanvasPositionOverride = {
  x: number;
  y: number;
};

export type BeeroomCanvasViewportState = {
  zoom: number;
  position: [number, number];
  centerOffset: [number, number];
};

export type BeeroomMissionCanvasState = {
  nodePositionOverrides: Record<string, BeeroomCanvasPositionOverride>;
  activeNodeId: string;
  chatCollapsed: boolean;
  viewport: BeeroomCanvasViewportState | null;
};

const MAX_CACHE_ENTRIES = 48;
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
  const zoom = Number(viewport.zoom);
  const x = Number(viewport.position?.[0]);
  const y = Number(viewport.position?.[1]);
  const offsetX = Number((viewport as Partial<BeeroomCanvasViewportState>).centerOffset?.[0]);
  const offsetY = Number((viewport as Partial<BeeroomCanvasViewportState>).centerOffset?.[1]);
  if (!Number.isFinite(zoom) || !Number.isFinite(x) || !Number.isFinite(y)) {
    return null;
  }
  return {
    zoom,
    position: [x, y] as [number, number],
    centerOffset: [
      Number.isFinite(offsetX) ? offsetX : 0,
      Number.isFinite(offsetY) ? offsetY : 0
    ] as [number, number]
  };
};

const cloneState = (state: BeeroomMissionCanvasState): BeeroomMissionCanvasState => ({
  nodePositionOverrides: cloneNodePositionOverrides(state.nodePositionOverrides),
  activeNodeId: String(state.activeNodeId || '').trim(),
  chatCollapsed: !!state.chatCollapsed,
  viewport: cloneViewport(state.viewport)
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
