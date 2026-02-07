<template>
  <el-dialog
    v-model="visible"
    class="feature-window-dialog feature-window-dialog--agent"
    width="1080px"
    top="8vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="feature-window-header">
        <div class="feature-window-title">{{ t('chat.features.agentSettings') }}</div>
        <button class="feature-window-close" type="button" @click="visible = false">×</button>
      </div>
    </template>
    <div class="feature-window-body">
      <div v-if="!canEdit" class="feature-window-empty">{{ t('chat.features.agentMissing') }}</div>
      <template v-else>
        <div class="feature-window-editable">
          <div class="feature-window-form-grid">
            <label class="feature-window-field">
              <span>{{ t('portal.agent.form.name') }}</span>
              <input v-model="form.name" class="feature-window-input" :placeholder="t('portal.agent.form.placeholder.name')" />
            </label>
            <label class="feature-window-field">
              <span>{{ t('portal.agent.form.description') }}</span>
              <input
                v-model="form.description"
                class="feature-window-input"
                :placeholder="t('portal.agent.form.placeholder.description')"
              />
            </label>
            <label class="feature-window-field feature-window-field-full feature-window-avatar-field">
              <span>{{ t('portal.agent.avatarTitle') }}</span>
              <div class="feature-window-avatar-card">
                <div class="feature-window-avatar-header">
                  <button
                    class="feature-window-avatar-trigger"
                    type="button"
                    :aria-expanded="avatarPanelVisible"
                    @click="avatarPanelVisible = !avatarPanelVisible"
                  >
                    <div class="feature-window-avatar-preview" :style="getAvatarStyle({ name: form.icon_name, color: form.icon_color })">
                      <span v-if="form.icon_name === DEFAULT_ICON_NAME" class="feature-window-avatar-text">Aa</span>
                      <i
                        v-else-if="getAvatarIconOption(form.icon_name)"
                        class="feature-window-avatar-icon"
                        :class="['fa-solid', getAvatarIconOption(form.icon_name).fa]"
                        aria-hidden="true"
                      ></i>
                      <span v-else class="feature-window-avatar-text">Aa</span>
                    </div>
                    <i
                      class="fa-solid feature-window-avatar-chevron"
                      :class="avatarPanelVisible ? 'fa-chevron-up' : 'fa-chevron-down'"
                      aria-hidden="true"
                    ></i>
                  </button>
                </div>
                <div v-show="avatarPanelVisible" class="feature-window-avatar-panel">
                  <div class="feature-window-avatar-row">
                    <div class="feature-window-avatar-custom">
                      <input
                        class="feature-window-avatar-color"
                        type="color"
                        :value="customColor || '#6ad9ff'"
                        @input="updateCustomColor($event.target.value)"
                      />
                      <input
                        class="feature-window-input"
                        type="text"
                        :value="customColor"
                        :placeholder="t('portal.agent.avatarCustom')"
                        @input="updateCustomColor($event.target.value)"
                      />
                    </div>
                  </div>
                  <div class="feature-window-avatar-icons">
                    <button
                      v-for="option in avatarIconOptions"
                      :key="option.name"
                      class="feature-window-avatar-option"
                      :class="{ active: form.icon_name === option.name }"
                      type="button"
                      :title="option.label"
                      @click="selectAvatarIcon(option)"
                    >
                      <span v-if="option.name === DEFAULT_ICON_NAME" class="feature-window-avatar-text">Aa</span>
                      <i v-else class="feature-window-avatar-icon" :class="['fa-solid', option.fa]" aria-hidden="true"></i>
                    </button>
                  </div>
                  <div class="feature-window-avatar-colors">
                    <button
                      v-for="color in avatarColorOptions"
                      :key="color || 'default'"
                      class="feature-window-avatar-swatch"
                      :class="{ active: (form.icon_color || '') === (color || '') }"
                      type="button"
                      :title="color ? color : t('portal.agent.avatarDefault')"
                      :style="color ? { background: color } : {}"
                      @click="selectAvatarColor(color)"
                    >
                      <span v-if="!color" class="feature-window-avatar-default">{{ t('portal.agent.avatarDefault') }}</span>
                    </button>
                  </div>
                </div>
              </div>
            </label>
            <label class="feature-window-field feature-window-field-full">
              <span>{{ t('portal.agent.form.prompt') }}</span>
              <textarea
                v-model="form.system_prompt"
                class="feature-window-input feature-window-textarea"
                :placeholder="t('portal.agent.form.placeholder.prompt')"
              ></textarea>
            </label>
          </div>
          <div class="feature-window-toolbar">
            <div class="feature-window-hint">{{ t('portal.agent.form.tools') }}</div>
          </div>
          <div class="feature-window-tool-panel">
            <div v-if="toolLoading" class="feature-window-empty">{{ t('portal.agent.tools.loading') }}</div>
            <div v-else-if="toolError" class="feature-window-empty">{{ toolError }}</div>
            <div v-else-if="!toolGroups.length" class="feature-window-empty">{{ t('portal.agent.tools.loadFailed') }}</div>
            <template v-else>
              <div v-for="group in toolGroups" :key="group.label" class="feature-window-tool-group">
                <div class="feature-window-tool-title">{{ group.label }}</div>
                <label v-for="item in group.items" :key="item.name" class="feature-window-tool-item">
                  <input
                    type="checkbox"
                    :checked="isToolSelected(item.name)"
                    @change="toggleTool(item.name, $event.target.checked)"
                  />
                  <div class="feature-window-tool-meta">
                    <div class="feature-window-tool-name">{{ item.name }}</div>
                    <div class="feature-window-tool-desc">{{ item.description || t('chat.ability.noDesc') }}</div>
                  </div>
                </label>
              </div>
            </template>
          </div>
          <label class="feature-window-checkbox">
            <input v-model="form.is_shared" type="checkbox" />
            <span>{{ t('portal.agent.share.label') }}</span>
          </label>
        </div>
      </template>
    </div>
    <template #footer>
      <div class="feature-window-actions">
        <button class="feature-window-btn danger" type="button" :disabled="!canEdit" @click="deleteAgent">
          {{ t('portal.agent.delete') }}
        </button>
        <button class="feature-window-btn primary" type="button" :disabled="saving || !canEdit" @click="saveAgent">
          {{ saving ? t('common.saving') : t('portal.agent.save') }}
        </button>
      </div>
    </template>
  </el-dialog>
</template>

<script setup>
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

const form = reactive({
  name: '',
  description: '',
  is_shared: false,
  system_prompt: '',
  tool_names: [],
  icon_name: DEFAULT_ICON_NAME,
  icon_color: ''
});

const saving = ref(false);
const toolSummary = ref(null);
const toolLoading = ref(false);
const toolError = ref('');

const customColor = ref('');

const avatarPanelVisible = ref(false);

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

const normalizeToolItem = (item) => {
  if (!item) return null;
  if (typeof item === 'string') {
    const name = item.trim();
    return name ? { name, description: '' } : null;
  }
  const name = String(item.name || item.tool_name || item.toolName || item.id || '').trim();
  if (!name) return null;
  return {
    name,
    description: String(item.description || '').trim()
  };
};

const filterToolItems = (list) =>
  Array.isArray(list) ? list.map((item) => normalizeToolItem(item)).filter(Boolean) : [];

const toolGroups = computed(() => {
  const summary = toolSummary.value || {};
  const groups = [
    { label: t('portal.agent.tools.group.builtin'), items: filterToolItems(summary.builtin_tools) },
    { label: t('portal.agent.tools.group.mcp'), items: filterToolItems(summary.mcp_tools) },
    { label: t('portal.agent.tools.group.a2a'), items: filterToolItems(summary.a2a_tools) },
    { label: t('portal.agent.tools.group.knowledge'), items: filterToolItems(summary.knowledge_tools) },
    { label: t('portal.agent.tools.group.user'), items: filterToolItems(summary.user_tools) },
    { label: t('portal.agent.tools.group.shared'), items: filterToolItems(summary.shared_tools) },
    { label: t('portal.agent.tools.group.skills'), items: filterToolItems(summary.skills) }
  ];
  return groups.filter((group) => group.items.length > 0);
});

const isToolSelected = (name) => Array.isArray(form.tool_names) && form.tool_names.includes(name);

const toggleTool = (name, checked) => {
  const current = Array.isArray(form.tool_names) ? [...form.tool_names] : [];
  if (checked) {
    if (!current.includes(name)) {
      current.push(name);
    }
  } else {
    const index = current.indexOf(name);
    if (index >= 0) {
      current.splice(index, 1);
    }
  }
  form.tool_names = current;
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
    await ElMessageBox.confirm(
      t('portal.agent.deleteConfirm', { name: targetName }),
      t('common.notice'),
      {
        confirmButtonText: t('portal.agent.delete'),
        cancelButtonText: t('portal.agent.cancel'),
        type: 'warning'
      }
    );
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
      avatarPanelVisible.value = false;
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

<style scoped>
:global(.feature-window-dialog--agent.el-dialog) {
  --fw-text: #e2e8f0;
  --fw-muted: #94a3b8;
  --fw-bg: linear-gradient(160deg, #070d1a, #0b1426);
  --fw-shadow: 0 24px 56px rgba(8, 12, 24, 0.55);
  --fw-border: rgba(51, 65, 85, 0.72);
  --fw-border-soft: rgba(51, 65, 85, 0.62);
  --fw-divider: rgba(51, 65, 85, 0.62);
  --fw-surface: #0b1527;
  --fw-surface-alt: #0d182c;
  --fw-control-bg: #111c31;
  --fw-control-hover: #162844;
  --fw-focus-border: rgba(56, 189, 248, 0.65);
  --fw-focus-ring: rgba(56, 189, 248, 0.18);
  --fw-accent-border: rgba(77, 216, 255, 0.5);
  --fw-accent-shadow: rgba(77, 216, 255, 0.22);
  --fw-danger: #fca5a5;
  --fw-danger-border: rgba(248, 113, 113, 0.4);
  width: min(96vw, 1080px) !important;
  max-width: 1080px;
  height: min(82vh, 760px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--fw-bg);
  border: 1px solid var(--fw-border);
  border-radius: 14px;
  box-shadow: var(--fw-shadow);
  color: var(--fw-text);
  color-scheme: dark;
}

:global(:root[data-user-theme='light'] .feature-window-dialog--agent.el-dialog) {
  --fw-text: #0f172a;
  --fw-muted: #64748b;
  --fw-bg: linear-gradient(180deg, #ffffff, #f7faff);
  --fw-shadow: 0 18px 40px rgba(15, 23, 42, 0.16);
  --fw-border: rgba(148, 163, 184, 0.52);
  --fw-border-soft: rgba(148, 163, 184, 0.36);
  --fw-divider: rgba(148, 163, 184, 0.42);
  --fw-surface: #f8fafc;
  --fw-surface-alt: #ffffff;
  --fw-control-bg: #f1f5f9;
  --fw-control-hover: #e2e8f0;
  --fw-focus-border: rgba(37, 99, 235, 0.55);
  --fw-focus-ring: rgba(37, 99, 235, 0.16);
  --fw-accent-border: rgba(37, 99, 235, 0.42);
  --fw-accent-shadow: rgba(37, 99, 235, 0.22);
  --fw-danger: #b91c1c;
  --fw-danger-border: rgba(220, 38, 38, 0.32);
  color-scheme: light;
}

:global(.feature-window-dialog--agent .el-dialog__header) {
  border-bottom: 1px solid var(--fw-divider);
  padding: 14px 18px;
}

:global(.feature-window-dialog--agent .el-dialog__body) {
  padding: 16px 18px 18px;
  color: var(--fw-text);
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

:global(.feature-window-dialog--agent .el-dialog__footer) {
  border-top: 1px solid var(--fw-divider);
  padding: 12px 18px 16px;
}

.feature-window-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-title {
  font-size: 15px;
  font-weight: 700;
}

.feature-window-close {
  width: 30px;
  height: 30px;
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  cursor: pointer;
}

.feature-window-close:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
}

.feature-window-close:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.feature-window-editable {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  padding-right: 2px;
}

.feature-window-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-hint {
  color: var(--fw-muted);
  font-size: 12px;
}

.feature-window-form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
  flex: 0 0 auto;
}

.feature-window-field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
}

.feature-window-field-full {
  grid-column: 1 / -1;
}

.feature-window-input {
  border: 1px solid var(--fw-border);
  border-radius: 8px;
  background: var(--fw-surface-alt);
  color: var(--fw-text);
  padding: 7px 9px;
  font-size: 12px;
  outline: none;
}

.feature-window-input::placeholder {
  color: var(--fw-muted);
}

.feature-window-input:focus {
  border-color: var(--fw-focus-border);
  box-shadow: 0 0 0 2px var(--fw-focus-ring);
}

.feature-window-input option {
  background: var(--fw-surface);
  color: var(--fw-text);
}

.feature-window-textarea {
  min-height: 96px;
  resize: vertical;
}

.feature-window-avatar-card {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.feature-window-avatar-header {
  display: flex;
  align-items: center;
}

.feature-window-avatar-trigger {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface-alt);
  color: inherit;
  min-height: 44px;
  padding: 6px 10px;
  display: inline-flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
}

.feature-window-avatar-trigger:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
}

.feature-window-avatar-trigger:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-avatar-chevron {
  color: var(--fw-muted);
  font-size: 12px;
}

.feature-window-avatar-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.feature-window-avatar-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.feature-window-avatar-preview {
  width: 42px;
  height: 42px;
  border-radius: 12px;
  border: 1px solid var(--fw-border);
  background: linear-gradient(145deg, var(--fw-control-bg), var(--fw-surface-alt));
  color: var(--fw-text);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.feature-window-avatar-icon {
  font-size: 16px;
}

.feature-window-avatar-text {
  font-weight: 700;
  font-size: 13px;
}

.feature-window-avatar-custom {
  display: grid;
  grid-template-columns: 42px minmax(0, 1fr);
  gap: 8px;
  flex: 1;
  min-width: 0;
}

.feature-window-avatar-color {
  width: 42px;
  height: 32px;
  border-radius: 8px;
  border: 1px solid var(--fw-border);
  background: transparent;
  padding: 2px;
  cursor: pointer;
}

.feature-window-avatar-icons {
  display: grid;
  grid-template-columns: repeat(9, minmax(0, 1fr));
  gap: 8px;
}

.feature-window-avatar-option {
  border: 1px solid var(--fw-border);
  border-radius: 8px;
  background: var(--fw-surface-alt);
  color: var(--fw-text);
  height: 34px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.feature-window-avatar-option.active {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-avatar-option:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
}

.feature-window-avatar-colors {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.feature-window-avatar-swatch {
  min-width: 32px;
  height: 28px;
  border-radius: 999px;
  border: 1px solid var(--fw-border);
  background: var(--fw-surface-alt);
  color: var(--fw-muted);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0 8px;
  cursor: pointer;
}

.feature-window-avatar-swatch.active {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-avatar-default {
  font-size: 11px;
  white-space: nowrap;
}

.feature-window-checkbox {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.feature-window-tool-panel {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface);
  padding: 10px;
  flex: 1 1 auto;
  min-height: 260px;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.feature-window-tool-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.feature-window-tool-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--fw-muted);
}

.feature-window-tool-item {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  border: 1px solid var(--fw-border-soft);
  border-radius: 8px;
  padding: 7px 8px;
  background: var(--fw-surface-alt);
}

.feature-window-tool-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.feature-window-tool-name {
  font-size: 12px;
  font-weight: 600;
}

.feature-window-tool-desc {
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.feature-window-btn {
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
}

.feature-window-btn:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
  color: var(--fw-text);
}

.feature-window-btn:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-btn.primary {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-btn.danger {
  border-color: var(--fw-danger-border);
  color: var(--fw-danger);
}

.feature-window-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.feature-window-empty {
  color: var(--fw-muted);
  font-size: 12px;
  text-align: center;
  padding: 12px;
}

@media (max-width: 900px) {
  .feature-window-form-grid {
    grid-template-columns: 1fr;
  }

  .feature-window-avatar-icons {
    grid-template-columns: repeat(6, minmax(0, 1fr));
  }

  .feature-window-avatar-custom {
    grid-template-columns: 36px minmax(0, 1fr);
  }

  .feature-window-tool-panel {
    min-height: 180px;
  }

  .feature-window-toolbar {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>