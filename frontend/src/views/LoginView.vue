<template>
  <div class="auth-page auth-page--user">
    <div class="auth-card auth-card--user">
      <h2 class="auth-title">{{ t('auth.login.title') }}</h2>
      <form class="auth-form" @submit.prevent="handleLogin">
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.login.username') }}</span>
          <input
            v-model.trim="form.username"
            class="auth-input"
            type="text"
            :placeholder="t('auth.placeholder.username')"
            autocomplete="username"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.login.password') }}</span>
          <input
            v-model="form.password"
            class="auth-input"
            type="password"
            :placeholder="t('auth.placeholder.password')"
            autocomplete="current-password"
          />
        </label>
        <div class="auth-actions auth-actions--user">
          <button class="auth-submit-btn" type="submit" :disabled="loading">
            {{ loading ? t('common.loading') : t('auth.login.action') }}
          </button>
          <button class="auth-link-btn" type="button" @click="goRegister">
            {{ t('auth.login.register') }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, computed } from 'vue';
import { useRouter } from 'vue-router';

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
    await authStore.login(form);
    router.push('/app/home');
  } catch (error) {
    showApiError(error, t('auth.login.error'));
  }
};

const goRegister = () => router.push('/register');
</script>
