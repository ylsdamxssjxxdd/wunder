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
        <el-form-item class="agent-form-item agent-form-item--prompt" :label="t('portal.agent.form.prompt')">
          <el-input
            v-model="form.system_prompt"
            type="textarea"
            :rows="8"
            :placeholder="t('portal.agent.form.placeholder.prompt')"
          />
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
            <div v-if="sharedToolsNotice" class="agent-editor-hint">{{ t('portal.agent.tools.notice') }}</div>
          </div>
        </el-form-item>
        <el-form-item class="agent-form-item agent-form-item--base" :label="t('portal.agent.form.base')">
          <div class="agent-basic-settings">
            <div class="agent-avatar-card">
              <div class="agent-avatar-header">
                <div class="agent-avatar-header-left">
                  <div class="agent-avatar-title">{{ t('portal.agent.avatarTitle') }}</div>
                  <div
                    class="agent-avatar-preview agent-avatar-preview--toggle"
                    role="button"
                    tabindex="0"
                    :aria-expanded="avatarPanelVisible"
                    :style="getAvatarStyle({ name: form.icon_name, color: form.icon_color })"
                    @click="avatarPanelVisible = !avatarPanelVisible"
                    @keydown.enter="avatarPanelVisible = !avatarPanelVisible"
                  >
                    <span v-if="form.icon_name === DEFAULT_ICON_NAME" class="agent-avatar-option-text">Aa</span>
                    <i
                      v-else-if="getAvatarIconOption(form.icon_name)"
                      class="agent-avatar-option-icon"
                      :class="['fa-solid', getAvatarIconOption(form.icon_name).fa]"
                      aria-hidden="true"
                    ></i>
                    <span v-else class="agent-avatar-option-text">Aa</span>
                  </div>
                </div>
              </div>
              <div v-show="avatarPanelVisible" class="agent-avatar-panel">
                <div class="agent-avatar-section">
                  <div class="agent-avatar-section-title">{{ t('portal.agent.avatarIcon') }}</div>
                  <div class="agent-avatar-options">
                    <button
                      v-for="option in avatarIconOptions"
                      :key="option.name"
                      class="agent-avatar-option"
                      :class="{ active: form.icon_name === option.name }"
                      type="button"
                      :title="option.label"
                      @click="selectAvatarIcon(option)"
                    >
                      <span v-if="option.name === DEFAULT_ICON_NAME" class="agent-avatar-option-text">Aa</span>
                      <i v-else class="agent-avatar-option-icon" :class="['fa-solid', option.fa]" aria-hidden="true"></i>
                    </button>
                  </div>
                </div>
                <div class="agent-avatar-section">
                  <div class="agent-avatar-section-title">{{ t('portal.agent.avatarColor') }}</div>
                  <div class="agent-avatar-colors">
                    <button
                      v-for="color in avatarColorOptions"
                      :key="color || 'default'"
                      class="agent-avatar-color"
                      :class="{ active: (form.icon_color || '') === (color || '') }"
                      type="button"
                      :title="color || 'Aa'"
                      :style="color ? { background: color } : {}"
                      @click="selectAvatarColor(color)"
                    >
                      <span v-if="!color" class="agent-avatar-color-text">Aa</span>
                    </button>
                  </div>
                  <div class="agent-avatar-custom">
                    <input
                      class="agent-avatar-custom-input"
                      type="color"
                      :value="customColor || '#6ad9ff'"
                      @input="updateCustomColor($event.target.value)"
                    />
                    <input
                      class="agent-avatar-custom-text"
                      type="text"
                      :value="customColor"
                      :placeholder="t('portal.agent.avatarCustom')"
                      @input="updateCustomColor($event.target.value)"
                    />
                  </div>
                </div>
              </div>
            </div>
            <div class="agent-share-card agent-share-card--combined">
              <div class="agent-share-title">{{ t('portal.agent.share.title') }}</div>
              <div class="agent-share-row">
                <el-switch v-model="form.is_shared" />
                <span>{{ t('portal.agent.share.label') }}</span>
              </div>
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
              <div class="agent-editor-hint">{{ t('portal.agent.sandbox.hint') }}</div>
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
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import { fetchUserToolsSummary } from '@/api/userTools';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
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

const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const normalizedAgentId = computed(() => String(props.agentId || '').trim());
const canEdit = computed(() => Boolean(normalizedAgentId.value));

const DEFAULT_ICON_NAME = 'initial';

const AVATAR_ICON_CLASS_MAP = {
  chat: 'fa-comment-dots',
  bot: 'fa-robot',
  idea: 'fa-lightbulb',
  target: 'fa-bullseye',
  bolt: 'fa-bolt',
  code: 'fa-code',
  chart: 'fa-chart-line',
  doc: 'fa-file-lines',
  pen: 'fa-pen-nib',
  calendar: 'fa-calendar-days',
  briefcase: 'fa-briefcase',
  clipboard: 'fa-clipboard-list',
  book: 'fa-book-open',
  check: 'fa-check',
  shield: 'fa-shield-halved',
  spark: 'fa-wand-sparkles'
};

const AVATAR_ICON_OPTIONS = [
  { name: DEFAULT_ICON_NAME, labelKey: 'portal.agent.avatar.icon.initial' },
  { name: 'chat', labelKey: 'portal.agent.avatar.icon.chat' },
  { name: 'bot', labelKey: 'portal.agent.avatar.icon.bot' },
  { name: 'idea', labelKey: 'portal.agent.avatar.icon.idea' },
  { name: 'target', labelKey: 'portal.agent.avatar.icon.target' },
  { name: 'bolt', labelKey: 'portal.agent.avatar.icon.bolt' },
  { name: 'code', labelKey: 'portal.agent.avatar.icon.code' },
  { name: 'chart', labelKey: 'portal.agent.avatar.icon.chart' },
  { name: 'doc', labelKey: 'portal.agent.avatar.icon.doc' },
  { name: 'pen', labelKey: 'portal.agent.avatar.icon.pen' },
  { name: 'calendar', labelKey: 'portal.agent.avatar.icon.calendar' },
  { name: 'briefcase', labelKey: 'portal.agent.avatar.icon.briefcase' },
  { name: 'clipboard', labelKey: 'portal.agent.avatar.icon.clipboard' },
  { name: 'book', labelKey: 'portal.agent.avatar.icon.book' },
  { name: 'check', labelKey: 'portal.agent.avatar.icon.check' },
  { name: 'shield', labelKey: 'portal.agent.avatar.icon.shield' },
  { name: 'spark', labelKey: 'portal.agent.avatar.icon.spark' }
];

const avatarColorOptions = [
  '',
  '#6ad9ff',
  '#a78bfa',
  '#34d399',
  '#f472b6',
  '#fbbf24',
  '#60a5fa',
  '#f97316',
  '#22d3ee',
  '#94a3b8',
  '#f87171'
];

const sandboxContainerOptions = Object.freeze(Array.from({ length: 10 }, (_, index) => index + 1));

const normalizeSandboxContainerId = (value) => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  if (!Number.isFinite(parsed)) return 1;
  return Math.min(10, Math.max(1, parsed));
};

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  system_prompt: '',
  tool_names: [],
  sandbox_container_id: 1,
  icon_name: DEFAULT_ICON_NAME,
  icon_color: ''
});

const saving = ref(false);
const toolSummary = ref(null);
const toolLoading = ref(false);
const toolError = ref('');

const customColor = ref('');
const avatarPanelVisible = ref(true);

AVATAR_ICON_OPTIONS.forEach((option) => {
  if (!option || option.name === DEFAULT_ICON_NAME) return;
  option.fa = AVATAR_ICON_CLASS_MAP[option.name] || 'fa-circle';
});

const avatarIconOptions = computed(() =>
  AVATAR_ICON_OPTIONS.map((option) => ({
    ...option,
    label: t(option.labelKey)
  }))
);

const normalizeIconName = (name) => {
  const trimmed = String(name || '').trim();
  if (!trimmed) return DEFAULT_ICON_NAME;
  return AVATAR_ICON_OPTIONS.some((option) => option.name === trimmed) ? trimmed : DEFAULT_ICON_NAME;
};

const getAvatarIconOption = (name) => AVATAR_ICON_OPTIONS.find((option) => option.name === name);

const parseIconValue = (value) => {
  if (!value || typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed) return null;
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === 'object') return parsed;
    if (typeof parsed === 'string') return { name: parsed };
  } catch (error) {
    return { name: trimmed };
  }
  return { name: trimmed };
};

const getIconConfig = (value) => {
  const parsed = parseIconValue(value);
  return {
    name: normalizeIconName(parsed?.name),
    color: typeof parsed?.color === 'string' ? parsed.color : ''
  };
};

const applyIconToForm = (value) => {
  const config = getIconConfig(value);
  form.icon_name = config.name;
  form.icon_color = config.color || '';
  customColor.value = form.icon_color || '';
};

const selectAvatarIcon = (option) => {
  if (!option) return;
  form.icon_name = option.name;
};

const selectAvatarColor = (color) => {
  form.icon_color = color || '';
  customColor.value = color || '';
};

const updateCustomColor = (value) => {
  const next = String(value || '').trim();
  form.icon_color = next;
  customColor.value = next;
};

const hexToRgba = (hex, alpha) => {
  const trimmed = String(hex || '').trim();
  const match = trimmed.match(/^#?([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/);
  if (!match) return '';
  let value = match[1];
  if (value.length === 3) {
    value = value
      .split('')
      .map((part) => part + part)
      .join('');
  }
  const parsed = Number.parseInt(value, 16);
  const r = (parsed >> 16) & 255;
  const g = (parsed >> 8) & 255;
  const b = parsed & 255;
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
};

const getAvatarStyle = (config) => {
  if (!config?.color) return {};
  const strong = hexToRgba(config.color, 0.55);
  const soft = hexToRgba(config.color, 0.12);
  const border = hexToRgba(config.color, 0.6);
  if (!strong || !soft || !border) return {};
  const style = {
    background: `radial-gradient(circle at 30% 30%, ${strong}, ${soft})`,
    borderColor: border
  };
  if (config.name !== DEFAULT_ICON_NAME) {
    style.color = config.color;
  }
  return style;
};

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
  const sharedSelected = new Set(
    Array.isArray(summary.shared_tools_selected) ? summary.shared_tools_selected : []
  );
  const sharedPool = Array.isArray(summary.shared_tools) ? summary.shared_tools : [];
  const sharedTools =
    sharedSelected.size > 0
      ? sharedPool.filter((tool) => sharedSelected.has(String(tool?.name || '').trim()))
      : sharedPool;
  return [
    { label: t('portal.agent.tools.group.builtin'), options: normalizeOptions(summary.builtin_tools) },
    { label: t('portal.agent.tools.group.mcp'), options: normalizeOptions(summary.mcp_tools) },
    { label: t('portal.agent.tools.group.a2a'), options: normalizeOptions(summary.a2a_tools) },
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
    const agent = await agentStore.getAgent(normalizedAgentId.value, { force: true });
    if (!agent) {
      ElMessage.error(t('portal.agent.loadingFailed'));
      return;
    }
    form.name = agent.name || '';
    form.description = agent.description || '';
    form.is_shared = Boolean(agent.is_shared);
    form.system_prompt = agent.system_prompt || '';
    form.tool_names = Array.isArray(agent.tool_names) ? [...agent.tool_names] : [];
    form.sandbox_container_id = normalizeSandboxContainerId(agent.sandbox_container_id);
    applyIconToForm(agent.icon);
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
    const iconPayload = (() => {
      const iconName = normalizeIconName(form.icon_name);
      const color = String(form.icon_color || '').trim();
      if (iconName === DEFAULT_ICON_NAME && !color) return '';
      const payload = { name: iconName };
      if (color) {
        payload.color = color;
      }
      return JSON.stringify(payload);
    })();
    const payload = {
      name,
      description: form.description || '',
      is_shared: Boolean(form.is_shared),
      tool_names: Array.isArray(form.tool_names) ? form.tool_names : [],
      system_prompt: form.system_prompt || '',
      sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
      icon: iconPayload
    };
    await agentStore.updateAgent(normalizedAgentId.value, payload);
    ElMessage.success(t('portal.agent.updateSuccess'));
    visible.value = false;
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
      avatarPanelVisible.value = true;
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
