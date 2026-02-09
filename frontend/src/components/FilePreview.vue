<template>
  <div class="file-preview">
    <div class="preview-title">{{ t('filePreview.title') }}</div>
    <div v-if="file">
      <p>{{ t('filePreview.name') }}: {{ file.name }}</p>
      <p>{{ t('filePreview.path') }}: {{ file.path }}</p>
      <p>{{ t('filePreview.size') }}: {{ file.size }}</p>
      <p>{{ t('filePreview.type') }}: {{ file.extension || '-' }}</p>
      <p>{{ t('filePreview.updatedAt') }}: {{ file.updated_time || '-' }}</p>
    </div>
    <div v-else class="preview-empty">{{ t('filePreview.empty') }}</div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

defineProps({
  file: {
    type: Object,
    default: null
  }
});

const { t } = useI18n();
</script>
