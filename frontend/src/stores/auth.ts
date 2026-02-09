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
          this.user = data.data.user;
          saveDemoProfile(data.data.user);
          return data.data.user;
        } catch (error) {
          const profile = ensureDemoProfile();
          this.user = profile;
          return profile;
        }
      }
      if (!this.token) return null;
      const { data } = await fetchMe();
      this.user = data.data;
      return data.data;
    },
    logout() {
      this.token = '';
      this.user = null;
      localStorage.removeItem('access_token');
    }
  }
});
