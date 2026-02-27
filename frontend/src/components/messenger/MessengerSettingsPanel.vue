<template>
  <div class="messenger-settings-page">
    <template v-if="mode === 'profile'">
      <section class="messenger-settings-card">
        <div class="messenger-settings-profile-head">
          <div class="messenger-settings-profile-avatar">{{ profileInitial }}</div>
          <div class="messenger-settings-profile-meta">
            <div class="messenger-settings-profile-name">{{ username || t('user.guest') }}</div>
            <div class="messenger-settings-profile-id">{{ t('profile.idLabel', { id: userId || '-' }) }}</div>
            <div class="messenger-settings-profile-tags">
              <span class="messenger-settings-profile-tag">{{ t('user.unitLabel', { unit: userUnitLabel }) }}</span>
              <span class="messenger-settings-profile-tag">{{ accountTypeLabel }}</span>
            </div>
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

    <template v-else-if="mode === 'desktop'">
      <section class="messenger-settings-card">
        <div class="messenger-settings-title">{{ t('desktop.settings.title') }}</div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('desktop.settings.toolCallMode') }}</div>
            <div class="messenger-settings-hint">{{ t('desktop.settings.toolCallHint') }}</div>
          </div>
          <select
            :value="desktopToolCallMode"
            class="messenger-settings-select"
            @change="handleDesktopToolCallModeChange"
          >
            <option value="tool_call">tool_call</option>
            <option value="function_call">function_call</option>
          </select>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('desktop.settings.tools') }}</div>
            <div class="messenger-settings-hint">{{ t('desktop.settings.toolsHint') }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('open-tools')">
            {{ t('desktop.settings.openTools') }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('desktop.settings.system') }}</div>
            <div class="messenger-settings-hint">{{ t('desktop.settings.systemHint') }}</div>
          </div>
          <button class="messenger-settings-action" type="button" @click="$emit('open-system')">
            {{ t('desktop.settings.openSystem') }}
          </button>
        </div>
        <div class="messenger-settings-row">
          <div>
            <div class="messenger-settings-label">{{ t('desktop.settings.devtools') }}</div>
            <div class="messenger-settings-hint">{{ t('desktop.settings.devtoolsHint') }}</div>
          </div>
          <button
            class="messenger-settings-action"
            type="button"
            :disabled="!devtoolsAvailable"
            @click="$emit('toggle-devtools')"
          >
            {{ t('desktop.settings.openDevtools') }}
          </button>
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
          <div class="messenger-settings-label">{{ t('messenger.settings.sendKey') }}</div>
          <select v-model="sendKey" class="messenger-settings-select">
            <option value="enter">Enter</option>
            <option value="ctrl_enter">Ctrl + Enter</option>
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
          </select>
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
        <div class="messenger-settings-row">
          <div class="messenger-settings-label">{{ t('nav.logout') }}</div>
          <button class="messenger-settings-action danger" type="button" @click="$emit('logout')">
            <i class="fa-solid fa-right-from-bracket" aria-hidden="true"></i>
            <span>{{ t('nav.logout') }}</span>
          </button>
        </div>
      </section>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';

type ThemePalette = 'hula-green' | 'eva-orange' | 'minimal';

const props = withDefaults(
  defineProps<{
    mode?: 'general' | 'profile' | 'desktop';
    username?: string;
    userId?: string;
    languageLabel?: string;
    themePalette?: ThemePalette;
    uiFontSize?: number;
    desktopToolCallMode?: 'tool_call' | 'function_call';
    devtoolsAvailable?: boolean;
  }>(),
  {
    mode: 'general',
    username: '',
    userId: '',
    languageLabel: '',
    themePalette: 'eva-orange',
    uiFontSize: 14,
    desktopToolCallMode: 'tool_call',
    devtoolsAvailable: false
  }
);

const emit = defineEmits<{
  (event: 'toggle-language'): void;
  (event: 'check-update'): void;
  (event: 'open-tools'): void;
  (event: 'open-system'): void;
  (event: 'toggle-devtools'): void;
  (event: 'logout'): void;
  (event: 'update:desktop-tool-call-mode', value: 'tool_call' | 'function_call'): void;
  (event: 'update:theme-palette', value: ThemePalette): void;
  (event: 'update:ui-font-size', value: number): void;
}>();

const { t } = useI18n();
const authStore = useAuthStore();
const chatStore = useChatStore();
const sendKey = ref('enter');
const themePalette = ref<ThemePalette>('eva-orange');
const fontSize = ref(Math.min(20, Math.max(12, Number(props.uiFontSize) || 14)));

const normalizeThemePalette = (value: unknown): ThemePalette => {
  const text = String(value || '').trim().toLowerCase();
  if (text === 'hula-green') return 'hula-green';
  if (text === 'minimal') return 'minimal';
  return 'eva-orange';
};

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

const handleDesktopToolCallModeChange = (event: Event) => {
  const value = String((event.target as HTMLSelectElement)?.value || '').trim().toLowerCase();
  emit('update:desktop-tool-call-mode', value === 'function_call' ? 'function_call' : 'tool_call');
};
</script>
