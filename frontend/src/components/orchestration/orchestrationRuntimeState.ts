import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { createSession, renameSession as renameChatSession } from '@/api/chat';
import { setDefaultSession } from '@/api/agents';
import {
  createWunderWorkspaceDir,
  listWunderWorkspace,
  fetchWunderWorkspaceContent
} from '@/api/workspace';
import { listRecentBeeroomAgentOutputs } from '@/components/beeroom/beeroomAgentOutputPreview';
import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import { useI18n } from '@/i18n';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';
import { useChatStore } from '@/stores/chat';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

export type OrchestrationRound = {
  id: string;
  index: number;
  situation: string;
  userMessage: string;
  createdAt: number;
  missionIds: string[];
};

export type OrchestrationArtifactEntry = {
  name: string;
  path: string;
  type: 'file' | 'dir';
  size: number;
  updatedTime: string;
  updatedAtMs: number;
  preview: string;
};

export type OrchestrationArtifactCard = {
  agentId: string;
  agentName: string;
  path: string;
  entries: OrchestrationArtifactEntry[];
  loading: boolean;
  error: string;
};

type OrchestrationMemberThread = {
  agentId: string;
  sessionId: string;
};

type OrchestrationCreatedThread = OrchestrationMemberThread & {
  agentName: string;
  title: string;
};

type PersistedRuntime = {
  version: 3;
  groupId: string;
  runId: string;
  createdAt: number;
  motherAgentId: string;
  motherSessionId: string;
  currentSituation: string;
  plannedSituations: Record<string, string>;
  rounds: OrchestrationRound[];
  activeRoundId: string;
  memberThreads: OrchestrationMemberThread[];
  motherPrimerInjected: boolean;
};

type WorkspaceEntryLike = {
  name?: unknown;
  path?: unknown;
  type?: unknown;
  size?: unknown;
  updated_time?: unknown;
};

const ORCHESTRATION_STORAGE_PREFIX = 'wunder:orchestration-runtime';
const ORCHESTRATION_RUNTIME_VERSION = 3;
const ORCHESTRATION_ARTIFACT_PREVIEW_MAX_BYTES = 4096;
const ORCHESTRATION_ARTIFACT_CARD_LIMIT = 6;

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeRoundIndexKey = (value: unknown) => {
  const parsed = Number.parseInt(String(value ?? '').trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? String(parsed) : '';
};

const normalizePlannedSituations = (
  value: unknown,
  fallbackRounds: OrchestrationRound[] = []
): Record<string, string> => {
  const next: Record<string, string> = {};
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    Object.entries(value as Record<string, unknown>).forEach(([key, raw]) => {
      const roundKey = normalizeRoundIndexKey(key);
      if (!roundKey) return;
      const text = String(raw || '').trim();
      if (!text) return;
      next[roundKey] = text;
    });
  }
  fallbackRounds.forEach((round) => {
    const roundKey = normalizeRoundIndexKey(round.index);
    const text = String(round.situation || '').trim();
    if (!roundKey || !text || next[roundKey]) return;
    next[roundKey] = text;
  });
  return next;
};

const resolveSituationByRoundIndex = (plannedSituations: Record<string, string>, roundIndex: number) =>
  String(plannedSituations[normalizeRoundIndexKey(roundIndex)] || '').trim();

const applyPlannedSituationsToRounds = (
  rounds: OrchestrationRound[],
  plannedSituations: Record<string, string>
) =>
  rounds.map((round) => {
    if (normalizeText(round.userMessage)) {
      return round;
    }
    return {
      ...round,
      situation: resolveSituationByRoundIndex(plannedSituations, round.index)
    };
  });

const normalizeMsTime = (value: unknown): number => {
  if (typeof value === 'number') {
    if (!Number.isFinite(value) || value <= 0) return 0;
    return value < 1_000_000_000_000 ? value * 1000 : value;
  }
  const text = normalizeText(value);
  if (!text) return 0;
  if (/^\d+(\.\d+)?$/.test(text)) {
    const numeric = Number(text);
    if (!Number.isFinite(numeric) || numeric <= 0) return 0;
    return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
  }
  const parsed = new Date(text).getTime();
  return Number.isNaN(parsed) ? 0 : parsed;
};

const buildScopeKey = (groupId: unknown) => normalizeText(groupId) || 'standby';

const buildStorageKey = (groupId: unknown) => `${ORCHESTRATION_STORAGE_PREFIX}:${buildScopeKey(groupId)}`;

const buildRuntimeScopeKey = (runId: string) => `runtime:orchestration:${normalizeText(runId)}`;

const buildClearScopeKey = (runId: string) => `chat:orchestration:${normalizeText(runId)}`;

const buildRunId = () =>
  `orch_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;

const buildRoundId = (index: number) => `round_${String(index).padStart(4, '0')}`;

const buildRoundDirName = (index: number) => `round_${String(index).padStart(4, '0')}`;

const buildAgentArtifactPath = (runId: string, roundIndex: number, agentId: string) =>
  ['orchestration', runId, buildRoundDirName(roundIndex), normalizeText(agentId)]
    .filter(Boolean)
    .join('/');

const toApiAgentId = (agentId: string) => (agentId === DEFAULT_AGENT_KEY ? '' : agentId);

const resolveWorkspaceAgentId = (agentId: string) => {
  const normalized = normalizeText(agentId);
  return normalized === DEFAULT_AGENT_KEY ? '' : normalized;
};

const resolveWorkspaceContainerId = (member: BeeroomMember | null | undefined) => {
  const parsed = Number.parseInt(String(member?.sandbox_container_id ?? 1), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 1;
};

const normalizeRound = (value: unknown): OrchestrationRound | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const index = Math.max(1, Number.parseInt(String(record.index ?? ''), 10) || 0);
  const id = normalizeText(record.id) || buildRoundId(index);
  return {
    id,
    index,
    situation: String(record.situation || ''),
    userMessage: String(record.userMessage || ''),
    createdAt: Number(record.createdAt || Date.now()),
    missionIds: Array.isArray(record.missionIds)
      ? record.missionIds.map((item) => normalizeText(item)).filter(Boolean)
      : []
  };
};

const normalizePersistedRuntime = (value: unknown, groupId: unknown): PersistedRuntime | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const normalizedGroupId = buildScopeKey(groupId);
  const storedGroupId = buildScopeKey(record.groupId);
  if (storedGroupId !== normalizedGroupId) return null;
  const runId = normalizeText(record.runId);
  const motherSessionId = normalizeText(record.motherSessionId);
  const motherAgentId = normalizeText(record.motherAgentId);
  if (!runId || !motherSessionId || !motherAgentId) return null;
  const rounds = Array.isArray(record.rounds)
    ? record.rounds.map(normalizeRound).filter((item): item is OrchestrationRound => Boolean(item))
    : [];
  const memberThreads = Array.isArray(record.memberThreads)
    ? record.memberThreads
        .map((item) => {
          if (!item || typeof item !== 'object' || Array.isArray(item)) return null;
          const thread = item as Record<string, unknown>;
          const agentId = normalizeText(thread.agentId);
          const sessionId = normalizeText(thread.sessionId);
          if (!agentId || !sessionId) return null;
          return { agentId, sessionId };
        })
        .filter((item): item is OrchestrationMemberThread => Boolean(item))
    : [];
  return {
    version: ORCHESTRATION_RUNTIME_VERSION,
    groupId: normalizedGroupId,
    runId,
    createdAt: Number(record.createdAt || Date.now()),
    motherAgentId,
    motherSessionId,
    currentSituation: String(record.currentSituation || ''),
    plannedSituations: normalizePlannedSituations(record.plannedSituations, rounds),
    rounds,
    activeRoundId:
      normalizeText(record.activeRoundId) || (rounds.length ? rounds[rounds.length - 1].id : ''),
    memberThreads,
    motherPrimerInjected: record.motherPrimerInjected === true
  };
};

const readPersistedRuntime = (groupId: unknown): PersistedRuntime | null => {
  if (typeof window === 'undefined') return null;
  try {
    const raw = window.sessionStorage.getItem(buildStorageKey(groupId));
    if (!raw) return null;
    return normalizePersistedRuntime(JSON.parse(raw), groupId);
  } catch {
    return null;
  }
};

const writePersistedRuntime = (groupId: unknown, state: PersistedRuntime | null) => {
  if (typeof window === 'undefined') return;
  const key = buildStorageKey(groupId);
  try {
    if (!state) {
      window.sessionStorage.removeItem(key);
      return;
    }
    window.sessionStorage.setItem(key, JSON.stringify(state));
  } catch {
    // Ignore persistence failures.
  }
};

const createFreshMainSession = async (agentId: string, title = '') => {
  const apiAgentId = agentId === DEFAULT_AGENT_KEY ? '' : agentId;
  const created = await createSession(agentId === DEFAULT_AGENT_KEY ? {} : { agent_id: agentId });
  const createdSession =
    created?.data?.data && typeof created.data.data === 'object' && !Array.isArray(created.data.data)
      ? (created.data.data as Record<string, unknown>)
      : null;
  const createdId = normalizeText(createdSession?.id);
  if (!createdId) {
    throw new Error('orchestration_main_session_missing');
  }
  await setDefaultSession(apiAgentId || DEFAULT_AGENT_KEY, { session_id: createdId });
  const nextTitle = normalizeText(title);
  if (nextTitle) {
    await renameChatSession(createdId, { title: nextTitle }).catch(() => null);
  }
  return createdId;
};

const shouldPreviewFile = (name: string) => {
  const extension = name.split('.').pop()?.toLowerCase() || '';
  return ['md', 'txt', 'json', 'yaml', 'yml', 'toml', 'csv', 'log', 'html', 'xml'].includes(extension);
};

const extractPreviewText = (value: unknown) => {
  const text = String(value || '').trim();
  if (!text) return '';
  return text.length > 180 ? `${text.slice(0, 177)}...` : text;
};

export const useOrchestrationRuntimeState = (options: {
  group: Ref<BeeroomGroup | null>;
  agents: Ref<BeeroomMember[]>;
  missions: Ref<BeeroomMission[]>;
  displayChatMessages: Ref<MissionChatMessage[]>;
}) => {
  const { t } = useI18n();
  const chatStore = useChatStore();

  const runtimeState = ref<PersistedRuntime | null>(null);
  const initializing = ref(false);
  const initError = ref('');
  const artifactLoading = ref(false);
  const artifactError = ref('');
  const artifactCards = ref<OrchestrationArtifactCard[]>([]);
  let artifactReloadTimer: number | null = null;

  const groupId = computed(() => buildScopeKey(options.group.value?.group_id || options.group.value?.hive_id));
  const motherAgentId = computed(() => normalizeText(options.group.value?.mother_agent_id));

  const visibleWorkers = computed(() => {
    const currentMotherAgentId = motherAgentId.value;
    return (Array.isArray(options.agents.value) ? options.agents.value : []).filter((item) => {
      const agentId = normalizeText(item?.agent_id);
      return agentId && agentId !== currentMotherAgentId;
    });
  });

  const orderedAgents = computed(() => {
    const motherId = motherAgentId.value;
    const members = Array.isArray(options.agents.value) ? options.agents.value : [];
    const result: BeeroomMember[] = [];
    if (motherId) {
      const mother = members.find((item) => normalizeText(item?.agent_id) === motherId);
      if (mother) {
        result.push(mother);
      }
    }
    return [...result, ...visibleWorkers.value];
  });

  const activeRound = computed(() => {
    const rounds = runtimeState.value?.rounds || [];
    const activeRoundId = normalizeText(runtimeState.value?.activeRoundId);
    return rounds.find((item) => item.id === activeRoundId) || rounds[rounds.length - 1] || null;
  });

  const latestRound = computed(() => {
    const rounds = runtimeState.value?.rounds || [];
    return rounds[rounds.length - 1] || null;
  });

  const isReady = computed(() => Boolean(runtimeState.value?.runId));
  const runtimeScopeKey = computed(() =>
    runtimeState.value?.runId ? buildRuntimeScopeKey(runtimeState.value.runId) : ''
  );
  const clearScopeKey = computed(() =>
    runtimeState.value?.runId ? buildClearScopeKey(runtimeState.value.runId) : ''
  );
  const activeRoundChatMessages = computed(() => {
    const current = runtimeState.value;
    const round = activeRound.value;
    const source = Array.isArray(options.displayChatMessages.value) ? options.displayChatMessages.value : [];
    if (!current || !round) {
      return source;
    }
    const roundIndex = current.rounds.findIndex((item) => item.id === round.id);
    const nextRound = roundIndex >= 0 ? current.rounds[roundIndex + 1] || null : null;
    const roundStart = Number(round.createdAt || 0);
    const nextRoundStart = Number(nextRound?.createdAt || 0);
    return source.filter((message) => {
      const timeMs = normalizeMsTime(message?.time);
      if (!timeMs) {
        return round.id === current.activeRoundId;
      }
      if (timeMs < roundStart) {
        return false;
      }
      if (nextRoundStart > 0 && timeMs >= nextRoundStart) {
        return false;
      }
      return true;
    });
  });

  const rememberRuntime = () => {
    writePersistedRuntime(groupId.value, runtimeState.value);
  };

  const setRuntime = (value: PersistedRuntime | null) => {
    runtimeState.value = value;
    rememberRuntime();
  };

  const ensureRoundArtifactDirs = async (state: PersistedRuntime, round: OrchestrationRound) => {
    const agents = visibleWorkers.value;
    await Promise.all(
      agents.map((member) => {
        const agentId = normalizeText(member?.agent_id);
        if (!agentId) return Promise.resolve();
        const containerId = resolveWorkspaceContainerId(member);
        return createWunderWorkspaceDir({
          agent_id: resolveWorkspaceAgentId(agentId),
          container_id: containerId,
          path: buildAgentArtifactPath(state.runId, round.index, agentId)
        }).catch(() => null);
      })
    );
  };

  const bindMemberThreadsAsMain = async (state: PersistedRuntime) => {
    await Promise.all(
      (Array.isArray(state.memberThreads) ? state.memberThreads : []).map((item) => {
        const agentId = normalizeText(item.agentId);
        const sessionId = normalizeText(item.sessionId);
        if (!agentId || !sessionId) return Promise.resolve();
        return setDefaultSession(toApiAgentId(agentId) || DEFAULT_AGENT_KEY, { session_id: sessionId }).catch(() => null);
      })
    );
  };

  const initializeRun = async () => {
    const currentGroupId = groupId.value;
    const currentMotherAgentId = motherAgentId.value;
    if (!currentGroupId || !currentMotherAgentId) {
      throw new Error('orchestration_missing_mother');
    }
    initializing.value = true;
    initError.value = '';
    try {
      const createdThreads = await Promise.all(
        orderedAgents.value.map(async (member) => {
          const agentId = normalizeText(member?.agent_id);
          if (!agentId) return null;
          const agentName = normalizeText(member?.name) || agentId;
          const title = `\u7f16\u6392+${agentName}`;
          const sessionId = await createFreshMainSession(agentId, title);
          return { agentId, agentName, sessionId, title } satisfies OrchestrationCreatedThread;
        })
      );
      const runId = buildRunId();
      const firstRound: OrchestrationRound = {
        id: buildRoundId(1),
        index: 1,
        situation: '',
        userMessage: '',
        createdAt: Date.now(),
        missionIds: []
      };
      const nextState: PersistedRuntime = {
        version: ORCHESTRATION_RUNTIME_VERSION,
        groupId: currentGroupId,
        runId,
        createdAt: Date.now(),
        motherAgentId: currentMotherAgentId,
        motherSessionId:
          createdThreads.find((item) => item?.agentId === currentMotherAgentId)?.sessionId || '',
        currentSituation: '',
        plannedSituations: {},
        rounds: [firstRound],
        activeRoundId: firstRound.id,
        memberThreads: createdThreads.filter((item): item is OrchestrationCreatedThread => Boolean(item)),
        motherPrimerInjected: false
      };
      await bindMemberThreadsAsMain(nextState);
      await ensureRoundArtifactDirs(nextState, firstRound);
      setRuntime(nextState);
      createdThreads.forEach((item) => {
        if (!item?.agentId || !item?.sessionId) return;
        chatStore.syncSessionSummary(
          {
            id: item.sessionId,
            agent_id: item.agentId,
            is_main: true,
            title: item.title
          },
          {
            agentId: item.agentId,
            remember: true
          }
        );
      });
      return nextState;
    } finally {
      initializing.value = false;
    }
  };

  const ensureRuntime = async () => {
    if (runtimeState.value?.runId) return runtimeState.value;
    const persisted = readPersistedRuntime(groupId.value);
    if (
      persisted &&
      persisted.motherAgentId === motherAgentId.value &&
      buildScopeKey(persisted.groupId) === groupId.value
    ) {
      runtimeState.value = persisted;
      await bindMemberThreadsAsMain(persisted);
      return persisted;
    }
    return initializeRun();
  };

  const createRound = async (situation: string, userMessage = '') => {
    const current = await ensureRuntime();
    if (!current) return null;
    const nextIndex = Math.max(1, ...(current.rounds || []).map((item) => item.index)) + 1;
    const resolvedSituation = String(situation || '').trim() || resolveSituationByRoundIndex(current.plannedSituations, nextIndex);
    const round: OrchestrationRound = {
      id: buildRoundId(nextIndex),
      index: nextIndex,
      situation: resolvedSituation,
      userMessage,
      createdAt: Date.now(),
      missionIds: []
    };
    const nextState: PersistedRuntime = {
      ...current,
      currentSituation: resolvedSituation,
      rounds: [...current.rounds, round],
      activeRoundId: round.id
    };
    await ensureRoundArtifactDirs(nextState, round);
    setRuntime(nextState);
    return round;
  };

  const commitUserRound = async (payload: { situation?: string; userMessage: string; targetRoundId?: string }) => {
    const current = await ensureRuntime();
    if (!current) return null;
    const targetRoundId = normalizeText(payload.targetRoundId);
    const normalizedMessage = String(payload.userMessage || '').trim();
    const currentRound =
      current.rounds.find((item) => item.id === targetRoundId) ||
      current.rounds.find((item) => item.id === current.activeRoundId) ||
      current.rounds[current.rounds.length - 1];
    const normalizedSituation =
      String(payload.situation || '').trim() ||
      resolveSituationByRoundIndex(current.plannedSituations, Number(currentRound?.index || 0));
    if (!currentRound || normalizeText(currentRound.userMessage)) {
      return createRound(normalizedSituation, normalizedMessage);
    }
    const nextRounds = current.rounds.map((item) =>
      item.id === currentRound.id
        ? {
            ...item,
            situation: normalizedSituation,
            userMessage: normalizedMessage || item.userMessage
          }
        : item
    );
    setRuntime({
      ...current,
      currentSituation: normalizedSituation,
      rounds: nextRounds
    });
    return nextRounds.find((item) => item.id === currentRound.id) || null;
  };

  const markMotherPrimerInjected = () => {
    const current = runtimeState.value;
    if (!current || current.motherPrimerInjected) return;
    setRuntime({
      ...current,
      motherPrimerInjected: true
    });
  };

  const updateSituation = (value: string) => {
    const current = runtimeState.value;
    if (!current) return;
    const round = activeRound.value;
    if (!round) return;
    const roundKey = normalizeRoundIndexKey(round.index);
    if (!roundKey) return;
    const normalizedValue = String(value || '').trim();
    const plannedSituations = { ...(current.plannedSituations || {}) };
    if (normalizedValue) {
      plannedSituations[roundKey] = normalizedValue;
    } else {
      delete plannedSituations[roundKey];
    }
    const nextRounds = current.rounds.map((item) =>
      item.id === current.activeRoundId
        ? { ...item, situation: normalizedValue }
        : !normalizeText(item.userMessage) && item.index === round.index
          ? { ...item, situation: normalizedValue }
          : item
    );
    setRuntime({
      ...current,
      currentSituation: normalizedValue,
      plannedSituations,
      rounds: nextRounds
    });
  };

  const updatePlannedSituations = (entries: Record<string, string>) => {
    const current = runtimeState.value;
    if (!current) return;
    const plannedSituations = normalizePlannedSituations(entries);
    const nextRounds = applyPlannedSituationsToRounds(current.rounds, plannedSituations);
    const activeRoundId = normalizeText(current.activeRoundId);
    const currentActiveRound = nextRounds.find((item) => item.id === activeRoundId) || nextRounds[nextRounds.length - 1] || null;
    setRuntime({
      ...current,
      currentSituation: String(currentActiveRound?.situation || '').trim(),
      plannedSituations,
      rounds: nextRounds
    });
  };

  const updateRoundMissionIds = () => {
    const current = runtimeState.value;
    const motherSessionId = normalizeText(current?.motherSessionId);
    if (!current || !motherSessionId) return;
    const missions = (Array.isArray(options.missions.value) ? options.missions.value : [])
      .filter((mission) => normalizeText(mission?.parent_session_id) === motherSessionId)
      .map((mission) => ({
        missionId: normalizeText(mission?.mission_id || mission?.team_run_id),
        time: normalizeMsTime(mission?.started_time ?? mission?.updated_time ?? mission?.finished_time)
      }))
      .filter((item) => Boolean(item.missionId));
    const nextRounds = current.rounds.map((round, index, rounds) => {
      const roundStart = Number(round.createdAt || 0);
      const nextRound = rounds[index + 1] || null;
      const nextRoundStart = Number(nextRound?.createdAt || 0);
      const missionIds = missions
        .filter((mission) => {
          if (!mission.time) {
            return round.id === current.activeRoundId;
          }
          if (mission.time < roundStart) {
            return false;
          }
          if (nextRoundStart > 0 && mission.time >= nextRoundStart) {
            return false;
          }
          return true;
        })
        .map((mission) => mission.missionId);
      return {
        ...round,
        missionIds
      };
    });
    setRuntime({
      ...current,
      rounds: nextRounds
    });
  };

  const selectRound = (roundId: string) => {
    const current = runtimeState.value;
    if (!current) return;
    const resolvedRoundId = normalizeText(roundId);
    if (!resolvedRoundId || resolvedRoundId === current.activeRoundId) return;
    if (!current.rounds.some((item) => item.id === resolvedRoundId)) return;
    setRuntime({
      ...current,
      activeRoundId: resolvedRoundId
    });
  };

  const resolveWorkerThreadSessionId = (agentId: string) =>
    runtimeState.value?.memberThreads.find((item) => item.agentId === normalizeText(agentId))?.sessionId || '';

  const reloadArtifacts = async () => {
    const current = runtimeState.value;
    const round = activeRound.value;
    if (!current || !round) {
      artifactCards.value = [];
      return;
    }
    artifactLoading.value = true;
    artifactError.value = '';
    try {
      const cards = await Promise.all(
        visibleWorkers.value.map(async (member) => {
          const agentId = normalizeText(member?.agent_id);
          const agentName = normalizeText(member?.name) || agentId;
          const containerId = resolveWorkspaceContainerId(member);
          const path = buildAgentArtifactPath(current.runId, round.index, agentId);
          try {
            await createWunderWorkspaceDir({
              agent_id: resolveWorkspaceAgentId(agentId),
              container_id: containerId,
              path
            }).catch(() => null);
            const response = await listWunderWorkspace({
              agent_id: resolveWorkspaceAgentId(agentId),
              container_id: containerId,
              path
            });
            const entries = Array.isArray(response?.data?.entries) ? response.data.entries : [];
            const trimmedEntries = await Promise.all(
              entries
                .map(async (entry: WorkspaceEntryLike) => {
                  const name = normalizeText(entry?.name);
                  const entryPath = normalizeText(entry?.path);
                  const type = normalizeText(entry?.type) === 'dir' ? 'dir' : 'file';
                  if (!name || !entryPath) return null;
                  let preview = '';
                  if (type === 'file' && shouldPreviewFile(name)) {
                    try {
                      const content = await fetchWunderWorkspaceContent({
                        agent_id: resolveWorkspaceAgentId(agentId),
                        container_id: containerId,
                        path: entryPath,
                        include_content: true,
                        max_bytes: ORCHESTRATION_ARTIFACT_PREVIEW_MAX_BYTES
                      });
                      preview = extractPreviewText(content?.data?.content);
                    } catch {
                      preview = '';
                    }
                  }
                  return {
                    name,
                    path: entryPath,
                    type,
                    size: Number(entry?.size || 0),
                    updatedTime: String(entry?.updated_time || ''),
                    updatedAtMs: normalizeMsTime(entry?.updated_time),
                    preview
                  } satisfies OrchestrationArtifactEntry;
                })
                .slice(0, ORCHESTRATION_ARTIFACT_CARD_LIMIT)
            );
            return {
              agentId,
              agentName,
              path,
              entries: trimmedEntries.filter((item): item is OrchestrationArtifactEntry => Boolean(item)),
              loading: false,
              error: ''
            } satisfies OrchestrationArtifactCard;
          } catch (error) {
            return {
              agentId,
              agentName,
              path,
              entries: [],
              loading: false,
              error: String((error as { message?: unknown })?.message || '')
            } satisfies OrchestrationArtifactCard;
          }
        })
      );
      artifactCards.value = cards;
    } catch (error) {
      artifactError.value = String((error as { message?: unknown })?.message || '');
    } finally {
      artifactLoading.value = false;
    }
  };

  const clearArtifactReloadTimer = () => {
    if (artifactReloadTimer === null || typeof window === 'undefined') return;
    window.clearTimeout(artifactReloadTimer);
    artifactReloadTimer = null;
  };

  const scheduleArtifactReload = (delayMs = 240) => {
    if (!runtimeState.value?.runId || !activeRound.value?.id) {
      artifactCards.value = [];
      return;
    }
    if (typeof window === 'undefined') {
      void reloadArtifacts();
      return;
    }
    clearArtifactReloadTimer();
    artifactReloadTimer = window.setTimeout(() => {
      artifactReloadTimer = null;
      void reloadArtifacts();
    }, Math.max(0, Math.floor(delayMs)));
  };

  const resolveWorkerOutputs = (agentId: string) =>
    listRecentBeeroomAgentOutputs(activeRoundChatMessages.value, {
      agentId,
      limit: 3
    });

  watch(
    groupId,
    (value) => {
      const persisted = readPersistedRuntime(value);
      runtimeState.value =
        persisted && persisted.motherAgentId === motherAgentId.value ? persisted : null;
      artifactCards.value = [];
    },
    { immediate: true }
  );

  watch(
    () => [
      runtimeState.value?.runId || '',
      activeRound.value?.id || '',
      visibleWorkers.value.map((item) => normalizeText(item?.agent_id)).join('|')
    ],
    () => {
      if (!runtimeState.value?.runId || !activeRound.value?.id) {
        artifactCards.value = [];
        return;
      }
      clearArtifactReloadTimer();
      void reloadArtifacts();
    },
    { immediate: true }
  );

  watch(
    () => [
      activeRound.value?.id || '',
      activeRound.value?.missionIds.join('|') || '',
      activeRoundChatMessages.value.length,
      activeRoundChatMessages.value[activeRoundChatMessages.value.length - 1]?.key || '',
      activeRoundChatMessages.value[activeRoundChatMessages.value.length - 1]?.time || 0
    ].join('|'),
    (signature, previousSignature) => {
      if (!signature || signature === previousSignature) return;
      scheduleArtifactReload();
    }
  );

  watch(
    () =>
      (Array.isArray(options.missions.value) ? options.missions.value : [])
        .map((item) => `${normalizeText(item?.mission_id || item?.team_run_id)}:${normalizeText(item?.parent_session_id)}`)
        .join('|'),
    () => {
      updateRoundMissionIds();
    },
    { immediate: true }
  );

  onBeforeUnmount(() => {
    clearArtifactReloadTimer();
  });

  return {
    runtimeState,
    runtimeScopeKey,
    clearScopeKey,
    activeRound,
    latestRound,
    activeRoundChatMessages,
    visibleWorkers,
    artifactCards,
    artifactLoading,
    artifactError,
    initializing,
    initError,
    isReady,
    ensureRuntime,
    initializeRun,
    createRound,
    commitUserRound,
    markMotherPrimerInjected,
    updateSituation,
    updatePlannedSituations,
    selectRound,
    reloadArtifacts,
    resolveWorkerOutputs,
    resolveWorkerThreadSessionId
  };
};
