<template>
  <div class="chat-goal-composer">
    <div class="chat-goal-composer-head">
      <div class="chat-goal-composer-title">
        <i class="fa-solid fa-bullseye" aria-hidden="true"></i>
        <span>{{ t('chat.goal.timelineBadge') }}</span>
      </div>
      <div class="chat-goal-composer-head-actions">
        <div v-if="statusLabel" class="chat-goal-composer-status">
          {{ statusLabel }}
        </div>
        <button
          v-if="showCancelAction"
          class="chat-goal-composer-close"
          type="button"
          :title="t('common.cancel')"
          :aria-label="t('common.cancel')"
          @click="$emit('cancel')"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
    </div>

    <div class="chat-goal-composer-body">
      <label class="chat-goal-composer-field">
        <span class="chat-goal-composer-label">{{ t('chat.goal.objectiveLabel') }}</span>
        <textarea
          ref="textareaRef"
          class="chat-goal-composer-textarea"
          :value="objective"
          :maxlength="4000"
          :placeholder="t('chat.goal.objectivePlaceholder')"
          :disabled="submitting"
          rows="5"
          @input="updateObjective"
          @keydown.ctrl.enter.prevent="submit"
          @keydown.meta.enter.prevent="submit"
        ></textarea>
      </label>

      <div v-if="loading" class="chat-goal-composer-meta">
        <span class="chat-goal-composer-loading">
          <i class="fa-solid fa-spinner fa-spin" aria-hidden="true"></i>
          {{ t('chat.goal.loadingCurrent') }}
        </span>
      </div>
    </div>

    <div class="chat-goal-composer-footer">
      <button
        class="chat-goal-composer-btn chat-goal-composer-btn--primary"
        type="button"
        :disabled="submitDisabled"
        @click="submit"
      >
        {{ submitting ? t('common.loading') : actionLabel }}
      </button>
      <button
        class="chat-goal-composer-btn chat-goal-composer-btn--danger"
        type="button"
        :disabled="secondaryDisabled"
        @click="handleSecondaryAction"
      >
        {{ secondaryActionLabel }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    visible?: boolean;
    objective?: string;
    loading?: boolean;
    submitting?: boolean;
    active?: boolean;
    status?: string;
  }>(),
  {
    visible: false,
    objective: '',
    loading: false,
    submitting: false,
    active: false,
    status: ''
  }
);

const emit = defineEmits<{
  (event: 'update:objective', value: string): void;
  (event: 'submit'): void;
  (event: 'stop'): void;
  (event: 'cancel'): void;
}>();

const { t } = useI18n();
const textareaRef = ref<HTMLTextAreaElement | null>(null);

const normalizedStatus = computed(() => String(props.status || '').trim().toLowerCase());

const statusLabel = computed(() => {
  if (normalizedStatus.value === 'active') return t('chat.goal.statusActive');
  if (normalizedStatus.value === 'paused') return t('chat.goal.statusPaused');
  if (normalizedStatus.value === 'budget_limited') return t('chat.goal.statusBudgetLimited');
  if (normalizedStatus.value === 'complete') return t('chat.goal.statusComplete');
  return '';
});

const actionLabel = computed(() => {
  if (props.active) {
    return String(props.objective || '').trim() ? t('chat.goal.update') : t('chat.goal.start');
  }
  return t('chat.goal.start');
});

const showCancelAction = computed(() => !props.active);

const submitDisabled = computed(
  () => props.submitting || props.loading || !String(props.objective || '').trim()
);
const secondaryDisabled = computed(
  () => props.submitting || (!props.active && !String(props.objective || '').trim() && !showCancelAction.value)
);
const secondaryActionLabel = computed(() => (props.active ? t('common.stop') : t('common.cancel')));

const updateObjective = (event: Event) => {
  const target = event.target as HTMLTextAreaElement | null;
  emit('update:objective', String(target?.value || ''));
};

const submit = () => {
  if (submitDisabled.value) return;
  emit('submit');
};

const handleSecondaryAction = () => {
  if (secondaryDisabled.value) return;
  if (props.active) {
    emit('stop');
    return;
  }
  emit('cancel');
};

const focusTextarea = () => {
  void nextTick(() => {
    textareaRef.value?.focus();
  });
};

watch(
  () => props.visible,
  (visible) => {
    if (visible) {
      focusTextarea();
    }
  }
);

defineExpose({
  focusTextarea
});
</script>

<style scoped>
.chat-goal-composer {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
}

.chat-goal-composer-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.chat-goal-composer-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.chat-goal-composer-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: #2d2d2d;
  font-size: 13px;
  font-weight: 700;
}

.chat-goal-composer-status {
  color: #8b5c2a;
  font-size: 12px;
  font-weight: 600;
}

.chat-goal-composer-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: #7a828f;
  cursor: pointer;
}

.chat-goal-composer-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.chat-goal-composer-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.chat-goal-composer-label {
  color: #5e6674;
  font-size: 12px;
  font-weight: 600;
}

.chat-goal-composer-textarea {
  width: 100%;
  min-height: 112px;
  resize: vertical;
  border: 1px solid #d9dee8;
  border-radius: 12px;
  background: #fffdf7;
  color: #202020;
  padding: 12px 14px;
  line-height: 1.55;
  font: inherit;
  outline: none;
}

.chat-goal-composer-textarea:focus {
  border-color: var(--ui-accent);
  box-shadow: 0 0 0 2px rgba(246, 177, 76, 0.14);
}

.chat-goal-composer-meta {
  min-height: 18px;
  color: #6d7482;
  font-size: 12px;
}

.chat-goal-composer-loading {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.chat-goal-composer-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.chat-goal-composer-btn {
  min-width: 88px;
  border: none;
  border-radius: 10px;
  padding: 10px 14px;
  font: inherit;
  cursor: pointer;
}

.chat-goal-composer-btn:disabled {
  opacity: 0.55;
  cursor: default;
}

.chat-goal-composer-btn--primary {
  background: linear-gradient(135deg, #f6b14c, #ee8a38);
  color: #1c1408;
  font-weight: 700;
}

.chat-goal-composer-btn--danger {
  background: #f4ece6;
  color: #8a3f23;
  font-weight: 600;
}
</style>
