import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import {
  cancelBeeroomOrchestrationRound,
  createBeeroomOrchestrationState,
  exitBeeroomOrchestrationState,
  finalizeBeeroomOrchestrationRound,
  getBeeroomOrchestrationState,
  listBeeroomOrchestrationHistory,
  reserveBeeroomOrchestrationRound,
  restoreBeeroomOrchestrationHistory
} from '@/api/beeroom';
import {
  createWunderWorkspaceDir,
  deleteWunderWorkspaceEntry,
  listWunderWorkspace,
  fetchWunderWorkspaceContent,
  saveWunderWorkspaceFile
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

export type OrchestrationHistoryItem = {
  orchestrationId: string;
  runId: string;
  groupId: string;
  motherAgentId: string;
  motherAgentName: string;
  motherSessionId: string;
  status: string;
  latestRoundIndex: number;
  enteredAt: number;
  updatedAt: number;
  exitedAt: number;
  restoredAt: number;
};

type OrchestrationMemberThread = {
  agentId: string;
  sessionId: string;
};

type OrchestrationSuppressedMessageRange = {
  startAt: number;
  endAt: number;
};

type PersistedRuntime = {
  version: 4;
  groupId: string;
  orchestrationId: string;
  runId: string;
  active: boolean;
  createdAt: number;
  motherAgentId: string;
  motherSessionId: string;
  currentSituation: string;
  plannedSituations: Record<string, string>;
  rounds: OrchestrationRound[];
  activeRoundId: string;
  memberThreads: OrchestrationMemberThread[];
  motherPrimerInjected: boolean;
  pendingRoundId?: string;
  pendingRoundCreated?: boolean;
  pendingMessageStartedAt?: number;
  suppressedMessageRanges?: OrchestrationSuppressedMessageRange[];
};

type WorkspaceEntryLike = {
  name?: unknown;
  path?: unknown;
  type?: unknown;
  size?: unknown;
  updated_time?: unknown;
};

const ORCHESTRATION_STORAGE_PREFIX = 'wunder:orchestration-runtime';
const ORCHESTRATION_RUNTIME_VERSION = 4;
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

const normalizeSuppressedMessageRanges = (value: unknown): OrchestrationSuppressedMessageRange[] =>
  Array.isArray(value)
    ? value
        .map((item) => {
          if (!item || typeof item !== 'object' || Array.isArray(item)) return null;
          const record = item as Record<string, unknown>;
          const startAt = normalizeMsTime(record.startAt ?? record.start_at);
          const endAt = normalizeMsTime(record.endAt ?? record.end_at);
          if (!startAt || !endAt || endAt < startAt) return null;
          return { startAt, endAt } satisfies OrchestrationSuppressedMessageRange;
        })
        .filter((item): item is OrchestrationSuppressedMessageRange => Boolean(item))
    : [];

const normalizeSecondsTime = (value: unknown): number => {
  const ms = normalizeMsTime(value);
  return ms > 0 ? Math.floor(ms / 1000) : 0;
};

const buildScopeKey = (groupId: unknown) => normalizeText(groupId) || 'standby';

const buildStorageKey = (groupId: unknown) => `${ORCHESTRATION_STORAGE_PREFIX}:${buildScopeKey(groupId)}`;

const buildRuntimeScopeKey = (runId: string) => `runtime:orchestration:${normalizeText(runId)}`;

const buildClearScopeKey = (runId: string) => `chat:orchestration:${normalizeText(runId)}`;

const buildRoundId = (index: number) => `round_${String(index).padStart(4, '0')}`;

const buildRoundDirName = (index: number) => `round_${String(index).padStart(4, '0')}`;

const buildAgentArtifactPath = (runId: string, roundIndex: number, agentId: string) =>
  ['orchestration', runId, buildRoundDirName(roundIndex), normalizeText(agentId)]
    .filter(Boolean)
    .join('/');

const buildRoundSituationPath = (runId: string, roundIndex: number) =>
  ['orchestration', runId, buildRoundDirName(roundIndex), 'situation.txt']
    .filter(Boolean)
    .join('/');

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
    userMessage: String(record.userMessage ?? record.user_message ?? ''),
    createdAt: normalizeMsTime(record.createdAt ?? record.created_at) || Date.now(),
    missionIds: Array.isArray(record.missionIds)
      ? record.missionIds.map((item) => normalizeText(item)).filter(Boolean)
      : []
  };
};

const normalizeRemoteRoundState = (value: unknown) => {
  const record = value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
  const rounds = Array.isArray(record.rounds)
    ? record.rounds.map(normalizeRound).filter((item): item is OrchestrationRound => Boolean(item))
    : [];
  return {
    rounds,
    suppressedMessageRanges: normalizeSuppressedMessageRanges(record.suppressed_message_ranges)
  };
};

const normalizePersistedRuntime = (value: unknown, groupId: unknown): PersistedRuntime | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const normalizedGroupId = buildScopeKey(groupId);
  const storedGroupId = buildScopeKey(record.groupId);
  if (storedGroupId !== normalizedGroupId) return null;
  const runId = normalizeText(record.runId);
  const orchestrationId = normalizeText(record.orchestrationId);
  const motherSessionId = normalizeText(record.motherSessionId);
  const motherAgentId = normalizeText(record.motherAgentId);
  if (!runId || !orchestrationId || !motherSessionId || !motherAgentId) return null;
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
    orchestrationId,
    runId,
    active: record.active !== false,
    createdAt: Number(record.createdAt || Date.now()),
    motherAgentId,
    motherSessionId,
    currentSituation: String(record.currentSituation || ''),
    plannedSituations: normalizePlannedSituations(record.plannedSituations, rounds),
    rounds,
    activeRoundId:
      normalizeText(record.activeRoundId) || (rounds.length ? rounds[rounds.length - 1].id : ''),
    memberThreads,
    motherPrimerInjected: record.motherPrimerInjected === true,
    pendingRoundId: normalizeText(record.pendingRoundId),
    pendingRoundCreated: record.pendingRoundCreated === true,
    pendingMessageStartedAt: normalizeMsTime(record.pendingMessageStartedAt),
    suppressedMessageRanges: normalizeSuppressedMessageRanges(record.suppressedMessageRanges)
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
  const historyLoading = ref(false);
  const historyItems = ref<OrchestrationHistoryItem[]>([]);
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

  const activeRound = computed(() => {
    const rounds = runtimeState.value?.rounds || [];
    const activeRoundId = normalizeText(runtimeState.value?.activeRoundId);
    return rounds.find((item) => item.id === activeRoundId) || rounds[rounds.length - 1] || null;
  });

  const latestRound = computed(() => {
    const rounds = runtimeState.value?.rounds || [];
    return rounds[rounds.length - 1] || null;
  });
  const pendingRound = computed(() => {
    const current = runtimeState.value;
    const pendingRoundId = normalizeText(current?.pendingRoundId);
    if (!current || !pendingRoundId) return null;
    return current.rounds.find((item) => item.id === pendingRoundId) || null;
  });

  const hasRuntime = computed(() => Boolean(runtimeState.value?.runId && runtimeState.value?.orchestrationId));
  const isActive = computed(() => runtimeState.value?.active === true);
  const isReady = computed(() => hasRuntime.value);
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
      if ((current.suppressedMessageRanges || []).some((range) => timeMs >= range.startAt && timeMs <= range.endAt)) {
        return false;
      }
      return true;
    });
  });

  const normalizeHistoryItem = (value: unknown): OrchestrationHistoryItem | null => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
    const record = value as Record<string, unknown>;
    const orchestrationId = normalizeText(record.orchestration_id);
    const runId = normalizeText(record.run_id);
    const motherSessionId = normalizeText(record.mother_session_id);
    if (!orchestrationId || !runId || !motherSessionId) return null;
    return {
      orchestrationId,
      runId,
      groupId: normalizeText(record.group_id),
      motherAgentId: normalizeText(record.mother_agent_id),
      motherAgentName: normalizeText(record.mother_agent_name),
      motherSessionId,
      status: normalizeText(record.status),
      latestRoundIndex: Math.max(1, Number.parseInt(String(record.latest_round_index ?? ''), 10) || 1),
      enteredAt: normalizeMsTime(record.entered_at),
      updatedAt: normalizeMsTime(record.updated_at),
      exitedAt: normalizeMsTime(record.exited_at),
      restoredAt: normalizeMsTime(record.restored_at)
    };
  };

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

  const ensureMotherRoundDir = async (state: PersistedRuntime, round: OrchestrationRound) => {
    const motherId = normalizeText(state.motherAgentId);
    if (!motherId) return;
    await createWunderWorkspaceDir({
      agent_id: resolveWorkspaceAgentId(motherId),
      path: ['orchestration', state.runId, buildRoundDirName(round.index)].filter(Boolean).join('/')
    }).catch(() => null);
  };

  const saveRoundSituationFile = async (
    state: PersistedRuntime,
    roundIndex: number,
    situation: string
  ) => {
    const motherId = normalizeText(state.motherAgentId);
    const runId = normalizeText(state.runId);
    if (!motherId || !runId || !Number.isFinite(roundIndex) || roundIndex <= 0) return;
    await createWunderWorkspaceDir({
      agent_id: resolveWorkspaceAgentId(motherId),
      path: ['orchestration', runId, buildRoundDirName(roundIndex)].filter(Boolean).join('/')
    }).catch(() => null);
    await saveWunderWorkspaceFile({
      agent_id: resolveWorkspaceAgentId(motherId),
      path: buildRoundSituationPath(runId, roundIndex),
      content: String(situation || ''),
      create_if_missing: true
    });
  };

  const loadRoundSituationFile = async (
    state: PersistedRuntime,
    roundIndex: number
  ): Promise<string | null> => {
    const motherId = normalizeText(state.motherAgentId);
    const runId = normalizeText(state.runId);
    if (!motherId || !runId || !Number.isFinite(roundIndex) || roundIndex <= 0) return null;
    try {
      const response = await fetchWunderWorkspaceContent({
        agent_id: resolveWorkspaceAgentId(motherId),
        path: buildRoundSituationPath(runId, roundIndex),
        include_content: true,
        max_bytes: ORCHESTRATION_ARTIFACT_PREVIEW_MAX_BYTES
      });
      return String(response?.data?.content || '').trim();
    } catch {
      return null;
    }
  };

  const syncSituationFiles = async (
    state: PersistedRuntime,
    entries: Record<string, string>,
    rounds: OrchestrationRound[] = state.rounds
  ) => {
    const roundIndexes = new Set<number>();
    Object.keys(entries || {}).forEach((key) => {
      const parsed = Number.parseInt(key, 10);
      if (Number.isFinite(parsed) && parsed > 0) {
        roundIndexes.add(parsed);
      }
    });
    rounds.forEach((round) => {
      if (Number.isFinite(round.index) && round.index > 0) {
        roundIndexes.add(round.index);
      }
    });
    await Promise.all(
      Array.from(roundIndexes).map((roundIndex) => {
        const value =
          String(entries[String(roundIndex)] || '').trim() ||
          String(rounds.find((round) => round.index === roundIndex)?.situation || '').trim();
        return saveRoundSituationFile(state, roundIndex, value);
      })
    );
  };

  const hydrateSituationFiles = async (state: PersistedRuntime) => {
    const plannedSituations = { ...(state.plannedSituations || {}) };
    let changed = false;
    await Promise.all(
      (state.rounds || []).map(async (round) => {
        const fileSituation = await loadRoundSituationFile(state, round.index);
        if (fileSituation === null) return;
        const key = normalizeRoundIndexKey(round.index);
        if (!key) return;
        const currentValue = String(plannedSituations[key] || '').trim();
        if (currentValue === fileSituation) return;
        if (fileSituation) {
          plannedSituations[key] = fileSituation;
        } else {
          delete plannedSituations[key];
        }
        changed = true;
      })
    );
    if (!changed) return state;
    const rounds = applyPlannedSituationsToRounds(state.rounds, plannedSituations);
    const activeRoundId = normalizeText(state.activeRoundId);
    const currentActiveRound = rounds.find((item) => item.id === activeRoundId) || rounds[rounds.length - 1] || null;
    return {
      ...state,
      currentSituation: String(currentActiveRound?.situation || '').trim(),
      plannedSituations,
      rounds
    } satisfies PersistedRuntime;
  };

  const bindMemberThreadsAsMain = async (state: PersistedRuntime) => {
    (Array.isArray(state.memberThreads) ? state.memberThreads : []).forEach((item) => {
      const agentId = normalizeText(item.agentId);
      const sessionId = normalizeText(item.sessionId);
      if (!agentId || !sessionId) return;
      chatStore.syncSessionSummary(
        {
          id: sessionId,
          agent_id: agentId,
          is_main: true,
          title: ''
        },
        {
          agentId,
          remember: true
        }
      );
    });
  };

  const buildInitialRuntime = (payload: {
    orchestrationId: string;
    runId: string;
    motherSessionId: string;
    memberThreads: OrchestrationMemberThread[];
  }): PersistedRuntime => {
    const firstRound: OrchestrationRound = {
      id: buildRoundId(1),
      index: 1,
      situation: '',
      userMessage: '',
      createdAt: Date.now(),
      missionIds: []
    };
    return {
      version: ORCHESTRATION_RUNTIME_VERSION,
      groupId: groupId.value,
      orchestrationId: payload.orchestrationId,
      runId: payload.runId,
      active: true,
      createdAt: Date.now(),
      motherAgentId: motherAgentId.value,
      motherSessionId: payload.motherSessionId,
      currentSituation: '',
      plannedSituations: {},
      rounds: [firstRound],
      activeRoundId: firstRound.id,
      memberThreads: payload.memberThreads,
      motherPrimerInjected: false,
      pendingRoundId: '',
      pendingRoundCreated: false,
      pendingMessageStartedAt: 0,
      suppressedMessageRanges: []
    };
  };

  const applyRemoteState = async (remoteState: Record<string, unknown> | null, preserveExisting = true) => {
    if (!remoteState) {
      setRuntime(null);
      return null;
    }
    const runId = normalizeText(remoteState.run_id);
    const orchestrationId = normalizeText(remoteState.orchestration_id);
    const motherSessionId = normalizeText(remoteState.mother_session_id);
    if (!runId || !motherSessionId) {
      setRuntime(null);
      return null;
    }
    if (!orchestrationId) {
      setRuntime(null);
      return null;
    }
    const memberThreadsRaw = Array.isArray((remoteState as Record<string, unknown>).member_threads)
      ? ((remoteState as Record<string, unknown>).member_threads as Array<Record<string, unknown>>)
      : [];
    const memberThreads = memberThreadsRaw
      .map((item) => ({
        agentId: normalizeText(item?.agent_id),
        sessionId: normalizeText(item?.session_id)
      }))
      .filter((item) => item.agentId && item.sessionId);
    const remoteRoundState = normalizeRemoteRoundState(remoteState.round_state);
    const existing = preserveExisting ? runtimeState.value || readPersistedRuntime(groupId.value) : null;
    const nextRounds = remoteRoundState.rounds.length
      ? remoteRoundState.rounds
      : existing && normalizeText(existing.runId) === runId && existing.rounds.length
        ? existing.rounds
        : buildInitialRuntime({
            orchestrationId,
            runId,
            motherSessionId,
            memberThreads
          }).rounds;
    const existingActiveRoundId =
      existing && normalizeText(existing.runId) === runId ? normalizeText(existing.activeRoundId) : '';
    const activeRound =
      nextRounds.find((item) => item.id === existingActiveRoundId) ||
      nextRounds[nextRounds.length - 1] ||
      null;
    const nextState =
      existing && normalizeText(existing.runId) === runId
        ? {
            ...existing,
            groupId: groupId.value,
            orchestrationId,
            runId,
            active: remoteState.active === true,
            motherAgentId: motherAgentId.value,
            motherSessionId,
            currentSituation: String(activeRound?.situation || '').trim(),
            rounds: nextRounds,
            activeRoundId: activeRound?.id || '',
            memberThreads,
            pendingRoundId: '',
            pendingRoundCreated: false,
            pendingMessageStartedAt: 0,
            suppressedMessageRanges: remoteRoundState.suppressedMessageRanges
          }
        : {
            ...buildInitialRuntime({
              orchestrationId,
              runId,
              motherSessionId,
              memberThreads
            }),
            active: remoteState.active === true,
            currentSituation: String(activeRound?.situation || '').trim(),
            rounds: nextRounds,
            activeRoundId: activeRound?.id || '',
            suppressedMessageRanges: remoteRoundState.suppressedMessageRanges
          };
    if (nextState.active) {
      await bindMemberThreadsAsMain(nextState);
    }
    await Promise.all(
      nextState.rounds.map(async (round) => {
        await ensureRoundArtifactDirs(nextState, round);
        await ensureMotherRoundDir(nextState, round);
        await saveRoundSituationFile(nextState, round.index, round.situation);
      })
    );
    setRuntime(nextState);
    return nextState;
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
      const response = await createBeeroomOrchestrationState({
        group_id: currentGroupId,
        mother_agent_id: currentMotherAgentId
      });
      const stateRecord =
        response?.data?.data?.state && typeof response.data.data.state === 'object'
          ? (response.data.data.state as Record<string, unknown>)
          : null;
      const memberThreadsRaw = Array.isArray(response?.data?.data?.member_threads)
        ? (response.data.data.member_threads as Array<Record<string, unknown>>)
        : [];
      const memberThreads = memberThreadsRaw
        .map((item) => ({
          agentId: normalizeText(item?.agent_id),
          sessionId: normalizeText(item?.session_id)
        }))
        .filter((item) => item.agentId && item.sessionId);
      const nextState = await applyRemoteState(
        stateRecord
          ? {
              ...stateRecord,
              member_threads: memberThreadsRaw
            }
          : null,
        false
      );
      memberThreads.forEach((item) => {
        if (!item.agentId || !item.sessionId) return;
        chatStore.syncSessionSummary(
          {
            id: item.sessionId,
            agent_id: item.agentId,
            is_main: true,
            title: ''
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

  const exitRun = async () => {
    const currentGroupId = groupId.value;
    const current = runtimeState.value;
    if (!currentGroupId || !current) return null;
    const response = await exitBeeroomOrchestrationState({ group_id: currentGroupId });
    const freshThreads = Array.isArray(response?.data?.data?.member_threads)
      ? (response.data.data.member_threads as Array<Record<string, unknown>>)
          .map((item) => ({
            agentId: normalizeText(item?.agent_id),
            sessionId: normalizeText(item?.session_id)
          }))
          .filter((item) => item.agentId && item.sessionId)
      : [];
    freshThreads.forEach((item) => {
      chatStore.syncSessionSummary(
        {
          id: item.sessionId,
          agent_id: item.agentId,
          is_main: true,
          title: ''
        },
        {
          agentId: item.agentId,
          remember: true
        }
      );
    });
    const nextState = {
      ...current,
      active: false,
      memberThreads: freshThreads.length ? freshThreads : current.memberThreads,
      pendingRoundId: '',
      pendingRoundCreated: false,
      pendingMessageStartedAt: 0
    } satisfies PersistedRuntime;
    setRuntime(nextState);
    return nextState;
  };

  const loadHistory = async () => {
    const currentGroupId = groupId.value;
    if (!currentGroupId) {
      historyItems.value = [];
      return [];
    }
    historyLoading.value = true;
    try {
      const response = await listBeeroomOrchestrationHistory({ group_id: currentGroupId });
      const items = Array.isArray(response?.data?.data?.items)
        ? response.data.data.items.map(normalizeHistoryItem).filter((item): item is OrchestrationHistoryItem => Boolean(item))
        : [];
      historyItems.value = items;
      return items;
    } finally {
      historyLoading.value = false;
    }
  };

  const restoreHistory = async (orchestrationId: string, options: { activate?: boolean } = {}) => {
    const currentGroupId = groupId.value;
    if (!currentGroupId) {
      throw new Error('orchestration_group_missing');
    }
    const normalizedOrchestrationId = normalizeText(orchestrationId);
    if (!normalizedOrchestrationId) {
      throw new Error('orchestration_history_missing');
    }
    const response = await restoreBeeroomOrchestrationHistory({
      group_id: currentGroupId,
      orchestration_id: normalizedOrchestrationId,
      activate: options.activate ?? isActive.value
    });
    const stateRecord =
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null;
    const memberThreads = Array.isArray(response?.data?.data?.member_threads)
      ? (response.data.data.member_threads as Array<Record<string, unknown>>)
      : [];
    const nextState = await applyRemoteState(
      stateRecord
        ? {
            ...stateRecord,
            member_threads: memberThreads
          }
        : null,
      false
    );
    if (!nextState) {
      return null;
    }
    const hydrated = await hydrateSituationFiles(nextState);
    if (hydrated !== nextState) {
      setRuntime(hydrated);
    }
    await loadHistory().catch(() => []);
    return hydrated;
  };

  const ensureRuntime = async () => {
    if (runtimeState.value?.runId && runtimeState.value?.orchestrationId) return runtimeState.value;
    const currentGroupId = groupId.value;
    if (!currentGroupId || !motherAgentId.value) {
      return null;
    }
    try {
      const remote = await getBeeroomOrchestrationState({ group_id: currentGroupId });
      const remoteState =
        remote?.data?.data?.state && typeof remote.data.data.state === 'object'
          ? (remote.data.data.state as Record<string, unknown>)
          : null;
      const memberThreads = Array.isArray(remote?.data?.data?.member_threads)
        ? (remote.data.data.member_threads as Array<Record<string, unknown>>)
        : [];
      if (remoteState) {
        const hydrated = await applyRemoteState(
          {
            ...remoteState,
            member_threads: memberThreads
          },
          true
        );
        if (hydrated) {
          const nextHydrated = await hydrateSituationFiles(hydrated);
          if (nextHydrated !== hydrated) {
            setRuntime(nextHydrated);
          }
          return nextHydrated;
        }
      }
    } catch {
      // Fall through to local cache / creation path.
    }
    const persisted = readPersistedRuntime(groupId.value);
    if (
      persisted &&
      persisted.motherAgentId === motherAgentId.value &&
      buildScopeKey(persisted.groupId) === groupId.value
    ) {
      runtimeState.value = persisted;
      if (persisted.active) {
        await bindMemberThreadsAsMain(persisted);
      }
      const hydrated = await hydrateSituationFiles(persisted);
      if (hydrated !== persisted) {
        setRuntime(hydrated);
      }
      return hydrated;
    }
    return null;
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
    await ensureMotherRoundDir(nextState, round);
    await saveRoundSituationFile(nextState, round.index, resolvedSituation);
    setRuntime(nextState);
    return round;
  };

  const reserveUserRound = async (payload: { situation?: string; userMessage: string; targetRoundId?: string }) => {
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
    const shouldCreateRound = !currentRound || Boolean(normalizeText(currentRound.userMessage));
    const targetRoundIndex = shouldCreateRound
      ? Math.max(1, ...(current.rounds || []).map((item) => item.index)) + 1
      : currentRound.index;
    const response = await reserveBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: shouldCreateRound ? '' : currentRound?.id || '',
      round_index: targetRoundIndex,
      situation: normalizedSituation,
      user_message: normalizedMessage
    });
    const remoteState =
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null;
    const nextState = await applyRemoteState(
      remoteState
        ? {
            ...remoteState,
            member_threads: (current.memberThreads || []).map((item) => ({
              agent_id: item.agentId,
              session_id: item.sessionId
            }))
          }
        : null,
      true
    );
    const reservedRound =
      normalizeRound(response?.data?.data?.round) ||
      nextState?.rounds.find((item) => item.index === targetRoundIndex) ||
      null;
    if (!nextState || !reservedRound) return reservedRound;
    await saveRoundSituationFile(nextState, reservedRound.index, normalizedSituation);
    setRuntime({
      ...nextState,
      currentSituation: normalizedSituation,
      activeRoundId: reservedRound.id,
      pendingRoundId: reservedRound.id,
      pendingRoundCreated: shouldCreateRound,
      pendingMessageStartedAt: Date.now()
    });
    return reservedRound;
  };

  const commitUserRound = async (payload: { situation?: string; userMessage: string; targetRoundId?: string }) => {
    const reserved = await reserveUserRound(payload);
    if (!reserved) return null;
    return finalizePendingRound(reserved.id);
  };

  const finalizePendingRound = async (roundId?: string) => {
    const current = runtimeState.value;
    if (!current) return null;
    const resolvedRoundId = normalizeText(roundId) || normalizeText(current.pendingRoundId);
    if (!resolvedRoundId) return null;
    if (!current.rounds.some((item) => item.id === resolvedRoundId)) return null;
    const response = await finalizeBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: resolvedRoundId
    });
    const remoteState =
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null;
    const nextState = await applyRemoteState(
      remoteState
        ? {
            ...remoteState,
            member_threads: (current.memberThreads || []).map((item) => ({
              agent_id: item.agentId,
              session_id: item.sessionId
            }))
          }
        : null,
      true
    );
    if (!nextState) return normalizeRound(response?.data?.data?.round);
    setRuntime({
      ...nextState,
      pendingRoundId: '',
      pendingRoundCreated: false,
      pendingMessageStartedAt: 0
    });
    return normalizeRound(response?.data?.data?.round) || nextState.rounds.find((item) => item.id === resolvedRoundId) || null;
  };

  const discardPendingRound = async (roundId?: string) => {
    const current = runtimeState.value;
    if (!current) return null;
    const resolvedRoundId = normalizeText(roundId) || normalizeText(current.pendingRoundId);
    if (!resolvedRoundId) return null;
    const round = current.rounds.find((item) => item.id === resolvedRoundId) || null;
    if (!round) return null;
    const currentPendingId = normalizeText(current.pendingRoundId);
    const pendingMessageStartedAt = normalizeMsTime(current.pendingMessageStartedAt);
    const discardCompletedAt = Date.now();
    const removeWholeRound =
      currentPendingId === resolvedRoundId &&
      current.pendingRoundCreated === true &&
      normalizeText(round.userMessage) &&
      current.rounds[current.rounds.length - 1]?.id === resolvedRoundId;
    const nextRounds = removeWholeRound
      ? current.rounds.filter((item) => item.id !== resolvedRoundId)
      : current.rounds.map((item) =>
          item.id === resolvedRoundId
            ? {
                ...item,
                userMessage: ''
              }
            : item
        );
    const fallbackRound = nextRounds.find((item) => item.id === current.activeRoundId) || nextRounds[nextRounds.length - 1] || null;
    const response = await cancelBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: resolvedRoundId,
      message_started_at: pendingMessageStartedAt || undefined,
      message_ended_at: discardCompletedAt,
      remove_round: removeWholeRound
    });
    const remoteState =
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null;
    const appliedState = await applyRemoteState(
      remoteState
        ? {
            ...remoteState,
            member_threads: (current.memberThreads || []).map((item) => ({
              agent_id: item.agentId,
              session_id: item.sessionId
            }))
          }
        : null,
      true
    );
    const nextState = appliedState
      ? {
          ...appliedState,
          activeRoundId: appliedState.rounds.find((item) => item.id === fallbackRound?.id)?.id || appliedState.activeRoundId,
          pendingRoundId: '',
          pendingRoundCreated: false,
          pendingMessageStartedAt: 0
        }
      : {
          ...current,
        rounds: nextRounds.length ? nextRounds : [buildInitialRuntime({
          orchestrationId: current.orchestrationId,
          runId: current.runId,
          motherSessionId: current.motherSessionId,
          memberThreads: current.memberThreads
        }).rounds[0]],
          activeRoundId: fallbackRound?.id || buildRoundId(1),
          currentSituation: String(fallbackRound?.situation || '').trim(),
          pendingRoundId: '',
          pendingRoundCreated: false,
          pendingMessageStartedAt: 0
        };
    setRuntime(nextState);
    if (removeWholeRound) {
      const motherId = normalizeText(current.motherAgentId);
      const roundPath = ['orchestration', current.runId, buildRoundDirName(round.index)].filter(Boolean).join('/');
      if (motherId && roundPath) {
        await deleteWunderWorkspaceEntry({
          agent_id: resolveWorkspaceAgentId(motherId),
          path: roundPath
        }).catch(() => null);
      }
      await Promise.all(
        visibleWorkers.value.map((member) => {
          const agentId = normalizeText(member?.agent_id);
          if (!agentId) return Promise.resolve();
          return deleteWunderWorkspaceEntry({
            agent_id: resolveWorkspaceAgentId(agentId),
            container_id: resolveWorkspaceContainerId(member),
            path: buildAgentArtifactPath(current.runId, round.index, agentId)
          }).catch(() => null);
        })
      );
    } else {
      await saveRoundSituationFile(nextState, round.index, round.situation);
    }
    return nextState.rounds.find((item) => item.id === nextState.activeRoundId) || null;
  };

  const startRun = async () => {
    const current = runtimeState.value;
    const currentGroupId = groupId.value;
    if (!current || !currentGroupId || current.active) {
      return current;
    }
    return restoreHistory(current.orchestrationId, { activate: true });
  };

  const resolveRoundSituation = async (roundIndex: number) => {
    const current = await ensureRuntime();
    if (!current || !Number.isFinite(roundIndex) || roundIndex <= 0) {
      return '';
    }
    const roundKey = normalizeRoundIndexKey(roundIndex);
    if (!roundKey) {
      return '';
    }
    const fileSituation = await loadRoundSituationFile(current, roundIndex);
    const currentPlanned = String(current.plannedSituations[roundKey] || '').trim();
    const currentRoundSituation = String(
      current.rounds.find((item) => item.index === roundIndex)?.situation || ''
    ).trim();
    const resolvedSituation =
      fileSituation !== null ? fileSituation : currentPlanned || currentRoundSituation;
    if (fileSituation === null) {
      return resolvedSituation;
    }
    const nextPlannedSituations = { ...(current.plannedSituations || {}) };
    if (resolvedSituation) {
      nextPlannedSituations[roundKey] = resolvedSituation;
    } else {
      delete nextPlannedSituations[roundKey];
    }
    const nextRounds = applyPlannedSituationsToRounds(current.rounds, nextPlannedSituations);
    const activeRoundId = normalizeText(current.activeRoundId);
    const currentActiveRound =
      nextRounds.find((item) => item.id === activeRoundId) || nextRounds[nextRounds.length - 1] || null;
    const nextCurrentSituation =
      currentActiveRound?.index === roundIndex
        ? resolvedSituation
        : String(currentActiveRound?.situation || '').trim();
    const plannedChanged = currentPlanned !== resolvedSituation;
    const roundsChanged = nextRounds.some((round, index) => {
      const previous = current.rounds[index];
      return previous?.situation !== round.situation;
    });
    if (plannedChanged || roundsChanged || current.currentSituation !== nextCurrentSituation) {
      setRuntime({
        ...current,
        currentSituation: nextCurrentSituation,
        plannedSituations: nextPlannedSituations,
        rounds: nextRounds
      });
    }
    return resolvedSituation;
  };

  const markMotherPrimerInjected = () => {
    const current = runtimeState.value;
    if (!current || current.motherPrimerInjected) return;
    setRuntime({
      ...current,
      motherPrimerInjected: true
    });
  };

  const updateSituation = async (value: string) => {
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
    const nextState = {
      ...current,
      currentSituation: normalizedValue,
      plannedSituations,
      rounds: nextRounds
    };
    setRuntime(nextState);
    await saveRoundSituationFile(nextState, round.index, normalizedValue);
  };

  const updatePlannedSituations = async (entries: Record<string, string>) => {
    const current = runtimeState.value;
    if (!current) return;
    const plannedSituations = normalizePlannedSituations(entries);
    const nextRounds = applyPlannedSituationsToRounds(current.rounds, plannedSituations);
    const activeRoundId = normalizeText(current.activeRoundId);
    const currentActiveRound = nextRounds.find((item) => item.id === activeRoundId) || nextRounds[nextRounds.length - 1] || null;
    const nextState = {
      ...current,
      currentSituation: String(currentActiveRound?.situation || '').trim(),
      plannedSituations,
      rounds: nextRounds
    };
    setRuntime(nextState);
    await syncSituationFiles(nextState, plannedSituations, nextRounds);
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
      historyItems.value = [];
      void loadHistory().catch(() => []);
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
    pendingRound,
    activeRoundChatMessages,
    visibleWorkers,
    artifactCards,
    artifactLoading,
    artifactError,
    historyLoading,
    historyItems,
    initializing,
    initError,
    hasRuntime,
    isActive,
    isReady,
    ensureRuntime,
    initializeRun,
    startRun,
    exitRun,
    loadHistory,
    restoreHistory,
    createRound,
    reserveUserRound,
    commitUserRound,
    finalizePendingRound,
    discardPendingRound,
    resolveRoundSituation,
    markMotherPrimerInjected,
    updateSituation,
    updatePlannedSituations,
    selectRound,
    reloadArtifacts,
    resolveWorkerOutputs,
    resolveWorkerThreadSessionId
  };
};
