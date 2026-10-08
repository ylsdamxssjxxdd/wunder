<template>
  <div class="layout theme-light admin-shell">
    <el-container class="layout-container admin-container">
      <el-aside width="240px" class="layout-aside admin-aside">
        <div class="admin-brand">
          <div class="brand-mark">W</div>
          <div class="brand-meta">
            <div class="brand-title">wille</div>
            <div class="brand-sub">{{ t('admin.title') }}</div>
          </div>
        </div>
        <el-menu class="layout-menu admin-menu" router :default-active="activePath">
          <el-menu-item index="/admin/users">
            <span>{{ t('admin.nav.users') }}</span>
          </el-menu-item>
          <el-menu-item index="/admin/agents">
            <span>{{ t('admin.nav.agents') }}</span>
          </el-menu-item>
          <el-menu-item index="/admin/system">
            <span>{{ t('admin.nav.system') }}</span>
          </el-menu-item>
        </el-menu>
      </el-aside>
      <el-container>
        <el-header class="layout-header admin-header">
          <div class="header-left">
            <div class="header-title">{{ currentTitle }}</div>
            <div class="header-sub">{{ t('admin.subtitle') }}</div>
          </div>
          <div class="header-actions">
            <div class="admin-user">
              <div class="admin-user-meta">
                <div class="admin-user-name">{{ userName }}</div>
                <div class="admin-user-role">{{ roleLabel }}</div>
              </div>
            </div>
            <el-button type="primary" size="small" @click="logout">
              {{ t('admin.logout') }}
            </el-button>
          </div>
        </el-header>
        <el-main class="layout-main admin-main">
          <router-view />
        </el-main>
      </el-container>
    </el-container>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();
const { t } = useI18n();

const pageTitleMap = {
  '/admin/users': 'admin.nav.users',
  '/admin/agents': 'admin.nav.agents',
  '/admin/system': 'admin.nav.system'
};

const activePath = computed(() => route.path);
const currentTitle = computed(() => t(pageTitleMap[route.path] || 'admin.title'));
const userName = computed(() => authStore.user?.username || t('admin.userRole.default'));
const roleLabel = computed(() => {
  const roles = authStore.user?.roles || [];
  if (roles.includes('super_admin')) return t('admin.userRole.super');
  if (roles.includes('admin')) return t('admin.userRole.admin');
  return t('admin.userRole.default');
});

const logout = () => {
  authStore.logout();
  router.push('/admin/login');
};

onMounted(() => {
  authStore.loadProfile();
});
</script>
