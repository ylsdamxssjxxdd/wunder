import { createRouter, createWebHistory } from 'vue-router';
import type { LocationQuery, LocationQueryRaw, RouteRecordRaw } from 'vue-router';

import UserLayout from '@/layouts/UserLayout.vue';
import AdminLayout from '@/layouts/AdminLayout.vue';
import LoginView from '@/views/LoginView.vue';
import RegisterView from '@/views/RegisterView.vue';
import MessengerView from '@/views/MessengerView.vue';
import ExternalAppView from '@/views/ExternalAppView.vue';
import AdminLoginView from '@/views/AdminLoginView.vue';
import AdminUsersView from '@/views/AdminUsersView.vue';
import AdminAgentsView from '@/views/AdminAgentsView.vue';
import AdminSystemView from '@/views/AdminSystemView.vue';
import { disableDemoMode, enableDemoMode } from '@/utils/demo';
import { useAuthStore } from '@/stores/auth';
import { isDesktopModeEnabled, isDesktopRemoteAuthMode } from '@/config/desktop';
import { resolveApiBase } from '@/config/runtime';

const USER_LOGIN_PATH = '/login';
const USER_BEEHIVE_PATH = '/app/home';
const DESKTOP_HOME_PATH = '/desktop/home';
const EMBED_AUTH_QUERY_KEYS = new Set(['wunder_token', 'access_token', 'wunder_code']);

const hasAccessToken = () => Boolean(localStorage.getItem('access_token'));

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const asQueryText = (value: unknown): string => {
  if (Array.isArray(value)) {
    for (const item of value) {
      const text = String(item || '').trim();
      if (text) return text;
    }
    return '';
  }
  return String(value || '').trim();
};

const resolveQueryToken = (query: LocationQuery): string => {
  const wunderToken = asQueryText(query.wunder_token);
  if (wunderToken) return wunderToken;
  return asQueryText(query.access_token);
};

const resolveQueryCode = (query: LocationQuery): string => asQueryText(query.wunder_code);

const stripEmbedAuthQuery = (query: LocationQuery): LocationQueryRaw => {
  const output: LocationQueryRaw = {};
  Object.entries(query).forEach(([key, value]) => {
    if (!EMBED_AUTH_QUERY_KEYS.has(key)) {
      output[key] = value as string | null | (string | null)[];
    }
  });
  return output;
};

const hasEmbedAuthQuery = (query: LocationQuery): boolean =>
  Object.keys(query).some((key) => EMBED_AUTH_QUERY_KEYS.has(key));

const resolveApiEndpoint = (path: string): string => {
  const apiBase = resolveApiBase();
  const base = apiBase ? apiBase.replace(/\/+$/, '') : '/wunder';
  return `${base}${path}`;
};

const exchangeEmbedCode = async (code: string): Promise<string> => {
  const response = await fetch(resolveApiEndpoint('/auth/external/exchange'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ code })
  });

  const payload = asRecord(await response.json().catch(() => ({})));
  if (!response.ok) {
    const error = asRecord(payload.error);
    const message = String(error.message || payload.message || 'external auth exchange failed').trim();
    throw new Error(message || 'external auth exchange failed');
  }

  const data = asRecord(payload.data);
  const token = String(data.access_token || '').trim();
  if (!token) {
    throw new Error('external auth token is empty');
  }
  return token;
};

const isAuthRequiredError = (error: unknown): boolean => {
  const source = asRecord(error);
  const response = asRecord(source.response);
  const status = Number(response.status || 0);
  if (status === 401) {
    return true;
  }
  const payload = asRecord(response.data);
  const payloadError = asRecord(payload.error);
  const errorCode = String(payloadError.code || payload.code || payload.message || '')
    .trim()
    .toLowerCase();
  return errorCode === 'auth_required' || errorCode === 'error.auth_required';
};

const routes: RouteRecordRaw[] = [
  {
    path: '/',
    redirect: () => (isDesktopModeEnabled() ? DESKTOP_HOME_PATH : hasAccessToken() ? USER_BEEHIVE_PATH : USER_LOGIN_PATH)
  },
  {
    path: '/home',
    redirect: () => (isDesktopModeEnabled() ? DESKTOP_HOME_PATH : USER_BEEHIVE_PATH)
  },
  {
    path: '/portal',
    redirect: () => (isDesktopModeEnabled() ? DESKTOP_HOME_PATH : '/home')
  },
  {
    path: '/login',
    name: 'login',
    component: LoginView
  },
  {
    path: '/register',
    name: 'register',
    component: RegisterView
  },
  {
    path: '/desktop',
    component: UserLayout,
    redirect: DESKTOP_HOME_PATH,
    children: [
      { path: 'home', name: 'desktop-home', component: MessengerView },
      { path: 'external/:linkId', name: 'desktop-external-app', component: ExternalAppView },
      { path: 'tools', name: 'desktop-tools', component: MessengerView },
      { path: 'cron', name: 'desktop-cron', component: MessengerView },
      { path: 'channels', name: 'desktop-channels', component: MessengerView },
      { path: 'chat', name: 'desktop-chat', component: MessengerView },
      { path: 'user-world', name: 'desktop-user-world', component: MessengerView },
      { path: 'workspace', name: 'desktop-workspace', component: MessengerView },
      { path: 'settings', name: 'desktop-settings', component: MessengerView },
      { path: 'containers', redirect: '/desktop/files?section=files&panel=containers' },
      { path: 'system', redirect: '/desktop/settings?section=more&panel=desktop' },
      { path: 'profile', name: 'desktop-profile', component: MessengerView }
    ]
  },
  {
    path: '/app',
    component: UserLayout,
    meta: { requiresAuth: true },
    redirect: USER_BEEHIVE_PATH,
    children: [
      { path: 'home', name: 'home', component: MessengerView },
      { path: 'external/:linkId', name: 'external-app', component: ExternalAppView },
      { path: 'tools', name: 'tools', component: MessengerView },
      { path: 'cron', name: 'cron', component: MessengerView },
      { path: 'channels', name: 'channels', component: MessengerView },
      { path: 'chat', name: 'chat', component: MessengerView },
      { path: 'user-world', name: 'user-world', component: MessengerView },
      { path: 'workspace', name: 'workspace', component: MessengerView },
      { path: 'settings', name: 'settings', component: MessengerView },
      { path: 'profile', name: 'profile', component: MessengerView }
    ]
  },
  {
    path: '/demo',
    component: UserLayout,
    meta: { demo: true },
    redirect: '/demo/chat',
    children: [
      { path: 'home', name: 'demo-home', component: MessengerView, meta: { demo: true } },
      { path: 'external/:linkId', name: 'demo-external-app', component: ExternalAppView, meta: { demo: true } },
      { path: 'tools', name: 'demo-tools', component: MessengerView, meta: { demo: true } },
      { path: 'cron', name: 'demo-cron', component: MessengerView, meta: { demo: true } },
      { path: 'channels', name: 'demo-channels', component: MessengerView, meta: { demo: true } },
      { path: 'chat', name: 'demo-chat', component: MessengerView, meta: { demo: true } },
      { path: 'user-world', name: 'demo-user-world', component: MessengerView, meta: { demo: true } },
      { path: 'workspace', name: 'demo-workspace', component: MessengerView, meta: { demo: true } },
      { path: 'settings', name: 'demo-settings', component: MessengerView, meta: { demo: true } },
      { path: 'profile', name: 'demo-profile', component: MessengerView, meta: { demo: true } }
    ]
  },
  {
    path: '/admin/login',
    name: 'admin-login',
    component: AdminLoginView
  },
  {
    path: '/admin',
    component: AdminLayout,
    meta: { requiresAuth: true, requiresAdmin: true },
    children: [
      { path: 'users', name: 'admin-users', component: AdminUsersView },
      { path: 'agents', name: 'admin-agents', component: AdminAgentsView },
      { path: 'system', name: 'admin-system', component: AdminSystemView }
    ]
  }
];

const router = createRouter({
  history: createWebHistory(),
  routes
});

router.beforeEach(async (to) => {
  const authStore = useAuthStore();

  const query = to.query;
  let tokenFromQuery = resolveQueryToken(query);
  if (!tokenFromQuery) {
    const code = resolveQueryCode(query);
    if (code) {
      try {
        tokenFromQuery = await exchangeEmbedCode(code);
      } catch {
        authStore.logout();
        return { path: USER_LOGIN_PATH, replace: true };
      }
    }
  }
  if (tokenFromQuery) {
    authStore.token = tokenFromQuery;
    authStore.user = null;
    localStorage.setItem('access_token', tokenFromQuery);
    if (hasEmbedAuthQuery(query)) {
      return {
        path: to.path,
        query: stripEmbedAuthQuery(query),
        hash: to.hash,
        replace: true
      };
    }
  }

  const desktopMode = isDesktopModeEnabled();

  if (!desktopMode && to.path.startsWith('/desktop')) {
    return hasAccessToken() ? USER_BEEHIVE_PATH : USER_LOGIN_PATH;
  }

  if (to.path.startsWith('/demo') && !desktopMode) {
    enableDemoMode();
    await authStore.loadProfile();
  } else {
    disableDemoMode();
  }

  if (desktopMode && !to.path.startsWith('/admin')) {
    const remoteAuthMode = isDesktopRemoteAuthMode();

    if (remoteAuthMode && (to.path === '/login' || to.path === '/register')) {
      if (!hasAccessToken()) {
        return true;
      }
      if (!authStore.user) {
        try {
          await authStore.loadProfile();
          return DESKTOP_HOME_PATH;
        } catch (error) {
          if (isAuthRequiredError(error)) {
            authStore.logout();
          }
          return true;
        }
      }
      return DESKTOP_HOME_PATH;
    }

    if (to.path.startsWith('/desktop')) {
      if (remoteAuthMode && !hasAccessToken()) {
        return USER_LOGIN_PATH;
      }
      if (!authStore.user) {
        try {
          await authStore.loadProfile();
        } catch (error) {
          if (remoteAuthMode && isAuthRequiredError(error)) {
            authStore.logout();
            return USER_LOGIN_PATH;
          }
          if (!remoteAuthMode) {
            // Ignore initial desktop profile load failures in local mode.
          }
        }
      }
      return true;
    }

    if (to.path === '/app') {
      return DESKTOP_HOME_PATH;
    }
    if (to.path.startsWith('/app/')) {
      return to.fullPath.replace(/^\/app\//, '/desktop/');
    }
    if (to.path === '/home' || to.path === '/portal') {
      return remoteAuthMode && !hasAccessToken() ? USER_LOGIN_PATH : DESKTOP_HOME_PATH;
    }

    if (remoteAuthMode && !hasAccessToken()) {
      return USER_LOGIN_PATH;
    }
    return DESKTOP_HOME_PATH;
  }

  const token = hasAccessToken();

  if ((to.path === '/login' || to.path === '/register') && token) {
    try {
      if (!authStore.user) {
        await authStore.loadProfile();
      }
      return USER_BEEHIVE_PATH;
    } catch (error) {
      if (isAuthRequiredError(error)) {
        authStore.logout();
      }
      return true;
    }
  }

  if (to.meta.requiresAuth && !token) {
    return to.path.startsWith('/admin') ? '/admin/login' : USER_LOGIN_PATH;
  }

  if (to.meta.requiresAuth && token && !authStore.user) {
    try {
      await authStore.loadProfile();
    } catch (error) {
      if (isAuthRequiredError(error)) {
        authStore.logout();
        return to.path.startsWith('/admin') ? '/admin/login' : USER_LOGIN_PATH;
      }
    }
  }

  return true;
});

export default router;
