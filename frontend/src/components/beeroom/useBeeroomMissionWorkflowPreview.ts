import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';

import { getSessionEvents } from '@/api/chat';
import type { BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';

import {
  buildTaskWorkflowRuntime,
  compareBeeroomMissionTasksByDisplayPriority,
  isBeeroomTaskStatusActive,
  resolveBeeroomTaskMoment,
  type BeeroomTaskWorkflowPreview,
  type BeeroomWorkflowItem
} from './beeroomTaskWorkflow';

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

type TaskWorkflowFetchMeta = {
  requestKey: string;
  fetchedAt: number;
};

const WORKFLOW_POLL_INTERVAL_MS = 1200;

const normalizeText = (value: unknown): string => String(value || '').trim();

const resolveTaskSessionId = (task: BeeroomMissionTask): string =>
  normalizeText(task.spawned_session_id || task.target_session_id);

const buildTaskRequestKey = (task: BeeroomMissionTask): string =>
  [
    task.task_id,
    normalizeText(task.agent_id),
    normalizeText(task.status),
    normalizeText(task.updated_time),
    normalizeText(task.started_time),
    normalizeText(task.finished_time),
    normalizeText(task.spawned_session_id),
    normalizeText(task.target_session_id),
    normalizeText(task.session_run_id),
    normalizeText(task.retry_count),
    normalizeText(task.result_summary),
    normalizeText(task.error)
  ].join('|');

const buildWorkflowItemsFingerprint = (items: BeeroomWorkflowItem[]): string =>
  items
    .map((item) => [item.id, item.title, item.detail, item.status, item.eventType, item.toolName, item.toolCallId].join(':'))
    .join('||');

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

// Keep session-event requests scoped to the latest task per worker so the canvas stays light.
export const useBeeroomMissionWorkflowPreview = (options: {
  mission: Ref<BeeroomMission | null>;
  t: TranslationFn;
}) => {
  const workflowItemsByTask = ref<Record<string, BeeroomWorkflowItem[]>>({});
  const workflowPreviewByTask = ref<Record<string, BeeroomTaskWorkflowPreview>>({});
  const workflowLoadingByTask = ref<Record<string, boolean>>({});

  const latestMissionTasks = computed(() => pickLatestMissionTasks(options.mission.value));
  const workflowPreviewSignature = computed(() =>
    Object.entries(workflowPreviewByTask.value)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([taskId, preview]) => `${taskId}:${preview.fingerprint}`)
      .join('||')
  );
  const workflowItemsSignature = computed(() =>
    Object.entries(workflowItemsByTask.value)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([taskId, items]) => `${taskId}:${buildWorkflowItemsFingerprint(items)}`)
      .join('||')
  );
  const latestTaskSignature = computed(() =>
    latestMissionTasks.value.map((task) => buildTaskRequestKey(task)).join('||')
  );

  let mounted = false;
  let disposed = false;
  let syncTimer: number | null = null;
  const fetchMeta = new Map<string, TaskWorkflowFetchMeta>();
  const activeControllers = new Map<string, AbortController>();
  const inFlightRequestKeys = new Map<string, string>();

  const clearSyncTimer = () => {
    if (syncTimer !== null && typeof window !== 'undefined') {
      window.clearTimeout(syncTimer);
      syncTimer = null;
    }
  };

  const updateTaskWorkflowState = (
    taskId: string,
    payload: { items: BeeroomWorkflowItem[]; preview: BeeroomTaskWorkflowPreview }
  ) => {
    const currentPreview = workflowPreviewByTask.value[taskId];
    const currentItems = workflowItemsByTask.value[taskId] || [];
    if (
      currentPreview?.fingerprint === payload.preview.fingerprint &&
      buildWorkflowItemsFingerprint(currentItems) === buildWorkflowItemsFingerprint(payload.items)
    ) {
      return;
    }
    workflowItemsByTask.value = {
      ...workflowItemsByTask.value,
      [taskId]: payload.items
    };
    workflowPreviewByTask.value = {
      ...workflowPreviewByTask.value,
      [taskId]: payload.preview
    };
  };

  const removeStaleTaskState = (taskIds: Set<string>) => {
    const nextItems = { ...workflowItemsByTask.value };
    const nextPreviews = { ...workflowPreviewByTask.value };
    const nextLoading = { ...workflowLoadingByTask.value };
    let changed = false;
    Object.keys(nextItems).forEach((taskId) => {
      if (taskIds.has(taskId)) return;
      delete nextItems[taskId];
      delete nextPreviews[taskId];
      delete nextLoading[taskId];
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
      workflowItemsByTask.value = nextItems;
      workflowPreviewByTask.value = nextPreviews;
      workflowLoadingByTask.value = nextLoading;
    }
  };

  const setTaskLoading = (taskId: string, loading: boolean) => {
    if (workflowLoadingByTask.value[taskId] === loading) return;
    workflowLoadingByTask.value = {
      ...workflowLoadingByTask.value,
      [taskId]: loading
    };
  };

  const fetchTaskWorkflow = async (task: BeeroomMissionTask, force = false) => {
    const taskId = normalizeText(task.task_id);
    if (!taskId) return;

    const sessionId = resolveTaskSessionId(task);
    const requestKey = buildTaskRequestKey(task);
    const previous = fetchMeta.get(taskId);
    const isActive = isBeeroomTaskStatusActive(task.status);

    if (
      !force &&
      previous?.requestKey === requestKey &&
      (!isActive || Date.now() - previous.fetchedAt < WORKFLOW_POLL_INTERVAL_MS - 120)
    ) {
      return;
    }

    if (!sessionId) {
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, [], options.t));
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
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
    setTaskLoading(taskId, true);

    try {
      const response = await getSessionEvents(sessionId, { signal: controller.signal });
      if (disposed || !mounted || controller.signal.aborted) return;
      const rounds = Array.isArray(response?.data?.data?.rounds) ? response.data.data.rounds : [];
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, rounds, options.t));
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
    } catch {
      if (disposed || controller.signal.aborted) return;
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, [], options.t));
      fetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
    } finally {
      if (activeControllers.get(taskId) === controller) {
        activeControllers.delete(taskId);
      }
      if (inFlightRequestKeys.get(taskId) === requestKey) {
        inFlightRequestKeys.delete(taskId);
      }
      setTaskLoading(taskId, false);
    }
  };

  const scheduleSync = () => {
    clearSyncTimer();
    if (!mounted || disposed || typeof window === 'undefined') return;
    if (!latestMissionTasks.value.some((task) => isBeeroomTaskStatusActive(task.status))) return;
    syncTimer = window.setTimeout(() => {
      syncTimer = null;
      if (disposed || !mounted) return;
      void syncMissionWorkflowState(false);
    }, WORKFLOW_POLL_INTERVAL_MS);
  };

  const syncMissionWorkflowState = async (force = false) => {
    if (disposed || !mounted) return;
    const tasks = latestMissionTasks.value;
    removeStaleTaskState(new Set(tasks.map((task) => normalizeText(task.task_id)).filter(Boolean)));
    if (!tasks.length) {
      clearSyncTimer();
      return;
    }

    await Promise.allSettled(tasks.map((task) => fetchTaskWorkflow(task, force)));
    scheduleSync();
  };

  const isTaskWorkflowLoading = (taskId: string): boolean => Boolean(workflowLoadingByTask.value[taskId]);

  watch(latestTaskSignature, () => {
    if (!mounted || disposed) return;
    void syncMissionWorkflowState(true);
  });

  onMounted(() => {
    mounted = true;
    void syncMissionWorkflowState(true);
  });

  onBeforeUnmount(() => {
    disposed = true;
    mounted = false;
    clearSyncTimer();
    activeControllers.forEach((controller) => controller.abort());
    activeControllers.clear();
    inFlightRequestKeys.clear();
    fetchMeta.clear();
  });

  return {
    workflowItemsByTask,
    workflowItemsSignature,
    workflowPreviewByTask,
    workflowPreviewSignature,
    isTaskWorkflowLoading,
    syncMissionWorkflowState
  };
};
