const parseCount = (value: unknown): number | null => {
  if (typeof value !== 'number' && typeof value !== 'string') return null;
  if (typeof value === 'string' && !value.trim()) return null;
  const count = Number(value);
  return Number.isSafeInteger(count) && count >= 0 ? count : null;
};

/** Explicit invocation counts only: unknown values stay unknown, including
 * provider tool proposal arrays, booleans and empty strings. */
export const resolveMessageToolCalls = (stats: Record<string, any> | null | undefined): number | null => {
  if (!stats) return null;
  return parseCount(stats.toolCalls) ?? parseCount(stats.tool_calls);
};
