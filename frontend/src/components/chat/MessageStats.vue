<template>
  <div v-if="entries.length" class="messenger-message-stats">
    <span
      v-for="entry in entries"
      :key="entry.key"
      :class="[
        'messenger-message-stat',
        entry.kind === 'status' ? 'is-status' : 'is-metric',
        entry.tone ? `is-${entry.tone}` : '',
        entry.live ? 'is-live' : ''
      ]"
    >
      <i
        :class="[entry.iconClass || 'fa-solid fa-circle-info', 'messenger-message-stat-icon']"
        :title="entry.kind === 'metric' ? entry.label : undefined"
        :aria-label="entry.kind === 'metric' ? entry.label : undefined"
        aria-hidden="true"
      ></i>
      <span class="messenger-message-stat-value">{{ entry.value }}</span>
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { buildAssistantMessageStatsEntries, type MessageStatsEntry } from '@/utils/messageStats';
import { hasActiveSubagentItems } from '@/utils/subagentRuntime';
import {
  hasAssistantPendingQuestion,
  hasAssistantWaitingForCurrentOutput,
  isAssistantMessageRunning
} from '@/utils/assistantMessageRuntime';

type MessageRecord = Record<string, unknown>;

const props = withDefaults(defineProps<{
  message: MessageRecord;
  activeSessionBusy?: boolean;
  latestVisibleAssistant?: boolean;
}>(), {
  activeSessionBusy: false,
  latestVisibleAssistant: false
});

const { t } = useI18n();
const nowTick = ref(Date.now());
let timer: number | null = null;

const isLive = computed(() => {
  const message = props.message || {};
  return Boolean(
    props.latestVisibleAssistant && props.activeSessionBusy ||
      message.workflowStreaming ||
      message.reasoningStreaming ||
      message.stream_incomplete ||
      message.retry_started_at_ms ||
      message.retry_next_attempt_at_ms ||
      message.retry_attempt ||
      hasAssistantPendingQuestion(message) ||
      hasAssistantWaitingForCurrentOutput(message) ||
      isAssistantMessageRunning(message) ||
      hasActiveSubagentItems(message.subagents)
  );
});

const entries = computed<MessageStatsEntry[]>(() => {
  const message = props.message || {};
  if (String(message.role || '') !== 'assistant' || message.isGreeting) return [];
  return buildAssistantMessageStatsEntries(message, t, undefined, nowTick.value, {
    activeSessionBusy: props.activeSessionBusy,
    latestVisibleAssistant: props.latestVisibleAssistant
  });
});

const syncTimer = () => {
  if (timer !== null) {
    window.clearInterval(timer);
    timer = null;
  }
  if (isLive.value && typeof window !== 'undefined') {
    timer = window.setInterval(() => {
      nowTick.value = Date.now();
    }, 1000);
  }
};

watch(isLive, syncTimer, { immediate: true });
onBeforeUnmount(() => {
  if (timer !== null) window.clearInterval(timer);
});
</script>
