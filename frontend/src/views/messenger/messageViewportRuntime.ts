import { nextTick, type Ref } from 'vue';

import { chatDebugLog, isChatDebugEnabled } from '../../utils/chatDebug';

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
  handleWorkflowLayoutChange: (messageKey?: string) => void;
  scrollMessagesToBottom: (force?: boolean) => Promise<void>;
  jumpToMessageBottom: () => Promise<void>;
  jumpToMessageTop: () => Promise<void>;
  scrollVirtualMessageToIndex: (keys: string[], index: number, align?: 'center' | 'start') => void;
  scrollLatestAssistantToCenter: () => Promise<void>;
  scheduleMessageViewportRefresh: (options?: {
    updateScrollState?: boolean;
    measure?: boolean;
    measureKeys?: string[];
  }) => void;
  scheduleMessageVirtualMeasure: (measureKeys?: string[]) => void;
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
  let scheduledViewportRefreshMeasureAll = false;
  let scheduledViewportRefreshMeasureKeys = new Set<string>();
  let scheduledVirtualMeasureAll = false;
  let scheduledVirtualMeasureKeys = new Set<string>();
  let messageResizeObserver: ResizeObserver | null = null;
  const observedMessageNodes = new Map<string, HTMLElement>();

  const logViewportDebug = (event: string, payload?: unknown) => {
    if (!isChatDebugEnabled()) {
      return;
    }
    chatDebugLog('messenger.viewport', event, payload);
  };

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

  const collectMeasureKeys = (keys: string[] | undefined): string[] =>
    Array.isArray(keys)
      ? keys.map((key) => String(key || '').trim()).filter(Boolean)
      : [];

  const measureMessageNode = (
    node: HTMLElement
  ): { key: string; previous: number | null; next: number } | null => {
    const key = String(node?.dataset?.virtualKey || '').trim();
    if (!key) {
      return null;
    }
    const offsetHeight = Math.round(node.offsetHeight || 0);
    const height = Math.max(
      1,
      offsetHeight || Math.round(node.getBoundingClientRect().height)
    );
    const cached = options.messageVirtualHeightCache.get(key);
    if (cached && Math.abs(cached - height) <= 1) {
      return null;
    }
    options.messageVirtualHeightCache.set(key, height);
    return {
      key,
      previous: typeof cached === 'number' ? cached : null,
      next: height
    };
  };

  const releaseObservedMessageNode = (key: string) => {
    const normalizedKey = String(key || '').trim();
    if (!normalizedKey) {
      return;
    }
    const node = observedMessageNodes.get(normalizedKey);
    if (!node) {
      return;
    }
    if (messageResizeObserver) {
      messageResizeObserver.unobserve(node);
    }
    observedMessageNodes.delete(normalizedKey);
  };

  const releaseObservedMessageNodes = () => {
    if (messageResizeObserver) {
      messageResizeObserver.disconnect();
    }
    observedMessageNodes.clear();
  };

  const syncVisibleMessageResizeObserverTargets = () => {
    const container = options.messageListRef.value;
    if (
      !container ||
      options.showChatSettingsView.value ||
      !options.shouldVirtualizeMessages.value ||
      typeof ResizeObserver === 'undefined'
    ) {
      releaseObservedMessageNodes();
      return;
    }
    if (!messageResizeObserver) {
      messageResizeObserver = new ResizeObserver((entries) => {
        const changes = entries
          .map((entry) => measureMessageNode(entry.target as HTMLElement))
          .filter((change): change is { key: string; previous: number | null; next: number } => Boolean(change));
        if (!changes.length) {
          return;
        }
        options.messageVirtualLayoutVersion.value += 1;
        syncMessageVirtualMetrics();
        updateMessageScrollState();
        logViewportDebug('row-resize', {
          changeCount: changes.length,
          changes
        });
      });
    }
    const nodes = container.querySelectorAll<HTMLElement>('.messenger-message[data-virtual-key]');
    const nextNodes = new Map<string, HTMLElement>();
    nodes.forEach((node) => {
      const key = String(node?.dataset?.virtualKey || '').trim();
      if (!key) {
        return;
      }
      nextNodes.set(key, node);
      const previousNode = observedMessageNodes.get(key);
      if (previousNode === node) {
        return;
      }
      if (previousNode && messageResizeObserver) {
        messageResizeObserver.unobserve(previousNode);
      }
      observedMessageNodes.set(key, node);
      messageResizeObserver.observe(node);
    });
    Array.from(observedMessageNodes.keys()).forEach((key) => {
      if (nextNodes.has(key)) {
        return;
      }
      releaseObservedMessageNode(key);
    });
  };

  const markMeasureTargets = (
    nextKeys: string[] | undefined,
    mode: 'viewport' | 'virtual'
  ) => {
    const normalizedKeys = collectMeasureKeys(nextKeys);
    if (mode === 'viewport') {
      if (!normalizedKeys.length) {
        scheduledViewportRefreshMeasureAll = true;
        scheduledViewportRefreshMeasureKeys.clear();
        return;
      }
      if (scheduledViewportRefreshMeasureAll) {
        return;
      }
      normalizedKeys.forEach((key) => scheduledViewportRefreshMeasureKeys.add(key));
      return;
    }
    if (!normalizedKeys.length) {
      scheduledVirtualMeasureAll = true;
      scheduledVirtualMeasureKeys.clear();
      return;
    }
    if (scheduledVirtualMeasureAll) {
      return;
    }
    normalizedKeys.forEach((key) => scheduledVirtualMeasureKeys.add(key));
  };

  const measureVisibleMessageHeights = (targetKeys?: string[]) => {
    const container = options.messageListRef.value;
    syncVisibleMessageResizeObserverTargets();
    if (!container || options.showChatSettingsView.value || !options.shouldVirtualizeMessages.value) {
      return;
    }
    const normalizedTargetKeys = collectMeasureKeys(targetKeys);
    const targetKeySet = normalizedTargetKeys.length ? new Set(normalizedTargetKeys) : null;
    const nodes = container.querySelectorAll<HTMLElement>('.messenger-message[data-virtual-key]');
    const changes: Array<{ key: string; previous: number | null; next: number }> = [];
    nodes.forEach((node) => {
      const key = String(node.dataset.virtualKey || '').trim();
      if (!key) return;
      if (targetKeySet && !targetKeySet.has(key)) {
        return;
      }
      const change = measureMessageNode(node);
      if (change) {
        changes.push(change);
      }
    });
    if (changes.length) {
      options.messageVirtualLayoutVersion.value += 1;
      logViewportDebug('measure-visible', {
        targetKeys: normalizedTargetKeys,
        changeCount: changes.length,
        changes
      });
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

  const scheduleMessageVirtualMeasure = (measureKeys?: string[]) => {
    if (typeof window === 'undefined') return;
    if (!options.shouldVirtualizeMessages.value) return;
    markMeasureTargets(measureKeys, 'virtual');
    if (messageVirtualMeasureFrame !== null) return;
    messageVirtualMeasureFrame = window.requestAnimationFrame(() => {
      messageVirtualMeasureFrame = null;
      const shouldMeasureAll = scheduledVirtualMeasureAll;
      const nextMeasureKeys = shouldMeasureAll
        ? undefined
        : Array.from(scheduledVirtualMeasureKeys);
      scheduledVirtualMeasureAll = false;
      scheduledVirtualMeasureKeys.clear();
      measureVisibleMessageHeights(nextMeasureKeys);
    });
  };

  const scheduleMessageViewportRefresh = (
    refreshOptions: {
      updateScrollState?: boolean;
      measure?: boolean;
      measureKeys?: string[];
    } = {}
  ) => {
    const shouldUpdateScrollState = refreshOptions.updateScrollState === true;
    const shouldMeasure = refreshOptions.measure === true;
    if (typeof window === 'undefined') {
      syncMessageVirtualMetrics();
      if (shouldUpdateScrollState) {
        updateMessageScrollState();
      }
      if (shouldMeasure) {
        measureVisibleMessageHeights(refreshOptions.measureKeys);
      }
      return;
    }
    scheduledViewportRefreshNeedsScrollState =
      scheduledViewportRefreshNeedsScrollState || shouldUpdateScrollState;
    scheduledViewportRefreshNeedsMeasure =
      scheduledViewportRefreshNeedsMeasure || shouldMeasure;
    if (shouldMeasure) {
      markMeasureTargets(refreshOptions.measureKeys, 'viewport');
    }
    if (messageViewportRefreshFrame !== null) return;
    messageViewportRefreshFrame = window.requestAnimationFrame(() => {
      messageViewportRefreshFrame = null;
      const shouldFlushScrollState = scheduledViewportRefreshNeedsScrollState;
      const shouldFlushMeasure = scheduledViewportRefreshNeedsMeasure;
      const shouldMeasureAll = scheduledViewportRefreshMeasureAll;
      const nextMeasureKeys = shouldMeasureAll
        ? undefined
        : Array.from(scheduledViewportRefreshMeasureKeys);
      scheduledViewportRefreshNeedsScrollState = false;
      scheduledViewportRefreshNeedsMeasure = false;
      scheduledViewportRefreshMeasureAll = false;
      scheduledViewportRefreshMeasureKeys.clear();
      syncMessageVirtualMetrics();
      if (shouldFlushScrollState) {
        updateMessageScrollState();
      }
      if (shouldFlushMeasure) {
        measureVisibleMessageHeights(nextMeasureKeys);
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

  const handleMessageWorkflowLayoutChange = (messageKey?: string) => {
    logViewportDebug('workflow-layout-change', {
      messageKey: String(messageKey || '').trim()
    });
    scheduleMessageViewportRefresh({
      updateScrollState: true,
      measure: true,
      measureKeys: messageKey ? [messageKey] : undefined
    });
    scheduleMessageVirtualMeasure(messageKey ? [messageKey] : undefined);
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
    releaseObservedMessageNodes();
    messageResizeObserver = null;
    scheduledViewportRefreshNeedsScrollState = false;
    scheduledViewportRefreshNeedsMeasure = false;
    scheduledViewportRefreshMeasureAll = false;
    scheduledViewportRefreshMeasureKeys.clear();
    scheduledVirtualMeasureAll = false;
    scheduledVirtualMeasureKeys.clear();
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
