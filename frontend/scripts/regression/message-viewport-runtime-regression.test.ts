import test from 'node:test';
import assert from 'node:assert/strict';

import { ref } from 'vue';

import { createMessageViewportRuntime } from '../../src/views/messenger/messageViewportRuntime';

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
