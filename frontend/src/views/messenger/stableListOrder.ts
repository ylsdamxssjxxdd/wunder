import { computed, shallowRef, watch, type ComputedRef, type Ref } from 'vue';

type KeySource = string | Readonly<Ref<string>> | ComputedRef<string>;
type KeyListSource = string[] | Readonly<Ref<string[]>> | ComputedRef<string[]>;
type MovePosition = 'before' | 'after';

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

const readStoredKeys = (storageKey: string): string[] => {
  if (typeof window === 'undefined') {
    return [];
  }
  const normalizedStorageKey = normalizeKey(storageKey);
  if (!normalizedStorageKey) {
    return [];
  }
  try {
    const raw = window.localStorage.getItem(normalizedStorageKey);
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

const writeStoredKeys = (storageKey: string, keys: string[]): void => {
  if (typeof window === 'undefined') {
    return;
  }
  const normalizedStorageKey = normalizeKey(storageKey);
  if (!normalizedStorageKey) {
    return;
  }
  try {
    window.localStorage.setItem(normalizedStorageKey, JSON.stringify(normalizeKeyList(keys)));
  } catch {
    // Ignore localStorage write failures and keep the session-local order.
  }
};

const readStoredKeysFromCandidates = (
  primaryStorageKey: string,
  fallbackStorageKeys: string[]
): { keys: string[]; matchedKey: string } => {
  const candidates = normalizeKeyList([primaryStorageKey, ...fallbackStorageKeys]);
  for (const candidate of candidates) {
    const keys = readStoredKeys(candidate);
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

const mergeKeyOrderPreservingMissing = (previousKeys: string[], incomingKeys: string[]): string[] => {
  const normalizedIncoming = normalizeKeyList(incomingKeys);
  const normalizedPrevious = normalizeKeyList(previousKeys);
  if (!normalizedIncoming.length) {
    return normalizedPrevious;
  }
  if (!normalizedPrevious.length) {
    return normalizedIncoming;
  }

  const previousSet = new Set(normalizedPrevious);
  const additions = normalizedIncoming.filter((key) => !previousSet.has(key));
  return [...additions, ...normalizedPrevious];
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
    storageKey: KeySource;
    storageFallbackKeys?: KeyListSource;
  }
) {
  const orderedKeys = shallowRef<string[]>([]);
  const activeStorageKey = shallowRef('');

  const syncFromSource = (items: T[], storageKey: string) => {
    const incomingKeys = normalizeKeyList((Array.isArray(items) ? items : []).map((item) => options.getKey(item)));
    const nextOrder = mergeKeyOrderPreservingMissing(orderedKeys.value, incomingKeys);
    if (!sameKeyList(nextOrder, orderedKeys.value)) {
      orderedKeys.value = nextOrder;
    }
    if (storageKey) {
      writeStoredKeys(storageKey, orderedKeys.value);
    }
  };

  watch(
    [source, () => resolveKeySource(options.storageKey), () => resolveKeyListSource(options.storageFallbackKeys)],
    ([items, storageKey, fallbackStorageKeys]) => {
      if (storageKey !== activeStorageKey.value) {
        activeStorageKey.value = storageKey;
        const { keys, matchedKey } = readStoredKeysFromCandidates(storageKey, fallbackStorageKeys);
        orderedKeys.value = keys;
        if (storageKey && matchedKey && matchedKey !== storageKey && keys.length > 0) {
          writeStoredKeys(storageKey, keys);
        }
      }
      syncFromSource(Array.isArray(items) ? items : [], storageKey);
    },
    {
      immediate: true,
      deep: false
    }
  );

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
    const storageKey = resolveKeySource(options.storageKey);
    if (storageKey) {
      writeStoredKeys(storageKey, nextOrder);
    }
  };

  return {
    orderedItems,
    orderedKeys,
    moveItem
  };
}
