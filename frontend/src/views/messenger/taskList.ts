import { readSessionQuotaUsed } from '@/stores/chatSessionQuota';

export type TaskListItem = {
  id: string;
  title: string;
  locked: boolean;
  runtimeStatus: string;
  createdAt: number;
  consumedTokens: number;
  toolCalls: number;
  quotaUsed: number | null;
};

export const isArchivedTaskRecord = (item: Record<string, any> | null | undefined): boolean =>
  Boolean(item && (
    String(item.status ?? '').trim().toLowerCase() === 'archived' ||
    item.archived === true ||
    item.is_archived === true ||
    item.isArchived === true ||
    item.archived_at ||
    item.archivedAt
  ));

export function isRootWorkThread(session: Record<string, any>): boolean {
  const parent = String(session?.parent_session_id ?? session?.parentSessionId ?? '').trim();
  const source = String(session?.spawned_by ?? session?.spawnedBy ?? '').trim();
  return !parent || !['model', 'subagent_control'].includes(source);
}

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
    // Child sessions are durable work units rendered in the parent subagent
    // projection; they must never become user-facing work-thread rows.
    if (!id || seen.has(id) || !isRootWorkThread(item) || isArchivedTaskRecord(item) || normalizeAgent(item.agent_id) !== normalizeAgent(agentId)) return false;
    seen.add(id);
    return true;
  }).map((item) => ({
    id: String(item.id), title: String(item.title || fallbackTitle),
    locked: Boolean(item.orchestration_lock?.active),
    runtimeStatus: String(
      item.runtime_status ?? item.runtimeStatus ?? item.thread_status ?? item.threadStatus ?? item.status ?? ''
    ).trim().toLowerCase(),
    createdAt: resolveTaskCreationTimestamp(item),
    consumedTokens: normalizeCount(item.consumed_tokens ?? item.consumedTokens),
    toolCalls: normalizeCount(item.tool_calls ?? item.toolCalls),
    quotaUsed: readSessionQuotaUsed(item)
  })).sort((a, b) => normalizeTimestamp(b.createdAt) - normalizeTimestamp(a.createdAt) || a.id.localeCompare(b.id));
}

function resolveTaskCreationTimestamp(item: Record<string, any>): number {
  // Creation time is immutable. Activity time must not reorder a manually
  // arranged list whenever an older thread receives a new event.
  const candidates = [item.created_at, item.createdAt, item.last_message_at, item.lastMessageAt, item.updated_at, item.updatedAt];
  for (const candidate of candidates) {
    if (typeof candidate === 'number' && Number.isFinite(candidate)) {
      return normalizeTimestamp(candidate);
    }
    const parsed = Date.parse(String(candidate || ''));
    if (Number.isFinite(parsed) && parsed > 0) return parsed;
  }
  return 0;
}

function normalizeTimestamp(value: number): number {
  // Keep small fixture/local counters intact; normalize real Unix-second
  // timestamps so numeric and ISO creation fields sort on one scale.
  return value > 100_000_000 && value < 1_000_000_000_000 ? value * 1000 : value;
}

function normalizeCount(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? Math.trunc(parsed) : 0;
}
