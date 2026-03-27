<template>
  <div class="auth-page auth-page--user">
    <div class="auth-card auth-card--user">
      <h2 class="auth-title">{{ t('auth.login.title') }}</h2>
      <p v-if="submitError" class="auth-alert" role="alert" aria-live="polite">
        {{ submitError }}
      </p>
      <form class="auth-form" @submit.prevent="handleLogin">
        <label class="auth-field" :class="{ 'auth-field--error': fieldErrors.username }">
          <span class="auth-label">{{ t('auth.login.username') }}</span>
          <input
            ref="usernameInput"
            v-model.trim="form.username"
            class="auth-input"
            :class="{ 'auth-input--error': fieldErrors.username }"
            type="text"
            :placeholder="t('auth.placeholder.username')"
            autocomplete="username"
            :aria-invalid="fieldErrors.username ? 'true' : 'false'"
            @input="handleFieldInput('username')"
          />
          <span v-if="fieldErrors.username" class="auth-field-error">{{ fieldErrors.username }}</span>
        </label>
        <label class="auth-field" :class="{ 'auth-field--error': fieldErrors.password }">
          <span class="auth-label">{{ t('auth.login.password') }}</span>
          <input
            ref="passwordInput"
            v-model="form.password"
            class="auth-input"
            :class="{ 'auth-input--error': fieldErrors.password }"
            type="password"
            :placeholder="t('auth.placeholder.password')"
            autocomplete="current-password"
            :aria-invalid="fieldErrors.password ? 'true' : 'false'"
            @input="handleFieldInput('password')"
          />
          <span v-if="fieldErrors.password" class="auth-field-error">{{ fieldErrors.password }}</span>
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
import { reactive, computed, nextTick, ref } from 'vue';
import { useRouter } from 'vue-router';

import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { buildDefaultAgentChatRoute } from '@/utils/authNavigation';
import { resolveLoginErrorMessage, validateLoginForm, type LoginField } from '@/utils/authFeedback';
import { isDesktopModeEnabled } from '@/config/desktop';

const router = useRouter();
const authStore = useAuthStore();
const { t } = useI18n();
const form = reactive({
  username: '',
  password: ''
});
const usernameInput = ref<HTMLInputElement | null>(null);
const passwordInput = ref<HTMLInputElement | null>(null);
const submitError = ref('');
const fieldErrors = reactive<Partial<Record<LoginField, string>>>({
  username: '',
  password: ''
});

const loading = computed(() => authStore.loading);

const clearFieldErrors = () => {
  fieldErrors.username = '';
  fieldErrors.password = '';
};

const focusField = async (field: LoginField | null) => {
  if (!field) return;
  await nextTick();
  if (field === 'username') {
    usernameInput.value?.focus();
    return;
  }
  passwordInput.value?.focus();
};

const handleFieldInput = (field: LoginField) => {
  if (fieldErrors[field]) {
    fieldErrors[field] = '';
  }
  if (submitError.value) {
    submitError.value = '';
  }
};

const handleLogin = async () => {
  clearFieldErrors();
  submitError.value = '';
  const validation = validateLoginForm(form, t);
  if (!validation.isValid) {
    fieldErrors.username = validation.fieldErrors.username || '';
    fieldErrors.password = validation.fieldErrors.password || '';
    submitError.value = validation.summary;
    await focusField(validation.focusField);
    return;
  }
  try {
    await authStore.login(form);
    router.push(buildDefaultAgentChatRoute({ desktop: isDesktopModeEnabled() }));
  } catch (error) {
    submitError.value = resolveLoginErrorMessage(error, t);
  }
};

const goRegister = () => router.push('/register');
</script>
