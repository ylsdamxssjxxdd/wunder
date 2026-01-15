<template>
  <div :class="['auth-page', themeClass]">
    <div class="auth-card">
      <h2>用户登录</h2>
      <el-form :model="form" label-position="top" @submit.prevent>
        <el-form-item label="用户名">
          <el-input v-model="form.username" placeholder="请输入用户名" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="form.password" type="password" placeholder="请输入密码" />
        </el-form-item>
        <div class="auth-actions">
          <el-button type="primary" :loading="loading" @click="handleLogin">登录</el-button>
          <el-button text @click="goRegister">注册账号</el-button>
        </div>
      </el-form>
    </div>
  </div>
</template>

<script setup>
import { reactive, computed } from 'vue';
import { useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useAuthStore } from '@/stores/auth';
import { useThemeStore } from '@/stores/theme';

const router = useRouter();
const authStore = useAuthStore();
const themeStore = useThemeStore();
const form = reactive({
  username: '',
  password: ''
});

const loading = computed(() => authStore.loading);
// 根据主题状态切换登录页暗色/浅色样式
const themeClass = computed(() => (themeStore.mode === 'light' ? 'theme-light' : 'theme-dark'));

const handleLogin = async () => {
  try {
    await authStore.login(form);
    router.push('/app/chat');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '登录失败');
  }
};

const goRegister = () => router.push('/register');
</script>
