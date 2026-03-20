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
        <el-form-item :label="t('portal.agent.form.tools')" class="messenger-agent-form-item messenger-agent-form-item--tools">
          <div class="messenger-tool-picker">
            <div v-if="toolLoading" class="messenger-list-empty">{{ t('portal.agent.tools.loading') }}</div>
            <div v-else-if="toolError" class="messenger-list-empty">{{ toolError }}</div>
            <div v-else-if="!toolSections.length" class="messenger-list-empty">
              {{ t('portal.agent.tools.loadFailed') }}
            </div>
            <el-checkbox-group
              v-else
              v-model="form.tool_names"
              class="messenger-tool-groups"
              :disabled="isReadonlyMode"
            >
              <div v-for="section in toolSections" :key="section.key" class="messenger-tool-section">
                <div class="messenger-tool-section-title">{{ section.label }}</div>
                <div v-for="group in section.groups" :key="group.key" class="messenger-tool-group">
                  <div class="messenger-tool-group-head">
                    <div class="messenger-tool-group-head-left">
                      <span class="messenger-tool-group-title">{{ t('chat.approval.kind') }}：{{ group.label }}</span>
                    </div>
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
                      <span :title="option.hint">{{ option.label }}</span>
                    </el-checkbox>
                  </div>
                </div>
              </div>
            </el-checkbox-group>
          </div>
        </el-form-item>
        <AgentDependencyNotice
          :notice-key="dependencyNoticeKey"
          :missing-tool-names="dependencyStatus.missingToolNames"
          :missing-skill-names="dependencyStatus.missingSkillNames"
        />

        <el-form-item :label="t('portal.agent.form.base')" class="messenger-agent-form-item messenger-agent-form-item--base">
          <div class="messenger-agent-base">
            <div class="messenger-agent-base-item messenger-agent-base-item--select">
              <div class="messenger-agent-base-meta">
                <span class="messenger-agent-base-label">{{ t('messenger.agentGroup.label') }}</span>
              </div>
              <BeeroomGroupField
                v-model="form.group"
                :groups="beeroomGroupOptions"
                :allow-create="false"
                :disabled="isReadonlyMode"
              />
            </div>
            <div ref="modelSectionRef" class="messenger-agent-base-item messenger-agent-base-item--select">
              <div class="messenger-agent-base-meta">
                <span class="messenger-agent-base-label">{{ t('portal.agent.model.title') }}</span>
                <span class="messenger-inline-hint">{{ t('portal.agent.model.hint') }}</span>
              </div>
              <el-select
                v-model="form.model_name"
                class="messenger-agent-base-select"
                :disabled="isReadonlyMode || modelLoading"
              >
                <el-option
                  :label="t('portal.agent.model.defaultOption', { name: defaultModelDisplayName })"
                  value=""
                />
                <el-option
                  v-for="model in modelSelectOptions"
                  :key="model"
                  :label="model"
                  :value="model"
                />
              </el-select>
            </div>
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
        <el-form-item
          :label="t('portal.agent.form.presetQuestions')"
          class="messenger-agent-form-item"
        >
          <AgentPresetQuestionsField v-model="form.preset_questions" :readonly="isReadonlyMode" />
        </el-form-item>
      </el-form>

      <div class="messenger-inline-actions">
        <button class="messenger-inline-btn" type="button" :disabled="saving" @click="reloadAgent">
          {{ t('common.refresh') }}
        </button>
        <template v-if="!isReadonlyMode">
          <button
            class="messenger-inline-btn danger"
            type="button"
            :disabled="saving || isDefaultAgent"
            @click="deleteAgent"
          >
            {{ t('portal.agent.delete') }}
          </button>
          <button class="messenger-inline-btn primary" type="button" :disabled="saving" @click="saveAgent">
            {{ saving ? t('common.saving') : t('portal.agent.save') }}
          </button>
        </template>
        <button class="messenger-inline-btn" type="button" :disabled="saving" @click="exportWorkerCard">
          {{ t('portal.agent.exportWorkerCard') }}
        </button>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { listAgentModels } from '@/api/agents';
import { fetchUserToolsCatalog } from '@/api/userTools';
import AgentDependencyNotice from '@/components/agent/AgentDependencyNotice.vue';
import AgentPresetQuestionsField from '@/components/agent/AgentPresetQuestionsField.vue';
import BeeroomGroupField from '@/components/beeroom/BeeroomGroupField.vue';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useBeeroomStore } from '@/stores/beeroom';
import {
  buildAgentToolSections,
  type AgentToolGroup,
  type AgentToolSection
} from '@/utils/agentToolCatalog';
import {
  buildDeclaredDependencyPayload,
  buildWorkerCardDependencyPayload,
  resolveAgentDependencyStatus
} from '@/utils/agentDependencyStatus';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { resolveToolUsageHint } from '@/utils/toolUsageHint';
import { downloadWorkerCard } from '@/utils/workerCard';
import {
  buildBeeroomGroupPayload,
  createBeeroomGroupDraft,
  resolveBeeroomGroupDraftForAgent
} from '@/utils/beeroomGroupDraft';
import { showApiError } from '@/utils/apiError';
import { onUserToolsUpdated } from '@/utils/userToolsEvents';

type ToolOption = {
  label: string;
  value: string;
  description: string;
  hint: string;
};

type ToolSection = AgentToolSection<ToolOption>;

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  },
  readonly: {
    type: Boolean,
    default: false
  },
  focusTarget: {
    type: String,
    default: ''
  },
  focusToken: {
    type: Number,
    default: 0
  }
});

const emit = defineEmits<{
  saved: [agentId: string];
  deleted: [agentId: string];
  'focus-consumed': [target: string];
}>();

const { t } = useI18n();
const agentStore = useAgentStore();
const beeroomStore = useBeeroomStore();
const desktopLocalMode = computed(
  () => isDesktopModeEnabled() && !isDesktopRemoteAuthMode()
);
const showApprovalModeSetting = computed(
  () => desktopLocalMode.value
);
const resolveDefaultApprovalMode = (): string =>
  'full_auto';

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
const isReadonlyMode = computed(() => Boolean(props.readonly));
const dependencyNoticeKey = computed(() => `agent:${normalizedAgentId.value || '__default__'}`);
const canView = computed(() => isReadonlyMode.value || Boolean(normalizedAgentId.value));
const canEdit = computed(() => !isReadonlyMode.value && Boolean(normalizedAgentId.value));

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  system_prompt: '',
  model_name: '',
  tool_names: [] as string[],
  preset_questions: [] as string[],
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
const toolSummary = ref<Record<string, unknown> | null>(null);
const toolLoading = ref(false);
const toolError = ref('');
const currentAgent = ref<Record<string, unknown> | null>(null);
const modelLoading = ref(false);
const availableModelNames = ref<string[]>([]);
const defaultModelName = ref('');
const modelSectionRef = ref<HTMLElement | null>(null);
const panelMounted = ref(false);
let panelDisposed = false;
let latestAgentLoadRequestId = 0;
let lastHandledFocusToken = 0;
let focusAnimationFrame = 0;
let stopUserToolsUpdatedListener: (() => void) | null = null;

const nextAgentLoadRequestId = (): number => {
  latestAgentLoadRequestId += 1;
  return latestAgentLoadRequestId;
};

const isAgentLoadRequestActive = (requestId: number): boolean =>
  !panelDisposed && requestId === latestAgentLoadRequestId;

function clearFocusAnimationFrame(): void {
  if (focusAnimationFrame && typeof window !== 'undefined') {
    window.cancelAnimationFrame(focusAnimationFrame);
  }
  focusAnimationFrame = 0;
}

function consumeFocusTarget(target: string): void {
  if (!target) return;
  emit('focus-consumed', target);
}

function focusModelSection(attempt = 0): void {
  if (panelDisposed) return;
  const target = modelSectionRef.value;
  if (!target) {
    if (attempt < 2 && typeof window !== 'undefined') {
      clearFocusAnimationFrame();
      focusAnimationFrame = window.requestAnimationFrame(() => focusModelSection(attempt + 1));
    } else {
      consumeFocusTarget('model');
    }
    return;
  }
  target.scrollIntoView({ behavior: 'smooth', block: 'center' });
  const focusTarget = target.querySelector('.el-select__wrapper, input, select, textarea, button') as
    | HTMLElement
    | null;
  if (focusTarget && typeof focusTarget.focus === 'function') {
    focusTarget.focus();
  }
  consumeFocusTarget('model');
}

function scheduleFocusTargetIfNeeded(): void {
  if (!panelMounted.value || panelDisposed) return;
  const target = String(props.focusTarget || '').trim();
  const token = Number(props.focusToken || 0);
  if (target !== 'model' || token <= 0 || token === lastHandledFocusToken) return;
  lastHandledFocusToken = token;
  clearFocusAnimationFrame();
  void nextTick(() => {
    if (panelDisposed) return;
    if (typeof window !== 'undefined') {
      focusAnimationFrame = window.requestAnimationFrame(() => focusModelSection());
      return;
    }
    focusModelSection();
  });
}

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

const resolveConfiguredModelName = (agent: Record<string, unknown>): string => {
  const configured = String(agent.configured_model_name || '').trim();
  if (configured) return configured;
  const fallback = String(agent.model_name || '').trim();
  const defaultName = String(defaultModelName.value || '').trim();
  if (fallback && fallback !== defaultName) {
    return fallback;
  }
  return '';
};

const normalizeOption = (item: unknown): ToolOption | null => {
  if (!item) return null;
  if (typeof item === 'string') {
    const value = item.trim();
    if (!value) return null;
    return { label: value, value, description: '', hint: value };
  }
  const source = item as Record<string, unknown>;
  const value = String(source.name || source.tool_name || source.toolName || source.id || '').trim();
  if (!value) return null;
  const option: ToolOption = {
    label: value,
    value,
    description: String(source.description || '').trim(),
    hint: ''
  };
  option.hint = resolveToolUsageHint({
    name: option.value,
    label: option.label,
    description: option.description,
    input_schema: source.input_schema,
    inputSchema: source.inputSchema,
    schema: source.schema
  });
  if (!option.hint) option.hint = option.label;
  return option;
};

const resolveToolGroupDisplayOrder = (key: string): number => {
  const normalized = String(key || '').trim().toLowerCase();
  if (!normalized) return 999;
  if (normalized === 'builtin' || normalized.endsWith('-builtin')) return 10;
  if (normalized.includes('mcp')) return 20;
  if (normalized.includes('skills')) return 30;
  if (normalized.includes('knowledge')) return 40;
  if (normalized.includes('a2a')) return 50;
  if (normalized.includes('shared')) return 60;
  if (normalized === 'user' || normalized.startsWith('user')) return 70;
  return 999;
};

const toolSections = computed<ToolSection[]>(() =>
  buildAgentToolSections(toolSummary.value, t, normalizeOption).map((section) => ({
    ...section,
    groups: section.groups
      .map((group, index) => ({ group, index }))
      .sort((left, right) => {
        const orderDiff =
          resolveToolGroupDisplayOrder(left.group.key) - resolveToolGroupDisplayOrder(right.group.key);
        return orderDiff || left.index - right.index;
      })
      .map(({ group }) => group)
  }))
);

const defaultModelDisplayName = computed(() => {
  const fallback = t('portal.agent.model.defaultName');
  const value = String(defaultModelName.value || '').trim();
  return value || fallback;
});

const modelSelectOptions = computed<string[]>(() => {
  const seen = new Set<string>();
  const output: string[] = [];
  for (const item of availableModelNames.value) {
    const cleaned = String(item || '').trim();
    if (!cleaned || seen.has(cleaned)) continue;
    seen.add(cleaned);
    output.push(cleaned);
  }
  const configured = String(form.model_name || '').trim();
  if (configured && !seen.has(configured)) {
    output.unshift(configured);
  }
  return output;
});

const dependencyStatus = computed(() =>
  resolveAgentDependencyStatus(currentAgent.value, toolSummary.value, form.tool_names)
);

const isGroupFullSelected = (group: AgentToolGroup<ToolOption>): boolean => {
  if (!group.options.length) return false;
  const selected = new Set(form.tool_names);
  return group.options.every((option) => selected.has(option.value));
};

const toggleGroup = (group: AgentToolGroup<ToolOption>) => {
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
    const result = await fetchUserToolsCatalog();
    toolSummary.value = (result?.data?.data as Record<string, unknown>) || {};
  } catch (error) {
    toolError.value =
      (error as { response?: { data?: { detail?: string } }; message?: string })?.response?.data?.detail ||
      t('portal.agent.tools.loadFailed');
  } finally {
    toolLoading.value = false;
  }
};

const loadModelOptions = async () => {
  if (modelLoading.value) return;
  modelLoading.value = true;
  try {
    const result = await listAgentModels();
    const payload = (result?.data?.data || {}) as Record<string, unknown>;
    const items = Array.isArray(payload.items) ? payload.items : [];
    availableModelNames.value = items
      .map((item) => String(item || '').trim())
      .filter(Boolean);
    defaultModelName.value = String(payload.default_model_name || '').trim();
  } catch {
    availableModelNames.value = [];
    defaultModelName.value = '';
  } finally {
    modelLoading.value = false;
  }
};

const loadAgent = async (requestId: number = nextAgentLoadRequestId()) => {
  if (!canView.value) return;
  try {
    if (!beeroomStore.groups.length) {
      await beeroomStore.loadGroups().catch(() => null);
    }
    if (!isAgentLoadRequestActive(requestId)) return;
    const agent = await agentStore.getAgent(normalizedAgentId.value, { force: true });
    if (!isAgentLoadRequestActive(requestId)) return;
    if (!agent) {
      ElMessage.error(t('portal.agent.loadingFailed'));
      return;
    }
    // Only the latest async selection is allowed to write into the form.
    currentAgent.value = agent as Record<string, unknown>;
    form.name = String(agent.name || '');
    form.description = String(agent.description || '');
    form.is_shared = false;
    form.system_prompt = String(agent.system_prompt || '');
    form.model_name = resolveConfiguredModelName(currentAgent.value);
    form.tool_names = Array.isArray(agent.tool_names) ? [...agent.tool_names] : [];
    form.preset_questions = normalizeAgentPresetQuestions(agent.preset_questions);
    form.group = resolveBeeroomGroupDraftForAgent(agent.hive_id) as ReturnType<typeof createBeeroomGroupDraft>;
    form.sandbox_container_id = normalizeSandboxContainerId(agent.sandbox_container_id);
    form.approval_mode = normalizeApprovalMode(agent.approval_mode);
  } catch (error) {
    showApiError(error, t('portal.agent.loadingFailed'));
  }
};

const reloadAgent = async () => {
  const requestId = nextAgentLoadRequestId();
  await Promise.all([loadToolSummary(), loadModelOptions()]);
  if (!isAgentLoadRequestActive(requestId)) return;
  await loadAgent(requestId);
};

const refreshAgentFromExternalChange = () => {
  if (!panelMounted.value || panelDisposed || !canView.value) return;
  if (document.visibilityState !== 'visible') return;
  void loadAgent();
};

const handleAgentSettingsVisibilityChange = () => {
  if (document.visibilityState !== 'visible') return;
  refreshAgentFromExternalChange();
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
      description: String(form.description || '').trim(),
      is_shared: false,
      tool_names: dependencyPayload.tool_names,
      declared_tool_names: dependencyPayload.declared_tool_names,
      declared_skill_names: dependencyPayload.declared_skill_names,
      preset_questions: normalizeAgentPresetQuestions(form.preset_questions),
      ...buildBeeroomGroupPayload(form.group),
      system_prompt: String(form.system_prompt || ''),
      model_name: String(form.model_name || '').trim(),
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    };
    if (!payload.hive_name) delete payload.hive_name;
    if (!payload.hive_description) delete payload.hive_description;
    const updated = await agentStore.updateAgent(normalizedAgentId.value, payload);
    currentAgent.value = (updated as Record<string, unknown> | null) || currentAgent.value;
    await beeroomStore.loadGroups().catch(() => null);
    ElMessage.success(t('portal.agent.updateSuccess'));
    emit('saved', normalizedAgentId.value);
  } catch (error) {
    showApiError(error, t('portal.agent.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const exportWorkerCard = () => {
  const groupPayload = buildBeeroomGroupPayload(form.group);
  const dependencyPayload = buildWorkerCardDependencyPayload(form.tool_names, currentAgent.value, toolSummary.value);
  const filename = downloadWorkerCard({
    id: normalizedAgentId.value,
    name: String(form.name || '').trim() || normalizedAgentId.value,
    description: String(form.description || '').trim(),
    system_prompt: String(form.system_prompt || ''),
    model_name: String(form.model_name || '').trim(),
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
  if (!canEdit.value || isDefaultAgent.value) return;
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

onMounted(() => {
  panelMounted.value = true;
  stopUserToolsUpdatedListener = onUserToolsUpdated((event) => {
    const detail = event?.detail || {};
    const scope = String((detail as Record<string, unknown>).scope || '').trim().toLowerCase();
    if (scope && scope !== 'all' && scope !== 'skills' && scope !== 'mcp' && scope !== 'knowledge') {
      return;
    }
    void loadToolSummary();
  });
  window.addEventListener('focus', refreshAgentFromExternalChange);
  document.addEventListener('visibilitychange', handleAgentSettingsVisibilityChange);
  void reloadAgent();
  scheduleFocusTargetIfNeeded();
});

watch(
  () => normalizedAgentId.value,
  () => {
    if (!panelMounted.value || panelDisposed) return;
    void reloadAgent();
  }
);

watch(
  () => [props.focusTarget, props.focusToken, canView.value] as const,
  () => {
    scheduleFocusTargetIfNeeded();
  }
);

onBeforeUnmount(() => {
  panelDisposed = true;
  latestAgentLoadRequestId += 1;
  window.removeEventListener('focus', refreshAgentFromExternalChange);
  document.removeEventListener('visibilitychange', handleAgentSettingsVisibilityChange);
  if (stopUserToolsUpdatedListener) {
    stopUserToolsUpdatedListener();
    stopUserToolsUpdatedListener = null;
  }
  clearFocusAnimationFrame();
});
</script>

