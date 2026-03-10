type AsyncTask = () => Promise<unknown> | unknown;

type MessengerBeeroomRealtimeSyncOptions = {
  refreshBeeroomGroups: AsyncTask;
  refreshBeeroomActiveGroup: AsyncTask;
  refreshRunningAgents?: AsyncTask;
  shouldSync?: () => boolean;
  isHotState?: () => boolean;
  onError?: (error: unknown) => void;
};

type MessengerBeeroomRealtimeSyncController = {
  start: () => void;
  stop: () => void;
  trigger: (reason?: string) => void;
};

const HOT_SYNC_MS = 1600;
const IDLE_SYNC_MS = 4200;
const HIDDEN_SYNC_MS = 12000;
const ERROR_RETRY_MS = 2800;
const TRIGGER_DELAY_MS = 120;

const isPageVisible = (): boolean => {
  if (typeof document === 'undefined') return true;
  return document.visibilityState !== 'hidden';
};

const scheduleTask = (task: AsyncTask): Promise<unknown> => Promise.resolve().then(task);

export const createBeeroomRealtimeSync = (
  options: MessengerBeeroomRealtimeSyncOptions
): MessengerBeeroomRealtimeSyncController => {
  let timerId: number | null = null;
  let started = false;
  let running = false;

  const clearTimer = () => {
    if (typeof window === 'undefined') return;
    if (timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  };

  const resolveDelay = () => {
    if (!isPageVisible()) return HIDDEN_SYNC_MS;
    if (options.isHotState?.()) return HOT_SYNC_MS;
    return IDLE_SYNC_MS;
  };

  const scheduleNext = (delayMs: number) => {
    if (typeof window === 'undefined' || !started) return;
    clearTimer();
    timerId = window.setTimeout(() => {
      timerId = null;
      void runTick();
    }, Math.max(0, Math.floor(delayMs)));
  };

  const runStep = async (task?: AsyncTask) => {
    if (!task) return true;
    try {
      await scheduleTask(task);
      return true;
    } catch (error) {
      options.onError?.(error);
      return false;
    }
  };

  const runTick = async () => {
    if (!started) return;
    if (running) {
      scheduleNext(TRIGGER_DELAY_MS);
      return;
    }
    if (options.shouldSync?.() === false) {
      scheduleNext(IDLE_SYNC_MS);
      return;
    }
    running = true;
    try {
      const groupsOk = await runStep(options.refreshBeeroomGroups);
      const activeOk = await runStep(options.refreshBeeroomActiveGroup);
      const runningAgentsOk = await runStep(options.refreshRunningAgents);
      if (groupsOk && activeOk && runningAgentsOk) {
        scheduleNext(resolveDelay());
      } else {
        scheduleNext(ERROR_RETRY_MS);
      }
    } finally {
      running = false;
    }
  };

  const trigger = (_reason = '') => {
    if (!started) return;
    if (running) {
      scheduleNext(TRIGGER_DELAY_MS);
      return;
    }
    scheduleNext(0);
  };

  const handleVisibility = () => {
    if (!started) return;
    trigger('visibility');
  };

  const start = () => {
    if (started) return;
    started = true;
    if (typeof window !== 'undefined') {
      window.addEventListener('focus', handleVisibility);
      window.addEventListener('online', handleVisibility);
      document.addEventListener('visibilitychange', handleVisibility);
    }
    trigger('start');
  };

  const stop = () => {
    if (!started) return;
    started = false;
    clearTimer();
    if (typeof window !== 'undefined') {
      window.removeEventListener('focus', handleVisibility);
      window.removeEventListener('online', handleVisibility);
      document.removeEventListener('visibilitychange', handleVisibility);
    }
  };

  return {
    start,
    stop,
    trigger
  };
};
