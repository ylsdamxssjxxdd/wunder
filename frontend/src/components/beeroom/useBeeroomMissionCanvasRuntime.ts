import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, type Ref, watch } from 'vue';

import {
  createSession,
  listSessions,
  resumeMessageStream,
  sendMessageStream
} from '@/api/chat';
import {
  listRecentBeeroomAgentOutputs,
  DEFAULT_BEEROOM_AGENT_OUTPUT_PREVIEW_LIMIT
} from '@/components/beeroom/beeroomAgentOutputPreview';
import {
  BEEROOM_SUBAGENT_REPLY_SORT_ORDER,
  BEEROOM_SUBAGENT_REQUEST_SORT_ORDER,
  collapseMissionChatAssistantTurns,
  ComposerTargetOption,
  compareMissionChatMessages,
  DispatchApprovalItem,
  DispatchRuntimeStatus,
  MissionChatMessage
} from '@/components/beeroom/beeroomCanvasChatModel';
import {
  isBeeroomDefaultAgentLike,
  normalizeBeeroomActorName
} from '@/components/beeroom/beeroomActorIdentity';
import {
  buildBeeroomRuntimeRelayMessageSignature,
  filterBeeroomRuntimeRelayMessagesAfter,
  mergeBeeroomRuntimeRelayMessages
} from '@/components/beeroom/beeroomRuntimeRelayMessages';
import { reconcileBeeroomSessionBackedManualMessages } from '@/components/beeroom/beeroomMissionChatSync';
import { setBeeroomMissionChatState, getBeeroomMissionChatState } from '@/components/beeroom/beeroomMissionChatStateCache';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import {
  resolveBeeroomMotherAgentId,
  resolveBeeroomSwarmScopeKey
} from '@/components/beeroom/canvas/swarmCanvasModel';
import { resolveBeeroomProjectedSubagentAvatarImage } from '@/components/beeroom/canvas/beeroomSwarmAvatarIdentity';
import {
  resolveNextBeeroomMotherDispatchSessionId,
  resolvePreferredBeeroomDispatchSessionId,
  shouldFinishBeeroomTerminalHydration
} from '@/components/beeroom/beeroomDispatchSessionPolicy';
import { overlayBeeroomLiveDispatchLabel } from '@/components/beeroom/beeroomDispatchPreviewOverlay';
import { useBeeroomDispatchSessionPreview } from '@/components/beeroom/useBeeroomDispatchSessionPreview';
import { useBeeroomDemo } from '@/components/beeroom/useBeeroomDemo';
import { useBeeroomMissionWorkflowPreview } from '@/components/beeroom/useBeeroomMissionWorkflowPreview';
import {
  type BeeroomMissionSubagentItem,
  useBeeroomMissionSubagentPreview
} from '@/components/beeroom/useBeeroomMissionSubagentPreview';
import {
  shouldForceImmediateTeamRealtimeReconcile,
  shouldForceWorkflowRefresh
} from '@/components/beeroom/beeroomRealtimeReconcile';
import {
  resolveSyncRequiredReloadDelayMs,
  shouldRunSyncRequiredReloadImmediately
} from '@/components/beeroom/beeroomRealtimeSyncGap';
import { createBeeroomChatRealtimeRuntime } from '@/realtime/beeroomChatRealtimeRuntime';
import { resolveChatRequestTextInputOverflow } from '@/utils/chatRequestInputLimit';
import { chatDebugLog } from '@/utils/chatDebug';
import { useAgentStore } from '@/stores/agents';
import { useAuthStore } from '@/stores/auth';
import { useChatStore } from '@/stores/chat';
import { replaceMessageArrayKeepingReference } from '@/stores/chatMessageArraySync';
import {
  type BeeroomGroup,
  type BeeroomMember,
  type BeeroomMission,
  useBeeroomStore
} from '@/stores/beeroom';
import { consumeSseStream } from '@/utils/sse';
import {
  DEFAULT_AGENT_AVATAR_IMAGE,
  parseAgentAvatarIconConfig,
  resolveAgentAvatarConfiguredColor,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';
import { PROFILE_AVATAR_OPTION_KEYS, resolveProfileAvatarImageByKey } from '@/utils/avatarCatalog';
import { DEFAULT_AVATAR_COLOR, normalizeAvatarIcon, readUserAppearanceFromStorage } from '@/utils/userPreferences';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

type DispatchSessionTarget = {
  sessionId: string;
  sessionSummary: Record<string, unknown> | null;
};

type DispatchSessionAssistantIdentity = {
  agentId: string;
  name: string;
  tone: MissionChatMessage['tone'];
};

type DispatchMessageRefreshRequest = {
  reason: string;
  sessionId: string;
  hydrate: boolean;
  clearWhenEmpty: boolean;
  forceReplace: boolean;
};

type BeeroomMissionCanvasRuntimeOverrides = {
  runtimeScopeKey?: Ref<string>;
  clearScopeKey?: Ref<string>;
  fixedMotherDispatchSessionId?: Ref<string>;
  lockedComposerTargetAgentId?: Ref<string>;
  disableAutoMotherDispatchReconcile?: boolean;
};

const MANUAL_CHAT_HISTORY_LIMIT = 120;
const CHAT_HEALTH_POLL_INTERVAL_MS = 30_000;
const TEAM_REALTIME_REFRESH_THROTTLE_MS = 360;
const DISPATCH_MESSAGE_REFRESH_THROTTLE_MS = 220;
const SYNC_REQUIRED_HISTORY_RELOAD_THROTTLE_MS = 520;
const TERMINAL_DISPATCH_PREVIEW_STATUSES = new Set(['completed', 'failed', 'cancelled']);
const TEAM_RUNTIME_EVENT_TYPES = new Set([
  'team_start',
  'team_task_dispatch',
  'team_task_update',
  'team_task_result',
  'team_merge',
  'team_finish',
  'team_error'
]);
const DEMO_RUNTIME_EVENT_TYPES = new Set(['beeroom_demo_status']);

const clipDebugText = (value: unknown, limit = 180) => {
  const text = String(value || '').trim().replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const summarizeDebugError = (error: unknown) => {
  const source = error as { name?: unknown; message?: unknown } | null;
  const name = String(source?.name || '').trim();
  const message = String(source?.message || '').trim();
  return [name, message].filter(Boolean).join(': ') || String(error || '').trim();
};

export const useBeeroomMissionCanvasRuntime = (options: {
  group: Ref<BeeroomGroup | null>;
  mission: Ref<BeeroomMission | null>;
  agents: Ref<BeeroomMember[]>;
  t: TranslationFn;
  onRefresh: () => void;
  runtimeOverrides?: BeeroomMissionCanvasRuntimeOverrides;
}) => {
  const agentStore = useAgentStore();
  const authStore = useAuthStore();
  const chatStore = useChatStore();
  const beeroomStore = useBeeroomStore();
  const chatCollapsed = ref(false);
  const manualChatMessages = ref<MissionChatMessage[]>([]);
  const runtimeRelayChatMessages = ref<MissionChatMessage[]>([]);
  const composerText = ref('');
  const composerTargetAgentId = ref('');
  const composerSending = ref(false);
  const composerError = ref('');
  const dispatchSessionId = ref('');
  const dispatchRequestId = ref('');
  const dispatchLastEventId = ref(0);
  const dispatchRuntimeStatus = ref<DispatchRuntimeStatus>('idle');
  const dispatchTargetAgentId = ref('');
  const dispatchTargetName = ref('');
  const dispatchTargetTone = ref<MissionChatMessage['tone']>('worker');
  const dispatchLabelPreview = ref('');
  const dispatchRespondingApprovalId = ref('');
  const chatRealtimeCursor = ref(0);
  const chatMessagesClearedAfter = ref(0);
  const sessionAssistantIdentityBySession = ref<Record<string, DispatchSessionAssistantIdentity>>({});

  let manualMessageSerial = 0;
  let chatAuthDenied = false;
  let teamRealtimeReconcileTimer: number | null = null;
  let lastTeamRealtimeRefreshAt = 0;
  let dispatchMessageRefreshTimer: number | null = null;
  let lastDispatchMessageRefreshAt = 0;
  let pendingDispatchMessageRefresh: DispatchMessageRefreshRequest | null = null;
  let syncRequiredHistoryReloadTimer: number | null = null;
  let lastSyncRequiredHistoryReloadAt = 0;
  let dispatchStreamController: AbortController | null = null;
  let dispatchStopRequested = false;
  let chatRealtimeRuntime:
    | ReturnType<typeof createBeeroomChatRealtimeRuntime>
    | null = null;
  const overrideRuntimeScopeKey = computed(() =>
    String(options.runtimeOverrides?.runtimeScopeKey?.value || '').trim()
  );
  const overrideChatClearScopeKey = computed(() =>
    String(options.runtimeOverrides?.clearScopeKey?.value || '').trim()
  );
  const fixedMotherDispatchSessionId = computed(() =>
    String(options.runtimeOverrides?.fixedMotherDispatchSessionId?.value || '').trim()
  );
  const lockedComposerTargetAgentId = computed(() =>
    String(options.runtimeOverrides?.lockedComposerTargetAgentId?.value || '').trim()
  );
  const disableAutoMotherDispatchReconcile =
    options.runtimeOverrides?.disableAutoMotherDispatchReconcile === true;

  const missionScopeKey = computed(() =>
    resolveBeeroomSwarmScopeKey({
      missionId: options.mission.value?.mission_id,
      teamRunId: options.mission.value?.team_run_id,
      groupId: options.group.value?.group_id
    })
  );
  const activeGroupId = computed(() => String(options.group.value?.group_id || '').trim());
  const chatClearScopeKey = computed(() => {
    if (overrideChatClearScopeKey.value) {
      return overrideChatClearScopeKey.value;
    }
    const groupId = String(activeGroupId.value || '').trim();
    if (groupId) return `chat:${groupId}`;
    return `chat:${missionScopeKey.value}`;
  });
  const chatRuntimeScopeKey = computed(() => {
    if (overrideRuntimeScopeKey.value) {
      return overrideRuntimeScopeKey.value;
    }
    const groupId = String(activeGroupId.value || '').trim();
    if (groupId) return `runtime:${groupId}`;
    return `runtime:${missionScopeKey.value}`;
  });
  const motherAgentId = computed(() =>
    resolveBeeroomMotherAgentId(options.mission.value, options.group.value, options.agents.value)
  );
  const logBeeroomRuntime = (event: string, payload?: unknown) => {
    chatDebugLog('beeroom.runtime', event, payload);
  };

  const {
    motherWorkflowItems,
    workflowItemsByTask,
    workflowItemsSignature,
    workflowPreviewByTask,
    workflowPreviewSignature,
    syncMissionWorkflowState
  } = useBeeroomMissionWorkflowPreview({
    mission: computed(() => options.mission.value || null),
    t: options.t
  });

  const { subagentsByTask, syncMissionSubagentState } = useBeeroomMissionSubagentPreview({
    mission: computed(() => options.mission.value || null),
    clearedAfter: chatMessagesClearedAfter,
    t: options.t
  });

  const { dispatchPreview } = useBeeroomDispatchSessionPreview({
    sessionId: dispatchSessionId,
    targetAgentId: dispatchTargetAgentId,
    targetName: dispatchTargetName,
    runtimeStatus: dispatchRuntimeStatus,
    clearedAfter: chatMessagesClearedAfter,
    t: options.t
  });
  const effectiveDispatchPreview = computed(() =>
    overlayBeeroomLiveDispatchLabel(dispatchPreview.value, {
      currentSessionId: dispatchSessionId.value,
      runtimeStatus: dispatchRuntimeStatus.value,
      composerSending: composerSending.value,
      dispatchLabelPreview: dispatchLabelPreview.value
    })
  );

  const swarmMemberAgentIds = computed(
    () =>
      new Set(
        options.agents.value
          .map((member) => String(member.agent_id || '').trim())
          .filter(Boolean)
      )
  );

  const avatarHydrationAgentIds = computed(() => {
    const ids = new Set<string>();
    const collect = (value: unknown) => {
      const agentId = String(value || '').trim();
      if (!agentId || swarmMemberAgentIds.value.has(agentId)) return;
      if (Object.prototype.hasOwnProperty.call(agentStore.agentMap, agentId)) return;
      ids.add(agentId);
    };
    collect(dispatchTargetAgentId.value);
    collect(dispatchPreview.value?.targetAgentId);
    Object.values(sessionAssistantIdentityBySession.value).forEach((identity) => collect(identity?.agentId));
    (Array.isArray(dispatchPreview.value?.subagents) ? dispatchPreview.value?.subagents : []).forEach((item) =>
      collect(item?.agentId)
    );
    Object.values(subagentsByTask.value || {}).forEach((items) =>
      (Array.isArray(items) ? items : []).forEach((item) => collect(item?.agentId))
    );
    return Array.from(ids);
  });

  watch(
    avatarHydrationAgentIds,
    (agentIds) => {
      agentIds.forEach((agentId) => {
        void agentStore.getAgent(agentId).catch(() => null);
      });
    },
    { immediate: true }
  );

  const {
    demoBusy,
    demoError,
    demoActionLabel,
    demoCanStart,
    demoCanCancel,
    handleDemoAction,
    handleDemoRealtimeEvent
  } = useBeeroomDemo({
    activeGroupId,
    selectedMotherAgentId: composerTargetAgentId,
    t: options.t,
    onRefresh: () => {
      scheduleTeamRealtimeReconcile(true);
      options.onRefresh();
    }
  });

  const demoActionDisabled = computed(() => {
    if (demoCanCancel.value) {
      return demoBusy.value;
    }
    return demoBusy.value || composerSending.value || !demoCanStart.value;
  });

  const currentUserId = computed(
    () =>
      String(
        (authStore.user as Record<string, unknown> | null)?.id ||
          (authStore.user as Record<string, unknown> | null)?.user_id ||
          (authStore.user as Record<string, unknown> | null)?.username ||
          ''
      ).trim()
  );

  const currentUserAvatarIcon = computed(() => {
    const currentUser = (authStore.user || null) as Record<string, unknown> | null;
    const persistedAppearance = readUserAppearanceFromStorage(currentUserId.value, PROFILE_AVATAR_OPTION_KEYS);
    return normalizeAvatarIcon(
      currentUser?.avatar_icon ?? currentUser?.avatarIcon ?? persistedAppearance.avatarIcon,
      PROFILE_AVATAR_OPTION_KEYS
    );
  });

  const currentUserAvatarImageUrl = computed(() => resolveProfileAvatarImageByKey(currentUserAvatarIcon.value));

  const hasExplicitAgentAvatarIcon = (value: unknown): boolean => {
    if (!value) return false;
    if (typeof value === 'string') {
      return value.trim().length > 0;
    }
    if (typeof value !== 'object' || Array.isArray(value)) {
      return false;
    }
    const record = value as Record<string, unknown>;
    return Boolean(String(record.name ?? record.icon ?? record.avatar_icon ?? record.avatarIcon ?? '').trim());
  };

  const agentAvatarImageMap = computed(() => {
    const map = new Map<string, string>();
    options.agents.value.forEach((member) => {
      const agentId = String(member.agent_id || '').trim();
      if (!agentId) return;
      const imageUrl = hasExplicitAgentAvatarIcon(member.icon)
        ? resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig(member.icon))
        : '';
      if (imageUrl) {
        map.set(agentId, imageUrl);
      }
    });
    Object.entries(agentStore.agentMap || {}).forEach(([agentId, agent]) => {
      const normalizedAgentId = String(agentId || '').trim();
      if (!normalizedAgentId || map.has(normalizedAgentId)) return;
      if (normalizedAgentId === DEFAULT_AGENT_KEY) {
        map.set(DEFAULT_AGENT_KEY, DEFAULT_AGENT_AVATAR_IMAGE);
        return;
      }
      const icon =
        agent && typeof agent === 'object' ? (agent as Record<string, unknown>).icon : undefined;
      const imageUrl = hasExplicitAgentAvatarIcon(icon)
        ? resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig(icon))
        : '';
      if (imageUrl) {
        map.set(normalizedAgentId, imageUrl);
      }
    });
    if (!map.has(DEFAULT_AGENT_KEY)) {
      map.set(DEFAULT_AGENT_KEY, DEFAULT_AGENT_AVATAR_IMAGE);
    }
    return map;
  });

  const agentAvatarColorMap = computed(() => {
    const map = new Map<string, string>();
    options.agents.value.forEach((member) => {
      const agentId = String(member.agent_id || '').trim();
      if (!agentId) return;
      const color = resolveAgentAvatarConfiguredColor(member.icon);
      if (color) {
        map.set(agentId, color);
      }
    });
    Object.entries(agentStore.agentMap || {}).forEach(([agentId, agent]) => {
      const normalizedAgentId = String(agentId || '').trim();
      if (!normalizedAgentId || map.has(normalizedAgentId)) return;
      const icon =
        agent && typeof agent === 'object' ? (agent as Record<string, unknown>).icon : undefined;
      const color = resolveAgentAvatarConfiguredColor(icon);
      if (color) {
        map.set(normalizedAgentId, color);
      }
    });
    if (!map.has(DEFAULT_AGENT_KEY)) {
      map.set(DEFAULT_AGENT_KEY, DEFAULT_AVATAR_COLOR);
    }
    return map;
  });

  const silentAgentIdSet = computed(() => {
    const set = new Set<string>();
    options.agents.value.forEach((member) => {
      const agentId = String(member.agent_id || '').trim();
      if (!agentId || member.silent !== true) return;
      set.add(agentId);
    });
    Object.entries(agentStore.agentMap || {}).forEach(([agentId, agent]) => {
      const normalizedAgentId = String(agentId || '').trim();
      if (!normalizedAgentId || !agent || typeof agent !== 'object') return;
      if (Boolean((agent as Record<string, unknown>).silent)) {
        set.add(normalizedAgentId);
      }
    });
    return set;
  });

  const resolveAgentAvatarImageByAgentId = (agentId: unknown): string =>
    agentAvatarImageMap.value.get(String(agentId || '').trim()) || '';

  const resolveAgentAvatarColorByAgentId = (agentId: unknown): string =>
    agentAvatarColorMap.value.get(String(agentId || '').trim()) || '';

  const resolveMessageAvatarImage = (message: MissionChatMessage): string => {
    const explicitAvatarImageUrl = String(message?.avatarImageUrl || '').trim();
    if (explicitAvatarImageUrl) {
      return explicitAvatarImageUrl;
    }
    if (message?.tone === 'user') {
      return currentUserAvatarImageUrl.value;
    }
    const senderAgentId = String(message?.senderAgentId || '').trim();
    if (senderAgentId) {
      const directAvatarImageUrl = resolveAgentAvatarImageByAgentId(senderAgentId);
      if (directAvatarImageUrl) {
        return directAvatarImageUrl;
      }
    }
    if (isBeeroomDefaultAgentLike(message?.senderName)) {
      return resolveAgentAvatarImageByAgentId(DEFAULT_AGENT_KEY) || DEFAULT_AGENT_AVATAR_IMAGE;
    }
    if (String(message?.key || '').startsWith('subagent:') && String(message?.key || '').endsWith(':reply')) {
      return resolveBeeroomProjectedSubagentAvatarImage({
        agentId: senderAgentId,
        name: message?.senderName,
        explicitAvatarImageUrl: '',
        resolveAgentAvatarImageByAgentId,
        defaultAgentAvatarImageUrl: DEFAULT_AGENT_AVATAR_IMAGE,
        fallbackAvatarImageUrl: resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig('avatar-048'))
      });
    }
    return '';
  };

  const avatarLabel = (value: unknown) => resolveAgentAvatarInitial(value);

  const resolveAgentNameById = (agentId: unknown) => {
    const normalized = String(agentId || '').trim();
    if (!normalized) return '';
    if (normalized === DEFAULT_AGENT_KEY) {
      return options.t('messenger.defaultAgent');
    }
    const member = options.agents.value.find((item) => String(item.agent_id || '').trim() === normalized);
    if (member?.name) {
      return String(member.name).trim();
    }
    if (normalized === String(options.group.value?.mother_agent_id || '').trim()) {
      return String(options.group.value?.mother_agent_name || normalized).trim();
    }
    return normalized;
  };

  const normalizeComparableName = (value: unknown) =>
    String(value || '')
      .trim()
      .toLowerCase()
      .replace(/\s+/g, '');

  const normalizeChatActorName = (value: unknown): string => normalizeBeeroomActorName(value, options.t);

  const composerTargetOptions = computed<ComposerTargetOption[]>(() => {
    const seen = new Set<string>();
    const items: ComposerTargetOption[] = [];

    const pushOption = (agentId: string, role: 'mother' | 'worker') => {
      const normalized = String(agentId || '').trim();
      if (!normalized || seen.has(normalized)) return;
      seen.add(normalized);
      const label =
        role === 'mother'
          ? `${resolveAgentNameById(normalized)} (${options.t('beeroom.summary.motherAgent')})`
          : resolveAgentNameById(normalized);
      items.push({ agentId: normalized, label, role });
    };

    if (motherAgentId.value) {
      pushOption(motherAgentId.value, 'mother');
    }
    options.agents.value.forEach((member) => {
      const agentId = String(member.agent_id || '').trim();
      if (!agentId || agentId === motherAgentId.value) return;
      pushOption(agentId, 'worker');
    });
    if (!lockedComposerTargetAgentId.value) {
      return items;
    }
    return items.filter((item) => item.agentId === lockedComposerTargetAgentId.value);
  });

  const composerCanSend = computed(
    () => String(composerText.value || '').trim().length > 0 && composerTargetOptions.value.length > 0
  );

  const dispatchApprovals = computed<DispatchApprovalItem[]>(() => {
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) return [];
    const source = Array.isArray(chatStore.pendingApprovals)
      ? (chatStore.pendingApprovals as Record<string, unknown>[])
      : [];
    return source
      .map((item) => {
        if (!item || typeof item !== 'object') return null;
        const currentSessionId = String(item.session_id || '').trim();
        const approvalId = String(item.approval_id || '').trim();
        if (!approvalId || currentSessionId !== sessionId) return null;
        return {
          approval_id: approvalId,
          session_id: currentSessionId,
          tool: String(item.tool || '').trim(),
          summary: String(item.summary || '').trim()
        } satisfies DispatchApprovalItem;
      })
      .filter((item: DispatchApprovalItem | null): item is DispatchApprovalItem => Boolean(item));
  });

  const dispatchApprovalBusy = computed(() => dispatchRespondingApprovalId.value !== '');
  const dispatchCanStop = computed(() => Boolean(dispatchSessionId.value) && composerSending.value);
  const dispatchCanResume = computed(
    () =>
      Boolean(dispatchSessionId.value) &&
      !composerSending.value &&
      dispatchLastEventId.value > 0 &&
      (dispatchRuntimeStatus.value === 'stopped' || dispatchRuntimeStatus.value === 'failed')
  );
  const dispatchRuntimeLabel = computed(() => {
    const keyMap: Record<DispatchRuntimeStatus, string> = {
      idle: 'beeroom.canvas.chatStandby',
      queued: 'beeroom.status.queued',
      running: 'beeroom.status.running',
      awaiting_approval: 'chat.approval.title',
      resuming: 'chat.message.resume',
      stopped: 'chat.workflow.aborted',
      completed: 'beeroom.status.completed',
      failed: 'beeroom.status.failed'
    };
    return options.t(keyMap[dispatchRuntimeStatus.value] || 'beeroom.status.unknown');
  });
  const dispatchRuntimeTone = computed(() => {
    if (dispatchRuntimeStatus.value === 'failed') return 'danger';
    if (dispatchRuntimeStatus.value === 'completed') return 'success';
    if (dispatchRuntimeStatus.value === 'awaiting_approval') return 'warn';
    if (
      dispatchRuntimeStatus.value === 'queued' ||
      dispatchRuntimeStatus.value === 'running' ||
      dispatchRuntimeStatus.value === 'resuming'
    ) {
      return 'running';
    }
    return 'idle';
  });

  const formatDateTime = (value: unknown) => {
    const numeric = Number(value || 0);
    if (!Number.isFinite(numeric) || numeric <= 0) return '-';
    return new Intl.DateTimeFormat(undefined, {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit'
    }).format(new Date(numeric * 1000));
  };

  const resolveDispatchSessionSummary = (sessionId: string): Record<string, unknown> | null => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return null;
    const match = chatStore.sessions.find((item) => String(item?.id || '').trim() === targetId);
    return match && typeof match === 'object' ? (match as Record<string, unknown>) : null;
  };

  const rememberSessionAssistantIdentity = (
    sessionId: string,
    identity: Partial<DispatchSessionAssistantIdentity> | null | undefined
  ) => {
    const targetId = String(sessionId || '').trim();
    if (!targetId || !identity) return;
    const nextAgentId = String(identity.agentId || '').trim();
    const nextName = normalizeChatActorName(identity.name);
    const nextTone = identity.tone === 'mother' ? 'mother' : 'worker';
    if (!nextAgentId && !nextName) return;
    const current = sessionAssistantIdentityBySession.value[targetId];
    if (
      current?.agentId === nextAgentId &&
      current?.name === nextName &&
      current?.tone === nextTone
    ) {
      return;
    }
    sessionAssistantIdentityBySession.value = {
      ...sessionAssistantIdentityBySession.value,
      [targetId]: {
        agentId: nextAgentId,
        name: nextName,
        tone: nextTone
      }
    };
  };

  const resolveSessionAssistantIdentity = (sessionId: string): DispatchSessionAssistantIdentity | null => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return null;
    return sessionAssistantIdentityBySession.value[targetId] || null;
  };

  const resolveStoredSessionAgentId = (sessionId: string) => {
    const summary = resolveDispatchSessionSummary(sessionId);
    const summaryAgentId = String(summary?.agent_id || '').trim();
    if (summaryAgentId) return summaryAgentId;
    const identity = resolveSessionAssistantIdentity(sessionId);
    if (identity?.agentId) {
      return identity.agentId;
    }
    if (summary?.is_default === true) {
      return DEFAULT_AGENT_KEY;
    }
    const requestedAgentId = String(summary?.beeroom_requested_agent_id || '').trim();
    if (requestedAgentId) return requestedAgentId;
    if (String(sessionId || '').trim() === String(dispatchSessionId.value || '').trim()) {
      const explicitAgentId = String(dispatchTargetAgentId.value || '').trim();
      if (explicitAgentId) {
        return explicitAgentId;
      }
    }
    return '';
  };

  const resolveDispatchAssistantAgentId = (sessionId: string) => {
    const stored = resolveStoredSessionAgentId(sessionId);
    if (stored) return stored;
    if (String(sessionId || '').trim() !== String(dispatchSessionId.value || '').trim()) {
      return '';
    }
    const explicit = String(dispatchTargetAgentId.value || '').trim();
    if (explicit) return explicit;
    return '';
  };

  const resolveDispatchAssistantName = (sessionId: string) => {
    const identity = resolveSessionAssistantIdentity(sessionId);
    if (identity?.name) return identity.name;
    if (String(sessionId || '').trim() === String(dispatchSessionId.value || '').trim()) {
      const explicitName = normalizeChatActorName(dispatchTargetName.value);
      if (explicitName) return explicitName;
    }
    const summary = resolveDispatchSessionSummary(sessionId);
    const requestedName = normalizeChatActorName(summary?.beeroom_target_name);
    if (requestedName) return requestedName;
    const agentId = resolveDispatchAssistantAgentId(sessionId);
    if (agentId === DEFAULT_AGENT_KEY) {
      return options.t('messenger.defaultAgent');
    }
    if (agentId) {
      const agentName = normalizeChatActorName(resolveAgentNameById(agentId));
      if (agentName) return agentName;
    }
    return options.t('messenger.defaultAgent');
  };

  const resolveDispatchAssistantTone = (
    sessionId: string,
    fallback: MissionChatMessage['tone'] = 'worker'
  ): MissionChatMessage['tone'] => {
    const identity = resolveSessionAssistantIdentity(sessionId);
    if (identity?.tone === 'mother') return 'mother';
    if (identity?.tone === 'worker') return 'worker';
    if (String(sessionId || '').trim() === String(dispatchSessionId.value || '').trim()) {
      return dispatchTargetTone.value === 'mother' ? 'mother' : 'worker';
    }
    const agentId = resolveDispatchAssistantAgentId(sessionId);
    if (agentId && agentId === String(motherAgentId.value || '').trim()) {
      return 'mother';
    }
    return fallback;
  };

  const mapSessionChatMessage = (
    value: unknown,
    index: number,
    sessionId: string
  ): MissionChatMessage | null => {
    if (!value || typeof value !== 'object') return null;
    const payload = value as Record<string, unknown>;
    if (payload.isGreeting === true || payload.hiddenInternal === true) {
      return null;
    }
    const role = String(payload.role || '').trim().toLowerCase();
    if (role !== 'user' && role !== 'assistant') {
      return null;
    }
    const body = String(payload.content || '').trim();
    if (!body) return null;
    const timeMs = toSessionTimestampMs(
      payload.created_at ?? payload.createdAt ?? payload.updated_at ?? payload.updatedAt ?? payload.time
    );
    if (!Number.isFinite(timeMs) || timeMs <= 0) return null;
    const time = Math.floor(timeMs / 1000);
    const historyId = String(payload.history_id ?? payload.historyId ?? '').trim();
    const streamEventId = normalizeStreamEventId(payload.stream_event_id ?? payload.streamEventId);
    const key =
      historyId ||
      (streamEventId > 0 ? `event:${streamEventId}` : `message:${role}:${time}:${index}`);
    return {
      key: `session:${sessionId}:${key}`,
      senderName:
        role === 'assistant' ? resolveDispatchAssistantName(sessionId) : options.t('chat.message.user'),
      senderAgentId: role === 'assistant' ? resolveDispatchAssistantAgentId(sessionId) : '',
      mention:
        role === 'assistant'
          ? options.t('chat.message.user')
          : resolveDispatchAssistantName(sessionId),
      body,
      meta: '',
      time,
      timeLabel: formatDateTime(time),
      tone: role === 'assistant' ? resolveDispatchAssistantTone(sessionId) : 'user'
    };
  };

  const readDispatchSessionMessages = (sessionId: string): MissionChatMessage[] => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return [];
    const activeSessionId = String(chatStore.activeSessionId || '').trim();
    const activeSource =
      activeSessionId === targetId && Array.isArray(chatStore.messages) ? chatStore.messages : [];
    const cachedSource = chatStore.getCachedSessionMessages(targetId);
    const preferCached =
      activeSessionId === targetId && shouldPreferCachedDispatchMessages(activeSource, cachedSource);
    const source =
      activeSessionId === targetId && !preferCached ? activeSource : cachedSource;
    logBeeroomRuntime('read-dispatch-session-messages', {
      sessionId: targetId,
      activeSessionId,
      activeCount: Array.isArray(activeSource) ? activeSource.length : 0,
      cachedCount: Array.isArray(cachedSource) ? cachedSource.length : 0,
      source: source === cachedSource ? 'cache' : 'active',
      preferCached
    });
    if (source === cachedSource) {
      syncActiveDispatchSourceFromCache(targetId, cachedSource);
    }
    const mapped = (Array.isArray(source) ? source : [])
      .map((message, index) => mapSessionChatMessage(message, index, targetId))
      .filter((message: MissionChatMessage | null): message is MissionChatMessage => Boolean(message));
    const collapsed = collapseMissionChatAssistantTurns(mapped);
    if (collapsed.length !== mapped.length) {
      logBeeroomRuntime('read-dispatch-session-messages:collapse-assistant-turns', {
        sessionId: targetId,
        sourceCount: mapped.length,
        visibleCount: collapsed.length
      });
    }
    return collapsed;
  };

  const nextManualMessageKey = (prefix: string) => {
    manualMessageSerial += 1;
    return `${prefix}:${Date.now()}:${manualMessageSerial}`;
  };

  const appendManualChatMessage = (message: MissionChatMessage) => {
    if (chatMessagesClearedAfter.value && Number(message.time || 0) <= chatMessagesClearedAfter.value) {
      return;
    }
    const current = Array.isArray(manualChatMessages.value) ? manualChatMessages.value : [];
    const existingIndex = current.findIndex((item) => item.key === message.key);
    const merged =
      existingIndex >= 0
        ? current.map((item, index) => (index === existingIndex ? message : item))
        : [...current, message];
    manualChatMessages.value = merged
      .sort(compareMissionChatMessages)
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    persistCachedChatState();
  };

  const replaceManualChatMessages = (messages: MissionChatMessage[]) => {
    manualChatMessages.value = [...messages]
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      )
      .sort(compareMissionChatMessages)
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    persistCachedChatState();
  };

  const replaceRuntimeRelayChatMessages = (messages: MissionChatMessage[]) => {
    runtimeRelayChatMessages.value = filterBeeroomRuntimeRelayMessagesAfter(
      messages,
      chatMessagesClearedAfter.value
    ).slice(-MANUAL_CHAT_HISTORY_LIMIT);
    persistCachedChatState();
  };

  const mergeRuntimeRelayChatMessages = (messages: MissionChatMessage[]) => {
    if (!messages.length) return;
    const next = mergeBeeroomRuntimeRelayMessages(
      runtimeRelayChatMessages.value,
      filterBeeroomRuntimeRelayMessagesAfter(messages, chatMessagesClearedAfter.value),
      MANUAL_CHAT_HISTORY_LIMIT
    );
    if (!sameManualChatMessages(runtimeRelayChatMessages.value, next)) {
      runtimeRelayChatMessages.value = next;
      persistCachedChatState();
    }
  };

  const sameManualChatMessages = (left: MissionChatMessage[], right: MissionChatMessage[]) => {
    if (left.length !== right.length) return false;
    for (let index = 0; index < left.length; index += 1) {
      const leftItem = left[index];
      const rightItem = right[index];
      if (
        !leftItem ||
        !rightItem ||
        leftItem.key !== rightItem.key ||
        leftItem.time !== rightItem.time ||
        leftItem.tone !== rightItem.tone ||
        leftItem.senderName !== rightItem.senderName ||
        leftItem.senderAgentId !== rightItem.senderAgentId ||
        String(leftItem.avatarImageUrl || '').trim() !== String(rightItem.avatarImageUrl || '').trim() ||
        leftItem.mention !== rightItem.mention ||
        leftItem.body !== rightItem.body ||
        leftItem.meta !== rightItem.meta ||
        Number(leftItem.sortOrder || 0) !== Number(rightItem.sortOrder || 0)
      ) {
        return false;
      }
    }
    return true;
  };

  const hasSessionScopedMessageFor = (messages: MissionChatMessage[], sessionId: string) => {
    const prefix = `session:${String(sessionId || '').trim()}:`;
    return messages.some((message) => String(message?.key || '').startsWith(prefix));
  };

  const hasAnySessionScopedMessage = (messages: MissionChatMessage[]) =>
    messages.some((message) => String(message?.key || '').startsWith('session:'));

  const resolveVisibleChatFreshness = (messages: MissionChatMessage[]) => {
    let lastTime = 0;
    let lastAssistantTime = 0;
    let sessionScopedCount = 0;
    messages.forEach((message) => {
      const time = Number(message?.time || 0);
      if (Number.isFinite(time) && time > lastTime) {
        lastTime = time;
      }
      if (message?.tone !== 'user' && Number.isFinite(time) && time > lastAssistantTime) {
        lastAssistantTime = time;
      }
      if (String(message?.key || '').startsWith('session:')) {
        sessionScopedCount += 1;
      }
    });
    return {
      length: messages.length,
      lastTime,
      lastAssistantTime,
      sessionScopedCount
    };
  };

  const shouldAcceptSessionChatMessages = (
    current: MissionChatMessage[],
    incoming: MissionChatMessage[],
    sessionId: string,
    forceReplace = false
  ) => {
    if (forceReplace) return true;
    if (incoming.length === 0) return current.length === 0;
    if (current.length === 0) return true;
    if (
      hasAnySessionScopedMessage(current) &&
      !hasSessionScopedMessageFor(current, sessionId) &&
      hasSessionScopedMessageFor(incoming, sessionId)
    ) {
      return true;
    }
    const currentFreshness = resolveVisibleChatFreshness(current);
    const incomingFreshness = resolveVisibleChatFreshness(incoming);
    if (incomingFreshness.lastTime > currentFreshness.lastTime) {
      return true;
    }
    if (incomingFreshness.lastAssistantTime > currentFreshness.lastAssistantTime) {
      return true;
    }
    if (
      incomingFreshness.lastTime === currentFreshness.lastTime &&
      incomingFreshness.length >= currentFreshness.length
    ) {
      return true;
    }
    if (
      incomingFreshness.sessionScopedCount > currentFreshness.sessionScopedCount &&
      incomingFreshness.lastTime >= currentFreshness.lastTime
    ) {
      return true;
    }
    return false;
  };

  const resolveSessionScopedAssistantMessages = (messages: MissionChatMessage[], sessionId: string) => {
    const prefix = `session:${String(sessionId || '').trim()}:`;
    return messages.filter(
      (message) => message?.tone !== 'user' && String(message?.key || '').startsWith(prefix)
    );
  };

  const buildSessionAssistantSignature = (messages: MissionChatMessage[], sessionId: string) => {
    const assistants = resolveSessionScopedAssistantMessages(messages, sessionId);
    const last = assistants[assistants.length - 1] || null;
    return [
      assistants.length,
      Number(last?.time || 0),
      String(last?.key || '').trim(),
      String(last?.body || '').trim()
    ].join('|');
  };

  const waitForBeeroomHydrationDelay = (delayMs: number) =>
    new Promise<void>((resolve) => {
      window.setTimeout(resolve, Math.max(0, Math.floor(delayMs)));
    });

  const readCachedChatState = (scopeKey = chatRuntimeScopeKey.value) => getBeeroomMissionChatState(scopeKey);

  const persistCachedChatState = (scopeKey = chatRuntimeScopeKey.value) => {
    setBeeroomMissionChatState(scopeKey, {
      version: 2,
      manualMessages: manualChatMessages.value,
      runtimeRelayMessages: runtimeRelayChatMessages.value,
      dispatch: dispatchSessionId.value
        ? {
            sessionId: String(dispatchSessionId.value || '').trim(),
            lastEventId: Math.max(0, Number(dispatchLastEventId.value || 0)),
            targetAgentId: String(dispatchTargetAgentId.value || '').trim(),
            targetName: String(dispatchTargetName.value || '').trim(),
            targetTone: dispatchTargetTone.value,
            runtimeStatus: dispatchRuntimeStatus.value
          }
        : null
    });
  };

  const restoreCachedChatState = (scopeKey = chatRuntimeScopeKey.value) => {
    const cached = readCachedChatState(scopeKey);
    const cachedMessages = Array.isArray(cached?.manualMessages) ? cached.manualMessages : [];
    const cachedRuntimeRelayMessages = Array.isArray(cached?.runtimeRelayMessages)
      ? cached.runtimeRelayMessages
      : [];
    replaceManualChatMessages(cachedMessages);
    replaceRuntimeRelayChatMessages(cachedRuntimeRelayMessages);
    const cachedDispatch = cached?.dispatch;
    if (!cachedDispatch) {
      logBeeroomRuntime('restore-cached-chat-state', {
        scopeKey,
        manualCount: cachedMessages.length,
        relayCount: runtimeRelayChatMessages.value.length,
        dispatchSessionId: '',
        dispatchTargetAgentId: '',
        runtimeStatus: 'idle'
      });
      return;
    }
    dispatchSessionId.value = String(cachedDispatch.sessionId || '').trim();
    dispatchLastEventId.value = Math.max(0, Number(cachedDispatch.lastEventId || 0));
    dispatchTargetAgentId.value = String(cachedDispatch.targetAgentId || '').trim();
    dispatchTargetName.value = String(cachedDispatch.targetName || '').trim();
    dispatchTargetTone.value =
      cachedDispatch.targetTone === 'mother' ? 'mother' : 'worker';
    dispatchRuntimeStatus.value = cachedDispatch.runtimeStatus || 'idle';
    rememberSessionAssistantIdentity(dispatchSessionId.value, {
      agentId: dispatchTargetAgentId.value,
      name: dispatchTargetName.value,
      tone: dispatchTargetTone.value
    });
    logBeeroomRuntime('restore-cached-chat-state', {
      scopeKey,
      manualCount: cachedMessages.length,
      relayCount: runtimeRelayChatMessages.value.length,
      dispatchSessionId: dispatchSessionId.value,
      dispatchLastEventId: dispatchLastEventId.value,
      dispatchTargetAgentId: dispatchTargetAgentId.value,
      dispatchTargetTone: dispatchTargetTone.value,
      runtimeStatus: dispatchRuntimeStatus.value
    });
  };

  const resolveLatestVisibleUserPreview = () => {
    const currentDispatchLabel = String(dispatchLabelPreview.value || '').trim();
    if (currentDispatchLabel) {
      return currentDispatchLabel;
    }
    const hit = [...manualChatMessages.value]
      .reverse()
      .find((message) => message?.tone === 'user' && String(message.body || '').trim());
    return String(hit?.body || '').trim();
  };

  const ensureDispatchSessionKnown = (sessionId: string, remember = false) => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return;
    syncDispatchSessionToChatStore(
      {
        sessionId: targetId,
        agentId: dispatchTargetAgentId.value,
        agentName: dispatchTargetName.value,
        targetTone: dispatchTargetTone.value,
        sessionSummary: resolveDispatchSessionSummary(targetId),
        userPreview: resolveLatestVisibleUserPreview()
      },
      { remember }
    );
  };

  const syncDispatchSessionMessages = async (
    loadOptions: { hydrate?: boolean; clearWhenEmpty?: boolean; forceReplace?: boolean } = {}
  ) => {
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) {
      logBeeroomRuntime('sync-dispatch-session-messages:no-session', {
        clearWhenEmpty: loadOptions.clearWhenEmpty === true,
        hydrate: loadOptions.hydrate !== false,
        forceReplace: loadOptions.forceReplace === true
      });
      if (loadOptions.clearWhenEmpty) {
        replaceManualChatMessages([]);
      }
      return [];
    }

    const applyFromCache = () => {
      const sessionBackedMessages = readDispatchSessionMessages(sessionId)
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
      const next = reconcileBeeroomSessionBackedManualMessages({
        current: manualChatMessages.value,
        incoming: sessionBackedMessages,
        sessionId,
        limit: MANUAL_CHAT_HISTORY_LIMIT
      });
      if (next.length > 0) {
        if (
          !sameManualChatMessages(manualChatMessages.value, next) &&
          shouldAcceptSessionChatMessages(
            manualChatMessages.value,
            next,
            sessionId,
            loadOptions.forceReplace === true
          )
        ) {
          replaceManualChatMessages(next);
        }
      } else if (loadOptions.clearWhenEmpty) {
        replaceManualChatMessages([]);
      }
      return next;
    };

    ensureDispatchSessionKnown(sessionId, false);
    logBeeroomRuntime('sync-dispatch-session-messages:start', {
      sessionId,
      hydrate: loadOptions.hydrate !== false,
      clearWhenEmpty: loadOptions.clearWhenEmpty === true,
      forceReplace: loadOptions.forceReplace === true
    });
    const cached = applyFromCache();
    logBeeroomRuntime('sync-dispatch-session-messages:cache', {
      sessionId,
      messageCount: cached.length
    });
    if (loadOptions.hydrate === false) {
      return cached;
    }
    try {
      await chatStore.preloadSessionDetail(sessionId, { force: true, syncActive: true });
    } catch (error) {
      logBeeroomRuntime('sync-dispatch-session-messages:hydrate-error', {
        sessionId,
        error: summarizeDebugError(error)
      });
      return cached;
    }
    const hydrated = applyFromCache();
    logBeeroomRuntime('sync-dispatch-session-messages:hydrated', {
      sessionId,
      messageCount: hydrated.length
    });
    return hydrated;
  };

  const collectDispatchSessionMessages = async (
    sessionId: string,
    loadOptions: { hydrate?: boolean } = {}
  ): Promise<MissionChatMessage[]> => {
    const targetSessionId = String(sessionId || '').trim();
    if (!targetSessionId) {
      return [];
    }
    ensureDispatchSessionKnown(targetSessionId, false);
    const collectFromCache = () =>
      readDispatchSessionMessages(targetSessionId)
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    const cached = collectFromCache();
    if (loadOptions.hydrate === false) {
      return cached;
    }
    try {
      await chatStore.preloadSessionDetail(targetSessionId, { force: true, syncActive: true });
    } catch (error) {
      logBeeroomRuntime('collect-dispatch-session-messages:hydrate-error', {
        sessionId: targetSessionId,
        error: summarizeDebugError(error)
      });
      return cached;
    }
    return collectFromCache();
  };

  const hydrateTerminalDispatchSessionMessages = async (options: {
    sessionId: string;
    expectedReplyText?: string;
    baselineAssistantSignature?: string;
  }) => {
    const targetSessionId = String(options.sessionId || '').trim();
    if (!targetSessionId) return [];
    const expectedReplyText = String(options.expectedReplyText || '').trim();
    const baselineAssistantSignature = String(options.baselineAssistantSignature || '').trim();
    const attemptDelaysMs = [0, 180, 520, 1100];
    let latestMessages: MissionChatMessage[] = [];
    for (let attemptIndex = 0; attemptIndex < attemptDelaysMs.length; attemptIndex += 1) {
      if (attemptDelaysMs[attemptIndex] > 0) {
        await waitForBeeroomHydrationDelay(attemptDelaysMs[attemptIndex]);
      }
      if (String(dispatchSessionId.value || '').trim() !== targetSessionId) {
        logBeeroomRuntime('hydrate-terminal-dispatch-session:session-switched', {
          targetSessionId,
          currentSessionId: dispatchSessionId.value,
          attemptIndex
        });
        return latestMessages;
      }
      latestMessages = await collectDispatchSessionMessages(targetSessionId, {
        hydrate: true
      });
      const assistantSignature = buildSessionAssistantSignature(latestMessages, targetSessionId);
      const hasExpectedReply =
        !!expectedReplyText &&
        resolveSessionScopedAssistantMessages(latestMessages, targetSessionId).some(
          (message) => String(message?.body || '').trim() === expectedReplyText
        );
      const shouldFinishHydration = shouldFinishBeeroomTerminalHydration({
        expectedReplyText,
        expectedReplyMatched: hasExpectedReply,
        baselineAssistantSignature,
        assistantSignature
      });
      logBeeroomRuntime('hydrate-terminal-dispatch-session:attempt', {
        sessionId: targetSessionId,
        attemptIndex,
        messageCount: latestMessages.length,
        expectedReplyMatched: hasExpectedReply,
        baselineAssistantSignature,
        assistantSignature,
        shouldFinishHydration
      });
      if (shouldFinishHydration) {
        replaceManualChatMessages(latestMessages);
        return latestMessages;
      }
    }
    logBeeroomRuntime('hydrate-terminal-dispatch-session:preserve-local-final-reply', {
      sessionId: targetSessionId,
      expectedReplyText: clipDebugText(expectedReplyText),
      finalAssistantSignature: buildSessionAssistantSignature(latestMessages, targetSessionId)
    });
    return latestMessages;
  };

  const loadManualChatHistory = async () => {
    await reconcileMotherDispatchSession({ hydrate: false, syncMessages: false });
    const cachedState = readCachedChatState();
    const cachedMessages = Array.isArray(cachedState?.manualMessages) ? cachedState.manualMessages : [];
    const cachedDispatchSessionId = String(cachedState?.dispatch?.sessionId || '').trim();
    const currentDispatchSessionId = String(dispatchSessionId.value || '').trim();
    const cachedSessionMatchesCurrent =
      !currentDispatchSessionId || !cachedDispatchSessionId || cachedDispatchSessionId === currentDispatchSessionId;
    if (!String(dispatchSessionId.value || '').trim()) {
      const next = [...cachedMessages]
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .sort(compareMissionChatMessages)
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
      if (!sameManualChatMessages(manualChatMessages.value, next)) {
        replaceManualChatMessages(next);
      }
      logBeeroomRuntime('load-manual-chat-history:cached-only', {
        scopeKey: chatRuntimeScopeKey.value,
        messageCount: next.length
      });
      return;
    }
    if (
      !sameManualChatMessages(manualChatMessages.value, cachedMessages) &&
      cachedMessages.length > 0 &&
      cachedSessionMatchesCurrent
    ) {
      replaceManualChatMessages(cachedMessages);
    } else if (!cachedSessionMatchesCurrent && hasAnySessionScopedMessage(manualChatMessages.value)) {
      replaceManualChatMessages([]);
    }
    logBeeroomRuntime('load-manual-chat-history:dispatch', {
      scopeKey: chatRuntimeScopeKey.value,
      sessionId: dispatchSessionId.value,
      cachedCount: cachedMessages.length,
      cachedDispatchSessionId,
      cachedSessionMatchesCurrent
    });
    await syncDispatchSessionMessages({ hydrate: true });
  };

  const clearManualChatHistory = async () => {
    composerError.value = '';
    const clearedAfter = Date.now() / 1000;
    chatMessagesClearedAfter.value = Math.max(chatMessagesClearedAfter.value, clearedAfter);
    manualChatMessages.value = [];
    replaceRuntimeRelayChatMessages([]);
    mergeBeeroomMissionCanvasState(chatClearScopeKey.value, {
      chatClearedAfter: chatMessagesClearedAfter.value
    });
  };

  const clipMessageBody = (value: unknown, limit = 240) => {
    const text = String(value || '').trim();
    if (!text) return '';
    if (text.length <= limit) return text;
    return `${text.slice(0, limit)}...`;
  };

  const safeJsonParse = (value: unknown) => {
    try {
      return JSON.parse(String(value || '')) as Record<string, any>;
    } catch {
      return null;
    }
  };

  const extractReplyText = (payload: Record<string, any> | null) => {
    const candidates = [
      payload?.content,
      payload?.reply,
      payload?.message,
      payload?.text,
      payload?.data?.content,
      payload?.data?.reply,
      payload?.data?.message,
      payload?.data?.text,
      payload?.data?.final_reply,
      payload?.final_reply
    ];
    return String(candidates.find((item) => String(item || '').trim()) || '').trim();
  };

  const extractErrorText = (payload: Record<string, any> | null) => {
    const candidates = [
      payload?.detail,
      payload?.error,
      payload?.message,
      payload?.data?.detail,
      payload?.data?.error,
      payload?.data?.message
    ];
    return String(candidates.find((item) => String(item || '').trim()) || '').trim();
  };

  const resolveDispatchTarget = (rawContent: string) => {
    const text = String(rawContent || '').trim();
    const explicitMatch = text.match(/^@([^\s]+)\s*(.*)$/);
    const normalizedExplicit = normalizeComparableName(explicitMatch?.[1] || '');
    const explicitTarget = normalizedExplicit
      ? composerTargetOptions.value.find((item) => {
          const optionName = normalizeComparableName(resolveAgentNameById(item.agentId));
          const optionLabel = normalizeComparableName(item.label.replace(/\(.+?\)/g, ''));
          return optionName === normalizedExplicit || optionLabel === normalizedExplicit;
        }) || null
      : null;
    const target =
      explicitTarget ||
      composerTargetOptions.value.find((item) => item.agentId === composerTargetAgentId.value) ||
      composerTargetOptions.value[0] ||
      null;
    return {
      target,
      body: String(explicitMatch?.[2] || text).trim() || text
    };
  };

  const normalizeDispatchAgentId = (agentId: string): string =>
    agentId === DEFAULT_AGENT_KEY ? '' : String(agentId || '').trim();

  const toSessionTimestampMs = (value: unknown): number => {
    if (value === null || value === undefined) return 0;
    if (typeof value === 'number') {
      if (!Number.isFinite(value)) return 0;
      return value < 1_000_000_000_000 ? value * 1000 : value;
    }
    const text = String(value).trim();
    if (!text) return 0;
    if (/^-?\d+(\.\d+)?$/.test(text)) {
      const numeric = Number(text);
      if (!Number.isFinite(numeric)) return 0;
      return numeric < 1_000_000_000_000 ? numeric * 1000 : numeric;
    }
    const parsed = new Date(text).getTime();
    return Number.isNaN(parsed) ? 0 : parsed;
  };

  const resolveSessionMessageFreshness = (messages: unknown[]) => {
    let lastEventId = 0;
    let lastHistoryId = 0;
    let lastTimeMs = 0;
    (Array.isArray(messages) ? messages : []).forEach((message) => {
      if (!message || typeof message !== 'object') return;
      const payload = message as Record<string, unknown>;
      const eventId = normalizeStreamEventId(payload.stream_event_id ?? payload.streamEventId);
      if (eventId > lastEventId) {
        lastEventId = eventId;
      }
      const historyId = Number.parseInt(String(payload.history_id ?? payload.historyId ?? '').trim(), 10);
      if (Number.isFinite(historyId) && historyId > lastHistoryId) {
        lastHistoryId = historyId;
      }
      const timeMs = toSessionTimestampMs(
        payload.created_at ?? payload.createdAt ?? payload.updated_at ?? payload.updatedAt ?? payload.time
      );
      if (timeMs > lastTimeMs) {
        lastTimeMs = timeMs;
      }
    });
    return {
      length: Array.isArray(messages) ? messages.length : 0,
      lastEventId,
      lastHistoryId,
      lastTimeMs
    };
  };

  const shouldPreferCachedDispatchMessages = (activeSource: unknown[], cachedSource: unknown[]) => {
    if (!Array.isArray(cachedSource) || cachedSource.length === 0) return false;
    if (!Array.isArray(activeSource) || activeSource.length === 0) return true;
    const activeFreshness = resolveSessionMessageFreshness(activeSource);
    const cachedFreshness = resolveSessionMessageFreshness(cachedSource);
    if (cachedFreshness.lastEventId > activeFreshness.lastEventId) return true;
    if (cachedFreshness.lastHistoryId > activeFreshness.lastHistoryId) return true;
    if (cachedFreshness.lastTimeMs > activeFreshness.lastTimeMs + 1000) return true;
    if (
      cachedFreshness.length > activeFreshness.length &&
      cachedFreshness.lastTimeMs >= activeFreshness.lastTimeMs
    ) {
      return true;
    }
    return false;
  };

  const syncActiveDispatchSourceFromCache = (sessionId: string, source: unknown[]) => {
    const targetId = String(sessionId || '').trim();
    if (!targetId || String(chatStore.activeSessionId || '').trim() !== targetId) return;
    if (!Array.isArray(source) || source.length === 0) return;
    const activeSource = Array.isArray(chatStore.messages) ? chatStore.messages : null;
    if (!activeSource || activeSource === source) return;
    replaceMessageArrayKeepingReference(
      activeSource as Record<string, any>[],
      source as Record<string, any>[]
    );
  };

  const resolveValidDispatchSessionId = (
    sessionId: string,
    sessionSummary: Record<string, unknown> | null = null
  ) => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return '';
    const summary = sessionSummary || resolveDispatchSessionSummary(targetId);
    const status = String(summary?.status || '').trim().toLowerCase();
    return status === 'archived' ? '' : targetId;
  };

  const resolveActiveDispatchSessionId = (agentId: string) => {
    const activeSessionId = String(chatStore.activeSessionId || '').trim();
    if (!activeSessionId) return '';
    const activeAgentId = resolveStoredSessionAgentId(activeSessionId);
    if (activeAgentId !== agentId) return '';
    return resolveValidDispatchSessionId(activeSessionId);
  };

  const resolvePrimaryDispatchSessionId = (
    agentId: string,
    sourceSessions: Record<string, any>[] | null = null
  ) => {
    const normalizedAgentId = String(agentId || '').trim();
    if (!normalizedAgentId) return '';
    const sessions = Array.isArray(sourceSessions) ? sourceSessions : chatStore.getCachedSessions(normalizedAgentId);
    const primarySessionId = String(chatStore.resolveInitialSessionId(normalizedAgentId, sessions) || '').trim();
    if (!primarySessionId) return '';
    const primarySummary =
      (Array.isArray(sessions)
        ? sessions.find((item) => String(item?.id || '').trim() === primarySessionId) || null
        : null) as Record<string, unknown> | null;
    return resolveValidDispatchSessionId(primarySessionId, primarySummary);
  };

  const resolveExplicitMainDispatchSessionId = (
    agentId: string,
    sourceSessions: Record<string, any>[] | null = null
  ) => {
    const normalizedAgentId = String(agentId || '').trim();
    if (!normalizedAgentId) return '';
    const sessions = Array.isArray(sourceSessions) ? sourceSessions : chatStore.getCachedSessions(normalizedAgentId);
    const mainSession = Array.isArray(sessions)
      ? sessions.find((item) => item?.is_main === true) || null
      : null;
    if (!mainSession?.id) return '';
    return resolveValidDispatchSessionId(String(mainSession.id), mainSession as Record<string, unknown>);
  };

  const resolvePreferredDispatchSessionId = (
    target: ComposerTargetOption,
    previousSessionId: string,
    previousTargetAgentId: string
  ) => {
    if (
      fixedMotherDispatchSessionId.value &&
      target.role === 'mother' &&
      target.agentId === String(motherAgentId.value || '').trim()
    ) {
      return fixedMotherDispatchSessionId.value;
    }
    const activeSessionId = resolveActiveDispatchSessionId(target.agentId);
    const primarySessionId = resolvePrimaryDispatchSessionId(target.agentId);
    const explicitPrimarySessionId =
      target.role === 'mother' && target.agentId === String(motherAgentId.value || '').trim()
        ? fixedMotherDispatchSessionId.value || resolveExplicitMainDispatchSessionId(target.agentId)
        : resolveExplicitMainDispatchSessionId(target.agentId);
    const resolvedSessionId = resolvePreferredBeeroomDispatchSessionId({
      targetRole: target.role,
      targetAgentId: target.agentId,
      previousSessionId,
      previousTargetAgentId,
      activeSessionId,
      primarySessionId,
      hasExplicitPrimarySession: Boolean(explicitPrimarySessionId)
    });
    logBeeroomRuntime('resolve-preferred-dispatch-session', {
      targetAgentId: target.agentId,
      targetRole: target.role,
      previousSessionId,
      previousTargetAgentId,
      activeSessionId,
      primarySessionId,
      explicitPrimarySessionId,
      resolvedSessionId
    });
    return resolvedSessionId;
  };

  const syncDispatchSessionToMessenger = async (sessionId: string, agentId: string) => {
    const targetSessionId = String(sessionId || '').trim();
    const targetAgentId = String(agentId || '').trim();
    if (!targetSessionId) return;
    const activeSessionId = String(chatStore.activeSessionId || '').trim();
    const activeAgentId = activeSessionId ? resolveStoredSessionAgentId(activeSessionId) : '';
    let syncPath = 'preload-background';
    if (activeSessionId === targetSessionId) {
      syncPath = 'preload-active';
      logBeeroomRuntime('sync-dispatch-session-to-messenger', {
        sessionId: targetSessionId,
        agentId: targetAgentId,
        activeSessionId,
        activeAgentId,
        syncPath
      });
      await chatStore.preloadSessionDetail(targetSessionId, { force: true, syncActive: true });
      return;
    }
    if (targetAgentId && activeAgentId === targetAgentId) {
      syncPath = 'load-switch-active-agent';
      logBeeroomRuntime('sync-dispatch-session-to-messenger', {
        sessionId: targetSessionId,
        agentId: targetAgentId,
        activeSessionId,
        activeAgentId,
        syncPath
      });
      await chatStore.loadSessionDetail(targetSessionId, { preserveWatcher: true });
      return;
    }
    logBeeroomRuntime('sync-dispatch-session-to-messenger', {
      sessionId: targetSessionId,
      agentId: targetAgentId,
      activeSessionId,
      activeAgentId,
      syncPath
    });
    await chatStore.preloadSessionDetail(targetSessionId, { force: true, syncActive: false });
  };

  const reconcileMotherDispatchSession = async (
    syncOptions: { hydrate?: boolean; syncMessages?: boolean } = {}
  ) => {
    if (disableAutoMotherDispatchReconcile) {
      return;
    }
    if (composerSending.value) {
      logBeeroomRuntime('reconcile-mother-dispatch-session:skip-sending', {
        dispatchSessionId: dispatchSessionId.value,
        targetAgentId: dispatchTargetAgentId.value
      });
      return;
    }
    const cachedTargetAgentId = String(dispatchTargetAgentId.value || '').trim();
    const resolvedMotherAgentId = String(motherAgentId.value || cachedTargetAgentId || '').trim();
    const isMotherTarget =
      dispatchTargetTone.value === 'mother' ||
      (resolvedMotherAgentId && cachedTargetAgentId === resolvedMotherAgentId);
    if (!isMotherTarget || !resolvedMotherAgentId) return;
    const currentSessionId = String(dispatchSessionId.value || '').trim();
    const currentValidSessionId = resolveValidDispatchSessionId(currentSessionId);
    const explicitPrimarySessionId =
      fixedMotherDispatchSessionId.value || resolveExplicitMainDispatchSessionId(resolvedMotherAgentId);
    const fallbackPrimarySessionId = resolvePrimaryDispatchSessionId(resolvedMotherAgentId);
    const nextSessionId = resolveNextBeeroomMotherDispatchSessionId({
      motherAgentId: resolvedMotherAgentId,
      currentSessionId: currentValidSessionId,
      currentSessionAgentId: currentValidSessionId ? resolveStoredSessionAgentId(currentValidSessionId) : '',
      explicitPrimarySessionId,
      fallbackPrimarySessionId
    });
    if (!nextSessionId) {
      logBeeroomRuntime('reconcile-mother-dispatch-session:no-primary', {
        motherAgentId: resolvedMotherAgentId,
        currentSessionId,
        explicitPrimarySessionId,
        fallbackPrimarySessionId
      });
      return;
    }
    if (nextSessionId === currentSessionId) return;
    logBeeroomRuntime('reconcile-mother-dispatch-session:switch', {
      motherAgentId: resolvedMotherAgentId,
      previousSessionId: currentSessionId,
      nextSessionId,
      explicitPrimarySessionId,
      fallbackPrimarySessionId,
      hydrate: syncOptions.hydrate !== false,
      syncMessages: syncOptions.syncMessages !== false
    });
    dispatchSessionId.value = nextSessionId;
    dispatchLastEventId.value = 0;
    dispatchRequestId.value = '';
    dispatchRuntimeStatus.value = 'idle';
    dispatchTargetAgentId.value = resolvedMotherAgentId;
    dispatchTargetName.value = resolveAgentNameById(resolvedMotherAgentId);
    dispatchTargetTone.value = 'mother';
    rememberSessionAssistantIdentity(nextSessionId, {
      agentId: resolvedMotherAgentId,
      name: dispatchTargetName.value,
      tone: 'mother'
    });
    ensureDispatchSessionKnown(nextSessionId, false);
    void syncDispatchSessionToMessenger(nextSessionId, resolvedMotherAgentId).catch(() => null);
    if (
      currentSessionId &&
      currentSessionId !== nextSessionId &&
      hasSessionScopedMessageFor(manualChatMessages.value, currentSessionId) &&
      !hasSessionScopedMessageFor(manualChatMessages.value, nextSessionId)
    ) {
      replaceManualChatMessages([]);
    }
    if (syncOptions.syncMessages === false) return;
    await syncDispatchSessionMessages({
      hydrate: syncOptions.hydrate !== false,
      clearWhenEmpty: true,
      forceReplace: true
    });
  };

  const applyFixedMotherDispatchSession = () => {
    const fixedSessionId = fixedMotherDispatchSessionId.value;
    const resolvedMotherAgentId = String(motherAgentId.value || '').trim();
    if (!fixedSessionId || !resolvedMotherAgentId) {
      return;
    }
    const previousSessionId = String(dispatchSessionId.value || '').trim();
    dispatchSessionId.value = fixedSessionId;
    dispatchTargetAgentId.value = resolvedMotherAgentId;
    dispatchTargetName.value = resolveAgentNameById(resolvedMotherAgentId);
    dispatchTargetTone.value = 'mother';
    if (previousSessionId !== fixedSessionId) {
      dispatchLastEventId.value = 0;
      dispatchRequestId.value = '';
      dispatchRuntimeStatus.value = 'idle';
      if (
        previousSessionId &&
        hasSessionScopedMessageFor(manualChatMessages.value, previousSessionId) &&
        !hasSessionScopedMessageFor(manualChatMessages.value, fixedSessionId)
      ) {
        replaceManualChatMessages([]);
      }
    }
    rememberSessionAssistantIdentity(fixedSessionId, {
      agentId: resolvedMotherAgentId,
      name: dispatchTargetName.value,
      tone: 'mother'
    });
    ensureDispatchSessionKnown(fixedSessionId, false);
  };

  const syncDispatchSessionToChatStore = (payload: {
    sessionId: string;
    agentId: string;
    agentName?: string;
    targetTone?: MissionChatMessage['tone'];
    sessionSummary: Record<string, unknown> | null;
    userPreview: string;
  }, syncOptions: { remember?: boolean } = {}) => {
    const targetSessionId = String(payload.sessionId || '').trim();
    if (!targetSessionId) return;
    const nowIso = new Date().toISOString();
    const preview = clipMessageBody(payload.userPreview, 120);
    const requestedAgentId = String(payload.agentId || '').trim();
    const requestedAgentName =
      normalizeChatActorName(payload.agentName) || normalizeChatActorName(resolveAgentNameById(requestedAgentId));
    const fallbackAgentId = normalizeDispatchAgentId(payload.agentId);
    const summary = payload.sessionSummary || null;
    const summaryAgentId = String(summary?.agent_id || '').trim();
    const resolvedAgentId = fallbackAgentId || summaryAgentId;
    const isDefaultSession = requestedAgentId === DEFAULT_AGENT_KEY || summary?.is_default === true;
    const nextSession: Record<string, unknown> = {
      ...(summary || {}),
      id: targetSessionId,
      agent_id: resolvedAgentId,
      beeroom_requested_agent_id: requestedAgentId,
      beeroom_target_name: requestedAgentName,
      is_default: isDefaultSession,
      updated_at: nowIso,
      last_message_at: nowIso
    };
    if (preview) {
      nextSession.beeroom_dispatch_label = preview;
      nextSession.last_message_preview = preview;
      nextSession.last_user_message_preview = preview;
    }
    if (!String(nextSession.title || '').trim()) {
      nextSession.title = preview || options.t('chat.newSession');
    }
    rememberSessionAssistantIdentity(targetSessionId, {
      agentId: requestedAgentId || (isDefaultSession ? DEFAULT_AGENT_KEY : resolvedAgentId),
      name: requestedAgentName,
      tone: payload.targetTone === 'mother' ? 'mother' : 'worker'
    });
    chatStore.syncSessionSummary(nextSession, {
      agentId: fallbackAgentId,
      remember: syncOptions.remember === true
    });
    logBeeroomRuntime('sync-dispatch-session-summary', {
      sessionId: targetSessionId,
      agentId: String(nextSession.beeroom_requested_agent_id || nextSession.agent_id || '').trim(),
      remember: syncOptions.remember === true,
      preview
    });
  };

  const buildVisibleChatMessage = (
    role: 'user' | 'assistant',
    body: string,
    createdAt = Math.floor(Date.now() / 1000)
  ): MissionChatMessage | null => {
    const text = String(body || '').trim();
    if (!text) return null;
    return {
      key: nextManualMessageKey(role),
      senderName:
        role === 'assistant'
          ? resolveDispatchAssistantName(String(dispatchSessionId.value || '').trim())
          : options.t('chat.message.user'),
      senderAgentId:
        role === 'assistant'
          ? resolveDispatchAssistantAgentId(String(dispatchSessionId.value || '').trim())
          : '',
      avatarImageUrl:
        role === 'assistant'
          ? resolveAgentAvatarImageByAgentId(resolveDispatchAssistantAgentId(String(dispatchSessionId.value || '').trim()))
          : currentUserAvatarImageUrl.value,
      mention:
        role === 'assistant'
          ? options.t('chat.message.user')
          : resolveDispatchAssistantName(String(dispatchSessionId.value || '').trim()),
      body: text,
      meta: '',
      time: createdAt,
      timeLabel: formatDateTime(createdAt),
      tone:
        role === 'assistant'
          ? resolveDispatchAssistantTone(String(dispatchSessionId.value || '').trim())
          : 'user'
    };
  };

  const resolveChatToneByAgentId = (
    agentId: string,
    fallback: MissionChatMessage['tone'] = 'worker'
  ): MissionChatMessage['tone'] => {
    const normalizedAgentId = String(agentId || '').trim();
    if (normalizedAgentId && normalizedAgentId === String(motherAgentId.value || '').trim()) {
      return 'mother';
    }
    return fallback;
  };

  const buildSubagentRuntimeMessages = (item: BeeroomMissionSubagentItem): MissionChatMessage[] => {
    const messageTime = Number(item.updatedTime || 0);
    const childAgentId = String(item.agentId || '').trim();
    const fallbackSubagentName = options.t('beeroom.canvas.legendSubagent');
    const childNameFromAgent = normalizeChatActorName(resolveAgentNameById(childAgentId));
    const childNameFromLabel = normalizeChatActorName(item.label);
    const childNameFromTitle = normalizeChatActorName(item.title);
    const childName = childNameFromAgent || childNameFromLabel || childNameFromTitle || fallbackSubagentName;
    const parentSessionId = String(item.controllerSessionId || item.parentSessionId || dispatchSessionId.value || '').trim();
    const parentAgentId = parentSessionId
      ? resolveStoredSessionAgentId(parentSessionId)
      : resolveDispatchAssistantAgentId(String(dispatchSessionId.value || '').trim());
    const parentNameFromAgent = normalizeChatActorName(parentAgentId ? resolveAgentNameById(parentAgentId) : '');
    const parentNameFromSession = parentSessionId ? resolveDispatchAssistantName(parentSessionId) : '';
    const parentNameFromDispatch = resolveDispatchAssistantName(String(dispatchSessionId.value || '').trim());
    const parentName = parentNameFromAgent || parentNameFromSession || parentNameFromDispatch;
    const parentTone = parentSessionId
      ? resolveDispatchAssistantTone(parentSessionId, dispatchTargetTone.value === 'mother' ? 'mother' : 'worker')
      : resolveChatToneByAgentId(parentAgentId, dispatchTargetTone.value === 'mother' ? 'mother' : 'worker');
    const childTone = resolveChatToneByAgentId(childAgentId, 'worker');
    const childAvatarImageUrl = resolveBeeroomProjectedSubagentAvatarImage({
      agentId: childAgentId,
      name: childName,
      explicitAvatarImageUrl: resolveAgentAvatarImageByAgentId(childAgentId),
      resolveAgentAvatarImageByAgentId,
      defaultAgentAvatarImageUrl: DEFAULT_AGENT_AVATAR_IMAGE,
      fallbackAvatarImageUrl: resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig('avatar-048'))
    });
    const parentAvatarImageUrl =
      resolveAgentAvatarImageByAgentId(parentAgentId) ||
      (isBeeroomDefaultAgentLike(parentName) ? DEFAULT_AGENT_AVATAR_IMAGE : '');
    const meta = String(item.dispatchLabel || '').trim();
    const requestBody = String(item.userMessage || '').trim();
    const replyBody = String((item.failed ? item.errorMessage || item.assistantMessage : item.assistantMessage || item.errorMessage) || '').trim();
    const keyBase = String(item.runId || item.sessionId || item.key || '').trim();
    const messages: MissionChatMessage[] = [];

    if (!childNameFromAgent && !childNameFromLabel && !childNameFromTitle) {
      logBeeroomRuntime('build-subagent-runtime-messages:fallback-child-name', {
        runId: item.runId,
        sessionId: item.sessionId,
        agentId: childAgentId,
        fallbackName: fallbackSubagentName
      });
    }
    if (!parentNameFromAgent && !parentNameFromDispatch) {
      logBeeroomRuntime('build-subagent-runtime-messages:fallback-parent-name', {
        runId: item.runId,
        sessionId: item.sessionId,
        parentSessionId,
        parentAgentId
      });
    }

    if (requestBody) {
      messages.push({
        key: `subagent:${keyBase}:request`,
        senderName: parentName || resolveDispatchAssistantName(String(dispatchSessionId.value || '').trim()),
        senderAgentId: parentAgentId,
        avatarImageUrl: parentAvatarImageUrl,
        mention: childName,
        body: requestBody,
        meta,
        time: messageTime,
        timeLabel: formatDateTime(messageTime),
        tone: parentTone,
        sortOrder: BEEROOM_SUBAGENT_REQUEST_SORT_ORDER
      });
    }

    if (replyBody) {
      messages.push({
        key: `subagent:${keyBase}:reply`,
        senderName: childName,
        senderAgentId: childAgentId,
        avatarImageUrl: childAvatarImageUrl,
        mention: parentName || resolveDispatchAssistantName(String(dispatchSessionId.value || '').trim()),
        body: replyBody,
        meta,
        time: messageTime,
        timeLabel: formatDateTime(messageTime),
        tone: childTone,
        sortOrder: BEEROOM_SUBAGENT_REPLY_SORT_ORDER
      });
    }

    return messages;
  };

  const derivedSubagentChatMessages = computed<MissionChatMessage[]>(() => {
    const previewSubagents = Array.isArray(dispatchPreview.value?.subagents) ? dispatchPreview.value?.subagents || [] : [];
    const uniqueItems = new Map<string, BeeroomMissionSubagentItem>();
    previewSubagents.forEach((item) => {
      const key = String(item?.runId || item?.sessionId || item?.key || '').trim();
      if (!key) return;
      uniqueItems.set(key, item);
    });
    return Array.from(uniqueItems.values())
      .flatMap((item) => buildSubagentRuntimeMessages(item))
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      );
  });

  const ensureDispatchSession = async (
    agentId: string,
    sessionOptions: { preferredSessionId?: string; preferPrimarySession?: boolean } = {}
  ): Promise<DispatchSessionTarget> => {
    const preferredSessionId = String(sessionOptions.preferredSessionId || '').trim();
    const preferPrimarySession = sessionOptions.preferPrimarySession === true;
    if (preferredSessionId) {
      const preferredSummary = resolveDispatchSessionSummary(preferredSessionId);
      if (resolveValidDispatchSessionId(preferredSessionId, preferredSummary)) {
        logBeeroomRuntime('ensure-dispatch-session:reuse-preferred', {
          agentId,
          preferredSessionId,
          preferPrimarySession
        });
        return {
          sessionId: preferredSessionId,
          sessionSummary:
            preferredSummary && typeof preferredSummary === 'object'
              ? (preferredSummary as Record<string, unknown>)
              : null
        };
      }
    }
    const apiAgentId = agentId === DEFAULT_AGENT_KEY ? '' : agentId;
    const { data } = await listSessions({ agent_id: apiAgentId });
    const source = Array.isArray(data?.data?.items) ? data.data.items : [];
    const matched = source
      .filter((item) => {
        const sessionAgentId = String(item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')).trim();
        return sessionAgentId === agentId;
      })
      .sort((left, right) => {
        const leftTime = toSessionTimestampMs(left?.updated_at ?? left?.last_message_at ?? left?.created_at);
        const rightTime = toSessionTimestampMs(right?.updated_at ?? right?.last_message_at ?? right?.created_at);
        return rightTime - leftTime;
      });
    logBeeroomRuntime('ensure-dispatch-session:list', {
      agentId,
      preferredSessionId,
      preferPrimarySession,
      matchedSessionIds: matched.slice(0, 8).map((item) => ({
        id: String(item?.id || '').trim(),
        isMain: item?.is_main === true,
        updatedAt: String(item?.updated_at || item?.last_message_at || item?.created_at || '').trim()
      }))
    });
    if (preferPrimarySession) {
      const resolvedPrimarySessionId = resolvePrimaryDispatchSessionId(agentId, matched);
      const primary =
        matched.find((item) => String(item?.id || '').trim() === resolvedPrimarySessionId) ||
        matched.find((item) => item?.is_main === true) ||
        (preferredSessionId
          ? matched.find((item) => String(item?.id || '').trim() === preferredSessionId) || null
          : null) ||
        matched[0];
      if (primary?.id) {
        logBeeroomRuntime('ensure-dispatch-session:prefer-primary-hit', {
          agentId,
          preferredSessionId,
          resolvedPrimarySessionId,
          selectedSessionId: String(primary.id || '').trim(),
          isMain: primary?.is_main === true
        });
        return {
          sessionId: String(primary.id),
          sessionSummary: primary && typeof primary === 'object' ? (primary as Record<string, unknown>) : null
        };
      }
    }
    if (preferredSessionId) {
      const preferred = matched.find((item) => String(item?.id || '').trim() === preferredSessionId);
      if (preferred?.id) {
        logBeeroomRuntime('ensure-dispatch-session:preferred-hit', {
          agentId,
          preferredSessionId
        });
        return {
          sessionId: String(preferred.id),
          sessionSummary: preferred && typeof preferred === 'object' ? (preferred as Record<string, unknown>) : null
        };
      }
    }
    const resolvedSessionId = String(chatStore.resolveInitialSessionId(agentId, matched) || '').trim();
    const primary =
      matched.find((item) => String(item?.id || '').trim() === resolvedSessionId) ||
      matched.find((item) => item?.is_main === true) ||
      matched[0];
    if (primary?.id) {
      logBeeroomRuntime('ensure-dispatch-session:resolved-existing', {
        agentId,
        resolvedSessionId,
        selectedSessionId: String(primary.id || '').trim(),
        isMain: primary?.is_main === true
      });
      return {
        sessionId: String(primary.id),
        sessionSummary: primary && typeof primary === 'object' ? (primary as Record<string, unknown>) : null
      };
    }
    const created = await createSession(agentId === DEFAULT_AGENT_KEY ? {} : { agent_id: agentId });
    const createdSummary = created?.data?.data;
    logBeeroomRuntime('ensure-dispatch-session:created', {
      agentId,
      createdSessionId: String(createdSummary?.id || '').trim()
    });
    return {
      sessionId: String(createdSummary?.id || ''),
      sessionSummary:
        createdSummary && typeof createdSummary === 'object'
          ? (createdSummary as Record<string, unknown>)
          : null
    };
  };

  const normalizeStreamEventId = (value: unknown): number => {
    const parsed = Number.parseInt(String(value || '').trim(), 10);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
  };

  const updateDispatchLastEventId = (value: unknown) => {
    const normalized = normalizeStreamEventId(value);
    if (normalized > dispatchLastEventId.value) {
      dispatchLastEventId.value = normalized;
    }
  };

  const resetDispatchRuntime = (
    options: { keepSession?: boolean; keepRuntimeStatus?: boolean; persist?: boolean } = {}
  ) => {
    if (dispatchStreamController) {
      dispatchStreamController.abort();
      dispatchStreamController = null;
    }
    composerSending.value = false;
    dispatchStopRequested = false;
    dispatchRespondingApprovalId.value = '';
    dispatchRequestId.value = '';
    if (!options.keepRuntimeStatus) {
      dispatchRuntimeStatus.value = 'idle';
    }
    if (!options.keepSession) {
      dispatchSessionId.value = '';
      dispatchLastEventId.value = 0;
      dispatchTargetAgentId.value = '';
      dispatchTargetName.value = '';
      dispatchTargetTone.value = 'worker';
      runtimeRelayChatMessages.value = [];
    }
    if (options.persist !== false) {
      persistCachedChatState();
    }
  };

  const consumeDispatchStream = async (response: Response) => {
    let finalPayload: Record<string, any> | null = null;
    let streamError = '';
    await consumeSseStream(response, (eventType, dataText, eventId) => {
      updateDispatchLastEventId(eventId);
      const payload = safeJsonParse(dataText);
      const data = payload?.data ?? payload;
      updateDispatchLastEventId(data?.event_id ?? data?.eventId ?? payload?.event_id ?? payload?.eventId);
      const eventRequestId = String(
        payload?.request_id ?? payload?.requestId ?? data?.request_id ?? data?.requestId ?? ''
      ).trim();
      if (eventRequestId) {
        dispatchRequestId.value = eventRequestId;
      }

      if (eventType === 'heartbeat' || eventType === 'ping') return;
      if (eventType === 'approval_request') {
        chatStore.enqueueApprovalRequest(dispatchRequestId.value, dispatchSessionId.value, data);
        dispatchRuntimeStatus.value = 'awaiting_approval';
        logBeeroomRuntime('dispatch-stream:approval-request', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value,
          approvalId: String(data?.approval_id || data?.approvalId || '').trim()
        });
        return;
      }
      if (eventType === 'approval_result') {
        chatStore.resolveApprovalResult(data);
        const status = String(data?.status || payload?.status || '').trim().toLowerCase();
        dispatchRuntimeStatus.value = status === 'approved' ? 'running' : 'failed';
        logBeeroomRuntime('dispatch-stream:approval-result', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value,
          status
        });
        return;
      }
      if (eventType === 'queued') {
        dispatchRuntimeStatus.value = 'queued';
        logBeeroomRuntime('dispatch-stream:queued', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value
        });
        return;
      }
      if (eventType === 'slow_client') {
        dispatchRuntimeStatus.value = 'stopped';
        logBeeroomRuntime('dispatch-stream:slow-client', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value
        });
        return;
      }
      if (eventType === 'error') {
        streamError = extractErrorText(payload) || options.t('common.requestFailed');
        dispatchRuntimeStatus.value = 'failed';
        logBeeroomRuntime('dispatch-stream:error', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value,
          error: streamError
        });
        return;
      }
      if (eventType === 'final') {
        finalPayload = payload;
        dispatchRuntimeStatus.value = 'completed';
        logBeeroomRuntime('dispatch-stream:final', {
          sessionId: dispatchSessionId.value,
          requestId: dispatchRequestId.value,
          replyPreview: clipDebugText(extractReplyText(payload))
        });
        return;
      }
      dispatchRuntimeStatus.value = 'running';
    });

    if (streamError) {
      throw new Error(streamError);
    }
    return finalPayload;
  };

  const startDispatchStream = async (
    mode: 'send' | 'resume',
    sessionId: string,
    payload: { content?: string; afterEventId?: number } = {},
    streamOptions: { onAccepted?: () => void } = {}
  ) => {
    if (dispatchStreamController) {
      dispatchStreamController.abort();
    }
    dispatchStopRequested = false;
    composerSending.value = true;
    dispatchRuntimeStatus.value = mode === 'resume' ? 'resuming' : 'running';
    const controller = new AbortController();
    dispatchStreamController = controller;
    logBeeroomRuntime('start-dispatch-stream', {
      mode,
      sessionId,
      afterEventId: Number(payload.afterEventId || 0),
      contentPreview: clipDebugText(payload.content)
    });

    const response =
      mode === 'resume'
        ? await resumeMessageStream(sessionId, {
            signal: controller.signal,
            afterEventId:
              Number.isFinite(payload.afterEventId) && Number(payload.afterEventId) > 0
                ? Number(payload.afterEventId)
                : undefined
          })
        : await sendMessageStream(
            sessionId,
            { content: String(payload.content || ''), stream: true },
            {
              signal: controller.signal,
              orchestrationSource: 'beeroom_orchestration'
            }
          );

    if (!response.ok) {
      const errorText = String(await response.text()).trim();
      logBeeroomRuntime('start-dispatch-stream:http-error', {
        mode,
        sessionId,
        status: response.status,
        error: clipDebugText(errorText)
      });
      throw new Error(
        errorText || (mode === 'resume' ? options.t('chat.error.resumeFailed') : options.t('common.requestFailed'))
      );
    }

    streamOptions.onAccepted?.();
    logBeeroomRuntime('start-dispatch-stream:accepted', {
      mode,
      sessionId
    });
    return consumeDispatchStream(response);
  };

  const handleComposerSend = async (payload?: { content?: string; displayContent?: string }) => {
    if (composerSending.value) {
      await handleDispatchStop();
      return;
    }
    const content = String(payload?.content ?? composerText.value ?? '').trim();
    if (!content) return;
    const inputOverflow = resolveChatRequestTextInputOverflow(content, [], ({ actualChars, maxChars }) =>
      options.t('chat.error.userInputTooLong', { actualChars, maxChars })
    );
    if (inputOverflow) {
      composerError.value = inputOverflow.message;
      ElMessage.warning(inputOverflow.message);
      return;
    }

    const { target, body } = resolveDispatchTarget(content);
    if (!target?.agentId) {
      const message = options.t('beeroom.canvas.chatTargetRequired');
      composerError.value = message;
      ElMessage.warning(message);
      return;
    }

    const targetName = resolveAgentNameById(target.agentId);
    const now = Math.floor(Date.now() / 1000);
    const dispatchBody = String(body || content).trim();
    const visibleBody = String(payload?.displayContent ?? dispatchBody).trim() || dispatchBody;
    dispatchLabelPreview.value = visibleBody;
    const targetTone = target.role === 'mother' ? 'mother' : 'worker';
    const previousSessionId = String(dispatchSessionId.value || '').trim();
    const previousTargetAgentId = String(dispatchTargetAgentId.value || '').trim();
    const preferredSessionId = resolvePreferredDispatchSessionId(
      target,
      previousSessionId,
      previousTargetAgentId
    );
    logBeeroomRuntime('composer-send:start', {
      targetAgentId: target.agentId,
      targetRole: target.role,
      previousSessionId,
      previousTargetAgentId,
      preferredSessionId,
      bodyPreview: clipDebugText(visibleBody)
    });

    composerError.value = '';
    composerText.value = '';
    dispatchTargetAgentId.value = target.agentId;
    dispatchTargetName.value = targetName;
    dispatchTargetTone.value = targetTone;
    const localUserMessage = buildVisibleChatMessage('user', visibleBody, now);
    let localUserAccepted = false;
    let terminalReplyText = '';
    let reachedTerminalReply = false;
    let baselineAssistantSignature = '';
    try {
      const dispatchSession = await ensureDispatchSession(target.agentId, {
        preferredSessionId,
        preferPrimarySession: target.role === 'mother'
      });
      const sessionId = String(dispatchSession.sessionId || '').trim();
      if (!sessionId) {
        throw new Error(options.t('common.requestFailed'));
      }
      const reuseCurrentSession = Boolean(preferredSessionId) && preferredSessionId === sessionId;
      logBeeroomRuntime('composer-send:resolved-session', {
        targetAgentId: target.agentId,
        sessionId,
        reuseCurrentSession,
        targetTone
      });
      dispatchSessionId.value = sessionId;
      dispatchRequestId.value = nextManualMessageKey('dispatch-request');
      dispatchLastEventId.value = 0;
      dispatchRuntimeStatus.value = 'queued';
      syncDispatchSessionToChatStore(
        {
          sessionId,
          agentId: target.agentId,
          agentName: targetName,
          targetTone,
          sessionSummary: dispatchSession.sessionSummary,
          userPreview: visibleBody
        },
        { remember: false }
      );
      void syncDispatchSessionToMessenger(sessionId, target.agentId).catch(() => null);
      await syncDispatchSessionMessages({
        hydrate: true,
        clearWhenEmpty: !reuseCurrentSession,
        forceReplace: !reuseCurrentSession
      });
      baselineAssistantSignature = buildSessionAssistantSignature(
        readDispatchSessionMessages(sessionId),
        sessionId
      );
      const finalPayload = await startDispatchStream(
        'send',
        sessionId,
        { content: dispatchBody },
        {
          onAccepted: () => {
            if (!localUserAccepted && localUserMessage) {
              appendManualChatMessage(localUserMessage);
              localUserAccepted = true;
            }
          }
        }
      );
      const replyText = extractReplyText(finalPayload);
      terminalReplyText = replyText;
      reachedTerminalReply = true;
      const assistantMessage = buildVisibleChatMessage('assistant', replyText);
      if (assistantMessage) {
        appendManualChatMessage(assistantMessage);
      }
      logBeeroomRuntime('composer-send:completed', {
        sessionId,
        targetAgentId: target.agentId,
        replyPreview: clipDebugText(replyText)
      });
    } catch (error: any) {
      if (error?.name === 'AbortError' || dispatchStopRequested) {
        dispatchRuntimeStatus.value = 'stopped';
        logBeeroomRuntime('composer-send:aborted', {
          sessionId: dispatchSessionId.value,
          targetAgentId: dispatchTargetAgentId.value
        });
        return;
      }
      const message = String(error?.message || '').trim() || options.t('common.requestFailed');
      dispatchRuntimeStatus.value = 'failed';
      composerError.value = message;
      ElMessage.error(message);
      logBeeroomRuntime('composer-send:error', {
        sessionId: dispatchSessionId.value,
        targetAgentId: dispatchTargetAgentId.value,
        error: clipDebugText(message)
      });
    } finally {
      if (dispatchSessionId.value) {
        if (reachedTerminalReply) {
          await hydrateTerminalDispatchSessionMessages({
            sessionId: String(dispatchSessionId.value || '').trim(),
            expectedReplyText: terminalReplyText,
            baselineAssistantSignature
          });
        } else {
          await syncDispatchSessionMessages({ hydrate: true });
        }
      }
      dispatchStreamController = null;
      composerSending.value = false;
      dispatchLabelPreview.value = '';
    }
  };

  const handleDispatchStop = async () => {
    if (!dispatchCanStop.value) return;
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) return;
    logBeeroomRuntime('dispatch-stop:start', {
      sessionId
    });
    dispatchStopRequested = true;
    dispatchRuntimeStatus.value = 'stopped';
    if (dispatchStreamController) {
      dispatchStreamController.abort();
      dispatchStreamController = null;
    }
    try {
      await chatStore.stopSessionActivity(sessionId, { terminateSubagents: true });
    } catch {
      // Keep local interrupt behavior even if cancel API fails.
    } finally {
      composerSending.value = false;
      await syncDispatchSessionMessages({ hydrate: true });
      logBeeroomRuntime('dispatch-stop:done', {
        sessionId,
        runtimeStatus: dispatchRuntimeStatus.value
      });
    }
  };

  const handleDispatchResume = async () => {
    if (!dispatchCanResume.value) return;
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) return;
    composerError.value = '';
    logBeeroomRuntime('dispatch-resume:start', {
      sessionId,
      afterEventId: dispatchLastEventId.value
    });
    let terminalReplyText = '';
    let reachedTerminalReply = false;
    const baselineAssistantSignature = buildSessionAssistantSignature(
      readDispatchSessionMessages(sessionId),
      sessionId
    );
    try {
      const finalPayload = await startDispatchStream('resume', sessionId, {
        afterEventId: dispatchLastEventId.value
      });
      const replyText = extractReplyText(finalPayload);
      terminalReplyText = replyText;
      reachedTerminalReply = true;
      const assistantMessage = buildVisibleChatMessage('assistant', replyText);
      if (assistantMessage) {
        appendManualChatMessage(assistantMessage);
      }
      logBeeroomRuntime('dispatch-resume:completed', {
        sessionId,
        replyPreview: clipDebugText(replyText)
      });
    } catch (error: any) {
      if (error?.name === 'AbortError' || dispatchStopRequested) {
        dispatchRuntimeStatus.value = 'stopped';
        logBeeroomRuntime('dispatch-resume:aborted', {
          sessionId
        });
        return;
      }
      const message = String(error?.message || '').trim() || options.t('chat.error.resumeFailed');
      dispatchRuntimeStatus.value = 'failed';
      composerError.value = message;
      ElMessage.error(message);
      logBeeroomRuntime('dispatch-resume:error', {
        sessionId,
        error: clipDebugText(message)
      });
    } finally {
      if (dispatchSessionId.value) {
        if (reachedTerminalReply) {
          await hydrateTerminalDispatchSessionMessages({
            sessionId: String(dispatchSessionId.value || '').trim(),
            expectedReplyText: terminalReplyText,
            baselineAssistantSignature
          });
        } else {
          await syncDispatchSessionMessages({ hydrate: true });
        }
      }
      dispatchStreamController = null;
      composerSending.value = false;
      dispatchLabelPreview.value = '';
    }
  };

  const handleDispatchApproval = async (
    decision: 'approve_once' | 'approve_session' | 'deny',
    approvalId: string
  ) => {
    const normalizedApprovalId = String(approvalId || '').trim();
    if (!normalizedApprovalId || dispatchRespondingApprovalId.value) return;
    dispatchRespondingApprovalId.value = normalizedApprovalId;
    try {
      await chatStore.respondApproval(decision, normalizedApprovalId);
      if (decision !== 'deny') {
        ElMessage.success(options.t('chat.approval.sent'));
        dispatchRuntimeStatus.value = 'running';
      }
    } catch {
      ElMessage.error(options.t('chat.approval.sendFailed'));
    } finally {
      dispatchRespondingApprovalId.value = '';
    }
  };

  const allRenderableChatMessages = computed(() =>
    [...manualChatMessages.value, ...runtimeRelayChatMessages.value]
      .map((message) => ({
        ...message,
        senderName:
          message.tone === 'user'
            ? String(message.senderName || '').trim()
            : normalizeChatActorName(message.senderName) || String(message.senderName || '').trim(),
        avatarImageUrl: String(message.avatarImageUrl || '').trim(),
        mention: normalizeChatActorName(message.mention) || String(message.mention || '').trim()
      }))
      .sort(compareMissionChatMessages)
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      )
  );

  const displayChatMessages = computed(() =>
    allRenderableChatMessages.value
      .filter((message) => {
        if (message.tone === 'user') return true;
        const senderAgentId = String(message.senderAgentId || '').trim();
        if (!senderAgentId) return true;
        return !silentAgentIdSet.value.has(senderAgentId);
      })
  );

  const listRecentAgentOutputs = (
    agentId: unknown,
    limit = DEFAULT_BEEROOM_AGENT_OUTPUT_PREVIEW_LIMIT
  ): MissionChatMessage[] =>
    listRecentBeeroomAgentOutputs(allRenderableChatMessages.value, {
      agentId,
      limit
    });

  const dispatchBindingSignature = computed(() =>
    [
      String(dispatchSessionId.value || '').trim(),
      String(dispatchTargetAgentId.value || '').trim(),
      dispatchTargetTone.value,
      dispatchRuntimeStatus.value,
      String(chatStore.activeSessionId || '').trim(),
      composerSending.value ? 'sending' : 'idle'
    ].join('|')
  );

  watch(
    dispatchBindingSignature,
    (signature, previousSignature) => {
      if (!signature || signature === previousSignature) return;
      logBeeroomRuntime('dispatch-binding-changed', {
        dispatchSessionId: String(dispatchSessionId.value || '').trim(),
        dispatchTargetAgentId: String(dispatchTargetAgentId.value || '').trim(),
        dispatchTargetTone: dispatchTargetTone.value,
        dispatchRuntimeStatus: dispatchRuntimeStatus.value,
        activeMessengerSessionId: String(chatStore.activeSessionId || '').trim(),
        composerSending: composerSending.value
      });
    },
    { immediate: true }
  );

  const derivedSubagentMessageSignature = computed(() =>
    buildBeeroomRuntimeRelayMessageSignature(derivedSubagentChatMessages.value)
  );

  watch(
    derivedSubagentMessageSignature,
    (signature, previousSignature) => {
      if (signature === previousSignature) return;
      mergeRuntimeRelayChatMessages(derivedSubagentChatMessages.value);
      logBeeroomRuntime('derived-subagent-messages-changed', {
        messageCount: derivedSubagentChatMessages.value.length,
        persistedCount: runtimeRelayChatMessages.value.length,
        messages: derivedSubagentChatMessages.value.slice(0, 8).map((message) => ({
          key: message.key,
          tone: message.tone,
          senderName: message.senderName,
          mention: message.mention
        }))
      });
    },
    { immediate: true }
  );

  const displayChatMessageSignature = computed(() =>
    displayChatMessages.value
      .map((message) =>
        [message.key, message.tone, message.senderName, message.mention, Number(message.time || 0)].join(':')
      )
      .join('|')
  );

  watch(
    displayChatMessageSignature,
    (signature, previousSignature) => {
      if (signature === previousSignature) return;
      logBeeroomRuntime('display-chat-messages-changed', {
        messageCount: displayChatMessages.value.length,
        manualCount: manualChatMessages.value.length,
        relayCount: runtimeRelayChatMessages.value.length,
        firstMessageKey: displayChatMessages.value[0]?.key || '',
        lastMessageKey: displayChatMessages.value[displayChatMessages.value.length - 1]?.key || '',
        messages: displayChatMessages.value.slice(0, 8).map((message) => ({
          key: message.key,
          tone: message.tone,
          senderName: message.senderName,
          mention: message.mention
        }))
      });
    },
    { immediate: true }
  );

  watch(
    composerTargetOptions,
    (optionsList) => {
      if (!optionsList.length) {
        composerTargetAgentId.value = '';
        return;
      }
      if (
        lockedComposerTargetAgentId.value &&
        optionsList.some((item) => item.agentId === lockedComposerTargetAgentId.value)
      ) {
        composerTargetAgentId.value = lockedComposerTargetAgentId.value;
        return;
      }
      if (!optionsList.some((item) => item.agentId === composerTargetAgentId.value)) {
        composerTargetAgentId.value = optionsList[0]?.agentId || '';
      }
    },
    { immediate: true }
  );

  const handleActiveGroupChanged = (value: unknown) => {
    const groupId = String(value || '').trim();
    const runtimeScopeKey = chatRuntimeScopeKey.value || (groupId ? `runtime:${groupId}` : '');
    const previousDispatchSessionId = String(dispatchSessionId.value || '').trim();
    chatAuthDenied = false;
    chatMessagesClearedAfter.value = Number(
      getBeeroomMissionCanvasState(chatClearScopeKey.value || (groupId ? `chat:${groupId}` : ''))?.chatClearedAfter || 0
    );
    chatRealtimeCursor.value = 0;
    resetDispatchRuntime({ persist: false });
    lastTeamRealtimeRefreshAt = 0;
    lastDispatchMessageRefreshAt = 0;
    lastSyncRequiredHistoryReloadAt = 0;
    clearDispatchMessageRefreshTimer();
    pendingDispatchMessageRefresh = null;
    clearSyncRequiredHistoryReloadTimer();
    composerText.value = '';
    composerError.value = '';
    logBeeroomRuntime('active-group-changed', {
      groupId,
      runtimeScopeKey,
      previousDispatchSessionId
    });
    applyFixedMotherDispatchSession();
    restoreCachedChatState(runtimeScopeKey);
    applyFixedMotherDispatchSession();
    void loadManualChatHistory();
    stopChatRealtimeWatch();
    if (groupId) {
      startChatRealtimeWatch(groupId);
    }
    restartChatPolling();
  };

  watch(
    activeGroupId,
    (groupId) => {
      handleActiveGroupChanged(groupId);
    },
    { immediate: true }
  );

  watch(
    missionScopeKey,
    (currentScopeKey) => {
      const cached = getBeeroomMissionCanvasState(currentScopeKey);
      chatCollapsed.value = !!cached?.chatCollapsed;
    },
    { immediate: true }
  );

  watch(
    chatClearScopeKey,
    (currentScopeKey) => {
      const cached = getBeeroomMissionCanvasState(currentScopeKey);
      chatMessagesClearedAfter.value = Number(cached?.chatClearedAfter || 0);
    },
    { immediate: true }
  );

  watch(chatCollapsed, (value) => {
    mergeBeeroomMissionCanvasState(missionScopeKey.value, {
      chatCollapsed: value
    });
  });

  watch(
    () => [chatRuntimeScopeKey.value, chatClearScopeKey.value, fixedMotherDispatchSessionId.value].join('|'),
    () => {
      handleActiveGroupChanged(activeGroupId.value);
    }
  );

  watch(
    () =>
      [
        chatRuntimeScopeKey.value,
        dispatchSessionId.value,
        dispatchLastEventId.value,
        dispatchTargetAgentId.value,
        dispatchTargetName.value,
        dispatchTargetTone.value,
        dispatchRuntimeStatus.value
      ].join('|'),
    () => {
      persistCachedChatState();
    }
  );

  watch(
    dispatchPreview,
    (preview) => {
      const sessionId = String(preview?.sessionId || '').trim();
      if (!sessionId) return;
      const previewAgentId = String(preview?.targetAgentId || '').trim();
      const previewName = normalizeChatActorName(preview?.targetName);
      const currentSessionId = String(dispatchSessionId.value || '').trim();
      const summary = resolveDispatchSessionSummary(sessionId);
      const summaryAgentId = String(summary?.agent_id || '').trim();
      const requestedAgentId = String(summary?.beeroom_requested_agent_id || '').trim();
      const summaryName = normalizeChatActorName(summary?.beeroom_target_name);
      const motherId = String(motherAgentId.value || '').trim();
      const nextTone: MissionChatMessage['tone'] =
        (previewAgentId && previewAgentId === motherId) ||
        (currentSessionId === sessionId && dispatchTargetTone.value === 'mother')
          ? 'mother'
          : 'worker';
      const nextAgentId =
        previewAgentId ||
        (currentSessionId === sessionId ? String(dispatchTargetAgentId.value || '').trim() : '') ||
        requestedAgentId ||
        (summary?.is_default === true ? DEFAULT_AGENT_KEY : '') ||
        summaryAgentId;
      const nextName =
        previewName ||
        (currentSessionId === sessionId ? normalizeChatActorName(dispatchTargetName.value) : '') ||
        summaryName;
      rememberSessionAssistantIdentity(sessionId, {
        agentId: nextAgentId,
        name: nextName,
        tone: nextTone
      });
      if (currentSessionId === sessionId) {
        if (previewAgentId && previewAgentId !== String(dispatchTargetAgentId.value || '').trim()) {
          dispatchTargetAgentId.value = previewAgentId;
        }
        if (previewName && previewName !== normalizeChatActorName(dispatchTargetName.value)) {
          dispatchTargetName.value = previewName;
        }
        if (dispatchTargetTone.value !== nextTone) {
          dispatchTargetTone.value = nextTone;
        }
      }
      const needsSummarySync =
        !summary ||
        requestedAgentId !== nextAgentId ||
        summaryName !== nextName ||
        (nextAgentId !== DEFAULT_AGENT_KEY && summaryAgentId !== normalizeDispatchAgentId(nextAgentId)) ||
        (nextAgentId === DEFAULT_AGENT_KEY && summary?.is_default !== true);
      if (!needsSummarySync) return;
      syncDispatchSessionToChatStore(
        {
          sessionId,
          agentId: nextAgentId,
          agentName: nextName,
          targetTone: nextTone,
          sessionSummary: summary,
          userPreview: resolveLatestVisibleUserPreview()
        },
        { remember: false }
      );
    },
    { immediate: true }
  );

  const dispatchPreviewRefreshSignature = computed(() => {
    const preview = dispatchPreview.value;
    const sessionId = String(preview?.sessionId || '').trim();
    if (!sessionId) return '';
    const subagentSignature = (Array.isArray(preview?.subagents) ? preview.subagents : [])
      .map((item) =>
        [
          String(item?.key || item?.sessionId || item?.runId || '').trim(),
          String(item?.status || '').trim().toLowerCase(),
          Number(item?.updatedTime || 0)
        ].join(':')
      )
      .join('|');
    return [
      sessionId,
      String(preview?.status || '').trim().toLowerCase(),
      Number(preview?.updatedTime || 0),
      String(preview?.summary || '').trim(),
      subagentSignature
    ].join('||');
  });

  watch(dispatchPreviewRefreshSignature, (signature, previousSignature) => {
    if (!signature || signature === previousSignature) return;
    const preview = dispatchPreview.value;
    const previewSessionId = String(preview?.sessionId || '').trim();
    const currentSessionId = String(dispatchSessionId.value || '').trim();
    const previewStatus = String(preview?.status || '').trim().toLowerCase();
    if (!previewSessionId || previewSessionId !== currentSessionId) return;
    logBeeroomRuntime('dispatch-preview:signature-changed', {
      sessionId: previewSessionId,
      status: previewStatus,
      updatedTime: Number(preview?.updatedTime || 0),
      subagentCount: Array.isArray(preview?.subagents) ? preview.subagents.length : 0
    });
    if (!TERMINAL_DISPATCH_PREVIEW_STATUSES.has(previewStatus)) return;
    scheduleDispatchMessageRefresh('dispatch-preview-terminal', {
      hydrate: true,
      forceReplace: true,
      immediate: true,
      targetSessionId: previewSessionId
    });
  });

  const motherDispatchSessionBinding = computed(() => {
    const cachedTargetAgentId = String(dispatchTargetAgentId.value || '').trim();
    const resolvedMotherAgentId = String(motherAgentId.value || cachedTargetAgentId || '').trim();
    const isMotherTarget =
      dispatchTargetTone.value === 'mother' ||
      (resolvedMotherAgentId && cachedTargetAgentId === resolvedMotherAgentId);
    if (!isMotherTarget || !resolvedMotherAgentId) return '';
    const explicitPrimarySessionId = resolveExplicitMainDispatchSessionId(resolvedMotherAgentId);
    const fallbackPrimarySessionId = resolvePrimaryDispatchSessionId(resolvedMotherAgentId);
    return [
      resolvedMotherAgentId,
      cachedTargetAgentId,
      dispatchTargetTone.value,
      String(dispatchSessionId.value || '').trim(),
      explicitPrimarySessionId,
      fallbackPrimarySessionId
    ].join('|');
  });

  watch(
    motherDispatchSessionBinding,
    (binding) => {
      if (!binding) return;
      void reconcileMotherDispatchSession();
    },
    { immediate: true }
  );

  function stopChatPolling() {
    chatRealtimeRuntime?.stopHealthPolling();
  }

  function clearTeamRealtimeReconcileTimer() {
    if (teamRealtimeReconcileTimer !== null) {
      window.clearTimeout(teamRealtimeReconcileTimer);
      teamRealtimeReconcileTimer = null;
    }
  }

  function clearDispatchMessageRefreshTimer() {
    if (dispatchMessageRefreshTimer !== null) {
      window.clearTimeout(dispatchMessageRefreshTimer);
      dispatchMessageRefreshTimer = null;
    }
  }

  function clearSyncRequiredHistoryReloadTimer() {
    if (syncRequiredHistoryReloadTimer !== null) {
      window.clearTimeout(syncRequiredHistoryReloadTimer);
      syncRequiredHistoryReloadTimer = null;
    }
  }

  function runDispatchMessageRefresh() {
    clearDispatchMessageRefreshTimer();
    const request = pendingDispatchMessageRefresh;
    pendingDispatchMessageRefresh = null;
    if (!request) return;
    const currentSessionId = String(dispatchSessionId.value || '').trim();
    if (!currentSessionId || currentSessionId !== request.sessionId) {
      logBeeroomRuntime('dispatch-message-refresh:skip-session-mismatch', {
        reason: request.reason,
        requestedSessionId: request.sessionId,
        currentSessionId
      });
      return;
    }
    lastDispatchMessageRefreshAt = Date.now();
    logBeeroomRuntime('dispatch-message-refresh:run', {
      reason: request.reason,
      sessionId: request.sessionId,
      hydrate: request.hydrate,
      clearWhenEmpty: request.clearWhenEmpty,
      forceReplace: request.forceReplace
    });
    void syncDispatchSessionMessages({
      hydrate: request.hydrate,
      clearWhenEmpty: request.clearWhenEmpty,
      forceReplace: request.forceReplace
    }).catch((error) => {
      logBeeroomRuntime('dispatch-message-refresh:error', {
        reason: request.reason,
        sessionId: request.sessionId,
        error: summarizeDebugError(error)
      });
    });
  }

  function scheduleDispatchMessageRefresh(
    reason: string,
    refreshOptions: {
      hydrate?: boolean;
      clearWhenEmpty?: boolean;
      forceReplace?: boolean;
      immediate?: boolean;
      targetSessionId?: string;
    } = {}
  ) {
    const sessionId = String(refreshOptions.targetSessionId || dispatchSessionId.value || '').trim();
    if (!sessionId) return;
    pendingDispatchMessageRefresh =
      pendingDispatchMessageRefresh && pendingDispatchMessageRefresh.sessionId === sessionId
        ? {
            reason,
            sessionId,
            hydrate: pendingDispatchMessageRefresh.hydrate || refreshOptions.hydrate !== false,
            clearWhenEmpty:
              pendingDispatchMessageRefresh.clearWhenEmpty || refreshOptions.clearWhenEmpty === true,
            forceReplace:
              pendingDispatchMessageRefresh.forceReplace || refreshOptions.forceReplace === true
          }
        : {
            reason,
            sessionId,
            hydrate: refreshOptions.hydrate !== false,
            clearWhenEmpty: refreshOptions.clearWhenEmpty === true,
            forceReplace: refreshOptions.forceReplace === true
          };
    const now = Date.now();
    logBeeroomRuntime('dispatch-message-refresh:schedule', {
      reason,
      sessionId,
      hydrate: pendingDispatchMessageRefresh.hydrate,
      clearWhenEmpty: pendingDispatchMessageRefresh.clearWhenEmpty,
      forceReplace: pendingDispatchMessageRefresh.forceReplace,
      immediate: refreshOptions.immediate === true
    });
    if (
      refreshOptions.immediate === true ||
      now - lastDispatchMessageRefreshAt >= DISPATCH_MESSAGE_REFRESH_THROTTLE_MS
    ) {
      runDispatchMessageRefresh();
      return;
    }
    if (dispatchMessageRefreshTimer !== null) return;
    const delayMs =
      DISPATCH_MESSAGE_REFRESH_THROTTLE_MS - (now - lastDispatchMessageRefreshAt);
    dispatchMessageRefreshTimer = window.setTimeout(
      () => runDispatchMessageRefresh(),
      Math.max(60, Math.floor(delayMs))
    );
  }

  function scheduleTeamRealtimeReconcile(immediate = false) {
    const now = Date.now();
    const run = () => {
      clearTeamRealtimeReconcileTimer();
      lastTeamRealtimeRefreshAt = Date.now();
      options.onRefresh();
    };
    if (immediate || now - lastTeamRealtimeRefreshAt >= TEAM_REALTIME_REFRESH_THROTTLE_MS) {
      run();
      return;
    }
    if (teamRealtimeReconcileTimer !== null) return;
    const delayMs = TEAM_REALTIME_REFRESH_THROTTLE_MS - (now - lastTeamRealtimeRefreshAt);
    teamRealtimeReconcileTimer = window.setTimeout(run, Math.max(80, Math.floor(delayMs)));
  }

  function scheduleSyncRequiredHistoryReload() {
    const run = () => {
      clearSyncRequiredHistoryReloadTimer();
      lastSyncRequiredHistoryReloadAt = Date.now();
      void loadManualChatHistory();
      scheduleTeamRealtimeReconcile(true);
    };
    const now = Date.now();
    if (
      shouldRunSyncRequiredReloadImmediately(
        now,
        lastSyncRequiredHistoryReloadAt,
        SYNC_REQUIRED_HISTORY_RELOAD_THROTTLE_MS
      )
    ) {
      run();
      return;
    }
    if (syncRequiredHistoryReloadTimer !== null) return;
    const delayMs = resolveSyncRequiredReloadDelayMs(
      now,
      lastSyncRequiredHistoryReloadAt,
      SYNC_REQUIRED_HISTORY_RELOAD_THROTTLE_MS
    );
    syncRequiredHistoryReloadTimer = window.setTimeout(run, delayMs);
  }

  chatRealtimeRuntime = createBeeroomChatRealtimeRuntime({
    getActiveGroupId: () => String(activeGroupId.value || '').trim(),
    getCursor: () => Number(chatRealtimeCursor.value || 0),
    setCursor: (cursor) => {
      chatRealtimeCursor.value = Math.max(0, Number(cursor || 0));
    },
    onPoll: () => loadManualChatHistory(),
    onEvent: ({ groupId, eventType, dataText, eventId, transport }) =>
      handleChatRealtimeEvent(groupId, eventType, dataText, eventId, transport),
    onTransportChange: () => undefined,
    isDisposed: () => false,
    isAuthDenied: () => chatAuthDenied,
    healthPollIntervalMs: CHAT_HEALTH_POLL_INTERVAL_MS,
    sseEventTypes: [...DEMO_RUNTIME_EVENT_TYPES, ...TEAM_RUNTIME_EVENT_TYPES]
  });

  function stopChatRealtimeWatch() {
    clearTeamRealtimeReconcileTimer();
    clearDispatchMessageRefreshTimer();
    pendingDispatchMessageRefresh = null;
    clearSyncRequiredHistoryReloadTimer();
    chatRealtimeRuntime?.stop();
  }

  function handleChatRealtimeEvent(
    groupId: string,
    eventType: string,
    dataText: string,
    _eventId: string,
    _transport: 'ws' | 'sse' = 'ws'
  ) {
    if (!groupId || groupId !== activeGroupId.value) return;
    const payload = safeJsonParse(dataText);
    const normalizedType = String(eventType || '').trim().toLowerCase();
    if (normalizedType === 'watching') return;
    if (normalizedType === 'sync_required') {
      logBeeroomRuntime('chat-realtime:sync-required', {
        groupId,
        dispatchSessionId: dispatchSessionId.value
      });
      scheduleSyncRequiredHistoryReload();
      return;
    }
    if (normalizedType === 'chat_cleared') {
      chatMessagesClearedAfter.value = Math.max(chatMessagesClearedAfter.value, Date.now() / 1000);
      manualChatMessages.value = [];
      replaceRuntimeRelayChatMessages([]);
      persistCachedChatState();
      mergeBeeroomMissionCanvasState(chatClearScopeKey.value, {
        chatClearedAfter: chatMessagesClearedAfter.value
      });
      logBeeroomRuntime('chat-realtime:chat-cleared', {
        groupId,
        clearedAfter: chatMessagesClearedAfter.value
      });
      return;
    }
    if (DEMO_RUNTIME_EVENT_TYPES.has(normalizedType)) {
      handleDemoRealtimeEvent(normalizedType, payload);
      return;
    }
    if (TEAM_RUNTIME_EVENT_TYPES.has(normalizedType)) {
      const accepted = beeroomStore.applyRealtimeEvent(groupId, normalizedType, payload);
      const forceWorkflowRefresh = shouldForceWorkflowRefresh(normalizedType);
      const forceImmediateReconcile = shouldForceImmediateTeamRealtimeReconcile({
        eventType: normalizedType,
        accepted
      });
      logBeeroomRuntime('chat-realtime:team-event', {
        groupId,
        eventType: normalizedType,
        accepted,
        forceWorkflowRefresh,
        forceImmediateReconcile
      });
      void nextTick(() => {
        logBeeroomRuntime('chat-realtime:subagent-sync', {
          groupId,
          eventType: normalizedType,
          force: forceWorkflowRefresh || forceImmediateReconcile
        });
        void syncMissionWorkflowState(forceWorkflowRefresh);
        void syncMissionSubagentState(forceWorkflowRefresh || forceImmediateReconcile);
      });
      if (normalizedType === 'team_start' || normalizedType === 'team_task_dispatch') {
        scheduleDispatchMessageRefresh(`team:${normalizedType}`, {
          hydrate: false,
          forceReplace: false,
          immediate: normalizedType === 'team_start'
        });
      } else if (
        normalizedType === 'team_task_result' ||
        normalizedType === 'team_finish' ||
        normalizedType === 'team_error'
      ) {
        scheduleDispatchMessageRefresh(`team:${normalizedType}`, {
          hydrate: true,
          forceReplace: true,
          immediate: true
        });
      }
      scheduleTeamRealtimeReconcile(forceImmediateReconcile);
    }
  }

  function startChatRealtimeWatch(groupId: string) {
    chatRealtimeRuntime?.activateGroup(groupId, { immediatePoll: false });
  }

  function restartChatPolling() {
    chatRealtimeRuntime?.triggerHealthPoll('restart');
  }

  onMounted(() => {
    logBeeroomRuntime('mounted', {
      groupId: activeGroupId.value,
      runtimeScopeKey: chatRuntimeScopeKey.value
    });
    handleActiveGroupChanged(activeGroupId.value);
  });

  onBeforeUnmount(() => {
    // Preserve the last dispatch session snapshot so returning to swarms can replay
    // the mother/worker/subagent canvas from cached session references.
    logBeeroomRuntime('before-unmount', {
      groupId: activeGroupId.value,
      dispatchSessionId: dispatchSessionId.value,
      runtimeStatus: dispatchRuntimeStatus.value
    });
    resetDispatchRuntime({ keepSession: true, keepRuntimeStatus: true });
    stopChatPolling();
    stopChatRealtimeWatch();
  });

  return {
    chatCollapsed,
    composerText,
    composerTargetAgentId,
    composerTargetOptions,
    composerSending,
    composerCanSend,
    composerError,
    demoError,
    demoActionDisabled,
    demoActionLabel,
    demoCanCancel,
    dispatchApprovals,
    dispatchApprovalBusy,
    dispatchCanResume,
    dispatchCanStop,
    dispatchRuntimeLabel,
    dispatchRuntimeStatus,
    dispatchRuntimeTone,
    dispatchSessionId,
    dispatchPreview: effectiveDispatchPreview,
    displayChatMessages,
    listRecentAgentOutputs,
    motherWorkflowItems,
    subagentsByTask,
    workflowItemsByTask,
    workflowItemsSignature,
    workflowPreviewByTask,
    workflowPreviewSignature,
    clearManualChatHistory,
    handleComposerSend,
    handleDispatchApproval,
    handleDispatchResume,
    handleDispatchStop,
    handleDemoAction,
    resolveAgentAvatarImageByAgentId,
    resolveAgentAvatarColorByAgentId,
    resolveMessageAvatarImage,
    avatarLabel
  };
};
