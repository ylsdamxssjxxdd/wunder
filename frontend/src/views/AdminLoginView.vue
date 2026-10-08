<template>
  <div class="auth-page theme-light admin-auth">
    <div class="auth-card">
      <h2>{{ t('admin.login.title') }}</h2>
      <el-form :model="form" label-position="top" @submit.prevent>
        <el-form-item :label="t('admin.login.username')">
          <el-input v-model="form.username" :placeholder="t('auth.placeholder.username')" />
        </el-form-item>
        <el-form-item :label="t('admin.login.password')">
          <el-input v-model="form.password" type="password" :placeholder="t('auth.placeholder.password')" />
        </el-form-item>
        <div class="auth-actions">
          <el-button type="primary" :loading="loading" @click="handleLogin">
            {{ t('admin.login.action') }}
          </el-button>
        </div>
      </el-form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, computed } from 'vue';
import { useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { showApiError } from '@/utils/apiError';

const router = useRouter();
const authStore = useAuthStore();
const { t } = useI18n();
const form = reactive({
  username: '',
  password: ''
});

const loading = computed(() => authStore.loading);

const handleLogin = async () => {
  try {
    const data = await authStore.login(form);
    const roles = data?.user?.roles || [];
    if (!roles.includes('admin') && !roles.includes('super_admin')) {
      authStore.logout();
      ElMessage.error(t('admin.login.noPermission'));
      return;
    }
    router.push('/admin/users');
  } catch (error) {
    showApiError(error, t('admin.login.error'));
  }
};
</script>