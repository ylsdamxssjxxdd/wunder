<template>
  <div class="portal-shell profile-shell">
    <UserTopbar title="我的" subtitle="账号与使用概况" />
    <main class="profile-content">
      <section class="profile-hero">
        <div class="profile-card profile-identity">
          <div class="profile-avatar">{{ userInitials }}</div>
          <div class="profile-info">
            <div class="profile-name">{{ userName }}</div>
            <div class="profile-id">ID：{{ userId }}</div>
            <div class="profile-tags">
              <span class="profile-tag">等级 {{ userLevel }}</span>
              <span class="profile-tag">{{ demoMode ? '演示模式' : '正式账号' }}</span>
            </div>
          </div>
          <div class="profile-identity-actions">
            <router-link :to="`${basePath}/chat`" class="profile-action-btn primary">
              进入聊天
            </router-link>
          </div>
        </div>
        <div class="profile-card profile-stats">
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
        </div>
      </section>

      <section class="profile-section">
        <div class="profile-section-header">
          <div>
            <div class="profile-section-title">对话统计</div>
            <div class="profile-section-desc">基于已加载会话与消息的统计信息</div>
          </div>
        </div>
        <div class="profile-stat-grid">
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
            <div class="profile-stat-label">累计 Token</div>
            <div class="profile-stat-value">{{ formatNumber(tokenUsageTotal) }}</div>
          </div>
          <div class="profile-stat">
            <div class="profile-stat-label">平均响应</div>
            <div class="profile-stat-value">{{ formatDuration(averageDuration) }}</div>
          </div>
        </div>
      </section>

      <section class="profile-section">
        <div class="profile-section-header">
          <div>
            <div class="profile-section-title">最近会话</div>
            <div class="profile-section-desc">快速返回最近使用的对话</div>
          </div>
          <button class="profile-refresh-btn" type="button" @click="loadSessions">
            刷新
          </button>
        </div>
        <div class="profile-session-list">
          <div v-if="sessionsLoading" class="profile-empty">正在加载会话...</div>
          <div v-else-if="sessionError" class="profile-empty">{{ sessionError }}</div>
          <div v-else-if="recentSessions.length === 0" class="profile-empty">
            暂无会话记录，去创建新的聊天吧。
          </div>
          <button
            v-for="session in recentSessions"
            :key="session.id"
            class="profile-session-card"
            type="button"
            @click="openSession(session)"
          >
            <div class="profile-session-title">{{ formatTitle(session.title) }}</div>
            <div class="profile-session-meta">
              更新于 {{ formatTime(session.updated_at || session.created_at) }}
            </div>
          </button>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import UserTopbar from '@/components/user/UserTopbar.vue';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { isDemoMode } from '@/utils/demo';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const chatStore = useChatStore();

const sessionsLoading = ref(false);
const sessionError = ref('');

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));

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
const sessionCount = computed(() => chatStore.sessions.length);
const conversationMessageCount = computed(() => conversationMessages.value.length);
const userMessageCount = computed(() =>
  conversationMessages.value.filter((message) => message.role === 'user').length
);
const assistantMessageCount = computed(() =>
  conversationMessages.value.filter((message) => message.role === 'assistant').length
);
const toolCallCount = computed(() =>
  conversationMessages.value.reduce((sum, message) => sum + (message?.stats?.toolCalls || 0), 0)
);
const tokenUsageTotal = computed(() =>
  conversationMessages.value.reduce((sum, message) => {
    const total = message?.stats?.usage?.total ?? 0;
    return sum + (Number.isFinite(total) ? total : 0);
  }, 0)
);
const averageDuration = computed(() => {
  const durations = conversationMessages.value
    .filter((message) => message.role === 'assistant')
    .map((message) => message?.stats?.interaction_duration_s)
    .filter((value) => Number.isFinite(value) && value > 0);
  if (!durations.length) return null;
  const total = durations.reduce((sum, value) => sum + value, 0);
  return total / durations.length;
});
const recentSessions = computed(() => chatStore.sessions.slice(0, 6));

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
  if (recentSessions.value.length === 0) return '-';
  const latest = recentSessions.value[0];
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

const formatTitle = (title) => {
  const text = String(title || '').trim();
  if (!text) return '未命名会话';
  return text.length > 20 ? `${text.slice(0, 20)}...` : text;
};

const formatNumber = (value) => {
  if (!Number.isFinite(value)) return '-';
  return new Intl.NumberFormat('zh-CN').format(value);
};

const formatDuration = (value) => {
  if (!Number.isFinite(value) || value <= 0) return '-';
  if (value < 1) return `${Math.round(value * 1000)}ms`;
  return `${value.toFixed(1)}s`;
};

const loadSessions = async () => {
  sessionsLoading.value = true;
  sessionError.value = '';
  try {
    await chatStore.loadSessions();
  } catch (error) {
    sessionError.value = error?.response?.data?.detail || '会话加载失败';
  } finally {
    sessionsLoading.value = false;
  }
};

const openSession = async (session) => {
  if (!session?.id) return;
  try {
    await chatStore.loadSessionDetail(session.id);
    router.push(`${basePath.value}/chat`);
  } catch (error) {
    ElMessage.error(error?.response?.data?.detail || '打开会话失败');
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
