<template>
  <div
    v-if="usePlainTextRender"
    class="markdown-body message-markdown-body"
    :class="{
      'message-markdown-body--streaming-text': isStreamingTextPreview,
      'message-markdown-body--plain-text': !isStreamingTextPreview
    }"
  >{{ visiblePlainText }}</div>
  <div
    v-else
    class="markdown-body message-markdown-body"
    v-html="visibleHtml"
  ></div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, toRaw, watch } from 'vue';
import { renderMarkdown } from '@/utils/markdown';
import { t } from '@/i18n';
import { buildAssistantDisplayContent } from '@/utils/assistantFailureNotice';
import {
  selectChatRuntimeMessage,
  selectChatRuntimeSession,
  selectLatestAssistantForTurn
} from '@/realtime/chat/chatRuntimeSelectors';
import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeProjection
} from '@/realtime/chat/chatRuntimeTypes';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { chatPerf } from '@/utils/chatPerf';

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
  workspacePathContext: ''
});

const emit = defineEmits<{
  (event: 'rendered', payload: {
    cacheKey: string;
    streaming: boolean;
    contentLength: number;
    needsHydration?: boolean;
    lightweight?: boolean;
  }): void;
}>();

type RenderCacheEntry = {
  source: string;
  html: string;
  updatedAt: number;
};

const MARKDOWN_BODY_CACHE_LIMIT = 240;
const streamingMarkdownCache = new Map<string, RenderCacheEntry>();
const chatStore = useChatStore();

const visibleHtml = ref('');
const visiblePlainText = ref('');
let renderTimer: number | null = null;
let plainTextLayoutTimer: number | null = null;
let plainTextFlushTimer: number | null = null;
let pendingPlainText = '';
let pendingPlainTextScheduledAt = 0;
let lastPlainTextLayoutAt = 0;
let lastPlainTextFlushAt = 0;
const STREAM_RENDER_DEBUG_SLOW_MS = 48;
const MARKDOWN_RENDER_DEBUG_SLOW_MS = 12;
const STREAM_TEXT_FLUSH_MIN_MS = 32;
const PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS = 220;
const STREAMING_TEXT_PREVIEW_MAX_CHARS = 60000;
let lastStreamRenderTraceAt = 0;
let lastStreamRenderTraceSignature = '';

const runtimeProjectionVersion = computed(() => Number(chatStore.runtimeProjectionVersion || 0));
const runtimeContentVersion = computed(() => {
  const _projectionVersion = runtimeProjectionVersion.value;
  const messageIds = resolveRuntimeContentSubscriptionMessageIds();
  if (messageIds.length === 0) return 0;
  return messageIds.reduce(
    (sum, messageId) =>
      sum + Number(chatStore.runtimeProjectionContentVersionByMessage?.[messageId] || 0),
    0
  );
});

const resolveRuntimeContentSubscriptionMessageIds = (): string[] => {
  const sessionId = String(props.sessionId || chatStore.activeSessionId || '').trim();
  const projection = toRaw(chatStore.runtimeProjection);
  const ids = new Set<string>();
  const explicitMessageId = String(props.runtimeMessageId || '').trim();
  if (explicitMessageId) ids.add(explicitMessageId);
  if (!sessionId) return Array.from(ids);

  const turnMessage = resolveRuntimeProjectedMessageByTurn(projection, sessionId);
  if (turnMessage?.id) ids.add(turnMessage.id);
  return Array.from(ids);
};

const resolveMessageText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = String(value ?? '').trim();
    if (text) return text;
  }
  return '';
};

const resolveRuntimeProjectedMessageByTurn = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: string
): ChatRuntimeMessageProjection | null => {
  const message = (props.message || {}) as MessageRecord;
  if (String(message.role || '').trim() !== 'assistant') {
    return null;
  }
  const modelTurnId = resolveMessageText(
    props.runtimeModelTurnId,
    message.__runtime_model_turn_id,
    message.model_turn_id,
    message.modelTurnId
  );
  if (modelTurnId) {
    const session = selectChatRuntimeSession(projection, sessionId);
    const modelTurn = session?.modelTurnById?.[modelTurnId];
    if (modelTurn) {
      for (let index = modelTurn.messageIds.length - 1; index >= 0; index -= 1) {
        const candidate = session.messageById[modelTurn.messageIds[index]];
        if (candidate?.role === 'assistant') return candidate;
      }
    }
  }

  const userTurnId = resolveMessageText(
    props.runtimeUserTurnId,
    message.__runtime_user_turn_id,
    message.user_turn_id,
    message.userTurnId
  );
  return userTurnId
    ? selectLatestAssistantForTurn(projection, sessionId, userTurnId)
    : null;
};

const resolveRuntimeProjectedMessage = () => {
  const sessionId = String(props.sessionId || chatStore.activeSessionId || '').trim();
  if (!sessionId) return null;
  const projection = toRaw(chatStore.runtimeProjection);
  const messageId = String(props.runtimeMessageId || '').trim();
  const explicitMessage = messageId
    ? selectChatRuntimeMessage(projection, sessionId, messageId)
    : null;
  const turnMessage = resolveRuntimeProjectedMessageByTurn(projection, sessionId);
  return turnMessage || explicitMessage;
};
const displayMessage = computed<MessageRecord>(() => {
  const _projectionVersion = runtimeProjectionVersion.value;
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
  const _projectionVersion = runtimeProjectionVersion.value;
  const _contentVersion = runtimeContentVersion.value;
  const projected = resolveRuntimeProjectedMessage();
  return props.assistantDisplay
    ? buildAssistantDisplayContent(displayMessage.value, t)
    : String(projected?.content ?? props.content ?? '');
});
const normalizedCacheKey = computed(() => String(props.cacheKey || '').trim());
const shouldThrottle = computed(() => props.streaming === true && Number(props.throttleMs || 0) > 0);
const workspacePathResolver = computed(() => {
  if (typeof props.resolveWorkspacePath !== 'function') return undefined;
  const context = String(props.workspacePathContext || '').trim();
  return (rawPath: string) => props.resolveWorkspacePath?.(rawPath, context) || '';
});

const trimStreamingMarkdownCache = () => {
  while (streamingMarkdownCache.size > MARKDOWN_BODY_CACHE_LIMIT) {
    const oldestKey = streamingMarkdownCache.keys().next().value;
    if (!oldestKey) break;
    streamingMarkdownCache.delete(oldestKey);
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

const flushPendingPlainText = () => {
  const source = pendingPlainText;
  const scheduledAt = pendingPlainTextScheduledAt;
  plainTextFlushTimer = null;
  pendingPlainText = '';
  pendingPlainTextScheduledAt = 0;
  visiblePlainText.value = source;
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
    visiblePlainText.value = source;
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
  const source = normalizedContent.value;
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

const renderNow = () => {
  if (renderTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(renderTimer);
    renderTimer = null;
  }
  if (!props.streaming) {
    clearPlainTextLayoutTimer();
  }
  const source = normalizedContent.value;
  const cacheKey = normalizedCacheKey.value;
  const plainTextRender = usePlainTextRender.value;
  const streamingTextPreview = isStreamingTextPreview.value;
  traceStreamingRenderSource(source, plainTextRender);
  if (plainTextRender) {
    updateVisiblePlainText(source, !streamingTextPreview);
  } else {
    clearPlainTextFlushTimer();
    visiblePlainText.value = '';
  }
  if (!source) {
    updateVisiblePlainText('', true);
    visibleHtml.value = '';
    if (cacheKey) streamingMarkdownCache.delete(cacheKey);
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
  const cached = cacheKey ? streamingMarkdownCache.get(cacheKey) : null;
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
    streamingMarkdownCache.set(cacheKey, {
      source,
      html,
      updatedAt: Date.now()
    });
    trimStreamingMarkdownCache();
  }
  emit('rendered', buildRenderedPayload(source, html));
};

const traceStreamingRenderSource = (source: string, plainStreaming: boolean) => {
  if (!props.streaming || !source || typeof window === 'undefined') return;
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
  const source = normalizedContent.value;
  const plainTextRender = usePlainTextRender.value;
  const streamingTextPreview = isStreamingTextPreview.value;
  traceStreamingRenderSource(source, plainTextRender);
  if (plainTextRender) {
    updateVisiblePlainText(source, !streamingTextPreview);
  } else {
    clearPlainTextFlushTimer();
    visiblePlainText.value = '';
  }
  if (!shouldThrottle.value || typeof window === 'undefined') {
    renderNow();
    return;
  }
  const cacheKey = normalizedCacheKey.value;
  const cached = cacheKey ? streamingMarkdownCache.get(cacheKey) : null;
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
    normalizedContent.value,
    props.streaming,
    props.throttleMs,
    props.resolveWorkspacePath,
    props.workspacePathContext,
    props.assistantDisplay,
    runtimeProjectionVersion.value,
    runtimeContentVersion.value
  ],
  () => scheduleRender(),
  { immediate: true }
);

onBeforeUnmount(() => {
  if (renderTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(renderTimer);
    renderTimer = null;
  }
  clearPlainTextLayoutTimer();
  clearPlainTextFlushTimer();
});
</script>
