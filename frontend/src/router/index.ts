import { createRouter, createWebHistory } from 'vue-router';

import UserLayout from '@/layouts/UserLayout.vue';
import AdminLayout from '@/layouts/AdminLayout.vue';
import LoginView from '@/views/LoginView.vue';
import RegisterView from '@/views/RegisterView.vue';
import ChatView from '@/views/ChatView.vue';
import PortalView from '@/views/PortalView.vue';
import ExternalAppView from '@/views/ExternalAppView.vue';
import WorkspaceView from '@/views/WorkspaceView.vue';
import SettingsView from '@/views/SettingsView.vue';
import ProfileView from '@/views/ProfileView.vue';
import ToolManagerView from '@/views/ToolManagerView.vue';
import CronManagerView from '@/views/CronManagerView.vue';
import ChannelManagerView from '@/views/ChannelManagerView.vue';
import AdminLoginView from '@/views/AdminLoginView.vue';
import AdminUsersView from '@/views/AdminUsersView.vue';
import AdminAgentsView from '@/views/AdminAgentsView.vue';
import AdminSystemView from '@/views/AdminSystemView.vue';
import { disableDemoMode, enableDemoMode } from '@/utils/demo';
import { useAuthStore } from '@/stores/auth';

const USER_LOGIN_PATH = '/login';
const USER_BEEHIVE_PATH = '/app/home';

const hasAccessToken = () => Boolean(localStorage.getItem('access_token'));

const isAuthRequiredError = (error) => {
  const status = Number(error?.response?.status || 0);
  if (status === 401) {
    return true;
  }
  const payload = error?.response?.data;
  const errorCode = String(payload?.error?.code || payload?.code || payload?.message || '')
    .trim()
    .toLowerCase();
  return errorCode === 'auth_required' || errorCode === 'error.auth_required';
};

const routes = [
  {
    path: '/',
    redirect: () => (hasAccessToken() ? USER_BEEHIVE_PATH : USER_LOGIN_PATH)
  },
  {
    path: '/home',
    redirect: USER_BEEHIVE_PATH
  },
  {
    path: '/portal',
    redirect: '/home'
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
    path: '/app',
    component: UserLayout,
    meta: { requiresAuth: true },
    redirect: USER_BEEHIVE_PATH,
    children: [
      { path: 'home', name: 'home', component: PortalView },
      { path: 'external/:linkId', name: 'external-app', component: ExternalAppView },
      { path: 'tools', name: 'tools', component: ToolManagerView },
      { path: 'cron', name: 'cron', component: CronManagerView },
      { path: 'channels', name: 'channels', component: ChannelManagerView },
      { path: 'chat', name: 'chat', component: ChatView },
      { path: 'workspace', name: 'workspace', component: WorkspaceView },
      { path: 'settings', name: 'settings', component: SettingsView },
      { path: 'profile', name: 'profile', component: ProfileView }
    ]
  },
  {
    path: '/demo',
    component: UserLayout,
    meta: { demo: true },
    redirect: '/demo/chat',
    children: [
      { path: 'home', name: 'demo-home', component: PortalView, meta: { demo: true } },
      { path: 'external/:linkId', name: 'demo-external-app', component: ExternalAppView, meta: { demo: true } },
      { path: 'tools', name: 'demo-tools', component: ToolManagerView, meta: { demo: true } },
      { path: 'cron', name: 'demo-cron', component: CronManagerView, meta: { demo: true } },
      { path: 'channels', name: 'demo-channels', component: ChannelManagerView, meta: { demo: true } },
      { path: 'chat', name: 'demo-chat', component: ChatView, meta: { demo: true } },
      { path: 'workspace', name: 'demo-workspace', component: WorkspaceView, meta: { demo: true } },
      { path: 'settings', name: 'demo-settings', component: SettingsView, meta: { demo: true } },
      { path: 'profile', name: 'demo-profile', component: ProfileView, meta: { demo: true } }
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
  // Enable demo mode only for /demo routes.
  if (to.path.startsWith('/demo')) {
    enableDemoMode();
    const authStore = useAuthStore();
    await authStore.loadProfile();
  } else {
    disableDemoMode();
  }

  const token = hasAccessToken();
  const authStore = useAuthStore();

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

