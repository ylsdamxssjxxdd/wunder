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
    <pre v-else class="workflow-dialog-detail">{{ content }}</pre>
  </el-dialog>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

defineProps<{
  visible: boolean;
  loading: boolean;
  content: string;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const handleVisibleChange = (nextVisible: boolean) => {
  emit('update:visible', Boolean(nextVisible));
};
</script>
