// Durable changes, transport events and item indexes have independent clocks.
export const threadChangeCursor = (runtime): number =>
  normalizeThreadChangeCursor(runtime?.threadChangeCursor) ?? 0;

export const normalizeThreadChangeCursor = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '' || typeof value === 'boolean') return null;
  const cursor = Number(value);
  return Number.isSafeInteger(cursor) && cursor >= 0 ? cursor : null;
};

export const advanceThreadChangeCursor = (runtime, value: unknown): void => {
  const cursor = normalizeThreadChangeCursor(value);
  if (runtime && cursor !== null) runtime.threadChangeCursor = Math.max(threadChangeCursor(runtime), cursor);
};
