export const collectSnapshotApprovalEvents = (snapshotOrEvents: unknown): unknown[] | null => {
  if (Array.isArray(snapshotOrEvents)) return snapshotOrEvents;
  if (!snapshotOrEvents || typeof snapshotOrEvents !== 'object') return null;
  const snapshot = snapshotOrEvents as Record<string, unknown>;
  const events = Array.isArray(snapshot.events) ? snapshot.events : [];
  const roundEvents = Array.isArray(snapshot.rounds)
    ? snapshot.rounds.flatMap((round) => {
        const record = round && typeof round === 'object' ? round as Record<string, unknown> : null;
        return Array.isArray(record?.events) ? record.events : [];
      })
    : [];
  // Workflow-only snapshots carry events by user round; normal snapshots can
  // include both representations. Keep both so approval recovery never drops
  // a pending request while the transport is offline.
  return [...events, ...roundEvents];
};
