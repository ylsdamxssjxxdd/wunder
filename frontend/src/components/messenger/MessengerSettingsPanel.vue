<template>
  <div class="messenger-settings-page">
    <template v-if="mode === 'profile'">
      <section class="messenger-settings-card">
        <div class="messenger-settings-profile-head">
          <div class="messenger-settings-profile-avatar" :style="profileAvatarStyle">
            <img
              v-if="resolvedProfileAvatarImageUrl"
              class="messenger-settings-profile-avatar-image"
              :src="resolvedProfileAvatarImageUrl"
              decoding="async"
              alt=""
            />
            <UserAvatarGlyph
              v-else-if="resolvedProfileAvatarIcon !== 'initial'"
              class="messenger-settings-profile-avatar-icon"
              :glyph="resolvedProfileAvatarIcon"
              :size="22"
            />
            <span v-else>{{ profileInitial }}</span>
          </div>
          <div class="messenger-settings-profile-meta">
            <div class="messenger-settings-profile-name">{{ username || t('user.guest') }}</div>
            <div class="messenger-settings-profile-id">{{ t('profile.idLabel', { id: userId || '-' }) }}</div>
            <div class="messenger-settings-profile-tags">
              <span class="messenger-settings-profile-tag">{{ t('user.unitLabel', { unit: userUnitLabel }) }}</span>
              <span class="messenger-settings-profile-tag messenger-settings-profile-tag--level">
                {{ t('profile.level.label', { level: userLevel }) }}
              </span>
            </div>
          </div>
          <div class="messenger-settings-profile-head-controls">
            <button
              class="messenger-settings-action messenger-settings-action--avatar messenger-settings-action--icon"
              type="button"
              :title="t('profile.avatar.settings')"
              :aria-label="t('profile.avatar.settings')"
              @click="openAvatarDialog"
            >
              <i class="fa-solid fa-user" aria-hidden="true"></i>
            </button>
            <button
              class="messenger-settings-action messenger-settings-action--icon"
              type="button"
              :title="t('profile.edit.username')"
              :aria-label="t('profile.edit.username')"
              @click="openUsernameDialog"
            >
              <i class="fa-solid fa-pen" aria-hidden="true"></i>
            </button>
          </div>
        </div>
        <div class="messenger-profile-level-progress messenger-profile-level-progress--bottom">
          <div class="messenger-profile-level-progress-bar">
            <span :style="{ width: `${levelProgressPercent}%` }"></span>
          </div>
          <div class="messenger-profile-level-progress-meta">
            <span>{{ levelProgressText }}</span>
            <span>{{ levelProgressHint }}</span>
          </div>
        </div>
      </section>
      <section class="messenger-settings-card">
        <div class="messenger-settings-title">{{ t('profile.metrics.title') }}</div>
        <div class="messenger-settings-subtitle">{{ t('profile.metrics.desc') }}</div>
        <div class="messenger-profile-stats-grid">
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.sessions') }}</div>
            <div class="messenger-settings-label">{{ sessionCount }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.sessions7d') }}</div>
            <div class="messenger-settings-label">{{ recentSessionCount }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.toolCalls') }}</div>
            <div class="messenger-settings-label">{{ toolCallCount }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.agents') }}</div>
            <div class="messenger-settings-label">{{ agentCount }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.totalTokens') }}</div>
            <div class="messenger-settings-label">{{ tokenUsageTotalCompactText }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.lastActive') }}</div>
            <div class="messenger-settings-label">{{ lastActiveTime }}</div>
          </div>
        </div>
        <div class="messenger-profile-chart-card">
          <div class="messenger-profile-chart-head">
            <span>{{ t('profile.stats.sessions7d') }}</span>
            <span>{{ weeklySessionTotal }}</span>
          </div>
          <div class="messenger-profile-chart-bars">
            <div
              v-for="item in weeklySessionTrend"
              :key="item.key"
              class="messenger-profile-chart-bar-item"
            >
              <span class="messenger-profile-chart-value">{{ item.count }}</span>
              <span class="messenger-profile-chart-bar-wrap">
                <span class="messenger-profile-chart-bar" :style="{ height: `${item.height}%` }"></span>
              </span>
              <span class="messenger-profile-chart-day">{{ item.label }}</span>
            </div>
          </div>
        </div>
        <div class="messenger-profile-quota-card">
          <div class="messenger-profile-quota-head">
            <span>{{ t('profile.quota.remaining') }}</span>
          </div>
          <div class="messenger-profile-token-balance">
            <div class="messenger-profile-token-balance-main">
              <span class="messenger-profile-token-balance-icon" aria-hidden="true">
                <i class="fa-solid fa-coins"></i>
              </span>
              <div class="messenger-profile-token-balance-copy">
                <div class="messenger-profile-token-balance-value">{{ quotaRemainingCompactText }}</div>
              </div>
            </div>
          </div>
          <div class="messenger-profile-quota-meta">
            <span>{{ t('profile.quota.dailyGrant') }}: {{ dailyTokenGrantCompactText }}</span>
          </div>
        </div>
      </section>
    </template>

    <template v-else>
      <section class="messenger-settings-card">
        <div class="messenger-settings-head">
          <div>
            <div class="messenger-settings-title">{{ t('messenger.settings.versionTitle') }}</div>
            <div class="messenger-settings-subtitle">{{ t('messenger.settings.versionHint') }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('check-update')">
            <i class="fa-solid fa-rotate" aria-hidden="true"></i>
            <span>{{ t('messenger.settings.checkUpdate') }}</span>
          </button>
        </div>
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('messenger.settings.versionNumber') }}</div>
          <div class="messenger-settings-label">{{ appVersion }}</div>
        </div>
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('messenger.settings.sendKey') }}</div>
          <select v-model="sendKey" class="messenger-settings-select">
            <option value="enter">Enter</option>
            <option value="ctrl_enter">Ctrl + Enter</option>
            <option value="none">{{ t('messenger.settings.sendKeyNone') }}</option>
          </select>
        </div>
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('messenger.settings.language') }}</div>
          <button class="messenger-settings-select-like" type="button" @click="$emit('toggle-language')">
            {{ languageLabel }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.theme') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.themeHint') }}</div>
          </div>
          <select v-model="themePalette" class="messenger-settings-select">
            <option value="hula-green">{{ t('messenger.settings.themeOptionHula') }}</option>
            <option value="eva-orange">{{ t('messenger.settings.themeOptionEva') }}</option>
            <option value="minimal">{{ t('messenger.settings.themeOptionMinimal') }}</option>
            <option value="tech-blue">{{ t('messenger.settings.themeOptionTechBlue') }}</option>
          </select>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.debugTools') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.debugHint') }}</div>
          </div>
          <button
            class="messenger-settings-action"
            type="button"
            :disabled="!devtoolsAvailable"
            @click="$emit('toggle-devtools')"
          >
            {{ t('messenger.settings.openDebug') }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.fontSize') }}</div>
            <div class="messenger-settings-hint">{{ t('messenger.settings.fontHint') }}</div>
          </div>
          <div class="messenger-settings-range-wrap">
            <div class="messenger-settings-stepper">
              <button type="button" @click="fontSize = Math.max(12, fontSize - 1)">-</button>
              <span>{{ fontSize }}</span>
              <button type="button" @click="fontSize = Math.min(20, fontSize + 1)">+</button>
            </div>
            <input v-model.number="fontSize" class="messenger-settings-range" type="range" min="12" max="20" />
          </div>
        </div>
      </section>
      <DesktopRuntimeSettingsPanel :desktop-local-mode="desktopLocalMode" />
    </template>

    <el-dialog
      v-model="usernameDialogVisible"
      class="messenger-dialog messenger-username-dialog"
      :title="t('profile.edit.title')"
      width="520px"
      :close-on-click-modal="false"
      append-to-body
      destroy-on-close
      @closed="closeUsernameDialog"
    >
      <div class="messenger-username-dialog-body">
        <label class="messenger-username-dialog-field">
          <span>{{ t('profile.edit.username') }}</span>
          <input
            v-model.trim="usernameDraft"
            class="messenger-settings-profile-edit-input messenger-settings-profile-edit-input--dialog"
            type="text"
            :placeholder="t('profile.edit.usernamePlaceholder')"
            @keydown.enter.prevent="submitUsernameUpdate"
          />
        </label>
        <div class="messenger-username-dialog-section">
          <div class="messenger-username-dialog-section-title">{{ t('profile.edit.newPassword') }}</div>
          <div class="messenger-username-dialog-section-hint">{{ t('profile.edit.passwordHint') }}</div>
        </div>
        <label class="messenger-username-dialog-field">
          <span>{{ t('profile.edit.currentPassword') }}</span>
          <input
            v-model="currentPasswordDraft"
            class="messenger-settings-profile-edit-input messenger-settings-profile-edit-input--dialog"
            type="password"
            :placeholder="t('profile.edit.currentPasswordPlaceholder')"
          />
        </label>
        <label class="messenger-username-dialog-field">
          <span>{{ t('profile.edit.newPassword') }}</span>
          <input
            v-model="newPasswordDraft"
            class="messenger-settings-profile-edit-input messenger-settings-profile-edit-input--dialog"
            type="password"
            :placeholder="t('profile.edit.newPasswordPlaceholder')"
          />
        </label>
        <label class="messenger-username-dialog-field">
          <span>{{ t('profile.edit.confirmPassword') }}</span>
          <input
            v-model="confirmPasswordDraft"
            class="messenger-settings-profile-edit-input messenger-settings-profile-edit-input--dialog"
            type="password"
            :placeholder="t('profile.edit.confirmPasswordPlaceholder')"
            @keydown.enter.prevent="submitUsernameUpdate"
          />
        </label>
      </div>
      <template #footer>
        <div class="messenger-username-dialog-footer">
          <button class="messenger-settings-action ghost" type="button" @click="closeUsernameDialog">
            {{ t('common.cancel') }}
          </button>
          <button
            class="messenger-settings-action"
            type="button"
            :disabled="!canSubmitUsername || profileSavePending"
            @click="submitUsernameUpdate"
          >
            {{ profileSavePending ? t('common.saving') : t('common.save') }}
          </button>
        </div>
      </template>
    </el-dialog>

    <el-dialog
      v-model="avatarDialogVisible"
      class="messenger-dialog messenger-avatar-dialog"
      :title="t('profile.avatar.settings')"
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
              decoding="async"
              alt=""
            />
            <UserAvatarGlyph
              v-else-if="avatarDialogIcon !== 'initial'"
              class="messenger-settings-profile-avatar-icon"
              :glyph="avatarDialogIcon"
              :size="20"
            />
            <span v-else>{{ avatarDialogInitial }}</span>
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
              loading="lazy"
              decoding="async"
              alt=""
            />
            <UserAvatarGlyph
              v-else-if="item.key !== 'initial'"
              class="messenger-settings-profile-avatar-icon"
              :glyph="item.key"
              :size="15"
            />
            <span v-else>{{ avatarDialogInitial }}</span>
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
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import { updateProfile } from '@/api/auth';
import { APP_VERSION } from '@/config/appVersion';
import { useI18n } from '@/i18n';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { showApiError } from '@/utils/apiError';
import { sumConversationConsumedTokens } from '@/utils/messageStats';
import DesktopRuntimeSettingsPanel from '@/components/messenger/DesktopRuntimeSettingsPanel.vue';
import UserAvatarGlyph from '@/components/messenger/UserAvatarGlyph.vue';
import { normalizeThemePalette, type ThemePalette } from '@/utils/themeAppearance';

type SendKeyMode = 'enter' | 'ctrl_enter' | 'none';
type ProfileAvatarOption = {
  key: string;
  label: string;
  image?: string;
};
type AvatarColorOption = {
  value: string;
  label: string;
};

const DEFAULT_AVATAR_ICON = 'initial';
const DEFAULT_AVATAR_COLOR = '#3b82f6';
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

const props = withDefaults(
  defineProps<{
    mode?: 'general' | 'profile';
    username?: string;
    userId?: string;
    languageLabel?: string;
    sendKey?: SendKeyMode;
    themePalette?: ThemePalette;
    usernameSaving?: boolean;
    desktopLocalMode?: boolean;
    uiFontSize?: number;
    devtoolsAvailable?: boolean;
    updateAvailable?: boolean;
    profileAvatarIcon?: string;
    profileAvatarColor?: string;
    profileAvatarOptions?: ProfileAvatarOption[];
    profileAvatarColors?: string[];
  }>(),
  {
    mode: 'general',
    username: '',
    userId: '',
    languageLabel: '',
    sendKey: 'enter',
    themePalette: 'eva-orange',
    usernameSaving: false,
    desktopLocalMode: false,
    uiFontSize: 14,
    devtoolsAvailable: false,
    updateAvailable: false,
    profileAvatarIcon: 'initial',
    profileAvatarColor: 'var(--ui-accent)',
    profileAvatarOptions: () => [],
    profileAvatarColors: () => []
  }
);

const emit = defineEmits<{
  (event: 'toggle-language'): void;
  (event: 'check-update'): void;
  (event: 'toggle-devtools'): void;
  (event: 'logout'): void;
  (event: 'update:send-key', value: SendKeyMode): void;
  (event: 'update:theme-palette', value: ThemePalette): void;
  (event: 'update:ui-font-size', value: number): void;
  (event: 'update:username', value: string): void;
  (event: 'update:profile-avatar-icon', value: string): void;
  (event: 'update:profile-avatar-color', value: string): void;
}>();

const { t } = useI18n();
const agentStore = useAgentStore();
const authStore = useAuthStore();
const chatStore = useChatStore();
const appVersion = APP_VERSION;
const sendKey = ref<SendKeyMode>('enter');
const themePalette = ref<ThemePalette>('eva-orange');
const usernameDraft = ref('');
const currentPasswordDraft = ref('');
const newPasswordDraft = ref('');
const confirmPasswordDraft = ref('');
const fontSize = ref(Math.min(20, Math.max(12, Number(props.uiFontSize) || 14)));
const usernameDialogVisible = ref(false);
const avatarDialogVisible = ref(false);
const avatarDialogIcon = ref(DEFAULT_AVATAR_ICON);
const avatarDialogColor = ref(DEFAULT_AVATAR_COLOR);
const avatarPage = ref(1);
const profileSavePending = ref(false);

const normalizeSendKey = (value: unknown): SendKeyMode =>
  (() => {
    const text = String(value || '').trim().toLowerCase();
    if (text === 'enter') return 'enter';
    if (text === 'none' || text === 'off' || text === 'disabled') return 'none';
    return 'enter';
  })();
const allowUsernameEdit = computed(() => true);
const passwordChangeRequested = computed(() =>
  Boolean(
    String(currentPasswordDraft.value || '').trim() ||
      String(newPasswordDraft.value || '').trim() ||
      String(confirmPasswordDraft.value || '').trim()
  )
);
const canSubmitUsername = computed(() => {
  if (!allowUsernameEdit.value) return false;
  const target = String(usernameDraft.value || '').trim();
  const current = String(props.username || '').trim();
  return Boolean(target) && (target !== current || passwordChangeRequested.value);
});

watch(
  () => props.username,
  (value) => {
    usernameDraft.value = String(value || '').trim();
  },
  { immediate: true }
);

watch(
  () => props.sendKey,
  (value) => {
    const normalized = normalizeSendKey(value);
    if (sendKey.value !== normalized) {
      sendKey.value = normalized;
    }
  },
  { immediate: true }
);

watch(sendKey, (value) => {
  emit('update:send-key', normalizeSendKey(value));
});

watch(
  () => props.themePalette,
  (value) => {
    const normalized = normalizeThemePalette(value);
    if (themePalette.value !== normalized) {
      themePalette.value = normalized;
    }
  },
  { immediate: true }
);

watch(themePalette, (value) => {
  emit('update:theme-palette', normalizeThemePalette(value));
});

watch(
  () => props.uiFontSize,
  (value) => {
    const normalized = Math.min(20, Math.max(12, Number(value) || 14));
    if (fontSize.value !== normalized) {
      fontSize.value = normalized;
    }
  }
);

watch(fontSize, (value) => {
  emit('update:ui-font-size', Math.min(20, Math.max(12, Number(value) || 14)));
});

const profileInitial = computed(() => {
  const source = String(props.username || '').trim();
  if (!source) return '?';
  return source.slice(0, 1).toUpperCase();
});

const resolvedProfileAvatarIcon = computed(() => {
  const key = String(props.profileAvatarIcon || '').trim();
  return key || DEFAULT_AVATAR_ICON;
});

const resolvedProfileAvatarColor = computed(() => {
  const color = String(props.profileAvatarColor || '').trim();
  return color || DEFAULT_AVATAR_COLOR;
});

const resolveAvatarOptionImage = (key: unknown): string => {
  const normalized = String(key || '').trim();
  if (!normalized) return '';
  const matched = props.profileAvatarOptions.find((item) => item.key === normalized);
  return String(matched?.image || '').trim();
};

const resolvedProfileAvatarImageUrl = computed(() =>
  resolveAvatarOptionImage(resolvedProfileAvatarIcon.value)
);

const avatarDialogImageUrl = computed(() => resolveAvatarOptionImage(avatarDialogIcon.value));

const avatarPageCount = computed(() =>
  Math.max(1, Math.ceil(Math.max(0, props.profileAvatarOptions.length) / AVATAR_PAGE_SIZE))
);

const pagedAvatarOptions = computed(() => {
  const page = Math.min(Math.max(avatarPage.value, 1), avatarPageCount.value);
  const start = (page - 1) * AVATAR_PAGE_SIZE;
  return props.profileAvatarOptions.slice(start, start + AVATAR_PAGE_SIZE);
});

const profileAvatarStyle = computed(() => ({
  background: resolvedProfileAvatarImageUrl.value ? 'transparent' : resolvedProfileAvatarColor.value
}));

const avatarDialogPreviewStyle = computed(() => ({
  background: avatarDialogImageUrl.value
    ? 'transparent'
    : avatarDialogColor.value || DEFAULT_AVATAR_COLOR
}));

const avatarColorOptions = computed<AvatarColorOption[]>(() =>
  props.profileAvatarColors.map((color) => {
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

const avatarDialogInitial = computed(() => {
  if (avatarDialogIcon.value === DEFAULT_AVATAR_ICON) {
    return profileInitial.value;
  }
  return profileInitial.value;
});

const resolveAvatarPageByKey = (key: string): number => {
  const normalized = String(key || '').trim();
  if (!normalized) return 1;
  const index = props.profileAvatarOptions.findIndex((item) => item.key === normalized);
  if (index < 0) return 1;
  return Math.floor(index / AVATAR_PAGE_SIZE) + 1;
};

watch(
  () => props.profileAvatarOptions,
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

const openAvatarDialog = () => {
  avatarDialogIcon.value = resolvedProfileAvatarIcon.value || DEFAULT_AVATAR_ICON;
  avatarDialogColor.value = resolvedProfileAvatarColor.value || DEFAULT_AVATAR_COLOR;
  avatarPage.value = resolveAvatarPageByKey(avatarDialogIcon.value);
  avatarDialogVisible.value = true;
};

const openUsernameDialog = () => {
  usernameDraft.value = String(props.username || '').trim();
  currentPasswordDraft.value = '';
  newPasswordDraft.value = '';
  confirmPasswordDraft.value = '';
  usernameDialogVisible.value = true;
};

const closeUsernameDialog = () => {
  currentPasswordDraft.value = '';
  newPasswordDraft.value = '';
  confirmPasswordDraft.value = '';
  usernameDialogVisible.value = false;
};

const closeAvatarDialog = () => {
  avatarDialogVisible.value = false;
};

const resetAvatarDialog = () => {
  avatarDialogIcon.value = DEFAULT_AVATAR_ICON;
  avatarDialogColor.value = avatarColorOptions.value[0]?.value || DEFAULT_AVATAR_COLOR;
  avatarPage.value = resolveAvatarPageByKey(DEFAULT_AVATAR_ICON);
};

const applyAvatarDialog = () => {
  emit('update:profile-avatar-icon', avatarDialogIcon.value || DEFAULT_AVATAR_ICON);
  emit('update:profile-avatar-color', avatarDialogColor.value || DEFAULT_AVATAR_COLOR);
  avatarDialogVisible.value = false;
};

const submitUsernameUpdate = async () => {
  if (!canSubmitUsername.value || profileSavePending.value) {
    return;
  }
  const username = String(usernameDraft.value || '').trim();
  const current = String(props.username || '').trim();
  const currentPassword = String(currentPasswordDraft.value || '').trim();
  const newPassword = String(newPasswordDraft.value || '').trim();
  const confirmPassword = String(confirmPasswordDraft.value || '').trim();

  if (!username) {
    ElMessage.warning(t('profile.edit.usernameRequired'));
    return;
  }
  if (passwordChangeRequested.value) {
    if (!currentPassword) {
      ElMessage.warning(t('profile.edit.currentPasswordRequired'));
      return;
    }
    if (!newPassword) {
      ElMessage.warning(t('profile.edit.newPasswordRequired'));
      return;
    }
    if (!confirmPassword) {
      ElMessage.warning(t('profile.edit.confirmPasswordRequired'));
      return;
    }
    if (newPassword !== confirmPassword) {
      ElMessage.warning(t('profile.edit.passwordMismatch'));
      return;
    }
    if (currentPassword === newPassword) {
      ElMessage.warning(t('profile.edit.passwordSameAsCurrent'));
      return;
    }
  }

  profileSavePending.value = true;
  try {
    const payload: Record<string, string> = {
      username
    };
    if (passwordChangeRequested.value) {
      payload.current_password = currentPassword;
      payload.new_password = newPassword;
    }
    const { data } = await updateProfile(payload);
    const profile = data?.data;
    if (profile && typeof profile === 'object') {
      authStore.user = profile;
    } else {
      await authStore.loadProfile();
    }
    if (username !== current) {
      emit('update:username', username);
    }
    closeUsernameDialog();
    ElMessage.success(t('profile.edit.saved'));
  } catch (error) {
    showApiError(error, t('profile.edit.saveFailed'));
  } finally {
    profileSavePending.value = false;
  }
};

const userUnitLabel = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  const unit = (user.unit || {}) as Record<string, unknown>;
  return String(unit.path_name || unit.pathName || unit.name || user.unit_id || '-');
});

const formatDateKey = (value: unknown): string => {
  const parsed = new Date(value as string | number);
  if (Number.isNaN(parsed.getTime())) return '';
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}`;
};

const usageSummary = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  return (user.usage_summary || user.usageSummary || null) as Record<string, unknown> | null;
});
const sessionSummary = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  return (user.session_summary || user.sessionSummary || null) as Record<string, unknown> | null;
});
const agentCount = computed(() => {
  const owned = Array.isArray(agentStore.agents) ? agentStore.agents.length : 0;
  const shared = Array.isArray(agentStore.sharedAgents) ? agentStore.sharedAgents.length : 0;
  return 1 + owned + shared;
});

const fallbackRecentSessionCount = (): number => {
  const now = Date.now();
  const cutoff = now - 7 * 24 * 60 * 60 * 1000;
  return chatStore.sessions.filter((session) => {
    const value = session?.last_message_at || session?.updated_at || session?.created_at;
    if (!value) return false;
    const parsed = new Date(value as string | number);
    const time = parsed.getTime();
    return Number.isFinite(time) && time >= cutoff;
  }).length;
};

const sessionCount = computed(() => {
  const total = Number(sessionSummary.value?.total_sessions ?? sessionSummary.value?.totalSessions);
  if (Number.isFinite(total) && total >= 0) return total;
  return chatStore.sessions.length;
});

const recentSessionCount = computed(() => {
  const total = Number(
    sessionSummary.value?.sessions_last_7d ?? sessionSummary.value?.sessionsLast7d
  );
  if (Number.isFinite(total) && total >= 0) return total;
  return fallbackRecentSessionCount();
});

const assistantMessages = computed(() =>
  chatStore.messages.filter((message) => message && !message.isGreeting && message.role === 'assistant')
);

const toolCallCount = computed(() =>
  (() => {
    const total = Number(usageSummary.value?.tool_calls ?? usageSummary.value?.toolCalls);
    if (Number.isFinite(total) && total >= 0) return total;
    return assistantMessages.value.reduce((sum, message) => sum + (message?.stats?.toolCalls || 0), 0);
  })()
);

const tokenUsageTotal = computed(() =>
  (() => {
    const total = Number(usageSummary.value?.consumed_tokens ?? usageSummary.value?.consumedTokens);
    if (Number.isFinite(total) && total > 0) return total;
    return sumConversationConsumedTokens(chatStore.messages.filter((message) => message && !message.isGreeting));
  })()
);

const lastActiveTime = computed(() => {
  const summaryValue = sessionSummary.value?.last_active_at ?? sessionSummary.value?.lastActiveAt;
  if (summaryValue) {
    return formatTime(summaryValue);
  }
  const latest = chatStore.sessions[0];
  if (!latest) return '-';
  return formatTime(latest.updated_at || latest.created_at);
});

const weeklySessionTrend = computed(() => {
  const summaryTrend = sessionSummary.value?.trend_last_7d ?? sessionSummary.value?.trendLast7d;
  if (Array.isArray(summaryTrend) && summaryTrend.length) {
    const points = summaryTrend.map((item, index) => {
      const record = (item || {}) as Record<string, unknown>;
      const key = String(record.date || record.key || `day-${index}`);
      const count = Math.max(0, Number(record.count) || 0);
      const label = key.length >= 10 ? key.slice(5).replace('-', '/') : key;
      return { key, label, count, height: 8 };
    });
    const maxCount = Math.max(...points.map((item) => item.count), 1);
    return points.map((item) => ({
      ...item,
      height: item.count > 0 ? Math.max(18, Math.round((item.count / maxCount) * 100)) : 8
    }));
  }
  const days = 7;
  const today = new Date();
  const start = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const points: Array<{ key: string; label: string; count: number; height: number }> = [];
  const countMap = new Map<string, number>();
  for (let index = days - 1; index >= 0; index -= 1) {
    const target = new Date(start);
    target.setDate(start.getDate() - index);
    const key = formatDateKey(target);
    countMap.set(key, 0);
  }
  chatStore.sessions.forEach((session) => {
    const raw = session?.last_message_at || session?.updated_at || session?.created_at;
    const key = formatDateKey(raw);
    if (!key || !countMap.has(key)) return;
    countMap.set(key, (countMap.get(key) || 0) + 1);
  });
  const maxCount = Math.max(...Array.from(countMap.values()), 1);
  countMap.forEach((count, key) => {
    const label = key.slice(5).replace('-', '/');
    const ratio = count > 0 ? Math.max(18, Math.round((count / maxCount) * 100)) : 8;
    points.push({
      key,
      label,
      count,
      height: ratio
    });
  });
  return points;
});

const weeklySessionTotal = computed(() =>
  weeklySessionTrend.value.reduce((sum, item) => sum + item.count, 0)
);

const parseQuotaNumber = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const quotaSnapshot = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  const remaining = parseQuotaNumber(
    user.token_balance
      ?? user.tokenBalance
      ?? user.daily_quota_remaining
      ?? user.dailyQuotaRemaining
  );
  const dailyGrant = parseQuotaNumber(
    user.daily_token_grant
      ?? user.dailyTokenGrant
      ?? user.token_daily_grant
      ?? user.tokenDailyGrant
      ?? user.daily_quota
      ?? user.dailyQuota
  );
  if (remaining === null && dailyGrant === null) return null;
  return {
    remaining,
    dailyGrant
  };
});

const quotaRemaining = computed(() => quotaSnapshot.value?.remaining ?? null);
const dailyTokenGrant = computed(() => quotaSnapshot.value?.dailyGrant ?? null);

const levelSnapshot = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  const level = parseQuotaNumber(user.level ?? user.userLevel) ?? 1;
  const maxLevel = parseQuotaNumber(user.max_level ?? user.maxLevel) ?? 200;
  const experienceTotal = parseQuotaNumber(user.experience_total ?? user.experienceTotal) ?? 0;
  const experienceCurrent = parseQuotaNumber(user.experience_current ?? user.experienceCurrent) ?? 0;
  const experienceForNextLevel =
    parseQuotaNumber(user.experience_for_next_level ?? user.experienceForNextLevel) ?? 0;
  const experienceRemaining =
    parseQuotaNumber(user.experience_remaining ?? user.experienceRemaining) ?? 0;
  const experienceProgress = Number(user.experience_progress ?? user.experienceProgress ?? 0) || 0;
  const reachedMaxLevel = Boolean(user.reached_max_level ?? user.reachedMaxLevel);
  return {
    level: Math.max(1, Math.trunc(level)),
    maxLevel: Math.max(1, Math.trunc(maxLevel)),
    experienceTotal: Math.max(0, Math.trunc(experienceTotal)),
    experienceCurrent: Math.max(0, Math.trunc(experienceCurrent)),
    experienceForNextLevel: Math.max(0, Math.trunc(experienceForNextLevel)),
    experienceRemaining: Math.max(0, Math.trunc(experienceRemaining)),
    experienceProgress: Math.min(Math.max(experienceProgress, 0), 1),
    reachedMaxLevel
  };
});

const userLevel = computed(() => levelSnapshot.value.level);
const levelProgressPercent = computed(() =>
  Math.round((levelSnapshot.value.reachedMaxLevel ? 1 : levelSnapshot.value.experienceProgress) * 1000) / 10
);

const formatNumber = (value: number | null): string => {
  if (!Number.isFinite(value as number)) return '-';
  return new Intl.NumberFormat().format(value as number);
};

const levelProgressText = computed(() => {
  if (levelSnapshot.value.reachedMaxLevel) return t('profile.level.maxProgress');
  return t('profile.level.progress', {
    current: formatNumber(levelSnapshot.value.experienceCurrent),
    total: formatNumber(levelSnapshot.value.experienceForNextLevel)
  });
});
const levelProgressHint = computed(() => {
  if (levelSnapshot.value.reachedMaxLevel) {
    return t('profile.level.maxHint', { level: levelSnapshot.value.maxLevel });
  }
  return t('profile.level.nextHint', {
    exp: formatNumber(levelSnapshot.value.experienceRemaining)
  });
});

const quotaRemainingCompactText = computed(() => formatCompactTokenUnit(quotaRemaining.value));
const dailyTokenGrantCompactText = computed(() => formatCompactTokenUnit(dailyTokenGrant.value));
const tokenUsageTotalCompactText = computed(() =>
  formatCompactTokenUnit(tokenUsageTotal.value, { zeroAsDash: true })
);

const formatTime = (value: unknown): string => {
  if (!value) return '-';
  const parsed = new Date(value as string | number);
  if (Number.isNaN(parsed.getTime())) return String(value);
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}`;
};

const formatCompactTokenUnit = (
  value: number | null,
  options: { zeroAsDash?: boolean } = {}
): string => {
  const { zeroAsDash = false } = options;
  if (!Number.isFinite(value as number)) return '-';
  const safeValue = Math.max(0, Number(value) || 0);
  if (safeValue === 0 && zeroAsDash) return '-';
  if (safeValue >= 1_000_000) {
    return `${(safeValue / 1_000_000).toFixed(1)}M`;
  }
  if (safeValue >= 1_000) {
    return `${(safeValue / 1_000).toFixed(1)}k`;
  }
  return formatNumber(safeValue);
};

const loadProfileStats = () => {
  if (props.mode !== 'profile') return;
  authStore.loadProfile().catch(() => {});
  agentStore.loadAgents().catch(() => {});
};

onMounted(() => {
  loadProfileStats();
});

watch(
  () => props.mode,
  (mode, previous) => {
    if (mode === 'profile' && previous !== 'profile') {
      loadProfileStats();
    }
  }
);

</script>

<style scoped>
:deep(.messenger-username-dialog.el-dialog) {
  width: min(520px, calc(100vw - 24px)) !important;
}

:deep(.messenger-username-dialog .el-dialog__body) {
  padding: 18px 18px 16px;
}

:deep(.messenger-username-dialog .el-dialog__footer) {
  padding: 14px 18px 16px;
}

.messenger-settings-profile-tag--level {
  background: linear-gradient(135deg, rgba(249, 115, 22, 0.14), rgba(251, 191, 36, 0.2));
  border-color: rgba(249, 115, 22, 0.18);
  color: #c2410c;
  font-weight: 700;
}

.messenger-profile-level-progress {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.messenger-profile-level-progress--bottom {
  margin-top: 4px;
  padding-top: 6px;
}

.messenger-profile-level-progress-bar {
  width: 100%;
  height: 10px;
  border-radius: 999px;
  overflow: hidden;
  background: rgba(148, 163, 184, 0.18);
}

.messenger-profile-level-progress-bar > span {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: linear-gradient(90deg, #f59e0b, #f97316);
  transition: width 0.2s ease;
}

.messenger-profile-level-progress-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  color: #6f6f6f;
  font-size: 11px;
}

.messenger-profile-token-balance {
  padding: 16px 16px 14px;
  border-radius: 18px;
  border: 1px solid rgba(217, 119, 6, 0.16);
  background: #fff8ee;
}

.messenger-profile-token-balance-main {
  display: flex;
  align-items: center;
  gap: 14px;
}

.messenger-profile-token-balance-icon {
  flex: 0 0 42px;
  width: 42px;
  height: 42px;
  border-radius: 14px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: #f6e7b0;
  color: #b7791f;
  font-size: 18px;
}

.messenger-profile-token-balance-copy {
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 5px;
}

.messenger-profile-token-balance-value {
  font-size: 32px;
  font-weight: 800;
  line-height: 1;
  letter-spacing: -0.03em;
  color: #273444;
}

.messenger-username-dialog-body {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 14px;
}

.messenger-username-dialog-field {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
  color: #5f6368;
  font-size: 12px;
}

.messenger-username-dialog-field > span {
  font-weight: 600;
  line-height: 1.4;
}

.messenger-username-dialog-field .messenger-settings-profile-edit-input--dialog {
  width: 100%;
  min-height: 42px;
  padding: 0 14px;
  box-sizing: border-box;
}

.messenger-username-dialog-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 4px;
  padding-top: 12px;
  border-top: 1px solid rgba(148, 163, 184, 0.18);
}

.messenger-username-dialog-section-title {
  color: #1f2937;
  font-size: 14px;
  font-weight: 700;
}

.messenger-username-dialog-section-hint {
  color: #7b8794;
  font-size: 12px;
  line-height: 1.5;
}

.messenger-username-dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

@media (max-width: 720px) {
  .messenger-profile-level-progress-meta {
    flex-direction: column;
    align-items: flex-start;
  }

  .messenger-username-dialog-footer {
    width: 100%;
  }

  .messenger-username-dialog-footer .messenger-settings-action {
    flex: 1;
    justify-content: center;
  }
}
</style>
