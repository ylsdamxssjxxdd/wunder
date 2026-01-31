<template>
  <div :class="['auth-page', themeClass]">
    <div class="auth-card">
      <h2>{{ t('auth.register.title') }}</h2>
      <el-form :model="form" label-position="top" @submit.prevent>
        <el-form-item :label="t('auth.register.username')">
          <el-input v-model="form.username" :placeholder="t('auth.placeholder.username')" />
        </el-form-item>
        <el-form-item :label="t('auth.register.email')">
          <el-input v-model="form.email" :placeholder="t('auth.placeholder.email')" />
        </el-form-item>
        <el-form-item :label="t('auth.register.password')">
          <el-input v-model="form.password" type="password" :placeholder="t('auth.placeholder.password')" />
        </el-form-item>
        <div class="auth-actions">
          <el-button type="primary" :loading="loading" @click="handleRegister">
            {{ t('auth.register.submit') }}
          </el-button>
          <el-button text @click="goLogin">{{ t('auth.register.login') }}</el-button>
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
  password: '',
  email: ''
});

const loading = computed(() => authStore.loading);
// 根据主题状态切换注册页暗色/浅色样式
const themeClass = computed(() => (themeStore.mode === 'light' ? 'theme-light' : 'theme-dark'));

const handleRegister = async () => {
  try {
    await authStore.register(form);
    router.push('/app/chat');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || t('auth.register.error'));
  }
};

const goLogin = () => router.push('/login');
</script>
