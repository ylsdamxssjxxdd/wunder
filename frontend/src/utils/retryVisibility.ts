const RETRY_VISIBILITY_GRACE_MS = 2_000;

const parsePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizeTimestamp = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const shouldDisplayTransientRetry = (
  source: {
    retry_attempt?: unknown;
    retry_started_at_ms?: unknown;
  } | null | undefined,
  nowMs = Date.now()
): boolean => {
  const attempt = parsePositiveInteger(source?.retry_attempt);
  if (!attempt) {
    return false;
  }
  if (attempt >= 2) {
    return true;
  }
  const startedAtMs = normalizeTimestamp(source?.retry_started_at_ms);
  if (!startedAtMs) {
    return false;
  }
  return nowMs - startedAtMs >= RETRY_VISIBILITY_GRACE_MS;
};

export const RETRY_VISIBILITY_GRACE_WINDOW_MS = RETRY_VISIBILITY_GRACE_MS;
