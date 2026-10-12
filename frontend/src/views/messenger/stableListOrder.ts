// AI生成
import { computed, getCurrentScope, onScopeDispose, shallowRef, watch, type ComputedRef, type Ref } from 'vue';

type KeySource = string | Readonly<Ref<string>> | ComputedRef<string>;
type KeyListSource = string[] | Readonly<Ref<string[]>> | ComputedRef<string[]>;
type MovePosition = 'before' | 'after';

/** Minimal storage contract so the composable can be exercised with a fake backend. */
export type StableListStorage = {
  getItem: (key: string) => string | null;
  setItem: (key: string, value: string) => void;
};

/** Schedules a deferred flush. Tests inject a manual scheduler to control timing. */
export type StableListFlushScheduler = (task: () => void) => void;

const normalizeKey = (value: unknown): string => String(value || '').trim();

const normalizeKeyList = (values: unknown[]): string[] => {
  const output: string[] = [];
  const seen = new Set<string>();
  values.forEach((value) => {
    const key = normalizeKey(value);
    if (!key || seen.has(key)) {
      return;
    }
    seen.add(key);
    output.push(key);
  });
  return output;
};

const resolveKeySource = (value: KeySource): string => {
  if (typeof value === 'string') {
    return value;
  }
  return normalizeKey(value.value);
};

const resolveKeyListSource = (value?: KeyListSource): string[] => {
  if (!value) {
    return [];
  }
  if (Array.isArray(value)) {
    return normalizeKeyList(value);
  }
  return normalizeKeyList(Array.isArray(value.value) ? value.value : []);
};

const resolveDefaultStorage = (): StableListStorage | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  try {
    return window.localStorage || null;
  } catch {
    return null;
  }
};

// Persistence is a preference, never a correctness dependency: coalesce every
// interaction into the next idle frame (or microtask) so a click/drag never
// blocks the main thread on a synchronous, full-order localStorage write.
const defaultScheduleFlush: StableListFlushScheduler = (task) => {
  const scope = (typeof window !== 'undefined' ? window : globalThis) as unknown as {
    requestIdleCallback?: (callback: () => void, options?: { timeout: number }) => unknown;
  };
  if (typeof scope.requestIdleCallback === 'function') {
    scope.requestIdleCallback(() => task(), { timeout: 500 });
    return;
  }
  if (typeof queueMicrotask === 'function') {
    queueMicrotask(task);
    return;
  }
  setTimeout(task, 0);
};

const readStoredKeys = (storage: StableListStorage | null, storageKey: string): string[] => {
  if (!storage) {
    return [];
  }
  const normalizedStorageKey = normalizeKey(storageKey);
  if (!normalizedStorageKey) {
    return [];
  }
  try {
    const raw = storage.getItem(normalizedStorageKey);
    if (!raw) {
      return [];
    }
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) {
      return [];
    }
    return normalizeKeyList(parsed);
  } catch {
    return [];
  }
};

const writeStoredKeys = (storage: StableListStorage | null, storageKey: string, keys: string[]): void => {
  if (!storage) {
    return;
  }
  const normalizedStorageKey = normalizeKey(storageKey);
  if (!normalizedStorageKey) {
    return;
  }
  try {
    storage.setItem(normalizedStorageKey, JSON.stringify(normalizeKeyList(keys)));
  } catch {
    // Ignore storage write failures and keep the session-local order.
  }
};

const readStoredKeysFromCandidates = (
  storage: StableListStorage | null,
  primaryStorageKey: string,
  fallbackStorageKeys: string[]
): { keys: string[]; matchedKey: string } => {
  const candidates = normalizeKeyList([primaryStorageKey, ...fallbackStorageKeys]);
  for (const candidate of candidates) {
    const keys = readStoredKeys(storage, candidate);
    if (keys.length > 0) {
      return { keys, matchedKey: candidate };
    }
  }
  return { keys: [], matchedKey: '' };
};

const sameKeyList = (left: string[], right: string[]): boolean => {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
};

export const stabilizeKeyOrder = (previousKeys: string[], incomingKeys: string[]): string[] => {
  const normalizedIncoming = normalizeKeyList(incomingKeys);
  const normalizedPrevious = normalizeKeyList(previousKeys);
  if (!normalizedIncoming.length) {
    return normalizedPrevious;
  }
  if (!normalizedPrevious.length) {
    return normalizedIncoming;
  }

  const previousSet = new Set(normalizedPrevious);
  const incomingSet = new Set(normalizedIncoming);
  const additions = normalizedIncoming.filter((key) => !previousSet.has(key));
  const existing = normalizedPrevious.filter((key) => incomingSet.has(key));
  return [...additions, ...existing];
};

/**
 * Merge a previous manual order with the server's incoming order in a single
 * linear pass. Surviving rows keep their previous relative order; rows that are
 * new position themselves just before the next surviving row they follow in the
 * incoming order, and trailing rows append at the end. This is the same result
 * the historical nested-scan implementation produced, without the O(n^2) scans.
 */
export const mergeKeyOrderPreservingMissing = (previousKeys: string[], incomingKeys: string[]): string[] => {
  const normalizedIncoming = normalizeKeyList(incomingKeys);
  const normalizedPrevious = normalizeKeyList(previousKeys);
  if (!normalizedIncoming.length) {
    return normalizedPrevious;
  }
  if (!normalizedPrevious.length) {
    return normalizedIncoming;
  }

  const previousSet = new Set(normalizedPrevious);
  const incomingSet = new Set(normalizedIncoming);
  const survivors = normalizedPrevious.filter((key) => incomingSet.has(key));
  if (!survivors.length) {
    return normalizedIncoming;
  }

  // For every incoming slot, precompute the nearest surviving old row to its
  // right. New rows bucket under that anchor so they keep the incoming order
  // without repeated lookups.
  const anchorByIndex: Array<string | null> = new Array(normalizedIncoming.length).fill(null);
  let following: string | null = null;
  for (let index = normalizedIncoming.length - 1; index >= 0; index -= 1) {
    anchorByIndex[index] = following;
    if (previousSet.has(normalizedIncoming[index])) {
      following = normalizedIncoming[index];
    }
  }

  const anchors = new Map<string, string[]>();
  const trailing: string[] = [];
  let tailAnchor = '';
  for (let index = 0; index < normalizedIncoming.length; index += 1) {
    const key = normalizedIncoming[index];
    if (previousSet.has(key)) {
      tailAnchor = key;
      continue;
    }
    const anchor = anchorByIndex[index];
    if (anchor) {
      const bucket = anchors.get(anchor);
      if (bucket) bucket.push(key);
      else anchors.set(anchor, [key]);
    } else {
      trailing.push(key);
    }
  }

  const next: string[] = [];
  for (const key of survivors) {
    const bucket = anchors.get(key);
    if (bucket) next.push(...bucket);
    next.push(key);
  }
  // Rows with no following survivor sit immediately after the last survivor in
  // the incoming order (the historical scan inserted them after the nearest
  // preceding key, which is that survivor).
  if (trailing.length) {
    const anchorIndex = next.indexOf(tailAnchor);
    next.splice(anchorIndex < 0 ? next.length : anchorIndex + 1, 0, ...trailing);
  }
  return next;
};

export const prependFreshKeys = (
  order: string[],
  previousKeys: string[],
  incomingKeys: string[],
  timestamps: Map<string, number>
): string[] => {
  if (!previousKeys.length || !timestamps.size) return order;
  const previousSet = new Set(previousKeys);
  const additions = incomingKeys.filter((key) => !previousSet.has(key));
  if (!additions.length) return order;
  let newestExisting = 0;
  for (const key of previousKeys) {
    const value = timestamps.get(key) ?? 0;
    if (Number.isFinite(value) && value > newestExisting) newestExisting = value;
  }
  if (!newestExisting) return order;
  const fresh = additions.filter((key) => (timestamps.get(key) ?? 0) > newestExisting);
  if (!fresh.length) return order;
  const freshSet = new Set(fresh);
  return [...fresh, ...order.filter((key) => !freshSet.has(key))];
};

export const moveKeyWithinOrder = (
  order: string[],
  draggedKey: string,
  targetKey: string,
  position: MovePosition = 'before'
): string[] => {
  const normalizedOrder = normalizeKeyList(order);
  const normalizedDraggedKey = normalizeKey(draggedKey);
  const normalizedTargetKey = normalizeKey(targetKey);
  if (!normalizedDraggedKey || !normalizedTargetKey || normalizedDraggedKey === normalizedTargetKey) {
    return normalizedOrder;
  }
  const draggedIndex = normalizedOrder.indexOf(normalizedDraggedKey);
  const targetIndex = normalizedOrder.indexOf(normalizedTargetKey);
  if (draggedIndex < 0 || targetIndex < 0) {
    return normalizedOrder;
  }

  const next = normalizedOrder.slice();
  next.splice(draggedIndex, 1);
  const adjustedTargetIndex = next.indexOf(normalizedTargetKey);
  if (adjustedTargetIndex < 0) {
    return normalizedOrder;
  }
  const insertionIndex = position === 'after' ? adjustedTargetIndex + 1 : adjustedTargetIndex;
  next.splice(insertionIndex, 0, normalizedDraggedKey);
  return next;
};

export const moveKeyWithinVisibleOrder = (
  order: string[],
  visibleKeys: string[],
  draggedKey: string,
  targetKey: string,
  position: MovePosition = 'before'
): string[] => {
  const normalizedOrder = normalizeKeyList(order);
  const normalizedVisibleKeys = normalizeKeyList(visibleKeys);
  if (!normalizedOrder.length || !normalizedVisibleKeys.length) {
    return normalizedOrder;
  }
  const visibleSet = new Set(normalizedVisibleKeys);
  const currentVisibleOrder = normalizedOrder.filter((key) => visibleSet.has(key));
  if (!currentVisibleOrder.includes(normalizeKey(draggedKey)) || !currentVisibleOrder.includes(normalizeKey(targetKey))) {
    return normalizedOrder;
  }

  const nextVisibleOrder = moveKeyWithinOrder(currentVisibleOrder, draggedKey, targetKey, position);
  if (sameKeyList(currentVisibleOrder, nextVisibleOrder)) {
    return normalizedOrder;
  }

  let visibleIndex = 0;
  return normalizedOrder.map((key) => {
    if (!visibleSet.has(key)) {
      return key;
    }
    const replacement = nextVisibleOrder[visibleIndex];
    visibleIndex += 1;
    return replacement || key;
  });
};

export function usePersistentStableListOrder<T>(
  source: Readonly<Ref<T[]>> | ComputedRef<T[]>,
  options: {
    getKey: (item: T) => string;
    getTimestamp?: (item: T) => number;
    storageKey: KeySource;
    storageFallbackKeys?: KeyListSource;
    /** Injectable storage backend; defaults to window.localStorage. */
    storage?: StableListStorage | null;
    /** Injectable flush scheduler; defaults to requestIdleCallback/microtask. */
    scheduleFlush?: StableListFlushScheduler;
  }
) {
  const orderedKeys = shallowRef<string[]>([]);
  const activeStorageKey = shallowRef('');
  const knownTimestamps = new Map<string, number>();

  const storage = options.storage === undefined ? resolveDefaultStorage() : options.storage;
  const scheduleFlush = options.scheduleFlush || defaultScheduleFlush;

  // Single pending write per composable: every interaction within one frame
  // collapses into one serialized localStorage write on the next idle slot.
  let pendingWrite: { key: string; keys: string[] } | null = null;
  let flushScheduled = false;
  const runFlush = () => {
    flushScheduled = false;
    const pending = pendingWrite;
    pendingWrite = null;
    if (pending) writeStoredKeys(storage, pending.key, pending.keys);
  };
  const scheduleWrite = (storageKey: string, keys: string[]) => {
    if (!storageKey) {
      return;
    }
    pendingWrite = { key: storageKey, keys: keys.slice() };
    if (flushScheduled) {
      return;
    }
    flushScheduled = true;
    scheduleFlush(runFlush);
  };

  const syncFromSource = (items: T[], storageKey: string) => {
    const incomingKeys = normalizeKeyList((Array.isArray(items) ? items : []).map((item) => options.getKey(item)));
    const previousKeys = orderedKeys.value.slice();
    const incomingTimestamps = new Map(knownTimestamps);
    if (options.getTimestamp) {
      items.forEach((item) => {
        const key = normalizeKey(options.getKey(item));
        const value = Number(options.getTimestamp?.(item));
        if (key && Number.isFinite(value) && value > 0) incomingTimestamps.set(key, value);
      });
    }
    const nextOrder = prependFreshKeys(
      mergeKeyOrderPreservingMissing(previousKeys, incomingKeys),
      previousKeys,
      incomingKeys,
      incomingTimestamps
    );
    if (!sameKeyList(nextOrder, orderedKeys.value)) {
      orderedKeys.value = nextOrder;
    }
    scheduleWrite(storageKey, orderedKeys.value);
    incomingTimestamps.forEach((value, key) => knownTimestamps.set(key, value));
  };

  watch(
    [source, () => resolveKeySource(options.storageKey), () => resolveKeyListSource(options.storageFallbackKeys)],
    ([items, storageKey, fallbackStorageKeys]) => {
      if (storageKey !== activeStorageKey.value) {
        activeStorageKey.value = storageKey;
        knownTimestamps.clear();
        const { keys, matchedKey } = readStoredKeysFromCandidates(storage, storageKey, fallbackStorageKeys);
        orderedKeys.value = keys;
        if (storageKey && matchedKey && matchedKey !== storageKey && keys.length > 0) {
          scheduleWrite(storageKey, keys);
        }
      }
      syncFromSource(Array.isArray(items) ? items : [], storageKey);
      const liveKeys = new Set(orderedKeys.value);
      for (const key of [...knownTimestamps.keys()]) {
        if (!liveKeys.has(key)) knownTimestamps.delete(key);
      }
    },
    {
      immediate: true,
      deep: false
    }
  );

  // Flush any deferred write when the owning scope is torn down so a quick
  // unmount cannot drop the last interaction.
  if (getCurrentScope()) {
    onScopeDispose(runFlush);
  }

  const orderedItems = computed(() => {
    const items = Array.isArray(source.value) ? source.value : [];
    if (!items.length) {
      return [] as T[];
    }
    const itemByKey = new Map<string, T>();
    items.forEach((item) => {
      const key = normalizeKey(options.getKey(item));
      if (!key || itemByKey.has(key)) {
        return;
      }
      itemByKey.set(key, item);
    });
    return orderedKeys.value
      .map((key) => itemByKey.get(key) || null)
      .filter((item): item is T => Boolean(item));
  });

  const moveItem = (
    draggedKey: string,
    targetKey: string,
    position: MovePosition = 'before',
    visibleKeys: string[] = []
  ): void => {
    const sourceItems = Array.isArray(source.value) ? source.value : [];
    const sourceKeys = normalizeKeyList(sourceItems.map((item) => options.getKey(item)));
    const baseOrder = stabilizeKeyOrder(orderedKeys.value, sourceKeys);
    const nextOrder =
      visibleKeys.length > 0
        ? moveKeyWithinVisibleOrder(baseOrder, visibleKeys, draggedKey, targetKey, position)
        : moveKeyWithinOrder(baseOrder, draggedKey, targetKey, position);
    if (sameKeyList(baseOrder, nextOrder)) {
      return;
    }
    orderedKeys.value = nextOrder;
    scheduleWrite(resolveKeySource(options.storageKey), nextOrder);
  };

  const prependKey = (key: string): void => {
    const normalizedKey = normalizeKey(key);
    if (!normalizedKey) return;
    const sourceKeys = normalizeKeyList((Array.isArray(source.value) ? source.value : []).map((item) => options.getKey(item)));
    if (!sourceKeys.includes(normalizedKey)) return;
    // Keep this operation idempotent. Callers commonly invoke it from a
    // watcher whose dependencies include orderedItems; assigning a fresh
    // array when the key is already first would retrigger that watcher
    // indefinitely and can freeze the whole messenger page.
    if (orderedKeys.value[0] === normalizedKey) return;
    const nextOrder = [normalizedKey, ...orderedKeys.value.filter((item) => item !== normalizedKey)];
    orderedKeys.value = nextOrder;
    scheduleWrite(resolveKeySource(options.storageKey), nextOrder);
  };

  return {
    orderedItems,
    orderedKeys,
    moveItem,
    prependKey
  };
}
