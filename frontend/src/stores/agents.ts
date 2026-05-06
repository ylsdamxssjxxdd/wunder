import { defineStore } from 'pinia';

import {
  createAgent as createAgentApi,
  deleteAgent as deleteAgentApi,
  getAgent as getAgentApi,
  listAgents,
  listSharedAgents,
  updateAgent as updateAgentApi
} from '@/api/agents';

const inflightAgentRequests = new Map<string, Promise<Record<string, unknown> | null>>();

export const useAgentStore = defineStore('agents', {
  state: () => ({
    agents: [] as Record<string, unknown>[],
    sharedAgents: [] as Record<string, unknown>[],
    agentMap: {} as Record<string, Record<string, unknown> | null>,
    loading: false
  }),
  actions: {
    resolveAgentId(item: unknown): string {
      if (!item || typeof item !== 'object') return '';
      return String((item as Record<string, unknown>)?.id || '').trim();
    },

    isSameAgentIdSet(previous: unknown[], incoming: unknown[]): boolean {
      if (previous.length !== incoming.length) return false;
      const previousSet = new Set<string>();
      for (const item of previous) {
        const id = this.resolveAgentId(item);
        if (!id) continue;
        previousSet.add(id);
      }
      const incomingSet = new Set<string>();
      for (const item of incoming) {
        const id = this.resolveAgentId(item);
        if (!id) continue;
        incomingSet.add(id);
      }
      if (previousSet.size !== incomingSet.size) return false;
      for (const id of incomingSet) {
        if (!previousSet.has(id)) return false;
      }
      return true;
    },

    stabilizeAgentOrder(previous: unknown[], incoming: unknown[]): Record<string, unknown>[] {
      const next = Array.isArray(incoming) ? (incoming as Record<string, unknown>[]) : [];
      const prev = Array.isArray(previous) ? previous : [];
      if (!prev.length || !next.length) return next;
      // Preserve visual order when only metadata changed; avoid middle-pane jumps after save.
      if (!this.isSameAgentIdSet(prev, next)) {
        return next;
      }
      const latestById = new Map<string, Record<string, unknown>>();
      next.forEach((item) => {
        const id = this.resolveAgentId(item);
        if (!id) return;
        latestById.set(id, item);
      });
      const ordered: Record<string, unknown>[] = [];
      prev.forEach((item) => {
        const id = this.resolveAgentId(item);
        if (!id) return;
        const latest = latestById.get(id);
        if (latest) {
          ordered.push(latest);
        }
      });
      return ordered.length === next.length ? ordered : next;
    },

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
        const ownedItemsRaw = ownedResult.value?.data?.data?.items || [];
        const sharedItemsRaw =
          sharedResult.status === 'fulfilled' ? (sharedResult.value?.data?.data?.items || []) : [];
        const ownedItems = this.stabilizeAgentOrder(this.agents, ownedItemsRaw);
        const sharedItems = this.stabilizeAgentOrder(this.sharedAgents, sharedItemsRaw);
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
      const inflightRequest = inflightAgentRequests.get(key);
      if (inflightRequest) {
        return inflightRequest;
      }
      const request = (async () => {
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
      } finally {
        inflightAgentRequests.delete(key);
      }
      })();
      inflightAgentRequests.set(key, request);
      return request;
    },

    async createAgent(payload) {
      const { data } = await createAgentApi(payload);
      const agent = data?.data;
      await this.loadAgents();
      return agent;
    },

    async updateAgent(id, payload) {
      const key = String(id || '').trim();
      const { data } = await updateAgentApi(id, payload);
      const agent = data?.data;
      if (key && agent) {
        this.agentMap = { ...this.agentMap, [key]: agent };
      }
      await this.loadAgents();
      if (key && agent) {
        this.agentMap = { ...this.agentMap, [key]: agent };
      }
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

