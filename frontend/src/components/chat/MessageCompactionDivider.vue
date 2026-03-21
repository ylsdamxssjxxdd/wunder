<template>
  <transition name="compaction-divider-fade">
    <div
      v-if="banner"
      :class="['message-compaction-divider', `is-${banner.status}`]"
      role="status"
      :aria-live="banner.status === 'running' ? 'polite' : 'off'"
    >
      <div class="message-compaction-divider-line" aria-hidden="true"></div>
      <div class="message-compaction-divider-chip">
        <span class="message-compaction-divider-dot" aria-hidden="true"></span>
        <span class="message-compaction-divider-copy">
          <span class="message-compaction-divider-title">{{ banner.title }}</span>
          <span class="message-compaction-divider-description">{{ banner.description }}</span>
        </span>
        <span v-if="banner.stageLabel" class="message-compaction-divider-stage">{{ banner.stageLabel }}</span>
      </div>
    </div>
  </transition>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from '@/i18n';
import { buildCompactionDisplay } from '@/utils/chatCompactionUi';
import { resolveLatestCompactionSnapshot } from '@/utils/chatCompactionWorkflow';

type Props = {
  items?: unknown[];
  isLatestAssistant?: boolean;
  isStreaming?: boolean;
};

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  isLatestAssistant: true,
  isStreaming: false
});

const { t } = useI18n();

const resolveCompactionStageLabel = (
  stages: Array<{ label: string; state: string }>
): string => {
  const active = stages.find((stage) => stage.state === 'active');
  if (active?.label) return active.label;
  const warning = [...stages].reverse().find((stage) => stage.state === 'warning');
  if (warning?.label) return warning.label;
  const done = [...stages].reverse().find((stage) => stage.state === 'done');
  return done?.label || '';
};

const banner = computed(() => {
  if (!props.isLatestAssistant) return null;
  const snapshot = resolveLatestCompactionSnapshot(props.items);
  if (!snapshot) return null;
  const display = buildCompactionDisplay(snapshot.detail, snapshot.status, t);
  const running =
    snapshot.eventType === 'compaction_progress'
    || snapshot.status === 'loading'
    || snapshot.status === 'pending';
  const failed = snapshot.status === 'failed';
  const continuing = !failed && !running && props.isStreaming;
  let description = display.summaryNote || display.view.description;
  if (failed && display.view.failure?.description) {
    description = display.view.failure.description;
  } else if (continuing) {
    description = t('chat.toolWorkflow.compaction.noteRecovered');
  } else if (!running) {
    description = t('chat.toolWorkflow.compaction.notePrepared');
  }
  return {
    status: failed ? 'failed' : running || continuing ? 'running' : 'completed',
    title: display.view.headline || t('chat.toolWorkflow.compaction.title'),
    description,
    stageLabel: resolveCompactionStageLabel(display.view.stages)
  };
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
  position: relative;
  margin-top: 10px;
  padding-top: 6px;
}

.message-compaction-divider-line {
  height: 1px;
  width: 100%;
  background: linear-gradient(
    90deg,
    rgba(148, 163, 184, 0),
    rgba(148, 163, 184, 0.56) 18%,
    rgba(148, 163, 184, 0.56) 82%,
    rgba(148, 163, 184, 0)
  );
}

.message-compaction-divider-chip {
  display: inline-flex;
  align-items: flex-start;
  gap: 8px;
  margin-top: -11px;
  padding: 5px 10px;
  border-radius: 999px;
  border: 1px solid rgba(var(--chat-primary-rgb, 59, 130, 246), 0.28);
  background: rgba(255, 255, 255, 0.92);
  color: var(--chat-text, #0f172a);
  max-width: min(100%, 680px);
}

.message-compaction-divider.is-running .message-compaction-divider-chip {
  border-color: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.4);
}

.message-compaction-divider.is-failed .message-compaction-divider-chip {
  border-color: rgba(239, 68, 68, 0.38);
  background: rgba(254, 242, 242, 0.95);
  color: #991b1b;
}

.message-compaction-divider-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  margin-top: 5px;
  flex: 0 0 auto;
  background: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.95);
  box-shadow: 0 0 0 3px rgba(var(--chat-primary-rgb, 59, 130, 246), 0.16);
}

.message-compaction-divider.is-running .message-compaction-divider-dot {
  animation: compaction-divider-pulse 1.2s ease-in-out infinite;
}

.message-compaction-divider.is-completed .message-compaction-divider-dot {
  background: rgba(34, 197, 94, 0.94);
  box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.16);
}

.message-compaction-divider.is-failed .message-compaction-divider-dot {
  background: rgba(239, 68, 68, 0.94);
  box-shadow: 0 0 0 3px rgba(239, 68, 68, 0.16);
}

.message-compaction-divider-copy {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.message-compaction-divider-title {
  font-size: 11px;
  line-height: 1.45;
  font-weight: 700;
  color: inherit;
  word-break: break-word;
}

.message-compaction-divider-description {
  font-size: 11px;
  line-height: 1.45;
  color: var(--chat-muted, #64748b);
  word-break: break-word;
}

.message-compaction-divider.is-failed .message-compaction-divider-description {
  color: #b91c1c;
}

.message-compaction-divider-stage {
  margin-left: auto;
  font-size: 10px;
  line-height: 1.4;
  font-weight: 700;
  border-radius: 999px;
  padding: 2px 8px;
  border: 1px solid rgba(148, 163, 184, 0.34);
  background: rgba(248, 250, 252, 0.85);
  color: var(--chat-muted, #64748b);
  flex: 0 0 auto;
}

@keyframes compaction-divider-pulse {
  0%,
  100% {
    transform: scale(1);
    opacity: 1;
  }
  50% {
    transform: scale(0.82);
    opacity: 0.56;
  }
}
</style>

