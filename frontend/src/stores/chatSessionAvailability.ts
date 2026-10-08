import { toRaw } from 'vue';

// Keep deletion evidence separate from paginated membership and runtime activity.
const unavailableByStore = new WeakMap<object, Set<string>>();
const entries = (store: object) => {
  const key = toRaw(store);
  let ids = unavailableByStore.get(key);
  if (!ids) { ids = new Set(); unavailableByStore.set(key, ids); }
  return ids;
};

export const isSessionUnavailable = (store: object, id: unknown) =>
  entries(store).has(String(id || '').trim());

export function markSessionUnavailable(store: object, id: string) {
  const ids = entries(store);
  ids.add(id);
  while (ids.size > 2048) ids.delete(ids.values().next().value!);
}

export const restoreSessionAvailability = (store: object, id: string) => entries(store).delete(id);
export const resetSessionAvailability = (store: object) => unavailableByStore.delete(toRaw(store));
