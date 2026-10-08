import type { Router } from 'vue-router';

import { fetchMe } from '@/api/auth';
import { useAuthStore } from '@/stores/auth';
import { redirectToLoginAfterLogout, resolveLogoutRedirectPath } from '@/utils/authNavigation';
import {
  ACCESS_TOKEN_STORAGE_KEY,
  isSessionAccessTokenMode,
  readAccessToken
} from '@/utils/authTokenStorage';

const SESSION_SYNC_INTERVAL_MS = 15000;

let installed = false;
let heartbeatTimer: number | null = null;
let checking = false;
let lastForcedLogoutAt = 0;

const isPublicAuthPath = (path: string): boolean => {
  const normalized = String(path || '').trim().toLowerCase();
  return (
    normalized === '/login' ||
    normalized === '/register' ||
    normalized === '/admin/login' ||
    normalized.startsWith('/demo')
  );
};

const isAuthFailure = (error: unknown): boolean => {
  const source = (error || {}) as {
    response?: {
      status?: unknown;
      data?: {
        error?: {
          code?: unknown;
        };
        code?: unknown;
      };
    };
  };
  const status = Number(source.response?.status || 0);
  if (status === 401) {
    return true;
  }
  const code = String(source.response?.data?.error?.code || source.response?.data?.code || '')
    .trim()
    .toUpperCase();
  return code === 'AUTH_REQUIRED' || code === 'SESSION_REPLACED';
};

const forceLogout = (router: Router): void => {
  const now = Date.now();
  if (now - lastForcedLogoutAt < 1500) {
    return;
  }
  lastForcedLogoutAt = now;
  const authStore = useAuthStore();
  authStore.logout();
  const target = resolveLogoutRedirectPath(router.currentRoute.value.path);
  redirectToLoginAfterLogout((to) => router.replace(to), target);
};

const shouldCheckSession = (router: Router): boolean => {
  if (typeof window === 'undefined') {
    return false;
  }
  const path = router.currentRoute.value.path;
  if (isPublicAuthPath(path)) {
    return false;
  }
  return Boolean(readAccessToken());
};

const checkSession = async (router: Router): Promise<void> => {
  if (checking || !shouldCheckSession(router)) {
    return;
  }
  checking = true;
  try {
    await fetchMe();
    lastForcedLogoutAt = 0;
  } catch (error) {
    if (isAuthFailure(error)) {
      forceLogout(router);
    }
  } finally {
    checking = false;
  }
};

const handleStorage = (router: Router, event: StorageEvent): void => {
  if (event.key !== ACCESS_TOKEN_STORAGE_KEY) {
    return;
  }
  if (isSessionAccessTokenMode()) {
    return;
  }
  if (isPublicAuthPath(router.currentRoute.value.path)) {
    return;
  }
  const previousToken = String(event.oldValue || '').trim();
  const nextToken = String(event.newValue || '').trim();
  if (previousToken && !nextToken) {
    forceLogout(router);
  }
};

export const installAuthSessionSync = (router: Router): void => {
  if (installed || typeof window === 'undefined') {
    return;
  }
  installed = true;
  const handleStorageEvent = (event: StorageEvent) => handleStorage(router, event);
  const handleFocus = () => {
    void checkSession(router);
  };
  const handleVisibilityChange = () => {
    if (document.visibilityState === 'visible') {
      void checkSession(router);
    }
  };
  window.addEventListener('storage', handleStorageEvent);
  window.addEventListener('focus', handleFocus);
  document.addEventListener('visibilitychange', handleVisibilityChange);
  heartbeatTimer = window.setInterval(() => {
    void checkSession(router);
  }, SESSION_SYNC_INTERVAL_MS);
};

export const stopAuthSessionSync = (): void => {
  if (heartbeatTimer !== null && typeof window !== 'undefined') {
    window.clearInterval(heartbeatTimer);
  }
  heartbeatTimer = null;
};
