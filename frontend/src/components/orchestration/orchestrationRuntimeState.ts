import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue';

import { getSessionEvents } from '@/api/chat';
import {
  branchBeeroomOrchestrationHistory,
  cancelBeeroomOrchestrationRound,
  createBeeroomOrchestrationState,
  deleteBeeroomOrchestrationHistory,
  exitBeeroomOrchestrationState,
  finalizeBeeroomOrchestrationRound,
  getBeeroomOrchestrationState,
  listBeeroomOrchestrationHistory,
  reserveBeeroomOrchestrationRound,
  truncateBeeroomOrchestrationHistory,
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
import {
  compareMissionChatMessages,
  type MissionChatMessage
} from '@/components/beeroom/beeroomCanvasChatModel';
import {
  buildSessionWorkflowItems,
  buildTaskWorkflowRuntime,
  type BeeroomTaskWorkflowPreview,
  type BeeroomWorkflowItem
} from '@/components/beeroom/beeroomTaskWorkflow';
import { useI18n } from '@/i18n';
import type { BeeroomGroup, BeeroomMember, BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import { useChatStore } from '@/stores/chat';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';
import {
  buildOrchestrationAgentArtifactPath,
  buildOrchestrationRoundDirName,
  buildOrchestrationRoundId,
  buildOrchestrationRoundSituationPath,
  normalizeOrchestrationText
} from '@/components/orchestration/orchestrationShared';
import { stabilizeOrchestrationRoundSnapshots } from '@/components/orchestration/orchestrationRoundStateStability';

export type OrchestrationRound = {
  id: string;
  index: number;
  situation: string;
  userMessage: string;
  createdAt: number;
  finalizedAt?: number;
  missionIds: string[];
  branchParentRoundId?: string;
  branchFromRoundIndex?: number;
  branchRootOrchestrationId?: string;
  orchestrationId?: string;
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
  parentOrchestrationId: string;
  branchRootOrchestrationId: string;
  branchFromRoundIndex: number;
  branchDepth: number;
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
  version: 5;
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

type SessionRoundLike = {
  events?: Array<{
    event?: string;
    data?: unknown;
    timestamp?: string;
  }>;
};

const ORCHESTRATION_STORAGE_PREFIX = 'wunder:orchestration-runtime';
const ORCHESTRATION_RUNTIME_VERSION = 5;
const ORCHESTRATION_ARTIFACT_PREVIEW_MAX_BYTES = 4096;
const ORCHESTRATION_ARTIFACT_CARD_LIMIT = 6;
const ORCHESTRATION_WORKFLOW_POLL_INTERVAL_MS = 1200;

const normalizeText = normalizeOrchestrationText;

const orchestrationDebugLog = (event: string, payload?: unknown) => {
  chatDebugLog('orchestration-runtime', event, payload);
};

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

const mergeRoundSituationsIntoPlanned = (
  rounds: OrchestrationRound[] | null | undefined,
  plannedSituations: Record<string, string> | null | undefined
): Record<string, string> => {
  const merged = normalizePlannedSituations(plannedSituations || {}, Array.isArray(rounds) ? rounds : []);
  (Array.isArray(rounds) ? rounds : []).forEach((round) => {
    const roundKey = normalizeRoundIndexKey(round.index);
    const situation = String(round?.situation || '').trim();
    if (!roundKey || !situation) return;
    merged[roundKey] = situation;
  });
  return merged;
};

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

const normalizeMessageBody = (value: unknown) => normalizeText(value).replace(/\s+/g, ' ');

const buildScopeKey = (groupId: unknown) => normalizeText(groupId) || 'standby';

const buildStorageKey = (groupId: unknown) => `${ORCHESTRATION_STORAGE_PREFIX}:${buildScopeKey(groupId)}`;

const buildRuntimeScopeKey = (runId: string) => `runtime:orchestration:${normalizeText(runId)}`;

const buildClearScopeKey = (runId: string) => `chat:orchestration:${normalizeText(runId)}`;

const buildRoundId = buildOrchestrationRoundId;

const buildRoundDirName = buildOrchestrationRoundDirName;

const buildAgentArtifactPath = buildOrchestrationAgentArtifactPath;

const buildRoundSituationPath = buildOrchestrationRoundSituationPath;

const findLatestFormalRound = (rounds: OrchestrationRound[] | null | undefined) => {
  const source = Array.isArray(rounds) ? rounds : [];
  return source.reduce<OrchestrationRound | null>((latest, round) => {
    if (!round) return latest;
    if (!normalizeText(round.userMessage)) return latest;
    if (!latest) return round;
    return Number(round.index || 0) >= Number(latest.index || 0) ? round : latest;
  }, null);
};

const resolveRoundUserMessageWindow = (
  rounds: OrchestrationRound[],
  targetRoundId: string,
  messages: MissionChatMessage[]
) => {
  const normalizedTargetRoundId = normalizeText(targetRoundId);
  if (!normalizedTargetRoundId) return null;
  const orderedRounds = [...(Array.isArray(rounds) ? rounds : [])].sort(
    (left, right) =>
      Number(left.index || 0) - Number(right.index || 0) ||
      Number(left.createdAt || 0) - Number(right.createdAt || 0)
  );
  const targetRound = orderedRounds.find((round) => round.id === normalizedTargetRoundId) || null;
  if (!targetRound) {
    return null;
  }
  const targetRoundPosition = orderedRounds.findIndex((round) => round.id === normalizedTargetRoundId);
  const nextRound = targetRoundPosition >= 0 ? orderedRounds[targetRoundPosition + 1] || null : null;
  const formalRounds = orderedRounds.filter((round) => normalizeText(round.userMessage));
  const orderedMessages = [...(Array.isArray(messages) ? messages : [])].sort(compareMissionChatMessages);
  const userMessages = orderedMessages.filter((message) => message.tone === 'user');
  if (!userMessages.length) {
    return null;
  }
  const targetCreatedAt = Number(targetRound.createdAt || 0);
  const nextCreatedAt = Number(nextRound?.createdAt || 0);
  if (targetCreatedAt > 0) {
    const targetMessageIndex = userMessages.findIndex((message) => {
      const timeMs = normalizeMsTime(message?.time);
      if (timeMs <= 0 || timeMs < targetCreatedAt) {
        return false;
      }
      return !(nextCreatedAt > 0 && timeMs >= nextCreatedAt);
    });
    if (targetMessageIndex >= 0) {
      const startMessage = userMessages[targetMessageIndex] || null;
      const endMessage =
        nextCreatedAt > 0
          ? userMessages.find((message) => normalizeMsTime(message?.time) >= nextCreatedAt) || null
          : null;
      if (startMessage) {
        return {
          orderedMessages,
          startKey: normalizeText(startMessage.key),
          endKey: normalizeText(endMessage?.key)
        };
      }
    }
  }
  const targetFormalPosition = formalRounds.findIndex((round) => round.id === normalizedTargetRoundId);
  if (targetFormalPosition < 0) {
    return null;
  }
  const mapping = new Map<string, number>();
  let searchCursor = 0;
  formalRounds.forEach((round) => {
    const roundMessage = normalizeMessageBody(round.userMessage);
    if (!roundMessage) return;
    let matchedIndex = -1;
    for (let index = searchCursor; index < userMessages.length; index += 1) {
      if (normalizeMessageBody(userMessages[index]?.body) === roundMessage) {
        matchedIndex = index;
        break;
      }
    }
    if (matchedIndex < 0 && searchCursor < userMessages.length) {
      matchedIndex = searchCursor;
    }
    if (matchedIndex < 0) return;
    mapping.set(round.id, matchedIndex);
    searchCursor = matchedIndex + 1;
  });
  const targetMessageIndex = mapping.get(normalizedTargetRoundId);
  if (targetMessageIndex == null || targetMessageIndex < 0 || targetMessageIndex >= userMessages.length) {
    return null;
  }
  const nextFormalRoundByText = formalRounds[targetFormalPosition + 1] || null;
  const nextMessageIndex = nextFormalRoundByText ? mapping.get(nextFormalRoundByText.id) ?? -1 : -1;
  const startMessage = userMessages[targetMessageIndex] || null;
  const endMessage =
    nextMessageIndex >= 0 && nextMessageIndex < userMessages.length ? userMessages[nextMessageIndex] : null;
  if (!startMessage) {
    return null;
  }
  return {
    orderedMessages,
    startKey: normalizeText(startMessage.key),
    endKey: normalizeText(endMessage?.key)
  };
};

const parseRoundIndexFromDirName = (value: unknown) => {
  const matched = normalizeText(value).match(/^round_(\d{4,})$/i);
  if (!matched?.[1]) return 0;
  return Number.parseInt(matched[1], 10) || 0;
};

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
    finalizedAt: normalizeMsTime(record.finalizedAt ?? record.finalized_at),
    branchParentRoundId: normalizeText(record.branchParentRoundId ?? record.branch_parent_round_id),
    branchFromRoundIndex: Math.max(
      0,
      Number.parseInt(String(record.branchFromRoundIndex ?? record.branch_from_round_index ?? ''), 10) || 0
    ),
    branchRootOrchestrationId: normalizeText(
      record.branchRootOrchestrationId ?? record.branch_root_orchestration_id
    ),
    orchestrationId: normalizeText(record.orchestrationId ?? record.orchestration_id),
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

const attachResponseRoundState = (
  remoteState: Record<string, unknown> | null,
  responseRoundState: unknown
): Record<string, unknown> | null => {
  if (!remoteState) return null;
  if (!responseRoundState || typeof responseRoundState !== 'object' || Array.isArray(responseRoundState)) {
    return remoteState;
  }
  return {
    ...remoteState,
    round_state: responseRoundState
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

const resolveTaskSessionId = (task: BeeroomMissionTask | null | undefined): string =>
  normalizeText(task?.spawned_session_id || task?.target_session_id || task?.session_run_id);

const buildTaskWorkflowRequestKey = (task: BeeroomMissionTask | null | undefined) =>
  [
    normalizeText(task?.task_id),
    normalizeText(task?.agent_id),
    normalizeText(task?.status),
    normalizeText(task?.updated_time),
    normalizeText(task?.started_time),
    normalizeText(task?.finished_time),
    normalizeText(task?.spawned_session_id),
    normalizeText(task?.target_session_id),
    normalizeText(task?.session_run_id),
    normalizeText(task?.retry_count),
    normalizeText(task?.result_summary),
    normalizeText(task?.error)
  ].join('|');

const buildSessionWorkflowFingerprint = (items: BeeroomWorkflowItem[]) =>
  items
    .map((item) =>
      [
        normalizeText(item.id),
        normalizeText(item.title),
        normalizeText(item.detail),
        normalizeText(item.status),
        normalizeText(item.eventType),
        normalizeText(item.toolName),
        normalizeText(item.toolCallId)
      ].join(':')
    )
    .join('||');

const pickLatestWorkerTask = (tasks: BeeroomMissionTask[]): BeeroomMissionTask | null =>
  tasks.reduce<BeeroomMissionTask | null>((latest, current) => {
    if (!latest) return current;
    const currentMoment = Math.max(
      Number(current?.updated_time || 0),
      Number(current?.finished_time || 0),
      Number(current?.started_time || 0)
    );
    const latestMoment = Math.max(
      Number(latest?.updated_time || 0),
      Number(latest?.finished_time || 0),
      Number(latest?.started_time || 0)
    );
    return currentMoment >= latestMoment ? current : latest;
  }, null);

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
  const motherWorkflowItems = ref<BeeroomWorkflowItem[]>([]);
  const workflowItemsByTask = ref<Record<string, BeeroomWorkflowItem[]>>({});
  const workflowPreviewByTask = ref<Record<string, BeeroomTaskWorkflowPreview>>({});
  let artifactReloadTimer: number | null = null;
  let workflowSyncTimer: number | null = null;
  let motherWorkflowController: AbortController | null = null;
  const workerWorkflowControllers = new Map<string, AbortController>();
  let motherWorkflowRequestKey = '';
  let motherWorkflowFetchedAt = 0;
  const workflowFetchMeta = new Map<string, { requestKey: string; fetchedAt: number }>();

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
    const formalRounds = current.rounds.filter((item) => Boolean(normalizeText(item.userMessage)));
    if (!formalRounds.length) {
      return [];
    }
    const userMessageWindow = resolveRoundUserMessageWindow(
      current.rounds,
      round.id,
      source
    );
    if (userMessageWindow) {
      const startIndex = userMessageWindow.orderedMessages.findIndex(
        (message) => normalizeText(message.key) === userMessageWindow.startKey
      );
      const endIndex = userMessageWindow.endKey
        ? userMessageWindow.orderedMessages.findIndex(
            (message) => normalizeText(message.key) === userMessageWindow.endKey
          )
        : -1;
      if (startIndex >= 0) {
        return userMessageWindow.orderedMessages.filter((message, index) => {
          if (index < startIndex) return false;
          if (endIndex >= 0 && index >= endIndex) return false;
          const timeMs = normalizeMsTime(message?.time);
          if (!timeMs) return true;
          if ((current.suppressedMessageRanges || []).some((range) => timeMs >= range.startAt && timeMs <= range.endAt)) {
            return false;
          }
          return true;
        });
      }
    }
    const roundIndex = current.rounds.findIndex((item) => item.id === round.id);
    const nextRound = roundIndex >= 0 ? current.rounds[roundIndex + 1] || null : null;
    const roundStart = Number(round.createdAt || 0);
    const nextRoundStart = Number(nextRound?.createdAt || 0);
    if (roundStart <= 0) {
      return [];
    }
    return source.filter((message) => {
      const timeMs = normalizeMsTime(message?.time);
      if (!timeMs) {
        return false;
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

  const activeRoundMissionTaskMap = computed(() => {
    const grouped = new Map<string, BeeroomMissionTask[]>();
    (Array.isArray(options.missions.value) ? options.missions.value : []).forEach((mission) => {
      const missionId = normalizeText(mission?.mission_id || mission?.team_run_id);
      if (!activeRound.value?.missionIds.includes(missionId)) return;
      (Array.isArray(mission?.tasks) ? mission.tasks : []).forEach((task) => {
        const agentId = normalizeText(task?.agent_id);
        if (!agentId) return;
        const bucket = grouped.get(agentId) || [];
        bucket.push(task);
        grouped.set(agentId, bucket);
      });
    });
    return grouped;
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
      restoredAt: normalizeMsTime(record.restored_at),
      parentOrchestrationId: normalizeText(record.parent_orchestration_id),
      branchRootOrchestrationId: normalizeText(record.branch_root_orchestration_id) || orchestrationId,
      branchFromRoundIndex: Math.max(0, Number.parseInt(String(record.branch_from_round_index ?? ''), 10) || 0),
      branchDepth: Math.max(0, Number.parseInt(String(record.branch_depth ?? ''), 10) || 0)
    };
  };

  const sortHistoryItems = (items: OrchestrationHistoryItem[]) =>
    [...items].sort((left, right) => {
      const leftHot = Math.max(left.updatedAt || 0, left.restoredAt || 0, left.enteredAt || 0);
      const rightHot = Math.max(right.updatedAt || 0, right.restoredAt || 0, right.enteredAt || 0);
      if (leftHot !== rightHot) return rightHot - leftHot;
      const branchDepthDiff = (left.branchDepth || 0) - (right.branchDepth || 0);
      if (branchDepthDiff !== 0) return branchDepthDiff;
      return String(left.runId || left.orchestrationId).localeCompare(
        String(right.runId || right.orchestrationId),
        'zh-Hans-CN'
      );
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
        const agentName = normalizeText(member?.name) || agentId;
        if (!agentId) return Promise.resolve();
        const containerId = resolveWorkspaceContainerId(member);
        return createWunderWorkspaceDir({
          agent_id: resolveWorkspaceAgentId(agentId),
          container_id: containerId,
          path: buildAgentArtifactPath(state.runId, round.index, agentName, agentId)
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
        allow_missing: true,
        max_bytes: ORCHESTRATION_ARTIFACT_PREVIEW_MAX_BYTES
      });
      return String(response?.data?.content || '').trim();
    } catch {
      return null;
    }
  };

  const listSituationRoundIndexes = async (state: PersistedRuntime): Promise<number[]> => {
    const motherId = normalizeText(state.motherAgentId);
    const runId = normalizeText(state.runId);
    if (!motherId || !runId) return [];
    try {
      const response = await listWunderWorkspace({
        agent_id: resolveWorkspaceAgentId(motherId),
        path: ['orchestration', runId].filter(Boolean).join('/')
      });
      const entries = Array.isArray(response?.data?.entries) ? response.data.entries : [];
      return entries
        .filter((entry: WorkspaceEntryLike) => normalizeText(entry?.type) === 'dir')
        .map((entry: WorkspaceEntryLike) => parseRoundIndexFromDirName(entry?.name || entry?.path))
        .filter((value, index, array) => Number.isFinite(value) && value > 0 && array.indexOf(value) === index)
        .sort((left, right) => left - right);
    } catch {
      return [];
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
    const existingRoundIndexes = (state.rounds || [])
      .map((round) => Number(round.index || 0))
      .filter((value) => Number.isFinite(value) && value > 0);
    const fileRoundIndexes = await listSituationRoundIndexes(state);
    const roundIndexes = Array.from(new Set([...existingRoundIndexes, ...fileRoundIndexes])).sort((left, right) => left - right);
    await Promise.all(
      roundIndexes.map(async (roundIndex) => {
        const fileSituation = await loadRoundSituationFile(state, roundIndex);
        if (fileSituation === null) return;
        const key = normalizeRoundIndexKey(roundIndex);
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
    const roundsSeed = [...state.rounds];
    roundIndexes.forEach((roundIndex) => {
      if (roundsSeed.some((round) => round.index === roundIndex)) return;
      roundsSeed.push({
        id: buildRoundId(roundIndex),
        index: roundIndex,
        situation: String(plannedSituations[String(roundIndex)] || '').trim(),
        userMessage: '',
        createdAt: Date.now(),
        missionIds: []
      });
    });
    roundsSeed.sort((left, right) => left.index - right.index);
    const rounds = applyPlannedSituationsToRounds(roundsSeed, plannedSituations);
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
    const existingMatchesRun = Boolean(existing && normalizeText(existing.runId) === runId);
    const existingRounds = existingMatchesRun ? existing!.rounds.map((round) => ({ ...round, orchestrationId })) : [];
    const remoteRounds = remoteRoundState.rounds.map((round) => ({
      ...round,
      orchestrationId: normalizeText(round.orchestrationId) || orchestrationId
    }));
    const nextRounds = remoteRounds.length
      ? existingMatchesRun
        ? stabilizeOrchestrationRoundSnapshots(existingRounds, remoteRounds)
        : remoteRounds
      : existingRounds.length
        ? existingRounds
        : buildInitialRuntime({
            orchestrationId,
            runId,
            motherSessionId,
            memberThreads
          }).rounds;
    const existingActiveRoundId =
      existing && normalizeText(existing.runId) === runId ? normalizeText(existing.activeRoundId) : '';
    const latestFormalRound = findLatestFormalRound(nextRounds);
    const retainedActiveRound =
      nextRounds.find((item) => item.id === existingActiveRoundId) || null;
    const activeRound =
      retainedActiveRound ||
      latestFormalRound ||
      nextRounds[nextRounds.length - 1] ||
      null;
    const nextPlannedSituations = mergeRoundSituationsIntoPlanned(
      nextRounds,
      existing && normalizeText(existing.runId) === runId
        ? existing.plannedSituations
        : undefined
    );
    const nextRoundsWithSituations = applyPlannedSituationsToRounds(nextRounds, nextPlannedSituations);
    const nextActiveRound =
      nextRoundsWithSituations.find((item) => item.id === activeRound?.id) ||
      nextRoundsWithSituations.find((item) => item.id === existingActiveRoundId) ||
      findLatestFormalRound(nextRoundsWithSituations) ||
      nextRoundsWithSituations[nextRoundsWithSituations.length - 1] ||
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
            currentSituation: String(nextActiveRound?.situation || '').trim(),
            plannedSituations: nextPlannedSituations,
            rounds: nextRoundsWithSituations,
            activeRoundId: nextActiveRound?.id || '',
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
            currentSituation: String(nextActiveRound?.situation || '').trim(),
            plannedSituations: nextPlannedSituations,
            rounds: nextRoundsWithSituations,
            activeRoundId: nextActiveRound?.id || '',
            suppressedMessageRanges: remoteRoundState.suppressedMessageRanges
          };
    orchestrationDebugLog('apply-remote-state', {
      preserveExisting,
      remoteRunId: runId,
      remoteOrchestrationId: orchestrationId,
      existingRunId: normalizeText(existing?.runId),
      existingActiveRoundId: normalizeText(existing?.activeRoundId),
      remoteRoundIds: remoteRoundState.rounds.map((round) => ({
        id: round.id,
        index: round.index,
        hasUserMessage: Boolean(normalizeText(round.userMessage)),
        createdAt: round.createdAt
      })),
      nextActiveRoundId: nextState.activeRoundId,
      nextRounds: nextState.rounds.map((round) => ({
        id: round.id,
        index: round.index,
        situation: round.situation,
        hasUserMessage: Boolean(normalizeText(round.userMessage)),
        createdAt: round.createdAt
      })),
      nextPlannedSituations
    });
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

  const initializeRun = async (options: { runName?: string } = {}) => {
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
        mother_agent_id: currentMotherAgentId,
        run_name: String(options.runName || '').trim() || undefined
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
      historyItems.value = sortHistoryItems(items);
      return historyItems.value;
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

  const branchHistory = async (
    sourceOrchestrationId: string,
    roundIndex: number,
    options: { activate?: boolean } = {}
  ) => {
    const currentGroupId = groupId.value;
    if (!currentGroupId) {
      throw new Error('orchestration_group_missing');
    }
    const normalizedSourceId = normalizeText(sourceOrchestrationId);
    const normalizedRoundIndex = Math.max(1, Number.parseInt(String(roundIndex || 1), 10) || 1);
    if (!normalizedSourceId) {
      throw new Error('orchestration_history_missing');
    }
    const response = await branchBeeroomOrchestrationHistory({
      group_id: currentGroupId,
      source_orchestration_id: normalizedSourceId,
      round_index: normalizedRoundIndex,
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

  const deleteHistory = async (orchestrationId: string) => {
    const currentGroupId = groupId.value;
    if (!currentGroupId) {
      throw new Error('orchestration_group_missing');
    }
    const normalizedOrchestrationId = normalizeText(orchestrationId);
    if (!normalizedOrchestrationId) {
      throw new Error('orchestration_history_missing');
    }
    await deleteBeeroomOrchestrationHistory({
      group_id: currentGroupId,
      orchestration_id: normalizedOrchestrationId
    });
    if (runtimeState.value?.orchestrationId === normalizedOrchestrationId) {
      setRuntime(null);
      artifactCards.value = [];
      motherWorkflowItems.value = [];
      workflowItemsByTask.value = {};
      workflowPreviewByTask.value = {};
    }
    historyItems.value = sortHistoryItems(
      historyItems.value.filter((item) => item.orchestrationId !== normalizedOrchestrationId)
    );
    return historyItems.value;
  };

  const truncateHistoryFromRound = async (orchestrationId: string, roundIndex: number) => {
    const currentGroupId = groupId.value;
    if (!currentGroupId) {
      throw new Error('orchestration_group_missing');
    }
    const normalizedOrchestrationId = normalizeText(orchestrationId);
    const normalizedRoundIndex = Math.max(1, Number.parseInt(String(roundIndex || 1), 10) || 1);
    if (!normalizedOrchestrationId) {
      throw new Error('orchestration_history_missing');
    }
    const response = await truncateBeeroomOrchestrationHistory({
      group_id: currentGroupId,
      orchestration_id: normalizedOrchestrationId,
      round_index: normalizedRoundIndex
    });
    const remoteState = attachResponseRoundState(
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null,
      response?.data?.data?.round_state
    );
    if (remoteState) {
      const nextState = await applyRemoteState(
        {
          ...remoteState,
          member_threads: (runtimeState.value?.memberThreads || []).map((item) => ({
            agent_id: item.agentId,
            session_id: item.sessionId
          }))
        },
        true
      );
      if (nextState) {
        const hydrated = await hydrateSituationFiles(nextState);
        if (hydrated !== nextState) {
          setRuntime(hydrated);
        }
      }
    } else if (runtimeState.value && runtimeState.value.orchestrationId === normalizedOrchestrationId) {
      const retained = runtimeState.value.rounds.filter((item) => item.index <= normalizedRoundIndex);
      const activeRound = retained.find((item) => item.index === normalizedRoundIndex) || retained[retained.length - 1] || null;
      setRuntime({
        ...runtimeState.value,
        rounds: retained,
        activeRoundId: activeRound?.id || runtimeState.value.activeRoundId,
        currentSituation: String(activeRound?.situation || '').trim()
      });
    }
    await loadHistory().catch(() => []);
    return runtimeState.value;
  };

  const ensureRuntime = async (options: { forceRemote?: boolean } = {}) => {
    const forceRemote = options.forceRemote === true;
    if (!forceRemote && runtimeState.value?.runId && runtimeState.value?.orchestrationId) {
      return runtimeState.value;
    }
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

  const createRound = async (
    situation: string,
    userMessage = '',
    options: { roundIndex?: number } = {}
  ) => {
    const current = await ensureRuntime();
    if (!current) return null;
    const requestedRoundIndex = Math.max(0, Number.parseInt(String(options.roundIndex || 0), 10) || 0);
    const nextIndex =
      requestedRoundIndex > 0
        ? requestedRoundIndex
        : Math.max(1, ...(current.rounds || []).map((item) => item.index)) + 1;
    const resolvedSituation = String(situation || '').trim() || resolveSituationByRoundIndex(current.plannedSituations, nextIndex);
    const sameIndexRounds = current.rounds
      .filter((item) => Number(item.index || 0) === nextIndex)
      .sort((left, right) => Number(left.createdAt || 0) - Number(right.createdAt || 0));
    const reusablePreviewRound =
      sameIndexRounds.find((item) => !normalizeText(item.userMessage)) || null;
    if (reusablePreviewRound) {
      const nextRounds = current.rounds.map((item) =>
        item.id === reusablePreviewRound.id
          ? {
              ...item,
              situation: resolvedSituation
            }
          : item
      );
      const updatedRound =
        nextRounds.find((item) => item.id === reusablePreviewRound.id) || reusablePreviewRound;
      const nextState: PersistedRuntime = {
        ...current,
        currentSituation:
          current.activeRoundId === reusablePreviewRound.id
            ? resolvedSituation
            : current.currentSituation,
        rounds: nextRounds
      };
      await ensureRoundArtifactDirs(nextState, updatedRound);
      await ensureMotherRoundDir(nextState, updatedRound);
      await saveRoundSituationFile(nextState, updatedRound.index, resolvedSituation);
      setRuntime(nextState);
      return updatedRound;
    }
    if (sameIndexRounds.length) {
      return sameIndexRounds[0];
    }
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
    const latestFormalRound = findLatestFormalRound(current.rounds);
    const nextFormalRoundIndex = Math.max(1, Number(latestFormalRound?.index || 0) + 1);
    const explicitTargetRound = current.rounds.find((item) => item.id === targetRoundId) || null;
    const reusableNextRound =
      explicitTargetRound && !normalizeText(explicitTargetRound.userMessage)
        ? explicitTargetRound
        : current.rounds.find(
            (item) => Number(item.index || 0) === nextFormalRoundIndex && !normalizeText(item.userMessage)
          ) || null;
    const currentRound = reusableNextRound || null;
    const shouldCreateRound = !currentRound;
    const targetRoundIndex = currentRound?.index || nextFormalRoundIndex;
    const normalizedSituation =
      String(payload.situation || '').trim() ||
      resolveSituationByRoundIndex(current.plannedSituations, targetRoundIndex);
    orchestrationDebugLog('reserve-user-round:request', {
      runId: current.runId,
      orchestrationId: current.orchestrationId,
      targetRoundId,
      nextFormalRoundIndex,
      targetRoundIndex,
      shouldCreateRound,
      currentRounds: current.rounds.map((item) => ({
        id: item.id,
        index: item.index,
        hasUserMessage: Boolean(normalizeText(item.userMessage)),
        createdAt: item.createdAt
      }))
    });
    const response = await reserveBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: shouldCreateRound ? '' : currentRound?.id || '',
      round_index: targetRoundIndex,
      situation: normalizedSituation,
      user_message: normalizedMessage
    });
    const remoteState = attachResponseRoundState(
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null,
      response?.data?.data?.round_state
    );
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
    orchestrationDebugLog('reserve-user-round:response', {
      reservedRound: reservedRound
        ? {
            id: reservedRound.id,
            index: reservedRound.index,
            hasUserMessage: Boolean(normalizeText(reservedRound.userMessage)),
            createdAt: reservedRound.createdAt
          }
        : null,
      nextStateActiveRoundId: nextState?.activeRoundId || '',
      nextStateRounds: nextState?.rounds.map((item) => ({
        id: item.id,
        index: item.index,
        hasUserMessage: Boolean(normalizeText(item.userMessage)),
        createdAt: item.createdAt
      })) || []
    });
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

  const finalizePendingRound = async (
    roundId?: string,
    payload: { situation?: string; userMessage?: string } = {}
  ) => {
    const current = runtimeState.value;
    if (!current) return null;
    const resolvedRoundId = normalizeText(roundId) || normalizeText(current.pendingRoundId);
    if (!resolvedRoundId) return null;
    if (!current.rounds.some((item) => item.id === resolvedRoundId)) return null;
    const response = await finalizeBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: resolvedRoundId,
      situation: String(payload.situation || '').trim() || undefined,
      user_message: String(payload.userMessage || '').trim() || undefined
    });
    const remoteState = attachResponseRoundState(
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null,
      response?.data?.data?.round_state
    );
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
    const finalizedRound =
      normalizeRound(response?.data?.data?.round) ||
      nextState.rounds.find((item) => item.id === resolvedRoundId) ||
      findLatestFormalRound(nextState.rounds);
    orchestrationDebugLog('finalize-pending-round:response', {
      resolvedRoundId,
      finalizedRound: finalizedRound
        ? {
            id: finalizedRound.id,
            index: finalizedRound.index,
            hasUserMessage: Boolean(normalizeText(finalizedRound.userMessage)),
            createdAt: finalizedRound.createdAt,
            finalizedAt: finalizedRound.finalizedAt
          }
        : null,
      nextStateActiveRoundId: nextState.activeRoundId,
      nextStateRounds: nextState.rounds.map((item) => ({
        id: item.id,
        index: item.index,
        hasUserMessage: Boolean(normalizeText(item.userMessage)),
        createdAt: item.createdAt,
        finalizedAt: item.finalizedAt
      }))
    });
    setRuntime({
      ...nextState,
      activeRoundId: finalizedRound?.id || nextState.activeRoundId,
      currentSituation: String(finalizedRound?.situation || nextState.currentSituation || '').trim(),
      pendingRoundId: '',
      pendingRoundCreated: false,
      pendingMessageStartedAt: 0
    });
    return finalizedRound || null;
  };

  const discardPendingRound = async (
    roundId?: string,
    options: { clearSituation?: boolean } = {}
  ) => {
    const current = runtimeState.value;
    if (!current) return null;
    const resolvedRoundId = normalizeText(roundId) || normalizeText(current.pendingRoundId);
    if (!resolvedRoundId) return null;
    const round = current.rounds.find((item) => item.id === resolvedRoundId) || null;
    if (!round) return null;
    const clearSituation = options.clearSituation === true;
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
                userMessage: '',
                situation: clearSituation ? '' : item.situation
              }
            : item
        );
    const nextPlannedSituations = { ...(current.plannedSituations || {}) };
    if (clearSituation) {
      const roundKey = normalizeRoundIndexKey(round.index);
      if (roundKey) {
        delete nextPlannedSituations[roundKey];
      }
    }
    const fallbackRound = nextRounds.find((item) => item.id === current.activeRoundId) || nextRounds[nextRounds.length - 1] || null;
    const response = await cancelBeeroomOrchestrationRound({
      group_id: groupId.value,
      round_id: resolvedRoundId,
      message_started_at: pendingMessageStartedAt || undefined,
      message_ended_at: discardCompletedAt,
      remove_round: removeWholeRound
    });
    const remoteState = attachResponseRoundState(
      response?.data?.data?.state && typeof response.data.data.state === 'object'
        ? (response.data.data.state as Record<string, unknown>)
        : null,
      response?.data?.data?.round_state
    );
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
    const appliedRounds = appliedState?.rounds
      ? appliedState.rounds.map((item) =>
          item.id === resolvedRoundId && clearSituation && !removeWholeRound
            ? {
                ...item,
                situation: ''
              }
            : item
        )
      : null;
    orchestrationDebugLog('discard-pending-round:response', {
      resolvedRoundId,
      removeWholeRound,
      fallbackRoundId: fallbackRound?.id || '',
      appliedStateActiveRoundId: appliedState?.activeRoundId || '',
      appliedStateRounds: appliedRounds?.map((item) => ({
        id: item.id,
        index: item.index,
        hasUserMessage: Boolean(normalizeText(item.userMessage)),
        createdAt: item.createdAt
      })) || []
    });
    const nextState = appliedState
      ? {
          ...appliedState,
          rounds: appliedRounds || appliedState.rounds,
          plannedSituations: clearSituation
            ? (() => {
                const nextEntries = { ...(appliedState.plannedSituations || {}) };
                const roundKey = normalizeRoundIndexKey(round.index);
                if (roundKey) {
                  delete nextEntries[roundKey];
                }
                return nextEntries;
              })()
            : appliedState.plannedSituations,
          activeRoundId:
            (appliedRounds || appliedState.rounds).find((item) => item.id === fallbackRound?.id)?.id ||
            appliedState.activeRoundId,
          pendingRoundId: '',
          pendingRoundCreated: false,
          pendingMessageStartedAt: 0
        }
      : {
          ...current,
          plannedSituations: nextPlannedSituations,
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
          const agentName = normalizeText(member?.name) || agentId;
          if (!agentId) return Promise.resolve();
          return deleteWunderWorkspaceEntry({
            agent_id: resolveWorkspaceAgentId(agentId),
            container_id: resolveWorkspaceContainerId(member),
            path: buildAgentArtifactPath(current.runId, round.index, agentName, agentId)
          }).catch(() => null);
        })
      );
    } else {
      await saveRoundSituationFile(nextState, round.index, clearSituation ? '' : round.situation);
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

  const updateMotherWorkflowState = (items: BeeroomWorkflowItem[]) => {
    if (buildSessionWorkflowFingerprint(motherWorkflowItems.value) === buildSessionWorkflowFingerprint(items)) {
      return;
    }
    motherWorkflowItems.value = items;
  };

  const updateTaskWorkflowState = (
    taskId: string,
    payload: { items: BeeroomWorkflowItem[]; preview: BeeroomTaskWorkflowPreview }
  ) => {
    const currentItems = workflowItemsByTask.value[taskId] || [];
    const currentPreview = workflowPreviewByTask.value[taskId];
    const nextFingerprint = buildSessionWorkflowFingerprint(payload.items);
    const currentFingerprint = buildSessionWorkflowFingerprint(currentItems);
    if (currentFingerprint === nextFingerprint && currentPreview?.fingerprint === payload.preview.fingerprint) {
      return;
    }
    workflowItemsByTask.value = {
      ...workflowItemsByTask.value,
      [taskId]: payload.items
    };
    workflowPreviewByTask.value = {
      ...workflowPreviewByTask.value,
      [taskId]: payload.preview
    };
  };

  const clearWorkflowSyncTimer = () => {
    if (workflowSyncTimer === null || typeof window === 'undefined') return;
    window.clearTimeout(workflowSyncTimer);
    workflowSyncTimer = null;
  };

  const resetWorkflowState = () => {
    motherWorkflowController?.abort();
    motherWorkflowController = null;
    workerWorkflowControllers.forEach((controller) => controller.abort());
    workerWorkflowControllers.clear();
    workflowFetchMeta.clear();
    motherWorkflowRequestKey = '';
    motherWorkflowFetchedAt = 0;
    motherWorkflowItems.value = [];
    workflowItemsByTask.value = {};
    workflowPreviewByTask.value = {};
    clearWorkflowSyncTimer();
  };

  const removeStaleWorkflowEntries = (taskIds: Set<string>) => {
    const nextItems = { ...workflowItemsByTask.value };
    const nextPreviews = { ...workflowPreviewByTask.value };
    let changed = false;
    Object.keys(nextItems).forEach((taskId) => {
      if (taskIds.has(taskId)) return;
      delete nextItems[taskId];
      delete nextPreviews[taskId];
      const controller = workerWorkflowControllers.get(taskId);
      if (controller) {
        controller.abort();
        workerWorkflowControllers.delete(taskId);
      }
      workflowFetchMeta.delete(taskId);
      changed = true;
    });
    if (changed) {
      workflowItemsByTask.value = nextItems;
      workflowPreviewByTask.value = nextPreviews;
    }
  };

  const fetchMotherWorkflow = async (force = false) => {
    const sessionId = normalizeText(runtimeState.value?.motherSessionId);
    const round = activeRound.value;
    if (!sessionId || !round) {
      updateMotherWorkflowState([]);
      return;
    }
    const requestKey = [
      sessionId,
      normalizeText(runtimeState.value?.orchestrationId),
      normalizeText(round.id),
      normalizeText(round.index),
      normalizeText(round.createdAt),
      normalizeText(round.situation),
      normalizeText(round.userMessage),
      normalizeText(round.missionIds.join('|'))
    ].join('|');
    const isRecent = Date.now() - motherWorkflowFetchedAt < ORCHESTRATION_WORKFLOW_POLL_INTERVAL_MS - 120;
    if (!force && motherWorkflowRequestKey === requestKey && isRecent) {
      return;
    }
    if (motherWorkflowController) {
      motherWorkflowController.abort();
      motherWorkflowController = null;
    }
    const controller = new AbortController();
    motherWorkflowController = controller;
    motherWorkflowRequestKey = requestKey;
    try {
      const response = await getSessionEvents(sessionId, { signal: controller.signal });
      if (controller.signal.aborted) return;
      const rounds = Array.isArray(response?.data?.data?.rounds)
        ? (response.data.data.rounds as SessionRoundLike[])
        : [];
      updateMotherWorkflowState(buildSessionWorkflowItems(rounds, t));
      motherWorkflowFetchedAt = Date.now();
    } catch {
      if (controller.signal.aborted) return;
      motherWorkflowFetchedAt = Date.now();
      updateMotherWorkflowState([]);
    } finally {
      if (motherWorkflowController === controller) {
        motherWorkflowController = null;
      }
    }
  };

  const fetchWorkerWorkflow = async (task: BeeroomMissionTask, force = false) => {
    const taskId = normalizeText(task?.task_id);
    if (!taskId) return;
    const sessionId = resolveTaskSessionId(task);
    const requestKey = buildTaskWorkflowRequestKey(task);
    const previous = workflowFetchMeta.get(taskId);
    const isRecent =
      previous && Date.now() - previous.fetchedAt < ORCHESTRATION_WORKFLOW_POLL_INTERVAL_MS - 120;
    if (!force && previous?.requestKey === requestKey && isRecent) {
      return;
    }
    if (!sessionId) {
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, [], t));
      workflowFetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
      return;
    }
    const previousController = workerWorkflowControllers.get(taskId);
    if (previousController) {
      previousController.abort();
      workerWorkflowControllers.delete(taskId);
    }
    const controller = new AbortController();
    workerWorkflowControllers.set(taskId, controller);
    try {
      const response = await getSessionEvents(sessionId, { signal: controller.signal });
      if (controller.signal.aborted) return;
      const rounds = Array.isArray(response?.data?.data?.rounds)
        ? (response.data.data.rounds as SessionRoundLike[])
        : [];
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, rounds, t));
      workflowFetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
    } catch {
      if (controller.signal.aborted) return;
      updateTaskWorkflowState(taskId, buildTaskWorkflowRuntime(task, [], t));
      workflowFetchMeta.set(taskId, { requestKey, fetchedAt: Date.now() });
    } finally {
      if (workerWorkflowControllers.get(taskId) === controller) {
        workerWorkflowControllers.delete(taskId);
      }
    }
  };

  const scheduleWorkflowSync = () => {
    clearWorkflowSyncTimer();
    if (!runtimeState.value?.runId || !activeRound.value?.id) return;
    if (typeof window === 'undefined') {
      void syncWorkflowState();
      return;
    }
    workflowSyncTimer = window.setTimeout(() => {
      workflowSyncTimer = null;
      void syncWorkflowState();
    }, ORCHESTRATION_WORKFLOW_POLL_INTERVAL_MS);
  };

  const syncWorkflowState = async (force = false) => {
    const current = runtimeState.value;
    const round = activeRound.value;
    if (!current || !round) {
      resetWorkflowState();
      return;
    }
    const latestTasks = visibleWorkers.value
      .map((member) => pickLatestWorkerTask(activeRoundMissionTaskMap.value.get(normalizeText(member?.agent_id)) || []))
      .filter((task): task is BeeroomMissionTask => Boolean(task));
    removeStaleWorkflowEntries(new Set(latestTasks.map((task) => normalizeText(task.task_id)).filter(Boolean)));
    await Promise.allSettled([
      fetchMotherWorkflow(force),
      ...latestTasks.map((task) => fetchWorkerWorkflow(task, force))
    ]);
    scheduleWorkflowSync();
  };

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
          const path = buildAgentArtifactPath(current.runId, round.index, agentName, agentId);
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
      resetWorkflowState();
      void loadHistory().catch(() => []);
    },
    { immediate: true }
  );

  if (isChatDebugEnabled()) {
    watch(
      () =>
        ({
          runId: runtimeState.value?.runId || '',
          orchestrationId: runtimeState.value?.orchestrationId || '',
          activeRoundId: runtimeState.value?.activeRoundId || '',
          pendingRoundId: runtimeState.value?.pendingRoundId || '',
          rounds: (runtimeState.value?.rounds || []).map((round) => ({
            id: round.id,
            index: round.index,
            hasUserMessage: Boolean(normalizeText(round.userMessage)),
            createdAt: round.createdAt,
            finalizedAt: round.finalizedAt
          }))
        }),
      (snapshot) => {
        orchestrationDebugLog('runtime-snapshot', snapshot);
      },
      { deep: true }
    );
  }

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
    () =>
      [
        runtimeState.value?.runId || '',
        runtimeState.value?.motherSessionId || '',
        runtimeState.value?.orchestrationId || '',
        activeRound.value?.id || '',
        activeRound.value?.missionIds.join('|') || '',
        visibleWorkers.value.map((item) => normalizeText(item?.agent_id)).join('|'),
        (Array.isArray(options.missions.value) ? options.missions.value : [])
          .map((mission) => {
            const missionId = normalizeText(mission?.mission_id || mission?.team_run_id);
            if (!activeRound.value?.missionIds.includes(missionId)) return '';
            const taskPart = (Array.isArray(mission?.tasks) ? mission.tasks : [])
              .map((task) =>
                [
                  normalizeText(task?.task_id),
                  normalizeText(task?.agent_id),
                  normalizeText(task?.status),
                  normalizeText(task?.updated_time),
                  normalizeText(task?.spawned_session_id),
                  normalizeText(task?.target_session_id),
                  normalizeText(task?.session_run_id)
                ].join(':')
              )
              .join(',');
            return `${missionId}|${normalizeText(mission?.status)}|${normalizeText(mission?.completion_status)}|${taskPart}`;
          })
          .filter(Boolean)
          .join('||')
      ].join('|'),
    (signature, previousSignature) => {
      if (!signature) {
        resetWorkflowState();
        return;
      }
      void syncWorkflowState(signature !== previousSignature);
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
    resetWorkflowState();
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
    motherWorkflowItems,
    workflowItemsByTask,
    workflowPreviewByTask,
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
    branchHistory,
    deleteHistory,
    truncateHistoryFromRound,
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
