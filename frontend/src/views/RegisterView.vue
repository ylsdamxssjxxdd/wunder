<template>
  <div class="auth-page auth-page--user">
    <div class="auth-card auth-card--user">
      <h2 class="auth-title">{{ t('auth.register.title') }}</h2>
      <form class="auth-form" @submit.prevent="handleRegister">
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.register.username') }}</span>
          <input
            v-model.trim="form.username"
            class="auth-input"
            type="text"
            :placeholder="t('auth.placeholder.username')"
            autocomplete="username"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.register.email') }}</span>
          <input
            v-model.trim="form.email"
            class="auth-input"
            type="email"
            :placeholder="t('auth.placeholder.email')"
            autocomplete="email"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.register.unit') }}</span>
          <select
            v-model="form.unit_id"
            class="auth-input auth-select"
            :disabled="unitLoading || unitOptions.length === 0"
          >
            <option value="">
              {{ unitLoading ? t('common.loading') : t('auth.placeholder.unit') }}
            </option>
            <option v-for="unit in unitOptions" :key="unit.value" :value="unit.value">
              {{ unit.label }}
            </option>
          </select>
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.register.password') }}</span>
          <input
            v-model="form.password"
            class="auth-input"
            type="password"
            :placeholder="t('auth.placeholder.password')"
            autocomplete="new-password"
          />
        </label>
        <div class="auth-actions auth-actions--user">
          <button class="auth-submit-btn" type="submit" :disabled="loading">
            {{ loading ? t('common.loading') : t('auth.register.submit') }}
          </button>
          <button class="auth-link-btn" type="button" @click="goLogin">
            {{ t('auth.register.login') }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue';
import { useRouter } from 'vue-router';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { fetchOrgUnits } from '@/api/auth';
import { showApiError } from '@/utils/apiError';

const router = useRouter();
const authStore = useAuthStore();
const { t } = useI18n();
const form = reactive({
  username: '',
  password: '',
  email: '',
  unit_id: ''
});

const loading = computed(() => authStore.loading);
const unitOptions = ref<Array<{ value: string; label: string }>>([]);
const unitLoading = ref(false);

const buildUnitOptions = (items: Array<Record<string, unknown>>) =>
  (items || [])
    .map((raw) => {
      const unit = raw || {};
      const value = String(unit.unit_id || unit.id || '').trim();
      const label = String(unit.path_name || unit.pathName || unit.name || value || '-').trim();
      return { value, label };
    })
    .filter((unit) => Boolean(unit.value))
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
    router.push('/app/home');
  } catch (error) {
    showApiError(error, t('auth.register.error'));
  }
};

const goLogin = () => router.push('/login');

onMounted(() => {
  loadUnits();
});
</script>
