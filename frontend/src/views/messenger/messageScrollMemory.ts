export type MessageScrollSnapshot = {
  scrollTop: number;
  distanceFromBottom: number;
  clientHeight: number;
  scrollHeight: number;
  updatedAt: number;
};

const MAX_SNAPSHOTS = 80;
const BOTTOM_THRESHOLD_PX = 96;

const snapshots = new Map<string, MessageScrollSnapshot>();

const normalizeKey = (value: unknown): string => String(value || '').trim();

const clamp = (value: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, value));

const trimSnapshots = () => {
  if (snapshots.size <= MAX_SNAPSHOTS) return;
  const staleKeys = Array.from(snapshots.entries())
    .sort((left, right) => left[1].updatedAt - right[1].updatedAt)
    .slice(0, Math.max(1, snapshots.size - MAX_SNAPSHOTS))
    .map(([key]) => key);
  staleKeys.forEach((key) => snapshots.delete(key));
};

export const rememberMessageScrollPosition = (
  key: unknown,
  container: HTMLElement | null | undefined
): void => {
  const normalizedKey = normalizeKey(key);
  if (!normalizedKey || !container) return;
  const scrollTop = Math.max(0, container.scrollTop || 0);
  const clientHeight = Math.max(0, container.clientHeight || 0);
  const scrollHeight = Math.max(0, container.scrollHeight || 0);
  snapshots.set(normalizedKey, {
    scrollTop,
    distanceFromBottom: Math.max(0, scrollHeight - clientHeight - scrollTop),
    clientHeight,
    scrollHeight,
    updatedAt: Date.now()
  });
  trimSnapshots();
};

export const restoreMessageScrollPosition = (
  key: unknown,
  container: HTMLElement | null | undefined
): boolean => {
  const normalizedKey = normalizeKey(key);
  if (!normalizedKey || !container) return false;
  const snapshot = snapshots.get(normalizedKey);
  if (!snapshot) return false;
  const maxTop = Math.max(0, (container.scrollHeight || 0) - (container.clientHeight || 0));
  const targetTop =
    snapshot.distanceFromBottom <= BOTTOM_THRESHOLD_PX
      ? maxTop
      : Math.max(0, (container.scrollHeight || 0) - (container.clientHeight || 0) - snapshot.distanceFromBottom);
  container.scrollTop = clamp(targetTop, 0, maxTop);
  return true;
};

export const isMessageScrollSnapshotNearBottom = (key: unknown): boolean => {
  const snapshot = snapshots.get(normalizeKey(key));
  return !snapshot || snapshot.distanceFromBottom <= BOTTOM_THRESHOLD_PX;
};
