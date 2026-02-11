<template>
  <header class="user-topbar topbar">
    <div class="brand">
      <div class="brand-mark">AI</div>
      <div class="brand-meta">
        <div class="brand-title-row">
          <div class="brand-title">{{ resolvedTitle }}</div>
        </div>
        <div class="brand-sub">
          <span>{{ resolvedSubtitle }}</span>
          <span v-if="demoMode" class="demo-badge">{{ t('user.demoMode') }}</span>
        </div>
      </div>
    </div>
    <div class="topbar-actions">
      <nav class="topbar-nav">
        <router-link
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          class="topbar-panel-btn topbar-nav-link icon-only"
          :title="item.label"
          :aria-label="item.label"
        >
          <i
            v-if="item.icon"
            class="fa-solid topbar-icon"
            :class="item.icon"
            aria-hidden="true"
          ></i>
        </router-link>
      </nav>
      <slot name="actions" />
      <ThemeToggle />
      <div class="topbar-user">
        <button
          class="user-meta user-meta-btn"
          type="button"
          :aria-label="t('user.profile.enter')"
          @click="handleOpenProfile"
        >
          <div class="user-name">{{ userName }}</div>
          <div class="user-level">{{ t('user.unitLabel', { unit: userUnitLabel }) }}</div>
        </button>
        <button
          class="logout-btn"
          type="button"
          :aria-label="t('nav.logout')"
          @click="handleLogout"
        >
          {{ t('nav.logout') }}
        </button>
      </div>
    </div>
    <div v-if="showSearch" class="user-topbar-center">
      <div class="portal-search topbar-search user-topbar-search">
        <i class="fa-solid fa-magnifying-glass portal-search-icon" aria-hidden="true"></i>
        <input
          :value="search"
          type="text"
          :placeholder="resolvedSearchPlaceholder"
          @input="updateSearch"
        />
        <button
          v-if="search"
          class="portal-search-clear"
          type="button"
          :aria-label="t('portal.search.clear')"
          @click="clearSearch"
        >
          ×
        </button>
      </div>
    </div>
  </header>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import ThemeToggle from '@/components/common/ThemeToggle.vue';
import { useAuthStore } from '@/stores/auth';
import { isDemoMode } from '@/utils/demo';
import { useI18n } from '@/i18n';
import { resolveUserBasePath } from '@/utils/basePath';

const props = defineProps({
  title: {
    type: String,
    default: ''
  },
  subtitle: {
    type: String,
    default: ''
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
    default: ''
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
const { t } = useI18n();

const demoMode = computed(() => route.path.startsWith('/demo') || isDemoMode());
const basePath = computed(() => resolveUserBasePath(route.path));
const desktopMode = computed(() => basePath.value === '/desktop');
const navItems = computed(() => {
  const items = [
    { key: 'nav.world', label: t('nav.world'), path: `${basePath.value}/home`, icon: 'fa-earth-asia' },
    { key: 'nav.tools', label: t('nav.tools'), path: `${basePath.value}/tools`, icon: 'fa-toolbox' }
  ];

  if (!props.hideChat) {
    items.push({ key: 'nav.chat', label: t('nav.chat'), path: `${basePath.value}/chat`, icon: 'fa-comment-dots' });
  }

  if (desktopMode.value) {
    items.push({
      key: 'desktop.system',
      label: t('desktop.settings.system'),
      path: '/desktop/system',
      icon: 'fa-gear'
    });
  }

  return items;
});

const userName = computed(() => authStore.user?.username || t('user.guest'));
const userUnitLabel = computed(() => {
  const unit = authStore.user?.unit;
  return unit?.path_name || unit?.pathName || unit?.name || authStore.user?.unit_id || '-';
});

const resolvedTitle = computed(() => props.title || t('portal.title'));
const resolvedSubtitle = computed(() => props.subtitle || t('portal.subtitle'));
const resolvedSearchPlaceholder = computed(
  () => props.searchPlaceholder || t('portal.search.placeholder')
);

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
  if (basePath.value === '/desktop') {
    router.push('/desktop/home');
    return;
  }
  if (demoMode.value) {
    router.push('/login');
    return;
  }
  authStore.logout();
  router.push('/login');
};
</script>
