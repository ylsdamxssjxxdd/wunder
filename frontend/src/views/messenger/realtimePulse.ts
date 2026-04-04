type AsyncTask = () => Promise<unknown> | unknown;

type MessengerRealtimePulseOptions = {
  refreshRunningAgents: AsyncTask;
  refreshCronAgentIds: AsyncTask;
  refreshChannelBoundAgentIds: AsyncTask;
  refreshChatSessions?: AsyncTask;
  refreshContacts?: AsyncTask;
  shouldRefreshCron?: () => boolean;
  shouldRefreshChannelBoundAgentIds?: () => boolean;
  shouldRefreshChatSessions?: () => boolean;
  shouldRefreshContacts?: () => boolean;
  isHotState?: () => boolean;
  onError?: (error: unknown) => void;
};

type MessengerRealtimePulseController = {
  start: () => void;
  stop: () => void;
  trigger: (reason?: string) => void;
};

const HOT_REFRESH_MS = 2500;
const IDLE_REFRESH_MS = 8000;
const HIDDEN_REFRESH_MS = 30000;
const ERROR_RETRY_MS = 5000;
const TRIGGER_DELAY_MS = 120;

const isPageVisible = (): boolean => {
  if (typeof document === 'undefined') return true;
  return document.visibilityState !== 'hidden';
};

const scheduleTask = (task: AsyncTask): Promise<unknown> => Promise.resolve().then(task);

export const createMessengerRealtimePulse = (
  options: MessengerRealtimePulseOptions
): MessengerRealtimePulseController => {
  let timerId: number | null = null;
  let running = false;
  let started = false;
  let pendingTrigger = false;

  const clearTimer = () => {
    if (typeof window === 'undefined') return;
    if (timerId !== null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  };

  const resolveDelay = () => {
    // Use shorter cadence while runtime is active; fall back to lower-frequency sync when idle/hidden.
    if (!isPageVisible()) return HIDDEN_REFRESH_MS;
    if (options.isHotState?.()) return HOT_REFRESH_MS;
    return IDLE_REFRESH_MS;
  };

  const scheduleNext = (delayMs: number) => {
    if (typeof window === 'undefined' || !started) return;
    clearTimer();
    timerId = window.setTimeout(() => {
      timerId = null;
      void runTick();
    }, Math.max(0, Math.floor(delayMs)));
  };

  const handleFailures = (results: PromiseSettledResult<unknown>[]): boolean => {
    const failures = results
      .filter((item): item is PromiseRejectedResult => item.status === 'rejected')
      .map((item) => item.reason);
    if (!failures.length) return false;
    failures.forEach((error) => options.onError?.(error));
    return true;
  };

  const runTick = async () => {
    if (!started) return;
    if (running) {
      scheduleNext(TRIGGER_DELAY_MS);
      return;
    }
    // Keep one in-flight tick at a time to avoid overlapping requests and stale writes.
    running = true;
    const tasks: Promise<unknown>[] = [scheduleTask(options.refreshRunningAgents)];
    if (options.shouldRefreshChannelBoundAgentIds?.() !== false) {
      tasks.push(scheduleTask(options.refreshChannelBoundAgentIds));
    }
    if (options.shouldRefreshCron?.() !== false) {
      tasks.push(scheduleTask(options.refreshCronAgentIds));
    }
    if (options.refreshChatSessions && options.shouldRefreshChatSessions?.() !== false) {
      tasks.push(scheduleTask(options.refreshChatSessions));
    }
    if (options.refreshContacts && options.shouldRefreshContacts?.() !== false) {
      tasks.push(scheduleTask(options.refreshContacts));
    }
    try {
      const results = await Promise.allSettled(tasks);
      if (handleFailures(results)) {
        scheduleNext(ERROR_RETRY_MS);
        return;
      }
      if (pendingTrigger) {
        pendingTrigger = false;
        scheduleNext(TRIGGER_DELAY_MS);
        return;
      }
      scheduleNext(resolveDelay());
    } catch (error) {
      options.onError?.(error);
      scheduleNext(ERROR_RETRY_MS);
    } finally {
      running = false;
    }
  };

  const trigger = (_reason = '') => {
    if (!started) return;
    if (running) {
      pendingTrigger = true;
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
    scheduleNext(resolveDelay());
  };

  const stop = () => {
    if (!started) return;
    started = false;
    pendingTrigger = false;
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
