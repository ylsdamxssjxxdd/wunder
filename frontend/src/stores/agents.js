import { defineStore } from 'pinia';

import {
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  getAgent as getAgentApi,
  listAgents,
  listSharedAgents,
  updateAgent as updateAgentApi
} from '@/api/agents';

export const useAgentStore = defineStore('agents', {
  state: () => ({
    agents: [],
    sharedAgents: [],
    agentMap: {},
    loading: false
  }),
  actions: {
    hydrateMap(agents, sharedAgents) {
      const map = {};
      [...(agents || []), ...(sharedAgents || [])].forEach((agent) => {
        if (agent?.id) {
          map[agent.id] = agent;
        }
      });
      this.agentMap = map;
    },
    async loadAgents() {
      this.loading = true;
      try {
        const [ownedRes, sharedRes] = await Promise.all([listAgents(), listSharedAgents()]);
        const ownedItems = ownedRes?.data?.data?.items || [];
        const sharedItems = sharedRes?.data?.data?.items || [];
        this.agents = ownedItems;
        this.sharedAgents = sharedItems;
        this.hydrateMap(ownedItems, sharedItems);
        return { owned: ownedItems, shared: sharedItems };
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
      this.sharedAgents = this.sharedAgents.filter((item) => item.id !== key);
      const map = { ...this.agentMap };
      delete map[key];
      this.agentMap = map;
      return data?.data;
    }
  }
});
