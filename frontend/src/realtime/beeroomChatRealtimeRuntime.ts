import { openBeeroomSocket } from '@/api/beeroom';
import { resolveNextRealtimeCursor } from '@/components/beeroom/beeroomRealtimeCursor';
import { beeroomRealtimePerf } from '@/utils/beeroomRealtimePerf';
import { createWsMultiplexer } from '@/utils/ws';

export type BeeroomChatRealtimeTransport = 'none' | 'ws';

export type BeeroomChatRealtimeEvent = {
  groupId: string;
  eventType: string;
  dataText: string;
  eventId: string;
  transport: Exclude<BeeroomChatRealtimeTransport, 'none'>;
  payload: unknown;
};

type ActivateGroupOptions = {
  immediatePoll?: boolean;
};

type BeeroomChatRealtimeRuntimeOptions = {
  getActiveGroupId: () => string;
  getCursor: () => number;
  setCursor: (cursor: number) => void;
  onPoll: (groupId: string) => Promise<void> | void;
  onEvent: (event: BeeroomChatRealtimeEvent) => void;
  onTransportChange?: (transport: BeeroomChatRealtimeTransport, reason: string) => void;
  onError?: (error: unknown) => void;
  isDisposed?: () => boolean;
  isAuthDenied?: () => boolean;
  healthPollIntervalMs?: number;
  wsRetryDelayMs?: number;
  triggerDelayMs?: number;
};

type BeeroomChatRealtimeRuntime = {
  activateGroup: (groupId: string, options?: ActivateGroupOptions) => void;
  stop: () => void;
  triggerHealthPoll: (reason?: string) => void;
  syncHealthPolling: () => void;
  stopHealthPolling: () => void;
};

const DEFAULT_HEALTH_POLL_INTERVAL_MS = 30_000;
const DEFAULT_WS_RETRY_DELAY_MS = 1_400;
const DEFAULT_TRIGGER_DELAY_MS = 120;

const normalizeGroupId = (value: unknown): string => String(value || '').trim();
const normalizeEventType = (value: unknown): string =>
  String(value || '')
    .trim()
    .toLowerCase();

const safeJsonParse = (value: unknown): unknown => {
  if (typeof value !== 'string') return value;
  const text = value.trim();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
};

const isVisiblePage = (): boolean => {
  if (typeof document === 'undefined') return true;
  return document.visibilityState !== 'hidden';
};

export const createBeeroomChatRealtimeRuntime = (
  options: BeeroomChatRealtimeRuntimeOptions
): BeeroomChatRealtimeRuntime => {
  const wsClient = createWsMultiplexer(() => openBeeroomSocket({ allowQueryToken: true }), {
    idleTimeoutMs: 20_000,
    connectTimeoutMs: 10_000,
    pingIntervalMs: 20_000
  });
  const healthPollIntervalMs = Math.max(
    5_000,
    Number(options.healthPollIntervalMs || DEFAULT_HEALTH_POLL_INTERVAL_MS)
  );
  const wsRetryDelayMs = Math.max(200, Number(options.wsRetryDelayMs || DEFAULT_WS_RETRY_DELAY_MS));
  const triggerDelayMs = Math.max(60, Number(options.triggerDelayMs || DEFAULT_TRIGGER_DELAY_MS));

  let activeGroupId = '';
  let transport: BeeroomChatRealtimeTransport = 'none';
  let healthPollTimer: number | null = null;
  let healthPollRunning = false;
  let wsRetryTimer: number | null = null;
  let wsWatchController: AbortController | null = null;
  let wsWatchRequestId = '';
  let boundVisibilityListener = false;

  const isDisposed = () => Boolean(options.isDisposed?.());
  const isAuthDenied = () => Boolean(options.isAuthDenied?.());

  const setTransport = (next: BeeroomChatRealtimeTransport, reason: string) => {
    if (transport === next) return;
    const prev = transport;
    transport = next;
    beeroomRealtimePerf.count('beeroom_realtime_transport_switch', 1, {
      from: prev,
      to: next,
      reason
    });
    options.onTransportChange?.(next, reason);
    syncHealthPolling();
  };

  const clearHealthPollTimer = () => {
    if (typeof window === 'undefined') return;
    if (healthPollTimer !== null) {
      window.clearTimeout(healthPollTimer);
      healthPollTimer = null;
    }
  };

  const clearWsRetryTimer = () => {
    if (typeof window === 'undefined') return;
    if (wsRetryTimer !== null) {
      window.clearTimeout(wsRetryTimer);
      wsRetryTimer = null;
    }
  };

  const stopWsWatch = () => {
    clearWsRetryTimer();
    if (wsWatchController) {
      wsWatchController.abort();
      wsWatchController = null;
    }
    if (wsWatchRequestId) {
      wsClient.sendCancel(wsWatchRequestId, activeGroupId);
      wsWatchRequestId = '';
    }
  };

  const stopWatchTransport = () => {
    stopWsWatch();
    setTransport('none', 'stop');
  };

  const triggerHealthPoll = (_reason = '') => {
    if (typeof window === 'undefined') return;
    if (isDisposed()) return;
    if (!activeGroupId || isAuthDenied()) return;
    if (transport !== 'none') return;
    clearHealthPollTimer();
    healthPollTimer = window.setTimeout(() => {
      healthPollTimer = null;
      void runHealthPoll();
    }, 0);
  };

  const scheduleHealthPoll = (delayMs: number) => {
    if (typeof window === 'undefined') return;
    if (isDisposed()) return;
    if (!activeGroupId || isAuthDenied() || transport !== 'none') {
      clearHealthPollTimer();
      return;
    }
    clearHealthPollTimer();
    healthPollTimer = window.setTimeout(() => {
      healthPollTimer = null;
      void runHealthPoll();
    }, Math.max(0, Math.floor(delayMs)));
  };

  const runHealthPoll = async () => {
    if (isDisposed()) return;
    const currentGroupId = normalizeGroupId(options.getActiveGroupId());
    if (!currentGroupId || currentGroupId !== activeGroupId || isAuthDenied()) {
      return;
    }
    if (transport !== 'none') {
      return;
    }
    if (healthPollRunning) {
      scheduleHealthPoll(triggerDelayMs);
      return;
    }
    healthPollRunning = true;
    try {
      beeroomRealtimePerf.count('beeroom_realtime_health_poll', 1, {
        groupId: currentGroupId
      });
      await Promise.resolve(options.onPoll(currentGroupId));
    } catch (error) {
      options.onError?.(error);
      beeroomRealtimePerf.count('beeroom_realtime_health_poll_error', 1, {
        groupId: currentGroupId
      });
    } finally {
      healthPollRunning = false;
      scheduleHealthPoll(healthPollIntervalMs);
    }
  };

  const syncHealthPolling = () => {
    if (!activeGroupId || isAuthDenied() || transport !== 'none') {
      clearHealthPollTimer();
      return;
    }
    if (healthPollTimer === null && !healthPollRunning) {
      scheduleHealthPoll(healthPollIntervalMs);
    }
  };

  const stopHealthPolling = () => {
    clearHealthPollTimer();
    healthPollRunning = false;
  };

  const scheduleWsRetry = (groupId: string) => {
    if (typeof window === 'undefined' || isDisposed() || isAuthDenied()) return;
    clearWsRetryTimer();
    beeroomRealtimePerf.count('beeroom_realtime_ws_retry_scheduled', 1, { groupId });
    wsRetryTimer = window.setTimeout(() => {
      wsRetryTimer = null;
      if (groupId !== activeGroupId) return;
      startWsWatch(groupId);
    }, wsRetryDelayMs);
  };

  const handleRealtimeEvent = (
    groupId: string,
    eventType: string,
    dataText: string,
    eventId: string,
    sourceTransport: Exclude<BeeroomChatRealtimeTransport, 'none'>
  ) => {
    if (isDisposed()) return;
    if (!groupId || groupId !== activeGroupId) return;
    const payload = safeJsonParse(dataText);
    const nextCursor = resolveNextRealtimeCursor({
      currentCursor: options.getCursor(),
      eventId,
      payload
    });
    if (nextCursor > 0) {
      options.setCursor(nextCursor);
    }
    const normalizedType = normalizeEventType(eventType);
    beeroomRealtimePerf.count('beeroom_realtime_event', 1, {
      eventType: normalizedType || eventType,
      transport: sourceTransport
    });
    if (normalizedType === 'watching') {
      setTransport('ws', 'watching');
    } else if (normalizedType && normalizedType !== 'heartbeat' && normalizedType !== 'ping') {
      setTransport(sourceTransport, `event:${normalizedType}`);
    }
    if (normalizedType === 'sync_required') {
      beeroomRealtimePerf.count('beeroom_realtime_sync_required', 1, {
        transport: sourceTransport
      });
    }
    options.onEvent({
      groupId,
      eventType,
      dataText,
      eventId,
      transport: sourceTransport,
      payload
    });
  };

  const startWsWatch = (groupId: string) => {
    const normalizedGroupId = normalizeGroupId(groupId);
    if (!normalizedGroupId || isDisposed() || isAuthDenied()) return;
    stopWsWatch();
    const controller = new AbortController();
    wsWatchController = controller;
    const requestId = `beeroom_watch_${Math.random().toString(36).slice(2, 10)}`;
    wsWatchRequestId = requestId;
    wsClient
      .request({
        requestId,
        sessionId: normalizedGroupId,
        message: {
          type: 'watch',
          request_id: requestId,
          payload: {
            group_id: normalizedGroupId,
            after_event_id: options.getCursor()
          }
        },
        closeOnFinal: false,
        signal: controller.signal,
        onEvent: (eventType, dataText, eventId) =>
          handleRealtimeEvent(normalizedGroupId, eventType, dataText, eventId, 'ws')
      })
      .catch((error) => {
        if (controller.signal.aborted) return;
        options.onError?.(error);
        beeroomRealtimePerf.count('beeroom_realtime_ws_error', 1, {
          groupId: normalizedGroupId
        });
        setTransport('none', 'ws_error');
        scheduleWsRetry(normalizedGroupId);
      })
      .finally(() => {
        if (wsWatchController === controller) {
          wsWatchController = null;
        }
        if (wsWatchRequestId === requestId) {
          wsWatchRequestId = '';
        }
      });
  };

  const handleVisibilityRefresh = () => {
    if (!activeGroupId || isDisposed()) return;
    if (!isVisiblePage()) return;
    triggerHealthPoll('visibility');
  };

  const bindVisibilityListeners = () => {
    if (boundVisibilityListener || typeof window === 'undefined') return;
    boundVisibilityListener = true;
    window.addEventListener('focus', handleVisibilityRefresh);
    window.addEventListener('online', handleVisibilityRefresh);
    document.addEventListener('visibilitychange', handleVisibilityRefresh);
  };

  const unbindVisibilityListeners = () => {
    if (!boundVisibilityListener || typeof window === 'undefined') return;
    boundVisibilityListener = false;
    window.removeEventListener('focus', handleVisibilityRefresh);
    window.removeEventListener('online', handleVisibilityRefresh);
    document.removeEventListener('visibilitychange', handleVisibilityRefresh);
  };

  const activateGroup = (groupId: string, activateOptions: ActivateGroupOptions = {}) => {
    const normalizedGroupId = normalizeGroupId(groupId);
    const currentGroupId = normalizeGroupId(options.getActiveGroupId());
    if (normalizedGroupId !== currentGroupId) {
      activeGroupId = currentGroupId;
    } else {
      activeGroupId = normalizedGroupId;
    }
    if (!activeGroupId || isAuthDenied()) {
      stopWatchTransport();
      stopHealthPolling();
      return;
    }
    bindVisibilityListeners();
    stopWatchTransport();
    startWsWatch(activeGroupId);
    if (activateOptions.immediatePoll !== false) {
      triggerHealthPoll('activate');
    } else {
      syncHealthPolling();
    }
  };

  const stop = () => {
    activeGroupId = '';
    stopWatchTransport();
    stopHealthPolling();
    unbindVisibilityListeners();
  };

  return {
    activateGroup,
    stop,
    triggerHealthPoll,
    syncHealthPolling,
    stopHealthPolling
  };
};
