<template>
  <transition name="compaction-divider-fade">
    <div
      v-if="status"
      :class="['message-compaction-divider', `is-${status}`]"
      role="separator"
      :aria-live="status === 'running' ? 'polite' : 'off'"
    >
      <span class="message-compaction-divider-track" aria-hidden="true"></span>
      <span class="message-compaction-divider-label">{{ label }}</span>
    </div>
  </transition>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from '@/i18n';
import {
  isCompactionRunningFromWorkflowItems,
  resolveLatestCompactionSnapshot
} from '@/utils/chatCompactionWorkflow';

type Props = {
  items?: unknown[];
  isStreaming?: boolean;
};

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  isStreaming: false
});

const { t } = useI18n();

const status = computed<'running' | 'completed' | 'failed' | 'cancelled' | null>(() => {
  const snapshot = resolveLatestCompactionSnapshot(props.items);
  if (!snapshot) return null;
  const running = isCompactionRunningFromWorkflowItems(props.items);
  if (snapshot.status === 'cancelled') return 'cancelled';
  if (snapshot.status === 'failed') return 'failed';
  if (running) return 'running';
  return 'completed';
});

const label = computed(() => {
  if (status.value === 'running') return t('chat.compactionDivider.running');
  if (status.value === 'cancelled') return t('chat.compactionDivider.cancelled');
  if (status.value === 'failed') return t('chat.compactionDivider.failed');
  return t('chat.compactionDivider.completed');
});
</script>

<style scoped>
.compaction-divider-fade-enter-active,
.compaction-divider-fade-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}

.compaction-divider-fade-enter-from,
.compaction-divider-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.message-compaction-divider {
  width: 100%;
  margin: 18px 0 20px;
  padding: 0;
}

.message-compaction-divider-track {
  display: block;
  height: 1px;
  width: 100%;
  background: linear-gradient(
    90deg,
    rgba(148, 163, 184, 0),
    rgba(148, 163, 184, 0.56) 22%,
    rgba(148, 163, 184, 0.56) 78%,
    rgba(148, 163, 184, 0)
  );
}

.message-compaction-divider.is-running .message-compaction-divider-track {
  background: linear-gradient(
    90deg,
    rgba(59, 130, 246, 0.1),
    rgba(59, 130, 246, 0.62),
    rgba(59, 130, 246, 0.1)
  );
  background-size: 220% 100%;
  animation: compaction-divider-running 1.8s linear infinite;
}

.message-compaction-divider.is-completed .message-compaction-divider-track {
  background: linear-gradient(
    90deg,
    rgba(34, 197, 94, 0.08),
    rgba(34, 197, 94, 0.42),
    rgba(34, 197, 94, 0.08)
  );
}

.message-compaction-divider.is-failed .message-compaction-divider-track {
  background: linear-gradient(
    90deg,
    rgba(239, 68, 68, 0.08),
    rgba(239, 68, 68, 0.46),
    rgba(239, 68, 68, 0.08)
  );
}

.message-compaction-divider.is-cancelled .message-compaction-divider-track {
  background: linear-gradient(
    90deg,
    rgba(148, 163, 184, 0.08),
    rgba(148, 163, 184, 0.5),
    rgba(148, 163, 184, 0.08)
  );
}

.message-compaction-divider-label {
  display: block;
  margin-top: 7px;
  text-align: center;
  font-size: 11px;
  line-height: 1.4;
  letter-spacing: 0.01em;
  color: var(--chat-muted, #6b7280);
}

.message-compaction-divider.is-running .message-compaction-divider-label {
  color: rgba(59, 130, 246, 0.92);
}

.message-compaction-divider.is-completed .message-compaction-divider-label {
  color: rgba(34, 197, 94, 0.9);
}

.message-compaction-divider.is-cancelled .message-compaction-divider-label {
  color: rgba(100, 116, 139, 0.95);
}

.message-compaction-divider.is-failed .message-compaction-divider-label {
  color: rgba(239, 68, 68, 0.92);
}

@keyframes compaction-divider-running {
  0% {
    background-position: 100% 50%;
  }
  100% {
    background-position: -100% 50%;
  }
}

@media (prefers-reduced-motion: reduce) {
  .message-compaction-divider.is-running .message-compaction-divider-track {
    animation: none;
  }
}
</style>
