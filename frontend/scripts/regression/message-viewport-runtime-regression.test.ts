import test from 'node:test';
import assert from 'node:assert/strict';

import { ref } from 'vue';

import {
  createMessageViewportRuntime,
  MESSAGE_VIRTUAL_HEIGHT_CACHE_LIMIT
} from '../../src/views/messenger/messageViewportRuntime';

type ResizeObserverCallbackLike = (
  entries: Array<{ target: unknown }>,
  observer: FakeResizeObserver
) => void;

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];

  private readonly callback: ResizeObserverCallbackLike;

  observed = new Set<unknown>();

  constructor(callback: ResizeObserverCallbackLike) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }

  observe(target: unknown) {
    this.observed.add(target);
  }

  unobserve(target: unknown) {
    this.observed.delete(target);
  }

  disconnect() {
    this.observed.clear();
  }

  emit(targets: unknown[]) {
    this.callback(
      targets.map((target) => ({ target })),
      this
    );
  }
}

const createFakeMessageNode = (key: string, height: number) =>
  ({
    dataset: { virtualKey: key },
    offsetHeight: height,
    getBoundingClientRect: () => ({ height })
  }) as unknown as HTMLElement;

test('message viewport runtime remeasures visible message rows on resize observer updates', async () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const originalResizeObserver = (globalThis as Record<string, unknown>).ResizeObserver;
  const frameQueue: FrameRequestCallback[] = [];
  const flushFrames = () => {
    while (frameQueue.length > 0) {
      const frame = frameQueue.shift();
      frame?.(performance.now());
    }
  };

  try {
    FakeResizeObserver.instances = [];
    const messageNode = createFakeMessageNode('assistant-1', 164);
    const container = {
      scrollTop: 0,
      clientHeight: 720,
      scrollHeight: 720,
      querySelectorAll: () => [messageNode],
      getBoundingClientRect: () => ({ top: 0, height: 720 })
    } as unknown as HTMLElement;

    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {}
    };
    (globalThis as Record<string, unknown>).ResizeObserver = FakeResizeObserver;

    const messageVirtualHeightCache = new Map<string, number>();
    const messageVirtualLayoutVersion = ref(0);
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(true),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:assistant-1'),
      shouldVirtualizeMessages: ref(true),
      agentRenderableMessages: ref([{ key: 'assistant-1', message: { role: 'assistant' } }]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache,
      messageVirtualLayoutVersion,
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: (key: string) => messageVirtualHeightCache.get(key) || 0
    });

    runtime.scheduleMessageViewportRefresh({
      measure: true,
      measureKeys: ['assistant-1']
    });
    flushFrames();

    assert.equal(messageVirtualHeightCache.get('assistant-1'), 164);
    assert.equal(messageVirtualLayoutVersion.value, 1);
    assert.equal(FakeResizeObserver.instances.length, 1);
    assert.ok(FakeResizeObserver.instances[0]?.observed.has(messageNode));

    const grownHeight = 252;
    Object.assign(messageNode, {
      offsetHeight: grownHeight,
      getBoundingClientRect: () => ({ height: grownHeight })
    });
    FakeResizeObserver.instances[0]?.emit([messageNode]);

    assert.equal(messageVirtualHeightCache.get('assistant-1'), grownHeight);
    assert.equal(messageVirtualLayoutVersion.value, 2);

    runtime.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
    if (originalResizeObserver === undefined) {
      delete (globalThis as Record<string, unknown>).ResizeObserver;
    } else {
      (globalThis as Record<string, unknown>).ResizeObserver = originalResizeObserver;
    }
  }
});

test('message viewport runtime batches rapid resize observer updates', () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const originalResizeObserver = (globalThis as Record<string, unknown>).ResizeObserver;
  const frameQueue: FrameRequestCallback[] = [];
  const timerQueue: Array<() => void> = [];
  const flushFrames = () => {
    while (frameQueue.length > 0) {
      frameQueue.shift()?.(performance.now());
    }
  };

  try {
    FakeResizeObserver.instances = [];
    const messageNode = createFakeMessageNode('assistant-batch', 120);
    const container = {
      scrollTop: 0,
      clientHeight: 720,
      scrollHeight: 720,
      querySelectorAll: () => [messageNode],
      getBoundingClientRect: () => ({ height: 720 })
    } as unknown as HTMLElement;
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {},
      setTimeout: (callback: () => void) => {
        timerQueue.push(callback);
        return timerQueue.length;
      },
      clearTimeout: () => {}
    };
    (globalThis as Record<string, unknown>).ResizeObserver = FakeResizeObserver;

    const messageVirtualHeightCache = new Map<string, number>();
    const messageVirtualLayoutVersion = ref(0);
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(true),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:assistant-batch'),
      shouldVirtualizeMessages: ref(true),
      agentRenderableMessages: ref([{ key: 'assistant-batch', message: { role: 'assistant' } }]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache,
      messageVirtualLayoutVersion,
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: (key: string) => messageVirtualHeightCache.get(key) || 0
    });

    runtime.scheduleMessageViewportRefresh({ measure: true, measureKeys: ['assistant-batch'] });
    flushFrames();
    assert.equal(messageVirtualLayoutVersion.value, 1);

    Object.assign(messageNode, {
      offsetHeight: 148,
      getBoundingClientRect: () => ({ height: 148 })
    });
    FakeResizeObserver.instances[0]?.emit([messageNode]);
    Object.assign(messageNode, {
      offsetHeight: 196,
      getBoundingClientRect: () => ({ height: 196 })
    });
    FakeResizeObserver.instances[0]?.emit([messageNode]);

    assert.equal(timerQueue.length, 1);
    assert.equal(messageVirtualLayoutVersion.value, 1);
    assert.equal(messageVirtualHeightCache.get('assistant-batch'), 196);

    timerQueue.shift()?.();
    assert.equal(messageVirtualLayoutVersion.value, 2);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
    if (originalResizeObserver === undefined) {
      delete (globalThis as Record<string, unknown>).ResizeObserver;
    } else {
      (globalThis as Record<string, unknown>).ResizeObserver = originalResizeObserver;
    }
  }
});

test('message viewport runtime bounds retained row-height measurements', () => {
  const messageVirtualHeightCache = new Map<string, number>();
  const messages = Array.from({ length: MESSAGE_VIRTUAL_HEIGHT_CACHE_LIMIT + 24 }, (_, index) => ({
    key: `height-${index}`
  }));
  messages.forEach((item, index) => messageVirtualHeightCache.set(item.key, 100 + index));
  const runtime = createMessageViewportRuntime({
    messageListRef: ref(null),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey: ref('agent:height-cache'),
    shouldVirtualizeMessages: ref(true),
    agentRenderableMessages: ref(messages),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache,
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0
  });

  runtime.pruneMessageVirtualHeightCache();

  assert.equal(messageVirtualHeightCache.size, MESSAGE_VIRTUAL_HEIGHT_CACHE_LIMIT);
  assert.equal(messageVirtualHeightCache.has('height-0'), false);
  assert.equal(messageVirtualHeightCache.has(`height-${messages.length - 1}`), true);
  runtime.dispose();
});

test('message viewport runtime does not synchronously measure rows while scrolling', () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const originalResizeObserver = (globalThis as Record<string, unknown>).ResizeObserver;
  const frameQueue: FrameRequestCallback[] = [];
  try {
    const messageNode = createFakeMessageNode('assistant-scroll', 164);
    let measureCalls = 0;
    const container = {
      scrollTop: 0,
      clientHeight: 600,
      scrollHeight: 3200,
      querySelectorAll: () => {
        measureCalls += 1;
        return [messageNode];
      },
      getBoundingClientRect: () => ({ top: 0, height: 600 })
    } as unknown as HTMLElement;
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {},
      setTimeout: () => 1,
      clearTimeout: () => {}
    };
    (globalThis as Record<string, unknown>).ResizeObserver = FakeResizeObserver;
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(true),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:scroll'),
      shouldVirtualizeMessages: ref(true),
      agentRenderableMessages: ref([{ key: 'assistant-scroll' }]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache: new Map(),
      messageVirtualLayoutVersion: ref(0),
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: () => 118
    });

    for (let index = 1; index <= 12; index += 1) {
      container.scrollTop = index * 72;
      runtime.handleMessageListScroll();
      frameQueue.shift()?.(performance.now());
    }

    assert.equal(measureCalls, 0);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) delete (globalThis as Record<string, unknown>).window;
    else (globalThis as Record<string, unknown>).window = originalWindow;
    if (originalResizeObserver === undefined) delete (globalThis as Record<string, unknown>).ResizeObserver;
    else (globalThis as Record<string, unknown>).ResizeObserver = originalResizeObserver;
  }
});

test('message viewport runtime releases bottom follow mode before a deferred scroll frame', async () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const frameQueue: FrameRequestCallback[] = [];
  try {
    const container = {
      scrollTop: 420,
      clientHeight: 500,
      scrollHeight: 1400,
      querySelectorAll: () => [],
      getBoundingClientRect: () => ({ top: 0, height: 500 })
    } as unknown as HTMLElement;
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {}
    };
    const autoStickToBottom = ref(true);
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom,
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:manual-scroll'),
      shouldVirtualizeMessages: ref(false),
      agentRenderableMessages: ref([]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache: new Map<string, number>(),
      messageVirtualLayoutVersion: ref(0),
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: () => 0
    });

    runtime.handleMessageListScroll();

    assert.equal(autoStickToBottom.value, false);
    await runtime.scrollMessagesToBottom();
    assert.equal(container.scrollTop, 420);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) delete (globalThis as Record<string, unknown>).window;
    else (globalThis as Record<string, unknown>).window = originalWindow;
  }
});

test('message viewport runtime restores remembered conversation scroll after remount', async () => {
  const messageVirtualHeightCache = new Map<string, number>();
  const activeConversationKey = ref('agent:session-a');
  const firstContainer = {
    scrollTop: 420,
    clientHeight: 500,
    scrollHeight: 1800,
    querySelectorAll: () => [],
    getBoundingClientRect: () => ({ top: 0, height: 500 })
  } as unknown as HTMLElement;
  const firstRuntime = createMessageViewportRuntime({
    messageListRef: ref(firstContainer),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey,
    shouldVirtualizeMessages: ref(false),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache,
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0
  });

  firstRuntime.rememberCurrentScroll();
  firstRuntime.dispose();

  const secondContainer = {
    scrollTop: 0,
    clientHeight: 500,
    scrollHeight: 2200,
    querySelectorAll: () => [],
    getBoundingClientRect: () => ({ top: 0, height: 500 })
  } as unknown as HTMLElement;
  const secondRuntime = createMessageViewportRuntime({
    messageListRef: ref(secondContainer),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey,
    shouldVirtualizeMessages: ref(false),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache: new Map<string, number>(),
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0
  });

  const restored = await secondRuntime.restoreConversationScroll();

  assert.equal(restored, true);
  assert.equal(secondContainer.scrollTop, 820);
  secondRuntime.dispose();
});

test('message viewport runtime can defer row measurement while restoring scroll', async () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const idleQueue: Array<() => void> = [];
  const flushIdle = () => {
    while (idleQueue.length > 0) {
      idleQueue.shift()?.();
    }
  };

  try {
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        callback(performance.now());
        return 1;
      },
      cancelAnimationFrame: () => {},
      requestIdleCallback: (callback: () => void) => {
        idleQueue.push(callback);
        return idleQueue.length;
      },
      cancelIdleCallback: () => {}
    };

    const messageNode = createFakeMessageNode('assistant-deferred', 188);
    const container = {
      scrollTop: 260,
      clientHeight: 500,
      scrollHeight: 1800,
      querySelectorAll: () => [messageNode],
      getBoundingClientRect: () => ({ top: 0, height: 500 })
    } as unknown as HTMLElement;
    const messageVirtualHeightCache = new Map<string, number>();
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(false),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:session-deferred'),
      shouldVirtualizeMessages: ref(true),
      agentRenderableMessages: ref([{ key: 'assistant-deferred', message: { role: 'assistant' } }]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache,
      messageVirtualLayoutVersion: ref(0),
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: (key: string) => messageVirtualHeightCache.get(key) || 0
    });

    runtime.rememberCurrentScroll();
    container.scrollTop = 0;

    assert.equal(await runtime.restoreConversationScroll({ deferMeasure: true }), true);
    assert.equal(container.scrollTop, 260);
    assert.equal(messageVirtualHeightCache.has('assistant-deferred'), false);
    assert.equal(idleQueue.length, 1);

    flushIdle();

    assert.equal(messageVirtualHeightCache.get('assistant-deferred'), 188);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
  }
});

test('message viewport runtime can store scroll under an explicit previous conversation key', async () => {
  const activeConversationKey = ref('agent:session-b');
  const container = {
    scrollTop: 360,
    clientHeight: 400,
    scrollHeight: 1600,
    querySelectorAll: () => [],
    getBoundingClientRect: () => ({ top: 0, height: 400 })
  } as unknown as HTMLElement;
  const runtime = createMessageViewportRuntime({
    messageListRef: ref(container),
    showChatSettingsView: ref(false),
    autoStickToBottom: ref(false),
    showScrollTopButton: ref(false),
    showScrollBottomButton: ref(false),
    isAgentConversationActive: ref(true),
    isWorldConversationActive: ref(false),
    activeConversationKey,
    shouldVirtualizeMessages: ref(false),
    agentRenderableMessages: ref([]),
    worldRenderableMessages: ref([]),
    messageVirtualHeightCache: new Map<string, number>(),
    messageVirtualLayoutVersion: ref(0),
    messageVirtualScrollTop: ref(0),
    messageVirtualViewportHeight: ref(0),
    estimateVirtualOffsetTop: () => 0,
    resolveVirtualMessageHeight: () => 0
  });

  runtime.rememberScrollForKey('agent:session-a');
  container.scrollTop = 0;
  activeConversationKey.value = 'agent:session-a';

  assert.equal(await runtime.restoreConversationScroll(), true);
  assert.equal(container.scrollTop, 360);

  runtime.dispose();
});

test('message viewport runtime loads older agent history near top and preserves viewport', async () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const frameQueue: FrameRequestCallback[] = [];
  const flushFrames = () => {
    while (frameQueue.length > 0) {
      const frame = frameQueue.shift();
      frame?.(performance.now());
    }
  };
  const waitForMicrotasks = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

  try {
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {}
    };

    let loadCalls = 0;
    const container = {
      scrollTop: 0,
      clientHeight: 500,
      scrollHeight: 1000,
      querySelectorAll: () => [],
      getBoundingClientRect: () => ({ top: 0, height: 500 })
    } as unknown as HTMLElement;
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(false),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:session-history'),
      shouldVirtualizeMessages: ref(false),
      agentRenderableMessages: ref([]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache: new Map<string, number>(),
      messageVirtualLayoutVersion: ref(0),
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: () => 0,
      loadOlderHistory: async () => {
        loadCalls += 1;
        (container as unknown as { scrollHeight: number }).scrollHeight = 1320;
        return [{ history_id: 1 }];
      }
    });

    runtime.handleMessageListScroll();
    flushFrames();
    await waitForMicrotasks();
    flushFrames();

    assert.equal(loadCalls, 1);
    assert.equal(container.scrollTop, 320);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
  }
});

test('message viewport runtime auto-loads older history when refreshed viewport is underfilled', async () => {
  const originalWindow = (globalThis as Record<string, unknown>).window;
  const frameQueue: FrameRequestCallback[] = [];
  const flushFrames = () => {
    while (frameQueue.length > 0) {
      const frame = frameQueue.shift();
      frame?.(performance.now());
    }
  };
  const waitForMicrotasks = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

  try {
    (globalThis as Record<string, unknown>).window = {
      requestAnimationFrame: (callback: FrameRequestCallback) => {
        frameQueue.push(callback);
        return frameQueue.length;
      },
      cancelAnimationFrame: () => {}
    };

    let loadCalls = 0;
    const container = {
      scrollTop: 0,
      clientHeight: 720,
      scrollHeight: 480,
      querySelectorAll: () => [],
      getBoundingClientRect: () => ({ top: 0, height: 720 })
    } as unknown as HTMLElement;
    const runtime = createMessageViewportRuntime({
      messageListRef: ref(container),
      showChatSettingsView: ref(false),
      autoStickToBottom: ref(true),
      showScrollTopButton: ref(false),
      showScrollBottomButton: ref(false),
      isAgentConversationActive: ref(true),
      isWorldConversationActive: ref(false),
      activeConversationKey: ref('agent:session-underfilled'),
      shouldVirtualizeMessages: ref(false),
      agentRenderableMessages: ref([]),
      worldRenderableMessages: ref([]),
      messageVirtualHeightCache: new Map<string, number>(),
      messageVirtualLayoutVersion: ref(0),
      messageVirtualScrollTop: ref(0),
      messageVirtualViewportHeight: ref(0),
      estimateVirtualOffsetTop: () => 0,
      resolveVirtualMessageHeight: () => 0,
      loadOlderHistory: async () => {
        loadCalls += 1;
        (container as unknown as { scrollHeight: number }).scrollHeight = 900;
        return [{ history_id: 7 }];
      }
    });

    runtime.scheduleMessageViewportRefresh({
      updateScrollState: true,
      measure: true,
      reason: 'detail-hydrated'
    });
    flushFrames();
    await waitForMicrotasks();
    flushFrames();

    assert.equal(loadCalls, 1);
    assert.equal(container.scrollTop, 420);
    runtime.dispose();
  } finally {
    if (originalWindow === undefined) {
      delete (globalThis as Record<string, unknown>).window;
    } else {
      (globalThis as Record<string, unknown>).window = originalWindow;
    }
  }
});
