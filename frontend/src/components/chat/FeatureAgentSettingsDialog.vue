<template>
  <el-dialog
    v-model="visible"
    class="user-tools-dialog agent-editor-dialog feature-agent-editor-dialog"
    width="820px"
    top="6vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="user-tools-header">
        <div class="user-tools-title">{{ t('chat.features.agentSettings') }}</div>
        <button class="icon-btn" type="button" @click="visible = false">&times;</button>
      </div>
    </template>
    <div class="agent-editor-body">
      <div v-if="!canEdit" class="agent-tool-loading">{{ t('chat.features.agentMissing') }}</div>
      <el-form v-else :model="form" label-position="top" class="agent-editor-form">
        <el-form-item class="agent-form-item agent-form-item--name" :label="t('portal.agent.form.name')">
          <el-input v-model="form.name" :placeholder="t('portal.agent.form.placeholder.name')" />
        </el-form-item>
        <el-form-item class="agent-form-item agent-form-item--description" :label="t('portal.agent.form.description')">
          <el-input v-model="form.description" :placeholder="t('portal.agent.form.placeholder.description')" />
        </el-form-item>
        <el-form-item class="agent-form-item agent-form-item--group" :label="t('messenger.agentGroup.label')">
          <BeeroomGroupField v-model="form.group" :groups="beeroomGroupOptions" :allow-create="false" />
        </el-form-item>
        <el-form-item class="agent-form-item agent-form-item--prompt" :label="t('portal.agent.form.prompt')">
          <el-input
            v-model="form.system_prompt"
            type="textarea"
            :rows="8"
            :placeholder="t('portal.agent.form.placeholder.prompt')"
          />
        </el-form-item>
        <el-form-item
          class="agent-form-item agent-form-item--preset-questions"
          :label="t('portal.agent.form.presetQuestions')"
        >
          <AgentPresetQuestionsField v-model="form.preset_questions" />
        </el-form-item>
        <el-form-item class="agent-form-item agent-form-item--tools" :label="t('portal.agent.form.tools')">
          <div class="agent-tool-picker">
            <div v-if="toolLoading" class="agent-tool-loading">{{ t('portal.agent.tools.loading') }}</div>
            <div v-else-if="toolError" class="agent-tool-loading">{{ toolError }}</div>
            <div v-else-if="!toolGroups.length" class="agent-tool-loading">{{ t('portal.agent.tools.loadFailed') }}</div>
            <el-checkbox-group v-else v-model="form.tool_names" class="agent-tool-groups">
              <div v-for="group in toolGroups" :key="group.label" class="agent-tool-group">
                <div class="agent-tool-group-header">
                  <div class="agent-tool-group-title">{{ group.label }}</div>
                  <button class="agent-tool-group-select" type="button" @click.stop="selectToolGroup(group)">
                    {{ isToolGroupFullySelected(group) ? t('portal.agent.tools.unselectAll') : t('portal.agent.tools.selectAll') }}
                  </button>
                </div>
                <div class="agent-tool-options">
                  <el-checkbox v-for="option in group.options" :key="option.value" :value="option.value">
                    <span :title="option.description || option.label">{{ option.label }}</span>
                  </el-checkbox>
                </div>
              </div>
            </el-checkbox-group>
          </div>
        </el-form-item>
        <AgentDependencyNotice
          :missing-tool-names="dependencyStatus.missingToolNames"
          :missing-skill-names="dependencyStatus.missingSkillNames"
        />
        <el-form-item class="agent-form-item agent-form-item--base" :label="t('portal.agent.form.base')">
          <div class="agent-basic-settings">
            <div class="agent-share-card agent-share-card--combined">
              <div class="agent-share-row agent-share-row--sandbox">
                <span>{{ t('portal.agent.sandbox.title') }}</span>
                <el-select v-model="form.sandbox_container_id" size="small" class="agent-sandbox-select">
                  <el-option
                    v-for="id in sandboxContainerOptions"
                    :key="id"
                    :label="t('portal.agent.sandbox.option', { id })"
                    :value="id"
                  />
                </el-select>
              </div>
              <div v-if="showApprovalModeSetting" class="agent-share-row agent-share-row--sandbox">
                <span>{{ t('portal.agent.permission.title') }}</span>
                <el-select v-model="form.approval_mode" size="small" class="agent-sandbox-select">
                  <el-option
                    v-for="item in approvalModeOptions"
                    :key="item.value"
                    :label="item.label"
                    :value="item.value"
                  />
                </el-select>
              </div>
              <div class="agent-editor-hint">{{ t('portal.agent.sandbox.hint') }}</div>
              <div v-if="showApprovalModeSetting" class="agent-editor-hint">
                {{ t('portal.agent.permission.hint') }}
              </div>
            </div>
          </div>
        </el-form-item>
      </el-form>
    </div>
    <template #footer>
      <el-button @click="visible = false">{{ t('portal.agent.cancel') }}</el-button>
      <el-button type="danger" plain :disabled="saving || !canEdit" @click="deleteAgent">
        {{ t('portal.agent.delete') }}
      </el-button>
      <el-button type="primary" :loading="saving" :disabled="!canEdit" @click="saveAgent">
        {{ saving ? t('common.saving') : t('portal.agent.save') }}
      </el-button>
      <el-button :disabled="saving" @click="exportWorkerCard">{{ t('portal.agent.exportWorkerCard') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserToolsSummary } from '@/api/userTools';
import AgentDependencyNotice from '@/components/agent/AgentDependencyNotice.vue';
import AgentPresetQuestionsField from '@/components/agent/AgentPresetQuestionsField.vue';
import BeeroomGroupField from '@/components/beeroom/BeeroomGroupField.vue';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useBeeroomStore } from '@/stores/beeroom';
import { buildDeclaredDependencyPayload, resolveAgentDependencyStatus } from '@/utils/agentDependencyStatus';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { downloadWorkerCard } from '@/utils/workerCard';
import {
  buildBeeroomGroupPayload,
  createBeeroomGroupDraft,
  resolveBeeroomGroupDraftForAgent
} from '@/utils/beeroomGroupDraft';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  },
  agentId: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['update:modelValue', 'deleted']);
const { t } = useI18n();
const agentStore = useAgentStore();
const beeroomStore = useBeeroomStore();
const showApprovalModeSetting = computed(
  () => isDesktopModeEnabled() && !isDesktopRemoteAuthMode()
);
const resolveDefaultApprovalMode = (): string =>
  showApprovalModeSetting.value ? 'auto_edit' : 'full_auto';

const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const canEdit = computed(() => Boolean(normalizedAgentId.value));

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));
const approvalModeOptions = computed(() => [
  { value: 'suggest', label: t('portal.agent.permission.option.suggest') },
  { value: 'auto_edit', label: t('portal.agent.permission.option.auto_edit') },
  { value: 'full_auto', label: t('portal.agent.permission.option.full_auto') }
]);

const normalizeSandboxContainerId = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const normalizeApprovalMode = (value) => {
  if (!showApprovalModeSetting.value) return 'full_auto';
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return resolveDefaultApprovalMode();
};

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  system_prompt: '',
  tool_names: [],
  preset_questions: [],
  group: createBeeroomGroupDraft(),
  sandbox_container_id: 1,
  approval_mode: resolveDefaultApprovalMode()
});

const saving = ref(false);
const beeroomGroupOptions = computed(() =>
  (Array.isArray(beeroomStore.groups) ? beeroomStore.groups : []).map((group) => ({
    group_id: String(group?.group_id || group?.hive_id || '').trim(),
    name: String(group?.name || group?.group_id || group?.hive_id || '').trim(),
    description: String(group?.description || '').trim(),
    is_default: Boolean(group?.is_default)
  }))
);
const toolSummary = ref(null);
const toolLoading = ref(false);
const toolError = ref('');
const currentAgent = ref<Record<string, unknown> | null>(null);

const normalizeToolOption = (item) => {
  if (!item) return null;
  if (typeof item === 'string') {
    const name = item.trim();
    return name ? { label: name, value: name, description: '' } : null;
  }
  const value = String(item.name || item.tool_name || item.toolName || item.id || '').trim();
  if (!value) return null;
  return {
    label: value,
    value,
    description: String(item.description || '').trim()
  };
};

const normalizeOptions = (list) =>
  Array.isArray(list) ? list.map((item) => normalizeToolOption(item)).filter(Boolean) : [];

const toolGroups = computed(() => {
  const summary = toolSummary.value || {};
  return [
    { label: t('portal.agent.tools.group.builtin'), options: normalizeOptions(summary.builtin_tools) },
    { label: t('portal.agent.tools.group.mcp'), options: normalizeOptions(summary.mcp_tools) },
    { label: t('portal.agent.tools.group.a2a'), options: normalizeOptions(summary.a2a_tools) },
    { label: t('portal.agent.tools.group.skills'), options: normalizeOptions(summary.skills) },
    { label: t('portal.agent.tools.group.knowledge'), options: normalizeOptions(summary.knowledge_tools) },
    { label: t('portal.agent.tools.group.user'), options: normalizeOptions(summary.user_tools) }
  ].filter((group) => group.options.length > 0);
});

const dependencyStatus = computed(() =>
  resolveAgentDependencyStatus(currentAgent.value, toolSummary.value, form.tool_names)
);

const isToolGroupFullySelected = (group) => {
  if (!group || !Array.isArray(group.options) || group.options.length === 0) return false;
  const current = new Set(form.tool_names);
  return group.options.every((option) => current.has(option.value));
};

const selectToolGroup = (group) => {
  if (!group || !Array.isArray(group.options) || group.options.length === 0) return;
  const next = new Set(form.tool_names);
  const fullySelected = group.options.every((option) => next.has(option.value));
  if (fullySelected) {
    group.options.forEach((option) => next.delete(option.value));
  } else {
    group.options.forEach((option) => next.add(option.value));
  }
  form.tool_names = Array.from(next);
};

const loadToolSummary = async () => {
  if (toolLoading.value) return;
  toolLoading.value = true;
  toolError.value = '';
  try {
    const result = await fetchUserToolsSummary();
    toolSummary.value = result?.data?.data || null;
  } catch (error) {
    toolError.value = error?.response?.data?.detail || t('portal.agent.tools.loadFailed');
  } finally {
    toolLoading.value = false;
  }
};

const loadAgent = async () => {
  if (!canEdit.value) {
    return;
  }
  try {
    if (!beeroomStore.groups.length) {
      await beeroomStore.loadGroups().catch(() => null);
    }
    const agent = await agentStore.getAgent(normalizedAgentId.value, { force: true });
    if (!agent) {
      ElMessage.error(t('portal.agent.loadingFailed'));
      return;
    }
    currentAgent.value = agent as Record<string, unknown>;
    form.name = agent.name || '';
    form.description = agent.description || '';
    form.is_shared = false;
    form.system_prompt = agent.system_prompt || '';
    form.tool_names = Array.isArray(agent.tool_names) ? [...agent.tool_names] : [];
    form.preset_questions = normalizeAgentPresetQuestions(agent.preset_questions);
    form.group = resolveBeeroomGroupDraftForAgent(agent.hive_id) as ReturnType<typeof createBeeroomGroupDraft>;
    form.sandbox_container_id = normalizeSandboxContainerId(agent.sandbox_container_id);
    form.approval_mode = normalizeApprovalMode(agent.approval_mode);
  } catch (error) {
    showApiError(error, t('portal.agent.loadingFailed'));
  }
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
    const dependencyPayload = buildDeclaredDependencyPayload(form.tool_names, currentAgent.value, toolSummary.value);
    const payload: Record<string, unknown> = {
      name,
      description: form.description || '',
      is_shared: false,
      tool_names: dependencyPayload.tool_names,
      declared_tool_names: dependencyPayload.declared_tool_names,
      declared_skill_names: dependencyPayload.declared_skill_names,
      preset_questions: normalizeAgentPresetQuestions(form.preset_questions),
      ...buildBeeroomGroupPayload(form.group),
      system_prompt: form.system_prompt || '',
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    };
    if (!payload.hive_name) delete payload.hive_name;
    if (!payload.hive_description) delete payload.hive_description;
    const updated = await agentStore.updateAgent(normalizedAgentId.value, payload);
    currentAgent.value = (updated as Record<string, unknown> | null) || currentAgent.value;
    await beeroomStore.loadGroups().catch(() => null);
    ElMessage.success(t('portal.agent.updateSuccess'));
    visible.value = false;
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const exportWorkerCard = () => {
  const groupPayload = buildBeeroomGroupPayload(form.group);
  const dependencyPayload = buildDeclaredDependencyPayload(form.tool_names, currentAgent.value, toolSummary.value);
  const filename = downloadWorkerCard({
    id: normalizedAgentId.value,
    name: String(form.name || '').trim() || normalizedAgentId.value,
    description: String(form.description || '').trim(),
    system_prompt: String(form.system_prompt || ''),
    tool_names: dependencyPayload.tool_names,
    declared_tool_names: dependencyPayload.declared_tool_names,
    declared_skill_names: dependencyPayload.declared_skill_names,
    preset_questions: normalizeAgentPresetQuestions(form.preset_questions),
    approval_mode: normalizeApprovalMode(form.approval_mode),
    sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
    hive_id: groupPayload.hive_id,
    hive_name: groupPayload.hive_name,
    hive_description: groupPayload.hive_description
  });
  ElMessage.success(t('portal.agent.workerCardExportSuccess', { name: filename }));
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
  } catch (error) {
    return;
  }
  try {
    await agentStore.deleteAgent(normalizedAgentId.value);
    ElMessage.success(t('portal.agent.deleteSuccess'));
    visible.value = false;
    emit('deleted', normalizedAgentId.value);
  } catch (error) {
    showApiError(error, t('portal.agent.deleteFailed'));
  }
};

watch(
  () => visible.value,
  (value) => {
    if (value) {
      loadToolSummary();
      loadAgent();
    }
  }
);

watch(
  () => normalizedAgentId.value,
  () => {
    if (visible.value) {
      loadAgent();
    }
  }
);
</script>

