<template>
  <el-dialog
    v-model="visible"
    class="messenger-dialog"
    width="860px"
    top="5vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-title">{{ t('messenger.agentCreate.title') }}</div>
        <button class="messenger-dialog-close" type="button" @click="visible = false">&times;</button>
      </div>
    </template>
    <div class="messenger-dialog-body">
      <el-form :model="form" label-position="top" class="messenger-form">
        <el-form-item :label="t('messenger.agentCreate.name')">
          <el-input v-model="form.name" :placeholder="t('messenger.agentCreate.namePlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.description')">
          <el-input
            v-model="form.description"
            :placeholder="t('messenger.agentCreate.descriptionPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.copyFrom')">
          <el-select
            v-model="form.copy_from_agent_id"
            clearable
            filterable
            class="messenger-form-full"
            :placeholder="t('messenger.agentCreate.copyFromPlaceholder')"
          >
            <el-option :label="t('messenger.agentCreate.copyFromNone')" value="" />
            <el-option
              v-for="agent in copyFromAgents"
              :key="agent.id"
              :label="agent.name || agent.id"
              :value="agent.id"
            />
          </el-select>
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.systemPrompt')">
          <el-input
            v-model="form.system_prompt"
            type="textarea"
            :rows="7"
            :placeholder="t('messenger.agentCreate.systemPromptPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.tools')">
          <div class="tool-picker">
            <div v-if="toolLoading" class="tool-picker-empty">{{ t('common.loading') }}</div>
            <div v-else-if="toolError" class="tool-picker-empty">{{ toolError }}</div>
            <el-checkbox-group v-else v-model="form.tool_names" class="tool-groups">
              <div v-for="group in toolGroups" :key="group.label" class="tool-group">
                <div class="tool-group-head">
                  <div class="tool-group-title">{{ group.label }}</div>
                  <button class="tool-group-toggle" type="button" @click.prevent="toggleGroup(group)">
                    {{
                      isGroupFullSelected(group)
                        ? t('messenger.agentCreate.unselectAll')
                        : t('messenger.agentCreate.selectAll')
                    }}
                  </button>
                </div>
                <div class="tool-options">
                  <el-checkbox v-for="tool in group.options" :key="tool.value" :value="tool.value">
                    <span :title="tool.description || tool.label">{{ tool.label }}</span>
                  </el-checkbox>
                </div>
              </div>
            </el-checkbox-group>
          </div>
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.base')">
          <div class="base-grid">
            <label class="base-item">
              <span>{{ t('messenger.agentCreate.isShared') }}</span>
              <el-switch v-model="form.is_shared" />
            </label>
            <label class="base-item base-item-select">
              <span>{{ t('messenger.agentCreate.sandbox') }}</span>
              <el-select v-model="form.sandbox_container_id">
                <el-option
                  v-for="id in sandboxContainerOptions"
                  :key="id"
                  :label="t('portal.agent.sandbox.option', { id })"
                  :value="id"
                />
              </el-select>
            </label>
            <label class="base-item base-item-select">
              <span>{{ t('portal.agent.permission.title') }}</span>
              <el-select v-model="form.approval_mode">
                <el-option
                  v-for="item in approvalModeOptions"
                  :key="item.value"
                  :label="item.label"
                  :value="item.value"
                />
              </el-select>
            </label>
          </div>
        </el-form-item>
      </el-form>
    </div>
    <template #footer>
      <div class="messenger-dialog-footer">
        <el-button @click="visible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="handleSave">
          {{ saving ? t('common.loading') : t('common.save') }}
        </el-button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsSummary } from '@/api/userTools';
import { useI18n } from '@/i18n';

type ToolOption = {
  label: string;
  value: string;
  description: string;
};

type ToolGroup = {
  label: string;
  options: ToolOption[];
};

type AgentLike = {
  id: string;
  name: string;
};

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  },
  copyFromAgents: {
    type: Array as () => AgentLike[],
    default: () => []
  }
});

const emit = defineEmits(['update:modelValue', 'submit']);
const { t } = useI18n();

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));
const approvalModeOptions = computed(() => [
  { value: 'suggest', label: t('portal.agent.permission.option.suggest') },
  { value: 'auto_edit', label: t('portal.agent.permission.option.auto_edit') },
  { value: 'full_auto', label: t('portal.agent.permission.option.full_auto') }
]);
const toolLoading = ref(false);
const toolError = ref('');
const toolSummary = ref<Record<string, unknown> | null>(null);
const saving = ref(false);

const form = reactive({
  name: '',
  description: '',
  copy_from_agent_id: '',
  system_prompt: '',
  tool_names: [] as string[],
  is_shared: false,
  sandbox_container_id: 1,
  approval_mode: 'auto_edit'
});

const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const normalizeOption = (item: unknown): ToolOption | null => {
  if (!item) return null;
  if (typeof item === 'string') {
    const value = item.trim();
    if (!value) return null;
    return { label: value, value, description: '' };
  }
  const source = item as Record<string, unknown>;
  const value = String(source.name || source.tool_name || source.toolName || source.id || '').trim();
  if (!value) return null;
  return {
    label: value,
    value,
    description: String(source.description || '').trim()
  };
};

const normalizeOptions = (list: unknown): ToolOption[] => {
  if (!Array.isArray(list)) return [];
  return list.map((item) => normalizeOption(item)).filter(Boolean) as ToolOption[];
};

const toolGroups = computed<ToolGroup[]>(() => {
  const summary = toolSummary.value || {};
  const sharedPool = Array.isArray(summary.shared_tools) ? summary.shared_tools : [];
  const sharedSelected = new Set(
    Array.isArray(summary.shared_tools_selected) ? summary.shared_tools_selected.map((name) => String(name)) : []
  );
  const sharedTools =
    sharedSelected.size > 0
      ? sharedPool.filter((tool) => sharedSelected.has(String((tool as Record<string, unknown>)?.name || '').trim()))
      : sharedPool;

  return [
    {
      label: t('portal.agent.tools.group.builtin'),
      options: normalizeOptions(summary.builtin_tools)
    },
    {
      label: t('portal.agent.tools.group.mcp'),
      options: normalizeOptions(summary.mcp_tools)
    },
    {
      label: t('portal.agent.tools.group.a2a'),
      options: normalizeOptions(summary.a2a_tools)
    },
    {
      label: t('portal.agent.tools.group.skills'),
      options: normalizeOptions(summary.skills)
    },
    {
      label: t('portal.agent.tools.group.knowledge'),
      options: normalizeOptions(summary.knowledge_tools)
    },
    {
      label: t('portal.agent.tools.group.user'),
      options: normalizeOptions(summary.user_tools)
    },
    {
      label: t('portal.agent.tools.group.shared'),
      options: normalizeOptions(sharedTools)
    }
  ].filter((group) => group.options.length > 0);
});

const allToolValues = computed(() => {
  const values = new Set<string>();
  toolGroups.value.forEach((group) => {
    group.options.forEach((tool) => values.add(tool.value));
  });
  return Array.from(values);
});

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.copy_from_agent_id = '';
  form.system_prompt = '';
  form.tool_names = [...allToolValues.value];
  form.is_shared = false;
  form.sandbox_container_id = 1;
  form.approval_mode = 'auto_edit';
};

const loadToolSummary = async () => {
  if (toolLoading.value) return;
  toolLoading.value = true;
  toolError.value = '';
  try {
    const result = await fetchUserToolsSummary();
    toolSummary.value = result?.data?.data || {};
  } catch (error: any) {
    toolError.value = String(error?.response?.data?.detail || error?.message || t('common.requestFailed'));
  } finally {
    toolLoading.value = false;
  }
};

const isGroupFullSelected = (group: ToolGroup) => {
  if (!group.options.length) return false;
  const selected = new Set(form.tool_names);
  return group.options.every((option) => selected.has(option.value));
};

const toggleGroup = (group: ToolGroup) => {
  const selected = new Set(form.tool_names);
  if (isGroupFullSelected(group)) {
    group.options.forEach((option) => selected.delete(option.value));
  } else {
    group.options.forEach((option) => selected.add(option.value));
  }
  form.tool_names = Array.from(selected);
};

const normalizeSandboxContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const normalizeApprovalMode = (value: unknown): string => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return 'auto_edit';
};

const handleSave = async () => {
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning(t('portal.agent.nameRequired'));
    return;
  }
  saving.value = true;
  try {
    const payload = {
      name,
      description: String(form.description || '').trim(),
      copy_from_agent_id: String(form.copy_from_agent_id || '').trim(),
      system_prompt: String(form.system_prompt || ''),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      is_shared: Boolean(form.is_shared),
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    };
    if (!payload.copy_from_agent_id) {
      delete (payload as Record<string, unknown>).copy_from_agent_id;
    }
    emit('submit', payload);
  } finally {
    saving.value = false;
  }
};

watch(
  () => visible.value,
  async (value) => {
    if (!value) return;
    await loadToolSummary();
    resetForm();
  }
);

watch(
  () => allToolValues.value.join(','),
  () => {
    if (!visible.value) return;
    if (form.tool_names.length === 0) {
      form.tool_names = [...allToolValues.value];
    }
  }
);
</script>
