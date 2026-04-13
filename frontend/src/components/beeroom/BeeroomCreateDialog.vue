<template>
  <el-dialog
    v-model="visible"
    class="messenger-dialog"
    width="560px"
    top="10vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-title">{{ dialogTitle }}</div>
        <button class="messenger-dialog-close" type="button" @click="visible = false">&times;</button>
      </div>
    </template>

    <div class="messenger-dialog-body">
      <el-form :model="form" label-position="top" class="messenger-form">
        <el-form-item :label="t('beeroom.dialog.name')">
          <el-input v-model="form.name" :placeholder="t('beeroom.dialog.namePlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('beeroom.dialog.description')">
          <el-input
            v-model="form.description"
            type="textarea"
            :rows="4"
            :placeholder="t('beeroom.dialog.descriptionPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="t('beeroom.dialog.motherAgent')">
          <el-select
            v-model="form.mother_agent_id"
            clearable
            filterable
            class="messenger-form-full"
            :placeholder="t('beeroom.dialog.motherAgentPlaceholder')"
          >
            <el-option :label="t('beeroom.dialog.noMother')" value="" />
            <el-option
              v-for="agent in candidateAgents"
              :key="agent.id"
              :label="agent.name || agent.id"
              :value="agent.id"
            />
          </el-select>
        </el-form-item>
      </el-form>
    </div>

    <template #footer>
      <div class="messenger-dialog-footer">
        <el-button
          v-if="showDeleteAction"
          type="danger"
          plain
          :loading="deleting"
          :disabled="saving"
          @click="handleDelete"
        >
          {{ t('common.delete') }}
        </el-button>
        <el-button @click="visible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="handleSubmit">
          {{ saving ? t('common.loading') : t('common.save') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';

type AgentOption = {
  id: string;
  name?: string;
};

type DialogMode = 'create' | 'edit';

type BeeroomDialogGroup = {
  group_id?: string;
  hive_id?: string;
  name?: string;
  description?: string;
  mother_agent_id?: string | null;
  is_default?: boolean;
};

const props = withDefaults(
  defineProps<{
    modelValue?: boolean;
    candidateAgents?: AgentOption[];
    mode?: DialogMode;
    initialGroup?: BeeroomDialogGroup | null;
    saving?: boolean;
    deleting?: boolean;
  }>(),
  {
    modelValue: false,
    candidateAgents: () => [],
    mode: 'create',
    initialGroup: null,
    saving: false,
    deleting: false
  }
);

const emit = defineEmits<{
  (event: 'update:modelValue', value: boolean): void;
  (
    event: 'submit',
    payload: { name: string; description: string; mother_agent_id: string }
  ): void;
  (event: 'delete'): void;
}>();
const { t } = useI18n();

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value)
});

const isEditMode = computed(() => props.mode === 'edit');
const dialogTitle = computed(() =>
  t(isEditMode.value ? 'beeroom.dialog.editTitle' : 'beeroom.dialog.createTitle')
);
const showDeleteAction = computed(() => isEditMode.value && !props.initialGroup?.is_default);

const form = reactive({
  name: '',
  description: '',
  mother_agent_id: ''
});

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.mother_agent_id = '';
};

const syncForm = () => {
  if (!isEditMode.value) {
    resetForm();
    return;
  }
  form.name = String(props.initialGroup?.name || '').trim();
  form.description = String(props.initialGroup?.description || '');
  form.mother_agent_id = String(props.initialGroup?.mother_agent_id || '').trim();
};

const handleSubmit = () => {
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning(t('beeroom.dialog.nameRequired'));
    return;
  }
  emit('submit', {
    name,
    description: String(form.description || '').trim(),
    mother_agent_id: String(form.mother_agent_id || '').trim()
  });
};

const handleDelete = () => {
  if (!showDeleteAction.value) {
    return;
  }
  emit('delete');
};

watch(
  [() => visible.value, () => props.initialGroup, () => props.mode],
  ([value]) => {
    if (value) {
      syncForm();
    }
  },
  { deep: true }
);
</script>
