import { defineStore } from 'pinia';

import {
  createBeeroomGroup,
  deleteBeeroomGroup,
  downloadBeeroomHivePack,
  exportBeeroomHivePack,
  getBeeroomGroup,
  getBeeroomHivePackExportJob,
  getBeeroomHivePackImportJob,
  getBeeroomMission,
  importBeeroomHivePack,
  listBeeroomGroups,
  listBeeroomMissions,
  moveBeeroomAgents
} from '@/api/beeroom';
import type { QueryParams } from '@/api/types';
import {
  isStaleRealtimeUpdate,
  shouldApplyRealtimeStatusTransition
} from './beeroomRealtimeStatus';

export type BeeroomMember = {
  agent_id: string;
  name?: string;
  description?: string;
  status?: string;
  hive_id?: string;
  icon?: string;
  is_shared?: boolean;
  approval_mode?: string;
  tool_names?: string[];
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
  retry_count?: number | null;
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

export type BeeroomPackArtifact = {
  filename?: string;
  path?: string;
  size_bytes?: number;
};

export type BeeroomPackJob = {
  job_id: string;
  job_type?: string;
  status?: string;
  phase?: string;
  progress?: number;
  summary?: string;
  detail?: Record<string, unknown> | null;
  report?: Record<string, unknown> | null;
  artifact?: BeeroomPackArtifact | null;
  created_at?: number;
  updated_at?: number;
};

export type BeeroomPackImportOptions = {
  group_id?: string;
  create_hive_if_missing?: boolean;
  conflict_mode?: 'auto_rename_only' | 'update_replace';
};

export type BeeroomPackExportMode = 'full' | 'reference_only';

const asArray = <T>(value: unknown): T[] => (Array.isArray(value) ? (value as T[]) : []);

const normalizeGroupId = (value: unknown): string =>
  String(value || '').trim();

const normalizeMissionId = (value: unknown): string =>
  String(value || '').trim();

const buildParamsKey = (params: QueryParams = {}): string =>
  Object.entries(params)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}:${String(value ?? '')}`)
    .join('|');

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

const normalizePackStatus = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const isTerminalPackStatus = (value: unknown): boolean => {
  const normalized = normalizePackStatus(value);
  return (
    normalized === 'completed' ||
    normalized === 'failed' ||
    normalized === 'error' ||
    normalized === 'cancelled' ||
    normalized === 'canceled'
  );
};

const isTerminalPackJob = (job: BeeroomPackJob | null | undefined): boolean =>
  Boolean(job && isTerminalPackStatus(job.status));

const resolveRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

// Normalize backend payload into a stable front-end shape.
const normalizePackJob = (value: unknown): BeeroomPackJob | null => {
  const source = resolveRecord(value);
  if (!source) return null;
  const jobId = String(source.job_id || '').trim();
  if (!jobId) return null;
  return {
    job_id: jobId,
    job_type: String(source.job_type || '').trim() || undefined,
    status: String(source.status || '').trim() || undefined,
    phase: String(source.phase || '').trim() || undefined,
    progress: Number(source.progress ?? 0),
    summary: String(source.summary || '').trim() || undefined,
    detail: resolveRecord(source.detail),
    report: resolveRecord(source.report),
    artifact: resolveRecord(source.artifact) as BeeroomPackArtifact | null,
    created_at: Number(source.created_at || 0),
    updated_at: Number(source.updated_at || 0)
  };
};

const normalizePackMode = (value: unknown): BeeroomPackExportMode =>
  String(value || '').trim().toLowerCase() === 'reference_only' ? 'reference_only' : 'full';

const decodeMaybeUriComponent = (value: string): string => {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
};

const resolveHeaderValue = (headers: unknown, key: string): string => {
  const source = headers as any;
  if (!source) return '';
  if (typeof source.get === 'function') {
    return String(source.get(key) || source.get(key.toLowerCase()) || '').trim();
  }
  return String(source[key] || source[key.toLowerCase()] || '').trim();
};

const resolveFilenameFromDisposition = (headers: unknown, fallback: string): string => {
  const disposition = resolveHeaderValue(headers, 'content-disposition');
  if (!disposition) return fallback;
  const utf8Match = disposition.match(/filename\*=UTF-8''([^;]+)/i);
  if (utf8Match?.[1]) {
    const decoded = decodeMaybeUriComponent(utf8Match[1].replace(/['"]/g, '').trim());
    if (decoded) return decoded;
  }
  const basicMatch = disposition.match(/filename=\"?([^\";]+)\"?/i);
  const basic = String(basicMatch?.[1] || '').trim();
  return basic || fallback;
};

const normalizeScalar = (value: unknown): string => String(value ?? '').trim();

const stableMissionTaskFingerprint = (task: BeeroomMissionTask): string =>
  [
    normalizeScalar(task.task_id),
    normalizeScalar(task.agent_id),
    normalizeScalar(task.target_session_id),
    normalizeScalar(task.spawned_session_id),
    normalizeScalar(task.session_run_id),
    normalizeScalar(task.status),
    normalizeScalar(task.retry_count),
    normalizeScalar(task.priority),
    normalizeScalar(task.started_time),
    normalizeScalar(task.finished_time),
    normalizeScalar(task.elapsed_s),
    normalizeScalar(task.updated_time),
    normalizeScalar(task.result_summary),
    normalizeScalar(task.error)
  ].join('|');

const stableMissionFingerprint = (mission: BeeroomMission): string =>
  [
    normalizeScalar(mission.mission_id || mission.team_run_id),
    normalizeScalar(mission.team_run_id),
    normalizeScalar(mission.status),
    normalizeScalar(mission.completion_status),
    normalizeScalar(mission.updated_time),
    normalizeScalar(mission.started_time),
    normalizeScalar(mission.finished_time),
    normalizeScalar(mission.task_total),
    normalizeScalar(mission.task_success),
    normalizeScalar(mission.task_failed),
    normalizeScalar(mission.context_tokens_total),
    normalizeScalar(mission.context_tokens_peak),
    normalizeScalar(mission.model_round_total),
    normalizeScalar(mission.summary),
    normalizeScalar(mission.error),
    asArray<BeeroomMissionTask>(mission.tasks).map(stableMissionTaskFingerprint).join('~')
  ].join('|');

const stableMemberFingerprint = (member: BeeroomMember): string =>
  [
    normalizeScalar(member.agent_id),
    normalizeScalar(member.name),
    normalizeScalar(member.description),
    normalizeScalar(member.status),
    normalizeScalar(member.hive_id),
    normalizeScalar(member.idle),
    normalizeScalar(member.active_session_total),
    normalizeScalar(member.sandbox_container_id),
    asArray<string>(member.active_session_ids).map(normalizeScalar).join(','),
    asArray<string>(member.tool_names).map(normalizeScalar).join(',')
  ].join('|');

const stableGroupFingerprint = (group: BeeroomGroup | null | undefined): string => {
  if (!group) return '';
  const latestMission = group.latest_mission || null;
  const latestMissionFingerprint = latestMission ? stableMissionFingerprint(latestMission) : '';
  const membersFingerprint = asArray<BeeroomMember>(group.members)
    .map(stableMemberFingerprint)
    .join('~');
  return [
    normalizeScalar(group.group_id || group.hive_id),
    normalizeScalar(group.hive_id),
    normalizeScalar(group.name),
    normalizeScalar(group.description),
    normalizeScalar(group.status),
    normalizeScalar(group.is_default),
    normalizeScalar(group.created_time),
    normalizeScalar(group.updated_time),
    normalizeScalar(group.agent_total),
    normalizeScalar(group.active_agent_total),
    normalizeScalar(group.idle_agent_total),
    normalizeScalar(group.running_mission_total),
    normalizeScalar(group.mission_total),
    normalizeScalar(group.mother_agent_id),
    normalizeScalar(group.mother_agent_name),
    latestMissionFingerprint,
    membersFingerprint
  ].join('|');
};

const stableMembersFingerprint = (items: BeeroomMember[]): string =>
  items.map(stableMemberFingerprint).join('||');

const stableMissionsFingerprint = (items: BeeroomMission[]): string =>
  items.map(stableMissionFingerprint).join('||');

const normalizeRealtimeEventType = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const TEAM_REALTIME_EVENT_TYPES = new Set([
  'team_start',
  'team_task_dispatch',
  'team_task_update',
  'team_task_result',
  'team_merge',
  'team_finish',
  'team_error'
]);

const normalizeStatusText = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const ACTIVE_TASK_STATUSES = new Set(['queued', 'pending', 'running', 'awaiting_idle', 'resuming', 'merging']);
const TERMINAL_TASK_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled']);
const TERMINAL_MISSION_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled']);

const isActiveTaskStatus = (value: unknown): boolean =>
  ACTIVE_TASK_STATUSES.has(normalizeStatusText(value));

const isTerminalTaskStatus = (value: unknown): boolean =>
  TERMINAL_TASK_STATUSES.has(normalizeStatusText(value));

const isTerminalMissionStatus = (value: unknown): boolean =>
  TERMINAL_MISSION_STATUSES.has(normalizeStatusText(value));

const parseMaybeNumber = (value: unknown): number | undefined => {
  const numeric = Number(value);
  return Number.isFinite(numeric) ? numeric : undefined;
};

const resolveRealtimeTimestamp = (value: unknown): number => {
  const numeric = parseMaybeNumber(value);
  if (!numeric || numeric <= 0) {
    return Math.floor(Date.now() / 1000);
  }
  if (numeric > 1_000_000_000_000) {
    return Math.floor(numeric / 1000);
  }
  return Math.floor(numeric);
};

const cloneMissionTask = (task: BeeroomMissionTask): BeeroomMissionTask => ({ ...task });

const cloneMission = (mission: BeeroomMission): BeeroomMission => ({
  ...mission,
  tasks: asArray<BeeroomMissionTask>(mission.tasks).map(cloneMissionTask)
});

const createMissionFromRealtimeEvent = (
  groupId: string,
  missionId: string,
  payload: Record<string, unknown>,
  eventType: string,
  nowSec: number
): BeeroomMission => {
  const missionStatus = normalizeStatusText(payload.status) || (eventType === 'team_error' ? 'failed' : 'running');
  const completionStatus = normalizeStatusText(payload.completion_status) || missionStatus;
  return {
    team_run_id: missionId,
    mission_id: missionId,
    hive_id: groupId,
    strategy: String(payload.strategy || '').trim() || undefined,
    status: missionStatus || 'running',
    completion_status: completionStatus || missionStatus || 'running',
    task_total: parseMaybeNumber(payload.task_total) ?? 0,
    task_success: parseMaybeNumber(payload.task_success) ?? 0,
    task_failed: parseMaybeNumber(payload.task_failed) ?? 0,
    context_tokens_total: parseMaybeNumber(payload.context_tokens_total),
    context_tokens_peak: parseMaybeNumber(payload.context_tokens_peak),
    model_round_total: parseMaybeNumber(payload.model_round_total),
    started_time: parseMaybeNumber(payload.started_time) ?? nowSec,
    finished_time: parseMaybeNumber(payload.finished_time),
    elapsed_s: parseMaybeNumber(payload.elapsed_s),
    summary: String(payload.summary || payload.result_summary || '').trim() || undefined,
    error: String(payload.error || '').trim() || undefined,
    updated_time: nowSec,
    tasks: []
  };
};

let groupsRequestSerial = 0;
let groupsInFlight: Promise<BeeroomGroup[]> | null = null;
let groupsInFlightKey = '';
let detailRequestSerial = 0;
let detailInFlight: Promise<BeeroomGroup | null> | null = null;
let detailInFlightKey = '';

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
    error: '',
    packImportJob: null as BeeroomPackJob | null,
    packExportJob: null as BeeroomPackJob | null,
    packImportLoading: false,
    packExportLoading: false,
    packError: ''
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

    clearActiveData() {
      this.activeGroup = null;
      this.activeAgents = [];
      this.activeMissions = [];
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
        const merged = { ...this.groups[index], ...nextGroup };
        if (stableGroupFingerprint(this.groups[index]) === stableGroupFingerprint(merged)) {
          return;
        }
        this.groups.splice(index, 1, merged);
      } else {
        this.groups.unshift(nextGroup);
      }
    },

    hydrateActivePayload(payload: unknown) {
      const source = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      const group = (source.group || source) as BeeroomGroup | null;
      const agents = asArray<BeeroomMember>(source.agents);
      const missions = asArray<BeeroomMission>(source.missions);
      const nextGroup = group && normalizeGroupId(group.group_id || group.hive_id) ? group : null;
      const currentGroupFingerprint = stableGroupFingerprint(this.activeGroup);
      const nextGroupFingerprint = stableGroupFingerprint(nextGroup);
      if (currentGroupFingerprint !== nextGroupFingerprint) {
        this.activeGroup = nextGroup;
      }
      if (stableMembersFingerprint(this.activeAgents) !== stableMembersFingerprint(agents)) {
        this.activeAgents = agents;
      }
      if (stableMissionsFingerprint(this.activeMissions) !== stableMissionsFingerprint(missions)) {
        this.activeMissions = missions;
      }
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

    applyRealtimeEvent(groupId: unknown, eventType: unknown, payload: unknown) {
      const normalizedGroupId = normalizeGroupId(groupId || this.activeGroupId);
      const normalizedType = normalizeRealtimeEventType(eventType);
      if (!normalizedGroupId || !TEAM_REALTIME_EVENT_TYPES.has(normalizedType)) {
        return false;
      }

      const source = resolveRecord(payload) || {};
      const nowSec = resolveRealtimeTimestamp(
        source.updated_time ?? source.updatedAt ?? source.created_at ?? source.createdAt ?? source.time
      );
      const missionId = normalizeMissionId(
        source.team_run_id || source.teamRunId || source.mission_id || source.missionId
      );
      const taskId = String(source.task_id || source.taskId || '').trim();
      const agentId = String(source.agent_id || source.agentId || '').trim();
      const targetSessionId = String(source.target_session_id || source.targetSessionId || '').trim();
      const spawnedSessionId = String(
        source.spawned_session_id || source.spawnedSessionId || source.session_id || source.sessionId || ''
      ).trim();
      const sessionRunId = String(source.session_run_id || source.sessionRunId || '').trim();
      const eventStatus = normalizeStatusText(source.status);

      const missions = this.activeMissions.map(cloneMission);
      let missionChanged = false;
      let mission =
        (missionId
          ? missions.find((item) => normalizeMissionId(item.mission_id || item.team_run_id) === missionId)
          : null) || null;
      if (!mission && missionId) {
        mission = createMissionFromRealtimeEvent(normalizedGroupId, missionId, source, normalizedType, nowSec);
        missions.unshift(mission);
        missionChanged = true;
      }
      if (mission) {
        const missionUpdatedBefore = Number(mission.updated_time || 0);
        const staleMissionEvent = isStaleRealtimeUpdate(missionUpdatedBefore, nowSec);
        const missionTerminalBefore = isTerminalMissionStatus(
          mission.completion_status || mission.status || ''
        );
        mission.updated_time = Math.max(missionUpdatedBefore, nowSec);
        if (!mission.hive_id) {
          mission.hive_id = normalizedGroupId;
        }
        if (!staleMissionEvent && normalizedType === 'team_start' && !mission.started_time) {
          mission.started_time = nowSec;
        }
        if (
          shouldApplyRealtimeStatusTransition({
            currentStatus: mission.status,
            currentUpdatedTime: missionUpdatedBefore,
            incomingStatus: eventStatus,
            incomingUpdatedTime: nowSec,
            isTerminalStatus: isTerminalMissionStatus
          })
        ) {
          mission.status = eventStatus;
          if (
            normalizedType === 'team_finish' ||
            normalizedType === 'team_error' ||
            isTerminalMissionStatus(eventStatus)
          ) {
            mission.completion_status = eventStatus;
          } else if (!isTerminalMissionStatus(mission.completion_status || '')) {
            mission.completion_status = eventStatus;
          }
        }
        if (
          !staleMissionEvent &&
          normalizedType === 'team_error' &&
          !eventStatus &&
          !missionTerminalBefore
        ) {
          mission.status = 'failed';
          mission.completion_status = 'failed';
        }
        if (
          !staleMissionEvent &&
          normalizedType === 'team_finish' &&
          !eventStatus &&
          !missionTerminalBefore
        ) {
          mission.status = mission.task_failed && mission.task_failed > 0 ? 'failed' : 'completed';
          mission.completion_status = mission.status;
        }
        const errorText = String(source.error || '').trim();
        if (!staleMissionEvent) {
          const summaryText = String(source.summary || source.result_summary || '').trim();
          if (summaryText) {
            mission.summary = summaryText;
          }
          if (errorText) {
            mission.error = errorText;
          }
          const maybeTaskTotal = parseMaybeNumber(source.task_total);
          if (maybeTaskTotal !== undefined) {
            mission.task_total = Math.max(mission.task_total || 0, maybeTaskTotal);
          }
          const maybeTaskSuccess = parseMaybeNumber(source.task_success);
          if (maybeTaskSuccess !== undefined) {
            mission.task_success = maybeTaskSuccess;
          }
          const maybeTaskFailed = parseMaybeNumber(source.task_failed);
          if (maybeTaskFailed !== undefined) {
            mission.task_failed = maybeTaskFailed;
          }
          const maybeTokensTotal = parseMaybeNumber(source.context_tokens_total);
          if (maybeTokensTotal !== undefined) {
            mission.context_tokens_total = maybeTokensTotal;
          }
          const maybeTokensPeak = parseMaybeNumber(source.context_tokens_peak);
          if (maybeTokensPeak !== undefined) {
            mission.context_tokens_peak = maybeTokensPeak;
          }
          const maybeModelRoundTotal = parseMaybeNumber(source.model_round_total);
          if (maybeModelRoundTotal !== undefined) {
            mission.model_round_total = maybeModelRoundTotal;
          }
          const maybeElapsed = parseMaybeNumber(source.elapsed_s);
          if (maybeElapsed !== undefined) {
            mission.elapsed_s = maybeElapsed;
          }
        }
        if (taskId) {
          const tasks = asArray<BeeroomMissionTask>(mission.tasks).map(cloneMissionTask);
          let task = tasks.find((item) => String(item.task_id || '').trim() === taskId) || null;
          if (!task) {
            task = {
              task_id: taskId,
              agent_id: agentId,
              status: eventStatus || 'running',
              updated_time: nowSec
            };
            tasks.push(task);
          }
          const taskUpdatedBefore = Number(task.updated_time || 0);
          const staleTaskEvent = isStaleRealtimeUpdate(taskUpdatedBefore, nowSec);
          if (agentId && (!task.agent_id || !staleTaskEvent)) {
            task.agent_id = agentId;
          }
          // Keep session linkage reactive so canvas workflow previews can follow the latest worker run.
          if (targetSessionId && (!task.target_session_id || !staleTaskEvent)) {
            task.target_session_id = targetSessionId;
          }
          if (spawnedSessionId && (!task.spawned_session_id || !staleTaskEvent)) {
            task.spawned_session_id = spawnedSessionId;
          }
          if (sessionRunId && (!task.session_run_id || !staleTaskEvent)) {
            task.session_run_id = sessionRunId;
          }
          if (
            shouldApplyRealtimeStatusTransition({
              currentStatus: task.status,
              currentUpdatedTime: taskUpdatedBefore,
              incomingStatus: eventStatus,
              incomingUpdatedTime: nowSec,
              isTerminalStatus: isTerminalTaskStatus
            })
          ) {
            task.status = eventStatus;
          }
          if (!staleTaskEvent) {
            const maybeRetryCount = parseMaybeNumber(source.retry_count);
            if (maybeRetryCount !== undefined) {
              task.retry_count = maybeRetryCount;
            }
            const maybePriority = parseMaybeNumber(source.priority);
            if (maybePriority !== undefined) {
              task.priority = maybePriority;
            }
            const maybeTaskStartedTime = parseMaybeNumber(source.started_time);
            if (maybeTaskStartedTime !== undefined) {
              task.started_time = maybeTaskStartedTime;
            } else if (!task.started_time && normalizedType === 'team_task_update') {
              task.started_time = nowSec;
            }
            const maybeTaskFinishedTime = parseMaybeNumber(source.finished_time);
            if (maybeTaskFinishedTime !== undefined) {
              task.finished_time = maybeTaskFinishedTime;
            } else if (normalizedType === 'team_task_result' && isTerminalTaskStatus(task.status)) {
              task.finished_time = nowSec;
            }
            const maybeTaskElapsed = parseMaybeNumber(source.elapsed_s);
            if (maybeTaskElapsed !== undefined) {
              task.elapsed_s = maybeTaskElapsed;
            }
            const resultSummary = String(source.result_summary || '').trim();
            if (resultSummary) {
              task.result_summary = resultSummary;
            }
            if (errorText) {
              task.error = errorText;
            }
          }
          task.updated_time = Math.max(taskUpdatedBefore, nowSec);
          mission.tasks = tasks;
        }

        const tasks = asArray<BeeroomMissionTask>(mission.tasks);
        if (tasks.length) {
          mission.task_total = Math.max(mission.task_total || 0, tasks.length);
          const successTotal = tasks.filter((item) =>
            ['success', 'completed'].includes(normalizeStatusText(item.status))
          ).length;
          const failedTotal = tasks.filter((item) =>
            ['failed', 'error', 'timeout', 'cancelled'].includes(normalizeStatusText(item.status))
          ).length;
          mission.task_success = successTotal;
          mission.task_failed = failedTotal;
          const allTerminal = tasks.every((item) => isTerminalTaskStatus(item.status));
          if (allTerminal && !isTerminalMissionStatus(mission.completion_status || mission.status || '')) {
            mission.status = failedTotal > 0 ? 'failed' : 'completed';
            mission.completion_status = mission.status;
          }
        }
        if (isTerminalMissionStatus(mission.completion_status || mission.status || '')) {
          if (mission.completion_status) {
            mission.status = mission.completion_status;
          }
          mission.finished_time = mission.finished_time || nowSec;
        }
        missionChanged = true;
      }

      if (missionChanged) {
        missions.sort((left, right) => Number(right.updated_time || 0) - Number(left.updated_time || 0));
        if (stableMissionsFingerprint(this.activeMissions) !== stableMissionsFingerprint(missions)) {
          this.activeMissions = missions;
        }
      }

      // Keep agent idle/runtime hints reactive from mission task transitions.
      let nextAgents = this.activeAgents.map((item) => ({ ...item }));
      if (nextAgents.length) {
        const busyAgentIds = new Set<string>();
        missions.forEach((item) => {
          asArray<BeeroomMissionTask>(item.tasks).forEach((task) => {
            const currentTaskAgentId = String(task.agent_id || '').trim();
            if (!currentTaskAgentId || !isActiveTaskStatus(task.status)) return;
            busyAgentIds.add(currentTaskAgentId);
          });
        });
        nextAgents = nextAgents.map((item) => {
          const currentAgentId = String(item.agent_id || '').trim();
          if (!currentAgentId) return item;
          const busy = busyAgentIds.has(currentAgentId);
          return {
            ...item,
            idle: !busy,
            active_session_total: busy ? Math.max(Number(item.active_session_total || 0), 1) : 0
          };
        });
        if (stableMembersFingerprint(this.activeAgents) !== stableMembersFingerprint(nextAgents)) {
          this.activeAgents = nextAgents;
        }
      }

      if (this.activeGroup) {
        const activeGroupId = normalizeGroupId(this.activeGroup.group_id || this.activeGroup.hive_id);
        if (activeGroupId === normalizedGroupId) {
          const runningMissionTotal = missions.filter(
            (item) => !isTerminalMissionStatus(item.completion_status || item.status)
          ).length;
          const activeAgentTotal = nextAgents.filter((item) => item.idle === false).length;
          const nextGroup: BeeroomGroup = {
            ...this.activeGroup,
            group_id: activeGroupId,
            hive_id: this.activeGroup.hive_id || activeGroupId,
            running_mission_total: runningMissionTotal,
            mission_total: missions.length,
            latest_mission: missions[0] || null,
            updated_time: Math.max(Number(this.activeGroup.updated_time || 0), nowSec),
            active_agent_total: nextAgents.length ? activeAgentTotal : this.activeGroup.active_agent_total,
            idle_agent_total: nextAgents.length
              ? Math.max(0, nextAgents.length - activeAgentTotal)
              : this.activeGroup.idle_agent_total,
            members: nextAgents.length ? nextAgents.slice(0, 6) : this.activeGroup.members
          };
          if (stableGroupFingerprint(this.activeGroup) !== stableGroupFingerprint(nextGroup)) {
            this.activeGroup = nextGroup;
          }
          this.upsertGroup(nextGroup);
        }
      }

      if (agentId && eventStatus && this.activeAgents.length) {
        const index = this.activeAgents.findIndex(
          (item) => String(item.agent_id || '').trim() === agentId
        );
        if (index >= 0) {
          const member = this.activeAgents[index];
          const busy = isActiveTaskStatus(eventStatus);
          const updatedMember: BeeroomMember = {
            ...member,
            idle: !busy,
            active_session_total: busy ? Math.max(Number(member.active_session_total || 0), 1) : 0
          };
          if (stableMemberFingerprint(member) !== stableMemberFingerprint(updatedMember)) {
            this.activeAgents.splice(index, 1, updatedMember);
          }
        }
      }

      return missionChanged;
    },

    async loadGroups(params: QueryParams = {}) {
      const requestKey = buildParamsKey(params);
      if (groupsInFlight && groupsInFlightKey === requestKey) {
        return groupsInFlight;
      }

      this.loading = true;
      this.error = '';
      const requestId = ++groupsRequestSerial;
      const request = (async () => {
        try {
          const { data } = await listBeeroomGroups(params);
          const items = asArray<BeeroomGroup>(data?.data?.items).map((item) => ({
            ...item,
            group_id: normalizeGroupId(item.group_id || item.hive_id),
            hive_id: String(item.hive_id || item.group_id || '').trim()
          }));

          // Ignore stale responses when multiple panels trigger refresh together.
          if (requestId !== groupsRequestSerial) {
            return items;
          }

          this.groups = items;
          const nextActiveGroupId = this.activeGroupId
            ? items.find((item) => normalizeGroupId(item.group_id || item.hive_id) === this.activeGroupId)
              ? this.activeGroupId
              : normalizeGroupId(items[0]?.group_id || items[0]?.hive_id)
            : normalizeGroupId(items[0]?.group_id || items[0]?.hive_id);
          this.activeGroupId = nextActiveGroupId;

          if (!items.length) {
            this.clearActiveData();
          } else if (!nextActiveGroupId) {
            this.clearActiveData();
          }

          return items;
        } catch (error: any) {
          if (requestId === groupsRequestSerial) {
            this.error = String(
              error?.response?.data?.detail || error?.message || 'load beeroom failed'
            );
            if (Number(error?.response?.status || 0) === 401) {
              this.groups = [];
              this.activeGroupId = '';
              this.clearActiveData();
            }
          }
          throw error;
        } finally {
          if (groupsInFlight === request) {
            groupsInFlight = null;
            groupsInFlightKey = '';
          }
          if (requestId === groupsRequestSerial) {
            this.loading = false;
          }
        }
      })();

      groupsInFlight = request;
      groupsInFlightKey = requestKey;
      return request;
    },

    async loadActiveGroup(params: QueryParams & { silent?: boolean } = {}) {
      const groupId = normalizeGroupId(this.activeGroupId);
      if (!groupId) {
        this.clearActiveData();
        return null;
      }
      const requestParams = { ...params };
      const silent = requestParams.silent === true;
      delete (requestParams as Record<string, unknown>).silent;
      const requestKey = `${groupId}::${buildParamsKey(requestParams)}`;
      if (detailInFlight && detailInFlightKey === requestKey) {
        return detailInFlight;
      }

      if (silent) {
        this.refreshing = true;
      } else {
        this.detailLoading = true;
      }
      this.error = '';
      const requestId = ++detailRequestSerial;
      const request = (async () => {
        try {
          const { data } = await getBeeroomGroup(groupId, requestParams);
          if (requestId !== detailRequestSerial || groupId !== normalizeGroupId(this.activeGroupId)) {
            return this.activeGroup;
          }
          this.hydrateActivePayload(data?.data);
          return this.activeGroup;
        } catch (error: any) {
          if (requestId === detailRequestSerial) {
            this.error = String(
              error?.response?.data?.detail || error?.message || 'load beeroom detail failed'
            );
            const status = Number(error?.response?.status || 0);
            if (status === 401 || status === 404) {
              this.activeGroupId = '';
              this.clearActiveData();
            }
          }
          throw error;
        } finally {
          if (detailInFlight === request) {
            detailInFlight = null;
            detailInFlightKey = '';
          }
          if (requestId === detailRequestSerial) {
            this.detailLoading = false;
            this.refreshing = false;
          }
        }
      })();

      detailInFlight = request;
      detailInFlightKey = requestKey;
      return request;
    },

    async selectGroup(groupId: unknown, params: QueryParams & { silent?: boolean } = {}) {
      const normalized = normalizeGroupId(groupId);
      this.activeGroupId = normalized;
      if (!normalized) {
        this.clearActiveData();
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

    async deleteGroup(groupId: unknown) {
      const normalizedGroupId = normalizeGroupId(groupId || this.activeGroupId);
      if (!normalizedGroupId) {
        return 0;
      }
      const { data } = await deleteBeeroomGroup(normalizedGroupId);
      const deleted = Number(data?.data?.deleted || 0);
      await this.loadGroups();
      if (this.activeGroupId) {
        await this.loadActiveGroup({ silent: true }).catch(() => null);
      } else {
        this.clearActiveData();
      }
      return deleted;
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
    },

    clearPackJobs() {
      this.packImportJob = null;
      this.packExportJob = null;
    },

    clearPackError() {
      this.packError = '';
    },

    async pollImportJob(
      jobId: unknown,
      options: { intervalMs?: number; timeoutMs?: number } = {}
    ) {
      const normalizedJobId = String(jobId || '').trim();
      if (!normalizedJobId) {
        return null;
      }
      const timeoutMs = Math.max(1500, Number(options.timeoutMs || 120000));
      const intervalMs = Math.max(500, Number(options.intervalMs || 1200));
      const startedAt = Date.now();
      let latestJob = this.packImportJob;

      // Poll until terminal status or timeout.
      while (Date.now() - startedAt <= timeoutMs) {
        const { data } = await getBeeroomHivePackImportJob(normalizedJobId);
        latestJob = normalizePackJob(data?.data);
        if (latestJob) {
          this.packImportJob = latestJob;
        }
        if (isTerminalPackJob(latestJob)) {
          return latestJob;
        }
        await sleep(intervalMs);
      }
      return latestJob;
    },

    async importHivePack(file: Blob | File, options: BeeroomPackImportOptions = {}) {
      if (!(file instanceof Blob) || !file.size) {
        throw new Error('hivepack file is required');
      }
      this.packImportLoading = true;
      this.packError = '';
      try {
        const normalizedGroupId = normalizeGroupId(options.group_id);
        const normalizedOptions: Record<string, unknown> = {};
        if (normalizedGroupId) {
          normalizedOptions.group_id = normalizedGroupId;
        }
        if (typeof options.create_hive_if_missing === 'boolean') {
          normalizedOptions.create_hive_if_missing = options.create_hive_if_missing;
        }
        if (typeof options.conflict_mode === 'string' && options.conflict_mode.trim()) {
          normalizedOptions.conflict_mode = options.conflict_mode.trim().toLowerCase();
        }

        const { data } = await importBeeroomHivePack({
          file,
          options: Object.keys(normalizedOptions).length ? normalizedOptions : undefined,
          groupId: normalizedGroupId || undefined
        });
        const firstJob = normalizePackJob(data?.data);
        if (!firstJob) {
          throw new Error('invalid hivepack import job response');
        }
        this.packImportJob = firstJob;
        const finalJob = isTerminalPackJob(firstJob)
          ? firstJob
          : await this.pollImportJob(firstJob.job_id);
        const resolvedJob = finalJob || firstJob;

        // Keep beeroom list/detail in sync when import has finished.
        if (normalizePackStatus(resolvedJob.status) === 'completed') {
          const targetHiveId = normalizeGroupId(
            resolvedJob.report?.hive_id || normalizedGroupId || this.activeGroupId
          );
          await this.loadGroups().catch(() => null);
          if (targetHiveId) {
            await this.selectGroup(targetHiveId, { silent: true }).catch(() => null);
          } else if (this.activeGroupId) {
            await this.loadActiveGroup({ silent: true }).catch(() => null);
          }
        }

        return resolvedJob;
      } catch (error: any) {
        this.packError = String(
          error?.response?.data?.detail || error?.message || 'import hivepack failed'
        );
        throw error;
      } finally {
        this.packImportLoading = false;
      }
    },

    async pollExportJob(
      jobId: unknown,
      options: { intervalMs?: number; timeoutMs?: number } = {}
    ) {
      const normalizedJobId = String(jobId || '').trim();
      if (!normalizedJobId) {
        return null;
      }
      const timeoutMs = Math.max(1500, Number(options.timeoutMs || 120000));
      const intervalMs = Math.max(500, Number(options.intervalMs || 1200));
      const startedAt = Date.now();
      let latestJob = this.packExportJob;

      // Poll until terminal status or timeout.
      while (Date.now() - startedAt <= timeoutMs) {
        const { data } = await getBeeroomHivePackExportJob(normalizedJobId);
        latestJob = normalizePackJob(data?.data);
        if (latestJob) {
          this.packExportJob = latestJob;
        }
        if (isTerminalPackJob(latestJob)) {
          return latestJob;
        }
        await sleep(intervalMs);
      }
      return latestJob;
    },

    async exportHivePack(groupId: unknown, mode: BeeroomPackExportMode = 'full') {
      const normalizedGroupId = normalizeGroupId(groupId || this.activeGroupId);
      if (!normalizedGroupId) {
        throw new Error('group_id is required');
      }
      this.packExportLoading = true;
      this.packError = '';
      try {
        const { data } = await exportBeeroomHivePack({
          group_id: normalizedGroupId,
          mode: normalizePackMode(mode)
        });
        const firstJob = normalizePackJob(data?.data);
        if (!firstJob) {
          throw new Error('invalid hivepack export job response');
        }
        this.packExportJob = firstJob;
        const finalJob = isTerminalPackJob(firstJob)
          ? firstJob
          : await this.pollExportJob(firstJob.job_id);
        return finalJob || firstJob;
      } catch (error: any) {
        this.packError = String(
          error?.response?.data?.detail || error?.message || 'export hivepack failed'
        );
        throw error;
      } finally {
        this.packExportLoading = false;
      }
    },

    async downloadExportPack(jobId: unknown = '') {
      const normalizedJobId = String(jobId || this.packExportJob?.job_id || '').trim();
      if (!normalizedJobId) {
        throw new Error('job_id is required');
      }
      const response = await downloadBeeroomHivePack(normalizedJobId);
      const blob = response?.data instanceof Blob ? response.data : new Blob([response?.data]);
      const fallbackFilename =
        String(this.packExportJob?.artifact?.filename || '').trim() ||
        `hivepack-${normalizedJobId}.hivepack`;
      const filename = resolveFilenameFromDisposition(response?.headers, fallbackFilename);

      if (typeof window !== 'undefined') {
        const url = window.URL.createObjectURL(blob);
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = filename;
        document.body.appendChild(anchor);
        anchor.click();
        document.body.removeChild(anchor);
        window.URL.revokeObjectURL(url);
      }

      return { filename, size_bytes: blob.size };
    }
  }
});
