<template>
  <div class="layout theme-light admin-shell">
    <el-container class="layout-container admin-container">
      <el-aside width="240px" class="layout-aside admin-aside">
        <div class="admin-brand">
          <div class="brand-mark">W</div>
          <div class="brand-meta">
            <div class="brand-title">wille</div>
            <div class="brand-sub">管理控制台</div>
          </div>
        </div>
        <el-menu class="layout-menu admin-menu" router :default-active="activePath">
          <el-menu-item index="/admin/users">
            <span>用户管理</span>
          </el-menu-item>
          <el-menu-item index="/admin/agents">
            <span>Wunder 设置</span>
          </el-menu-item>
          <el-menu-item index="/admin/system">
            <span>系统状态</span>
          </el-menu-item>
        </el-menu>
      </el-aside>
      <el-container>
        <el-header class="layout-header admin-header">
          <div class="header-left">
            <div class="header-title">{{ currentTitle }}</div>
            <div class="header-sub">后台管理 · wille</div>
          </div>
          <div class="header-actions">
            <div class="admin-user">
              <div class="admin-user-meta">
                <div class="admin-user-name">{{ userName }}</div>
                <div class="admin-user-role">{{ roleLabel }}</div>
              </div>
            </div>
            <el-button type="primary" size="small" @click="logout">退出</el-button>
          </div>
        </el-header>
        <el-main class="layout-main admin-main">
          <router-view />
        </el-main>
      </el-container>
    </el-container>
  </div>
</template>

<script setup>
import { computed, onMounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { useAuthStore } from '@/stores/auth';

const route = useRoute();
const router = useRouter();
const authStore = useAuthStore();

const pageTitleMap = {
  '/admin/users': '用户管理',
  '/admin/agents': 'Wunder 设置',
  '/admin/system': '系统状态'
};

const activePath = computed(() => route.path);
const currentTitle = computed(() => pageTitleMap[route.path] || '管理控制台');
const userName = computed(() => authStore.user?.username || '管理员');
const roleLabel = computed(() => {
  const roles = authStore.user?.roles || [];
  if (roles.includes('super_admin')) return '超级管理员';
  if (roles.includes('admin')) return '管理员';
  return '管理员';
});

const logout = () => {
  authStore.logout();
  router.push('/admin/login');
};

onMounted(() => {
  authStore.loadProfile();
});
</script>
