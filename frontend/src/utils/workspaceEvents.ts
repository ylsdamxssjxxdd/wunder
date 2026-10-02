const WORKSPACE_REFRESH_EVENT = 'wunder:workspace-refresh';
const AGENT_RUNTIME_REFRESH_EVENT = 'wunder:agent-runtime-refresh';

export type AgentRuntimeCompletion = {
  sessionId: string;
  turnId: string;
  agentId?: string;
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
