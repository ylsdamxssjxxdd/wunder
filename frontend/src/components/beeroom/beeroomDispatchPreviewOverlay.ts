import type { DispatchRuntimeStatus } from '@/components/beeroom/beeroomCanvasChatModel';
import type { BeeroomSwarmDispatchPreview } from '@/components/beeroom/canvas/swarmCanvasModel';

const LIVE_DISPATCH_LABEL_STATUSES = new Set(['queued', 'running', 'awaiting_idle', 'awaiting_approval', 'resuming']);

export const overlayBeeroomLiveDispatchLabel = (
  preview: BeeroomSwarmDispatchPreview | null | undefined,
  options: {
    currentSessionId?: unknown;
    runtimeStatus?: DispatchRuntimeStatus | string | null | undefined;
    composerSending?: boolean;
    dispatchLabelPreview?: unknown;
  }
): BeeroomSwarmDispatchPreview | null => {
  if (!preview) return null;
  const liveLabel = String(options.dispatchLabelPreview || '').trim();
  if (!liveLabel) return preview;
  const previewSessionId = String(preview.sessionId || '').trim();
  const currentSessionId = String(options.currentSessionId || '').trim();
  if (previewSessionId && currentSessionId && previewSessionId !== currentSessionId) {
    return preview;
  }
  const previewStatus = String(preview.status || '').trim().toLowerCase();
  const runtimeStatus = String(options.runtimeStatus || '').trim().toLowerCase();
  const dispatchActive =
    options.composerSending === true ||
    LIVE_DISPATCH_LABEL_STATUSES.has(previewStatus) ||
    LIVE_DISPATCH_LABEL_STATUSES.has(runtimeStatus);
  if (!dispatchActive) {
    return preview;
  }
  if (String(preview.dispatchLabel || '').trim() === liveLabel) {
    return preview;
  }
  return {
    ...preview,
    dispatchLabel: liveLabel
  };
};
