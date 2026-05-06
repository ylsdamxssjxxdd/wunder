<template>
  <el-dialog
    :model-value="visible"
    class="messenger-dialog messenger-goal-dialog"
    width="560px"
    top="calc(var(--desktop-window-chrome-height, 36px) + 12px)"
    :show-close="false"
    :close-on-click-modal="!submitting"
    append-to-body
    destroy-on-close
    @update:model-value="updateVisible"
    @opened="focusObjective"
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-title">
          <i class="fa-solid fa-bullseye" aria-hidden="true"></i>
          <span>{{ t('chat.goal.dialogTitle') }}</span>
        </div>
        <button
          class="messenger-dialog-close"
          type="button"
          :aria-label="t('common.close')"
          :title="t('common.close')"
          :disabled="submitting"
          @click.stop="close"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
    </template>

    <div class="messenger-goal-dialog-body">
      <label class="messenger-goal-dialog-field">
        <span class="messenger-goal-dialog-label">{{ t('chat.goal.objectiveLabel') }}</span>
        <el-input
          ref="objectiveInputRef"
          :model-value="objective"
          type="textarea"
          :rows="7"
          maxlength="4000"
          show-word-limit
          resize="none"
          :disabled="submitting"
          :placeholder="t('chat.goal.objectivePlaceholder')"
          @update:model-value="updateObjective"
          @keydown.ctrl.enter.prevent="start"
          @keydown.meta.enter.prevent="start"
        />
      </label>
      <div v-if="loading" class="messenger-goal-dialog-loading" role="status">
        <i class="fa-solid fa-spinner fa-spin" aria-hidden="true"></i>
        <span>{{ t('chat.goal.loadingCurrent') }}</span>
      </div>
    </div>

    <template #footer>
      <div class="messenger-dialog-footer">
        <el-button :disabled="submitting" @click="close">
          {{ t('common.cancel') }}
        </el-button>
        <el-button type="primary" :loading="submitting" :disabled="startDisabled" @click="start">
          {{ submitting ? t('common.loading') : t('chat.goal.start') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';
import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    visible?: boolean;
    objective?: string;
    loading?: boolean;
    submitting?: boolean;
  }>(),
  {
    visible: false,
    objective: '',
    loading: false,
    submitting: false
  }
);

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (event: 'update:objective', value: string): void;
  (event: 'start'): void;
}>();

const { t } = useI18n();
const objectiveInputRef = ref<{ focus?: () => void } | null>(null);

const startDisabled = computed(() => props.submitting || !String(props.objective || '').trim());

const updateVisible = (value: boolean) => {
  if (!value && props.submitting) {
    return;
  }
  emit('update:visible', value);
};

const updateObjective = (value: string | number) => {
  emit('update:objective', String(value || ''));
};

const close = () => {
  updateVisible(false);
};

const start = () => {
  if (startDisabled.value) {
    return;
  }
  emit('start');
};

const focusObjective = () => {
  void nextTick(() => {
    objectiveInputRef.value?.focus?.();
  });
};

defineExpose({
  focusObjective
});
</script>

<style scoped>
.messenger-goal-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.messenger-goal-dialog-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.messenger-goal-dialog-label {
  color: #343434;
  font-size: 13px;
  font-weight: 700;
}

.messenger-goal-dialog-loading {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: #68707d;
  font-size: 12px;
  min-height: 20px;
}

:deep(.messenger-goal-dialog .el-textarea__inner) {
  border-radius: 10px;
  border-color: #d9dee8;
  box-shadow: none;
  color: #202020;
  line-height: 1.55;
}

:deep(.messenger-goal-dialog .el-textarea__inner:focus) {
  border-color: var(--ui-accent);
  box-shadow: 0 0 0 2px rgba(246, 177, 76, 0.14);
}
</style>
