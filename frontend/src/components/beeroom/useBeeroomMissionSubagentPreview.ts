import { getSessionEvents, getSessionSubagents } from '@/api/chat';
import type { BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import { chatDebugLog } from '@/utils/chatDebug';
import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';

import {
  buildSessionWorkflowItems,
  compareBeeroomMissionTasksByDisplayPriority,
  isBeeroomTaskStatusActive,
  resolveBeeroomTaskMoment,
  type BeeroomWorkflowItem
} from './beeroomTaskWorkflow';

export type BeeroomMissionSubagentItem = {
  key: string;
  sessionId: string;
  runId: string;
  agentId: string;
  title: string;
  label: string;
  status: string;
  summary: string;
  userMessage: string;
  assistantMessage: string;
  errorMessage: string;
  updatedTime: number;
  terminal: boolean;
  failed: boolean;
  depth: number | null;
  role: string;
  controlScope: string;
  spawnMode: string;
  strategy: string;
  dispatchLabel: string;
  controllerSessionId: string;
  parentSessionId: string;
  workflowItems: BeeroomWorkflowItem[];
};

type TaskSubagentFetchMeta = {
  requestKey: string;
  fetchedAt: number;
};

type SessionWorkflowFetchMeta = {
  requestKey: string;
  fetchedAt: number;
};

const SUBAGENT_POLL_INTERVAL_MS = 1400;
const SUBAGENT_LIST_LIMIT = 64;

const ACTIVE_SUBAGENT_STATUSES = new Set(['running', 'waiting', 'queued', 'accepted', 'cancelling']);

const clipDebugText = (value: unknown, limit = 120) => {
  const text = String(value || '').trim().replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const summarizeDebugError = (error: unknown) => {
  const source = error as { name?: unknown; message?: unknown } | null;
  const name = String(source?.name || '').trim();
  const message = String(source?.message || '').trim();
  return [name, message].filter(Boolean).join(': ') || String(error || '').trim();
};

const summarizeDebugSubagent = (item: BeeroomMissionSubagentItem) => ({
  key: item.key,
  runId: item.runId,
  sessionId: item.sessionId,
  status: item.status,
  terminal: item.terminal,
  failed: item.failed,
  updatedTime: item.updatedTime,
  summary: clipDebugText(item.summary)
});

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeOptionalCount = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeSubagentUpdatedTime = (value: unknown): number => {
  if (value === null || value === undefined) return 0;
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return 0;
    return value > 1_000_000_000_000 ? value / 1000 : value;
  }
  const text = String(value).trim();
  if (!text) return 0;
  if (/^-?\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric)) return 0;
    return numeric > 1_000_000_000_000 ? numeric / 1000 : numeric;
  }
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed / 1000 : 0;
};

const normalizeSubagentStatus = (value: unknown): string => {
  const normalized = normalizeText(value).toLowerCase();
  return normalized || 'running';
};

const resolveSubagentWorkflowKey = (
  item: Pick<BeeroomMissionSubagentItem, 'sessionId' | 'runId' | 'key'>
): string => normalizeText(item.sessionId || item.runId || item.key);

const buildWorkflowItemsFingerprint = (items: BeeroomWorkflowItem[]): string =>
  items
    .map((item) =>
      [
        item.id,
        item.title,
        item.detail,
        item.status,
        item.eventType,
        item.toolName,
        item.toolCallId
      ].join(':')
    )
    .join('||');

export const normalizeBeeroomMissionSubagentItem = (
  value: unknown
): BeeroomMissionSubagentItem | null => {
  const source = asRecord(value);
  if (!source) return null;
  const sessionId = normalizeText(source.session_id ?? source.sessionId);
  const runId = normalizeText(source.run_id ?? source.runId);
  const key = runId || sessionId;
  if (!key) return null;
  const status = normalizeSubagentStatus(source.status);
  const detail = asRecord(source.detail) || asRecord(source.metadata) || {};
  const userMessage = normalizeText(
    source.user_message ??
      source.userMessage ??
      detail.user_message ??
      detail.userMessage
  );
  const assistantMessage = normalizeText(
    source.assistant_message ??
      source.assistantMessage ??
      detail.assistant_message ??
      detail.assistantMessage
  );
  const errorMessage = normalizeText(
    source.error_message ??
      source.errorMessage ??
      detail.error_message ??
      detail.errorMessage ??
      source.error
  );
  const summary = normalizeText(
    source.summary ??
      assistantMessage ??
      errorMessage
  );
  const title = normalizeText(source.title) || normalizeText(source.label) || sessionId || runId || 'subagent';
  const updatedTime = normalizeSubagentUpdatedTime(
    source.updated_time ??
      source.updatedTime ??
      source.finished_time ??
      source.finishedTime ??
      source.started_time ??
      source.startedTime
  );

  return {
    key,
    sessionId,
    runId,
    agentId: normalizeText(source.agent_id ?? source.agentId),
    title,
    label: normalizeText(source.label ?? source.spawn_label ?? source.spawnLabel),
    status,
    summary,
    userMessage,
    assistantMessage,
    errorMessage,
    updatedTime,
    terminal: Boolean(source.terminal),
    failed: Boolean(source.failed),
    depth: normalizeOptionalCount(source.depth ?? detail.depth),
    role: normalizeText(source.role ?? detail.role),
    controlScope: normalizeText(source.control_scope ?? source.controlScope ?? detail.control_scope),
    spawnMode: normalizeText(source.spawn_mode ?? source.spawnMode ?? detail.spawn_mode),
    strategy: normalizeText(source.strategy ?? detail.strategy),
    dispatchLabel: normalizeText(
      source.dispatch_label ?? source.dispatchLabel ?? detail.dispatch_label ?? source.label
    ),
    controllerSessionId: normalizeText(
      source.controller_session_id ?? source.controllerSessionId ?? detail.controller_session_id
    ),
    parentSessionId: normalizeText(source.parent_session_id ?? source.parentSessionId),
    workflowItems: []
  };
};

const buildSubagentFingerprint = (item: BeeroomMissionSubagentItem) =>
  [
    item.key,
    item.sessionId,
    item.runId,
    item.agentId,
    item.status,
    item.summary,
    item.userMessage,
    item.assistantMessage,
    item.errorMessage,
    item.updatedTime,
    item.terminal,
    item.failed,
    item.depth,
    item.role,
    item.controlScope,
    item.spawnMode,
    item.strategy,
    item.dispatchLabel
  ].join('|');

const sameSubagentList = (
  left: BeeroomMissionSubagentItem[],
  right: BeeroomMissionSubagentItem[]
): boolean => {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    if (buildSubagentFingerprint(left[index]) !== buildSubagentFingerprint(right[index])) {
      return false;
    }
  }
  return true;
};

const resolveTaskSessionId = (task: BeeroomMissionTask): string =>
  normalizeText(task.spawned_session_id || task.target_session_id);

const buildTaskRequestKey = (task: BeeroomMissionTask): string =>
  [
    normalizeText(task.task_id),
    normalizeText(task.agent_id),
    normalizeText(task.status),
    normalizeText(task.started_time),
    normalizeText(task.finished_time),
    normalizeText(task.updated_time),
    normalizeText(task.spawned_session_id),
    normalizeText(task.target_session_id),
    normalizeText(task.session_run_id),
    normalizeText(task.result_summary),
    normalizeText(task.error)
  ].join('|');

const pickLatestMissionTasks = (mission: BeeroomMission | null | undefined): BeeroomMissionTask[] => {
  const source = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const latestByAgent = new Map<string, BeeroomMissionTask>();

  source.forEach((task) => {
    const agentId = normalizeText(task.agent_id);
    if (!agentId) return;
    const current = latestByAgent.get(agentId);
    if (!current || compareBeeroomMissionTasksByDisplayPriority(task, current) < 0) {
      latestByAgent.set(agentId, task);
    }
  });

  return Array.from(latestByAgent.values()).sort((left, right) => {
    const timeDiff = resolveBeeroomTaskMoment(right) - resolveBeeroomTaskMoment(left);
    if (timeDiff !== 0) return timeDiff;
    return normalizeText(left.agent_id).localeCompare(normalizeText(right.agent_id), 'zh-Hans-CN');
  });
};

export const useBeeroomMissionSubagentPreview = (options: {
  mission: Ref<BeeroomMission | null>;
  clearedAfter: Ref<number>;
  t: (key: string, params?: Record<string, unknown>) => string;
}) => {
  const logBeeroomSubagents = (event: string, payload?: unknown) => {
    chatDebugLog('beeroom.subagents', event, payload);
  };
  const rawSubagentsByTask = ref<Record<string, BeeroomMissionSubagentItem[]>>({});
  const workflowItemsBySubagent = ref<Record<string, BeeroomWorkflowItem[]>>({});

  const latestMissionTasks = computed(() => pickLatestMissionTasks(options.mission.value));
  const filteredSubagentsByTask = computed<Record<string, BeeroomMissionSubagentItem[]>>(() => {
    const clearedAfter = Number(options.clearedAfter.value || 0);
    const next: Record<string, BeeroomMissionSubagentItem[]> = {};
    Object.entries(rawSubagentsByTask.value).forEach(([taskId, items]) => {
      const filtered = items
        .filter((item) => !clearedAfter || Number(item.updatedTime || 0) > clearedAfter)
        .map((item) => {
          const workflowKey = resolveSubagentWorkflowKey(item);
          const workflowItems = workflowKey ? workflowItemsBySubagent.value[workflowKey] || [] : [];
          return {
            ...item,
            workflowItems
          } satisfies BeeroomMissionSubagentItem;
        });
      if (filtered.length > 0) {
        next[taskId] = filtered;
      }
    });
    return next;
  });

  let mounted = false;
  let disposed = false;
  let syncTimer: number | null = null;
  const fetchMeta = new Map<string, TaskSubagentFetchMeta>();
  const workflowFetchMeta = new Map<string, SessionWorkflowFetchMeta>();
  const activeControllers = new Map<string, AbortController>();
  const workflowControllers = new Map<string, AbortController>();
  const inFlightRequestKeys = new Map<string, string>();
  const workflowInFlightRequestKeys = new Map<string, string>();

  const clearSyncTimer = () => {
    if (syncTimer !== null && typeof window !== 'undefined') {
      window.clearTimeout(syncTimer);
      syncTimer = null;
    }
  };

  const updateTaskSubagents = (taskId: string, items: BeeroomMissionSubagentItem[]) => {
    const current = rawSubagentsByTask.value[taskId] || [];
    if (sameSubagentList(current, items)) {
      return;
    }
    rawSubagentsByTask.value = {
      ...rawSubagentsByTask.value,
      [taskId]: items
    };
  };

  const updateSubagentWorkflowItems = (workflowKey: string, items: BeeroomWorkflowItem[]) => {
    const current = workflowItemsBySubagent.value[workflowKey] || [];
    if (buildWorkflowItemsFingerprint(current) === buildWorkflowItemsFingerprint(items)) {
      return;
    }
    workflowItemsBySubagent.value = {
      ...workflowItemsBySubagent.value,
      [workflowKey]: items
    };
  };

  const pruneStaleWorkflowState = () => {
    const activeWorkflowKeys = new Set(
      Object.values(rawSubagentsByTask.value)
        .flatMap((items) => items.map((item) => resolveSubagentWorkflowKey(item)))
        .filter(Boolean)
    );
    const nextWorkflowItems = { ...workflowItemsBySubagent.value };
    let changed = false;
    Object.keys(nextWorkflowItems).forEach((workflowKey) => {
      if (activeWorkflowKeys.has(workflowKey)) return;
      delete nextWorkflowItems[workflowKey];
      workflowFetchMeta.delete(workflowKey);
      const controller = workflowControllers.get(workflowKey);
      if (controller) {
        controller.abort();
        workflowControllers.delete(workflowKey);
      }
      workflowInFlightRequestKeys.delete(workflowKey);
      changed = true;
    });
    if (changed) {
      workflowItemsBySubagent.value = nextWorkflowItems;
    }
  };

  const removeStaleTaskState = (taskIds: Set<string>) => {
    const next = { ...rawSubagentsByTask.value };
    let changed = false;
    Object.keys(next).forEach((taskId) => {
      if (taskIds.has(taskId)) return;
      delete next[taskId];
      fetchMeta.delete(taskId);
      const controller = activeControllers.get(taskId);
      if (controller) {
        controller.abort();
        activeControllers.delete(taskId);
      }
      inFlightRequestKeys.delete(taskId);
      changed = true;
    });
    if (changed) {
      rawSubagentsByTask.value = next;
    }
    pruneStaleWorkflowState();
  };

  const fetchSubagentWorkflow = async (item: BeeroomMissionSubagentItem, force = false) => {
    const workflowKey = resolveSubagentWorkflowKey(item);
    if (!workflowKey) return;
    const sessionId = normalizeText(item.sessionId);
    const requestKey = [
      workflowKey,
      sessionId,
      item.runId,
      item.status,
      item.updatedTime,
      item.terminal,
      item.failed,
      item.summary
    ].join('|');
    const previous = workflowFetchMeta.get(workflowKey);
    const isActiveSubagent = ACTIVE_SUBAGENT_STATUSES.has(item.status);

    if (
      !force &&
      previous?.requestKey === requestKey &&
      (!isActiveSubagent || Date.now() - previous.fetchedAt < SUBAGENT_POLL_INTERVAL_MS - 120)
    ) {
      return;
    }

    if (!sessionId) {
      updateSubagentWorkflowItems(workflowKey, []);
      workflowFetchMeta.set(workflowKey, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('workflow-clear-empty-session', {
        workflowKey,
        requestKey
      });
      return;
    }

    if (workflowInFlightRequestKeys.get(workflowKey) === requestKey) {
      return;
    }

    const previousController = workflowControllers.get(workflowKey);
    if (previousController) {
      previousController.abort();
      workflowControllers.delete(workflowKey);
    }

    const controller = new AbortController();
    workflowControllers.set(workflowKey, controller);
    workflowInFlightRequestKeys.set(workflowKey, requestKey);

    try {
      const response = await getSessionEvents(sessionId, { signal: controller.signal });
      if (disposed || !mounted || controller.signal.aborted) return;
      const rounds = Array.isArray(response?.data?.data?.rounds) ? response.data.data.rounds : [];
      const workflowItems = buildSessionWorkflowItems(rounds, options.t).filter((workflowItem) => workflowItem.isTool);
      updateSubagentWorkflowItems(workflowKey, workflowItems);
      workflowFetchMeta.set(workflowKey, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('workflow-fetch-result', {
        workflowKey,
        sessionId,
        requestKey,
        workflowCount: workflowItems.length
      });
    } catch (error) {
      if (disposed || controller.signal.aborted) return;
      updateSubagentWorkflowItems(workflowKey, []);
      workflowFetchMeta.set(workflowKey, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('workflow-fetch-error', {
        workflowKey,
        sessionId,
        requestKey,
        error: summarizeDebugError(error)
      });
    } finally {
      if (workflowControllers.get(workflowKey) === controller) {
        workflowControllers.delete(workflowKey);
      }
      if (workflowInFlightRequestKeys.get(workflowKey) === requestKey) {
        workflowInFlightRequestKeys.delete(workflowKey);
      }
    }
  };

  const fetchTaskSubagents = async (task: BeeroomMissionTask, force = false) => {
    const taskId = normalizeText(task.task_id);
    if (!taskId) return;
    const sessionId = resolveTaskSessionId(task);
    const requestKey = buildTaskRequestKey(task);
    const previous = fetchMeta.get(taskId);
    const isActiveTask = isBeeroomTaskStatusActive(task.status);

    if (
      !force &&
      previous?.requestKey === requestKey &&
      (!isActiveTask || Date.now() - previous.fetchedAt < SUBAGENT_POLL_INTERVAL_MS - 120)
    ) {
      return;
    }

    if (!sessionId) {
      updateTaskSubagents(taskId, []);
      pruneStaleWorkflowState();
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('fetch-task-subagents:clear-empty-session', {
        taskId,
        requestKey,
        taskStatus: task.status
      });
      return;
    }

    if (inFlightRequestKeys.get(taskId) === requestKey) {
      return;
    }

    const previousController = activeControllers.get(taskId);
    if (previousController) {
      previousController.abort();
      activeControllers.delete(taskId);
    }

    const controller = new AbortController();
    activeControllers.set(taskId, controller);
    inFlightRequestKeys.set(taskId, requestKey);

    try {
      const response = await getSessionSubagents(
        sessionId,
        {
          limit: SUBAGENT_LIST_LIMIT
        },
        { signal: controller.signal }
      );
      if (disposed || !mounted || controller.signal.aborted) return;
      const source = Array.isArray(response?.data?.data?.items) ? response.data.data.items : [];
      const normalized = source
        .map((item: unknown) => normalizeBeeroomMissionSubagentItem(item))
        .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item))
        .sort((left, right) => {
          const activeDiff =
            Number(ACTIVE_SUBAGENT_STATUSES.has(right.status)) - Number(ACTIVE_SUBAGENT_STATUSES.has(left.status));
          if (activeDiff !== 0) return activeDiff;
          return Number(right.updatedTime || 0) - Number(left.updatedTime || 0);
        });
      updateTaskSubagents(taskId, normalized);
      pruneStaleWorkflowState();
      await Promise.allSettled(normalized.map((item) => fetchSubagentWorkflow(item, force)));
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('fetch-task-subagents:result', {
        taskId,
        sessionId,
        taskStatus: task.status,
        force,
        count: normalized.length,
        items: normalized.slice(0, 8).map((item) => summarizeDebugSubagent(item))
      });
    } catch (error) {
      if (disposed || controller.signal.aborted) return;
      updateTaskSubagents(taskId, []);
      pruneStaleWorkflowState();
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
      logBeeroomSubagents('fetch-task-subagents:error', {
        taskId,
        sessionId,
        taskStatus: task.status,
        requestKey,
        error: summarizeDebugError(error)
      });
    } finally {
      if (activeControllers.get(taskId) === controller) {
        activeControllers.delete(taskId);
      }
      if (inFlightRequestKeys.get(taskId) === requestKey) {
        inFlightRequestKeys.delete(taskId);
      }
    }
  };

  const scheduleSync = () => {
    clearSyncTimer();
    if (!mounted || disposed || typeof window === 'undefined') return;
    const hasActiveTask = latestMissionTasks.value.some((task) => isBeeroomTaskStatusActive(task.status));
    const hasActiveSubagent = Object.values(rawSubagentsByTask.value).some((items) =>
      items.some((item) => ACTIVE_SUBAGENT_STATUSES.has(item.status))
    );
    if (!hasActiveTask && !hasActiveSubagent) return;
    syncTimer = window.setTimeout(() => {
      syncTimer = null;
      if (disposed || !mounted) return;
      void syncMissionSubagentState(false);
    }, SUBAGENT_POLL_INTERVAL_MS);
  };

  const syncMissionSubagentState = async (force = false) => {
    if (disposed || !mounted) return;
    const tasks = latestMissionTasks.value;
    const activeTaskIds = new Set(tasks.map((task) => normalizeText(task.task_id)).filter(Boolean));
    removeStaleTaskState(activeTaskIds);
    if (!tasks.length) {
      clearSyncTimer();
      return;
    }
    await Promise.all(tasks.map((task) => fetchTaskSubagents(task, force)));
    scheduleSync();
  };

  watch(
    () =>
      latestMissionTasks.value
        .map((task) => buildTaskRequestKey(task))
        .join('||'),
    () => {
      if (!mounted || disposed) return;
      void syncMissionSubagentState(true);
    }
  );

  onMounted(() => {
    mounted = true;
    void syncMissionSubagentState(true);
  });

  onBeforeUnmount(() => {
    disposed = true;
    mounted = false;
    clearSyncTimer();
    activeControllers.forEach((controller) => controller.abort());
    activeControllers.clear();
    workflowControllers.forEach((controller) => controller.abort());
    workflowControllers.clear();
    inFlightRequestKeys.clear();
    workflowInFlightRequestKeys.clear();
    fetchMeta.clear();
    workflowFetchMeta.clear();
  });

  return {
    subagentsByTask: filteredSubagentsByTask
  };
};
