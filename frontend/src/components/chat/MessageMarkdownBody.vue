<template>
  <div
    v-if="usePlainTextRender"
    ref="plainTextRef"
    class="markdown-body message-markdown-body"
    :class="{
      'message-markdown-body--streaming-text': isStreamingTextPreview,
      'message-markdown-body--plain-text': !isStreamingTextPreview
    }"
  ></div>
  <div
    v-else
    class="markdown-body message-markdown-body"
    v-html="visibleHtml"
  ></div>
  <button
    v-if="isContentTruncated"
    class="message-markdown-body-expand"
    type="button"
    :disabled="detailLoading"
    @click="expandLongContent"
  >
    {{ t('common.expand') }}
  </button>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { renderMarkdown } from '@/utils/markdown';
import { t } from '@/i18n';
import { buildAssistantDisplayContent } from '@/utils/assistantFailureNotice';
import {
  resolveRuntimeMessageContentSource,
  resolveRuntimeMessageContentSubscriptionIds
} from './messageRuntimeContent';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import { chatPerf } from '@/utils/chatPerf';
import { getSessionHistoryMessage } from '@/api/chat';

type MessageRecord = Record<string, unknown>;

const props = withDefaults(defineProps<{
  cacheKey: string;
  content: string;
  message?: MessageRecord | null;
  runtimeMessageId?: string;
  runtimeUserTurnId?: string;
  runtimeModelTurnId?: string;
  sessionId?: string;
  assistantDisplay?: boolean;
  streaming?: boolean;
  throttleMs?: number;
  resolveWorkspacePath?: (rawPath: string, context?: string) => string;
  workspacePathContext?: string;
  historyId?: number | string;
  contentTruncated?: boolean;
}>(), {
  message: null,
  runtimeMessageId: '',
  runtimeUserTurnId: '',
  runtimeModelTurnId: '',
  sessionId: '',
  assistantDisplay: false,
  streaming: false,
  throttleMs: 120,
  resolveWorkspacePath: undefined,
  workspacePathContext: '',
  historyId: '',
  contentTruncated: false
});

const emit = defineEmits<{
  (event: 'rendered', payload: {
    cacheKey: string;
    streaming: boolean;
    contentLength: number;
    needsHydration?: boolean;
    lightweight?: boolean;
  }): void;
  (event: 'history-message-hydrated', detail: {
    content: string;
    reasoning?: string;
    attachments?: unknown;
    questionPanel?: unknown;
    feedback?: unknown;
    workflowItems?: unknown;
    subagents?: unknown;
  }): void;
}>();

type RenderCacheEntry = {
  source: string;
  html: string;
  updatedAt: number;
  bytes: number;
};

type HydratedHistoryContent = {
  content: string;
  bytes: number;
};

const MARKDOWN_BODY_CACHE_LIMIT = 240;
const MARKDOWN_BODY_CACHE_MAX_BYTES = 12 * 1024 * 1024;
const HYDRATED_HISTORY_CONTENT_CACHE_LIMIT = 64;
const HYDRATED_HISTORY_CONTENT_CACHE_MAX_BYTES = 8 * 1024 * 1024;
const streamingMarkdownCache = new Map<string, RenderCacheEntry>();
const hydratedHistoryContentCache = new Map<string, HydratedHistoryContent>();
let streamingMarkdownCacheBytes = 0;
let hydratedHistoryContentCacheBytes = 0;
const chatStore = useChatStore();

const visibleHtml = ref('');
const visiblePlainText = ref('');
const plainTextRef = ref<HTMLElement | null>(null);
const expandedLongContent = ref(false);
const hydratedContent = ref<string | null>(null);
const detailLoading = ref(false);
let renderTimer: number | null = null;
let plainTextLayoutTimer: number | null = null;
let plainTextFlushTimer: number | null = null;
let livePlainTextPollTimer: number | null = null;
let plainTextDomSyncPending = false;
let pendingPlainText = '';
let pendingPlainTextScheduledAt = 0;
let lastPlainTextLayoutAt = 0;
let lastPlainTextFlushAt = 0;
let historyDetailAbortController: AbortController | null = null;
const STREAM_RENDER_DEBUG_SLOW_MS = 48;
const MARKDOWN_RENDER_DEBUG_SLOW_MS = 12;
const STREAM_TEXT_FLUSH_MIN_MS = 32;
const LIVE_STREAM_TEXT_POLL_MS = 120;
const PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS = 220;
const STREAMING_TEXT_PREVIEW_MAX_CHARS = 60000;
const HISTORY_MARKDOWN_INITIAL_CHARS = 24000;
let lastStreamRenderTraceAt = 0;
let lastStreamRenderTraceSignature = '';
let lastPlainTextSource = '';

const runtimeContentVersion = computed(() => {
  const messageIds = resolveRuntimeMessageContentSubscriptionIds({
    // Keep this lookup reactive at the exact message path. Rendering the
    // transcript stays isolated, while a final event can still replace the
    // last streamed preview without requiring a full projection repaint.
    projection: chatStore.runtimeProjection,
    sessionId: String(props.sessionId || chatStore.activeSessionId || '').trim(),
    runtimeMessageId: props.runtimeMessageId,
    runtimeUserTurnId: props.runtimeUserTurnId,
    runtimeModelTurnId: props.runtimeModelTurnId,
    message: (props.message || {}) as MessageRecord
  });
  const messageScopedVersion = messageIds.reduce(
    (sum, messageId) =>
      sum + Number(chatStore.runtimeProjectionContentVersionByMessage?.[messageId] || 0),
    0
  );
  return messageScopedVersion;
});

const resolveRuntimeProjectedMessage = () => {
  const sessionId = String(props.sessionId || chatStore.activeSessionId || '').trim();
  if (!sessionId) return null;
  return resolveRuntimeMessageContentSource({
    projection: chatStore.runtimeProjection,
    sessionId,
    runtimeMessageId: props.runtimeMessageId,
    runtimeUserTurnId: props.runtimeUserTurnId,
    runtimeModelTurnId: props.runtimeModelTurnId,
    message: (props.message || {}) as MessageRecord
  });
};
const displayMessage = computed<MessageRecord>(() => {
  const _contentVersion = runtimeContentVersion.value;
  const projected = resolveRuntimeProjectedMessage();
  if (!projected) {
    return (props.message || {}) as MessageRecord;
  }
  const base = {
    ...((props.message || {}) as MessageRecord),
    role: projected.role,
    content: projected.content,
    reasoning: projected.reasoning,
    runtime_status: projected.status,
    stream_incomplete:
      projected.status === 'placeholder' ||
      projected.status === 'waiting_first_output' ||
      projected.status === 'streaming' ||
      projected.status === 'tooling'
  };
  return base;
});
const normalizedContent = computed(() => {
  const _contentVersion = runtimeContentVersion.value;
  const projected = resolveRuntimeProjectedMessage();
  const source = props.assistantDisplay
    ? buildAssistantDisplayContent(displayMessage.value, t)
    : String(projected?.content ?? props.content ?? '');
  return hydratedContent.value ?? source;
});
const normalizedCacheKey = computed(() => String(props.cacheKey || '').trim());
const isContentTruncated = computed(() =>
  props.streaming !== true &&
  !expandedLongContent.value &&
  hydratedContent.value === null &&
  (props.contentTruncated === true || normalizedContent.value.length > HISTORY_MARKDOWN_INITIAL_CHARS)
);
const renderContent = computed(() => {
  const source = normalizedContent.value;
  if (!isContentTruncated.value) return source;
  const limit = Math.min(HISTORY_MARKDOWN_INITIAL_CHARS, source.length);
  const breakAt = source.lastIndexOf('\n', limit);
  return source.slice(0, breakAt > limit / 2 ? breakAt : limit);
});
const shouldThrottle = computed(() => props.streaming === true && Number(props.throttleMs || 0) > 0);
const workspacePathResolver = computed(() => {
  if (typeof props.resolveWorkspacePath !== 'function') return undefined;
  const context = String(props.workspacePathContext || '').trim();
  return (rawPath: string) => props.resolveWorkspacePath?.(rawPath, context) || '';
});

const trimStreamingMarkdownCache = () => {
  while (
    streamingMarkdownCache.size > MARKDOWN_BODY_CACHE_LIMIT ||
    streamingMarkdownCacheBytes > MARKDOWN_BODY_CACHE_MAX_BYTES
  ) {
    const oldestKey = streamingMarkdownCache.keys().next().value as string | undefined;
    if (!oldestKey) break;
    const oldest = streamingMarkdownCache.get(oldestKey);
    if (oldest) streamingMarkdownCacheBytes -= oldest.bytes;
    streamingMarkdownCache.delete(oldestKey);
  }
};

const deleteMarkdownCacheEntry = (key: string) => {
  const cached = streamingMarkdownCache.get(key);
  if (cached) streamingMarkdownCacheBytes -= cached.bytes;
  streamingMarkdownCache.delete(key);
};

const readMarkdownCacheEntry = (key: string): RenderCacheEntry | null => {
  const cached = streamingMarkdownCache.get(key);
  if (!cached) return null;
  // Refresh the LRU order without duplicating the stored HTML string.
  streamingMarkdownCache.delete(key);
  streamingMarkdownCache.set(key, cached);
  return cached;
};

const writeMarkdownCacheEntry = (key: string, source: string, html: string) => {
  deleteMarkdownCacheEntry(key);
  streamingMarkdownCache.set(key, {
    source,
    html,
    updatedAt: Date.now(),
    bytes: source.length * 2 + html.length * 2
  });
  streamingMarkdownCacheBytes += source.length * 2 + html.length * 2;
  trimStreamingMarkdownCache();
};

const readHydratedHistoryContent = (key: string): string | null => {
  const cached = hydratedHistoryContentCache.get(key);
  if (!cached) return null;
  hydratedHistoryContentCache.delete(key);
  hydratedHistoryContentCache.set(key, cached);
  return cached.content;
};

const writeHydratedHistoryContent = (key: string, content: string) => {
  const previous = hydratedHistoryContentCache.get(key);
  if (previous) hydratedHistoryContentCacheBytes -= previous.bytes;
  const entry = { content, bytes: content.length * 2 };
  hydratedHistoryContentCache.set(key, entry);
  hydratedHistoryContentCacheBytes += entry.bytes;
  while (
    hydratedHistoryContentCache.size > HYDRATED_HISTORY_CONTENT_CACHE_LIMIT ||
    hydratedHistoryContentCacheBytes > HYDRATED_HISTORY_CONTENT_CACHE_MAX_BYTES
  ) {
    const oldestKey = hydratedHistoryContentCache.keys().next().value as string | undefined;
    if (!oldestKey) break;
    const oldest = hydratedHistoryContentCache.get(oldestKey);
    if (oldest) hydratedHistoryContentCacheBytes -= oldest.bytes;
    hydratedHistoryContentCache.delete(oldestKey);
  }
};

const looksLikeSimplePlainText = (source: string): boolean => {
  if (!source) return false;
  if (source.includes('```') || source.includes('~~~')) return false;
  if (source.includes('|') && /\n\s*\|?[\s:-]+\|/.test(source)) return false;
  if (/!\[[^\]]*]\(|\[[^\]]+]\(|<https?:\/\//i.test(source)) return false;
  if (/^\s{0,3}(#{1,6}\s|[-*+]\s|\d+\.\s|>\s)/m.test(source)) return false;
  if (/(\*|_|~~|`|\$\$|\\\(|\\\[)/.test(source)) return false;
  return source.length < 12000;
};

const isStreamingTextPreview = computed(() =>
  props.streaming === true &&
  normalizedContent.value.length > 0 &&
  normalizedContent.value.length <= STREAMING_TEXT_PREVIEW_MAX_CHARS
);
const usePlainTextRender = computed(() =>
  props.streaming === true
    ? isStreamingTextPreview.value
    : looksLikeSimplePlainText(normalizedContent.value)
);

const clearPlainTextLayoutTimer = () => {
  if (plainTextLayoutTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(plainTextLayoutTimer);
    plainTextLayoutTimer = null;
  }
};

const clearPlainTextFlushTimer = () => {
  if (plainTextFlushTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(plainTextFlushTimer);
  }
  plainTextFlushTimer = null;
  pendingPlainText = '';
  pendingPlainTextScheduledAt = 0;
};

const syncPlainTextDom = (source: string) => {
  const el = plainTextRef.value;
  if (el) {
    if (el.textContent === lastPlainTextSource && source.startsWith(lastPlainTextSource)) {
      const delta = source.slice(lastPlainTextSource.length);
      if (delta) {
        el.append(document.createTextNode(delta));
      }
    } else if (el.textContent !== source) {
      el.textContent = source;
    }
    lastPlainTextSource = source;
    return;
  }
  if (plainTextDomSyncPending) return;
  plainTextDomSyncPending = true;
  void nextTick(() => {
    plainTextDomSyncPending = false;
    const target = plainTextRef.value;
    if (target && target.textContent !== visiblePlainText.value) {
      target.textContent = visiblePlainText.value;
    }
    lastPlainTextSource = visiblePlainText.value;
  });
};

const setVisiblePlainText = (source: string) => {
  visiblePlainText.value = source;
  syncPlainTextDom(source);
};

const flushPendingPlainText = () => {
  const source = pendingPlainText;
  const scheduledAt = pendingPlainTextScheduledAt;
  plainTextFlushTimer = null;
  pendingPlainText = '';
  pendingPlainTextScheduledAt = 0;
  setVisiblePlainText(source);
  lastPlainTextFlushAt = Date.now();
  const latencyMs = scheduledAt > 0 ? lastPlainTextFlushAt - scheduledAt : 0;
  if (latencyMs >= STREAM_RENDER_DEBUG_SLOW_MS) {
    const payload = {
      latencyMs,
      contentLength: source.length,
      cacheKey: normalizedCacheKey.value,
      runtimeMessageId: props.runtimeMessageId || ''
    };
    chatDebugLog('chat.stream.perf', 'plain-text-slow-flush', payload);
    chatPerf.recordDuration('chat_stream_plain_text_slow_flush', latencyMs, payload);
  } else if (chatPerf.enabled()) {
    chatPerf.recordDuration('chat_stream_plain_text_flush', latencyMs, {
      contentLength: source.length
    });
  }
};

const updateVisiblePlainText = (source: string, immediate = false) => {
  if (immediate || !props.streaming || typeof window === 'undefined') {
    clearPlainTextFlushTimer();
    setVisiblePlainText(source);
    lastPlainTextFlushAt = Date.now();
    return;
  }
  pendingPlainText = source;
  if (!pendingPlainTextScheduledAt) {
    pendingPlainTextScheduledAt = Date.now();
  }
  if (plainTextFlushTimer !== null) return;
  const elapsedMs = Date.now() - lastPlainTextFlushAt;
  const waitMs = Math.max(0, STREAM_TEXT_FLUSH_MIN_MS - elapsedMs);
  plainTextFlushTimer = window.setTimeout(flushPendingPlainText, waitMs);
};

const buildRenderedPayload = (
  source: string,
  html = '',
  options: { lightweight?: boolean } = {}
): { cacheKey: string; streaming: boolean; contentLength: number; needsHydration?: boolean; lightweight?: boolean } => ({
  cacheKey: normalizedCacheKey.value,
  streaming: props.streaming,
  contentLength: source.length,
  ...(options.lightweight === true ? { lightweight: true } : {}),
  ...(html.includes('ai-resource-card') || html.includes('ai-external-image-card')
    ? { needsHydration: true }
    : {})
});

const emitPlainTextLayout = (lightweight: boolean) => {
  const source = renderContent.value;
  emit('rendered', {
    cacheKey: normalizedCacheKey.value,
    streaming: props.streaming,
    contentLength: source.length,
    lightweight
  });
};

const schedulePlainTextLayout = () => {
  if (typeof window === 'undefined') {
    emitPlainTextLayout(false);
    return;
  }
  const now = Date.now();
  const throttleMs = props.streaming
    ? Math.max(Number(props.throttleMs || 0), PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS)
    : Number(props.throttleMs || 0);
  const waitMs = Math.max(0, throttleMs - (now - lastPlainTextLayoutAt));
  if (waitMs <= 0) {
    lastPlainTextLayoutAt = now;
    emitPlainTextLayout(props.streaming);
    return;
  }
  if (plainTextLayoutTimer !== null) return;
  plainTextLayoutTimer = window.setTimeout(() => {
    plainTextLayoutTimer = null;
    lastPlainTextLayoutAt = Date.now();
    emitPlainTextLayout(props.streaming);
  }, waitMs);
};

const resolveLiveRuntimeContent = (): string => {
  const projected = resolveRuntimeProjectedMessage();
  if (!projected) return normalizedContent.value;
  if (!props.assistantDisplay) {
    return String(projected.content || props.content || '');
  }
  return buildAssistantDisplayContent({
    ...((props.message || {}) as MessageRecord),
    role: projected.role,
    content: projected.content,
    reasoning: projected.reasoning,
    runtime_status: projected.status,
    stream_incomplete: true
  }, t);
};

const syncLiveRuntimePlainText = () => {
  if (props.streaming !== true || typeof window === 'undefined') return;
  const source = resolveLiveRuntimeContent();
  if (!source || source.length > STREAMING_TEXT_PREVIEW_MAX_CHARS) return;
  if (source === visiblePlainText.value || source === pendingPlainText) return;
  visibleHtml.value = '';
  updateVisiblePlainText(source, false);
  schedulePlainTextLayout();
  traceStreamingRenderSource(source, true);
};

const startLivePlainTextPoll = () => {
  if (livePlainTextPollTimer !== null || typeof window === 'undefined') return;
  livePlainTextPollTimer = window.setInterval(syncLiveRuntimePlainText, LIVE_STREAM_TEXT_POLL_MS);
};

const stopLivePlainTextPoll = () => {
  if (livePlainTextPollTimer !== null && typeof window !== 'undefined') {
    window.clearInterval(livePlainTextPollTimer);
  }
  livePlainTextPollTimer = null;
};

const renderNow = () => {
  if (renderTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(renderTimer);
    renderTimer = null;
  }
  if (!props.streaming) {
    clearPlainTextLayoutTimer();
  }
  const source = renderContent.value;
  const cacheKey = normalizedCacheKey.value;
  const plainTextRender = usePlainTextRender.value;
  const streamingTextPreview = isStreamingTextPreview.value;
  traceStreamingRenderSource(source, plainTextRender);
  if (plainTextRender) {
    updateVisiblePlainText(source, !streamingTextPreview);
  } else {
    clearPlainTextFlushTimer();
    setVisiblePlainText('');
  }
  if (!source) {
    updateVisiblePlainText('', true);
    visibleHtml.value = '';
    if (cacheKey) deleteMarkdownCacheEntry(cacheKey);
    emit('rendered', buildRenderedPayload(source));
    return;
  }
  if (plainTextRender) {
    if (streamingTextPreview) {
      schedulePlainTextLayout();
    } else {
      emit('rendered', buildRenderedPayload(source));
    }
    return;
  }
  clearPlainTextLayoutTimer();
  const cached = cacheKey ? readMarkdownCacheEntry(cacheKey) : null;
  if (cached?.source === source) {
    visibleHtml.value = cached.html;
    emit('rendered', buildRenderedPayload(source, cached.html));
    return;
  }
  const renderStartedAt = Date.now();
  const html = renderMarkdown(source, { resolveWorkspacePath: workspacePathResolver.value });
  const renderMs = Date.now() - renderStartedAt;
  if (renderMs >= MARKDOWN_RENDER_DEBUG_SLOW_MS) {
    const payload = {
      renderMs,
      contentLength: source.length,
      cacheKey,
      runtimeMessageId: props.runtimeMessageId || '',
      streaming: props.streaming
    };
    chatDebugLog('chat.stream.perf', 'markdown-slow-render', payload);
    chatPerf.recordDuration('chat_stream_markdown_slow_render', renderMs, payload);
  } else if (chatPerf.enabled()) {
    chatPerf.recordDuration('chat_stream_markdown_render', renderMs, {
      contentLength: source.length,
      streaming: props.streaming
    });
  }
  visibleHtml.value = html;
  if (cacheKey) {
    writeMarkdownCacheEntry(cacheKey, source, html);
  }
  emit('rendered', buildRenderedPayload(source, html));
};

const traceStreamingRenderSource = (source: string, plainStreaming: boolean) => {
  if (!isChatDebugEnabled() || !props.streaming || !source || typeof window === 'undefined') return;
  const now = Date.now();
  const runtimeMessage = resolveRuntimeProjectedMessage();
  const signature = [
    runtimeMessage?.id || props.runtimeMessageId || '',
    runtimeMessage?.userTurnId || props.runtimeUserTurnId || '',
    runtimeMessage?.modelTurnId || props.runtimeModelTurnId || '',
    source.length,
    runtimeContentVersion.value
  ].join('|');
  if (signature === lastStreamRenderTraceSignature) return;
  if (now - lastStreamRenderTraceAt < 500 && source.length % 80 !== 0) return;
  lastStreamRenderTraceAt = now;
  lastStreamRenderTraceSignature = signature;
  chatDebugLog('chat.stream.perf', 'message-body-stream-render', {
    cacheKey: normalizedCacheKey.value,
    runtimeMessageId: runtimeMessage?.id || props.runtimeMessageId || '',
    runtimeUserTurnId: runtimeMessage?.userTurnId || props.runtimeUserTurnId || '',
    runtimeModelTurnId: runtimeMessage?.modelTurnId || props.runtimeModelTurnId || '',
    contentLength: source.length,
    contentVersion: runtimeContentVersion.value,
    plainStreaming
  });
};

const scheduleRender = () => {
  const source = renderContent.value;
  const plainTextRender = usePlainTextRender.value;
  const streamingTextPreview = isStreamingTextPreview.value;
  traceStreamingRenderSource(source, plainTextRender);
  if (plainTextRender) {
    updateVisiblePlainText(source, !streamingTextPreview);
  } else {
    clearPlainTextFlushTimer();
    setVisiblePlainText('');
  }
  if (!shouldThrottle.value || typeof window === 'undefined') {
    renderNow();
    return;
  }
  const cacheKey = normalizedCacheKey.value;
  const cached = cacheKey ? readMarkdownCacheEntry(cacheKey) : null;
  const now = Date.now();
  if (cached?.source === source) {
    visibleHtml.value = cached.html;
    return;
  }
  if (plainTextRender) {
    if (streamingTextPreview) {
      schedulePlainTextLayout();
    } else {
      emit('rendered', buildRenderedPayload(source));
    }
    return;
  }
  clearPlainTextLayoutTimer();
  const waitMs = Math.max(0, Number(props.throttleMs || 0) - (cached ? now - cached.updatedAt : Number.POSITIVE_INFINITY));
  if (waitMs <= 0) {
    renderNow();
    return;
  }
  if (renderTimer !== null) return;
  renderTimer = window.setTimeout(() => {
    renderTimer = null;
    renderNow();
  }, waitMs);
};

watch(
  () => [
    normalizedCacheKey.value,
    renderContent.value,
    props.streaming,
    props.throttleMs,
    props.resolveWorkspacePath,
    props.workspacePathContext,
    props.assistantDisplay,
    runtimeContentVersion.value
  ],
  () => scheduleRender(),
  { immediate: true }
);

watch(normalizedCacheKey, () => {
  expandedLongContent.value = false;
  hydratedContent.value = readHydratedHistoryContent(normalizedCacheKey.value);
}, { immediate: true });

const expandLongContent = async () => {
  if (detailLoading.value) return;
  const sessionId = String(props.sessionId || '').trim();
  const historyId = String(props.historyId || '').trim();
  if (!sessionId || !historyId || props.contentTruncated !== true) {
    expandedLongContent.value = true;
    return;
  }
  detailLoading.value = true;
  historyDetailAbortController?.abort();
  const controller = new AbortController();
  historyDetailAbortController = controller;
  try {
    const response = await getSessionHistoryMessage(sessionId, historyId, { signal: controller.signal });
    const message = response?.data?.data?.message as MessageRecord | undefined;
    if (message && typeof message.content === 'string') {
      hydratedContent.value = message.content;
      writeHydratedHistoryContent(normalizedCacheKey.value, message.content);
      emit('history-message-hydrated', {
        content: message.content,
        ...(typeof message.reasoning === 'string' ? { reasoning: message.reasoning } : {}),
        ...(message.attachments !== undefined ? { attachments: message.attachments } : {}),
        ...(message.questionPanel !== undefined ? { questionPanel: message.questionPanel } : {}),
        ...(message.feedback !== undefined ? { feedback: message.feedback } : {}),
        ...(message.workflowItems !== undefined ? { workflowItems: message.workflowItems } : {}),
        ...(message.subagents !== undefined ? { subagents: message.subagents } : {})
      });
    }
    expandedLongContent.value = true;
  } finally {
    if (historyDetailAbortController === controller) {
      historyDetailAbortController = null;
      detailLoading.value = false;
    }
  }
};

watch(
  () => [
    props.streaming,
    props.sessionId,
    props.runtimeMessageId,
    props.runtimeUserTurnId,
    props.runtimeModelTurnId
  ],
  () => {
    if (props.streaming === true) {
      startLivePlainTextPoll();
      syncLiveRuntimePlainText();
      return;
    }
    stopLivePlainTextPoll();
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  if (renderTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(renderTimer);
    renderTimer = null;
  }
  clearPlainTextLayoutTimer();
  clearPlainTextFlushTimer();
  stopLivePlainTextPoll();
  historyDetailAbortController?.abort();
  historyDetailAbortController = null;
});
</script>
