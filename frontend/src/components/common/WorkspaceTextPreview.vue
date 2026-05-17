<template>
  <div :class="['workspace-text-preview-shell', wrapperClass]">
    <CodeMirrorEditor
      :model-value="contentValue"
      :source-path="sourcePathValue"
      readonly
      :placeholder="emptyTextValue"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import CodeMirrorEditor from '@/components/common/CodeMirrorEditor.vue';

const props = defineProps<{
  content: string;
  sourcePath?: string;
  wrapperClass?: string;
  emptyText?: string;
}>();

const wrapperClass = computed(() => props.wrapperClass || '');
const emptyTextValue = computed(() => props.emptyText || '');
const sourcePathValue = computed(() => props.sourcePath || '');
const contentValue = computed(() => props.content || '');
</script>

<style scoped>
.workspace-text-preview-shell {
  width: 100%;
  min-width: 0;
  min-height: 0;
  height: 100%;
}
</style>
