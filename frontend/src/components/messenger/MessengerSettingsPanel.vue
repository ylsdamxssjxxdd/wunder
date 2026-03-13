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
              <span class="messenger-settings-profile-tag">{{ accountTypeLabel }}</span>
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
            <div class="messenger-settings-hint">{{ t('profile.stats.contextTokens') }}</div>
            <div class="messenger-settings-label">{{ formatK(contextTokensLatest) }}</div>
          </div>
          <div class="messenger-profile-stat-item">
            <div class="messenger-settings-hint">{{ t('profile.stats.totalTokens') }}</div>
            <div class="messenger-settings-label">{{ formatK(tokenUsageTotal) }}</div>
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
            <span>{{ t('profile.metrics.quotaToday') }}</span>
            <span>{{ quotaRemainingText }} / {{ quotaTotalText }}</span>
          </div>
          <div class="messenger-profile-quota-bar">
            <span :style="{ width: `${quotaUsedPercent}%` }"></span>
          </div>
          <div class="messenger-profile-quota-meta">
            <span>{{ t('profile.quota.used') }}: {{ quotaUsedText }}</span>
            <span>{{ t('profile.quota.remaining') }}: {{ quotaRemainingText }}</span>
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
            <option value="ctrl_enter">Ctrl + Enter</option>
            <option value="enter">Enter</option>
            <option value="none">{{ t('messenger.settings.sendKeyNone') }}</option>
          </select>
        </div>
        <div v-if="desktopWindowCloseAvailable" class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('messenger.settings.windowCloseBehavior') }}</div>
            <div class="messenger-settings-hint">
              {{ t('messenger.settings.windowCloseBehaviorHint') }}
            </div>
          </div>
          <select
            v-model="windowCloseBehavior"
            class="messenger-settings-select"
            :disabled="windowCloseBehaviorLoading"
            @change="handleWindowCloseBehaviorChange"
          >
            <option value="tray">{{ t('messenger.settings.windowCloseBehaviorHide') }}</option>
            <option value="quit">{{ t('messenger.settings.windowCloseBehaviorQuit') }}</option>
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
    </template>

    <el-dialog
      v-model="usernameDialogVisible"
      class="messenger-dialog messenger-username-dialog"
      :title="t('profile.edit.username')"
      width="420px"
      :close-on-click-modal="false"
      append-to-body
      destroy-on-close
      @closed="closeUsernameDialog"
    >
      <div class="messenger-username-dialog-body">
        <input
          v-model.trim="usernameDraft"
          class="messenger-settings-profile-edit-input messenger-settings-profile-edit-input--dialog"
          type="text"
          :placeholder="t('profile.edit.usernamePlaceholder')"
          @keydown.enter.prevent="submitUsernameUpdate"
        />
      </div>
      <template #footer>
        <div class="messenger-username-dialog-footer">
          <button class="messenger-settings-action ghost" type="button" @click="closeUsernameDialog">
            {{ t('common.cancel') }}
          </button>
          <button
            class="messenger-settings-action"
            type="button"
            :disabled="!canSubmitUsername || usernameSaving"
            @click="submitUsernameUpdate"
          >
            {{ usernameSaving ? t('common.saving') : t('common.save') }}
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
import { computed, ref, watch } from 'vue';
import { APP_VERSION } from '@/config/appVersion';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import UserAvatarGlyph from '@/components/messenger/UserAvatarGlyph.vue';
import { normalizeThemePalette, type ThemePalette } from '@/utils/themeAppearance';

type SendKeyMode = 'enter' | 'ctrl_enter' | 'none';
type WindowCloseBehavior = 'tray' | 'quit';
type ProfileAvatarOption = {
  key: string;
  label: string;
  image?: string;
};
type AvatarColorOption = {
  value: string;
  label: string;
};
type DesktopWindowCloseBridge = {
  getWindowCloseBehavior?: () => Promise<string | null> | string | null;
  setWindowCloseBehavior?: (behavior: string) => Promise<string | null> | string | null;
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
    sendKey: 'ctrl_enter',
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
const authStore = useAuthStore();
const chatStore = useChatStore();
const appVersion = APP_VERSION;
const sendKey = ref<SendKeyMode>('ctrl_enter');
const themePalette = ref<ThemePalette>('eva-orange');
const usernameDraft = ref('');
const windowCloseBehavior = ref<WindowCloseBehavior>('tray');
const windowCloseBehaviorLoading = ref(false);
const fontSize = ref(Math.min(20, Math.max(12, Number(props.uiFontSize) || 14)));
const usernameDialogVisible = ref(false);
const avatarDialogVisible = ref(false);
const avatarDialogIcon = ref(DEFAULT_AVATAR_ICON);
const avatarDialogColor = ref(DEFAULT_AVATAR_COLOR);
const avatarPage = ref(1);

const normalizeSendKey = (value: unknown): SendKeyMode =>
  (() => {
    const text = String(value || '').trim().toLowerCase();
    if (text === 'enter') return 'enter';
    if (text === 'none' || text === 'off' || text === 'disabled') return 'none';
    return 'ctrl_enter';
  })();

const normalizeWindowCloseBehavior = (value: unknown): WindowCloseBehavior => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (text === 'quit') return 'quit';
  return 'tray';
};

const getDesktopWindowCloseBridge = (): DesktopWindowCloseBridge | null => {
  if (typeof window === 'undefined') return null;
  const candidate = (window as Window & { wunderDesktop?: DesktopWindowCloseBridge }).wunderDesktop;
  return candidate && typeof candidate === 'object' ? candidate : null;
};

const desktopWindowCloseAvailable = computed(() => {
  if (!props.desktopLocalMode) {
    return false;
  }
  const bridge = getDesktopWindowCloseBridge();
  return Boolean(
    bridge &&
      typeof bridge.getWindowCloseBehavior === 'function' &&
      typeof bridge.setWindowCloseBehavior === 'function'
  );
});
const allowUsernameEdit = computed(() => true);
const usernameSaving = computed(() => props.usernameSaving === true);
const canSubmitUsername = computed(() => {
  if (!allowUsernameEdit.value) return false;
  const target = String(usernameDraft.value || '').trim();
  const current = String(props.username || '').trim();
  return Boolean(target) && target !== current;
});

const loadWindowCloseBehavior = async () => {
  if (!desktopWindowCloseAvailable.value) {
    return;
  }
  const bridge = getDesktopWindowCloseBridge();
  if (!bridge || typeof bridge.getWindowCloseBehavior !== 'function') {
    return;
  }
  windowCloseBehaviorLoading.value = true;
  try {
    const rawBehavior = await bridge.getWindowCloseBehavior();
    const normalized = normalizeWindowCloseBehavior(rawBehavior);
    windowCloseBehavior.value = normalized;
    const source = String(rawBehavior || '')
      .trim()
      .toLowerCase();
    if ((source === 'ask' || source === 'hide') && typeof bridge.setWindowCloseBehavior === 'function') {
      await bridge.setWindowCloseBehavior(normalized);
    }
  } catch {
    windowCloseBehavior.value = 'tray';
  } finally {
    windowCloseBehaviorLoading.value = false;
  }
};

const handleWindowCloseBehaviorChange = async () => {
  if (!desktopWindowCloseAvailable.value || windowCloseBehaviorLoading.value) {
    return;
  }
  const bridge = getDesktopWindowCloseBridge();
  if (!bridge || typeof bridge.setWindowCloseBehavior !== 'function') {
    return;
  }
  const target = normalizeWindowCloseBehavior(windowCloseBehavior.value);
  windowCloseBehaviorLoading.value = true;
  try {
    const next = await bridge.setWindowCloseBehavior(target);
    windowCloseBehavior.value = normalizeWindowCloseBehavior(next || target);
  } catch {
    await loadWindowCloseBehavior();
  } finally {
    windowCloseBehaviorLoading.value = false;
  }
};

watch(
  () => props.desktopLocalMode,
  (enabled) => {
    if (!enabled) {
      return;
    }
    void loadWindowCloseBehavior();
  },
  { immediate: true }
);

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
  usernameDialogVisible.value = true;
};

const closeUsernameDialog = () => {
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

const submitUsernameUpdate = () => {
  if (!canSubmitUsername.value || usernameSaving.value) {
    return;
  }
  emit('update:username', String(usernameDraft.value || '').trim());
  usernameDialogVisible.value = false;
};

const userUnitLabel = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  const unit = (user.unit || {}) as Record<string, unknown>;
  return String(unit.path_name || unit.pathName || unit.name || user.unit_id || '-');
});

const accountTypeLabel = computed(() => {
  if (typeof window !== 'undefined' && window.location.pathname.startsWith('/demo')) {
    return t('profile.account.demo');
  }
  return t('profile.account.live');
});

const formatDateKey = (value: unknown): string => {
  const parsed = new Date(value as string | number);
  if (Number.isNaN(parsed.getTime())) return '';
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}`;
};

const sessionCount = computed(() => chatStore.sessions.length);

const recentSessionCount = computed(() => {
  const now = Date.now();
  const cutoff = now - 7 * 24 * 60 * 60 * 1000;
  return chatStore.sessions.filter((session) => {
    const value = session?.last_message_at || session?.updated_at || session?.created_at;
    if (!value) return false;
    const parsed = new Date(value as string | number);
    const time = parsed.getTime();
    return Number.isFinite(time) && time >= cutoff;
  }).length;
});

const assistantMessages = computed(() =>
  chatStore.messages.filter((message) => message && !message.isGreeting && message.role === 'assistant')
);

const toolCallCount = computed(() =>
  assistantMessages.value.reduce((sum, message) => sum + (message?.stats?.toolCalls || 0), 0)
);

const tokenUsageTotal = computed(() =>
  assistantMessages.value.reduce((sum, message) => {
    const total = message?.stats?.usage?.total ?? 0;
    return sum + (Number.isFinite(total) ? total : 0);
  }, 0)
);

const contextTokensLatest = computed(() => {
  for (let i = assistantMessages.value.length - 1; i >= 0; i -= 1) {
    const value = assistantMessages.value[i]?.stats?.contextTokens;
    if (Number.isFinite(value) && value > 0) return value;
  }
  return null;
});

const lastActiveTime = computed(() => {
  const latest = chatStore.sessions[0];
  if (!latest) return '-';
  return formatTime(latest.updated_at || latest.created_at);
});

const weeklySessionTrend = computed(() => {
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

const normalizeQuotaDate = (value: unknown): string => {
  const text = String(value || '').trim();
  if (!text) return '';
  const match = text.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (match) return `${match[1]}-${match[2]}-${match[3]}`;
  const parsed = new Date(text);
  if (Number.isNaN(parsed.getTime())) return '';
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}`;
};

const resolveTodayString = (): string => {
  const now = new Date();
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
};

const quotaSnapshot = computed(() => {
  const user = (authStore.user || {}) as Record<string, unknown>;
  const daily = parseQuotaNumber(user.daily_quota ?? user.dailyQuota);
  const rawUsed = parseQuotaNumber(user.daily_quota_used ?? user.dailyQuotaUsed);
  const date = normalizeQuotaDate(user.daily_quota_date ?? user.dailyQuotaDate ?? '');
  const today = resolveTodayString();
  const used = date && date === today ? rawUsed : 0;
  if (daily === null && used === null) return null;
  const remaining =
    Number.isFinite(daily) && Number.isFinite(used) ? Math.max((daily as number) - (used as number), 0) : null;
  return { daily, used, remaining };
});

const quotaTotal = computed(() => quotaSnapshot.value?.daily ?? null);
const quotaUsed = computed(() => quotaSnapshot.value?.used ?? 0);
const quotaRemaining = computed(() => quotaSnapshot.value?.remaining ?? null);
const quotaUsedPercent = computed(() => {
  if (!Number.isFinite(quotaTotal.value) || (quotaTotal.value as number) <= 0) return 0;
  const total = quotaTotal.value as number;
  const used = Number.isFinite(quotaUsed.value) ? (quotaUsed.value as number) : 0;
  return Math.max(0, Math.min(100, Math.round((used / total) * 100)));
});

const formatNumber = (value: number | null): string => {
  if (!Number.isFinite(value as number)) return '-';
  return new Intl.NumberFormat().format(value as number);
};

const quotaRemainingText = computed(() => formatNumber(quotaRemaining.value));
const quotaUsedText = computed(() => formatNumber(quotaUsed.value));
const quotaTotalText = computed(() => formatNumber(quotaTotal.value));

const formatTime = (value: unknown): string => {
  if (!value) return '-';
  const parsed = new Date(value as string | number);
  if (Number.isNaN(parsed.getTime())) return String(value);
  const pad = (part: number) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}`;
};

const formatK = (value: number | null): string => {
  if (!Number.isFinite(value as number) || (value as number) <= 0) return '-';
  return `${((value as number) / 1000).toFixed(1)}k`;
};

</script>
