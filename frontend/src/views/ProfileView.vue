<template>
  <div class="portal-shell profile-shell">
    <UserTopbar title="我的" subtitle="账号与使用概况" :hide-chat="true" />
    <main class="profile-content">
      <section class="profile-hero">
        <div class="profile-card profile-identity">
          <div class="profile-identity-main">
            <div class="profile-avatar">{{ userInitials }}</div>
            <div class="profile-info">
              <div class="profile-name">{{ userName }}</div>
              <div class="profile-id">ID：{{ userId }}</div>
              <div class="profile-tags">
                <span class="profile-tag">等级 {{ userLevel }}</span>
                <span class="profile-tag">{{ demoMode ? '演示模式' : '正式账号' }}</span>
              </div>
            </div>
          </div>
          <div class="profile-identity-stats">
            <div class="profile-stat">
              <div class="profile-stat-label">会话数量</div>
              <div class="profile-stat-value">{{ sessionCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">近 7 天会话</div>
              <div class="profile-stat-value">{{ recentSessionCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">当前会话消息</div>
              <div class="profile-stat-value">{{ conversationMessageCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">最近活跃</div>
              <div class="profile-stat-value">{{ lastActiveTime }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">用户消息</div>
              <div class="profile-stat-value">{{ userMessageCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">助手消息</div>
              <div class="profile-stat-value">{{ assistantMessageCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">工具调用</div>
              <div class="profile-stat-value">{{ toolCallCount }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">上下文占用</div>
              <div class="profile-stat-value">{{ formatK(contextTokensLatest) }}</div>
            </div>
            <div class="profile-stat">
              <div class="profile-stat-label">累计 Token</div>
              <div class="profile-stat-value">{{ formatK(tokenUsageTotal) }}</div>
            </div>
          </div>
        </div>
      </section>

      <section class="profile-section profile-metrics-section">
        <div class="profile-section-header">
          <div>
            <div class="profile-section-title">对话统计</div>
            <div class="profile-section-desc">基于已加载会话与消息的图表统计</div>
          </div>
        </div>
        <div class="profile-charts">
          <div class="profile-chart-quota">
            <div class="profile-chart-label">今日额度</div>
            <div ref="quotaChartRef" class="profile-quota-chart"></div>
            <div class="profile-chart-summary">
              {{ quotaRemainingText }} / {{ quotaTotalText }}
            </div>
          </div>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import * as echarts from 'echarts';

import UserTopbar from '@/components/user/UserTopbar.vue';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { useThemeStore } from '@/stores/theme';
import { isDemoMode } from '@/utils/demo';

const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();
const themeStore = useThemeStore();

const quotaChartRef = ref(null);
let quotaChart = null;
let stopResizeListener = null;

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const userName = computed(() => authStore.user?.username || '访客');
const userId = computed(() => authStore.user?.id || '-');
const userLevel = computed(() => authStore.user?.access_level || '-');
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
const conversationMessageCount = computed(() => conversationMessages.value.length);
const userMessageCount = computed(() =>
  conversationMessages.value.filter((message) => message.role === 'user').length
);
const assistantMessageCount = computed(() =>
  conversationMessages.value.filter((message) => message.role === 'assistant').length
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

const quotaLabels = {
  used: '已用',
  remaining: '剩余'
};

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
    { value: used, name: quotaLabels.used },
    { value: remaining, name: quotaLabels.remaining }
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
  return new Intl.NumberFormat('zh-CN').format(value);
};

const formatK = (value) => {
  if (!Number.isFinite(value) || value <= 0) return '-';
  return `${(value / 1000).toFixed(1)}k`;
};

const ensureStatsSession = async () => {
  if (conversationMessages.value.length > 0) return;
  const persisted = chatStore.getPersistedState?.() || {};
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
