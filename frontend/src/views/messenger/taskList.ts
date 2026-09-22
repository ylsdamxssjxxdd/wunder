export type TaskListItem = { id: string; title: string; preview: string; locked: boolean; createdAt: number };

export function taskWindow(count: number, top: number, height: number, rowHeight: number) {
  const size = Math.ceil(Math.max(rowHeight, height) / rowHeight) + 8;
  const start = Math.max(0, Math.min(Math.floor(Math.max(0, top) / rowHeight) - 4, count - size));
  return { start, end: Math.min(count, start + size) };
}

export function buildTaskList(sessions: Record<string, any>[], agentId: string, fallbackTitle: string): TaskListItem[] {
  const normalizeAgent = (value: unknown) => String(value || '').trim().replace(/^(?:default|__default__)$/, '');
  const seen = new Set<string>();
  return sessions.filter((item) => {
    const id = String(item.id || '').trim();
    if (!id || seen.has(id) || item.status === 'archived' || normalizeAgent(item.agent_id) !== normalizeAgent(agentId)) return false;
    seen.add(id);
    return true;
  }).map((item) => ({
    id: String(item.id), title: String(item.title || fallbackTitle),
    // List summaries are bounded fields; never materialize transcripts for a sidebar.
    preview: String(item.last_user_message_preview || item.last_message_preview || item.summary || '').slice(0, 120),
    locked: Boolean(item.orchestration_lock?.active),
    createdAt: typeof item.created_at === 'number' ? item.created_at : Date.parse(item.created_at || '') || 0
  })).sort((a, b) => b.createdAt - a.createdAt || a.id.localeCompare(b.id));
}
