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
      <el-form label-position="top" class="agent-quick-create-form">
        <el-form-item :label="t('messenger.agentCreate.copyFrom')">
          <el-select
            v-model="selectedCopyFromAgentId"
            filterable
            class="messenger-form-full"
            :placeholder="t('messenger.agentCreate.copyFromPlaceholder')"
          >
            <el-option
              v-for="agent in copyFromAgents"
              :key="agent.id"
              :label="agent.name || agent.id"
              :value="agent.id"
            />
          </el-select>
        </el-form-item>
        <div class="agent-quick-create-hint">{{ t('messenger.agentQuickCreate.prompt') }}</div>
      </el-form>
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
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

type AgentOption = {
  id: string;
  name: string;
};

const props = withDefaults(
  defineProps<{
    modelValue?: boolean;
    creating?: boolean;
    copyFromAgents?: AgentOption[];
  }>(),
  {
    modelValue: false,
    creating: false,
    copyFromAgents: () => []
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

const selectedCopyFromAgentId = ref(DEFAULT_AGENT_KEY);

const resolveInitialCopyFromAgentId = (): string => {
  const availableIds = new Set(
    (Array.isArray(props.copyFromAgents) ? props.copyFromAgents : [])
      .map((agent) => String(agent?.id || '').trim())
      .filter(Boolean)
  );
  if (availableIds.has(DEFAULT_AGENT_KEY)) {
    return DEFAULT_AGENT_KEY;
  }
  return Array.from(availableIds)[0] || DEFAULT_AGENT_KEY;
};

const handleSubmit = () => {
  if (props.creating) return;
  emit('submit', {
    copy_from_agent_id: String(selectedCopyFromAgentId.value || '').trim() || resolveInitialCopyFromAgentId()
  });
};

watch(
  () => visible.value,
  (value: boolean) => {
    if (!value) return;
    selectedCopyFromAgentId.value = resolveInitialCopyFromAgentId();
  }
);
</script>

<style scoped>
.agent-quick-create-form {
  width: 100%;
}

.agent-quick-create-hint {
  font-size: 13px;
  color: #666666;
  line-height: 1.6;
}
</style>
