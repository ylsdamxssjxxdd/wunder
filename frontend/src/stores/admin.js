import { defineStore } from 'pinia';

import {
  createAgent,
  createUser,
  fetchUserToolAccess,
  fetchSystemStatus,
  fetchWunderSettings,
  fetchWunderTools,
  listAdminAgents,
  listUsers,
  resetUserPassword,
  updateUserToolAccess,
  updateAgent,
  updateWunderSettings,
  updateUser
} from '@/api/admin';

export const useAdminStore = defineStore('admin', {
  state: () => ({
    users: [],
    agents: [],
    systemStatus: null,
    wunderSettings: null,
    wunderStatus: null,
    wunderTools: null,
    userToolAccess: {}
  }),
  actions: {
    async loadUsers(params = {}) {
      const { data } = await listUsers(params);
      this.users = data.data.items || [];
      return data.data;
    },
    async createUser(payload) {
      const { data } = await createUser(payload);
      await this.loadUsers();
      return data.data;
    },
    async updateUser(id, payload) {
      const { data } = await updateUser(id, payload);
      await this.loadUsers();
      return data.data;
    },
    async resetUserPassword(id, payload) {
      const { data } = await resetUserPassword(id, payload);
      return data.data;
    },
    async loadAgents() {
      const { data } = await listAdminAgents();
      this.agents = data.data || [];
      return this.agents;
    },
    async createAgent(payload) {
      const { data } = await createAgent(payload);
      await this.loadAgents();
      return data.data;
    },
    async updateAgent(id, payload) {
      const { data } = await updateAgent(id, payload);
      await this.loadAgents();
      return data.data;
    },
    async loadSystemStatus() {
      const { data } = await fetchSystemStatus();
      this.systemStatus = data.data;
      return data.data;
    },
    async loadWunderSettings() {
      const { data } = await fetchWunderSettings();
      this.wunderSettings = data.data.settings;
      this.wunderStatus = data.data.status;
      return data.data;
    },
    async updateWunderSettings(payload) {
      const { data } = await updateWunderSettings(payload);
      this.wunderSettings = data.data.settings;
      this.wunderStatus = data.data.status;
      return data.data;
    },
    async loadWunderTools() {
      const { data } = await fetchWunderTools();
      this.wunderTools = data.data;
      return data.data;
    },
    async loadUserToolAccess(userId) {
      const { data } = await fetchUserToolAccess(userId);
      this.userToolAccess = {
        ...this.userToolAccess,
        [userId]: data.data.items || []
      };
      return data.data.items || [];
    },
    async updateUserToolAccess(userId, payload) {
      const { data } = await updateUserToolAccess(userId, payload);
      await this.loadUserToolAccess(userId);
      return data.data;
    }
  }
});
