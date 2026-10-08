/** Format counters for dense UI surfaces while keeping exact values in the title. */
export const formatCompactCount = (value: unknown, options: { zeroAsDash?: boolean } = {}): string => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) return '--';
  const count = Math.trunc(parsed);
  if (count === 0 && options.zeroAsDash) return '--';
  if (count < 1_000) return String(count);
  const unit = count >= 1_000_000 ? 'm' : 'k';
  const divisor = unit === 'm' ? 1_000_000 : 1_000;
  const compact = count / divisor;
  const digits = compact >= 100 ? 0 : 1;
  return `${compact.toFixed(digits).replace(/\.0$/, '')}${unit}`;
};
