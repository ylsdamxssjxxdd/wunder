<template>
  <transition name="message-waiting-fade">
    <section
      v-if="shouldRender"
      :class="['message-waiting-notice', `is-${tone}`]"
      role="status"
      aria-live="polite"
    >
      <div class="message-waiting-main">
        <span class="message-waiting-icon" aria-hidden="true">
          <i :class="iconClass"></i>
        </span>
        <div class="message-waiting-copy">
          <div class="message-waiting-title-row">
            <div class="message-waiting-title">{{ title }}</div>
            <span class="message-waiting-pill">{{ elapsedLabel }}</span>
            <span v-if="silentLabel" class="message-waiting-pill is-muted">{{ silentLabel }}</span>
          </div>
          <div class="message-waiting-detail">{{ detail }}</div>
          <div v-if="note" class="message-waiting-note">{{ note }}</div>
        </div>
      </div>
      <div v-if="showResumeAction || showStopAction" class="message-waiting-actions">
        <button
          v-if="showResumeAction"
          class="message-waiting-btn"
          type="button"
          @click="$emit('resume')"
        >
          <i class="fa-solid fa-rotate" aria-hidden="true"></i>
          <span>{{ t('chat.message.resume') }}</span>
        </button>
        <button
          v-if="showStopAction"
          class="message-waiting-btn is-secondary"
          type="button"
          @click="$emit('stop')"
        >
          <i class="fa-solid fa-stop" aria-hidden="true"></i>
          <span>{{ t('common.stop') }}</span>
        </button>
      </div>
    </section>
  </transition>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import { normalizeChatTimestampMs } from '@/utils/chatTiming';

type WorkflowItemLike = {
  eventType?: unknown;
  isTool?: unknown;
  attempt?: unknown;
  maxAttempts?: unknown;
  delayS?: unknown;
  retryReason?: unknown;
};

type MessageLike = {
  role?: unknown;
  content?: unknown;
  reasoning?: unknown;
  workflowStreaming?: unknown;
  stream_incomplete?: unknown;
  resume_available?: unknown;
  slow_client?: unknown;
  created_at?: unknown;
  waiting_updated_at_ms?: unknown;
  waiting_first_output_at_ms?: unknown;
  stats?: {
    interaction_start_ms?: unknown;
  } | null;
  workflowItems?: WorkflowItemLike[];
};

type WaitingPhase =
  | 'preparing'
  | 'queued'
  | 'model'
  | 'retrying'
  | 'stalled'
  | 'resumable';

type Props = {
  message?: MessageLike | null;
  canStop?: boolean;
  canResume?: boolean;
};

const TOOL_EVENT_PREFIXES = ['tool_', 'subagent_', 'team_'];
const TOOL_EVENT_TYPES = new Set([
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary',
  'command_session_delta'
]);
const COMPACTION_EVENT_TYPES = new Set(['compaction', 'compaction_progress']);

const props = withDefaults(defineProps<Props>(), {
  message: null,
  canStop: false,
  canResume: false
});

defineEmits<{
  (event: 'resume'): void;
  (event: 'stop'): void;
}>();

const { t } = useI18n();

const nowMs = ref(Date.now());
let timer: number | null = null;

const normalizeFlag = (value: unknown) => value === true || value === 'true';

const resolveTimestampMs = (value: unknown) => {
  const millis = normalizeChatTimestampMs(value);
  return Number.isFinite(millis) ? Number(millis) : null;
};

const workflowItems = computed<WorkflowItemLike[]>(() =>
  Array.isArray(props.message?.workflowItems) ? props.message.workflowItems : []
);

const isPending = computed(
  () =>
    normalizeFlag(props.message?.workflowStreaming) ||
    normalizeFlag(props.message?.stream_incomplete) ||
    normalizeFlag(props.message?.resume_available) ||
    normalizeFlag(props.message?.slow_client)
);

const hasVisibleOutput = computed(
  () =>
    Boolean(String(props.message?.content || '').trim()) ||
    Boolean(String(props.message?.reasoning || '').trim()) ||
    resolveTimestampMs(props.message?.waiting_first_output_at_ms) !== null
);

const startMs = computed(
  () =>
    resolveTimestampMs(props.message?.stats?.interaction_start_ms) ??
    resolveTimestampMs(props.message?.created_at) ??
    nowMs.value
);

const lastProgressMs = computed(
  () => resolveTimestampMs(props.message?.waiting_updated_at_ms) ?? startMs.value
);

const elapsedMs = computed(() => Math.max(0, nowMs.value - startMs.value));
const silentMs = computed(() => Math.max(0, nowMs.value - lastProgressMs.value));

const normalizeEventType = (value: unknown) => String(value || '').trim().toLowerCase();

const findLatestItem = (predicate: (item: WorkflowItemLike, index: number) => boolean) => {
  for (let index = workflowItems.value.length - 1; index >= 0; index -= 1) {
    const item = workflowItems.value[index];
    if (predicate(item, index)) {
      return { item, index };
    }
  }
  return { item: null, index: -1 };
};

const latestRetry = computed(() =>
  findLatestItem((item) => normalizeEventType(item?.eventType) === 'llm_stream_retry')
);
const latestQueue = computed(() =>
  findLatestItem((item) => ['queued', 'queue_enter', 'queue_start'].includes(normalizeEventType(item?.eventType)))
);
const latestRequest = computed(() =>
  findLatestItem((item) => normalizeEventType(item?.eventType) === 'llm_request')
);
const latestSlowClient = computed(() =>
  findLatestItem((item) => normalizeEventType(item?.eventType) === 'slow_client')
);

const hasToolActivity = computed(() =>
  workflowItems.value.some((item) => {
    if (normalizeFlag(item?.isTool)) return true;
    const eventType = normalizeEventType(item?.eventType);
    if (!eventType) return false;
    if (TOOL_EVENT_TYPES.has(eventType)) return true;
    return TOOL_EVENT_PREFIXES.some((prefix) => eventType.startsWith(prefix));
  })
);

const hasCompactionActivity = computed(() =>
  workflowItems.value.some((item) => COMPACTION_EVENT_TYPES.has(normalizeEventType(item?.eventType)))
);

const showResumeAction = computed(() => props.canResume === true);
const showStopAction = computed(() => props.canStop === true);

const phase = computed<WaitingPhase>(() => {
  if (showResumeAction.value && !normalizeFlag(props.message?.workflowStreaming)) {
    return 'resumable';
  }
  if (latestRetry.value.index >= 0) {
    return 'retrying';
  }
  if (latestSlowClient.value.index >= 0 && !normalizeFlag(props.message?.workflowStreaming)) {
    return 'resumable';
  }
  if (latestQueue.value.index >= 0 && latestQueue.value.index > latestRequest.value.index) {
    return 'queued';
  }
  if (latestRequest.value.index >= 0) {
    return silentMs.value >= 18000 ? 'stalled' : 'model';
  }
  if (elapsedMs.value >= 12000 || silentMs.value >= 12000) {
    return 'stalled';
  }
  return 'preparing';
});

const shouldRender = computed(() => {
  if (String(props.message?.role || '').trim().toLowerCase() !== 'assistant') {
    return false;
  }
  if (!isPending.value || hasVisibleOutput.value) {
    return false;
  }
  if (hasCompactionActivity.value || hasToolActivity.value) {
    return false;
  }
  return true;
});

const formatElapsedCompact = (millis: number) => {
  const totalSeconds = Math.max(1, Math.round(millis / 1000));
  if (totalSeconds < 60) return `${totalSeconds}s`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) {
    return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
  }
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes > 0 ? `${hours}h ${restMinutes}m` : `${hours}h`;
};

const resolveRetryReasonLabel = (reason: unknown) => {
  const normalized = String(reason || '').trim().toLowerCase();
  if (!normalized) return '';
  const key = `chat.waiting.retryReason.${normalized}`;
  const translated = t(key);
  return translated === key ? normalized : translated;
};

const retryAttempt = computed(() => {
  const parsed = Number.parseInt(String(latestRetry.value.item?.attempt ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
});

const retryMaxAttempts = computed(() => {
  const parsed = Number.parseInt(String(latestRetry.value.item?.maxAttempts ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
});

const retryDelayLabel = computed(() => {
  const parsed = Number(latestRetry.value.item?.delayS);
  if (!Number.isFinite(parsed) || parsed <= 0) return '';
  return formatElapsedCompact(parsed * 1000);
});

const retryReasonLabel = computed(() =>
  resolveRetryReasonLabel(latestRetry.value.item?.retryReason)
);

const title = computed(() => {
  switch (phase.value) {
    case 'queued':
      return t('chat.waiting.queuedTitle');
    case 'model':
      return t('chat.waiting.modelTitle');
    case 'retrying':
      return t('chat.waiting.retryTitle');
    case 'stalled':
      return t('chat.waiting.stalledTitle');
    case 'resumable':
      return t('chat.waiting.resumableTitle');
    default:
      return t('chat.waiting.preparingTitle');
  }
});

const detail = computed(() => {
  switch (phase.value) {
    case 'queued':
      return t('chat.waiting.queuedDetail');
    case 'model':
      return elapsedMs.value >= 20000
        ? t('chat.waiting.modelDetailLong')
        : t('chat.waiting.modelDetail');
    case 'retrying':
      if (retryAttempt.value && retryMaxAttempts.value && retryDelayLabel.value) {
        return t('chat.waiting.retryDetail', {
          attempt: retryAttempt.value,
          maxAttempts: retryMaxAttempts.value,
          delay: retryDelayLabel.value
        });
      }
      if (retryAttempt.value && retryMaxAttempts.value) {
        return t('chat.waiting.retryDetailNoDelay', {
          attempt: retryAttempt.value,
          maxAttempts: retryMaxAttempts.value
        });
      }
      return t('chat.waiting.retryDetailGeneric');
    case 'stalled':
      return t('chat.waiting.stalledDetail');
    case 'resumable':
      return t('chat.waiting.resumableDetail');
    default:
      return t('chat.waiting.preparingDetail');
  }
});

const note = computed(() => {
  if (phase.value === 'retrying' && retryReasonLabel.value) {
    return retryReasonLabel.value;
  }
  if (silentMs.value >= 15000) {
    return t('chat.waiting.silent', { time: formatElapsedCompact(silentMs.value) });
  }
  return '';
});

const elapsedLabel = computed(() =>
  t('chat.waiting.elapsed', { time: formatElapsedCompact(elapsedMs.value) })
);

const silentLabel = computed(() => {
  if (silentMs.value < 15000 || phase.value === 'retrying') {
    return '';
  }
  return t('chat.waiting.silentBrief', { time: formatElapsedCompact(silentMs.value) });
});

const tone = computed(() => {
  if (phase.value === 'retrying' || phase.value === 'resumable') return 'warning';
  if (phase.value === 'queued') return 'muted';
  return 'running';
});

const iconClass = computed(() => {
  if (phase.value === 'queued') return 'fa-solid fa-clock message-waiting-icon-static';
  if (phase.value === 'retrying') return 'fa-solid fa-rotate message-waiting-icon-spin';
  if (phase.value === 'resumable' || phase.value === 'stalled') {
    return 'fa-solid fa-triangle-exclamation message-waiting-icon-static';
  }
  return 'fa-solid fa-circle-notch message-waiting-icon-spin';
});

const stopTimer = () => {
  if (timer !== null) {
    window.clearInterval(timer);
    timer = null;
  }
};

const ensureTimer = () => {
  if (timer !== null || typeof window === 'undefined') return;
  timer = window.setInterval(() => {
    nowMs.value = Date.now();
  }, 1000);
};

watch(
  shouldRender,
  (value) => {
    nowMs.value = Date.now();
    if (value) {
      ensureTimer();
      return;
    }
    stopTimer();
  },
  { immediate: true }
);

onMounted(() => {
  if (shouldRender.value) {
    ensureTimer();
  }
});

onBeforeUnmount(() => {
  stopTimer();
});
</script>

<style scoped>
.message-waiting-fade-enter-active,
.message-waiting-fade-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}

.message-waiting-fade-enter-from,
.message-waiting-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.message-waiting-notice {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 14px;
  margin: 8px 0 12px;
  padding: 12px 14px;
  border-radius: 16px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background:
    linear-gradient(135deg, rgba(255, 255, 255, 0.92), rgba(248, 250, 252, 0.98));
}

.message-waiting-notice.is-running {
  border-color: rgba(59, 130, 246, 0.2);
  background:
    linear-gradient(135deg, rgba(239, 246, 255, 0.96), rgba(248, 250, 252, 0.98));
}

.message-waiting-notice.is-warning {
  border-color: rgba(245, 158, 11, 0.28);
  background:
    linear-gradient(135deg, rgba(255, 247, 237, 0.96), rgba(255, 251, 235, 0.98));
}

.message-waiting-notice.is-muted {
  border-color: rgba(148, 163, 184, 0.22);
  background:
    linear-gradient(135deg, rgba(248, 250, 252, 0.96), rgba(241, 245, 249, 0.98));
}

.message-waiting-main {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  min-width: 0;
  flex: 1;
}

.message-waiting-icon {
  width: 28px;
  height: 28px;
  border-radius: 999px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: rgba(15, 23, 42, 0.06);
  color: rgba(30, 41, 59, 0.86);
  flex: 0 0 auto;
}

.message-waiting-notice.is-running .message-waiting-icon {
  background: rgba(59, 130, 246, 0.12);
  color: rgba(37, 99, 235, 0.92);
}

.message-waiting-notice.is-warning .message-waiting-icon {
  background: rgba(245, 158, 11, 0.14);
  color: rgba(217, 119, 6, 0.94);
}

.message-waiting-notice.is-muted .message-waiting-icon {
  background: rgba(148, 163, 184, 0.12);
  color: rgba(71, 85, 105, 0.88);
}

.message-waiting-copy {
  min-width: 0;
  flex: 1;
}

.message-waiting-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.message-waiting-title {
  font-size: 13px;
  line-height: 1.4;
  font-weight: 600;
  color: var(--chat-text, #0f172a);
}

.message-waiting-pill {
  display: inline-flex;
  align-items: center;
  min-height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.8);
  border: 1px solid rgba(148, 163, 184, 0.18);
  color: var(--chat-muted, #64748b);
  font-size: 11px;
  line-height: 1.2;
}

.message-waiting-pill.is-muted {
  background: rgba(255, 255, 255, 0.62);
}

.message-waiting-detail {
  margin-top: 6px;
  font-size: 12px;
  line-height: 1.65;
  color: rgba(51, 65, 85, 0.92);
}

.message-waiting-note {
  margin-top: 6px;
  font-size: 11px;
  line-height: 1.6;
  color: var(--chat-muted, #64748b);
}

.message-waiting-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.message-waiting-btn {
  border: none;
  border-radius: 999px;
  padding: 8px 12px;
  font-size: 12px;
  line-height: 1.2;
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
  color: #fff;
  background: linear-gradient(135deg, #2563eb, #1d4ed8);
}

.message-waiting-btn.is-secondary {
  color: #0f172a;
  background: rgba(255, 255, 255, 0.82);
  box-shadow: inset 0 0 0 1px rgba(148, 163, 184, 0.28);
}

.message-waiting-btn:hover {
  transform: translateY(-1px);
}

.message-waiting-icon-spin {
  animation: message-waiting-spin 1.1s linear infinite;
}

@keyframes message-waiting-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

@media (max-width: 720px) {
  .message-waiting-notice {
    flex-direction: column;
  }

  .message-waiting-actions {
    width: 100%;
    justify-content: flex-start;
  }
}

@media (prefers-reduced-motion: reduce) {
  .message-waiting-fade-enter-active,
  .message-waiting-fade-leave-active,
  .message-waiting-btn {
    transition: none;
  }

  .message-waiting-icon-spin {
    animation: none;
  }
}
</style>
