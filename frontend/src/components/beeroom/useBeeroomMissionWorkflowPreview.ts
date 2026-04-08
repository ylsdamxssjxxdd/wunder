import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from 'vue';

import { getSessionEvents } from '@/api/chat';
import type { BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import { chatDebugLog } from '@/utils/chatDebug';

import {
  buildSessionWorkflowItems,
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

type MotherWorkflowFetchMeta = {
  requestKey: string;
  fetchedAt: number;
};

const WORKFLOW_POLL_INTERVAL_MS = 1200;

const normalizeText = (value: unknown): string => String(value || '').trim();

const clipDebugText = (value: unknown, limit = 120): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const summarizeDebugError = (error: unknown) => {
  const source = error as { name?: unknown; message?: unknown } | null;
  const name = normalizeText(source?.name);
  const message = normalizeText(source?.message);
  return [name, message].filter(Boolean).join(': ') || normalizeText(error);
};

const summarizeDebugWorkflowItems = (items: BeeroomWorkflowItem[]) =>
  items.slice(-4).map((item) => ({
    id: item.id,
    title: item.title,
    status: item.status,
    eventType: item.eventType,
    toolName: item.toolName,
    detail: clipDebugText(item.detail, 80)
  }));

const resolveTaskSessionId = (task: BeeroomMissionTask): string =>
  normalizeText(task.spawned_session_id || task.target_session_id);

const resolveMissionSessionId = (mission: BeeroomMission | null | undefined): string =>
  normalizeText(mission?.parent_session_id);

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

const buildMissionRequestKey = (mission: BeeroomMission | null | undefined): string =>
  [
    normalizeText(mission?.mission_id),
    resolveMissionSessionId(mission),
    normalizeText(mission?.status),
    normalizeText(mission?.completion_status),
    normalizeText(mission?.updated_time),
    normalizeText(mission?.started_time),
    normalizeText(mission?.finished_time),
    normalizeText(mission?.summary),
    normalizeText(mission?.error)
  ].join('|');

const buildWorkflowItemsFingerprint = (items: BeeroomWorkflowItem[]): string =>
  items
    .map((item) => [item.id, item.title, item.detail, item.status, item.eventType, item.toolName, item.toolCallId].join(':'))
    .join('||');

const isMissionWorkflowActive = (mission: BeeroomMission | null | undefined): boolean =>
  isBeeroomTaskStatusActive(mission?.completion_status || mission?.status);

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

// Keep workflow previews aligned with every task currently present in the selected mission.
export const useBeeroomMissionWorkflowPreview = (options: {
  mission: Ref<BeeroomMission | null>;
  t: TranslationFn;
}) => {
  const logBeeroomWorkflow = (event: string, payload?: unknown) => {
    chatDebugLog('beeroom.workflow', event, payload);
  };
  const workflowItemsByTask = ref<Record<string, BeeroomWorkflowItem[]>>({});
  const workflowPreviewByTask = ref<Record<string, BeeroomTaskWorkflowPreview>>({});
  const workflowLoadingByTask = ref<Record<string, boolean>>({});
  const motherWorkflowItems = ref<BeeroomWorkflowItem[]>([]);

  const missionTasks = computed(() => pickMissionTasks(options.mission.value));
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
    missionTasks.value.map((task) => buildTaskRequestKey(task)).join('||')
  );
  const motherWorkflowRequestSignature = computed(() => buildMissionRequestKey(options.mission.value));

  let mounted = false;
  let disposed = false;
  let syncTimer: number | null = null;
  const fetchMeta = new Map<string, TaskWorkflowFetchMeta>();
  const activeControllers = new Map<string, AbortController>();
  const inFlightRequestKeys = new Map<string, string>();
  let motherFetchMeta: MotherWorkflowFetchMeta | null = null;
  let motherController: AbortController | null = null;
  let motherInFlightRequestKey = '';
  let motherSessionId = '';

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

  const updateMotherWorkflowState = (items: BeeroomWorkflowItem[]) => {
    if (buildWorkflowItemsFingerprint(motherWorkflowItems.value) === buildWorkflowItemsFingerprint(items)) {
      return;
    }
    motherWorkflowItems.value = items;
  };

  const clearMotherWorkflowState = () => {
    if (motherController) {
      motherController.abort();
      motherController = null;
    }
    motherInFlightRequestKey = '';
    motherFetchMeta = null;
    motherSessionId = '';
    if (motherWorkflowItems.value.length > 0) {
      motherWorkflowItems.value = [];
    }
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

  const fetchMotherWorkflow = async (force = false) => {
    const mission = options.mission.value;
    const sessionId = resolveMissionSessionId(mission);
    if (!sessionId) {
      clearMotherWorkflowState();
      return;
    }

    const requestKey = buildMissionRequestKey(mission);
    const previous = motherFetchMeta;
    const isActive = isMissionWorkflowActive(mission);

    if (
      !force &&
      previous?.requestKey === requestKey &&
      (!isActive || Date.now() - previous.fetchedAt < WORKFLOW_POLL_INTERVAL_MS - 120)
    ) {
      return;
    }

    if (motherInFlightRequestKey === requestKey) {
      return;
    }

    if (motherSessionId && motherSessionId !== sessionId) {
      updateMotherWorkflowState([]);
    }
    motherSessionId = sessionId;

    if (motherController) {
      motherController.abort();
      motherController = null;
    }

    const controller = new AbortController();
    motherController = controller;
    motherInFlightRequestKey = requestKey;
    logBeeroomWorkflow('mother-sync-start', {
      sessionId,
      requestKey,
      force,
      missionStatus: normalizeText(mission?.completion_status || mission?.status)
    });

    try {
      const response = await getSessionEvents(sessionId, { signal: controller.signal });
      if (disposed || !mounted || controller.signal.aborted) return;
      const rounds = Array.isArray(response?.data?.data?.rounds) ? response.data.data.rounds : [];
      const items = buildSessionWorkflowItems(rounds, options.t);
      updateMotherWorkflowState(items);
      motherFetchMeta = { requestKey, fetchedAt: Date.now() };
      logBeeroomWorkflow('mother-sync-success', {
        sessionId,
        roundCount: rounds.length,
        itemCount: items.length,
        items: summarizeDebugWorkflowItems(items)
      });
    } catch (error) {
      if (disposed || controller.signal.aborted) return;
      motherFetchMeta = { requestKey, fetchedAt: Date.now() };
      logBeeroomWorkflow('mother-sync-error', {
        sessionId,
        error: summarizeDebugError(error)
      });
    } finally {
      if (motherController === controller) {
        motherController = null;
      }
      if (motherInFlightRequestKey === requestKey) {
        motherInFlightRequestKey = '';
      }
    }
  };

  const scheduleSync = () => {
    clearSyncTimer();
    if (!mounted || disposed || typeof window === 'undefined') return;
    if (
      !missionTasks.value.some((task) => isBeeroomTaskStatusActive(task.status)) &&
      !isMissionWorkflowActive(options.mission.value)
    ) {
      return;
    }
    syncTimer = window.setTimeout(() => {
      syncTimer = null;
      if (disposed || !mounted) return;
      void syncMissionWorkflowState(false);
    }, WORKFLOW_POLL_INTERVAL_MS);
  };

  const syncMissionWorkflowState = async (force = false) => {
    if (disposed || !mounted) return;
    const tasks = missionTasks.value;
    removeStaleTaskState(new Set(tasks.map((task) => normalizeText(task.task_id)).filter(Boolean)));
    if (!tasks.length && !resolveMissionSessionId(options.mission.value)) {
      clearMotherWorkflowState();
      clearSyncTimer();
      return;
    }

    await Promise.allSettled([
      ...tasks.map((task) => fetchTaskWorkflow(task, force)),
      fetchMotherWorkflow(force)
    ]);
    scheduleSync();
  };

  const isTaskWorkflowLoading = (taskId: string): boolean => Boolean(workflowLoadingByTask.value[taskId]);

  watch(latestTaskSignature, () => {
    if (!mounted || disposed) return;
    void syncMissionWorkflowState(true);
  });

  watch(motherWorkflowRequestSignature, () => {
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
    if (motherController) {
      motherController.abort();
      motherController = null;
    }
    motherInFlightRequestKey = '';
    motherFetchMeta = null;
  });

  return {
    motherWorkflowItems,
    workflowItemsByTask,
    workflowItemsSignature,
    workflowPreviewByTask,
    workflowPreviewSignature,
    isTaskWorkflowLoading,
    syncMissionWorkflowState
  };
};
