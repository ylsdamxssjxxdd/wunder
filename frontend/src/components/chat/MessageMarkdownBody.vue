<template>
  <div
    v-if="usePlainStreamingText"
    class="markdown-body message-markdown-body message-markdown-body--streaming-text"
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
import { selectChatRuntimeMessage } from '@/realtime/chat/chatRuntimeSelectors';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog } from '@/utils/chatDebug';
import { chatPerf } from '@/utils/chatPerf';

type MessageRecord = Record<string, unknown>;

const props = withDefaults(defineProps<{
  cacheKey: string;
  content: string;
  message?: MessageRecord | null;
  runtimeMessageId?: string;
  sessionId?: string;
  assistantDisplay?: boolean;
  streaming?: boolean;
  throttleMs?: number;
  resolveWorkspacePath?: (rawPath: string, context?: string) => string;
  workspacePathContext?: string;
}>(), {
  message: null,
  runtimeMessageId: '',
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
let plainTextFrame: number | null = null;
let plainTextFrameFallbackTimer: number | null = null;
let pendingVisiblePlainText = '';
let pendingVisiblePlainTextScheduledAt = 0;
let lastPlainTextLayoutAt = 0;
const PLAIN_TEXT_VISIBLE_FALLBACK_MS = 16;
const STREAM_RENDER_DEBUG_SLOW_MS = 48;
const MARKDOWN_RENDER_DEBUG_SLOW_MS = 12;
const PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS = 220;

const runtimeContentVersion = computed(() => {
  const messageId = String(props.runtimeMessageId || '').trim();
  if (!messageId) return 0;
  return Number(chatStore.runtimeProjectionContentVersionByMessage?.[messageId] || 0);
});
const runtimeProjectedMessage = computed(() => {
  const _contentVersion = runtimeContentVersion.value;
  const sessionId = String(props.sessionId || chatStore.activeSessionId || '').trim();
  const messageId = String(props.runtimeMessageId || '').trim();
  if (!sessionId || !messageId) return null;
  return selectChatRuntimeMessage(toRaw(chatStore.runtimeProjection), sessionId, messageId);
});
const resolveRuntimeProjectedMessage = () => {
  const sessionId = String(props.sessionId || chatStore.activeSessionId || '').trim();
  const messageId = String(props.runtimeMessageId || '').trim();
  if (!sessionId || !messageId) return null;
  return selectChatRuntimeMessage(toRaw(chatStore.runtimeProjection), sessionId, messageId);
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

const looksLikePlainStreamingText = (source: string): boolean => {
  if (!source) return false;
  if (source.includes('```') || source.includes('~~~')) return false;
  if (source.includes('|') && /\n\s*\|?[\s:-]+\|/.test(source)) return false;
  if (/!\[[^\]]*]\(|\[[^\]]+]\(|<https?:\/\//i.test(source)) return false;
  if (/^\s{0,3}(#{1,6}\s|[-*+]\s|\d+\.\s|>\s)/m.test(source)) return false;
  if (/(\*|_|~~|`|\$\$|\\\(|\\\[)/.test(source)) return false;
  return source.length < 12000;
};

const usePlainStreamingText = computed(() => looksLikePlainStreamingText(normalizedContent.value));

const clearPlainTextLayoutTimer = () => {
  if (plainTextLayoutTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(plainTextLayoutTimer);
    plainTextLayoutTimer = null;
  }
};

const clearPlainTextFrame = () => {
  if (plainTextFrame !== null && typeof window !== 'undefined') {
    window.cancelAnimationFrame(plainTextFrame);
    plainTextFrame = null;
  }
  if (plainTextFrameFallbackTimer !== null && typeof window !== 'undefined') {
    window.clearTimeout(plainTextFrameFallbackTimer);
    plainTextFrameFallbackTimer = null;
  }
};

const updateVisiblePlainText = (source: string, immediate = false) => {
  if (immediate || !props.streaming || typeof window === 'undefined') {
    clearPlainTextFrame();
    pendingVisiblePlainText = '';
    pendingVisiblePlainTextScheduledAt = 0;
    visiblePlainText.value = source;
    return;
  }
  pendingVisiblePlainText = source;
  if (pendingVisiblePlainTextScheduledAt <= 0) {
    pendingVisiblePlainTextScheduledAt = Date.now();
  }
  if (plainTextFrame !== null) return;
  const flush = () => {
    if (plainTextFrame !== null) {
      window.cancelAnimationFrame(plainTextFrame);
      plainTextFrame = null;
    }
    if (plainTextFrameFallbackTimer !== null) {
      window.clearTimeout(plainTextFrameFallbackTimer);
      plainTextFrameFallbackTimer = null;
    }
    const nextText = pendingVisiblePlainText;
    const scheduledAt = pendingVisiblePlainTextScheduledAt;
    const latencyMs = scheduledAt > 0 ? Date.now() - scheduledAt : 0;
    pendingVisiblePlainText = '';
    pendingVisiblePlainTextScheduledAt = 0;
    visiblePlainText.value = nextText;
    if (latencyMs >= STREAM_RENDER_DEBUG_SLOW_MS) {
      const payload = {
        latencyMs,
        contentLength: nextText.length,
        cacheKey: normalizedCacheKey.value,
        runtimeMessageId: props.runtimeMessageId || ''
      };
      chatDebugLog('chat.stream.perf', 'plain-text-slow-flush', payload);
      chatPerf.recordDuration('chat_stream_plain_text_slow_flush', latencyMs, payload);
    } else if (chatPerf.enabled()) {
      chatPerf.recordDuration('chat_stream_plain_text_flush', latencyMs, {
        contentLength: nextText.length
      });
    }
  };
  plainTextFrame = window.requestAnimationFrame(() => {
    flush();
  });
  // Keep local-model bursts visibly streaming even when rAF is delayed by inference load.
  plainTextFrameFallbackTimer = window.setTimeout(() => {
    flush();
  }, PLAIN_TEXT_VISIBLE_FALLBACK_MS);
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
  const plainStreaming = usePlainStreamingText.value;
  if (plainStreaming) {
    updateVisiblePlainText(source);
  } else {
    clearPlainTextFrame();
    pendingVisiblePlainText = '';
    pendingVisiblePlainTextScheduledAt = 0;
  }
  if (!source) {
    updateVisiblePlainText('', true);
    visibleHtml.value = '';
    if (cacheKey) streamingMarkdownCache.delete(cacheKey);
    emit('rendered', buildRenderedPayload(source));
    return;
  }
  if (plainStreaming) {
    schedulePlainTextLayout();
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

const scheduleRender = () => {
  const source = normalizedContent.value;
  const plainStreaming = usePlainStreamingText.value;
  if (plainStreaming) {
    updateVisiblePlainText(source);
  } else {
    clearPlainTextFrame();
    pendingVisiblePlainText = '';
    pendingVisiblePlainTextScheduledAt = 0;
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
  if (plainStreaming) {
    schedulePlainTextLayout();
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
  clearPlainTextFrame();
});
</script>
