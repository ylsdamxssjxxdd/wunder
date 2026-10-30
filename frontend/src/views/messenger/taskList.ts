import { readSessionQuotaUsed } from '@/stores/chatSessionQuota';
import type { AgentRuntimeState } from './model';

export type TaskListItem = {
  id: string;
  title: string;
  locked: boolean;
  runtimeStatus: string;
  createdAt: number;
  /** 最后一轮落库的时间（毫秒）；侧栏悬停提示用它回答「这条线程什么时候完成过」。 */
  lastActiveAt: number;
  consumedTokens: number;
  toolCalls: number;
  modelRequestCount: number | null;
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

export type TaskRowRange = { start: number; end: number };
export type TaskRowStateResolver<T> = (item: T) => AgentRuntimeState;

/**
 * Resolve render state only for the rows inside the virtual window. The caller
 * passes the same range used for slicing, so a single session bump can never
 * fan out into a full-list state recompute (the sidebar dep-magnet).
 */
export function resolveVisibleTaskRows<T>(
  items: ReadonlyArray<T>,
  range: TaskRowRange,
  resolveState: TaskRowStateResolver<T>
): Array<T & { state: AgentRuntimeState }> {
  const rows: Array<T & { state: AgentRuntimeState }> = [];
  const end = Math.min(range.end, items.length);
  for (let index = Math.max(0, range.start); index < end; index += 1) {
    const item = items[index];
    if (item === undefined || item === null) {
      continue;
    }
    rows.push({ ...item, state: resolveState(item) });
  }
  return rows;
}

// Identity-keyed memo shared by the two call sites that build the same list from
// the same source within one render (`MessengerSidebar.vue` and the controller's
// right-dock session history). They pass the very same `sessions` array plus the
// same agent id / new-session label, so the second call reuses the first
// computation instead of rebuilding it. A mutated/replaced array identity (or a
// different agent/label) naturally misses and recomputes; the memo never changes
// the returned value, only whether it was recomputed.
type TaskListMemoEntry = { signature: string; items: TaskListItem[] };
const taskListMemoCache = new WeakMap<Record<string, any>[], Map<string, TaskListMemoEntry>>();

const taskListInputSignature = (sessions: Record<string, any>[]): string => sessions
  .map((item) => [
    String(item?.id || ''),
    String(item?.agent_id || ''),
    String(item?.title || ''),
    String(item?.created_at ?? item?.createdAt ?? ''),
    String(item?.updated_at ?? item?.updatedAt ?? item?.last_message_at ?? item?.lastMessageAt ?? ''),
    String(item?.runtime_status ?? item?.runtimeStatus ?? item?.thread_status ?? item?.threadStatus ?? item?.status ?? ''),
    String(item?.consumed_tokens ?? item?.consumedTokens ?? ''),
    String(item?.tool_calls ?? item?.toolCalls ?? ''),
    String(item?.model_request_count ?? item?.modelRequestCount ?? item?.quota_used ?? item?.quotaUsed ?? readSessionQuotaUsed(item) ?? ''),
    item?.orchestration_lock?.active === true || item?.orchestrationLock?.active === true ? '1' : '0',
    item?.archived === true || item?.is_archived === true || Boolean(item?.archived_at || item?.archivedAt) ? '1' : '0',
    String(item?.parent_session_id ?? item?.parentSessionId ?? ''),
    String(item?.spawned_by ?? item?.spawnedBy ?? '')
  ].join('\u0001'))
  .join('\u0002');

export function buildTaskList(sessions: Record<string, any>[], agentId: string, fallbackTitle: string): TaskListItem[] {
  if (Array.isArray(sessions)) {
    let byInput = taskListMemoCache.get(sessions);
    if (!byInput) {
      byInput = new Map<string, TaskListMemoEntry>();
      taskListMemoCache.set(sessions, byInput);
    }
    const memoKey = `${String(agentId ?? '')}\u0000${String(fallbackTitle ?? '')}`;
    const signature = taskListInputSignature(sessions);
    const cached = byInput.get(memoKey);
    if (cached?.signature === signature) {
      return cached.items;
    }
    const built = computeTaskList(sessions, agentId, fallbackTitle);
    byInput.set(memoKey, { signature, items: built });
    return built;
  }
  return computeTaskList(sessions, agentId, fallbackTitle);
}

function computeTaskList(sessions: Record<string, any>[], agentId: string, fallbackTitle: string): TaskListItem[] {
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
    lastActiveAt: resolveTaskActivityTimestamp(item),
    consumedTokens: normalizeCount(item.consumed_tokens ?? item.consumedTokens),
    toolCalls: normalizeCount(item.tool_calls ?? item.toolCalls),
    modelRequestCount: readSessionQuotaUsed(item),
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

/**
 * Activity time is what the sidebar tooltip shows: the newest persisted turn.
 * `updated_at` also moves on rename, so it only serves as a fallback below the
 * message timestamp.
 */
function resolveTaskActivityTimestamp(item: Record<string, any>): number {
  const candidates = [item.last_message_at, item.lastMessageAt, item.updated_at, item.updatedAt, item.created_at, item.createdAt];
  for (const candidate of candidates) {
    if (typeof candidate === 'number' && Number.isFinite(candidate) && candidate > 0) {
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
