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
    agents: [] as Record<string, unknown>[],
    sharedAgents: [] as Record<string, unknown>[],
    agentMap: {} as Record<string, Record<string, unknown> | null>,
    loading: false
  }),
  actions: {
    hydrateMap(agents, sharedAgents) {
      const map: Record<string, Record<string, unknown> | null> = {};
      if (this.agentMap?.__default__) {
        map.__default__ = this.agentMap.__default__;
      }
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
        const [ownedResult, sharedResult] = await Promise.allSettled([listAgents(), listSharedAgents()]);
        if (ownedResult.status !== 'fulfilled') {
          throw ownedResult.reason;
        }
        if (sharedResult.status !== 'fulfilled') {
          console.warn('[agents] load shared agents failed, fallback to empty list', sharedResult.reason);
        }
        const ownedItems = ownedResult.value?.data?.data?.items || [];
        const sharedItems =
          sharedResult.status === 'fulfilled' ? (sharedResult.value?.data?.data?.items || []) : [];
        this.agents = ownedItems;
        this.sharedAgents = sharedItems;
        this.hydrateMap(ownedItems, sharedItems);
        return { owned: ownedItems, shared: sharedItems };
      } finally {
        this.loading = false;
      }
    },

    async getAgent(id, options: { force?: boolean } = {}) {
      const key = String(id || '').trim();
      if (!key) return null;
      if (!options.force && Object.prototype.hasOwnProperty.call(this.agentMap, key)) {
        return this.agentMap[key] || null;
      }
      try {
        const { data } = await getAgentApi(key);
        const agent = data?.data || null;
        if (agent) {
          this.agentMap = { ...this.agentMap, [key]: agent };
        }
        return agent;
      } catch (error) {
        const status = (error as { response?: { status?: number } })?.response?.status;
        if (status === 404) {
          this.agentMap = { ...this.agentMap, [key]: null };
          return null;
        }
        throw error;
      }
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
      await this.loadAgents();
      return data?.data;
    }
  }
});

