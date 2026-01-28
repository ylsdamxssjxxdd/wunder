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
            <div class="profile-quota-ring" :class="{ 'is-empty': !quotaAvailable }" :style="quotaRingStyle"></div>
            <div class="profile-chart-summary">
              {{ quotaUsedText }} / {{ quotaTotalText }}
            </div>
          </div>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, watch } from 'vue';
import { useRoute } from 'vue-router';

import UserTopbar from '@/components/user/UserTopbar.vue';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { isDemoMode } from '@/utils/demo';

const route = useRoute();
const authStore = useAuthStore();
const chatStore = useChatStore();

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

const accountQuotaSnapshot = computed(() => {
  const user = authStore.user || {};
  const daily = parseQuotaNumber(user.daily_quota ?? user.dailyQuota);
  const used = parseQuotaNumber(user.daily_quota_used ?? user.dailyQuotaUsed);
  const date = user.daily_quota_date ?? user.dailyQuotaDate ?? '';
  if (daily === null && used === null && !date) return null;
  const remaining =
    Number.isFinite(daily) && Number.isFinite(used) ? Math.max(daily - used, 0) : null;
  return {
    daily,
    used,
    remaining,
    date: date ? String(date) : ''
  };
});

const latestQuotaSnapshot = computed(() => {
  for (let i = assistantMessages.value.length - 1; i >= 0; i -= 1) {
    const snapshot = assistantMessages.value[i]?.stats?.quotaSnapshot;
    if (snapshot) return snapshot;
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

const quotaRingStyle = computed(() => ({
  '--quota-angle': `${(quotaPercent.value * 360).toFixed(1)}deg`
}));

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
  if (!authStore.user) {
    authStore.loadProfile();
  }
  loadSessions();
});

watch(
  () => route.path,
  () => {
    loadSessions();
  }
);
</script>
