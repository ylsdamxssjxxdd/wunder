<template>
  <div class="agent-preset-questions-field">
    <div v-if="!draftQuestions.length" class="agent-preset-questions-field__empty">
      {{ t('portal.agent.presetQuestions.empty') }}
    </div>
    <div
      v-for="(question, index) in draftQuestions"
      :key="`preset-question-${index}`"
      class="agent-preset-questions-field__row"
    >
      <div class="agent-preset-questions-field__index">{{ index + 1 }}</div>
      <el-input
        :model-value="question"
        type="textarea"
        :autosize="{ minRows: 1, maxRows: 4 }"
        :placeholder="t('portal.agent.presetQuestions.placeholder')"
        :disabled="readonly"
        @update:model-value="updateQuestion(index, $event)"
      />
      <button
        v-if="!readonly"
        class="agent-preset-questions-field__remove"
        type="button"
        :title="t('common.remove')"
        :aria-label="t('common.remove')"
        @click="removeQuestion(index)"
      >
        <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import { normalizeAgentPresetQuestionDrafts } from '@/utils/agentPresetQuestions';

const props = defineProps({
  modelValue: {
    type: Array,
    default: () => []
  },
  readonly: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:modelValue']);

const { t } = useI18n();
const draftQuestions = ref<string[]>([]);

function syncDraftQuestions(value: unknown) {
  draftQuestions.value = normalizeAgentPresetQuestionDrafts(value);
}

function emitQuestions() {
  emit('update:modelValue', [...draftQuestions.value]);
}

function updateQuestion(index: number, value: string) {
  const next = [...draftQuestions.value];
  next[index] = String(value ?? '');
  draftQuestions.value = next;
  emitQuestions();
}

function addQuestion() {
  draftQuestions.value = [...draftQuestions.value, ''];
  emitQuestions();
}

function removeQuestion(index: number) {
  draftQuestions.value = draftQuestions.value.filter((_, itemIndex) => itemIndex !== index);
  emitQuestions();
}

defineExpose({
  addQuestion
});

watch(
  () => props.modelValue,
  (value) => {
    syncDraftQuestions(value);
  },
  { immediate: true, deep: true }
);
</script>

<style scoped>
.agent-preset-questions-field {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  min-width: 0;
}

.agent-preset-questions-field__empty {
  font-size: 12px;
  line-height: 1.6;
  color: var(--el-text-color-secondary, #6b7280);
  padding: 10px 12px;
  border: 1px dashed rgba(148, 163, 184, 0.28);
  border-radius: 10px;
  background: rgba(148, 163, 184, 0.04);
}

.agent-preset-questions-field__row {
  display: flex;
  width: 100%;
  min-width: max-content;
  gap: 10px;
  align-items: start;
}

.agent-preset-questions-field__row :deep(.el-textarea) {
  flex: 1 0 520px;
  min-width: 520px;
}

.agent-preset-questions-field__row :deep(.el-textarea__inner) {
  overflow-x: auto;
  white-space: nowrap;
  resize: none;
}

.agent-preset-questions-field__index {
  flex: 0 0 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 32px;
  border-radius: 8px;
  background: rgba(148, 163, 184, 0.12);
  color: var(--el-text-color-secondary, #64748b);
  font-size: 12px;
  font-weight: 600;
}

.agent-preset-questions-field__remove {
  border: 1px solid rgba(148, 163, 184, 0.2);
  border-radius: 8px;
  background: transparent;
  color: inherit;
  cursor: pointer;
}

.agent-preset-questions-field__remove {
  flex: 0 0 34px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 32px;
}

.agent-preset-questions-field__remove:hover {
  border-color: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.3);
  background: var(--ui-accent-soft, rgba(59, 130, 246, 0.08));
}
</style>
