<template>
  <div
    v-if="status"
    :class="['message-compaction-divider', `is-${status}`]"
    role="separator"
    :aria-live="status === 'running' ? 'polite' : 'off'"
    :title="tooltipText || undefined"
  >
    <span class="message-compaction-divider-track" aria-hidden="true"></span>
    <span class="message-compaction-divider-label">{{ label }}</span>
  </div>
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
  manualMarker?: boolean;
};

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  isStreaming: false,
  manualMarker: false
});

const { t } = useI18n();
const numberFormatter = new Intl.NumberFormat('en-US');

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string') {
      const trimmed = value.trim();
      if (trimmed) return trimmed;
    }
  }
  return '';
};

const toOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) {
      return Math.round(value);
    }
    if (typeof value === 'string') {
      const normalized = Number(value.trim());
      if (Number.isFinite(normalized)) {
        return Math.round(normalized);
      }
    }
  }
  return null;
};

const formatTokenCount = (value: number | null): string =>
  value === null ? '' : `${numberFormatter.format(value)}`;

const snapshot = computed(() => resolveLatestCompactionSnapshot(props.items));

const status = computed<'running' | 'completed' | 'failed' | 'cancelled' | null>(() => {
  if (!snapshot.value) {
    return props.manualMarker && props.isStreaming ? 'running' : null;
  }
  const running = isCompactionRunningFromWorkflowItems(props.items);
  if (snapshot.value.status === 'cancelled') return 'cancelled';
  if (snapshot.value.status === 'failed') return 'failed';
  if (running) return 'running';
  return 'completed';
});

const hasCollapsedCompactionSummary = computed(() => {
  const detail = snapshot.value?.detail;
  if (!detail) return false;
  const fallbackTrimApplied = Boolean(
    detail.context_guard_fallback_trim_applied ?? detail.contextGuardFallbackTrimApplied
  );
  if (!fallbackTrimApplied) return false;
  const summaryTokens = toOptionalInt(detail.summary_tokens, detail.summaryTokens);
  if (summaryTokens !== null && summaryTokens <= 1) {
    return true;
  }
  const injectedSummary = pickString(
    detail.summary_text,
    detail.summaryText,
    detail.summary_context_text,
    detail.summaryContextText,
    detail.compaction_summary_text,
    detail.compactionSummaryText
  ).toLowerCase();
  return injectedSummary === '...(' || injectedSummary === '...(truncated)' || injectedSummary === '...';
});

const transitionLabel = computed(() => {
  const detail = snapshot.value?.detail;
  if (!detail) return '';
  if (hasCollapsedCompactionSummary.value) return '';
  const before = toOptionalInt(
    detail.projected_request_tokens,
    detail.total_tokens,
    detail.context_tokens,
    detail.context_guard_tokens_before
  );
  const after = toOptionalInt(
    detail.projected_request_tokens_after,
    detail.total_tokens_after,
    detail.context_tokens_after,
    detail.context_guard_tokens_after,
    detail.final_context_tokens
  );
  if (before === null || after === null) return '';
  return `${formatTokenCount(before)} -> ${formatTokenCount(after)}`;
});

const label = computed(() => {
  if (status.value === 'running') return t('chat.compactionDivider.running');
  if (status.value === 'cancelled') return t('chat.compactionDivider.cancelled');
  if (status.value === 'failed') return t('chat.compactionDivider.failed');
  if (transitionLabel.value) {
    return `${t('chat.compactionDivider.completed')} ${transitionLabel.value}`;
  }
  return t('chat.compactionDivider.completed');
});

const tooltipText = computed(() => {
  const detail = snapshot.value?.detail;
  if (!detail) return '';
  const blocks: string[] = [];
  if (transitionLabel.value) {
    blocks.push(`${t('chat.compactionDivider.completed')} ${transitionLabel.value}`);
  } else if (status.value === 'completed') {
    blocks.push(t('chat.compactionDivider.completed'));
  }
  const modelOutput = pickString(
    detail.summary_model_output,
    detail.summaryModelOutput,
    detail.compaction_model_output,
    detail.compactionModelOutput
  );
  const injectedSummary = pickString(
    detail.summary_text,
    detail.summaryText,
    detail.summary_context_text,
    detail.summaryContextText,
    detail.compaction_summary_text,
    detail.compactionSummaryText
  );
  if (modelOutput) {
    if (blocks.length > 0) blocks.push('');
    blocks.push(`${t('chat.toolWorkflow.compaction.output.modelTitle')}:`);
    blocks.push(modelOutput);
  }
  if (injectedSummary && injectedSummary !== modelOutput) {
    if (blocks.length > 0) blocks.push('');
    blocks.push(`${t('chat.toolWorkflow.compaction.output.injectedTitle')}:`);
    blocks.push(injectedSummary);
  }
  return blocks.join('\n').trim();
});
</script>

<style scoped>
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
