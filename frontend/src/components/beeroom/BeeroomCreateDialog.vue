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
        <div class="messenger-dialog-title">{{ t('beeroom.dialog.createTitle') }}</div>
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

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  },
  candidateAgents: {
    type: Array as () => AgentOption[],
    default: () => []
  }
});

const emit = defineEmits(['update:modelValue', 'submit']);
const { t } = useI18n();

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value)
});

const form = reactive({
  name: '',
  description: '',
  mother_agent_id: ''
});

const saving = computed(() => false);

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.mother_agent_id = '';
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

watch(
  () => visible.value,
  (value) => {
    if (value) {
      resetForm();
    }
  }
);
</script>
