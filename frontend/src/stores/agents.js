import { defineStore } from 'pinia';

import {
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  getAgent as getAgentApi,
  listAgents,
  updateAgent as updateAgentApi
} from '@/api/agents';

export const useAgentStore = defineStore('agents', {
  state: () => ({
    agents: [],
    agentMap: {},
    loading: false
  }),
  actions: {
    hydrateMap(agents) {
      const map = {};
      (agents || []).forEach((agent) => {
        if (agent?.id) {
          map[agent.id] = agent;
        }
      });
      this.agentMap = map;
    },
    async loadAgents() {
      this.loading = true;
      try {
        const { data } = await listAgents();
        const items = data?.data?.items || [];
        this.agents = items;
        this.hydrateMap(items);
        return items;
      } finally {
        this.loading = false;
      }
    },
    async getAgent(id, options = {}) {
      const key = String(id || '').trim();
      if (!key) return null;
      if (!options.force && this.agentMap[key]) {
        return this.agentMap[key];
      }
      const { data } = await getAgentApi(key);
      const agent = data?.data || null;
      if (agent) {
        this.agentMap = { ...this.agentMap, [key]: agent };
        if (!this.agents.find((item) => item.id === key)) {
          this.agents = [agent, ...this.agents];
        }
      }
      return agent;
    },
    async createAgent(payload) {
      const { data } = await createAgentApi(payload);
      const agent = data?.data;
      await this.loadAgents();
      return agent;
    },
    async updateAgent(id, payload) {
      const { data } = await updateAgentApi(id, payload);
      const agent = data?.data;
      await this.loadAgents();
      return agent;
    },
    async deleteAgent(id) {
      const key = String(id || '').trim();
      if (!key) return null;
      const { data } = await deleteAgentApi(key);
      this.agents = this.agents.filter((item) => item.id !== key);
      const map = { ...this.agentMap };
      delete map[key];
      this.agentMap = map;
      return data?.data;
    }
  }
});
