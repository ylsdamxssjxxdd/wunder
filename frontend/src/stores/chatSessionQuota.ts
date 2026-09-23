type SessionRecord = Record<string, unknown>;

export function readSessionQuotaUsed(record: SessionRecord | null | undefined): number | null {
  const value = record?.quota_used ?? record?.quotaUsed;
  if (value === null || value === undefined || value === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.trunc(parsed) : null;
}

// Replayed absolute totals and late pages must never add quota twice or roll it back.
export function mergeSessionQuotaUsed(current: SessionRecord, incoming: SessionRecord): number | null {
  const left = readSessionQuotaUsed(current);
  const right = readSessionQuotaUsed(incoming);
  return left === null ? right : right === null ? left : Math.max(left, right);
}

export function applySessionQuotaUsage(sessions: SessionRecord[], id: string, payload: SessionRecord): boolean {
  const total = readSessionQuotaUsed({ quota_used: payload.session_quota_used });
  if (total === null) return false;
  const index = sessions.findIndex(session => String(session.id || '') === id);
  if (index < 0) return false;
  const current = readSessionQuotaUsed(sessions[index]);
  if (current !== null && total <= current) return false;
  sessions[index] = { ...sessions[index], quota_used: total };
  return true;
}
