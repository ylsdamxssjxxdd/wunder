import { nextTick, type Ref } from 'vue';

export type RenderableMessage = {
  key: string;
  message?: Record<string, unknown>;
};

export type MessageViewportRuntimeOptions = {
  messageListRef: Ref<HTMLElement | null>;
  showChatSettingsView: Ref<boolean>;
  autoStickToBottom: Ref<boolean>;
  showScrollTopButton: Ref<boolean>;
  showScrollBottomButton: Ref<boolean>;
  isAgentConversationActive: Ref<boolean>;
  isWorldConversationActive: Ref<boolean>;
  shouldVirtualizeMessages: Ref<boolean>;
  agentRenderableMessages: Ref<RenderableMessage[]>;
  worldRenderableMessages: Ref<RenderableMessage[]>;
  messageVirtualHeightCache: Map<string, number>;
  messageVirtualLayoutVersion: Ref<number>;
  messageVirtualScrollTop: Ref<number>;
  messageVirtualViewportHeight: Ref<number>;
  estimateVirtualOffsetTop: (keys: string[], index: number) => number;
  resolveVirtualMessageHeight: (key: string) => number;
};

export type MessageViewportRuntime = {
  handleMessageListScroll: () => void;
  handleWorkflowLayoutChange: () => void;
  scrollMessagesToBottom: (force?: boolean) => Promise<void>;
  jumpToMessageBottom: () => Promise<void>;
  jumpToMessageTop: () => Promise<void>;
  scrollVirtualMessageToIndex: (keys: string[], index: number, align?: 'center' | 'start') => void;
  scrollLatestAssistantToCenter: () => Promise<void>;
  scheduleMessageViewportRefresh: (options?: { updateScrollState?: boolean; measure?: boolean }) => void;
  scheduleMessageVirtualMeasure: () => void;
  updateMessageScrollState: () => void;
  syncMessageVirtualMetrics: () => void;
  pruneMessageVirtualHeightCache: () => void;
  dispose: () => void;
};

const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));

export const createMessageViewportRuntime = (
  options: MessageViewportRuntimeOptions
): MessageViewportRuntime => {
  let messageScrollFrame: number | null = null;
  let messageVirtualMeasureFrame: number | null = null;
  let messageViewportRefreshFrame: number | null = null;
  let scheduledViewportRefreshNeedsScrollState = false;
  let scheduledViewportRefreshNeedsMeasure = false;

  const applyMessageVirtualMetrics = (scrollTop: number, viewportHeight: number) => {
    if (options.messageVirtualViewportHeight.value !== viewportHeight) {
      options.messageVirtualViewportHeight.value = viewportHeight;
    }
    if (options.messageVirtualScrollTop.value !== scrollTop) {
      options.messageVirtualScrollTop.value = scrollTop;
    }
  };

  const syncMessageVirtualMetrics = () => {
    const container = options.messageListRef.value;
    if (!container || options.showChatSettingsView.value) {
      applyMessageVirtualMetrics(0, 0);
      return;
    }
    applyMessageVirtualMetrics(container.scrollTop, container.clientHeight);
  };

  const pruneMessageVirtualHeightCache = () => {
    const keys = new Set<string>([
      ...options.agentRenderableMessages.value.map((item) => item.key),
      ...options.worldRenderableMessages.value.map((item) => item.key)
    ]);
    let changed = false;
    options.messageVirtualHeightCache.forEach((_value, key) => {
      if (keys.has(key)) {
        return;
      }
      options.messageVirtualHeightCache.delete(key);
      changed = true;
    });
    if (changed) {
      options.messageVirtualLayoutVersion.value += 1;
    }
  };

  const measureVisibleMessageHeights = () => {
    const container = options.messageListRef.value;
    if (!container || options.showChatSettingsView.value || !options.shouldVirtualizeMessages.value) {
      return;
    }
    const nodes = container.querySelectorAll<HTMLElement>('.messenger-message[data-virtual-key]');
    let changed = false;
    nodes.forEach((node) => {
      const key = String(node.dataset.virtualKey || '').trim();
      if (!key) return;
      const offsetHeight = Math.round(node.offsetHeight || 0);
      const height = Math.max(
        1,
        offsetHeight || Math.round(node.getBoundingClientRect().height)
      );
      const cached = options.messageVirtualHeightCache.get(key);
      if (cached && Math.abs(cached - height) <= 1) {
        return;
      }
      options.messageVirtualHeightCache.set(key, height);
      changed = true;
    });
    if (changed) {
      options.messageVirtualLayoutVersion.value += 1;
    }
  };

  const updateMessageScrollState = () => {
    const container = options.messageListRef.value;
    if (!container || options.showChatSettingsView.value) {
      options.showScrollTopButton.value = false;
      options.showScrollBottomButton.value = false;
      options.autoStickToBottom.value = true;
      return;
    }
    const nearTop = container.scrollTop <= 72;
    const remaining = container.scrollHeight - container.clientHeight - container.scrollTop;
    const shouldStick = remaining <= 72;
    const isConversation = options.isAgentConversationActive.value || options.isWorldConversationActive.value;
    options.autoStickToBottom.value = shouldStick;
    options.showScrollTopButton.value = !nearTop && isConversation;
    options.showScrollBottomButton.value = !shouldStick && isConversation;
  };

  const scheduleMessageVirtualMeasure = () => {
    if (typeof window === 'undefined') return;
    if (!options.shouldVirtualizeMessages.value) return;
    if (messageVirtualMeasureFrame !== null) return;
    messageVirtualMeasureFrame = window.requestAnimationFrame(() => {
      messageVirtualMeasureFrame = null;
      measureVisibleMessageHeights();
    });
  };

  const scheduleMessageViewportRefresh = (
    refreshOptions: { updateScrollState?: boolean; measure?: boolean } = {}
  ) => {
    const shouldUpdateScrollState = refreshOptions.updateScrollState === true;
    const shouldMeasure = refreshOptions.measure === true;
    if (typeof window === 'undefined') {
      syncMessageVirtualMetrics();
      if (shouldUpdateScrollState) {
        updateMessageScrollState();
      }
      if (shouldMeasure) {
        measureVisibleMessageHeights();
      }
      return;
    }
    scheduledViewportRefreshNeedsScrollState =
      scheduledViewportRefreshNeedsScrollState || shouldUpdateScrollState;
    scheduledViewportRefreshNeedsMeasure =
      scheduledViewportRefreshNeedsMeasure || shouldMeasure;
    if (messageViewportRefreshFrame !== null) return;
    messageViewportRefreshFrame = window.requestAnimationFrame(() => {
      messageViewportRefreshFrame = null;
      const shouldFlushScrollState = scheduledViewportRefreshNeedsScrollState;
      const shouldFlushMeasure = scheduledViewportRefreshNeedsMeasure;
      scheduledViewportRefreshNeedsScrollState = false;
      scheduledViewportRefreshNeedsMeasure = false;
      syncMessageVirtualMetrics();
      if (shouldFlushScrollState) {
        updateMessageScrollState();
      }
      if (shouldFlushMeasure) {
        measureVisibleMessageHeights();
      }
    });
  };

  const handleMessageListScroll = () => {
    if (typeof window === 'undefined') {
      syncMessageVirtualMetrics();
      updateMessageScrollState();
      return;
    }
    if (messageScrollFrame !== null) return;
    messageScrollFrame = window.requestAnimationFrame(() => {
      messageScrollFrame = null;
      syncMessageVirtualMetrics();
      updateMessageScrollState();
      scheduleMessageVirtualMeasure();
    });
  };

  const handleMessageWorkflowLayoutChange = () => {
    scheduleMessageViewportRefresh({
      updateScrollState: true,
      measure: true
    });
  };

  const scrollVirtualMessageToIndex = (
    keys: string[],
    index: number,
    align: 'center' | 'start' = 'center'
  ) => {
    const container = options.messageListRef.value;
    if (!container || !keys.length) return;
    const safeIndex = clamp(Math.trunc(index), 0, keys.length - 1);
    const top = options.estimateVirtualOffsetTop(keys, safeIndex);
    const height = options.resolveVirtualMessageHeight(keys[safeIndex]);
    const targetTop = align === 'center'
      ? top - container.clientHeight / 2 + height / 2
      : top;
    const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
    container.scrollTop = clamp(targetTop, 0, maxTop);
    syncMessageVirtualMetrics();
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  };

  const scrollMessagesToBottom = async (force = false) => {
    await nextTick();
    const container = options.messageListRef.value;
    if (!container) return;
    if (!force && !options.autoStickToBottom.value) {
      updateMessageScrollState();
      scheduleMessageVirtualMeasure();
      return;
    }
    container.scrollTop = container.scrollHeight;
    syncMessageVirtualMetrics();
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  };

  const jumpToMessageBottom = async () => {
    options.autoStickToBottom.value = true;
    await scrollMessagesToBottom(true);
  };

  const jumpToMessageTop = async () => {
    await nextTick();
    const container = options.messageListRef.value;
    if (!container) return;
    options.autoStickToBottom.value = false;
    container.scrollTop = 0;
    syncMessageVirtualMetrics();
    updateMessageScrollState();
    scheduleMessageVirtualMeasure();
  };

  const scrollLatestAssistantToCenter = async () => {
    if (!options.isAgentConversationActive.value) return;
    if (options.shouldVirtualizeMessages.value) {
      const latestIndex = (() => {
        for (let cursor = options.agentRenderableMessages.value.length - 1; cursor >= 0; cursor -= 1) {
          const item = options.agentRenderableMessages.value[cursor];
          if (String(item.message?.role || '').toLowerCase() !== 'assistant') continue;
          return cursor;
        }
        return -1;
      })();
      if (latestIndex >= 0) {
        scrollVirtualMessageToIndex(
          options.agentRenderableMessages.value.map((item) => item.key),
          latestIndex,
          'center'
        );
        await nextTick();
      }
    }
    await nextTick();
    const container = options.messageListRef.value;
    if (!container) return;
    const items = container.querySelectorAll('.messenger-message:not(.mine)');
    if (!items.length) return;
    const target = items[items.length - 1] as HTMLElement;
    requestAnimationFrame(() => {
      const containerRect = container.getBoundingClientRect();
      const targetRect = target.getBoundingClientRect();
      const targetCenter = targetRect.top - containerRect.top + targetRect.height / 2;
      const nextTop = container.scrollTop + targetCenter - container.clientHeight / 2;
      const maxTop = Math.max(0, container.scrollHeight - container.clientHeight);
      container.scrollTop = clamp(nextTop, 0, maxTop);
      syncMessageVirtualMetrics();
      updateMessageScrollState();
      scheduleMessageVirtualMeasure();
    });
  };

  const dispose = () => {
    if (typeof window !== 'undefined' && messageScrollFrame !== null) {
      window.cancelAnimationFrame(messageScrollFrame);
      messageScrollFrame = null;
    }
    if (typeof window !== 'undefined' && messageVirtualMeasureFrame !== null) {
      window.cancelAnimationFrame(messageVirtualMeasureFrame);
      messageVirtualMeasureFrame = null;
    }
    if (typeof window !== 'undefined' && messageViewportRefreshFrame !== null) {
      window.cancelAnimationFrame(messageViewportRefreshFrame);
      messageViewportRefreshFrame = null;
    }
  };

  return {
    handleMessageListScroll,
    handleWorkflowLayoutChange: handleMessageWorkflowLayoutChange,
    scrollMessagesToBottom,
    jumpToMessageBottom,
    jumpToMessageTop,
    scrollVirtualMessageToIndex,
    scrollLatestAssistantToCenter,
    scheduleMessageViewportRefresh,
    scheduleMessageVirtualMeasure,
    updateMessageScrollState,
    syncMessageVirtualMetrics,
    pruneMessageVirtualHeightCache,
    dispose
  };
};
