export type AgentOpenSessionState = {
  activeSessionId?: unknown;
  activeConversationKey?: unknown;
  draftAgentId?: unknown;
  sessions?: unknown[];
};

const DEFAULT_AGENT_KEY = '__default__';

const normalizeAgentId = (value: unknown): string => {
  const text = String(value || '').trim();
  return text || DEFAULT_AGENT_KEY;
};

const readSessionId = (value: unknown): string => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  return String(source.id || source.session_id || '').trim();
};

const readSessionAgentId = (value: unknown, fallback: unknown = ''): string => {
  const source = value && typeof value === 'object' ? (value as Record<string, unknown>) : {};
  return normalizeAgentId(source.agent_id || source.agentId || fallback);
};

export const resolveOpenActiveSessionAgentId = (
  state: AgentOpenSessionState,
  targetSessionId: string
): string => {
  const sessions = Array.isArray(state.sessions) ? state.sessions : [];
  const session = sessions.find((item) => readSessionId(item) === targetSessionId);
  return readSessionAgentId(session, state.draftAgentId);
};

export const isAgentAlreadyOpen = (
  targetAgentId: unknown,
  state: AgentOpenSessionState
): boolean => {
  const target = normalizeAgentId(targetAgentId);
  const activeSessionId = String(state.activeSessionId || '').trim();
  const activeConversationKey = String(state.activeConversationKey || '').trim();
  if (activeSessionId && activeConversationKey === `agent:${activeSessionId}`) {
    return resolveOpenActiveSessionAgentId(state, activeSessionId) === target;
  }
  const activeDraftAgentId = normalizeAgentId(state.draftAgentId);
  return !activeSessionId &&
    activeDraftAgentId === target &&
    activeConversationKey === `agent:draft:${target}`;
};
