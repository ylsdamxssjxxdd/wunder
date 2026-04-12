import { getSessionEvents, getSessionSubagents } from '@/api/chat';
import { resolveBeeroomSwarmSubagentProjectionDecision } from '@/components/beeroom/canvas/beeroomSwarmSubagentProjection';
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
import {
  ACTIVE_BEEROOM_SUBAGENT_STATUSES,
  collectBeeroomHistoricalSubagentItems,
  flattenBeeroomSessionEventRounds,
  mergeBeeroomMissionSubagentItems,
  normalizeBeeroomMissionSubagentItem,
  sortBeeroomMissionSubagentItems,
  type BeeroomMissionSubagentItem
} from './beeroomMissionSubagentState';

export type { BeeroomMissionSubagentItem } from './beeroomMissionSubagentState';
export { normalizeBeeroomMissionSubagentItem } from './beeroomMissionSubagentState';

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
const RECENT_TASK_SUBAGENT_POLL_GRACE_S = 18;

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
  runKind: item.runKind,
  requestedBy: item.requestedBy,
  spawnedBy: item.spawnedBy,
  status: item.status,
  terminal: item.terminal,
  failed: item.failed,
  updatedTime: item.updatedTime,
  summary: clipDebugText(item.summary)
});

const summarizeProjectionDecision = (item: BeeroomMissionSubagentItem) => {
  const decision = resolveBeeroomSwarmSubagentProjectionDecision(item);
  return {
    key: item.key,
    sessionId: item.sessionId,
    runId: item.runId,
    runKind: item.runKind,
    requestedBy: item.requestedBy,
    projectable: decision.projectable,
    reason: decision.reason,
    status: item.status
  };
};

const normalizeText = (value: unknown): string => String(value || '').trim();

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


const buildSubagentFingerprint = (item: BeeroomMissionSubagentItem) =>
  [
    item.key,
    item.sessionId,
    item.runId,
    item.runKind,
    item.requestedBy,
    item.spawnedBy,
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
    item.dispatchLabel,
    item.parentTurnRef,
    item.parentUserRound,
    item.parentModelRound
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

export const shouldPollBeeroomTaskSubagents = (
  task: BeeroomMissionTask,
  knownItems: BeeroomMissionSubagentItem[] = [],
  nowSeconds = Math.floor(Date.now() / 1000)
) => {
  if (!resolveTaskSessionId(task)) return false;
  if (isBeeroomTaskStatusActive(task.status)) return true;
  if (knownItems.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status))) {
    return true;
  }
  const updatedTime = Number(resolveBeeroomTaskMoment(task) || 0);
  if (!updatedTime || !Number.isFinite(updatedTime)) return false;
  return nowSeconds - updatedTime <= RECENT_TASK_SUBAGENT_POLL_GRACE_S;
};

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

const pickMissionTasks = (mission: BeeroomMission | null | undefined): BeeroomMissionTask[] => {
  const source = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const latestByTaskId = new Map<string, BeeroomMissionTask>();

  source.forEach((task) => {
    const taskId = normalizeText(task.task_id);
    if (!taskId) return;
    const current = latestByTaskId.get(taskId);
    if (!current || compareBeeroomMissionTasksByDisplayPriority(task, current) < 0) {
      latestByTaskId.set(taskId, task);
    }
  });

  return Array.from(latestByTaskId.values()).sort((left, right) => {
    const timeDiff = resolveBeeroomTaskMoment(right) - resolveBeeroomTaskMoment(left);
    if (timeDiff !== 0) return timeDiff;
    const priorityDiff = compareBeeroomMissionTasksByDisplayPriority(left, right);
    if (priorityDiff !== 0) return priorityDiff;
    return normalizeText(left.task_id).localeCompare(normalizeText(right.task_id), 'zh-Hans-CN');
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

  const missionTasks = computed(() => pickMissionTasks(options.mission.value));
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
    const isActiveSubagent = ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status);

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
    const currentItems = rawSubagentsByTask.value[taskId] || [];
    const shouldPollTask = shouldPollBeeroomTaskSubagents(task, currentItems);

    if (
      !force &&
      previous?.requestKey === requestKey &&
      (!shouldPollTask || Date.now() - previous.fetchedAt < SUBAGENT_POLL_INTERVAL_MS - 120)
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
      let subagentsError: unknown = null;
      let eventsError: unknown = null;
      const [subagentsResponse, eventsResponse] = await Promise.all([
        getSessionSubagents(
          sessionId,
          {
            limit: SUBAGENT_LIST_LIMIT
          },
          { signal: controller.signal }
        ).catch((error) => {
          subagentsError = error;
          return null;
        }),
        getSessionEvents(sessionId, { signal: controller.signal }).catch((error) => {
          eventsError = error;
          return null;
        })
      ]);
      if (disposed || !mounted || controller.signal.aborted) return;
      if (!subagentsResponse && !eventsResponse) {
        throw subagentsError || eventsError || new Error('beeroom task subagent preview sync failed');
      }
      const source = Array.isArray(subagentsResponse?.data?.data?.items) ? subagentsResponse.data.data.items : [];
      const liveItems = source
        .map((item: unknown) => normalizeBeeroomMissionSubagentItem(item))
        .filter((item: BeeroomMissionSubagentItem | null): item is BeeroomMissionSubagentItem => Boolean(item))
        .map((item) => ({ ...item, workflowItems: [] as BeeroomWorkflowItem[] }));
      const events = flattenBeeroomSessionEventRounds(eventsResponse?.data?.data?.rounds);
      const historicalItems = collectBeeroomHistoricalSubagentItems(events).map((item) => ({
        ...item,
        workflowItems: [] as BeeroomWorkflowItem[]
      }));
      const normalized = sortBeeroomMissionSubagentItems(
        mergeBeeroomMissionSubagentItems(liveItems, historicalItems) as BeeroomMissionSubagentItem[]
      );
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
        liveCount: liveItems.length,
        historicalCount: historicalItems.length,
        canvasProjectableCount: normalized.filter((item) =>
          resolveBeeroomSwarmSubagentProjectionDecision(item).projectable
        ).length,
        items: normalized.slice(0, 8).map((item) => summarizeDebugSubagent(item)),
        projectionDecisions: normalized.slice(0, 8).map((item) => summarizeProjectionDecision(item))
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
    const hasPollableTask = missionTasks.value.some((task) =>
      shouldPollBeeroomTaskSubagents(task, rawSubagentsByTask.value[normalizeText(task.task_id)] || [])
    );
    const hasActiveSubagent = Object.values(rawSubagentsByTask.value).some((items) =>
      items.some((item) => ACTIVE_BEEROOM_SUBAGENT_STATUSES.has(item.status))
    );
    if (!hasPollableTask && !hasActiveSubagent) return;
    syncTimer = window.setTimeout(() => {
      syncTimer = null;
      if (disposed || !mounted) return;
      void syncMissionSubagentState(false);
    }, SUBAGENT_POLL_INTERVAL_MS);
  };

  const syncMissionSubagentState = async (force = false) => {
    if (disposed || !mounted) return;
    const tasks = missionTasks.value;
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
      missionTasks.value
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
    subagentsByTask: filteredSubagentsByTask,
    syncMissionSubagentState
  };
};
