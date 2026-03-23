import { resolveApiError } from '@/utils/apiError';

type Translate = (key: string, params?: Record<string, unknown>) => string;

export type LoginField = 'username' | 'password';

export type LoginFormState = {
  username: string;
  password: string;
};

export type LoginValidationResult = {
  isValid: boolean;
  summary: string;
  focusField: LoginField | null;
  fieldErrors: Partial<Record<LoginField, string>>;
};

const containsChinese = (value: string): boolean => /[\u4e00-\u9fff]/.test(value);

const normalizeText = (value: unknown): string => String(value || '').trim();

const includesAny = (value: string, patterns: string[]): boolean =>
  patterns.some((pattern) => value.includes(pattern));

export const validateLoginForm = (
  form: LoginFormState,
  t: Translate
): LoginValidationResult => {
  const username = normalizeText(form.username);
  const password = normalizeText(form.password);
  const fieldErrors: Partial<Record<LoginField, string>> = {};

  if (!username) {
    fieldErrors.username = t('auth.login.usernameRequired');
  }
  if (!password) {
    fieldErrors.password = t('auth.login.passwordRequired');
  }

  if (!fieldErrors.username && !fieldErrors.password) {
    return {
      isValid: true,
      summary: '',
      focusField: null,
      fieldErrors
    };
  }

  if (fieldErrors.username && fieldErrors.password) {
    return {
      isValid: false,
      summary: t('auth.login.fieldsRequired'),
      focusField: 'username',
      fieldErrors
    };
  }

  return {
    isValid: false,
    summary: fieldErrors.username || fieldErrors.password || t('auth.login.error'),
    focusField: fieldErrors.username ? 'username' : 'password',
    fieldErrors
  };
};

export const resolveLoginErrorMessage = (source: unknown, t: Translate): string => {
  const resolved = resolveApiError(source, t('auth.login.error'));
  const message = normalizeText(resolved.message);
  const normalized = message.toLowerCase();

  if (!message) {
    return t('auth.login.error');
  }
  if (containsChinese(message)) {
    return message;
  }
  if (
    includesAny(normalized, [
      'network error',
      'failed to fetch',
      'load failed',
      'timeout',
      'timed out',
      'econnrefused',
      'socket hang up'
    ])
  ) {
    return t('auth.login.networkError');
  }
  if (
    includesAny(normalized, [
      'invalid password',
      'user not found',
      'invalid credentials',
      'authentication failed',
      'unauthorized'
    ])
  ) {
    return t('auth.login.credentialsInvalid');
  }
  if (includesAny(normalized, ['invalid username'])) {
    return t('auth.login.usernameInvalid');
  }
  if (
    includesAny(normalized, [
      'content required',
      'username is empty',
      'password is empty',
      'missing username',
      'missing password'
    ])
  ) {
    return t('auth.login.fieldsRequired');
  }
  if ((resolved.status || 0) >= 500) {
    return t('auth.login.serverUnavailable');
  }
  return t('auth.login.error');
};
