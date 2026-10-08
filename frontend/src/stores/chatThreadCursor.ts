// Durable changes, transport events and item indexes have independent clocks.
export const threadLogCursor = (runtime): number =>
  normalizeThreadLogCursor(runtime?.threadLogCursor) ?? 0;

export const normalizeThreadLogCursor = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '' || typeof value === 'boolean') return null;
  const cursor = Number(value);
  return Number.isSafeInteger(cursor) && cursor >= 0 ? cursor : null;
};

export const advanceThreadLogCursor = (runtime, value: unknown): void => {
  const cursor = normalizeThreadLogCursor(value);
  if (runtime && cursor !== null) runtime.threadLogCursor = Math.max(threadLogCursor(runtime), cursor);
};
