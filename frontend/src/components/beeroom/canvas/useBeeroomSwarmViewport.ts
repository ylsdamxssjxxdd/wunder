import type { SwarmProjectionBounds } from './swarmCanvasModel';

export type SwarmViewportState = {
  scale: number;
  offsetX: number;
  offsetY: number;
};

export type SwarmViewportSize = {
  width: number;
  height: number;
};

export const SWARM_SCALE_MIN = 0.5;
export const SWARM_SCALE_MAX = 1.8;
export const SWARM_SCALE_STEP = 0.12;

export const clampSwarmScale = (value: number) =>
  Math.min(SWARM_SCALE_MAX, Math.max(SWARM_SCALE_MIN, value));

export const createDefaultSwarmViewportState = (): SwarmViewportState => ({
  scale: 1,
  offsetX: 0,
  offsetY: 0
});

export const normalizeSwarmViewportSize = (size: Partial<SwarmViewportSize> | null | undefined): SwarmViewportSize => ({
  width: Math.max(360, Number(size?.width || 0) || 0),
  height: Math.max(520, Number(size?.height || 0) || 0)
});

export const fitSwarmViewportToBounds = (options: {
  bounds: SwarmProjectionBounds;
  worldWidth: number;
  worldHeight: number;
  viewport: SwarmViewportSize;
  padding?: number;
}): SwarmViewportState => {
  const viewport = normalizeSwarmViewportSize(options.viewport);
  const padding = Math.max(24, Number(options.padding || 0) || 0);
  const worldWidth = Math.max(1, Number(options.worldWidth || options.bounds.width || 1));
  const worldHeight = Math.max(1, Number(options.worldHeight || options.bounds.height || 1));
  const scale = clampSwarmScale(
    Math.min((viewport.width - padding * 2) / worldWidth, (viewport.height - padding * 2) / worldHeight, 1)
  );
  return {
    scale,
    offsetX: Math.round((viewport.width - worldWidth * scale) / 2),
    offsetY: Math.round((viewport.height - worldHeight * scale) / 2)
  };
};

export const zoomSwarmViewportAroundPoint = (options: {
  viewportState: SwarmViewportState;
  nextScale: number;
  anchorX: number;
  anchorY: number;
}): SwarmViewportState => {
  const currentScale = clampSwarmScale(options.viewportState.scale);
  const nextScale = clampSwarmScale(options.nextScale);
  if (Math.abs(nextScale - currentScale) < 0.0001) {
    return {
      scale: currentScale,
      offsetX: options.viewportState.offsetX,
      offsetY: options.viewportState.offsetY
    };
  }
  const worldX = (options.anchorX - options.viewportState.offsetX) / currentScale;
  const worldY = (options.anchorY - options.viewportState.offsetY) / currentScale;
  return {
    scale: nextScale,
    offsetX: Math.round(options.anchorX - worldX * nextScale),
    offsetY: Math.round(options.anchorY - worldY * nextScale)
  };
};

export const screenPointToWorld = (options: {
  viewportState: SwarmViewportState;
  x: number;
  y: number;
}) => ({
  x: (options.x - options.viewportState.offsetX) / clampSwarmScale(options.viewportState.scale),
  y: (options.y - options.viewportState.offsetY) / clampSwarmScale(options.viewportState.scale)
});
