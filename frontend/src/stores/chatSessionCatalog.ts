import { mergeSessionsByIdPreservingRuntimeFields } from './chatSessionMerge';
import { patchSessionRuntimeFields } from './chatPersist';
import { sortSessionsByActivity } from './chatDemoPanels';
import { filterSessionsByAgent, purgeUnavailableSession, writeSessionListCache } from './chatRuntimeState';
import { ALL_SESSION_LIST_CACHE_KEY, normalizeSessionListItems } from './chatSessionListLoadCache';

const states = new WeakMap<object, { cursor: number; unavailable: Set<string> }>();
const stateFor = (store: object) => {
  let state = states.get(store);
  if (!state) { state = { cursor: 0, unavailable: new Set() }; states.set(store, state); }
  return state;
};

// Rotate a bounded catalog check alongside ordinary list reads. Missing rows on a
// single page are never evidence of deletion, even after reaching the last page.
export function sessionCatalogCheckIds(store: any, agentId: string | null): string[] {
  const state = stateFor(store);
  const sessions = agentId === null ? store.sessions : filterSessionsByAgent(agentId, store.sessions);
  const ids = new Set<string>();
  if (store.activeSessionId) ids.add(String(store.activeSessionId));
  let visited = 0;
  while (visited < sessions.length && ids.size < 100) {
    const id = String(sessions[(state.cursor + visited) % sessions.length]?.id || '').trim();
    if (id) ids.add(id);
    visited += 1;
  }
  state.cursor = sessions.length ? (state.cursor + visited) % sessions.length : 0;
  return [...ids];
}

export function mergeSessionCatalogPage(
  store: any,
  payload: { items?: unknown; unavailable_session_ids?: unknown },
  checkedIds: string[] = []
) {
  const state = stateFor(store);
  const checked = new Set(checkedIds);
  for (const value of Array.isArray(payload.unavailable_session_ids) ? payload.unavailable_session_ids : []) {
    const id = String(value || '').trim();
    if (!checked.has(id)) continue;
    state.unavailable.add(id);
    purgeUnavailableSession(store, id);
  }
  // Bound protection against stale in-flight pages; ids are globally unique.
  while (state.unavailable.size > 2048) state.unavailable.delete(state.unavailable.values().next().value!);
  const incoming = normalizeSessionListItems(payload.items).filter(item => !state.unavailable.has(String(item.id)));
  const incomingIds = new Set(incoming.map(item => String(item.id)));
  const retained = store.sessions.filter(item => !incomingIds.has(String(item.id)) && !state.unavailable.has(String(item.id)));
  store.sessions = mergeSessionsByIdPreservingRuntimeFields(
    store.sessions, [...incoming, ...retained], patchSessionRuntimeFields, sortSessionsByActivity
  );
  return store.sessions;
}

export function cacheSessionCatalog(store: any, agentId: string | null) {
  writeSessionListCache(ALL_SESSION_LIST_CACHE_KEY, store.sessions);
  if (agentId !== null) writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
}

export function restoreSessionCatalogEntry(store: object, sessionId: string) {
  stateFor(store).unavailable.delete(sessionId);
}
