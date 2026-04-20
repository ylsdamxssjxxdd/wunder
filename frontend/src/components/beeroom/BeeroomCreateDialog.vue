<template>
  <el-dialog
    v-model="visible"
    class="messenger-dialog beeroom-create-dialog"
    width="560px"
    top="calc(var(--desktop-window-chrome-height, 36px) + 12px)"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-title">{{ dialogTitle }}</div>
        <button
          class="messenger-dialog-close"
          type="button"
          :aria-label="t('common.close')"
          :title="t('common.close')"
          @click.stop="visible = false"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
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
        <el-form-item :label="t('beeroom.dialog.workerAgents')">
          <div class="beeroom-worker-picker" :class="{ 'is-disabled': disableWorkerSelection }">
            <div class="beeroom-worker-picker-toolbar">
              <span class="beeroom-worker-picker-count">
                {{ t('beeroom.dialog.workerSelectedCount', { count: selectedWorkerAgents.length }) }}
              </span>
              <button
                v-if="!disableWorkerSelection && form.member_agent_ids.length"
                type="button"
                class="beeroom-worker-picker-clear"
                @click="clearWorkerAgents"
              >
                {{ t('common.clear') }}
              </button>
            </div>

            <div v-if="selectedWorkerAgents.length" class="beeroom-worker-picker-tags">
              <button
                v-for="agent in selectedWorkerAgents"
                :key="`selected-${agent.id}`"
                type="button"
                class="beeroom-worker-chip"
                :disabled="disableWorkerSelection"
                @click="toggleWorkerAgent(agent.id)"
              >
                <span class="beeroom-worker-chip-label">{{ agent.name || agent.id }}</span>
                <span class="beeroom-worker-chip-close" aria-hidden="true">
                  <i class="fa-solid fa-xmark"></i>
                </span>
              </button>
            </div>

            <div v-else class="beeroom-worker-picker-empty-state">
              {{ t('beeroom.dialog.workerAgentsPlaceholder') }}
            </div>

            <el-input
              v-model="workerSearchQuery"
              class="messenger-form-full beeroom-worker-picker-search"
              :disabled="disableWorkerSelection"
              :placeholder="t('beeroom.dialog.workerSearchPlaceholder')"
              clearable
            />

            <div class="beeroom-worker-picker-panel">
              <button
                v-for="agent in filteredWorkerAgents"
                :key="agent.id"
                type="button"
                class="beeroom-worker-option"
                :class="{ 'is-selected': isWorkerSelected(agent.id) }"
                :disabled="disableWorkerSelection"
                @click="toggleWorkerAgent(agent.id)"
              >
                <span class="beeroom-worker-option-check" aria-hidden="true">
                  <i class="fa-solid fa-check"></i>
                </span>
                <span class="beeroom-worker-option-body">
                  <span class="beeroom-worker-option-name">{{ agent.name || agent.id }}</span>
                  <span class="beeroom-worker-option-id">{{ agent.id }}</span>
                </span>
              </button>
              <div v-if="!filteredWorkerAgents.length" class="beeroom-worker-picker-empty">
                {{ t('beeroom.dialog.workerSearchEmpty') }}
              </div>
            </div>
          </div>
          <div v-if="disableWorkerSelection" class="beeroom-dialog-hint">
            {{ t('beeroom.dialog.defaultWorkerHint') }}
          </div>
          <div v-else class="beeroom-dialog-hint">
            {{ t('beeroom.dialog.workerAgentHint') }}
          </div>
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
import { computed, reactive, ref, watch } from 'vue';
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
  member_agent_ids?: string[];
  members?: Array<{ agent_id?: string; id?: string }>;
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
    payload: {
      name: string;
      description: string;
      mother_agent_id: string;
      member_agent_ids: string[];
    }
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
const disableWorkerSelection = computed(() => isEditMode.value && Boolean(props.initialGroup?.is_default));

const form = reactive({
  name: '',
  description: '',
  mother_agent_id: '',
  member_agent_ids: [] as string[]
});
const workerSearchQuery = ref('');

const normalizeAgentIds = (value: unknown): string[] => {
  const list = Array.isArray(value) ? value : [];
  const unique = new Set<string>();
  list.forEach((item) => {
    if (typeof item === 'string') {
      const id = item.trim();
      if (id) {
        unique.add(id);
      }
      return;
    }
    if (!item || typeof item !== 'object') {
      return;
    }
    const source = item as { agent_id?: string; id?: string };
    const id = String(source.agent_id || source.id || '').trim();
    if (id) {
      unique.add(id);
    }
  });
  return Array.from(unique);
};

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.mother_agent_id = '';
  form.member_agent_ids = [];
  workerSearchQuery.value = '';
};

const syncForm = () => {
  if (!isEditMode.value) {
    resetForm();
    return;
  }
  form.name = String(props.initialGroup?.name || '').trim();
  form.description = String(props.initialGroup?.description || '');
  form.mother_agent_id = String(props.initialGroup?.mother_agent_id || '').trim();
  form.member_agent_ids = normalizeAgentIds(
    props.initialGroup?.member_agent_ids?.length
      ? props.initialGroup.member_agent_ids
      : props.initialGroup?.members
  ).filter((agentId) => agentId !== form.mother_agent_id);
  workerSearchQuery.value = '';
};

const lowerCaseIncludes = (source: string, keyword: string) =>
  source.toLocaleLowerCase().includes(keyword.toLocaleLowerCase());

const candidateWorkerAgents = computed(() =>
  (Array.isArray(props.candidateAgents) ? props.candidateAgents : []).filter(
    (agent) => String(agent.id || '').trim() && String(agent.id || '').trim() !== form.mother_agent_id
  )
);

const selectedWorkerAgents = computed(() => {
  const agentMap = new Map(
    (Array.isArray(props.candidateAgents) ? props.candidateAgents : []).map((agent) => [
      String(agent.id || '').trim(),
      agent
    ])
  );
  return form.member_agent_ids
    .map((agentId) => {
      const id = String(agentId || '').trim();
      if (!id || id === form.mother_agent_id) return null;
      const matched = agentMap.get(id);
      return matched || { id, name: id };
    })
    .filter((agent): agent is AgentOption => Boolean(agent));
});

const filteredWorkerAgents = computed(() => {
  const keyword = String(workerSearchQuery.value || '').trim();
  return candidateWorkerAgents.value
    .filter((agent) => {
      if (!keyword) return true;
      return lowerCaseIncludes(String(agent.name || ''), keyword) || lowerCaseIncludes(String(agent.id || ''), keyword);
    })
    .sort((left, right) => {
      const leftSelected = Number(form.member_agent_ids.includes(left.id));
      const rightSelected = Number(form.member_agent_ids.includes(right.id));
      if (leftSelected !== rightSelected) return rightSelected - leftSelected;
      const leftName = String(left.name || left.id || '').trim();
      const rightName = String(right.name || right.id || '').trim();
      return leftName.localeCompare(rightName, 'zh-Hans-CN');
    });
});

const isWorkerSelected = (agentId: string) => form.member_agent_ids.includes(String(agentId || '').trim());

const toggleWorkerAgent = (agentId: string) => {
  if (disableWorkerSelection.value) return;
  const normalizedAgentId = String(agentId || '').trim();
  if (!normalizedAgentId || normalizedAgentId === form.mother_agent_id) return;
  form.member_agent_ids = isWorkerSelected(normalizedAgentId)
    ? form.member_agent_ids.filter((item) => item !== normalizedAgentId)
    : normalizeAgentIds([...form.member_agent_ids, normalizedAgentId]);
};

const clearWorkerAgents = () => {
  if (disableWorkerSelection.value) return;
  form.member_agent_ids = [];
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
    mother_agent_id: String(form.mother_agent_id || '').trim(),
    member_agent_ids: normalizeAgentIds(form.member_agent_ids)
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
      return;
    }
    workerSearchQuery.value = '';
  },
  { deep: true }
);

watch(
  () => form.mother_agent_id,
  (value) => {
    const normalizedMotherId = String(value || '').trim();
    if (!normalizedMotherId) return;
    form.member_agent_ids = form.member_agent_ids.filter((agentId) => agentId !== normalizedMotherId);
  }
);
</script>

<style scoped>
:deep(.beeroom-create-dialog.el-dialog) {
  max-height: calc(var(--app-viewport-height, 100vh) - 24px);
  box-sizing: border-box;
}

:deep(.beeroom-create-dialog .el-dialog__body) {
  max-height: calc(var(--app-viewport-height, 100vh) - 210px);
  overflow: auto;
  box-sizing: border-box;
}

.messenger-dialog-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition:
    border-color 160ms ease,
    background-color 160ms ease,
    color 160ms ease,
    transform 160ms ease,
    box-shadow 160ms ease;
}

.messenger-dialog-close:hover,
.messenger-dialog-close:focus-visible {
  border-color: rgba(var(--ui-accent-rgb), 0.26);
  background: rgba(var(--ui-accent-rgb), 0.1);
  color: var(--ui-accent-hover, #ff8430);
  box-shadow: 0 0 0 3px rgba(var(--ui-accent-rgb), 0.12);
  outline: none;
}

.messenger-dialog-close:active {
  transform: translateY(1px) scale(0.98);
}

.beeroom-worker-picker {
  display: grid;
  gap: 10px;
  padding: 12px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.14);
  border-radius: 16px;
  background:
    linear-gradient(180deg, rgba(var(--ui-accent-rgb), 0.08), rgba(var(--ui-accent-rgb), 0.03)),
    var(--dark-panel, #ffffff);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.18);
}

.beeroom-worker-picker.is-disabled {
  opacity: 0.8;
  filter: saturate(0.92);
}

.beeroom-worker-picker-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.beeroom-worker-picker-count {
  font-size: 12px;
  font-weight: 600;
  color: var(--el-text-color-primary, var(--dark-text, #18181c));
}

.beeroom-worker-picker-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 28px;
  padding: 0 10px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.16);
  border-radius: 999px;
  background: rgba(var(--ui-accent-rgb), 0.06);
  color: var(--ui-accent, #ff9448);
  font-size: 12px;
  transition:
    border-color 160ms ease,
    background-color 160ms ease,
    color 160ms ease,
    transform 160ms ease;
  cursor: pointer;
}

.beeroom-worker-picker-clear:hover,
.beeroom-worker-picker-clear:focus-visible {
  border-color: rgba(var(--ui-accent-rgb), 0.24);
  background: rgba(var(--ui-accent-rgb), 0.12);
  color: var(--ui-accent-hover, #ff8430);
  outline: none;
}

.beeroom-worker-picker-clear:active {
  transform: translateY(1px);
}

.beeroom-worker-picker-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.beeroom-worker-chip {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-height: 30px;
  padding: 0 10px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.18);
  border-radius: 999px;
  background: rgba(var(--ui-accent-rgb), 0.1);
  color: var(--el-text-color-primary, var(--dark-text, #18181c));
  cursor: pointer;
  transition:
    border-color 160ms ease,
    background-color 160ms ease,
    color 160ms ease,
    transform 160ms ease;
}

.beeroom-worker-chip:hover,
.beeroom-worker-chip:focus-visible {
  border-color: rgba(var(--ui-accent-rgb), 0.28);
  background: rgba(var(--ui-accent-rgb), 0.16);
  outline: none;
}

.beeroom-worker-chip:active {
  transform: translateY(1px);
}

.beeroom-worker-chip:disabled {
  cursor: default;
  transform: none;
}

.beeroom-worker-chip-label {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}

.beeroom-worker-chip-close {
  color: rgba(var(--ui-accent-rgb), 0.88);
  font-size: 11px;
}

.beeroom-worker-picker-empty-state {
  padding: 10px 12px;
  border: 1px dashed rgba(var(--ui-accent-rgb), 0.16);
  border-radius: 12px;
  background: rgba(var(--ui-accent-rgb), 0.04);
  color: var(--el-text-color-secondary, var(--dark-muted, #808080));
  font-size: 12px;
}

.beeroom-worker-picker-search :deep(.el-input__wrapper) {
  background: rgba(var(--ui-accent-rgb), 0.03);
  box-shadow: 0 0 0 1px rgba(var(--ui-accent-rgb), 0.12) inset;
}

.beeroom-worker-picker-panel {
  display: grid;
  gap: 8px;
  max-height: 220px;
  padding-right: 4px;
  overflow: auto;
}

.beeroom-worker-option {
  display: grid;
  grid-template-columns: auto minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.12);
  border-radius: 14px;
  background: rgba(var(--ui-accent-rgb), 0.05);
  color: var(--el-text-color-primary, var(--dark-text, #18181c));
  text-align: left;
  cursor: pointer;
  transition: border-color 160ms ease, background 160ms ease, transform 160ms ease;
}

.beeroom-worker-option:hover,
.beeroom-worker-option:focus-visible {
  border-color: rgba(var(--ui-accent-rgb), 0.22);
  background: rgba(var(--ui-accent-rgb), 0.1);
  transform: translateY(-1px);
  outline: none;
}

.beeroom-worker-option.is-selected {
  border-color: rgba(var(--ui-accent-rgb), 0.32);
  background: rgba(var(--ui-accent-rgb), 0.14);
}

.beeroom-worker-option:disabled {
  cursor: default;
  transform: none;
}

.beeroom-worker-option-check {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.16);
  border-radius: 6px;
  color: transparent;
  background: rgba(var(--ui-accent-rgb), 0.05);
  transition: border-color 160ms ease, background 160ms ease, color 160ms ease;
}

.beeroom-worker-option.is-selected .beeroom-worker-option-check {
  border-color: rgba(var(--ui-accent-rgb), 0.38);
  background: var(--ui-accent, #ff9448);
  color: #fff7ed;
}

.beeroom-worker-option-body {
  display: grid;
  min-width: 0;
  gap: 3px;
}

.beeroom-worker-option-name,
.beeroom-worker-option-id {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.beeroom-worker-option-name {
  font-size: 13px;
  font-weight: 600;
}

.beeroom-worker-option-id {
  font-size: 11px;
  color: var(--el-text-color-secondary, var(--dark-muted, #808080));
}

.beeroom-worker-picker-empty {
  padding: 18px 12px;
  border: 1px dashed rgba(var(--ui-accent-rgb), 0.16);
  border-radius: 14px;
  background: rgba(var(--ui-accent-rgb), 0.04);
  color: var(--el-text-color-secondary, var(--dark-muted, #808080));
  font-size: 12px;
  text-align: center;
}

.beeroom-dialog-hint {
  margin-top: 8px;
  color: var(--el-text-color-secondary, var(--dark-muted, #808080));
  font-size: 12px;
  line-height: 1.5;
}
</style>
