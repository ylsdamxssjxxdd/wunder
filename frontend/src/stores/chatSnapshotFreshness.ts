type RuntimeClock = { realtimeRevision?: number; snapshotRequestId?: number } | null | undefined;

export const readChatRealtimeRevision = (runtime: RuntimeClock): number =>
  Number(runtime?.realtimeRevision || 0);

// A response describes the state when its request started. A newer stream event
// wins even when the response carries the same persisted event cursor.
export const isChatSnapshotCurrent = (
  runtime: RuntimeClock,
  payload: Record<string, unknown> | null | undefined,
  requestRevision = readChatRealtimeRevision(runtime)
): boolean => {
  const current = readChatRealtimeRevision(runtime);
  const captured = payload?.__clientRuntimeRevision;
  return current === requestRevision &&
    (captured === undefined || Number(captured) === current) &&
    (payload?.__clientSnapshotRequestId === undefined ||
      Number(payload.__clientSnapshotRequestId) === Number(runtime?.snapshotRequestId || 0));
};
