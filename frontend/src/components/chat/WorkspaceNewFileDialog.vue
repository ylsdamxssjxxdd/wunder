<template>
  <el-dialog
    v-model="dialogVisible"
    width="520px"
    top="clamp(24px, 8vh, 72px)"
    class="workspace-dialog workspace-dialog--new-file"
    append-to-body
    :title="t('workspace.createFile.dialogTitle')"
    @closed="handleClosed"
  >
    <div class="workspace-new-file-body">
      <label class="workspace-new-file-field">
        <span class="workspace-new-file-label">{{ t('workspace.createFile.typeLabel') }}</span>
        <div class="workspace-new-file-type-grid" role="listbox" :aria-label="t('workspace.createFile.typeLabel')">
          <button
            v-for="option in fileTypeOptions"
            :key="option.id"
            type="button"
            class="workspace-new-file-type"
            :class="{ active: selectedTypeId === option.id }"
            @click="selectType(option.id)"
          >
            <span class="workspace-new-file-type-icon" aria-hidden="true">
              <img :src="option.icon" :alt="option.label" />
            </span>
            <span class="workspace-new-file-type-copy">
              <span class="workspace-new-file-type-name">{{ option.label }}</span>
              <span class="workspace-new-file-type-ext">{{ option.extensionLabel }}</span>
            </span>
          </button>
        </div>
      </label>

      <label class="workspace-new-file-field">
        <span class="workspace-new-file-label">{{ t('workspace.createFile.nameLabel') }}</span>
        <input
          ref="nameInputRef"
          v-model="fileName"
          class="workspace-new-file-input"
          type="text"
          :placeholder="t('workspace.createFile.placeholder')"
          @keydown.enter.prevent="confirm"
        />
      </label>

      <div class="workspace-new-file-hint">{{ selectedTypeHint }}</div>
    </div>

    <template #footer>
      <button class="workspace-btn secondary" type="button" @click="dialogVisible = false">
        {{ t('common.cancel') }}
      </button>
      <button class="workspace-btn workspace-btn--primary" type="button" @click="confirm">
        {{ t('common.confirm') }}
      </button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

export type WorkspaceNewFileTemplate = {
  id: string;
  label: string;
  extension: string;
  extensionLabel: string;
  icon: string;
  hint: string;
  defaultName: string;
  content: string;
};

const props = defineProps<{
  visible: boolean;
  fileTypeOptions: WorkspaceNewFileTemplate[];
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
  (event: 'confirm', payload: { name: string; content: string; typeId: string }): void;
}>();

const { t } = useI18n();
const nameInputRef = ref<HTMLInputElement | null>(null);
const selectedTypeId = ref('');
const fileName = ref('');

const dialogVisible = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const selectedType = computed(() => {
  const targetId = selectedTypeId.value || props.fileTypeOptions[0]?.id || '';
  return props.fileTypeOptions.find((option) => option.id === targetId) || props.fileTypeOptions[0] || null;
});

const selectedTypeHint = computed(() => selectedType.value?.hint || '');

watch(
  () => selectedType.value,
  (option) => {
    if (!option) return;
    fileName.value = option.defaultName;
  }
);

const applyTypeDefaults = (typeId: string) => {
  const option = props.fileTypeOptions.find((item) => item.id === typeId);
  if (!option) return;
  selectedTypeId.value = option.id;
  fileName.value = option.defaultName;
  void nextTick(() => {
    nameInputRef.value?.focus();
    nameInputRef.value?.select();
  });
};

const selectType = (typeId: string) => {
  applyTypeDefaults(typeId);
};

const confirm = () => {
  const option = selectedType.value;
  if (!option) return;
  emit('confirm', {
    name: String(fileName.value || '').trim(),
    content: option.content,
    typeId: option.id
  });
};

const handleClosed = () => {
  emit('update:visible', false);
};

watch(
  () => props.visible,
  (visible) => {
    if (!visible) return;
    applyTypeDefaults(props.fileTypeOptions[0]?.id || '');
  }
);
</script>

<style scoped>
.workspace-new-file-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.workspace-new-file-field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.workspace-new-file-label {
  font-size: 13px;
  font-weight: 600;
  color: var(--chat-text, var(--el-text-color-primary, #1f2937));
}

.workspace-new-file-type-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.workspace-new-file-type {
  display: flex;
  align-items: center;
  gap: 10px;
  min-height: 58px;
  padding: 10px 12px;
  border: 1px solid rgba(var(--ui-accent-rgb, 77, 216, 255), 0.18);
  border-radius: 10px;
  background: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.04);
  color: inherit;
  cursor: pointer;
  text-align: left;
  transition: border-color 0.18s ease, background 0.18s ease, transform 0.18s ease;
}

.workspace-new-file-type:hover,
.workspace-new-file-type:focus-visible {
  border-color: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.42);
  background: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.1);
  transform: translateY(-1px);
  outline: none;
}

.workspace-new-file-type.active {
  border-color: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.52);
  background: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.14);
}

.workspace-new-file-type-icon {
  flex: 0 0 28px;
  width: 28px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.workspace-new-file-type-icon img {
  width: 24px;
  height: 24px;
  object-fit: contain;
}

.workspace-new-file-type-copy {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.workspace-new-file-type-name {
  font-size: 13px;
  font-weight: 600;
}

.workspace-new-file-type-ext {
  font-size: 12px;
  color: var(--chat-muted, var(--el-text-color-secondary, #64748b));
}

.workspace-new-file-input {
  width: 100%;
  min-height: 40px;
  padding: 0 12px;
  border: 1px solid rgba(var(--ui-accent-rgb, 77, 216, 255), 0.2);
  border-radius: 10px;
  background: transparent;
  color: inherit;
  font: inherit;
}

.workspace-new-file-input:focus {
  border-color: rgba(var(--ui-accent-rgb, 77, 216, 255), 0.48);
  outline: none;
}

.workspace-new-file-hint {
  font-size: 12px;
  line-height: 1.6;
  color: var(--chat-muted, var(--el-text-color-secondary, #64748b));
}

@media (max-width: 640px) {
  .workspace-new-file-type-grid {
    grid-template-columns: 1fr;
  }
}
</style>
