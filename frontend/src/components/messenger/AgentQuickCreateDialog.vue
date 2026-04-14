<template>
  <el-dialog
    v-model="visible"
    class="messenger-dialog messenger-agent-quick-create-dialog"
    width="460px"
    top="14vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-title">{{ t('messenger.agentQuickCreate.title') }}</div>
        <button class="messenger-dialog-close" type="button" @click="visible = false">&times;</button>
      </div>
    </template>
    <div class="messenger-dialog-body">
      <div class="agent-quick-create-prompt">{{ t('messenger.agentQuickCreate.prompt') }}</div>
    </div>
    <template #footer>
      <div class="messenger-dialog-footer">
        <el-button :disabled="creating" @click="visible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="creating" :disabled="creating" @click="handleSubmit">
          {{ t('common.create') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

const props = withDefaults(
  defineProps<{
    modelValue?: boolean;
    creating?: boolean;
  }>(),
  {
    modelValue: false,
    creating: false
  }
);

const emit = defineEmits<{
  (event: 'update:modelValue', value: boolean): void;
  (event: 'submit', payload: { copy_from_agent_id?: string }): void;
}>();

const { t } = useI18n();

const visible = computed({
  get: () => props.modelValue,
  set: (value: boolean) => emit('update:modelValue', value)
});

const handleSubmit = () => {
  if (props.creating) return;
  emit('submit', { copy_from_agent_id: DEFAULT_AGENT_KEY });
};
</script>

<style scoped>
.agent-quick-create-prompt {
  font-size: 13px;
  color: #666666;
  line-height: 1.6;
}
</style>
