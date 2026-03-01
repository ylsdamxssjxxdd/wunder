<template>
  <div class="tool-approval-composer">
    <div class="tool-approval-composer__header">
      <i class="fa-solid fa-shield-halved" aria-hidden="true"></i>
      <span>{{ t('chat.approval.title') }}</span>
    </div>
    <div class="tool-approval-composer__summary">{{ approval.summary }}</div>
    <div class="tool-approval-composer__meta">
      <span v-if="approval.tool">{{ t('chat.approval.tool') }}: {{ approval.tool }}</span>
      <span v-if="approval.kind">{{ t('chat.approval.kind') }}: {{ approvalKindLabel }}</span>
    </div>
    <pre v-if="detailText" class="tool-approval-composer__detail">{{ detailText }}</pre>
    <div class="tool-approval-composer__actions">
      <button class="tool-approval-btn ghost" type="button" :disabled="busy" @click="$emit('decide', 'deny')">
        {{ t('chat.approval.deny') }}
      </button>
      <button
        class="tool-approval-btn primary"
        type="button"
        :disabled="busy"
        @click="$emit('decide', 'approve_once')"
      >
        {{ t('chat.approval.once') }}
      </button>
      <button
        class="tool-approval-btn success"
        type="button"
        :disabled="busy"
        @click="$emit('decide', 'approve_session')"
      >
        {{ t('chat.approval.session') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps<{
  approval: {
    approval_id: string;
    summary: string;
    tool: string;
    kind: string;
    detail: unknown;
  };
  busy?: boolean;
}>();

defineEmits<{
  (event: 'decide', decision: 'approve_once' | 'approve_session' | 'deny'): void;
}>();

const { t } = useI18n();

const approvalKindLabel = computed(() => {
  const raw = String(props.approval?.kind || '').trim().toLowerCase();
  if (raw === 'exec') return t('chat.approval.kind.exec');
  if (raw === 'patch') return t('chat.approval.kind.patch');
  return raw || '-';
});

const detailText = computed(() => {
  const value = props.approval?.detail;
  if (value === null || value === undefined || value === '') {
    return '';
  }
  if (typeof value === 'string') {
    return value;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
});
</script>

<style scoped>
.tool-approval-composer {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  padding: 12px 14px;
  border: 1px solid var(--messenger-line, rgba(148, 163, 184, 0.25));
  border-radius: 14px;
  background: var(--messenger-surface, #fff);
}

.tool-approval-composer__header {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 700;
}

.tool-approval-composer__summary {
  font-size: 13px;
  line-height: 1.6;
  word-break: break-word;
}

.tool-approval-composer__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  font-size: 12px;
  color: var(--messenger-text-sub, #64748b);
}

.tool-approval-composer__detail {
  margin: 0;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid var(--messenger-line, rgba(148, 163, 184, 0.25));
  background: var(--messenger-soft, #f8fafc);
  font-size: 12px;
  line-height: 1.5;
  max-height: 180px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-approval-composer__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.tool-approval-btn {
  border: none;
  border-radius: 10px;
  padding: 7px 12px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}

.tool-approval-btn:disabled {
  opacity: 0.65;
  cursor: not-allowed;
}

.tool-approval-btn.ghost {
  background: var(--messenger-soft, #f1f5f9);
  color: var(--messenger-text-sub, #64748b);
}

.tool-approval-btn.primary {
  background: #2563eb;
  color: #fff;
}

.tool-approval-btn.success {
  background: #16a34a;
  color: #fff;
}
</style>
