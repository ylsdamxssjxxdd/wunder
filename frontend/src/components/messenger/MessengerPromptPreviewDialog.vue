<template>
  <el-dialog
    :model-value="visible"
    class="system-prompt-dialog"
    :title="t('chat.systemPrompt.title')"
    width="720px"
    append-to-body
    @update:model-value="handleVisibleChange"
  >
    <div v-if="loading" class="messenger-list-empty">{{ t('chat.systemPrompt.loading') }}</div>
    <template v-else>
      <section class="system-prompt-memory-panel" :class="`system-prompt-memory-panel--${memoryMode}`">
        <div class="system-prompt-section-head">
          <div class="system-prompt-section-title">{{ t('chat.systemPrompt.memoryTitle') }}</div>
          <div v-if="memoryMode !== 'none'" class="system-prompt-section-badges">
            <span class="system-prompt-badge">
              {{ memoryMode === 'frozen' ? t('chat.systemPrompt.memoryFrozen') : t('chat.systemPrompt.memoryPending') }}
            </span>
            <span class="system-prompt-badge">{{ t('chat.systemPrompt.memoryCount', { count: memoryCount }) }}</span>
          </div>
        </div>
        <div class="system-prompt-memory-hint muted">
          {{
            memoryMode === 'frozen'
              ? t('chat.systemPrompt.memoryFrozenHint')
              : memoryMode === 'pending'
                ? t('chat.systemPrompt.memoryPendingHint')
                : t('chat.systemPrompt.memoryEmpty')
          }}
        </div>
        <pre v-if="memoryMode !== 'none' && memoryContent" class="system-prompt-memory-content">{{ memoryContent }}</pre>
      </section>
      <section class="system-prompt-full-panel">
        <div class="system-prompt-section-head">
          <div class="system-prompt-section-title">{{ t('chat.systemPrompt.fullPromptTitle') }}</div>
        </div>
        <pre class="workflow-dialog-detail system-prompt-content" v-html="htmlContent"></pre>
      </section>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

defineProps<{
  visible: boolean;
  loading: boolean;
  htmlContent: string;
  memoryContent: string;
  memoryCount: number;
  memoryMode: 'none' | 'pending' | 'frozen';
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const handleVisibleChange = (nextVisible: boolean) => {
  emit('update:visible', Boolean(nextVisible));
};
</script>

<style scoped>
.system-prompt-memory-panel,
.system-prompt-full-panel {
  border: 1px solid var(--app-border-color, rgba(148, 163, 184, 0.24));
  border-radius: 14px;
  background: var(--app-panel-bg, rgba(15, 23, 42, 0.04));
  padding: 14px;
}

.system-prompt-memory-panel {
  margin-bottom: 14px;
}

.system-prompt-memory-panel--frozen {
  border-color: rgba(59, 130, 246, 0.32);
}

.system-prompt-memory-panel--pending {
  border-color: rgba(245, 158, 11, 0.32);
}

.system-prompt-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
  flex-wrap: wrap;
}

.system-prompt-section-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--app-text-color, #0f172a);
}

.system-prompt-section-badges {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.system-prompt-badge {
  display: inline-flex;
  align-items: center;
  border-radius: 999px;
  padding: 3px 10px;
  font-size: 12px;
  background: rgba(148, 163, 184, 0.12);
  color: var(--app-text-muted, #64748b);
}

.system-prompt-memory-content {
  margin: 12px 0 0;
  padding: 12px;
  border-radius: 12px;
  background: var(--app-surface-bg, rgba(255, 255, 255, 0.9));
  color: var(--app-text-color, #0f172a);
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.7;
}

.system-prompt-memory-hint {
  line-height: 1.7;
}
</style>
