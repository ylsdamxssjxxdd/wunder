import type { AgentRuntimeState } from './model';

/**
 * The last settled outcome per work thread, kept outside the catalog row and
 * the message cache. Both can be rebuilt (paged list, agent switch, registry
 * eviction, session cleanup) while the outcome itself has not changed; the
 * sidebar must not flicker back to idle in that gap. A new live turn clears
 * the memory, and a thread that never ran has none.
 */
const TERMINAL_STATES = new Set<AgentRuntimeState>(['done', 'error']);
const LIVE_STATES = new Set<AgentRuntimeState>(['running', 'pending']);
const MAX_ENTRIES = 512;

const settled = new Map<string, AgentRuntimeState>();

/**
 * Feed one resolved row state. Terminal states are remembered, live states
 * retire the memory, and an idle row reads the remembered outcome back (if
 * any) instead of guessing from a cache that may have been evicted.
 */
export const rememberTaskSettledState = (
  sessionId: string,
  state: AgentRuntimeState
): AgentRuntimeState | null => {
  const key = String(sessionId || '').trim();
  if (!key) return null;
  if (LIVE_STATES.has(state)) {
    settled.delete(key);
    return null;
  }
  if (TERMINAL_STATES.has(state)) {
    if (!settled.has(key) && settled.size >= MAX_ENTRIES) {
      const oldest = settled.keys().next().value;
      if (oldest !== undefined) settled.delete(oldest);
    }
    settled.set(key, state);
    return state;
  }
  return settled.get(key) ?? null;
};

export const forgetTaskSettledState = (sessionId: string): void => {
  settled.delete(String(sessionId || '').trim());
};

/** Test-only: drop all remembered outcomes. */
export const clearTaskSettledStates = (): void => {
  settled.clear();
};
