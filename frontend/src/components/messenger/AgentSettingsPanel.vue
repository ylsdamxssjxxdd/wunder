<template>
  <div class="messenger-agent-settings">
    <div v-if="!canView" class="messenger-list-empty">
      {{ t('chat.features.agentMissing') }}
    </div>

    <template v-else>
      <el-alert
        v-if="canEdit && hasUnsavedChanges"
        :title="t('messenger.agentSettings.unsavedHint')"
        type="warning"
        show-icon
        :closable="false"
      />
      <el-form :model="form" label-position="top" class="messenger-agent-form messenger-form">
        <el-form-item :label="t('portal.agent.form.name')" class="messenger-agent-form-item">
          <div class="messenger-agent-name-row">
            <el-input
              v-model="form.name"
              class="messenger-agent-field"
              :placeholder="t('portal.agent.form.placeholder.name')"
              :disabled="isReadonlyMode"
            />
            <button
              class="messenger-agent-avatar-trigger"
              type="button"
              :disabled="isReadonlyMode"
              :title="t('portal.agent.avatarTitle')"
              :aria-label="t('portal.agent.avatarTitle')"
              @click.prevent="openAvatarDialog"
            >
              <span class="messenger-agent-avatar-trigger-preview" :style="agentAvatarPreviewStyle" aria-hidden="true">
                <img
                  v-if="agentAvatarPreviewImageUrl"
                  class="messenger-settings-profile-avatar-image"
                  :src="agentAvatarPreviewImageUrl"
                  alt=""
                />
                <span v-else>{{ agentAvatarInitial }}</span>
              </span>
              <span class="messenger-agent-avatar-trigger-text">{{ t('portal.agent.avatarToggle') }}</span>
            </button>
          </div>
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
            <div class="messenger-agent-base-item">
              <div class="messenger-agent-base-label">{{ t('messenger.agentGroup.label') }}</div>
              <div class="messenger-agent-base-control">
                <BeeroomGroupField
                  v-model="form.group"
                  :groups="beeroomGroupOptions"
                  :allow-create="false"
                  :disabled="isReadonlyMode"
                />
              </div>
            </div>
            <div ref="modelSectionRef" class="messenger-agent-base-item">
              <div class="messenger-agent-base-label">{{ t('portal.agent.model.title') }}</div>
              <div class="messenger-agent-base-control">
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
            </div>
            <div class="messenger-agent-base-item">
              <div class="messenger-agent-base-label">{{ t('portal.agent.sandbox.title') }}</div>
              <div class="messenger-agent-base-control">
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
            </div>
            <div v-if="showApprovalModeSetting" class="messenger-agent-base-item">
              <div class="messenger-agent-base-label">{{ t('portal.agent.permission.title') }}</div>
              <div class="messenger-agent-base-control">
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
          </div>
        </el-form-item>
        <el-form-item class="messenger-agent-form-item">
          <template #label>
            <div class="messenger-agent-section-head">
              <span>{{ t('portal.agent.form.presetQuestions') }}</span>
              <button
                v-if="!isReadonlyMode"
                class="messenger-agent-icon-btn"
                type="button"
                :title="t('portal.agent.presetQuestions.add')"
                :aria-label="t('portal.agent.presetQuestions.add')"
                @click.prevent="addPresetQuestion"
              >
                <i class="fa-solid fa-plus" aria-hidden="true"></i>
              </button>
            </div>
          </template>
          <AgentPresetQuestionsField
            ref="presetQuestionsFieldRef"
            v-model="form.preset_questions"
            :readonly="isReadonlyMode"
          />
        </el-form-item>
      </el-form>

      <el-dialog
        v-model="avatarDialogVisible"
        class="messenger-dialog messenger-avatar-dialog"
        :title="t('portal.agent.avatarTitle')"
        width="420px"
        :close-on-click-modal="false"
        append-to-body
        destroy-on-close
      >
        <div class="messenger-avatar-dialog-body">
          <div class="messenger-avatar-dialog-preview">
            <div class="messenger-settings-profile-avatar messenger-settings-profile-avatar--dialog" :style="avatarDialogPreviewStyle">
              <img
                v-if="avatarDialogImageUrl"
                class="messenger-settings-profile-avatar-image"
                :src="avatarDialogImageUrl"
                alt=""
              />
              <span v-else>{{ agentAvatarInitial }}</span>
            </div>
            <div class="messenger-settings-hint">{{ t('profile.avatar.tip') }}</div>
          </div>
          <div class="messenger-settings-label">{{ t('portal.agent.avatarIcon') }}</div>
          <div class="messenger-settings-avatar-icon-grid">
            <button
              v-for="item in pagedAvatarOptions"
              :key="item.key"
              class="messenger-settings-avatar-icon-btn"
              :class="{ active: avatarDialogIcon === item.key }"
              type="button"
              :title="item.label"
              :aria-label="item.label"
              @click="avatarDialogIcon = item.key"
            >
              <img
                v-if="item.image"
                class="messenger-settings-avatar-option-image"
                :src="item.image"
                alt=""
              />
              <span v-else>{{ agentAvatarInitial }}</span>
            </button>
          </div>
          <div v-if="avatarPageCount > 1" class="messenger-avatar-dialog-pager">
            <button
              class="messenger-settings-action ghost compact"
              type="button"
              :disabled="avatarPage <= 1"
              @click="avatarPage = Math.max(1, avatarPage - 1)"
            >
              {{ t('profile.avatar.pagePrev') }}
            </button>
            <span class="messenger-settings-hint">
              {{ t('profile.avatar.pageIndicator', { current: avatarPage, total: avatarPageCount }) }}
            </span>
            <button
              class="messenger-settings-action ghost compact"
              type="button"
              :disabled="avatarPage >= avatarPageCount"
              @click="avatarPage = Math.min(avatarPageCount, avatarPage + 1)"
            >
              {{ t('profile.avatar.pageNext') }}
            </button>
          </div>
          <div v-if="!avatarDialogImageUrl" class="messenger-settings-row messenger-settings-row--compact">
            <div class="messenger-settings-label">{{ t('portal.agent.avatarColor') }}</div>
            <div class="messenger-settings-avatar-color-select-wrap">
              <span class="messenger-settings-avatar-color-chip" :style="{ '--avatar-color': avatarDialogColor }"></span>
              <select v-model="avatarDialogColor" class="messenger-settings-select messenger-settings-select--avatar">
                <option v-for="item in avatarColorOptions" :key="item.value" :value="item.value">
                  {{ item.label }}
                </option>
              </select>
            </div>
          </div>
        </div>
        <template #footer>
          <div class="messenger-avatar-dialog-footer">
            <button class="messenger-settings-action ghost" type="button" @click="resetAvatarDialog">
              {{ t('common.reset') }}
            </button>
            <div class="messenger-avatar-dialog-footer-actions">
              <button class="messenger-settings-action ghost" type="button" @click="closeAvatarDialog">
                {{ t('common.cancel') }}
              </button>
              <button class="messenger-settings-action" type="button" @click="applyAvatarDialog">
                {{ t('common.confirm') }}
              </button>
            </div>
          </div>
        </template>
      </el-dialog>

    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';
import { onBeforeRouteLeave, onBeforeRouteUpdate } from 'vue-router';

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
import {
  AGENT_AVATAR_COLORS,
  AGENT_AVATAR_IMAGE_OPTIONS,
  AGENT_AVATAR_OPTION_KEYS,
  DEFAULT_AGENT_AVATAR_IMAGE_KEY,
  resolveAgentAvatarImageByKey
} from '@/utils/agentAvatarCatalog';
import {
  parseAgentAvatarIconConfig,
  resolveAgentAvatarInitial,
  stringifyAgentAvatarIconConfig
} from '@/utils/agentAvatar';
import { showApiError } from '@/utils/apiError';
import { onUserToolsUpdated } from '@/utils/userToolsEvents';
import { DEFAULT_AVATAR_COLOR, normalizeAvatarColor, normalizeAvatarIcon } from '@/utils/userPreferences';
import { registerUnsavedChangesGuard } from '@/utils/unsavedChangesGuard';

type ToolOption = {
  label: string;
  value: string;
  description: string;
  hint: string;
};

type ToolSection = AgentToolSection<ToolOption>;
type ProfileAvatarOption = {
  key: string;
  label: string;
  image?: string;
};
type AvatarColorOption = {
  value: string;
  label: string;
};

const AVATAR_PAGE_SIZE = 24;
const AVATAR_COLOR_LABEL_KEY_BY_HEX: Record<string, string> = {
  '#f97316': 'profile.avatar.color.sunset',
  '#ef4444': 'profile.avatar.color.coral',
  '#ec4899': 'profile.avatar.color.rose',
  '#8b5cf6': 'profile.avatar.color.violet',
  '#6366f1': 'profile.avatar.color.indigo',
  '#3b82f6': 'profile.avatar.color.sky',
  '#06b6d4': 'profile.avatar.color.cyan',
  '#14b8a6': 'profile.avatar.color.teal',
  '#10b981': 'profile.avatar.color.emerald',
  '#84cc16': 'profile.avatar.color.lime',
  '#f59e0b': 'profile.avatar.color.amber',
  '#64748b': 'profile.avatar.color.slate'
};

type AgentFormSnapshot = {
  name: string;
  description: string;
  system_prompt: string;
  model_name: string;
  tool_names: string[];
  preset_questions: string[];
  hive_id: string;
  hive_name: string;
  hive_description: string;
  sandbox_container_id: number;
  approval_mode: string;
  icon_name: string;
  icon_color: string;
};

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
  approval_mode: resolveDefaultApprovalMode(),
  icon_name: DEFAULT_AGENT_AVATAR_IMAGE_KEY,
  icon_color: DEFAULT_AVATAR_COLOR
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
const presetQuestionsFieldRef = ref<{ addQuestion: () => void } | null>(null);
const panelMounted = ref(false);
const loadedSnapshot = ref<AgentFormSnapshot | null>(null);
const avatarDialogVisible = ref(false);
const avatarDialogIcon = ref(DEFAULT_AGENT_AVATAR_IMAGE_KEY);
const avatarDialogColor = ref(DEFAULT_AVATAR_COLOR);
const avatarPage = ref(1);
let panelDisposed = false;
let latestAgentLoadRequestId = 0;
let lastHandledFocusToken = 0;
let focusAnimationFrame = 0;
let stopUserToolsUpdatedListener: (() => void) | null = null;
let stopUnsavedGuard: (() => void) | null = null;

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

const normalizeAgentAvatarName = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) return DEFAULT_AGENT_AVATAR_IMAGE_KEY;
  const normalized = normalizeAvatarIcon(text, AGENT_AVATAR_OPTION_KEYS);
  if (normalized === 'initial') {
    return 'initial';
  }
  return normalized;
};

const normalizeAgentAvatarColor = (value: unknown): string => normalizeAvatarColor(value || DEFAULT_AVATAR_COLOR);

const avatarOptions = computed<ProfileAvatarOption[]>(() => [
  {
    key: 'initial',
    label: t('portal.agent.avatar.icon.initial')
  },
  ...AGENT_AVATAR_IMAGE_OPTIONS
]);

const resolveAvatarOptionImage = (key: unknown): string =>
  resolveAgentAvatarImageByKey(normalizeAgentAvatarName(key));

const agentAvatarPreviewImageUrl = computed(() => resolveAvatarOptionImage(form.icon_name));
const avatarDialogImageUrl = computed(() => resolveAvatarOptionImage(avatarDialogIcon.value));
const agentAvatarPreviewStyle = computed(() => ({
  background: agentAvatarPreviewImageUrl.value ? 'transparent' : normalizeAgentAvatarColor(form.icon_color)
}));
const avatarDialogPreviewStyle = computed(() => ({
  background: avatarDialogImageUrl.value ? 'transparent' : normalizeAgentAvatarColor(avatarDialogColor.value)
}));
const agentAvatarInitial = computed(() =>
  resolveAgentAvatarInitial(String(form.name || '').trim() || normalizedAgentId.value || t('messenger.defaultAgent'))
);

const avatarPageCount = computed(() =>
  Math.max(1, Math.ceil(Math.max(0, avatarOptions.value.length) / AVATAR_PAGE_SIZE))
);

const pagedAvatarOptions = computed(() => {
  const page = Math.min(Math.max(avatarPage.value, 1), avatarPageCount.value);
  const start = (page - 1) * AVATAR_PAGE_SIZE;
  return avatarOptions.value.slice(start, start + AVATAR_PAGE_SIZE);
});

const avatarColorOptions = computed<AvatarColorOption[]>(() =>
  AGENT_AVATAR_COLORS.map((color) => {
    const normalized = String(color || '')
      .trim()
      .toLowerCase();
    const labelKey = AVATAR_COLOR_LABEL_KEY_BY_HEX[normalized];
    const label = labelKey ? t(labelKey) : t('profile.avatar.color.custom');
    return {
      value: color,
      label: `${label} · ${String(color || '').toUpperCase()}`
    };
  })
);

const resolveAvatarPageByKey = (key: string): number => {
  const normalized = normalizeAgentAvatarName(key);
  const index = avatarOptions.value.findIndex((item) => item.key === normalized);
  if (index < 0) return 1;
  return Math.floor(index / AVATAR_PAGE_SIZE) + 1;
};

const openAvatarDialog = () => {
  avatarDialogIcon.value = normalizeAgentAvatarName(form.icon_name);
  avatarDialogColor.value = normalizeAgentAvatarColor(form.icon_color);
  avatarPage.value = resolveAvatarPageByKey(avatarDialogIcon.value);
  avatarDialogVisible.value = true;
};

const closeAvatarDialog = () => {
  avatarDialogVisible.value = false;
};

const resetAvatarDialog = () => {
  avatarDialogIcon.value = DEFAULT_AGENT_AVATAR_IMAGE_KEY;
  avatarDialogColor.value = DEFAULT_AVATAR_COLOR;
  avatarPage.value = resolveAvatarPageByKey(avatarDialogIcon.value);
};

const applyAvatarDialog = () => {
  form.icon_name = normalizeAgentAvatarName(avatarDialogIcon.value);
  form.icon_color = normalizeAgentAvatarColor(avatarDialogColor.value);
  avatarDialogVisible.value = false;
};

const normalizeStringArrayForSnapshot = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  const unique = new Set<string>();
  value.forEach((item) => {
    const text = String(item || '').trim();
    if (!text) return;
    unique.add(text);
  });
  return Array.from(unique).sort((left, right) => left.localeCompare(right));
};

const buildFormSnapshot = (): AgentFormSnapshot => {
  const groupPayload = buildBeeroomGroupPayload(form.group);
  return {
    name: String(form.name || '').trim(),
    description: String(form.description || '').trim(),
    system_prompt: String(form.system_prompt || ''),
    model_name: String(form.model_name || '').trim(),
    tool_names: normalizeStringArrayForSnapshot(form.tool_names),
    preset_questions: normalizeAgentPresetQuestions(form.preset_questions),
    hive_id: String(groupPayload.hive_id || '').trim(),
    hive_name: String(groupPayload.hive_name || '').trim(),
    hive_description: String(groupPayload.hive_description || '').trim(),
    sandbox_container_id: normalizeSandboxContainerId(form.sandbox_container_id),
    approval_mode: normalizeApprovalMode(form.approval_mode),
    icon_name: normalizeAgentAvatarName(form.icon_name),
    icon_color: normalizeAgentAvatarColor(form.icon_color)
  };
};

const markFormClean = (): void => {
  loadedSnapshot.value = buildFormSnapshot();
};

const addPresetQuestion = () => {
  if (isReadonlyMode.value) {
    return;
  }
  presetQuestionsFieldRef.value?.addQuestion();
};

const hasUnsavedChanges = computed(() => {
  if (!canEdit.value || !loadedSnapshot.value) return false;
  const current = buildFormSnapshot();
  return JSON.stringify(current) !== JSON.stringify(loadedSnapshot.value);
});

watch(
  () => avatarOptions.value,
  () => {
    avatarPage.value = Math.min(Math.max(avatarPage.value, 1), avatarPageCount.value);
  },
  { immediate: true }
);

watch(avatarDialogIcon, (value) => {
  const targetPage = resolveAvatarPageByKey(String(value || ''));
  if (targetPage !== avatarPage.value) {
    avatarPage.value = targetPage;
  }
});

const confirmDiscardChanges = async (): Promise<boolean> => {
  if (!hasUnsavedChanges.value) {
    return true;
  }
  try {
    await ElMessageBox.confirm(t('messenger.agentSettings.confirmDiscard'), t('common.notice'), {
      type: 'warning',
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel')
    });
    return true;
  } catch {
    return false;
  }
};

const handleBeforeUnload = (event: BeforeUnloadEvent): void => {
  if (!hasUnsavedChanges.value) return;
  event.preventDefault();
  event.returnValue = '';
};

const handleGlobalKeydown = (event: KeyboardEvent): void => {
  if (!canEdit.value || !hasUnsavedChanges.value || saving.value) return;
  if (!(event.ctrlKey || event.metaKey)) return;
  if (String(event.key || '').toLowerCase() !== 's') return;
  event.preventDefault();
  void saveAgent();
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
  if (!canView.value) {
    loadedSnapshot.value = null;
    return;
  }
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
    const avatarConfig = parseAgentAvatarIconConfig(agent.icon);
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
    form.icon_name = normalizeAgentAvatarName(avatarConfig.name);
    form.icon_color = normalizeAgentAvatarColor(avatarConfig.color);
    markFormClean();
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
  if (hasUnsavedChanges.value) return;
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
      approval_mode: normalizeApprovalMode(form.approval_mode),
      icon: stringifyAgentAvatarIconConfig({
        name: form.icon_name,
        color: form.icon_color
      })
    };
    if (!payload.hive_name) delete payload.hive_name;
    if (!payload.hive_description) delete payload.hive_description;
    const updated = await agentStore.updateAgent(normalizedAgentId.value, payload);
    currentAgent.value = (updated as Record<string, unknown> | null) || currentAgent.value;
    markFormClean();
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
    icon: stringifyAgentAvatarIconConfig({
      name: form.icon_name,
      color: form.icon_color
    }),
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

onBeforeRouteLeave(async () => {
  const allowed = await confirmDiscardChanges();
  if (!allowed) {
    return false;
  }
  return true;
});

onBeforeRouteUpdate(async () => {
  const allowed = await confirmDiscardChanges();
  if (!allowed) {
    return false;
  }
  return true;
});

defineExpose({
  triggerReload: reloadAgent,
  triggerSave: saveAgent,
  triggerDelete: deleteAgent,
  triggerExportWorkerCard: exportWorkerCard,
  hasUnsavedChanges: () => hasUnsavedChanges.value,
  confirmDiscardChanges
});

onMounted(() => {
  panelMounted.value = true;
  stopUnsavedGuard = registerUnsavedChangesGuard('messenger-agent-settings', confirmDiscardChanges);
  stopUserToolsUpdatedListener = onUserToolsUpdated((event) => {
    const detail = event?.detail || {};
    const scope = String((detail as Record<string, unknown>).scope || '').trim().toLowerCase();
    if (scope && scope !== 'all' && scope !== 'skills' && scope !== 'mcp' && scope !== 'knowledge') {
      return;
    }
    void loadToolSummary();
  });
  window.addEventListener('beforeunload', handleBeforeUnload);
  window.addEventListener('keydown', handleGlobalKeydown);
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
  window.removeEventListener('beforeunload', handleBeforeUnload);
  window.removeEventListener('keydown', handleGlobalKeydown);
  window.removeEventListener('focus', refreshAgentFromExternalChange);
  document.removeEventListener('visibilitychange', handleAgentSettingsVisibilityChange);
  if (stopUnsavedGuard) {
    stopUnsavedGuard();
    stopUnsavedGuard = null;
  }
  if (stopUserToolsUpdatedListener) {
    stopUserToolsUpdatedListener();
    stopUserToolsUpdatedListener = null;
  }
  clearFocusAnimationFrame();
});
</script>

<style scoped>
.messenger-agent-name-row {
  width: 100%;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
}

.messenger-agent-avatar-trigger {
  height: 40px;
  padding: 0 10px 0 6px;
  border: 1px solid var(--el-border-color, rgba(148, 163, 184, 0.3));
  border-radius: 10px;
  background: var(--el-bg-color, #ffffff);
  color: var(--el-text-color-regular, #334155);
  display: inline-flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  transition: border-color 0.18s ease, color 0.18s ease, background-color 0.18s ease;
}

.messenger-agent-avatar-trigger:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.messenger-agent-avatar-trigger:not(:disabled):hover {
  border-color: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.36);
  color: var(--ui-accent-deep, #2563eb);
  background: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.06);
}

.messenger-agent-avatar-trigger:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb, 59, 130, 246), 0.22);
  outline-offset: 2px;
}

.messenger-agent-avatar-trigger-preview {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  color: #ffffff;
  font-size: 12px;
  font-weight: 700;
  line-height: 1;
  text-transform: uppercase;
}

.messenger-agent-avatar-trigger-text {
  font-size: 12px;
  font-weight: 600;
}

.messenger-agent-base {
  width: 100%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.messenger-agent-base-item {
  width: 100%;
  min-width: 0;
  display: grid;
  grid-template-columns: 148px minmax(0, 1fr);
  gap: 16px;
  align-items: center;
}

.messenger-agent-base-label {
  color: var(--el-text-color-regular, #111827);
  font-size: 13px;
  font-weight: 600;
  line-height: 1.5;
}

.messenger-agent-base-control {
  width: 100%;
  min-width: 0;
}

.messenger-agent-base-select {
  width: 100%;
}

.messenger-agent-form-item--base :deep(.el-form-item__content) {
  display: block;
  width: 100%;
  min-width: 0;
}

.messenger-agent-base-control :deep(.el-select),
.messenger-agent-base-control :deep(.el-select__wrapper),
.messenger-agent-base-control :deep(.beeroom-group-field) {
  width: 100%;
}

.messenger-agent-section-head {
  display: inline-flex;
  align-items: center;
  gap: 10px;
}

.messenger-agent-icon-btn {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid rgba(148, 163, 184, 0.24);
  border-radius: 8px;
  background: transparent;
  color: var(--el-text-color-secondary, #64748b);
  cursor: pointer;
  transition: border-color 0.18s ease, background-color 0.18s ease, color 0.18s ease;
}

.messenger-agent-icon-btn:hover {
  border-color: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.34);
  background: rgba(var(--ui-accent-rgb, 59, 130, 246), 0.08);
  color: var(--ui-accent-deep, #2563eb);
}

.messenger-agent-icon-btn:focus-visible {
  outline: 2px solid rgba(var(--ui-accent-rgb, 59, 130, 246), 0.22);
  outline-offset: 2px;
}

@media (max-width: 720px) {
  .messenger-agent-name-row {
    grid-template-columns: minmax(0, 1fr);
  }

  .messenger-agent-avatar-trigger {
    width: fit-content;
  }

  .messenger-agent-base-item {
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    align-items: stretch;
  }
}
</style>

