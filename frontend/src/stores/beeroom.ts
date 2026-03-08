import { defineStore } from 'pinia';

import {
  createBeeroomGroup,
  getBeeroomGroup,
  getBeeroomMission,
  listBeeroomGroups,
  listBeeroomMissions,
  moveBeeroomAgents
} from '@/api/beeroom';
import type { QueryParams } from '@/api/types';

export type BeeroomMember = {
  agent_id: string;
  name?: string;
  description?: string;
  status?: string;
  hive_id?: string;
  icon?: string;
  is_shared?: boolean;
  approval_mode?: string;
  sandbox_container_id?: number;
  active_session_total?: number;
  active_session_ids?: string[];
  idle?: boolean;
};

export type BeeroomMissionTask = {
  task_id: string;
  agent_id: string;
  target_session_id?: string | null;
  spawned_session_id?: string | null;
  session_run_id?: string | null;
  status?: string;
  priority?: number;
  started_time?: number | null;
  finished_time?: number | null;
  elapsed_s?: number | null;
  result_summary?: string | null;
  error?: string | null;
  updated_time?: number;
};

export type BeeroomMission = {
  team_run_id: string;
  mission_id: string;
  hive_id: string;
  parent_session_id?: string;
  entry_agent_id?: string | null;
  mother_agent_id?: string | null;
  strategy?: string;
  status?: string;
  completion_status?: string;
  task_total?: number;
  task_success?: number;
  task_failed?: number;
  context_tokens_total?: number;
  context_tokens_peak?: number;
  model_round_total?: number;
  started_time?: number | null;
  finished_time?: number | null;
  elapsed_s?: number | null;
  summary?: string | null;
  error?: string | null;
  updated_time?: number;
  all_tasks_terminal?: boolean;
  all_agents_idle?: boolean;
  active_agent_ids?: string[];
  idle_agent_ids?: string[];
  tasks?: BeeroomMissionTask[];
};

export type BeeroomGroup = {
  group_id: string;
  hive_id?: string;
  name: string;
  description?: string;
  status?: string;
  is_default?: boolean;
  created_time?: number;
  updated_time?: number;
  agent_total?: number;
  active_agent_total?: number;
  idle_agent_total?: number;
  running_mission_total?: number;
  mission_total?: number;
  mother_agent_id?: string | null;
  mother_agent_name?: string | null;
  members?: BeeroomMember[];
  latest_mission?: BeeroomMission | null;
};

const asArray = <T>(value: unknown): T[] => (Array.isArray(value) ? (value as T[]) : []);

const normalizeGroupId = (value: unknown): string =>
  String(value || '').trim();

const normalizeMissionId = (value: unknown): string =>
  String(value || '').trim();

export const useBeeroomStore = defineStore('beeroom', {
  state: () => ({
    groups: [] as BeeroomGroup[],
    activeGroupId: '',
    activeGroup: null as BeeroomGroup | null,
    activeAgents: [] as BeeroomMember[],
    activeMissions: [] as BeeroomMission[],
    loading: false,
    detailLoading: false,
    refreshing: false,
    error: ''
  }),
  getters: {
    activeGroupSummary(state): BeeroomGroup | null {
      const activeGroupId = normalizeGroupId(state.activeGroupId);
      if (!activeGroupId) return null;
      return (
        state.groups.find((item) => normalizeGroupId(item.group_id || item.hive_id) === activeGroupId) ||
        state.activeGroup ||
        null
      );
    }
  },
  actions: {
    resetState() {
      this.$reset();
    },

    setActiveGroup(groupId: unknown) {
      this.activeGroupId = normalizeGroupId(groupId);
    },

    upsertGroup(group: BeeroomGroup | null | undefined) {
      if (!group) return;
      const groupId = normalizeGroupId(group.group_id || group.hive_id);
      if (!groupId) return;
      const nextGroup = { ...group, group_id: groupId, hive_id: group.hive_id || groupId };
      const index = this.groups.findIndex(
        (item) => normalizeGroupId(item.group_id || item.hive_id) === groupId
      );
      if (index >= 0) {
        this.groups.splice(index, 1, { ...this.groups[index], ...nextGroup });
      } else {
        this.groups.unshift(nextGroup);
      }
    },

    hydrateActivePayload(payload: unknown) {
      const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      const group = (source.group || source) as BeeroomGroup | null;
      const agents = asArray<BeeroomMember>(source.agents);
      const missions = asArray<BeeroomMission>(source.missions);
      this.activeGroup = group && normalizeGroupId(group.group_id || group.hive_id) ? group : null;
      this.activeAgents = agents;
      this.activeMissions = missions;
      if (this.activeGroup) {
        const groupId = normalizeGroupId(this.activeGroup.group_id || this.activeGroup.hive_id);
        this.activeGroupId = groupId;
        this.upsertGroup({
          ...this.activeGroup,
          members: this.activeGroup.members || agents.slice(0, 6),
          latest_mission: this.activeGroup.latest_mission || missions[0] || null,
          agent_total: this.activeGroup.agent_total ?? agents.length,
          mission_total: this.activeGroup.mission_total ?? missions.length
        });
      }
    },

    async loadGroups(params: QueryParams = {}) {
      this.loading = true;
      this.error = '';
      try {
        const { data } = await listBeeroomGroups(params);
        const items = asArray<BeeroomGroup>(data?.data?.items).map((item) => ({
          ...item,
          group_id: normalizeGroupId(item.group_id || item.hive_id),
          hive_id: String(item.hive_id || item.group_id || '').trim()
        }));
        this.groups = items;
        if (this.activeGroupId) {
          const exists = items.some(
            (item) => normalizeGroupId(item.group_id || item.hive_id) === this.activeGroupId
          );
          if (!exists) {
            this.activeGroupId = normalizeGroupId(items[0]?.group_id || items[0]?.hive_id);
          }
        } else {
          this.activeGroupId = normalizeGroupId(items[0]?.group_id || items[0]?.hive_id);
        }
        return items;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'load beeroom failed');
        throw error;
      } finally {
        this.loading = false;
      }
    },

    async loadActiveGroup(params: QueryParams & { silent?: boolean } = {}) {
      const groupId = normalizeGroupId(this.activeGroupId);
      if (!groupId) {
        this.activeGroup = null;
        this.activeAgents = [];
        this.activeMissions = [];
        return null;
      }
      const silent = params.silent === true;
      if (silent) {
        this.refreshing = true;
      } else {
        this.detailLoading = true;
      }
      this.error = '';
      try {
        const requestParams = { ...params };
        delete (requestParams as Record<string, unknown>).silent;
        const { data } = await getBeeroomGroup(groupId, requestParams);
        this.hydrateActivePayload(data?.data);
        return this.activeGroup;
      } catch (error: any) {
        this.error = String(error?.response?.data?.detail || error?.message || 'load beeroom detail failed');
        throw error;
      } finally {
        this.detailLoading = false;
        this.refreshing = false;
      }
    },

    async selectGroup(groupId: unknown, params: QueryParams & { silent?: boolean } = {}) {
      const normalized = normalizeGroupId(groupId);
      this.activeGroupId = normalized;
      if (!normalized) {
        this.activeGroup = null;
        this.activeAgents = [];
        this.activeMissions = [];
        return null;
      }
      return this.loadActiveGroup(params);
    },

    async createGroup(payload: Record<string, unknown>) {
      const { data } = await createBeeroomGroup(payload);
      const group = (data?.data || null) as BeeroomGroup | null;
      if (group) {
        this.upsertGroup(group);
        this.activeGroupId = normalizeGroupId(group.group_id || group.hive_id);
        await this.loadActiveGroup();
      }
      return group;
    },

    async moveAgents(groupId: unknown, agentIds: string[]) {
      const normalizedGroupId = normalizeGroupId(groupId);
      const normalizedAgentIds = agentIds
        .map((item) => String(item || '').trim())
        .filter((item) => item.length > 0);
      if (!normalizedGroupId || !normalizedAgentIds.length) {
        return 0;
      }
      const { data } = await moveBeeroomAgents(normalizedGroupId, { agent_ids: normalizedAgentIds });
      await Promise.all([this.loadGroups(), this.selectGroup(normalizedGroupId)]);
      return Number(data?.data?.moved || 0);
    },

    async loadMissions(groupId: unknown, params: QueryParams = {}) {
      const normalizedGroupId = normalizeGroupId(groupId || this.activeGroupId);
      if (!normalizedGroupId) {
        this.activeMissions = [];
        return [];
      }
      const { data } = await listBeeroomMissions(normalizedGroupId, params);
      const items = asArray<BeeroomMission>(data?.data?.items);
      if (normalizedGroupId === this.activeGroupId) {
        this.activeMissions = items;
      }
      return items;
    },

    async loadMission(groupId: unknown, missionId: unknown) {
      const normalizedGroupId = normalizeGroupId(groupId || this.activeGroupId);
      const normalizedMissionId = normalizeMissionId(missionId);
      if (!normalizedGroupId || !normalizedMissionId) {
        return null;
      }
      const { data } = await getBeeroomMission(normalizedGroupId, normalizedMissionId);
      return (data?.data || null) as BeeroomMission | null;
    }
  }
});
