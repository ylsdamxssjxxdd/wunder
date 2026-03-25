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
      <div class="agent-quick-create-mode-switch">
        <button
          class="agent-quick-create-mode-btn"
          :class="{ active: mode === 'blank' }"
          type="button"
          @click="mode = 'blank'"
        >
          {{ t('messenger.agentQuickCreate.blank') }}
        </button>
        <button
          class="agent-quick-create-mode-btn"
          :class="{ active: mode === 'clone' }"
          type="button"
          @click="mode = 'clone'"
        >
          {{ t('messenger.agentQuickCreate.clone') }}
        </button>
      </div>
      <el-select
        v-if="mode === 'clone'"
        v-model="copyFromAgentId"
        clearable
        filterable
        class="agent-quick-create-select"
        :placeholder="t('messenger.agentQuickCreate.clonePlaceholder')"
      >
        <el-option
          v-for="agent in copyFromOptions"
          :key="agent.id"
          :label="agent.name || agent.id"
          :value="agent.id"
        />
      </el-select>
    </div>
    <template #footer>
      <div class="messenger-dialog-footer">
        <el-button :disabled="creating" @click="visible = false">{{ t('common.cancel') }}</el-button>
        <el-button
          type="primary"
          :loading="creating"
          :disabled="!canSubmit"
          @click="handleSubmit"
        >
          {{ t('common.create') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

type AgentLike = {
  id: string;
  name?: string;
};

const props = withDefaults(
  defineProps<{
    modelValue?: boolean;
    creating?: boolean;
    copyFromAgents?: AgentLike[];
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

const mode = ref<'blank' | 'clone'>('blank');
const copyFromAgentId = ref('');

const copyFromOptions = computed<AgentLike[]>(() => {
  const seen = new Set<string>();
  return (Array.isArray(props.copyFromAgents) ? props.copyFromAgents : [])
    .map((item) => ({
      id: String(item?.id || '').trim(),
      name: String(item?.name || item?.id || '').trim()
    }))
    .filter((item) => {
      if (!item.id || seen.has(item.id)) return false;
      seen.add(item.id);
      return true;
    });
});

const canSubmit = computed(
  () => mode.value === 'blank' || Boolean(String(copyFromAgentId.value || '').trim())
);

const handleSubmit = () => {
  if (!canSubmit.value || props.creating) return;
  const payload: { copy_from_agent_id?: string } = {};
  if (mode.value === 'clone') {
    const copyId = String(copyFromAgentId.value || '').trim();
    if (copyId) {
      payload.copy_from_agent_id = copyId;
    }
  }
  emit('submit', payload);
};

watch(
  () => visible.value,
  (value) => {
    if (!value) return;
    mode.value = 'blank';
    copyFromAgentId.value = '';
  }
);
</script>

<style scoped>
.agent-quick-create-prompt {
  font-size: 13px;
  color: #666666;
}

.agent-quick-create-mode-switch {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
  margin-top: 12px;
}

.agent-quick-create-mode-btn {
  border: 1px solid #dadada;
  border-radius: 10px;
  padding: 8px 10px;
  background: #ffffff;
  color: #4b5563;
  cursor: pointer;
  font-size: 12px;
  font-weight: 600;
  transition: all 0.18s ease;
}

.agent-quick-create-mode-btn.active {
  border-color: var(--ui-accent, #2563eb);
  background: rgba(var(--ui-accent-rgb, 37, 99, 235), 0.12);
  color: var(--ui-accent-deep, #1d4ed8);
}

.agent-quick-create-select {
  width: 100%;
  margin-top: 12px;
}
</style>
