import { defineStore } from 'pinia';

import {
  createHive as createHiveApi,
  getHiveSummary,
  listHives,
  moveHiveAgents,
  updateHive as updateHiveApi
} from '@/api/hives';

const DEFAULT_HIVE_ID = 'default';
const ACTIVE_HIVE_STORAGE_KEY = 'wunder.activeHiveId';

const normalizeHiveId = (value) => {
  const cleaned = String(value || '').trim().toLowerCase();
  return cleaned || DEFAULT_HIVE_ID;
};

const pickInitialHive = (items) => {
  if (!Array.isArray(items) || !items.length) {
    return DEFAULT_HIVE_ID;
  }
  const explicitDefault = items.find((item) => item?.is_default);
  if (explicitDefault?.hive_id) {
    return normalizeHiveId(explicitDefault.hive_id);
  }
  const active = items.find((item) => String(item?.status || '').toLowerCase() === 'active');
  if (active?.hive_id) {
    return normalizeHiveId(active.hive_id);
  }
  return normalizeHiveId(items[0]?.hive_id);
};

export const useBeehiveStore = defineStore('beehive', {
  state: () => ({
    hives: [],
    hiveMap: {},
    activeHiveId: DEFAULT_HIVE_ID,
    summary: null,
    loading: false,
    summaryLoading: false
  }),
  getters: {
    activeHive(state) {
      return state.hiveMap[state.activeHiveId] || null;
    }
  },
  actions: {
    hydrateMap(items) {
      const map = {};
      (items || []).forEach((hive) => {
        const hiveId = normalizeHiveId(hive?.hive_id);
        if (hiveId) {
          map[hiveId] = { ...hive, hive_id: hiveId };
        }
      });
      this.hiveMap = map;
    },

    setActiveHive(hiveId) {
      this.activeHiveId = normalizeHiveId(hiveId);
      this.summary = null;
      try {
        localStorage.setItem(ACTIVE_HIVE_STORAGE_KEY, this.activeHiveId);
      } catch (error) {
        // ignore storage failures
      }
    },

    async loadHives(options = {}) {
      const includeArchived = options.includeArchived === true;
      this.loading = true;
      try {
        const { data } = await listHives({ include_archived: includeArchived });
        const items = data?.data?.items || [];
        this.hives = items;
        this.hydrateMap(items);
        const storedHiveId = (() => {
          try {
            return localStorage.getItem(ACTIVE_HIVE_STORAGE_KEY);
          } catch (error) {
            return '';
          }
        })();
        if (!options.keepActive) {
          const fallbackActive = pickInitialHive(items);
          this.activeHiveId = this.hiveMap[normalizeHiveId(storedHiveId)]
            ? normalizeHiveId(storedHiveId)
            : fallbackActive;
        } else {
          const normalized = normalizeHiveId(this.activeHiveId);
          if (!this.hiveMap[normalized]) {
            this.activeHiveId = pickInitialHive(items);
          }
        }
        try {
          localStorage.setItem(ACTIVE_HIVE_STORAGE_KEY, this.activeHiveId);
        } catch (error) {
          // ignore storage failures
        }
        return items;
      } finally {
        this.loading = false;
      }
    },

    async createHive(payload) {
      const { data } = await createHiveApi(payload);
      const hive = data?.data || null;
      await this.loadHives({ keepActive: true });
      if (hive?.hive_id) {
        this.setActiveHive(hive.hive_id);
      }
      return hive;
    },

    async updateHive(hiveId, payload) {
      const { data } = await updateHiveApi(hiveId, payload);
      const hive = data?.data || null;
      await this.loadHives({ keepActive: true });
      if (hive?.hive_id && normalizeHiveId(hiveId) === normalizeHiveId(this.activeHiveId)) {
        this.activeHiveId = normalizeHiveId(hive.hive_id);
      }
      return hive;
    },

    async moveAgentsToHive(hiveId, agentIds) {
      const ids = Array.isArray(agentIds) ? agentIds.filter((id) => String(id || '').trim()) : [];
      if (!ids.length) {
        return { moved: 0, hive_id: normalizeHiveId(hiveId) };
      }
      const { data } = await moveHiveAgents(hiveId, { agent_ids: ids });
      return data?.data || { moved: 0, hive_id: normalizeHiveId(hiveId) };
    },

    async loadSummary(hiveId, options = {}) {
      const resolved = normalizeHiveId(hiveId || this.activeHiveId);
      const lookbackMinutes = options.lookbackMinutes;
      this.summaryLoading = true;
      try {
        const params = {};
        if (Number.isFinite(lookbackMinutes)) {
          params.lookback_minutes = Math.max(5, Math.min(1440, Math.floor(lookbackMinutes)));
        }
        const { data } = await getHiveSummary(resolved, params);
        const summary = data?.data || null;
        if (normalizeHiveId(this.activeHiveId) === resolved) {
          this.summary = summary;
        }
        return summary;
      } finally {
        this.summaryLoading = false;
      }
    }
  }
});

export const normalizeHiveIdForUi = normalizeHiveId;
