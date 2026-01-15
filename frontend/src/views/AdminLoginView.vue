<template>
  <div class="auth-page theme-light admin-auth">
    <div class="auth-card">
      <h2>管理员登录</h2>
      <el-form :model="form" label-position="top" @submit.prevent>
        <el-form-item label="用户名">
          <el-input v-model="form.username" placeholder="请输入用户名" />
        </el-form-item>
        <el-form-item label="密码">
          <el-input v-model="form.password" type="password" placeholder="请输入密码" />
        </el-form-item>
        <div class="auth-actions">
          <el-button type="primary" :loading="loading" @click="handleLogin">登录</el-button>
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

const router = useRouter();
const authStore = useAuthStore();
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
      ElMessage.error('当前账号无管理权限');
      return;
    }
    router.push('/admin/users');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '登录失败');
  }
};
</script>
