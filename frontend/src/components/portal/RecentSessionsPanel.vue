<template>
  <div class="portal-side-card">
    <div class="portal-side-header">
      <div>
        <div class="portal-side-title">最近会话</div>
        <div class="portal-side-desc">快速返回近期对话</div>
      </div>
      <button class="portal-side-action" type="button" @click="loadSessions">
        刷新
      </button>
    </div>
    <div class="portal-side-scroll">
      <div v-if="loading" class="portal-side-empty">正在加载会话...</div>
      <div v-else-if="error" class="portal-side-empty">{{ error }}</div>
      <div v-else-if="recentSessions.length === 0" class="portal-side-empty">
        暂无会话记录
      </div>
      <button
        v-for="session in recentSessions"
        :key="session.id"
        class="portal-side-session"
        type="button"
        @click="openSession(session)"
      >
        <div class="portal-side-session-title">{{ formatTitle(session.title) }}</div>
        <div class="portal-side-session-meta">
          更新于 {{ formatTime(session.updated_at || session.created_at) }}
        </div>
      </button>
    </div>
  </div>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useChatStore } from '@/stores/chat';

const props = defineProps({
  maxCount: {
    type: Number,
    default: 5
  }
});

const route = useRoute();
const router = useRouter();
const chatStore = useChatStore();
const loading = ref(false);
const error = ref('');

const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const recentSessions = computed(() => chatStore.sessions.slice(0, props.maxCount));

const loadSessions = async () => {
  loading.value = true;
  error.value = '';
  try {
    await chatStore.loadSessions();
  } catch (err) {
    error.value = err?.response?.data?.detail || '会话加载失败';
  } finally {
    loading.value = false;
  }
};

const openSession = async (session) => {
  if (!session?.id) return;
  try {
    await chatStore.loadSessionDetail(session.id);
    router.push(`${basePath.value}/chat`);
  } catch (err) {
    ElMessage.error(err?.response?.data?.detail || '打开会话失败');
  }
};

const formatTitle = (title) => {
  const text = String(title || '').trim();
  if (!text) return '未命名会话';
  return text.length > 18 ? `${text.slice(0, 18)}...` : text;
};

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

onMounted(loadSessions);
</script>
