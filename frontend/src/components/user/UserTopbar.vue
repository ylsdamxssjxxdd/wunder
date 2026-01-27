<template>
  <header class="user-topbar">
    <div class="user-topbar-brand">
      <div class="user-topbar-logo">W</div>
      <div class="user-topbar-title">
        <div class="user-topbar-title-text">{{ title }}</div>
        <div class="user-topbar-sub">
          <span>{{ subtitle }}</span>
          <span v-if="demoMode" class="user-topbar-badge">演示模式</span>
        </div>
      </div>
    </div>
    <nav class="user-topbar-nav">
      <router-link
        v-for="item in navItems"
        :key="item.path"
        :to="item.path"
        class="user-topbar-link"
      >
        {{ item.label }}
      </router-link>
    </nav>
    <div v-if="showSearch" class="portal-search user-topbar-search">
      <svg class="portal-search-icon" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="11" cy="11" r="7" />
        <path d="M16.5 16.5L21 21" />
      </svg>
      <input
        :value="search"
        type="text"
        :placeholder="searchPlaceholder"
        @input="updateSearch"
      />
      <button
        v-if="search"
        class="portal-search-clear"
        type="button"
        aria-label="清空搜索"
        @click="clearSearch"
      >
        ×
      </button>
    </div>
    <div class="user-topbar-actions">
      <slot name="actions" />
      <ThemeToggle />
      <router-link
        :to="profilePath"
        class="user-topbar-user"
        aria-label="进入我的概况"
      >
        <div class="user-topbar-user-meta">
          <div class="user-topbar-user-name">{{ userName }}</div>
          <div class="user-topbar-user-level">等级 {{ userLevel }}</div>
        </div>
      </router-link>
      <button class="user-topbar-logout" type="button" aria-label="退出登录" @click="handleLogout">
        退出
      </button>
    </div>
  </header>
</template>

<script setup>
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import ThemeToggle from '@/components/common/ThemeToggle.vue';
import { useAuthStore } from '@/stores/auth';
import { isDemoMode } from '@/utils/demo';

defineProps({
  title: {
    type: String,
    default: '功能广场'
  },
  subtitle: {
    type: String,
    default: '面向用户的智能体入口'
  },
  showSearch: {
    type: Boolean,
    default: false
  },
  search: {
    type: String,
    default: ''
  },
  searchPlaceholder: {
    type: String,
    default: '搜索功能、标签或描述'
  }
});

const emit = defineEmits(['update:search']);

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const profilePath = computed(() => `${basePath.value}/profile`);

const navItems = computed(() => [
  { label: '广场', path: `${basePath.value}/home` },
  { label: '智能体', path: `${basePath.value}/agents` },
  { label: '工具管理', path: `${basePath.value}/tools` },
  { label: '聊天', path: `${basePath.value}/chat` }
]);

const userName = computed(() => authStore.user?.username || '访客');
const userLevel = computed(() => authStore.user?.access_level || '-');

const updateSearch = (event) => {
  emit('update:search', event.target.value);
};

const clearSearch = () => {
  emit('update:search', '');
};

const handleLogout = () => {
  if (demoMode.value) {
    router.push('/login');
    return;
  }
  authStore.logout();
  router.push('/login');
};
</script>
