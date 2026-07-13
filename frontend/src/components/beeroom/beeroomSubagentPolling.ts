export type BeeroomSubagentPollingTask = {
  spawned_session_id?: string | null;
  target_session_id?: string | null;
  status?: string;
  updated_time?: number;
  started_time?: number | null;
  finished_time?: number | null;
};

export type BeeroomSubagentPollingItem = { status?: string };

const ACTIVE_TASK_STATUSES = new Set(['queued', 'accepted', 'running', 'waiting', 'awaiting_idle', 'resuming']);
const ACTIVE_SUBAGENT_STATUSES = new Set(['queued', 'running', 'waiting', 'awaiting_approval', 'resuming']);
const RECENT_TASK_GRACE_S = 18;

const normalizeText = (value: unknown): string => String(value || '').trim();

export const shouldPollBeeroomTaskSubagents = (
  task: BeeroomSubagentPollingTask,
  knownItems: BeeroomSubagentPollingItem[] = [],
  nowSeconds = Math.floor(Date.now() / 1000)
): boolean => {
  if (!normalizeText(task.spawned_session_id || task.target_session_id)) return false;
  if (ACTIVE_TASK_STATUSES.has(normalizeText(task.status).toLowerCase())) return true;
  if (knownItems.some((item) => ACTIVE_SUBAGENT_STATUSES.has(normalizeText(item.status).toLowerCase()))) {
    return true;
  }
  const updatedTime = Number(task.updated_time || task.finished_time || task.started_time || 0);
  return Number.isFinite(updatedTime) && updatedTime > 0 && nowSeconds - updatedTime <= RECENT_TASK_GRACE_S;
};
