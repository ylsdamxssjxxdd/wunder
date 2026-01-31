<template>
  <div :class="['auth-page', themeClass]">
    <div class="auth-card">
      <h2>{{ t('auth.login.title') }}</h2>
      <el-form :model="form" label-position="top" @submit.prevent>
        <el-form-item :label="t('auth.login.username')">
          <el-input v-model="form.username" :placeholder="t('auth.placeholder.username')" />
        </el-form-item>
        <el-form-item :label="t('auth.login.password')">
          <el-input v-model="form.password" type="password" :placeholder="t('auth.placeholder.password')" />
        </el-form-item>
        <div class="auth-actions">
          <el-button type="primary" :loading="loading" @click="handleLogin">
            {{ t('auth.login.action') }}
          </el-button>
          <el-button text @click="goRegister">{{ t('auth.login.register') }}</el-button>
        </div>
      </el-form>
    </div>
  </div>
</template>

<script setup>
import { reactive, computed } from 'vue';
import { useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useThemeStore } from '@/stores/theme';

const router = useRouter();
const authStore = useAuthStore();
const themeStore = useThemeStore();
const { t } = useI18n();
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
    ElMessage.error(error.response?.data?.detail || t('auth.login.error'));
  }
};

const goRegister = () => router.push('/register');
</script>
