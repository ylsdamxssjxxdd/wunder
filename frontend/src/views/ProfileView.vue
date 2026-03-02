<template>
  <div class="portal-shell profile-shell">
    <UserTopbar :title="t('profile.topbar.title')" :subtitle="t('profile.topbar.subtitle')" :hide-chat="true" />
    <main class="profile-content">
      <section class="profile-hero">
        <div class="profile-card profile-identity">
          <div class="profile-identity-header">
            <div class="profile-identity-main">
              <div class="profile-avatar">{{ userInitials }}</div>
              <div class="profile-info">
                <div class="profile-name">{{ userName }}</div>
                <div class="profile-id">{{ t('profile.idLabel', { id: userId }) }}</div>
                <div class="profile-tags">
                  <span class="profile-tag">{{ t('user.unitLabel', { unit: userUnitLabel }) }}</span>
                  <span class="profile-tag">
                    {{ demoMode ? t('profile.account.demo') : t('profile.account.live') }}
                  </span>
                </div>
              </div>
            </div>
            <button
              class="profile-edit-btn"
              type="button"
              :aria-label="t('profile.edit.action')"
              @click="openProfileEditor"
            >
              <i class="fa-solid fa-pen-to-square profile-edit-icon" aria-hidden="true"></i>
            </button>
          </div>
          <div class="profile-identity-stats">
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.sessions') }}</div>
              <div class="profile-stat-value">{{ sessionCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.sessions7d') }}</div>
              <div class="profile-stat-value">{{ recentSessionCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.lastActive') }}</div>
              <div class="profile-stat-value">{{ lastActiveTime }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.toolCalls') }}</div>
              <div class="profile-stat-value">{{ toolCallCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.contextTokens') }}</div>
              <div class="profile-stat-value">{{ formatK(contextTokensLatest) }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">{{ t('profile.stats.totalTokens') }}</div>
              <div class="profile-stat-value">{{ formatK(tokenUsageTotal) }}</div>
            </div>
          </div>
        </div>
      </section>

      <section class="profile-section profile-metrics-section">
        <div class="profile-card profile-section-card">
          <div class="profile-section-header">
            <div>
              <div class="profile-section-title">{{ t('profile.metrics.title') }}</div>
              <div class="profile-section-desc">{{ t('profile.metrics.desc') }}</div>
            </div>
          </div>
          <div class="profile-charts">
            <div class="profile-chart-quota">
              <div class="profile-chart-label">{{ t('profile.metrics.quotaToday') }}</div>
              <div ref="quotaChartRef" class="profile-quota-chart"></div>
              <div class="profile-chart-summary">
                {{ quotaRemainingText }} / {{ quotaTotalText }}
              </div>
            </div>
            <div class="profile-metric-breakdown">
              <div class="profile-metric-item">
                <div class="profile-stat-label">{{ quotaLabels.used }}</div>
                <div class="profile-stat-value">{{ quotaUsedText }}</div>
              </div>
              <div class="profile-metric-item">
                <div class="profile-stat-label">{{ quotaLabels.remaining }}</div>
                <div class="profile-stat-value">{{ quotaRemainingText }}</div>
              </div>
              <div class="profile-metric-item">
                <div class="profile-stat-label">{{ t('profile.metrics.quotaToday') }}</div>
                <div class="profile-stat-value">{{ quotaTotalText }}</div>
              </div>
            </div>
          </div>
        </div>
      </section>
    </main>

    <el-dialog
      v-model="editDialogVisible"
      class="profile-edit-dialog"
      width="460px"
      top="8vh"
      :show-close="false"
      append-to-body
    >
      <template #header>
        <div class="profile-edit-header">
          <div class="profile-edit-title">{{ t('profile.edit.title') }}</div>
          <button class="icon-btn" type="button" @click="editDialogVisible = false">&times;</button>
        </div>
      </template>
      <el-form :model="editForm" label-position="top" class="profile-edit-form">
        <el-form-item :label="t('profile.edit.username')">
          <el-input v-model="editForm.username" :placeholder="t('profile.edit.usernamePlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('profile.edit.email')">
          <el-input v-model="editForm.email" :placeholder="t('profile.edit.emailPlaceholder')" />
        </el-form-item>
        <el-form-item :label="t('profile.edit.unit')">
          <el-select
            v-model="editForm.unit_id"
            :placeholder="t('profile.edit.unitPlaceholder')"
            filterable
            :allow-create="desktopLocalMode"
            :default-first-option="desktopLocalMode"
            clearable
            :loading="unitLoading"
            :disabled="unitLoading || (!desktopLocalMode && unitOptions.length === 0)"
            style="width: 100%"
          >
            <el-option
              v-for="unit in unitOptions"
              :key="unit.value"
              :label="unit.label"
              :value="unit.value"
            />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="editDialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="editSaving" @click="saveProfile">
          {{ t('common.save') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import * as echarts from 'echarts';
import { ElMessage } from 'element-plus';

import UserTopbar from '@/components/user/UserTopbar.vue';
import { fetchOrgUnits, updateProfile } from '@/api/auth';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { useThemeStore } from '@/stores/theme';
import { isDemoMode } from '@/utils/demo';
import { showApiError } from '@/utils/apiError';

const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();
const themeStore = useThemeStore();
const { t, language } = useI18n();

const quotaChartRef = ref(null);
let quotaChart = null;
let stopResizeListener = null;
const editDialogVisible = ref(false);
const editSaving = ref(false);
const editForm = reactive({
  username: '',
  email: '',
  unit_id: ''
});
const unitOptions = ref([]);
const unitLoading = ref(false);

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const desktopLocalMode = computed(() => isDesktopModeEnabled() && !isDesktopRemoteAuthMode());
const userName = computed(() => authStore.user?.username || t('user.guest'));
const userId = computed(() => authStore.user?.id || '-');
const userUnitLabel = computed(() => {
  const unit = authStore.user?.unit;
  return unit?.path_name || unit?.pathName || unit?.name || authStore.user?.unit_id || '-';
});
const userInitials = computed(() => {
  const text = String(userName.value || '').trim();
  if (!text) return 'U';
  return text.slice(0, 2).toUpperCase();
});

const conversationMessages = computed(() =>
  chatStore.messages.filter((message) => message && !message.isGreeting)
);
const assistantMessages = computed(() =>
  conversationMessages.value.filter((message) => message.role === 'assistant')
);
const sessionCount = computed(() => chatStore.sessions.length);
const toolCallCount = computed(() =>
  assistantMessages.value.reduce((sum, message) => sum + (message?.stats?.toolCalls || 0), 0)
);
const tokenUsageTotal = computed(() =>
  assistantMessages.value.reduce((sum, message) => {
    const total = message?.stats?.usage?.total ?? 0;
    return sum + (Number.isFinite(total) ? total : 0);
  }, 0)
);

const openProfileEditor = () => {
  editForm.username = authStore.user?.username || '';
  editForm.email = authStore.user?.email || '';
  editForm.unit_id = authStore.user?.unit_id || '';
  editDialogVisible.value = true;
};

const buildUnitOptions = (items) =>
  (items || [])
    .map((unit) => ({
      value: unit.unit_id || unit.id || '',
      label: unit.path_name || unit.pathName || unit.name || unit.unit_id || '-'
    }))
    .filter((unit) => unit.value)
    .sort((left, right) => left.label.localeCompare(right.label, 'zh-CN'));

const loadUnits = async () => {
  unitLoading.value = true;
  try {
    const { data } = await fetchOrgUnits();
    const items = data?.data?.items || [];
    unitOptions.value = buildUnitOptions(items);
  } catch (error) {
    unitOptions.value = [];
  } finally {
    unitLoading.value = false;
  }
};

const saveProfile = async () => {
  const username = String(editForm.username || '').trim();
  const email = String(editForm.email || '').trim();
  if (!username) {
    ElMessage.warning(t('profile.edit.usernameRequired'));
    return;
  }
  editSaving.value = true;
  try {
    const payload = {
      username,
      email: email || '',
      unit_id: String(editForm.unit_id || '').trim()
    };
    const { data } = await updateProfile(payload);
    const profile = data?.data;
    if (profile) {
      authStore.user = profile;
    }
    editDialogVisible.value = false;
    ElMessage.success(t('profile.edit.saved'));
  } catch (error) {
    showApiError(error, t('profile.edit.saveFailed'));
  } finally {
    editSaving.value = false;
  }
};
const parseQuotaNumber = (value) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeQuotaDate = (value) => {
  const text = String(value || '').trim();
  if (!text) return '';
  const match = text.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (match) {
    return `${match[1]}-${match[2]}-${match[3]}`;
  }
  const parsed = new Date(text);
  if (Number.isNaN(parsed.getTime())) return '';
  const pad = (part) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())}`;
};

const resolveTodayString = () => {
  const now = new Date();
  const pad = (part) => String(part).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
};

const accountQuotaSnapshot = computed(() => {
  const user = authStore.user || {};
  const daily = parseQuotaNumber(user.daily_quota ?? user.dailyQuota);
  const rawUsed = parseQuotaNumber(user.daily_quota_used ?? user.dailyQuotaUsed);
  const date = normalizeQuotaDate(user.daily_quota_date ?? user.dailyQuotaDate ?? '');
  const today = resolveTodayString();
  const used = date && date === today ? rawUsed : 0;
  if (daily === null && used === null && !date) return null;
  const remaining =
    Number.isFinite(daily) && Number.isFinite(used) ? Math.max(daily - used, 0) : null;
  return {
    daily,
    used,
    remaining,
    date: date || today
  };
});

const latestQuotaSnapshot = computed(() => {
  const accountSnapshot = accountQuotaSnapshot.value;
  if (accountSnapshot) return accountSnapshot;
  const today = resolveTodayString();
  for (let i = assistantMessages.value.length - 1; i >= 0; i -= 1) {
    const snapshot = assistantMessages.value[i]?.stats?.quotaSnapshot;
    const date = normalizeQuotaDate(snapshot?.date);
    if (snapshot && date && date === today) return snapshot;
  }
  return accountQuotaSnapshot.value;
});

const quotaTotal = computed(() => {
  const snapshot = latestQuotaSnapshot.value;
  if (!snapshot) return null;
  const daily = snapshot.daily;
  if (Number.isFinite(daily)) return daily;
  const used = snapshot.used;
  const remaining = snapshot.remaining;
  if (Number.isFinite(used) && Number.isFinite(remaining)) {
    return used + remaining;
  }
  return null;
});

const quotaRemaining = computed(() => {
  const snapshot = latestQuotaSnapshot.value;
  if (!snapshot) return null;
  const remaining = snapshot.remaining;
  if (Number.isFinite(remaining)) return remaining;
  const total = quotaTotal.value;
  const used = snapshot.used;
  if (Number.isFinite(total) && Number.isFinite(used)) {
    return Math.max(total - used, 0);
  }
  return null;
});

const quotaUsed = computed(() => {
  const snapshot = latestQuotaSnapshot.value;
  if (!snapshot) return null;
  const used = snapshot.used;
  if (Number.isFinite(used)) return used;
  const total = quotaTotal.value;
  const remaining = snapshot.remaining;
  if (Number.isFinite(total) && Number.isFinite(remaining)) {
    return Math.max(total - remaining, 0);
  }
  return null;
});

const quotaAvailable = computed(() => Number.isFinite(quotaTotal.value) && quotaTotal.value > 0);

const quotaPercent = computed(() => {
  if (!quotaAvailable.value) return 0;
  const used = quotaUsed.value ?? 0;
  return Math.min(Math.max(used / quotaTotal.value, 0), 1);
});

const quotaLabels = computed(() => ({
  used: t('profile.quota.used'),
  remaining: t('profile.quota.remaining')
}));

const resolveQuotaPalette = () => {
  const isLight = themeStore.mode === 'light';
  return {
    usedLight: isLight ? '#7dd3fc' : '#5eead4',
    used: '#38bdf8',
    remainingLight: isLight ? '#86efac' : '#4ade80',
    remaining: '#22c55e',
    empty: isLight ? '#f8fafc' : '#0f172a',
    border: isLight ? 'rgba(15, 23, 42, 0.25)' : 'rgba(15, 23, 42, 0.6)',
    shadow: isLight ? 'rgba(15, 23, 42, 0.2)' : 'rgba(0, 0, 0, 0.55)',
    tooltipBg: isLight ? 'rgba(255, 255, 255, 0.95)' : 'rgba(15, 23, 42, 0.95)',
    tooltipText: isLight ? '#0f172a' : '#e2e8f0',
    tooltipBorder: isLight ? 'rgba(59, 130, 246, 0.2)' : 'rgba(59, 130, 246, 0.35)'
  };
};

const buildQuotaChartData = () => {
  if (!quotaAvailable.value) {
    return {
      data: [
        {
          value: 1,
          name: '__empty__',
          itemStyle: {
            color: resolveQuotaPalette().empty,
            borderColor: resolveQuotaPalette().border,
            borderWidth: 2,
            borderRadius: 8
          }
        }
      ],
      isEmpty: true,
      visibleCount: 0
    };
  }
  const total = quotaTotal.value ?? 0;
  const remainingRaw = Number.isFinite(quotaRemaining.value)
    ? quotaRemaining.value
    : Math.max(total - (quotaUsed.value ?? 0), 0);
  const remaining = Math.max(Math.min(remainingRaw, total), 0);
  const used = Math.max(total - remaining, 0);
  const data = [
    { value: used, name: quotaLabels.value.used },
    { value: remaining, name: quotaLabels.value.remaining }
  ];
  const visibleCount = data.filter((item) => Number(item.value) > 0).length;
  return { data, isEmpty: visibleCount === 0, visibleCount };
};

const renderQuotaChart = () => {
  const container = quotaChartRef.value;
  if (!container) return;
  if (!quotaChart) {
    quotaChart = echarts.init(container);
  }
  const palette = resolveQuotaPalette();
  const { data, isEmpty, visibleCount } = buildQuotaChartData();
  const padAngle = isEmpty || visibleCount <= 1 ? 0 : 1;
  const colorStops = [
    {
      type: 'linear',
      x: 0,
      y: 0,
      x2: 1,
      y2: 1,
      colorStops: [
        { offset: 0, color: palette.usedLight },
        { offset: 1, color: palette.used }
      ]
    },
    {
      type: 'linear',
      x: 0,
      y: 0,
      x2: 1,
      y2: 1,
      colorStops: [
        { offset: 0, color: palette.remainingLight },
        { offset: 1, color: palette.remaining }
      ]
    }
  ];
  const ringStyle = isEmpty
    ? {
        borderColor: palette.border,
        borderWidth: 2,
        borderRadius: 10,
        shadowBlur: 0
      }
    : {
        borderColor: palette.border,
        borderWidth: 2,
        borderRadius: 8,
        shadowBlur: 18,
        shadowColor: palette.shadow,
        shadowOffsetY: 4
      };
  quotaChart.setOption(
    {
      tooltip: {
        trigger: 'item',
        show: !isEmpty,
        backgroundColor: palette.tooltipBg,
        borderColor: palette.tooltipBorder,
        textStyle: { color: palette.tooltipText },
        formatter: '{b}: {c}'
      },
      series: [
        {
          type: 'pie',
          radius: ['38%', '88%'],
          center: ['50%', '50%'],
          avoidLabelOverlap: true,
          label: { show: false },
          labelLine: { show: false },
          padAngle,
          itemStyle: ringStyle,
          data,
          color: colorStops,
          silent: isEmpty,
          emphasis: {
            scale: true,
            scaleSize: 6
          }
        }
      ]
    },
    true
  );
};

const handleQuotaResize = () => {
  if (quotaChart) {
    quotaChart.resize();
  }
};

const quotaRemainingText = computed(() =>
  Number.isFinite(quotaRemaining.value) ? formatNumber(quotaRemaining.value) : '-'
);

const quotaUsedText = computed(() =>
  Number.isFinite(quotaUsed.value) ? formatNumber(quotaUsed.value) : '-'
);

const quotaTotalText = computed(() =>
  Number.isFinite(quotaTotal.value) ? formatNumber(quotaTotal.value) : '-'
);

const contextTokensLatest = computed(() => {
  for (let i = assistantMessages.value.length - 1; i >= 0; i -= 1) {
    const value = assistantMessages.value[i]?.stats?.contextTokens;
    if (Number.isFinite(value) && value > 0) return value;
  }
  return null;
});

const recentSessionCount = computed(() => {
  const now = Date.now();
  const cutoff = now - 7 * 24 * 60 * 60 * 1000;
  return chatStore.sessions.filter((session) => {
    const value = session?.last_message_at || session?.updated_at || session?.created_at;
    if (!value) return false;
    const parsed = new Date(value);
    const time = parsed.getTime();
    return Number.isFinite(time) && time >= cutoff;
  }).length;
});

const lastActiveTime = computed(() => {
  const latest = chatStore.sessions[0];
  if (!latest) return '-';
  return formatTime(latest.updated_at || latest.created_at);
});

const formatTime = (value) => {
  if (!value) return '-';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return String(value);
  }
  const pad = (part) => String(part).padStart(2, '0');
  return `${parsed.getFullYear()}-${pad(parsed.getMonth() + 1)}-${pad(parsed.getDate())} ${pad(
    parsed.getHours()
  )}:${pad(parsed.getMinutes())}`;
};

const formatNumber = (value) => {
  if (!Number.isFinite(value)) return '-';
  return new Intl.NumberFormat(language.value || 'zh-CN').format(value);
};

const formatK = (value) => {
  if (!Number.isFinite(value) || value <= 0) return '-';
  return `${(value / 1000).toFixed(1)}k`;
};

const ensureStatsSession = async () => {
  if (conversationMessages.value.length > 0) return;
  const persisted = (chatStore.getPersistedState?.() || {}) as { activeSessionId?: string };
  const targetId =
    chatStore.activeSessionId ||
    persisted.activeSessionId ||
    chatStore.sessions[0]?.id;
  if (!targetId) return;
  try {
    await chatStore.loadSessionDetail(targetId);
  } catch (error) {
    // ignore to avoid blocking stats rendering
  }
};

const loadSessions = async () => {
  try {
    await chatStore.loadSessions();
    await ensureStatsSession();
  } catch (error) {
    // ignore load failures; stats will render as empty
  }
};

onMounted(() => {
  authStore.loadProfile();
  loadUnits();
  loadSessions();
  nextTick(() => {
    renderQuotaChart();
  });
  window.addEventListener('resize', handleQuotaResize);
  stopResizeListener = () => window.removeEventListener('resize', handleQuotaResize);
});

onBeforeUnmount(() => {
  if (stopResizeListener) {
    stopResizeListener();
    stopResizeListener = null;
  }
  if (quotaChart) {
    quotaChart.dispose();
    quotaChart = null;
  }
});

watch(
  () => route.path,
  () => {
    loadSessions();
  }
);

watch(
  [quotaTotal, quotaUsed, () => themeStore.mode],
  () => {
    nextTick(() => {
      renderQuotaChart();
    });
  }
);
</script>
