const toCursorNumber = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? '').trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

const resolvePayloadCursor = (payload: unknown): number => {
  if (!payload || typeof payload !== 'object') {
    return 0;
  }
  const source = payload as Record<string, unknown>;
  return Math.max(
    toCursorNumber(source.event_id),
    toCursorNumber(source.eventId),
    toCursorNumber(source.after_event_id),
    toCursorNumber(source.afterEventId),
    toCursorNumber(source.latest_event_id),
    toCursorNumber(source.latestEventId)
  );
};

type ResolveNextRealtimeCursorOptions = {
  currentCursor: unknown;
  eventId: unknown;
  payload: unknown;
};

// Keep a monotonic cursor so WS/SSE reconnect can resume from the latest known event.
export const resolveNextRealtimeCursor = (
  options: ResolveNextRealtimeCursorOptions
): number =>
  Math.max(
    toCursorNumber(options.currentCursor),
    toCursorNumber(options.eventId),
    resolvePayloadCursor(options.payload)
  );
