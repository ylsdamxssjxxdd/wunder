<template>
  <div
    class="mz-dialog"
    :class="{ 'is-danger': entry.tone === 'danger', 'is-warning': entry.tone === 'warning' }"
    role="dialog"
    aria-modal="true"
    :aria-labelledby="titleId"
    @pointerdown.stop
    @keydown.esc.prevent="close"
  >
    <header class="mz-dialog-head">
      <span class="mz-dialog-badge" aria-hidden="true">
        <i :class="['fa-solid', badgeIcon]"></i>
      </span>
      <h2 :id="titleId" class="mz-dialog-title">{{ entry.title }}</h2>
      <button
        class="mz-dialog-close"
        type="button"
        :title="t('common.close')"
        :aria-label="t('common.close')"
        @click="close"
      >
        <i class="fa-solid fa-xmark" aria-hidden="true"></i>
      </button>
    </header>

    <div class="mz-dialog-body">
      <p v-if="entry.message" class="mz-dialog-message">{{ entry.message }}</p>

      <div v-if="entry.kind === 'prompt'" class="mz-dialog-field">
        <label v-if="entry.label" class="mz-dialog-label" :for="inputId">{{ entry.label }}</label>
        <textarea
          v-if="entry.multiline"
          :id="inputId"
          ref="fieldRef"
          v-model="entry.value"
          class="mz-dialog-input is-multiline"
          rows="4"
          :placeholder="entry.placeholder"
          :maxlength="entry.maxlength || undefined"
          @input="entry.error = ''"
          @keydown.ctrl.enter.prevent="confirm"
          @keydown.meta.enter.prevent="confirm"
        ></textarea>
        <input
          v-else
          :id="inputId"
          ref="fieldRef"
          v-model="entry.value"
          class="mz-dialog-input"
          type="text"
          autocomplete="off"
          :placeholder="entry.placeholder"
          :maxlength="entry.maxlength || undefined"
          @input="entry.error = ''"
          @keydown.enter.prevent="confirm"
          @focus="handleFocus"
        />
        <p v-if="entry.error" class="mz-dialog-error" role="alert">{{ entry.error }}</p>
      </div>
    </div>

    <footer class="mz-dialog-foot">
      <button
        v-if="!entry.alertOnly"
        class="mz-dialog-btn"
        type="button"
        @click="cancel"
      >
        {{ entry.cancelText || t('common.cancel') }}
      </button>
      <button
        ref="confirmRef"
        class="mz-dialog-btn is-primary"
        :class="{ 'is-danger': entry.tone === 'danger' }"
        type="button"
        @click="confirm"
      >
        {{ entry.confirmText || t('common.confirm') }}
      </button>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from '@/i18n';
import { settleMzDialog, type MzDialogEntry } from '@/utils/mzDialog';

const props = defineProps<{ entry: MzDialogEntry }>();

const { t } = useI18n();

const fieldRef = ref<HTMLInputElement | HTMLTextAreaElement | null>(null);
const confirmRef = ref<HTMLButtonElement | null>(null);

const titleId = computed(() => `mz-dialog-title-${props.entry.id}`);
const inputId = computed(() => `mz-dialog-input-${props.entry.id}`);
const badgeIcon = computed(() => {
  if (props.entry.tone === 'danger' || props.entry.tone === 'warning') return 'fa-triangle-exclamation';
  return props.entry.kind === 'prompt' ? 'fa-pen' : 'fa-circle-question';
});

const handleFocus = (event: FocusEvent) => {
  if (!props.entry.selectOnFocus) return;
  // 重命名类场景：聚焦即全选，直接输入覆盖旧值，不必先清空。
  (event.target as HTMLInputElement | null)?.select?.();
};

const close = () => settleMzDialog(props.entry.id, { action: 'close', value: '' });
const cancel = () => settleMzDialog(props.entry.id, { action: 'cancel', value: '' });

const confirm = () => {
  if (props.entry.kind === 'prompt') {
    const validator = props.entry.validator;
    if (validator) {
      const verdict = validator(props.entry.value ?? '');
      if (verdict !== true) {
        props.entry.error = typeof verdict === 'string' && verdict ? verdict : t('common.invalidInput');
        fieldRef.value?.focus();
        return;
      }
    }
  }
  settleMzDialog(props.entry.id, { action: 'confirm', value: props.entry.value ?? '' });
};

onMounted(() => {
  const field = fieldRef.value;
  if (field) {
    field.focus();
    if (props.entry.selectOnFocus && field instanceof HTMLInputElement) field.select();
    return;
  }
  confirmRef.value?.focus();
});
</script>
