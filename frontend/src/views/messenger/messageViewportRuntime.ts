import { nextTick, type Ref } from 'vue';

import { chatDebugLog, isChatDebugEnabled } from '../../utils/chatDebug';
import {
  rememberMessageScrollPosition,
  restoreMessageScrollPosition
} from './messageScrollMemory';

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
  activeConversationKey: Ref<string>;
  shouldVirtualizeMessages: Ref<boolean>;
  agentRenderableMessages: Ref<RenderableMessage[]>;
  worldRenderableMessages: Ref<RenderableMessage[]>;
  messageVirtualHeightCache: Map<string, number>;
  messageVirtualLayoutVersion: Ref<number>;
  messageVirtualScrollTop: Ref<number>;
  messageVirtualViewportHeight: Ref<number>;
  estimateVirtualOffsetTop: (keys: string[], index: number) => number;
  resolveVirtualMessageHeight: (key: string) => number;
  loadOlderHistory?: () => Promise<unknown[] | unknown>;
};

export type MessageViewportRuntime = {
  handleMessageListScroll: () => void;
  handleWorkflowLayoutChange: (messageKey?: string) => void;
  scrollMessagesToBottom: (force?: boolean) => Promise<void>;
  jumpToMessageBottom: () => Promise<void>;
  jumpToMessageTop: () => Promise<void>;
  scrollVirtualMessageToIndex: (keys: string[], index: number, align?: 'center' | 'start') => void;
  scrollLatestAssistantToCenter: () => Promise<void>;
  restoreConversationScroll: (options?: { deferMeasure?: boolean }) => Promise<boolean>;
  rememberCurrentScroll: () => void;
  rememberScrollForKey: (key: string) => void;
  scheduleMessageViewportRefresh: (options?: {
    updateScrollState?: boolean;
    measure?: boolean;
    measureKeys?: string[];
    reason?: string;
  }) => void;
  scheduleMessageVirtualMeasure: (measureKeys?: string[]) => void;
  updateMessageScrollState: () => void;
  syncMessageVirtualMetrics: () => void;
  pruneMessageVirtualHeightCache: () => void;
  dispose: () => void;
};

const clamp = (value: number, min: number, max: number): number => Math.max(min, Math.min(max, value));

// Measurements are disposable UI state; retain only a bounded recent working set.
export const MESSAGE_VIRTUAL_HEIGHT_CACHE_LIMIT = 384;
const MESSAGE_RESIZE_REFRESH_DELAY_MS = 64;
const WORKFLOW_LAYOUT_REFRESH_INTERVAL_MS = 120;

export const createMessageViewportRuntime = (
  options: MessageViewportRuntimeOptions
): MessageViewportRuntime => {
  let messageScrollFrame: number | null = null;
  let messageScrollSettleHandle: number | null = null;
  let messageVirtualMeasureFrame: number | null = null;
  let messageViewportRefreshFrame: number | null = null;
  let messageDeferredMeasureHandle: number | null = null;
  let messageResizeRefreshHandle: number | null = null;
  let messageDeferredMeasureUsesIdleCallback = false;
  let scheduledViewportRefreshNeedsScrollState = false;
  let scheduledViewportRefreshNeedsMeasure = false;
  let scheduledViewportRefreshMeasureAll = false;
  let scheduledViewportRefreshMeasureKeys = new Set<string>();
  let scheduledViewportRefreshReason = '';
  let scheduledVirtualMeasureAll = false;
  let scheduledVirtualMeasureKeys = new Set<string>();
  let messageResizeObserver: ResizeObserver | null = null;
  let workflowLayoutRefreshHandle: number | null = null;
  let lastWorkflowLayoutRefreshAt = Number.NEGATIVE_INFINITY;
  let pendingWorkflowLayoutMessageKey = '';
  const observedMessageNodes = new Map<string, HTMLElement>();
  const pendingObservedResizeChanges = new Map<string, { key: string; previous: number | null; next: number }>();
  let olderHistoryLoadInFlight = false;

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

  const syncMessageVirtualMetrics = (duringScroll = false) => {
    const container = options.messageListRef.value;
    if (!container) {
      applyMessageVirtualMetrics(0, 0);
      return;
    }
    if (options.showChatSettingsView.value) {
      return;
    }
    const nextScrollTop = container.scrollTop;
    // The virtual window has overscan, so sub-row movement need not invalidate Vue's
    // message tree. This avoids rebuilding the height prefix on every wheel tick.
    const shouldUpdateScrollTop = !duringScroll ||
      Math.abs(options.messageVirtualScrollTop.value - nextScrollTop) >= 48;
    applyMessageVirtualMetrics(
      shouldUpdateScrollTop ? nextScrollTop : options.messageVirtualScrollTop.value,
      container.clientHeight
    );
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
    while (options.messageVirtualHeightCache.size > MESSAGE_VIRTUAL_HEIGHT_CACHE_LIMIT) {
      const oldestKey = options.messageVirtualHeightCache.keys().next().value;
      if (!oldestKey) {
        break;
      }
      options.messageVirtualHeightCache.delete(oldestKey);
      changed = true;
    }
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
    // Refresh insertion order so recently measured visible rows survive eviction.
    if (cached !== undefined) {
      options.messageVirtualHeightCache.delete(key);
    }
    options.messageVirtualHeightCache.set(key, height);
    // Map insertion order doubles as a cheap LRU clock for measured rows.
    pruneMessageVirtualHeightCache();
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

  const flushObservedMessageResizeChanges = () => {
    messageResizeRefreshHandle = null;
    const changes = Array.from(pendingObservedResizeChanges.values());
    pendingObservedResizeChanges.clear();
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
  };

  const scheduleObservedMessageResizeRefresh = (
    changes: Array<{ key: string; previous: number | null; next: number }>
  ) => {
    changes.forEach((change) => pendingObservedResizeChanges.set(change.key, change));
    if (typeof window === 'undefined' || typeof window.setTimeout !== 'function') {
      flushObservedMessageResizeChanges();
      return;
    }
    if (messageResizeRefreshHandle !== null) {
      return;
    }
    messageResizeRefreshHandle = window.setTimeout(
      flushObservedMessageResizeChanges,
      MESSAGE_RESIZE_REFRESH_DELAY_MS
    );
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
        scheduleObservedMessageResizeRefresh(changes);
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
    const startedAt = typeof performance !== 'undefined' ? performance.now() : Date.now();
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
    }
    logViewportDebug('measure-visible', {
      targetKeys: normalizedTargetKeys,
      measuredNodeCount: nodes.length,
      changeCount: changes.length,
      durationMs: Number(((typeof performance !== 'undefined' ? performance.now() : Date.now()) - startedAt).toFixed(1)),
      changes: changes.slice(0, 12)
    });
  };

  const scheduleDeferredVisibleMeasure = (reason = '') => {
    if (typeof window === 'undefined') {
      measureVisibleMessageHeights();
      return;
    }
    if (messageDeferredMeasureHandle !== null) return;
    const run = () => {
      messageDeferredMeasureHandle = null;
      messageDeferredMeasureUsesIdleCallback = false;
      logViewportDebug('deferred-measure-run', { reason });
      measureVisibleMessageHeights();
    };
    const requestIdle = (window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
    }).requestIdleCallback;
    if (typeof requestIdle === 'function') {
      messageDeferredMeasureUsesIdleCallback = true;
      messageDeferredMeasureHandle = requestIdle(run, { timeout: 220 });
      logViewportDebug('deferred-measure-scheduled', { reason, mode: 'idle' });
      return;
    }
    messageDeferredMeasureUsesIdleCallback = false;
    messageDeferredMeasureHandle = window.setTimeout(run, 64);
    logViewportDebug('deferred-measure-scheduled', { reason, mode: 'timeout' });
  };

  const updateMessageScrollState = () => {
    const container = options.messageListRef.value;
    if (!container) {
      options.showScrollTopButton.value = false;
      options.showScrollBottomButton.value = false;
      options.autoStickToBottom.value = true;
      return;
    }
    if (options.showChatSettingsView.value) {
      options.showScrollTopButton.value = false;
      options.showScrollBottomButton.value = false;
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

  const maybeLoadOlderHistory = async () => {
    const container = options.messageListRef.value;
    if (
      olderHistoryLoadInFlight ||
      !container ||
      options.showChatSettingsView.value ||
      !options.isAgentConversationActive.value ||
      container.scrollTop > 96 ||
      typeof options.loadOlderHistory !== 'function'
    ) {
      return;
    }

    olderHistoryLoadInFlight = true;
    const previousScrollHeight = container.scrollHeight;
    const previousScrollTop = container.scrollTop;
    try {
      const loaded = await options.loadOlderHistory();
      const loadedCount = Array.isArray(loaded) ? loaded.length : 0;
      if (loadedCount <= 0) return;
      await nextTick();
      const nextContainer = options.messageListRef.value;
      if (!nextContainer) return;
      const heightDelta = Math.max(0, nextContainer.scrollHeight - previousScrollHeight);
      nextContainer.scrollTop = previousScrollTop + heightDelta;
      syncMessageVirtualMetrics();
      updateMessageScrollState();
      scheduleMessageViewportRefresh({
        updateScrollState: true,
        measure: true,
        reason: 'older-history-loaded'
      });
    } finally {
      olderHistoryLoadInFlight = false;
    }
  };

  const rememberCurrentScroll = () => {
    rememberMessageScrollPosition(options.activeConversationKey.value, options.messageListRef.value);
  };

  const rememberScrollForKey = (key: string) => {
    rememberMessageScrollPosition(key, options.messageListRef.value);
  };

  const restoreConversationScroll = async (restoreOptions: { deferMeasure?: boolean } = {}) => {
    await nextTick();
    const container = options.messageListRef.value;
    if (!container || options.showChatSettingsView.value) return false;
    const restored = restoreMessageScrollPosition(options.activeConversationKey.value, container);
    if (!restored) return false;
    syncMessageVirtualMetrics();
    updateMessageScrollState();
    if (restoreOptions.deferMeasure === true) {
      scheduleDeferredVisibleMeasure('restore-conversation-scroll');
    } else {
      scheduleMessageVirtualMeasure();
    }
    return true;
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

  const scheduleScrollSettleMeasure = () => {
    if (typeof window === 'undefined' || !options.shouldVirtualizeMessages.value) return;
    if (messageScrollSettleHandle !== null) {
      window.clearTimeout(messageScrollSettleHandle);
    }
    // Reading row geometry during every scroll frame forces synchronous layout on older Chrome.
    // ResizeObserver handles modern browsers; this is only a quiet-period fallback.
    messageScrollSettleHandle = window.setTimeout(() => {
      messageScrollSettleHandle = null;
      syncMessageVirtualMetrics();
      if (typeof ResizeObserver === 'undefined') {
        scheduleMessageVirtualMeasure();
      }
    }, 120);
  };

  const scheduleMessageViewportRefresh = (
    refreshOptions: {
      updateScrollState?: boolean;
      measure?: boolean;
      measureKeys?: string[];
      reason?: string;
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
    if (refreshOptions.reason) {
      scheduledViewportRefreshReason = scheduledViewportRefreshReason
        ? `${scheduledViewportRefreshReason},${refreshOptions.reason}`
        : refreshOptions.reason;
    }
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
      const reason = scheduledViewportRefreshReason;
      scheduledViewportRefreshNeedsScrollState = false;
      scheduledViewportRefreshNeedsMeasure = false;
      scheduledViewportRefreshMeasureAll = false;
      scheduledViewportRefreshReason = '';
      scheduledViewportRefreshMeasureKeys.clear();
      syncMessageVirtualMetrics();
      if (shouldFlushScrollState) {
        updateMessageScrollState();
      }
      if (shouldFlushMeasure) {
        if (shouldMeasureAll && options.shouldVirtualizeMessages.value) {
          scheduleDeferredVisibleMeasure(reason || 'viewport-refresh');
        } else {
          measureVisibleMessageHeights(nextMeasureKeys);
        }
      }
      void maybeLoadOlderHistory();
    });
  };

  const handleMessageListScroll = () => {
    // Commit the follow mode in the scroll event itself. Streaming layout
    // updates may request a bottom scroll before the deferred frame runs.
    updateMessageScrollState();
    if (typeof window === 'undefined') {
      syncMessageVirtualMetrics();
      rememberCurrentScroll();
      return;
    }
    if (messageScrollFrame !== null) return;
    messageScrollFrame = window.requestAnimationFrame(() => {
      messageScrollFrame = null;
      syncMessageVirtualMetrics(true);
      updateMessageScrollState();
      rememberCurrentScroll();
      scheduleScrollSettleMeasure();
      void maybeLoadOlderHistory();
    });
  };

  const flushWorkflowLayoutChange = () => {
    workflowLayoutRefreshHandle = null;
    const messageKey = pendingWorkflowLayoutMessageKey;
    pendingWorkflowLayoutMessageKey = '';
    lastWorkflowLayoutRefreshAt = Date.now();
    logViewportDebug('workflow-layout-change', {
      messageKey: String(messageKey || '').trim()
    });
    scheduleMessageViewportRefresh({
      updateScrollState: true,
      // The tool list owns a fixed-height scroll area. Its output deltas do
      // not change the message-row height, so skip forced geometry reads.
      measure: false
    });
  };

  const handleMessageWorkflowLayoutChange = (messageKey?: string) => {
    const normalizedKey = String(messageKey || '').trim();
    if (normalizedKey) {
      pendingWorkflowLayoutMessageKey = normalizedKey;
    }
    if (typeof window === 'undefined') {
      flushWorkflowLayoutChange();
      return;
    }
    if (workflowLayoutRefreshHandle !== null) return;
    const delay = Math.max(0, WORKFLOW_LAYOUT_REFRESH_INTERVAL_MS - (Date.now() - lastWorkflowLayoutRefreshAt));
    workflowLayoutRefreshHandle = window.setTimeout(flushWorkflowLayoutChange, delay);
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
    rememberCurrentScroll();
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
    rememberCurrentScroll();
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
    rememberCurrentScroll();
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
      rememberCurrentScroll();
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
    if (typeof window !== 'undefined' && messageScrollSettleHandle !== null) {
      window.clearTimeout(messageScrollSettleHandle);
      messageScrollSettleHandle = null;
    }
    if (typeof window !== 'undefined' && messageViewportRefreshFrame !== null) {
      window.cancelAnimationFrame(messageViewportRefreshFrame);
      messageViewportRefreshFrame = null;
    }
    if (typeof window !== 'undefined' && messageDeferredMeasureHandle !== null) {
      if (messageDeferredMeasureUsesIdleCallback) {
        const cancelIdle = (window as Window & {
          cancelIdleCallback?: (handle: number) => void;
        }).cancelIdleCallback;
        if (typeof cancelIdle === 'function') {
          cancelIdle(messageDeferredMeasureHandle);
        }
      } else {
        window.clearTimeout(messageDeferredMeasureHandle);
      }
      messageDeferredMeasureHandle = null;
      messageDeferredMeasureUsesIdleCallback = false;
    }
    if (typeof window !== 'undefined' && messageResizeRefreshHandle !== null) {
      window.clearTimeout(messageResizeRefreshHandle);
      messageResizeRefreshHandle = null;
    }
    if (typeof window !== 'undefined' && workflowLayoutRefreshHandle !== null) {
      window.clearTimeout(workflowLayoutRefreshHandle);
      workflowLayoutRefreshHandle = null;
    }
    rememberCurrentScroll();
    releaseObservedMessageNodes();
    messageResizeObserver = null;
    scheduledViewportRefreshNeedsScrollState = false;
    scheduledViewportRefreshNeedsMeasure = false;
    scheduledViewportRefreshMeasureAll = false;
    scheduledViewportRefreshReason = '';
    scheduledViewportRefreshMeasureKeys.clear();
    scheduledVirtualMeasureAll = false;
    scheduledVirtualMeasureKeys.clear();
    pendingObservedResizeChanges.clear();
    pendingWorkflowLayoutMessageKey = '';
  };

  return {
    handleMessageListScroll,
    handleWorkflowLayoutChange: handleMessageWorkflowLayoutChange,
    scrollMessagesToBottom,
    jumpToMessageBottom,
    jumpToMessageTop,
    scrollVirtualMessageToIndex,
    scrollLatestAssistantToCenter,
    restoreConversationScroll,
    rememberCurrentScroll,
    rememberScrollForKey,
    scheduleMessageViewportRefresh,
    scheduleMessageVirtualMeasure,
    updateMessageScrollState,
    syncMessageVirtualMetrics,
    pruneMessageVirtualHeightCache,
    dispose
  };
};
