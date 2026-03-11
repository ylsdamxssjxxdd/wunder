import { defineStore } from 'pinia';

import { fetchMe, login, loginDemo, register } from '@/api/auth';
import {
  ensureDemoIdentity,
  ensureDemoProfile,
  getDemoToken,
  isDemoMode,
  saveDemoProfile,
  setDemoToken
} from '@/utils/demo';

let profileInFlight: Promise<any> | null = null;

const shouldUpdateUser = (currentUser: any, nextUser: any): boolean => {
  if (!currentUser && !nextUser) return false;
  if (!currentUser || !nextUser) return true;
  const currentId = String(currentUser.id || currentUser.user_id || currentUser.username || '').trim();
  const nextId = String(nextUser.id || nextUser.user_id || nextUser.username || '').trim();
  if (currentId !== nextId) return true;
  const currentVersion = String(
    currentUser.updated_at || currentUser.updated_time || currentUser.last_login_at || ''
  ).trim();
  const nextVersion = String(
    nextUser.updated_at || nextUser.updated_time || nextUser.last_login_at || ''
  ).trim();
  if (currentVersion && nextVersion) {
    return currentVersion !== nextVersion;
  }
  return JSON.stringify(currentUser) !== JSON.stringify(nextUser);
};

export const useAuthStore = defineStore('auth', {
  state: () => ({
    token: localStorage.getItem('access_token') || '',
    user: null,
    loading: false
  }),
  actions: {
    async login(payload) {
      this.loading = true;
      try {
        const { data } = await login(payload);
        const token = data.data.access_token;
        this.token = token;
        localStorage.setItem('access_token', token);
        this.user = data.data.user;
        return data.data;
      } finally {
        this.loading = false;
      }
    },
    async register(payload) {
      this.loading = true;
      try {
        const { data } = await register(payload);
        const token = data.data.access_token;
        this.token = token;
        localStorage.setItem('access_token', token);
        this.user = data.data.user;
        return data.data;
      } finally {
        this.loading = false;
      }
    },
    async loadProfile() {
      if (profileInFlight) {
        return profileInFlight;
      }
      profileInFlight = (async () => {
        if (isDemoMode()) {
          // 演示模式优先使用免登录接口获取完整权限
          const cachedToken = getDemoToken();
          const isCurrentDemo = Boolean(
            this.user && String(this.user.username || '').startsWith('demo_')
          );
          if (isCurrentDemo && cachedToken) {
            return this.user;
          }
          try {
            const identity = ensureDemoIdentity();
            const { data } = await loginDemo({ demo_id: identity.demo_id });
            const token = data.data.access_token;
            if (token) {
              setDemoToken(token);
            }
            const nextUser = data.data.user;
            if (shouldUpdateUser(this.user, nextUser)) {
              this.user = nextUser;
            }
            saveDemoProfile(nextUser);
            return nextUser;
          } catch (error) {
            const profile = ensureDemoProfile();
            if (shouldUpdateUser(this.user, profile)) {
              this.user = profile;
            }
            return profile;
          }
        }
        if (!this.token) {
          if (this.user) {
            this.user = null;
          }
          return null;
        }
        const { data } = await fetchMe();
        const nextUser = data.data;
        if (shouldUpdateUser(this.user, nextUser)) {
          this.user = nextUser;
        }
        return nextUser;
      })();
      try {
        return await profileInFlight;
      } finally {
        profileInFlight = null;
      }
    },
    logout() {
      this.token = '';
      this.user = null;
      localStorage.removeItem('access_token');
    }
  }
});
