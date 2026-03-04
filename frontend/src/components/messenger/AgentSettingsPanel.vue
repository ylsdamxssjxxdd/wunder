<template>
  <div class="messenger-agent-settings">
    <div v-if="!canView" class="messenger-list-empty">
      {{ t('chat.features.agentMissing') }}
    </div>

    <template v-else>
      <el-form :model="form" label-position="top" class="messenger-agent-form messenger-form">
        <el-form-item :label="t('portal.agent.form.name')" class="messenger-agent-form-item">
          <el-input
            v-model="form.name"
            class="messenger-agent-field"
            :placeholder="t('portal.agent.form.placeholder.name')"
            :disabled="isReadonlyMode"
          />
        </el-form-item>
        <el-form-item :label="t('portal.agent.form.description')" class="messenger-agent-form-item">
          <el-input
            v-model="form.description"
            class="messenger-agent-field"
            :placeholder="t('portal.agent.form.placeholder.description')"
            :disabled="isReadonlyMode"
          />
        </el-form-item>
        <el-form-item :label="t('portal.agent.form.prompt')" class="messenger-agent-form-item">
          <el-input
            v-model="form.system_prompt"
            class="messenger-agent-field messenger-agent-field--prompt"
            type="textarea"
            :rows="6"
            :placeholder="t('portal.agent.form.placeholder.prompt')"
            :disabled="isReadonlyMode"
          />
        </el-form-item>
        <el-form-item :label="t('portal.agent.form.tools')">
          <div class="messenger-tool-picker">
            <div v-if="toolLoading" class="messenger-list-empty">{{ t('portal.agent.tools.loading') }}</div>
            <div v-else-if="toolError" class="messenger-list-empty">{{ toolError }}</div>
            <div v-else-if="!toolGroups.length" class="messenger-list-empty">
              {{ t('portal.agent.tools.loadFailed') }}
            </div>
            <el-checkbox-group
              v-else
              v-model="form.tool_names"
              class="messenger-tool-groups"
              :disabled="isReadonlyMode"
            >
              <div v-for="group in toolGroups" :key="group.label" class="messenger-tool-group">
                <div class="messenger-tool-group-head">
                  <span>{{ group.label }}</span>
                  <button
                    class="messenger-tool-group-toggle"
                    type="button"
                    :disabled="isReadonlyMode"
                    @click.prevent="toggleGroup(group)"
                  >
                    {{
                      isGroupFullSelected(group)
                        ? t('portal.agent.tools.unselectAll')
                        : t('portal.agent.tools.selectAll')
                    }}
                  </button>
                </div>
                <div class="messenger-tool-options">
                  <el-checkbox v-for="option in group.options" :key="option.value" :value="option.value">
                    <span :title="option.description || option.label">{{ option.label }}</span>
                  </el-checkbox>
                </div>
              </div>
            </el-checkbox-group>
            <div v-if="sharedToolsNotice" class="messenger-inline-hint">
              {{ t('portal.agent.tools.notice') }}
            </div>
          </div>
        </el-form-item>

        <el-form-item :label="t('portal.agent.form.base')" class="messenger-agent-form-item">
          <div class="messenger-agent-base">
            <label class="messenger-agent-base-item">
              <span class="messenger-agent-base-label">{{ t('portal.agent.share.label') }}</span>
              <el-switch v-model="form.is_shared" :disabled="isReadonlyMode" />
            </label>
            <div class="messenger-agent-base-item messenger-agent-base-item--select">
              <div class="messenger-agent-base-meta">
                <span class="messenger-agent-base-label">{{ t('portal.agent.sandbox.title') }}</span>
                <span class="messenger-inline-hint">{{ t('portal.agent.sandbox.hint') }}</span>
              </div>
              <el-select
                v-model="form.sandbox_container_id"
                class="messenger-agent-base-select"
                :disabled="isReadonlyMode"
              >
                <el-option
                  v-for="id in sandboxContainerOptions"
                  :key="id"
                  :label="t('portal.agent.sandbox.option', { id })"
                  :value="id"
                />
              </el-select>
            </div>
            <div v-if="showApprovalModeSetting" class="messenger-agent-base-item messenger-agent-base-item--select">
              <div class="messenger-agent-base-meta">
                <span class="messenger-agent-base-label">{{ t('portal.agent.permission.title') }}</span>
                <span class="messenger-inline-hint">{{ t('portal.agent.permission.hint') }}</span>
              </div>
              <el-select
                v-model="form.approval_mode"
                class="messenger-agent-base-select"
                :disabled="isReadonlyMode"
              >
                <el-option
                  v-for="item in approvalModeOptions"
                  :key="item.value"
                  :label="item.label"
                  :value="item.value"
                />
              </el-select>
            </div>
          </div>
        </el-form-item>
      </el-form>

      <div class="messenger-inline-actions">
        <button class="messenger-inline-btn" type="button" :disabled="saving" @click="reloadAgent">
          {{ t('common.refresh') }}
        </button>
        <template v-if="!isReadonlyMode">
          <button class="messenger-inline-btn danger" type="button" :disabled="saving" @click="deleteAgent">
            {{ t('portal.agent.delete') }}
          </button>
          <button class="messenger-inline-btn primary" type="button" :disabled="saving" @click="saveAgent">
            {{ saving ? t('common.saving') : t('portal.agent.save') }}
          </button>
        </template>
      </div>
      <div v-if="isReadonlyMode" class="messenger-inline-hint">
        {{ t('messenger.defaultAgentReadonlyHint') }}
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserToolsSummary } from '@/api/userTools';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { showApiError } from '@/utils/apiError';

type ToolOption = {
  label: string;
  value: string;
  description: string;
};

type ToolGroup = {
  label: string;
  options: ToolOption[];
};

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  },
  readonly: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits<{
  saved: [agentId: string];
  deleted: [agentId: string];
}>();

const { t } = useI18n();
const agentStore = useAgentStore();
const showApprovalModeSetting = computed(
  () => isDesktopModeEnabled() && !isDesktopRemoteAuthMode()
);
const resolveDefaultApprovalMode = (): string =>
  showApprovalModeSetting.value ? 'auto_edit' : 'full_auto';

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));
const approvalModeOptions = computed(() => [
  { value: 'suggest', label: t('portal.agent.permission.option.suggest') },
  { value: 'auto_edit', label: t('portal.agent.permission.option.auto_edit') },
  { value: 'full_auto', label: t('portal.agent.permission.option.full_auto') }
]);

const isDefaultAgentAlias = (value: string): boolean => {
  const lowered = value.trim().toLowerCase();
  return !lowered || lowered === '__default__' || lowered === 'default';
};

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const isDefaultAgent = computed(() => isDefaultAgentAlias(normalizedAgentId.value));
const isReadonlyMode = computed(() => Boolean(props.readonly) || isDefaultAgent.value);
const canView = computed(() => isReadonlyMode.value || Boolean(normalizedAgentId.value));
const canEdit = computed(() => !isReadonlyMode.value && Boolean(normalizedAgentId.value));

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  system_prompt: '',
  tool_names: [] as string[],
  sandbox_container_id: 1,
  approval_mode: resolveDefaultApprovalMode()
});

const saving = ref(false);
const toolSummary = ref<Record<string, unknown> | null>(null);
const toolLoading = ref(false);
const toolError = ref('');

const normalizeSandboxContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const normalizeApprovalMode = (value: unknown): string => {
  if (!showApprovalModeSetting.value) return 'full_auto';
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return resolveDefaultApprovalMode();
};

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

const resolveSharedTools = (summary: Record<string, unknown>): unknown[] => {
  const sharedPool = Array.isArray(summary.shared_tools) ? summary.shared_tools : [];
  const sharedSelected = new Set(
    Array.isArray(summary.shared_tools_selected)
      ? summary.shared_tools_selected.map((name) => String(name || '').trim())
      : []
  );
  const sharedTools =
    sharedSelected.size > 0
      ? sharedPool.filter((tool) =>
          sharedSelected.has(String((tool as Record<string, unknown>)?.name || '').trim())
        )
      : sharedPool;
  return sharedTools;
};

const collectSummaryToolNames = (summary: Record<string, unknown>): string[] => {
  const names = new Set<string>();
  const collect = (items: unknown) => {
    normalizeOptions(items).forEach((option) => names.add(option.value));
  };
  collect(summary.builtin_tools);
  collect(summary.mcp_tools);
  collect(summary.skills);
  collect(summary.knowledge_tools);
  collect(summary.user_tools);
  collect(resolveSharedTools(summary));
  const output = Array.from(names);
  output.sort();
  return output;
};

const applyReadonlyDefaultAgentForm = () => {
  form.name = t('messenger.defaultAgent');
  form.description = t('messenger.defaultAgentDesc');
  form.is_shared = false;
  form.system_prompt = '';
  form.tool_names = collectSummaryToolNames(toolSummary.value || {});
  form.sandbox_container_id = 1;
  form.approval_mode = resolveDefaultApprovalMode();
};

const toolGroups = computed<ToolGroup[]>(() => {
  const summary = toolSummary.value || {};
  const sharedTools = resolveSharedTools(summary);

  return [
    { label: t('portal.agent.tools.group.builtin'), options: normalizeOptions(summary.builtin_tools) },
    { label: t('portal.agent.tools.group.mcp'), options: normalizeOptions(summary.mcp_tools) },
    { label: t('portal.agent.tools.group.skills'), options: normalizeOptions(summary.skills) },
    { label: t('portal.agent.tools.group.knowledge'), options: normalizeOptions(summary.knowledge_tools) },
    { label: t('portal.agent.tools.group.user'), options: normalizeOptions(summary.user_tools) },
    { label: t('portal.agent.tools.group.shared'), options: normalizeOptions(sharedTools) }
  ].filter((group) => group.options.length > 0);
});

const sharedToolsNotice = computed(() => {
  const summary = toolSummary.value || {};
  const shared = Array.isArray(summary.shared_tools) ? summary.shared_tools : [];
  const selected = Array.isArray(summary.shared_tools_selected) ? summary.shared_tools_selected : [];
  return shared.length > 0 && selected.length === 0;
});

const isGroupFullSelected = (group: ToolGroup): boolean => {
  if (!group.options.length) return false;
  const selected = new Set(form.tool_names);
  return group.options.every((option) => selected.has(option.value));
};

const toggleGroup = (group: ToolGroup) => {
  if (isReadonlyMode.value) return;
  const selected = new Set(form.tool_names);
  if (isGroupFullSelected(group)) {
    group.options.forEach((option) => selected.delete(option.value));
  } else {
    group.options.forEach((option) => selected.add(option.value));
  }
  form.tool_names = Array.from(selected);
};

const loadToolSummary = async () => {
  if (toolLoading.value) return;
  toolLoading.value = true;
  toolError.value = '';
  try {
    const result = await fetchUserToolsSummary();
    toolSummary.value = (result?.data?.data as Record<string, unknown>) || {};
    if (isDefaultAgent.value) {
      form.tool_names = collectSummaryToolNames(toolSummary.value || {});
    }
  } catch (error) {
    toolError.value =
      (error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail ||
      t('portal.agent.tools.loadFailed');
  } finally {
    toolLoading.value = false;
  }
};

const loadAgent = async () => {
  if (!canView.value) return;
  if (isDefaultAgent.value) {
    applyReadonlyDefaultAgentForm();
    return;
  }
  try {
    const agent = await agentStore.getAgent(normalizedAgentId.value, { force: true });
    if (!agent) {
      ElMessage.error(t('portal.agent.loadingFailed'));
      return;
    }
    form.name = String(agent.name || '');
    form.description = String(agent.description || '');
    form.is_shared = Boolean(agent.is_shared);
    form.system_prompt = String(agent.system_prompt || '');
    form.tool_names = Array.isArray(agent.tool_names) ? [...agent.tool_names] : [];
    form.sandbox_container_id = normalizeSandboxContainerId(agent.sandbox_container_id);
    form.approval_mode = normalizeApprovalMode(agent.approval_mode);
  } catch (error) {
    showApiError(error, t('portal.agent.loadingFailed'));
  }
};

const reloadAgent = async () => {
  if (isDefaultAgent.value) {
    await loadToolSummary();
    applyReadonlyDefaultAgentForm();
    return;
  }
  await Promise.all([loadAgent(), loadToolSummary()]);
};

const saveAgent = async () => {
  if (!canEdit.value) return;
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning(t('portal.agent.nameRequired'));
    return;
  }
  saving.value = true;
  try {
    await agentStore.updateAgent(normalizedAgentId.value, {
      name,
      description: String(form.description || '').trim(),
      is_shared: Boolean(form.is_shared),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      system_prompt: String(form.system_prompt || ''),
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    });
    ElMessage.success(t('portal.agent.updateSuccess'));
    emit('saved', normalizedAgentId.value);
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const deleteAgent = async () => {
  if (!canEdit.value) return;
  const targetName = String(form.name || normalizedAgentId.value || '').trim();
  try {
    await ElMessageBox.confirm(t('portal.agent.deleteConfirm', { name: targetName }), t('common.notice'), {
      confirmButtonText: t('portal.agent.delete'),
      cancelButtonText: t('portal.agent.cancel'),
      type: 'warning'
    });
  } catch {
    return;
  }
  try {
    await agentStore.deleteAgent(normalizedAgentId.value);
    ElMessage.success(t('portal.agent.deleteSuccess'));
    emit('deleted', normalizedAgentId.value);
  } catch (error) {
    showApiError(error, t('portal.agent.deleteFailed'));
  }
};

watch(
  () => normalizedAgentId.value,
  async () => {
    await loadToolSummary();
    await loadAgent();
  },
  { immediate: true }
);
</script>
