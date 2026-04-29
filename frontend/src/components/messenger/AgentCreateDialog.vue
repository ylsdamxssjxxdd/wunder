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
        <el-form-item :label="t('messenger.agentGroup.label')">
          <BeeroomGroupField
            v-model="form.group"
            :groups="beeroomGroups"
            :default-group-id="defaultBeeroomGroupId"
            :allow-create="false"
          />
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.systemPrompt')">
          <el-input
            v-model="form.system_prompt"
            type="textarea"
            :rows="7"
            :placeholder="t('messenger.agentCreate.systemPromptPlaceholder')"
          />
        </el-form-item>
        <el-form-item :label="t('portal.agent.form.presetQuestions')">
          <AgentPresetQuestionsField v-model="form.preset_questions" />
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.tools')">
          <div class="messenger-tool-picker">
            <div v-if="toolLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
            <div v-else-if="toolError" class="messenger-list-empty">{{ toolError }}</div>
            <el-checkbox-group v-else v-model="form.tool_names" class="messenger-tool-groups">
              <div v-for="section in toolSections" :key="section.key" class="messenger-tool-section">
                <div class="messenger-tool-section-title">{{ section.label }}</div>
                <div v-for="group in section.groups" :key="group.key" class="messenger-tool-group">
                  <div class="messenger-tool-group-head">
                    <div class="messenger-tool-group-head-left">
                      <div class="messenger-tool-group-title">{{ group.label }}</div>
                    </div>
                    <button class="messenger-tool-group-toggle" type="button" @click.prevent="toggleGroup(group)">
                      {{
                        isGroupFullSelected(group)
                          ? t('messenger.agentCreate.unselectAll')
                          : t('messenger.agentCreate.selectAll')
                      }}
                    </button>
                  </div>
                  <div
                    class="messenger-tool-options"
                    :class="{ 'messenger-tool-options--scrollable': group.options.length > 3 }"
                  >
                    <div
                      v-for="tool in group.options"
                      :key="tool.value"
                      class="messenger-tool-option-item"
                      @contextmenu.prevent="showToolDetail($event, group, tool)"
                    >
                      <el-checkbox :value="tool.value">
                        <AgentToolOptionLabel
                          :label="tool.label"
                          :description="tool.description"
                          :hint="tool.hint"
                          :group-key="group.key"
                        />
                      </el-checkbox>
                    </div>
                  </div>
                </div>
              </div>
            </el-checkbox-group>
          </div>
        </el-form-item>
        <el-form-item :label="t('messenger.agentCreate.base')">
          <div class="base-grid">
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
              <span>{{ t('portal.agent.model.title') }}</span>
              <el-select v-model="form.model_name" :disabled="modelLoading">
                <el-option :label="t('portal.agent.model.defaultOption', { name: defaultModelDisplayName })" value="" />
                <el-option
                  v-for="model in modelSelectOptions"
                  :key="model"
                  :label="model"
                  :value="model"
                />
              </el-select>
            </label>
            <label v-if="showApprovalModeSetting" class="base-item base-item-select">
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

  <Teleport to="body">
    <div
      v-if="detailOption"
      class="messenger-tool-detail-overlay"
      @mousedown="detailOption = null"
      @contextmenu.prevent="detailOption = null"
    >
      <div class="messenger-tool-detail-popup" :style="detailPopupStyle" @mousedown.stop>
        <AbilityTooltipCard
          :name="detailOption.option.label"
          :description="detailOption.option.description"
          :hint="detailOption.option.hint"
          :kind="detailOption.kind"
          :group="detailOption.groupKey"
          :source="detailOption.groupKey"
          :chips="[detailOption.groupLabel]"
        />
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { listAgentModels } from '@/api/agents';
import { fetchUserToolsCatalog } from '@/api/userTools';
import AgentPresetQuestionsField from '@/components/agent/AgentPresetQuestionsField.vue';
import AgentToolOptionLabel from '@/components/agent/AgentToolOptionLabel.vue';
import BeeroomGroupField from '@/components/beeroom/BeeroomGroupField.vue';
import AbilityTooltipCard from '@/components/common/AbilityTooltipCard.vue';
import { isDesktopModeEnabled } from '@/config/desktop';
import { useI18n } from '@/i18n';
import {
  buildAgentToolSections,
  collectToolValuesFromSections,
  resolveDefaultAgentToolNames,
  type AgentToolGroup,
  type AgentToolSection
} from '@/utils/agentToolCatalog';
import { normalizeAgentPresetQuestions } from '@/utils/agentPresetQuestions';
import { isAbilitySkillGroup } from '@/utils/abilityVisuals';
import {
  buildBeeroomGroupPayload,
  createBeeroomGroupDraft,
  type BeeroomGroupDraft,
  type BeeroomGroupOption
} from '@/utils/beeroomGroupDraft';
import { resolveToolUsageHint } from '@/utils/toolUsageHint';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

type ToolOption = {
  label: string;
  value: string;
  description: string;
  hint: string;
};

type ToolSection = AgentToolSection<ToolOption>;

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
  },
  beeroomGroups: {
    type: Array as () => BeeroomGroupOption[],
    default: () => []
  },
  defaultBeeroomGroupId: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['update:modelValue', 'submit']);
const { t } = useI18n();
const showApprovalModeSetting = computed(
  () => isDesktopModeEnabled()
);
const resolveDefaultApprovalMode = (): string =>
  'full_auto';

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));
const approvalModeOptions = computed(() => [
  { value: 'suggest', label: t('portal.agent.permission.option.suggest') },
  { value: 'auto_edit', label: t('portal.agent.permission.option.auto_edit') },
  { value: 'full_auto', label: t('portal.agent.permission.option.full_auto') }
]);

const resolveGroupAbilityKind = (groupKey: string): 'tool' | 'skill' =>
  isAbilitySkillGroup(groupKey) ? 'skill' : 'tool';

const toolLoading = ref(false);
const toolError = ref('');
const toolSummary = ref<Record<string, unknown> | null>(null);
const modelLoading = ref(false);
const availableModelNames = ref<string[]>([]);
const defaultModelName = ref('');
const saving = ref(false);

type DetailPopupData = {
  option: ToolOption;
  kind: 'tool' | 'skill';
  groupKey: string;
  groupLabel: string;
};

const detailOption = ref<DetailPopupData | null>(null);
const detailPos = ref({ x: 0, y: 0 });

function showToolDetail(event: MouseEvent, group: AgentToolGroup<ToolOption>, tool: ToolOption): void {
  const x = Math.min(event.clientX, window.innerWidth - 380);
  const y = Math.min(event.clientY, window.innerHeight - 220);
  detailPos.value = { x, y };
  detailOption.value = {
    option: tool,
    kind: resolveGroupAbilityKind(group.key),
    groupKey: group.key,
    groupLabel: group.label
  };
}

const detailPopupStyle = computed(() => ({
  left: `${detailPos.value.x}px`,
  top: `${detailPos.value.y}px`
}));

const form = reactive({
  name: '',
  description: '',
  copy_from_agent_id: DEFAULT_AGENT_KEY,
  group: createBeeroomGroupDraft(),
  system_prompt: '',
  model_name: '',
  tool_names: [] as string[],
  preset_questions: [] as string[],
  is_shared: false,
  sandbox_container_id: 1,
  approval_mode: resolveDefaultApprovalMode()
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
    return { label: value, value, description: '', hint: value };
  }
  const source = item as Record<string, unknown>;
  const value = String(
    source.runtime_name || source.runtimeName || source.name || source.tool_name || source.toolName || source.id || ''
  ).trim();
  if (!value) return null;
  const label = String(source.display_name || source.displayName || source.title || source.label || value).trim() || value;
  const option: ToolOption = {
    label,
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

const toolSections = computed<ToolSection[]>(() =>
  buildAgentToolSections(toolSummary.value, t, normalizeOption)
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

const allToolValues = computed(() => {
  return collectToolValuesFromSections(toolSections.value);
});

const resetForm = () => {
  form.name = '';
  form.description = '';
  form.copy_from_agent_id = DEFAULT_AGENT_KEY;
  form.group = createBeeroomGroupDraft(String(props.defaultBeeroomGroupId || '').trim()) as BeeroomGroupDraft;
  form.system_prompt = '';
  form.model_name = '';
  form.tool_names = resolveDefaultAgentToolNames(toolSummary.value, toolSections.value);
  form.preset_questions = [];
  form.is_shared = false;
  form.sandbox_container_id = 1;
  form.approval_mode = resolveDefaultApprovalMode();
};

const loadToolSummary = async () => {
  if (toolLoading.value) return;
  toolLoading.value = true;
  toolError.value = '';
  try {
    const result = await fetchUserToolsCatalog();
    toolSummary.value = result?.data?.data || {};
  } catch (error: any) {
    toolError.value = String(error?.response?.data?.detail || error?.message || t('common.requestFailed'));
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

const isGroupFullSelected = (group: AgentToolGroup<ToolOption>) => {
  if (!group.options.length) return false;
  const selected = new Set(form.tool_names);
  return group.options.every((option) => selected.has(option.value));
};

const toggleGroup = (group: AgentToolGroup<ToolOption>) => {
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
  if (!showApprovalModeSetting.value) return 'full_auto';
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return resolveDefaultApprovalMode();
};

const handleSave = async () => {
  const name = String(form.name || '').trim();
  if (!name) {
    ElMessage.warning(t('portal.agent.nameRequired'));
    return;
  }
  saving.value = true;
  try {
    const payload: Record<string, unknown> = {
      name,
      description: String(form.description || '').trim(),
      copy_from_agent_id: String(form.copy_from_agent_id || DEFAULT_AGENT_KEY).trim() || DEFAULT_AGENT_KEY,
      ...buildBeeroomGroupPayload(form.group, props.beeroomGroups),
      system_prompt: String(form.system_prompt || ''),
      model_name: String(form.model_name || '').trim(),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      preset_questions: normalizeAgentPresetQuestions(form.preset_questions),
      is_shared: false,
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      approval_mode: normalizeApprovalMode(form.approval_mode)
    };
    if (!payload.hive_name) {
      delete payload.hive_name;
    }
    if (!payload.hive_description) {
      delete payload.hive_description;
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
    await Promise.all([loadToolSummary(), loadModelOptions()]);
    resetForm();
  }
);

watch(
  () => allToolValues.value.join(','),
  () => {
    if (!visible.value) return;
    if (form.tool_names.length === 0) {
      form.tool_names = resolveDefaultAgentToolNames(toolSummary.value, toolSections.value);
    }
  }
);
</script>




