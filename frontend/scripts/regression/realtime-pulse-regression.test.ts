import test from 'node:test';
import assert from 'node:assert/strict';

import { createMessengerRealtimePulse } from '../../src/views/messenger/realtimePulse';

type VoidListener = () => void;

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

const waitFor = async (
  condition: () => boolean,
  options: { timeoutMs?: number; intervalMs?: number } = {}
) => {
  const timeoutMs = Math.max(1, Math.floor(options.timeoutMs ?? 800));
  const intervalMs = Math.max(1, Math.floor(options.intervalMs ?? 5));
  const startAt = Date.now();
  while (!condition()) {
    if (Date.now() - startAt >= timeoutMs) {
      throw new Error(`waitFor timeout after ${timeoutMs}ms`);
    }
    await sleep(intervalMs);
  }
};

const createListenerRegistry = () => {
  const listeners = new Map<string, Set<VoidListener>>();
  const add = (event: string, listener: VoidListener) => {
    const set = listeners.get(event) || new Set<VoidListener>();
    set.add(listener);
    listeners.set(event, set);
  };
  const remove = (event: string, listener: VoidListener) => {
    const set = listeners.get(event);
    if (!set) return;
    set.delete(listener);
    if (!set.size) {
      listeners.delete(event);
    }
  };
  const emit = (event: string) => {
    const set = listeners.get(event);
    if (!set?.size) return;
    Array.from(set).forEach((listener) => listener());
  };
  return { add, remove, emit };
};

const installBrowserMocks = (visibilityState: 'visible' | 'hidden' = 'visible') => {
  const previousWindow = (globalThis as { window?: unknown }).window;
  const previousDocument = (globalThis as { document?: unknown }).document;

  const windowListeners = createListenerRegistry();
  const documentListeners = createListenerRegistry();
  const timerMap = new Map<number, NodeJS.Timeout>();
  let timerSeq = 1;
  const clearAllTimers = () => {
    timerMap.forEach((handle) => clearTimeout(handle));
    timerMap.clear();
  };

  const windowMock = {
    setTimeout(handler: () => void, timeoutMs?: number) {
      const timerId = timerSeq++;
      const handle = setTimeout(() => {
        timerMap.delete(timerId);
        handler();
      }, Math.max(0, Number(timeoutMs) || 0));
      timerMap.set(timerId, handle);
      return timerId;
    },
    clearTimeout(timerId: number) {
      const handle = timerMap.get(timerId);
      if (!handle) return;
      clearTimeout(handle);
      timerMap.delete(timerId);
    },
    addEventListener(event: string, listener: VoidListener) {
      windowListeners.add(event, listener);
    },
    removeEventListener(event: string, listener: VoidListener) {
      windowListeners.remove(event, listener);
    },
    dispatch(event: string) {
      windowListeners.emit(event);
    }
  };

  const documentMock = {
    visibilityState,
    addEventListener(event: string, listener: VoidListener) {
      documentListeners.add(event, listener);
    },
    removeEventListener(event: string, listener: VoidListener) {
      documentListeners.remove(event, listener);
    },
    dispatch(event: string) {
      documentListeners.emit(event);
    }
  };

  (globalThis as { window?: unknown }).window = windowMock;
  (globalThis as { document?: unknown }).document = documentMock;

  return {
    windowMock,
    documentMock,
    cleanup: () => {
      clearAllTimers();
      if (previousWindow === undefined) {
        delete (globalThis as { window?: unknown }).window;
      } else {
        (globalThis as { window?: unknown }).window = previousWindow;
      }
      if (previousDocument === undefined) {
        delete (globalThis as { document?: unknown }).document;
      } else {
        (globalThis as { document?: unknown }).document = previousDocument;
      }
    }
  };
};

test('realtime pulse refreshes chat sessions only when condition is met', async (context) => {
  const browser = installBrowserMocks('visible');
  context.after(() => browser.cleanup());

  let runningCount = 0;
  let cronCount = 0;
  let channelCount = 0;
  let chatCount = 0;
  let allowChatRefresh = false;

  const pulse = createMessengerRealtimePulse({
    refreshRunningAgents: () => {
      runningCount += 1;
    },
    refreshCronAgentIds: () => {
      cronCount += 1;
    },
    refreshChannelBoundAgentIds: () => {
      channelCount += 1;
    },
    refreshChatSessions: () => {
      chatCount += 1;
    },
    shouldRefreshChatSessions: () => allowChatRefresh
  });

  pulse.start();
  await waitFor(() => runningCount > 0 && cronCount > 0 && channelCount > 0);
  assert.equal(chatCount, 0);

  allowChatRefresh = true;
  pulse.trigger('enable-chat-refresh');
  await waitFor(() => chatCount > 0);

  pulse.stop();
});

test('realtime pulse trigger while running schedules follow-up tick without overlap', async (context) => {
  const browser = installBrowserMocks('visible');
  context.after(() => browser.cleanup());

  let runningCount = 0;
  let concurrentRunning = 0;
  let maxConcurrentRunning = 0;
  let releaseFirstTick: (() => void) | null = null;
  const firstTickGate = new Promise<void>((resolve) => {
    releaseFirstTick = resolve;
  });

  const pulse = createMessengerRealtimePulse({
    refreshRunningAgents: async () => {
      runningCount += 1;
      concurrentRunning += 1;
      maxConcurrentRunning = Math.max(maxConcurrentRunning, concurrentRunning);
      try {
        if (runningCount === 1) {
          await firstTickGate;
        }
      } finally {
        concurrentRunning -= 1;
      }
    },
    refreshCronAgentIds: () => undefined,
    refreshChannelBoundAgentIds: () => undefined
  });

  pulse.start();
  await waitFor(() => runningCount === 1);

  pulse.trigger('running-1');
  pulse.trigger('running-2');
  await sleep(30);
  assert.equal(runningCount, 1);

  releaseFirstTick?.();
  await waitFor(() => runningCount >= 2);
  assert.equal(maxConcurrentRunning, 1);

  pulse.stop();
});
