import { isDesktopModeEnabled, reportDesktopRendererStage } from '@/config/desktop';

type AsyncTask = () => Promise<unknown> | unknown;

type MessengerRealtimePulseOptions = {
  refreshRunningAgents: AsyncTask;
  refreshCronAgentIds: AsyncTask;
  refreshChannelBoundAgentIds: AsyncTask;
  refreshChatSessions?: AsyncTask;
  refreshContacts?: AsyncTask;
  runSequentially?: boolean;
  shouldRefreshCron?: () => boolean;
  shouldRefreshChannelBoundAgentIds?: () => boolean;
  shouldRefreshChatSessions?: () => boolean;
  shouldRefreshContacts?: () => boolean;
  shouldDefer?: () => boolean;
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
const DESKTOP_META_REFRESH_MIN_MS = 30000;

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
  let lastDesktopMetaRefreshAt = 0;

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

  const shouldRefreshDesktopMeta = (): boolean => {
    if (!isDesktopModeEnabled()) {
      return true;
    }
    const now = Date.now();
    if (lastDesktopMetaRefreshAt > 0 && now - lastDesktopMetaRefreshAt < DESKTOP_META_REFRESH_MIN_MS) {
      return false;
    }
    lastDesktopMetaRefreshAt = now;
    return true;
  };

  const runTasksSequentially = async (
    tasks: Array<() => Promise<unknown>>
  ): Promise<PromiseSettledResult<unknown>[]> => {
    const results: PromiseSettledResult<unknown>[] = [];
    for (const task of tasks) {
      try {
        results.push({ status: 'fulfilled', value: await task() });
      } catch (error) {
        results.push({ status: 'rejected', reason: error });
      }
    }
    return results;
  };

  const runTrackedTask = async (name: string, task: AsyncTask): Promise<unknown> => {
    if (isDesktopModeEnabled()) {
      reportDesktopRendererStage('messenger-realtime-pulse-task-start', { task: name });
    }
    try {
      const result = await scheduleTask(task);
      if (isDesktopModeEnabled()) {
        reportDesktopRendererStage('messenger-realtime-pulse-task-finish', { task: name });
      }
      return result;
    } catch (error) {
      if (isDesktopModeEnabled()) {
        reportDesktopRendererStage('messenger-realtime-pulse-task-error', { task: name });
      }
      throw error;
    }
  };

  const runTick = async () => {
    if (!started) return;
    if (running) {
      scheduleNext(TRIGGER_DELAY_MS);
      return;
    }
    // Keep one in-flight tick at a time to avoid overlapping requests and stale writes.
    running = true;
    if (options.shouldDefer?.() === true) {
      running = false;
      pendingTrigger = false;
      scheduleNext(resolveDelay());
      return;
    }
    const desktopMode = isDesktopModeEnabled();
    const shouldRefreshMeta = shouldRefreshDesktopMeta();
    const tasks: Array<() => Promise<unknown>> = [
      () => runTrackedTask('running-agents', options.refreshRunningAgents)
    ];
    if (shouldRefreshMeta && options.shouldRefreshChannelBoundAgentIds?.() !== false) {
      tasks.push(() => runTrackedTask('channel-bound-agent-ids', options.refreshChannelBoundAgentIds));
    }
    if (shouldRefreshMeta && options.shouldRefreshCron?.() !== false) {
      tasks.push(() => runTrackedTask('cron-agent-ids', options.refreshCronAgentIds));
    }
    if (options.refreshChatSessions && options.shouldRefreshChatSessions?.() !== false) {
      tasks.push(() => runTrackedTask('chat-sessions', options.refreshChatSessions));
    }
    if (!desktopMode && options.refreshContacts && options.shouldRefreshContacts?.() !== false) {
      tasks.push(() => runTrackedTask('contacts', options.refreshContacts));
    }
    try {
      const results = options.runSequentially === true
        ? await runTasksSequentially(tasks)
        : await Promise.allSettled(tasks.map((task) => task()));
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
    // Run the first pulse immediately so visible chat state catches up after navigation/resume.
    scheduleNext(0);
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
