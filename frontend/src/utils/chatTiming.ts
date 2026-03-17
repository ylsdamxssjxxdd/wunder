const MIN_PLAUSIBLE_EPOCH_MS = Date.UTC(2000, 0, 1);
const MAX_PLAUSIBLE_EPOCH_MS = Date.UTC(2100, 0, 1);
const MAX_PLAUSIBLE_CHAT_DURATION_S = 6 * 60 * 60;

const asFiniteNumber = (value: unknown): number | null => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const isPlausibleEpochMs = (value: number): boolean =>
  Number.isFinite(value) && value >= MIN_PLAUSIBLE_EPOCH_MS && value <= MAX_PLAUSIBLE_EPOCH_MS;

const normalizeNumericTimestampMs = (value: number): number | null => {
  const absolute = Math.abs(value);
  const candidates =
    absolute >= 1e17
      ? [value / 1_000_000, value / 1_000, value]
      : absolute >= 1e14
        ? [value / 1_000, value / 1_000_000, value]
        : absolute >= 1e11
          ? [value, value / 1_000, value * 1_000]
          : absolute >= 1e8
            ? [value * 1_000, value, value / 1_000]
            : [];
  for (const candidate of candidates) {
    if (isPlausibleEpochMs(candidate)) {
      return candidate;
    }
  }
  return null;
};

export const normalizeChatTimestampMs = (value: unknown): number | null => {
  if (value instanceof Date) {
    const time = value.getTime();
    return isPlausibleEpochMs(time) ? time : null;
  }
  const numeric = asFiniteNumber(value);
  if (numeric !== null) {
    // Reject numeric payload fields that only happen to be named timestamp but are not wall-clock epochs.
    return normalizeNumericTimestampMs(numeric);
  }
  const text = String(value ?? '').trim();
  if (!text) return null;
  if (/^[+-]?\d+(?:\.\d+)?$/.test(text)) {
    const parsed = Number(text);
    return Number.isFinite(parsed) ? normalizeNumericTimestampMs(parsed) : null;
  }
  const time = new Date(text).getTime();
  return isPlausibleEpochMs(time) ? time : null;
};

export const normalizeChatDurationSeconds = (value: unknown): number | null => {
  const parsed = asFiniteNumber(value);
  if (parsed === null || parsed < 0) return null;
  if (parsed === 0) return 0;
  if (parsed <= MAX_PLAUSIBLE_CHAT_DURATION_S) {
    return parsed;
  }

  const millisecondCandidate = parsed / 1_000;
  if (
    parsed >= 10_000 &&
    millisecondCandidate > 0 &&
    millisecondCandidate <= MAX_PLAUSIBLE_CHAT_DURATION_S
  ) {
    return millisecondCandidate;
  }

  const microsecondCandidate = parsed / 1_000_000;
  if (microsecondCandidate > 0 && microsecondCandidate <= MAX_PLAUSIBLE_CHAT_DURATION_S) {
    return microsecondCandidate;
  }

  const nanosecondCandidate = parsed / 1_000_000_000;
  if (nanosecondCandidate > 0 && nanosecondCandidate <= MAX_PLAUSIBLE_CHAT_DURATION_S) {
    return nanosecondCandidate;
  }

  return parsed;
};
