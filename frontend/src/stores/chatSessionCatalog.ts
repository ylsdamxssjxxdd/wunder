import { mergeSessionsByIdPreservingRuntimeFields } from './chatSessionMerge';
import { patchSessionRuntimeFields } from './chatPersist';
import { sortSessionsByActivity } from './chatDemoPanels';
import { filterSessionsByAgent, purgeUnavailableSession, writeSessionListCache } from './chatRuntimeState';
import { ALL_SESSION_LIST_CACHE_KEY, normalizeSessionListItems } from './chatSessionListLoadCache';
import { isSessionUnavailable, restoreSessionAvailability } from './chatSessionAvailability';

const states = new WeakMap<object, { cursor: number }>();
const stateFor = (store: object) => {
  let state = states.get(store);
  if (!state) { state = { cursor: 0 }; states.set(store, state); }
  return state;
};

// Rotate a bounded catalog check alongside ordinary list reads. Missing rows on a
// partial page are never evidence of deletion, even after reaching the last page.
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
  payload: { items?: unknown; unavailable_session_ids?: unknown; total?: unknown },
  checkedIds: string[] = [],
  snapshot?: { candidateIds: string[]; offset: number }
) {
  const items = normalizeSessionListItems(payload.items);
  const unavailable = new Set<string>();
  const checked = new Set(checkedIds);
  for (const value of Array.isArray(payload.unavailable_session_ids) ? payload.unavailable_session_ids : []) {
    const id = String(value || '').trim();
    if (!checked.has(id)) continue;
    unavailable.add(id);
  }
  // A complete first page is authoritative only for entries known BEFORE this
  // request. Partial pages and threads created during the request remain intact.
  if (snapshot?.offset === 0 && Array.isArray(payload.items) && typeof payload.total === 'number' &&
      payload.total === items.length) {
    const present = new Set(items.map(item => String(item.id)));
    for (const id of snapshot.candidateIds) if (!present.has(id)) unavailable.add(id);
  }
  for (const id of unavailable) purgeUnavailableSession(store, id);
  const incoming = items.filter(item => !isSessionUnavailable(store, item.id));
  const incomingIds = new Set(incoming.map(item => String(item.id)));
  const retained = store.sessions.filter(item => !incomingIds.has(String(item.id)) && !isSessionUnavailable(store, item.id));
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
  restoreSessionAvailability(store, sessionId);
}

export const sessionCatalogCandidateIds = (store: any, agentId: string | null): string[] =>
  (agentId === null ? store.sessions : filterSessionsByAgent(agentId, store.sessions))
    .map(item => String(item.id || '').trim()).filter(Boolean);
