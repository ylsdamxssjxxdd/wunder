import { DEFAULT_AGENT_KEY } from './model';

export type RecentAgentSelection = {
  agentId: string;
  sessionId: string;
};

const STORAGE_KEY = 'wunder_messenger_recent_agent_selection_v1';

const normalizeAgentId = (value: unknown): string => {
  const text = String(value || '').trim();
  return text || DEFAULT_AGENT_KEY;
};

const normalizeSessionId = (value: unknown): string => String(value || '').trim();

export const buildRecentAgentSelection = (
  agentId: unknown,
  sessionId: unknown = ''
): RecentAgentSelection => ({
  agentId: normalizeAgentId(agentId),
  sessionId: normalizeSessionId(sessionId)
});

export const readRecentAgentSelection = (): RecentAgentSelection | null => {
  if (typeof window === 'undefined') return null;
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<RecentAgentSelection> | null;
    if (!parsed || typeof parsed !== 'object') return null;
    const rawAgentId = String(parsed.agentId || '').trim();
    const agentId = rawAgentId ? normalizeAgentId(rawAgentId) : '';
    const sessionId = normalizeSessionId(parsed.sessionId);
    return agentId || sessionId ? { agentId, sessionId } : null;
  } catch {
    return null;
  }
};

export const writeRecentAgentSelection = (selection: RecentAgentSelection): void => {
  if (typeof window === 'undefined') return;
  const normalized = buildRecentAgentSelection(selection.agentId, selection.sessionId);
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // Recent selection is a UI convenience; ignore storage failures.
  }
};
