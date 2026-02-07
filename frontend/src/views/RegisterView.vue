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
        <el-form-item :label="t('auth.register.unit')">
          <el-select
            v-model="form.unit_id"
            :placeholder="t('auth.placeholder.unit')"
            filterable
            clearable
            :loading="unitLoading"
            :disabled="unitLoading || unitOptions.length === 0"
            style="width: 100%"
          >
            <el-option
              v-for="unit in unitOptions"
              :key="unit.value"
              :label="unit.label"
              :value="unit.value"
            />
          </el-select>
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
import { computed, onMounted, reactive, ref } from 'vue';
import { useRouter } from 'vue-router';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { useThemeStore } from '@/stores/theme';
import { fetchOrgUnits } from '@/api/auth';
import { showApiError } from '@/utils/apiError';

const router = useRouter();
const authStore = useAuthStore();
const themeStore = useThemeStore();
const { t } = useI18n();
const form = reactive({
  username: '',
  password: '',
  email: '',
  unit_id: ''
});

const loading = computed(() => authStore.loading);
const unitOptions = ref([]);
const unitLoading = ref(false);
// 根据主题状态切换注册页暗色/浅色样式
const themeClass = computed(() => (themeStore.mode === 'light' ? 'theme-light' : 'theme-dark'));

const buildUnitOptions = (items) =>
  (items || [])
    .map((unit) => ({
      value: unit.unit_id || unit.id || '',
      label: unit.path_name || unit.pathName || unit.name || unit.unit_id || '-'
    }))
    .filter((unit) => unit.value)
    .sort((left, right) => left.label.localeCompare(right.label, 'zh-CN'));

const loadUnits = async () => {
  unitLoading.value = true;
  try {
    const { data } = await fetchOrgUnits();
    const items = data?.data?.items || [];
    unitOptions.value = buildUnitOptions(items);
  } catch (error) {
    unitOptions.value = [];
  } finally {
    unitLoading.value = false;
  }
};

const handleRegister = async () => {
  try {
    await authStore.register({
      ...form,
      unit_id: form.unit_id || ''
    });
    router.push('/app/chat');
  } catch (error) {
    showApiError(error, t('auth.register.error'));
  }
};

const goLogin = () => router.push('/login');

onMounted(() => {
  loadUnits();
});
</script>