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
import { useMessageMarkdownCache } from './messageMarkdownCache';
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

const chatStore = useChatStore();
const { readMarkdownCacheEntry, writeMarkdownCacheEntry, deleteMarkdownCacheEntry,
  readHydratedHistoryContent, writeHydratedHistoryContent } = useMessageMarkdownCache(chatStore.runtimeProjection);

const visibleHtml = ref('');
const visiblePlainText = ref('');
const plainTextRef = ref<HTMLElement | null>(null);
const expandedLongContent = ref(false);
const hydratedContent = ref<string | null>(null);
const detailLoading = ref(false);
let renderTimer: number | null = null;
let plainTextLayoutTimer: number | null = null;
let plainTextFlushTimer: number | null = null;
let plainTextDomSyncPending = false;
let pendingPlainText = '';
let pendingPlainTextScheduledAt = 0;
let lastPlainTextLayoutAt = 0;
let lastPlainTextFlushAt = 0;
let disposed = false;
let historyDetailAbortController: AbortController | null = null;
const STREAM_RENDER_DEBUG_SLOW_MS = 48;
const MARKDOWN_RENDER_DEBUG_SLOW_MS = 12;
const STREAM_TEXT_FLUSH_MIN_MS = 32;
const PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS = 220;
const HISTORY_MARKDOWN_INITIAL_CHARS = 24000;
let lastStreamRenderTraceAt = 0;
let lastStreamRenderTraceSignature = '';
let lastPlainTextSource = '';

const runtimeContentVersion = computed(() => {
  const structureVersion = chatStore.runtimeProjectionVersion;
  const messageIds = resolveRuntimeMessageContentSubscriptionIds({
    // Raw projection changes are published through explicit clocks: per-row
    // clocks drive deltas, while the structural clock resolves replaced ids.
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
  return `${structureVersion}:${messageScopedVersion}`;
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
const normalizedCacheKey = computed(() => [
  String(props.sessionId || chatStore.activeSessionId || ''), props.workspacePathContext,
  t('common.expand'), String(props.cacheKey || '').trim()
].join('::'));
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
  normalizedContent.value.length > 0
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
        const textNode = el.firstChild;
        if (textNode?.nodeType === Node.TEXT_NODE) (textNode as Text).appendData(delta);
        else el.textContent = source;
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
    if (disposed) return;
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
      cacheKey: props.cacheKey,
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
  cacheKey: props.cacheKey,
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
    cacheKey: props.cacheKey,
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
    cacheKey: props.cacheKey,
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
  const cacheKey = normalizedCacheKey.value;
  try {
    const response = await getSessionHistoryMessage(sessionId, historyId, { signal: controller.signal });
    if (disposed || controller.signal.aborted || cacheKey !== normalizedCacheKey.value) return;
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
      if (!disposed) detailLoading.value = false;
    }
  }
};

onBeforeUnmount(() => {
  disposed = true;
  if (renderTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(renderTimer);
    renderTimer = null;
  }
  clearPlainTextLayoutTimer();
  clearPlainTextFlushTimer();
  historyDetailAbortController?.abort();
  historyDetailAbortController = null;
});
</script>
