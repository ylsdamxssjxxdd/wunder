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
          <button class="auth-link-btn" type="button" @click="toggleResetPanel">
            {{ resetPanelVisible ? t('auth.reset.cancel') : t('auth.login.resetPassword') }}
          </button>
        </div>
      </form>

      <form v-if="resetPanelVisible" class="auth-reset-panel" @submit.prevent="handleResetPassword">
        <div class="auth-reset-head">
          <div class="auth-reset-title">{{ t('auth.reset.title') }}</div>
          <div class="auth-reset-hint">{{ t('auth.reset.hint') }}</div>
        </div>
        <p v-if="resetError" class="auth-alert auth-alert--compact" role="alert" aria-live="polite">
          {{ resetError }}
        </p>
        <p
          v-else-if="resetSuccess"
          class="auth-reset-status auth-reset-status--success"
          role="status"
          aria-live="polite"
        >
          {{ resetSuccess }}
        </p>
        <label class="auth-field">
          <span class="auth-label">{{ t('auth.login.username') }}</span>
          <input
            v-model.trim="resetForm.username"
            class="auth-input"
            type="text"
            :placeholder="t('auth.placeholder.username')"
            autocomplete="username"
            @input="handleResetFieldInput"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('profile.edit.email') }}</span>
          <input
            v-model.trim="resetForm.email"
            class="auth-input"
            type="email"
            :placeholder="t('profile.edit.emailPlaceholder')"
            autocomplete="email"
            @input="handleResetFieldInput"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('profile.edit.newPassword') }}</span>
          <input
            v-model="resetForm.newPassword"
            class="auth-input"
            type="password"
            :placeholder="t('profile.edit.newPasswordPlaceholder')"
            autocomplete="new-password"
            @input="handleResetFieldInput"
          />
        </label>
        <label class="auth-field">
          <span class="auth-label">{{ t('profile.edit.confirmPassword') }}</span>
          <input
            v-model="resetForm.confirmPassword"
            class="auth-input"
            type="password"
            :placeholder="t('profile.edit.confirmPasswordPlaceholder')"
            autocomplete="new-password"
            @input="handleResetFieldInput"
          />
        </label>
        <div class="auth-reset-actions">
          <button class="auth-submit-btn" type="submit" :disabled="resetSubmitting">
            {{ resetSubmitting ? t('common.loading') : t('auth.reset.action') }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, reactive, ref } from 'vue';
import { useRouter } from 'vue-router';

import { isDesktopModeEnabled } from '@/config/desktop';
import { resetPassword } from '@/api/auth';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { buildDefaultAgentChatRoute } from '@/utils/authNavigation';
import { resolveApiError } from '@/utils/apiError';
import { resolveLoginErrorMessage, validateLoginForm, type LoginField } from '@/utils/authFeedback';

const router = useRouter();
const authStore = useAuthStore();
const { t } = useI18n();

const form = reactive({
  username: '',
  password: ''
});
const resetForm = reactive({
  username: '',
  email: '',
  newPassword: '',
  confirmPassword: ''
});

const usernameInput = ref<HTMLInputElement | null>(null);
const passwordInput = ref<HTMLInputElement | null>(null);
const submitError = ref('');
const resetError = ref('');
const resetSuccess = ref('');
const resetSubmitting = ref(false);
const resetPanelVisible = ref(false);
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

const handleResetFieldInput = () => {
  if (resetError.value) {
    resetError.value = '';
  }
  if (resetSuccess.value) {
    resetSuccess.value = '';
  }
};

const toggleResetPanel = () => {
  resetPanelVisible.value = !resetPanelVisible.value;
  resetError.value = '';
  resetSuccess.value = '';
  if (resetPanelVisible.value) {
    resetForm.username = String(form.username || '').trim();
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

const handleResetPassword = async () => {
  const username = String(resetForm.username || '').trim();
  const email = String(resetForm.email || '').trim();
  const newPassword = String(resetForm.newPassword || '').trim();
  const confirmPassword = String(resetForm.confirmPassword || '').trim();
  resetError.value = '';
  resetSuccess.value = '';

  if (!username) {
    resetError.value = t('profile.edit.usernameRequired');
    return;
  }
  if (!email) {
    resetError.value = t('auth.reset.emailRequired');
    return;
  }
  if (!newPassword) {
    resetError.value = t('profile.edit.newPasswordRequired');
    return;
  }
  if (!confirmPassword) {
    resetError.value = t('profile.edit.confirmPasswordRequired');
    return;
  }
  if (newPassword !== confirmPassword) {
    resetError.value = t('profile.edit.passwordMismatch');
    return;
  }

  resetSubmitting.value = true;
  try {
    await resetPassword({
      username,
      email,
      new_password: newPassword
    });
    form.username = username;
    form.password = '';
    resetForm.newPassword = '';
    resetForm.confirmPassword = '';
    resetSuccess.value = t('auth.reset.success');
  } catch (error) {
    resetError.value = resolveApiError(error, t('auth.reset.failed')).message;
  } finally {
    resetSubmitting.value = false;
  }
};

const goRegister = () => router.push('/register');
</script>
