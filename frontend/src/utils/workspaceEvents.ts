const WORKSPACE_REFRESH_EVENT = 'wunder:workspace-refresh';
const AGENT_RUNTIME_REFRESH_EVENT = 'wunder:agent-runtime-refresh';

export type AgentRuntimeCompletion = {
  sessionId: string;
  turnId: string;
  agentId?: string;
  status?: 'completed' | 'failed' | 'cancelled';
};

export type AgentRuntimeRefreshDetail = {
  agentIds?: string[];
  /**
   * Durable, normally completed turns.  This lets the UI acknowledge a
   * completion even when an aggregate running-agent poll races the terminal
   * thread frame.
   */
  completedTurns?: AgentRuntimeCompletion[];
};

// Completion frames can be observed by more than one controller instance
// during route replacement.  Keep the idempotency boundary outside a
// component so an old and a new listener cannot both show the same toast.
// This is deliberately bounded and time based; it is only a presentation
// guard, while the durable turn log remains the source of truth.
const completionClaims = new Map<string, number>();
const COMPLETION_CLAIM_TTL_MS = 10 * 60 * 1000;
const COMPLETION_CLAIM_LIMIT = 2048;

export const claimAgentRuntimeCompletion = (sessionId: unknown, turnId: unknown): boolean => {
  const session = String(sessionId ?? '').trim();
  const turn = String(turnId ?? '').trim();
  if (!session || !turn) return false;
  const now = Date.now();
  for (const [key, claimedAt] of completionClaims) {
    if (now - claimedAt > COMPLETION_CLAIM_TTL_MS) completionClaims.delete(key);
  }
  const key = `${session}:${turn}`;
  if (completionClaims.has(key)) return false;
  completionClaims.set(key, now);
  while (completionClaims.size > COMPLETION_CLAIM_LIMIT) {
    const oldest = completionClaims.keys().next().value;
    if (!oldest) break;
    completionClaims.delete(oldest);
  }
  return true;
};

/** Claim the aggregate fallback notification for the same agent. */
export const claimAgentRuntimeAgentCompletion = (agentId: unknown): boolean => {
  const agent = String(agentId ?? '').trim();
  if (!agent) return false;
  const now = Date.now();
  for (const [key, claimedAt] of completionClaims) {
    if (now - claimedAt > COMPLETION_CLAIM_TTL_MS) completionClaims.delete(key);
  }
  const key = `agent:${agent}`;
  const previous = completionClaims.get(key);
  // Aggregate state has no turn identity, so keep this fallback claim short;
  // a later independent task for the same agent must still be announceable.
  if (previous !== undefined && now - previous < 15_000) return false;
  completionClaims.set(key, now);
  while (completionClaims.size > COMPLETION_CLAIM_LIMIT) {
    const oldest = completionClaims.keys().next().value;
    if (!oldest) break;
    completionClaims.delete(oldest);
  }
  return true;
};

export const emitWorkspaceRefresh = (detail = {}) => {
  if (typeof window === 'undefined') return;
  const payload = detail && typeof detail === 'object' ? detail : { detail };
  window.dispatchEvent(new CustomEvent(WORKSPACE_REFRESH_EVENT, { detail: payload }));
};

export const onWorkspaceRefresh = (handler) => {
  if (typeof window === 'undefined') return () => {};
  const listener = (event) => {
    if (typeof handler === 'function') {
      handler(event);
    }
  };
  window.addEventListener(WORKSPACE_REFRESH_EVENT, listener);
  return () => window.removeEventListener(WORKSPACE_REFRESH_EVENT, listener);
};

export const emitAgentRuntimeRefresh = (detail?: AgentRuntimeRefreshDetail) => {
  if (typeof window === 'undefined') return;
  const payload = detail && typeof detail === 'object' ? detail : {};
  window.dispatchEvent(new CustomEvent(AGENT_RUNTIME_REFRESH_EVENT, { detail: payload }));
};

export const onAgentRuntimeRefresh = (handler: (detail?: AgentRuntimeRefreshDetail) => void) => {
  if (typeof window === 'undefined') return () => {};
  const listener = (event: Event) => {
    if (typeof handler === 'function') {
      const detail = (event as CustomEvent<AgentRuntimeRefreshDetail>)?.detail ?? {};
      handler(detail);
    }
  };
  window.addEventListener(AGENT_RUNTIME_REFRESH_EVENT, listener);
  return () => window.removeEventListener(AGENT_RUNTIME_REFRESH_EVENT, listener);
};
