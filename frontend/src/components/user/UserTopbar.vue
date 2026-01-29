<template>
  <header class="user-topbar topbar">
    <div class="brand">
      <div class="brand-mark">AI</div>
      <div class="brand-meta">
        <div class="brand-title-row">
          <div class="brand-title">{{ title }}</div>
        </div>
        <div class="brand-sub">
          <span>{{ subtitle }}</span>
          <span v-if="demoMode" class="demo-badge">演示模式</span>
        </div>
      </div>
    </div>
    <div class="topbar-actions">
      <nav class="topbar-nav">
        <router-link
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          class="topbar-panel-btn topbar-nav-link"
        >
          {{ item.label }}
        </router-link>
      </nav>
      <slot name="actions" />
      <ThemeToggle />
      <div class="topbar-user">
        <button
          class="user-meta user-meta-btn"
          type="button"
          aria-label="进入我的概况"
          @click="handleOpenProfile"
        >
          <div class="user-name">{{ userName }}</div>
          <div class="user-level">单位 {{ userUnitLabel }}</div>
        </button>
        <button class="logout-btn" type="button" aria-label="退出登录" @click="handleLogout">
          退出
        </button>
      </div>
    </div>
    <div v-if="showSearch" class="user-topbar-center">
      <div class="portal-search topbar-search user-topbar-search">
        <i class="fa-solid fa-magnifying-glass portal-search-icon" aria-hidden="true"></i>
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
    </div>
  </header>
</template>

<script setup>
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import ThemeToggle from '@/components/common/ThemeToggle.vue';
import { useAuthStore } from '@/stores/auth';
import { isDemoMode } from '@/utils/demo';

const props = defineProps({
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
    default: '搜索智能体应用'
  },
  hideChat: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:search']);

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const navItems = computed(() => {
  const items = [
    { label: '广场', path: `${basePath.value}/home` },
    { label: '工具管理', path: `${basePath.value}/tools` },
    { label: '聊天', path: `${basePath.value}/chat` }
  ];
  return props.hideChat ? items.filter((item) => item.label !== '聊天') : items;
});

const userName = computed(() => authStore.user?.username || '访客');
const userUnitLabel = computed(() => {
  const unit = authStore.user?.unit;
  return unit?.path_name || unit?.pathName || unit?.name || authStore.user?.unit_id || '-';
});

const updateSearch = (event) => {
  emit('update:search', event.target.value);
};

const clearSearch = () => {
  emit('update:search', '');
};

const handleOpenProfile = () => {
  router.push(`${basePath.value}/profile`);
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
