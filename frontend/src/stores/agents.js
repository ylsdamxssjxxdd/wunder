import { defineStore } from 'pinia';

import {
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  getAgent as getAgentApi,
  listAgents,
  listSharedAgents,
  updateAgent as updateAgentApi
} from '@/api/agents';

const normalizeHiveId = (value) => String(value || '').trim();

export const useAgentStore = defineStore('agents', {
  state: () => ({
    agents: [],
    sharedAgents: [],
    agentMap: {},
    loading: false,
    activeHiveId: ''
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

    async loadAgents(options = {}) {
      const all = options.all === true;
      const hiveId = all
        ? ''
        : normalizeHiveId(options.hiveId !== undefined ? options.hiveId : this.activeHiveId);
      if (!all) {
        this.activeHiveId = hiveId;
      }
      const ownedParams = hiveId ? { hive_id: hiveId } : {};
      this.loading = true;
      try {
        const [ownedRes, sharedRes] = await Promise.all([
          listAgents(ownedParams),
          listSharedAgents()
        ]);
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

    async createAgent(payload, options = {}) {
      const { data } = await createAgentApi(payload);
      const agent = data?.data;
      await this.loadAgents({ hiveId: options.hiveId ?? payload?.hive_id ?? this.activeHiveId });
      return agent;
    },

    async updateAgent(id, payload, options = {}) {
      const { data } = await updateAgentApi(id, payload);
      const agent = data?.data;
      await this.loadAgents({ hiveId: options.hiveId ?? this.activeHiveId });
      return agent;
    },

    async deleteAgent(id, options = {}) {
      const key = String(id || '').trim();
      if (!key) return null;
      const { data } = await deleteAgentApi(key);
      await this.loadAgents({ hiveId: options.hiveId ?? this.activeHiveId });
      return data?.data;
    }
  }
});
