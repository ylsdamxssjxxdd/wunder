import { ElMessage } from 'element-plus';
import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { cancelBeeroomDemoRun, getBeeroomDemoRun, startBeeroomDemoRun } from '@/api/beeroom';

type DemoStatus =
  | 'idle'
  | 'starting'
  | 'running'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled';

type Translate = (key: string, params?: Record<string, unknown>) => string;

const POLL_INTERVAL_MS = 1200;
const TERMINAL_STATUSES = new Set<DemoStatus>(['completed', 'failed', 'cancelled']);

const asTrimmed = (value: unknown) => String(value || '').trim();

const normalizeStatus = (value: unknown): DemoStatus => {
  const status = asTrimmed(value).toLowerCase();
  if (
    status === 'starting' ||
    status === 'running' ||
    status === 'cancelling' ||
    status === 'completed' ||
    status === 'failed' ||
    status === 'cancelled'
  ) {
    return status;
  }
  return 'idle';
};

const normalizeErrorMessage = (error: any): string => {
  const detail = error?.response?.data?.detail;
  if (detail && typeof detail === 'object') {
    const message = asTrimmed(detail.message || detail.error || detail.detail);
    if (message) {
      return message;
    }
  }
  const message = asTrimmed(detail || error?.response?.data?.message || error?.message);
  return message || '';
};

export const useBeeroomDemo = (options: {
  activeGroupId: Ref<string>;
  selectedMotherAgentId: Ref<string>;
  t: Translate;
  onRefresh?: () => void;
}) => {
  const demoRunId = ref('');
  const demoTeamRunId = ref('');
  const demoStatus = ref<DemoStatus>('idle');
  const demoBusy = ref(false);
  const demoError = ref('');

  let pollTimer: number | null = null;
  let pollInFlight = false;
  let lastNotifiedRunId = '';

  const clearPollTimer = () => {
    if (pollTimer !== null) {
      window.clearTimeout(pollTimer);
      pollTimer = null;
    }
  };

  const isRunning = computed(() =>
    ['starting', 'running', 'cancelling'].includes(demoStatus.value)
  );
  const canStart = computed(
    () => !demoBusy.value && !isRunning.value && Boolean(asTrimmed(options.activeGroupId.value))
  );
  const canCancel = computed(
    () => !demoBusy.value && isRunning.value && Boolean(asTrimmed(demoRunId.value))
  );
  const actionLabel = computed(() => {
    if (demoBusy.value && demoStatus.value === 'cancelling') {
      return options.t('beeroom.canvas.demoCancelling');
    }
    if (canCancel.value) {
      return options.t('beeroom.canvas.demoStop');
    }
    if (demoBusy.value && demoStatus.value === 'starting') {
      return options.t('beeroom.canvas.demoStarting');
    }
    if (demoStatus.value === 'running') {
      return options.t('beeroom.canvas.demoRunning');
    }
    return options.t('beeroom.canvas.demoStart');
  });

  const applySnapshot = (payload: any) => {
    const runId = asTrimmed(payload?.run_id || payload?.runId);
    if (runId) {
      demoRunId.value = runId;
    }
    demoStatus.value = normalizeStatus(payload?.status);
    demoTeamRunId.value = asTrimmed(payload?.team_run_id || payload?.teamRunId);
    const nextError = asTrimmed(payload?.error);
    demoError.value = nextError;
  };

  const stopPolling = () => {
    clearPollTimer();
    pollInFlight = false;
  };

  const schedulePoll = (delayMs = POLL_INTERVAL_MS) => {
    if (pollTimer !== null) return;
    pollTimer = window.setTimeout(() => {
      pollTimer = null;
      void pollStatus();
    }, Math.max(300, delayMs));
  };

  const notifyTerminal = (status: DemoStatus, runId: string, errorText: string) => {
    if (!runId || runId === lastNotifiedRunId) return;
    lastNotifiedRunId = runId;
    if (status === 'completed') {
      ElMessage.success(options.t('beeroom.canvas.demoCompleted'));
      return;
    }
    if (status === 'cancelled') {
      ElMessage.warning(options.t('beeroom.canvas.demoCancelled'));
      return;
    }
    if (status === 'failed') {
      const detail = errorText || options.t('beeroom.canvas.demoFailed');
      ElMessage.error(detail);
    }
  };

  const pollStatus = async () => {
    const groupId = asTrimmed(options.activeGroupId.value);
    const runId = asTrimmed(demoRunId.value);
    if (!groupId || !runId) {
      return;
    }
    if (pollInFlight) {
      schedulePoll();
      return;
    }
    pollInFlight = true;
    try {
      const { data } = await getBeeroomDemoRun(groupId, runId);
      const snapshot = data?.data || {};
      if (asTrimmed(options.activeGroupId.value) !== groupId) {
        return;
      }
      applySnapshot(snapshot);
      const status = demoStatus.value;
      if (TERMINAL_STATUSES.has(status)) {
        stopPolling();
        demoBusy.value = false;
        notifyTerminal(status, runId, demoError.value);
        options.onRefresh?.();
        return;
      }
      options.onRefresh?.();
      schedulePoll();
    } catch {
      schedulePoll();
    } finally {
      pollInFlight = false;
    }
  };

  const startDemo = async () => {
    const groupId = asTrimmed(options.activeGroupId.value);
    const selectedMotherAgentId = asTrimmed(options.selectedMotherAgentId.value);
    if (!groupId || demoBusy.value) return;
    demoBusy.value = true;
    demoError.value = '';
    demoStatus.value = 'starting';
    stopPolling();
    try {
      const { data } = await startBeeroomDemoRun(groupId, {
        speed: 'normal',
        worker_count_mode: 'random',
        tool_profile: 'safe',
        mother_agent_id: selectedMotherAgentId || undefined
      });
      const snapshot = data?.data || {};
      const runId = asTrimmed(snapshot?.run_id || snapshot?.runId);
      if (!runId) {
        throw new Error(options.t('beeroom.canvas.demoFailed'));
      }
      demoRunId.value = runId;
      applySnapshot(snapshot);
      options.onRefresh?.();
      ElMessage.success(options.t('beeroom.canvas.demoStarted'));
      schedulePoll(300);
    } catch (error: any) {
      const statusCode = Number(error?.response?.status || 0);
      if (statusCode === 409) {
        const message = options.t('beeroom.canvas.demoAlreadyRunning');
        demoError.value = message;
        ElMessage.warning(message);
      } else {
        const detail = normalizeErrorMessage(error) || options.t('beeroom.canvas.demoFailed');
        demoError.value = detail;
        ElMessage.error(detail);
      }
      demoStatus.value = 'idle';
    } finally {
      demoBusy.value = false;
    }
  };

  const cancelDemo = async () => {
    const groupId = asTrimmed(options.activeGroupId.value);
    const runId = asTrimmed(demoRunId.value);
    if (!groupId || !runId || demoBusy.value) return;
    demoBusy.value = true;
    demoStatus.value = 'cancelling';
    demoError.value = '';
    try {
      const { data } = await cancelBeeroomDemoRun(groupId, runId);
      applySnapshot(data?.data || {});
      schedulePoll(250);
    } catch (error: any) {
      demoError.value = normalizeErrorMessage(error) || options.t('beeroom.canvas.demoCancelFailed');
      ElMessage.error(demoError.value);
      schedulePoll();
    } finally {
      demoBusy.value = false;
    }
  };

  const handleDemoAction = async () => {
    if (canCancel.value) {
      await cancelDemo();
      return;
    }
    if (canStart.value) {
      await startDemo();
    }
  };

  const handleRealtimeEvent = (eventType: unknown, payload: any) => {
    const normalizedType = asTrimmed(eventType).toLowerCase();
    if (normalizedType !== 'beeroom_demo_status') {
      return false;
    }
    const activeGroupId = asTrimmed(options.activeGroupId.value);
    const eventGroupId = asTrimmed(payload?.group_id || payload?.groupId);
    if (eventGroupId && activeGroupId && eventGroupId !== activeGroupId) {
      return false;
    }
    const incomingRunId = asTrimmed(payload?.run_id || payload?.runId);
    const currentRunId = asTrimmed(demoRunId.value);
    if (currentRunId && incomingRunId && incomingRunId !== currentRunId) {
      return false;
    }
    applySnapshot(payload);
    const status = demoStatus.value;
    if (TERMINAL_STATUSES.has(status)) {
      stopPolling();
      demoBusy.value = false;
      notifyTerminal(status, asTrimmed(demoRunId.value), demoError.value);
      options.onRefresh?.();
      return true;
    }
    demoBusy.value = false;
    options.onRefresh?.();
    schedulePoll(600);
    return true;
  };

  const reset = () => {
    stopPolling();
    demoRunId.value = '';
    demoTeamRunId.value = '';
    demoStatus.value = 'idle';
    demoBusy.value = false;
    demoError.value = '';
  };

  watch(
    () => asTrimmed(options.activeGroupId.value),
    (next, prev) => {
      if (next === prev) return;
      reset();
      lastNotifiedRunId = '';
    }
  );

  onBeforeUnmount(() => {
    stopPolling();
  });

  return {
    demoRunId,
    demoTeamRunId,
    demoStatus,
    demoBusy,
    demoError,
    demoActionLabel: actionLabel,
    demoCanStart: canStart,
    demoCanCancel: canCancel,
    handleDemoAction,
    handleDemoRealtimeEvent: handleRealtimeEvent,
    resetDemoState: reset
  };
};
