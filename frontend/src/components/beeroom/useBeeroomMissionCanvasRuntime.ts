import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, type Ref, watch } from 'vue';

import {
  cancelMessageStream,
  createSession,
  listSessions,
  resumeMessageStream,
  sendMessageStream
} from '@/api/chat';
import {
  ComposerTargetOption,
  DispatchApprovalItem,
  DispatchRuntimeStatus,
  MissionChatMessage
} from '@/components/beeroom/beeroomCanvasChatModel';
import { setBeeroomMissionChatState, getBeeroomMissionChatState } from '@/components/beeroom/beeroomMissionChatStateCache';
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import {
  resolveBeeroomMotherAgentId,
  resolveBeeroomSwarmScopeKey
} from '@/components/beeroom/canvas/swarmCanvasModel';
import { useBeeroomDispatchSessionPreview } from '@/components/beeroom/useBeeroomDispatchSessionPreview';
import { useBeeroomDemo } from '@/components/beeroom/useBeeroomDemo';
import { useBeeroomMissionWorkflowPreview } from '@/components/beeroom/useBeeroomMissionWorkflowPreview';
import { useBeeroomMissionSubagentPreview } from '@/components/beeroom/useBeeroomMissionSubagentPreview';
import {
  shouldForceImmediateTeamRealtimeReconcile,
  shouldForceWorkflowRefresh
} from '@/components/beeroom/beeroomRealtimeReconcile';
import {
  resolveSyncRequiredReloadDelayMs,
  shouldRunSyncRequiredReloadImmediately
} from '@/components/beeroom/beeroomRealtimeSyncGap';
import { createBeeroomChatRealtimeRuntime } from '@/realtime/beeroomChatRealtimeRuntime';
import { useChatStore } from '@/stores/chat';
import {
  type BeeroomGroup,
  type BeeroomMember,
  type BeeroomMission,
  useBeeroomStore
} from '@/stores/beeroom';
import { consumeSseStream } from '@/utils/sse';
import {
  parseAgentAvatarIconConfig,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

type DispatchSessionTarget = {
  sessionId: string;
  sessionSummary: Record<string, unknown> | null;
};

const MANUAL_CHAT_HISTORY_LIMIT = 120;
const CHAT_HEALTH_POLL_INTERVAL_MS = 30_000;
const TEAM_REALTIME_REFRESH_THROTTLE_MS = 360;
const SYNC_REQUIRED_HISTORY_RELOAD_THROTTLE_MS = 520;
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

export const useBeeroomMissionCanvasRuntime = (options: {
  group: Ref<BeeroomGroup | null>;
  mission: Ref<BeeroomMission | null>;
  agents: Ref<BeeroomMember[]>;
  t: TranslationFn;
  onRefresh: () => void;
}) => {
  const chatStore = useChatStore();
  const beeroomStore = useBeeroomStore();
  const chatCollapsed = ref(false);
  const manualChatMessages = ref<MissionChatMessage[]>([]);
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
  const dispatchRespondingApprovalId = ref('');
  const chatRealtimeCursor = ref(0);
  const chatMessagesClearedAfter = ref(0);

  let manualMessageSerial = 0;
  let chatAuthDenied = false;
  let teamRealtimeReconcileTimer: number | null = null;
  let lastTeamRealtimeRefreshAt = 0;
  let syncRequiredHistoryReloadTimer: number | null = null;
  let lastSyncRequiredHistoryReloadAt = 0;
  let dispatchStreamController: AbortController | null = null;
  let dispatchStopRequested = false;

  const missionScopeKey = computed(() =>
    resolveBeeroomSwarmScopeKey({
      missionId: options.mission.value?.mission_id,
      teamRunId: options.mission.value?.team_run_id,
      groupId: options.group.value?.group_id
    })
  );
  const activeGroupId = computed(() => String(options.group.value?.group_id || '').trim());
  const chatClearScopeKey = computed(() => {
    const groupId = String(activeGroupId.value || '').trim();
    if (groupId) return `chat:${groupId}`;
    return `chat:${missionScopeKey.value}`;
  });
  const chatRuntimeScopeKey = computed(() => {
    const groupId = String(activeGroupId.value || '').trim();
    if (groupId) return `runtime:${groupId}`;
    return `runtime:${missionScopeKey.value}`;
  });
  const motherAgentId = computed(() =>
    resolveBeeroomMotherAgentId(options.mission.value, options.group.value, options.agents.value)
  );

  const {
    workflowItemsByTask,
    workflowItemsSignature,
    workflowPreviewByTask,
    workflowPreviewSignature,
    syncMissionWorkflowState
  } = useBeeroomMissionWorkflowPreview({
    mission: computed(() => options.mission.value || null),
    t: options.t
  });

  const { subagentsByTask } = useBeeroomMissionSubagentPreview({
    mission: computed(() => options.mission.value || null),
    clearedAfter: chatMessagesClearedAfter
  });

  const { dispatchPreview } = useBeeroomDispatchSessionPreview({
    sessionId: dispatchSessionId,
    targetAgentId: dispatchTargetAgentId,
    targetName: dispatchTargetName,
    runtimeStatus: dispatchRuntimeStatus,
    clearedAfter: chatMessagesClearedAfter
  });

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

  const agentAvatarImageMap = computed(() => {
    const map = new Map<string, string>();
    options.agents.value.forEach((member) => {
      const agentId = String(member.agent_id || '').trim();
      if (!agentId) return;
      const imageUrl = resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig(member.icon));
      if (imageUrl) {
        map.set(agentId, imageUrl);
      }
    });
    return map;
  });

  const resolveAgentAvatarImageByAgentId = (agentId: unknown): string =>
    agentAvatarImageMap.value.get(String(agentId || '').trim()) || '';

  const avatarLabel = (value: unknown) => resolveAgentAvatarInitial(value);

  const resolveAgentNameById = (agentId: unknown) => {
    const normalized = String(agentId || '').trim();
    if (!normalized) return '-';
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
    return items;
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

  const resolveDispatchAssistantAgentId = (sessionId: string) => {
    const explicit = String(dispatchTargetAgentId.value || '').trim();
    if (explicit) return explicit;
    const summary = resolveDispatchSessionSummary(sessionId);
    const summaryAgentId = String(summary?.agent_id || '').trim();
    if (summaryAgentId) return summaryAgentId;
    if (summary?.is_default === true) {
      return DEFAULT_AGENT_KEY;
    }
    return '';
  };

  const resolveDispatchAssistantName = (sessionId: string) => {
    const explicitName = String(dispatchTargetName.value || '').trim();
    if (explicitName) return explicitName;
    const agentId = resolveDispatchAssistantAgentId(sessionId);
    if (agentId) return resolveAgentNameById(agentId);
    return options.t('messenger.defaultAgent');
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
      mention: '',
      body,
      meta: '',
      time,
      timeLabel: formatDateTime(time),
      tone: role === 'assistant' ? (dispatchTargetTone.value === 'mother' ? 'mother' : 'worker') : 'user'
    };
  };

  const readDispatchSessionMessages = (sessionId: string): MissionChatMessage[] => {
    const targetId = String(sessionId || '').trim();
    if (!targetId) return [];
    const source =
      String(chatStore.activeSessionId || '').trim() === targetId
        ? chatStore.messages
        : chatStore.getCachedSessionMessages(targetId);
    return (Array.isArray(source) ? source : [])
      .map((message, index) => mapSessionChatMessage(message, index, targetId))
      .filter((message: MissionChatMessage | null): message is MissionChatMessage => Boolean(message));
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
      .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    persistCachedChatState();
  };

  const replaceManualChatMessages = (messages: MissionChatMessage[]) => {
    manualChatMessages.value = [...messages]
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      )
      .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
    persistCachedChatState();
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
        leftItem.mention !== rightItem.mention ||
        leftItem.body !== rightItem.body ||
        leftItem.meta !== rightItem.meta
      ) {
        return false;
      }
    }
    return true;
  };

  const readCachedChatState = (scopeKey = chatRuntimeScopeKey.value) => getBeeroomMissionChatState(scopeKey);

  const persistCachedChatState = (scopeKey = chatRuntimeScopeKey.value) => {
    setBeeroomMissionChatState(scopeKey, {
      version: 1,
      manualMessages: manualChatMessages.value,
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
    replaceManualChatMessages(cachedMessages);
    const cachedDispatch = cached?.dispatch;
    if (!cachedDispatch) return;
    dispatchSessionId.value = String(cachedDispatch.sessionId || '').trim();
    dispatchLastEventId.value = Math.max(0, Number(cachedDispatch.lastEventId || 0));
    dispatchTargetAgentId.value = String(cachedDispatch.targetAgentId || '').trim();
    dispatchTargetName.value = String(cachedDispatch.targetName || '').trim();
    dispatchTargetTone.value =
      cachedDispatch.targetTone === 'mother' ? 'mother' : 'worker';
    dispatchRuntimeStatus.value = cachedDispatch.runtimeStatus || 'idle';
  };

  const resolveLatestVisibleUserPreview = () => {
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
        sessionSummary: resolveDispatchSessionSummary(targetId),
        userPreview: resolveLatestVisibleUserPreview()
      },
      { remember }
    );
  };

  const syncDispatchSessionMessages = async (
    loadOptions: { hydrate?: boolean; clearWhenEmpty?: boolean } = {}
  ) => {
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) {
      if (loadOptions.clearWhenEmpty) {
        replaceManualChatMessages([]);
      }
      return [];
    }

    const applyFromCache = () => {
      const next = readDispatchSessionMessages(sessionId)
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
      if (next.length > 0) {
        if (!sameManualChatMessages(manualChatMessages.value, next)) {
          replaceManualChatMessages(next);
        }
      } else if (loadOptions.clearWhenEmpty) {
        replaceManualChatMessages([]);
      }
      return next;
    };

    ensureDispatchSessionKnown(sessionId, false);
    const cached = applyFromCache();
    if (loadOptions.hydrate === false) {
      return cached;
    }
    try {
      await chatStore.preloadSessionDetail(sessionId);
    } catch {
      return cached;
    }
    return applyFromCache();
  };

  const loadManualChatHistory = async () => {
    const cachedState = readCachedChatState();
    const cachedMessages = Array.isArray(cachedState?.manualMessages) ? cachedState.manualMessages : [];
    if (!String(dispatchSessionId.value || '').trim()) {
      const next = [...cachedMessages]
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
      if (!sameManualChatMessages(manualChatMessages.value, next)) {
        replaceManualChatMessages(next);
      }
      return;
    }
    if (!sameManualChatMessages(manualChatMessages.value, cachedMessages) && cachedMessages.length > 0) {
      replaceManualChatMessages(cachedMessages);
    }
    await syncDispatchSessionMessages({ hydrate: true });
  };

  const clearManualChatHistory = async () => {
    composerError.value = '';
    const clearedAfter = Date.now() / 1000;
    chatMessagesClearedAfter.value = Math.max(chatMessagesClearedAfter.value, clearedAfter);
    manualChatMessages.value = [];
    persistCachedChatState();
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

  const syncDispatchSessionToChatStore = (payload: {
    sessionId: string;
    agentId: string;
    sessionSummary: Record<string, unknown> | null;
    userPreview: string;
  }, syncOptions: { remember?: boolean } = {}) => {
    const targetSessionId = String(payload.sessionId || '').trim();
    if (!targetSessionId) return;
    const nowIso = new Date().toISOString();
    const preview = clipMessageBody(payload.userPreview, 120);
    const fallbackAgentId = normalizeDispatchAgentId(payload.agentId);
    const summary = payload.sessionSummary || null;
    const summaryAgentId = String(summary?.agent_id || '').trim();
    const nextSession: Record<string, unknown> = {
      ...(summary || {}),
      id: targetSessionId,
      agent_id: summaryAgentId || fallbackAgentId,
      updated_at: nowIso,
      last_message_at: nowIso
    };
    if (preview) {
      nextSession.last_message_preview = preview;
      nextSession.last_user_message_preview = preview;
    }
    if (!String(nextSession.title || '').trim()) {
      nextSession.title = preview || options.t('chat.newSession');
    }
    chatStore.syncSessionSummary(nextSession, {
      agentId: fallbackAgentId,
      remember: syncOptions.remember === true
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
      mention: '',
      body: text,
      meta: '',
      time: createdAt,
      timeLabel: formatDateTime(createdAt),
      tone: role === 'assistant' ? (dispatchTargetTone.value === 'mother' ? 'mother' : 'worker') : 'user'
    };
  };

  const ensureDispatchSession = async (agentId: string): Promise<DispatchSessionTarget> => {
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
    const primary = matched.find((item) => item?.is_main === true) || matched[0];
    if (primary?.id) {
      return {
        sessionId: String(primary.id),
        sessionSummary: primary && typeof primary === 'object' ? (primary as Record<string, unknown>) : null
      };
    }
    const created = await createSession(agentId === DEFAULT_AGENT_KEY ? {} : { agent_id: agentId });
    const createdSummary = created?.data?.data;
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
        return;
      }
      if (eventType === 'approval_result') {
        chatStore.resolveApprovalResult(data);
        const status = String(data?.status || payload?.status || '').trim().toLowerCase();
        dispatchRuntimeStatus.value = status === 'approved' ? 'running' : 'failed';
        return;
      }
      if (eventType === 'queued') {
        dispatchRuntimeStatus.value = 'queued';
        return;
      }
      if (eventType === 'slow_client') {
        dispatchRuntimeStatus.value = 'stopped';
        return;
      }
      if (eventType === 'error') {
        streamError = extractErrorText(payload) || options.t('common.requestFailed');
        dispatchRuntimeStatus.value = 'failed';
        return;
      }
      if (eventType === 'final') {
        finalPayload = payload;
        dispatchRuntimeStatus.value = 'completed';
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
            { signal: controller.signal }
          );

    if (!response.ok) {
      const errorText = String(await response.text()).trim();
      throw new Error(
        errorText || (mode === 'resume' ? options.t('chat.error.resumeFailed') : options.t('common.requestFailed'))
      );
    }

    streamOptions.onAccepted?.();
    return consumeDispatchStream(response);
  };

  const handleComposerSend = async () => {
    if (composerSending.value) return;
    const content = String(composerText.value || '').trim();
    if (!content) return;

    const { target, body } = resolveDispatchTarget(content);
    if (!target?.agentId) {
      const message = options.t('beeroom.canvas.chatTargetRequired');
      composerError.value = message;
      ElMessage.warning(message);
      return;
    }

    const targetName = resolveAgentNameById(target.agentId);
    const now = Math.floor(Date.now() / 1000);
    const visibleBody = String(body || content).trim();
    const targetTone = target.role === 'mother' ? 'mother' : 'worker';

    composerError.value = '';
    composerText.value = '';
    dispatchTargetAgentId.value = target.agentId;
    dispatchTargetName.value = targetName;
    dispatchTargetTone.value = targetTone;
    const localUserMessage = buildVisibleChatMessage('user', visibleBody, now);
    let localUserAccepted = false;
    try {
      const dispatchSession = await ensureDispatchSession(target.agentId);
      const sessionId = String(dispatchSession.sessionId || '').trim();
      if (!sessionId) {
        throw new Error(options.t('common.requestFailed'));
      }
      dispatchSessionId.value = sessionId;
      dispatchRequestId.value = nextManualMessageKey('dispatch-request');
      dispatchLastEventId.value = 0;
      dispatchRuntimeStatus.value = 'queued';
      syncDispatchSessionToChatStore({
        sessionId,
        agentId: target.agentId,
        sessionSummary: dispatchSession.sessionSummary,
        userPreview: visibleBody
      }, { remember: true });
      await syncDispatchSessionMessages({ hydrate: true, clearWhenEmpty: true });
      const finalPayload = await startDispatchStream(
        'send',
        sessionId,
        { content: visibleBody },
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
      const assistantMessage = buildVisibleChatMessage('assistant', replyText);
      if (assistantMessage) {
        appendManualChatMessage(assistantMessage);
      }
    } catch (error: any) {
      if (error?.name === 'AbortError' || dispatchStopRequested) {
        dispatchRuntimeStatus.value = 'stopped';
        return;
      }
      const message = String(error?.message || '').trim() || options.t('common.requestFailed');
      dispatchRuntimeStatus.value = 'failed';
      composerError.value = message;
      ElMessage.error(message);
    } finally {
      if (dispatchSessionId.value) {
        await syncDispatchSessionMessages({ hydrate: true });
      }
      dispatchStreamController = null;
      composerSending.value = false;
    }
  };

  const handleDispatchStop = async () => {
    if (!dispatchCanStop.value) return;
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) return;
    dispatchStopRequested = true;
    dispatchRuntimeStatus.value = 'stopped';
    if (dispatchStreamController) {
      dispatchStreamController.abort();
      dispatchStreamController = null;
    }
    try {
      await cancelMessageStream(sessionId);
    } catch {
      // Keep local interrupt behavior even if cancel API fails.
    } finally {
      composerSending.value = false;
      await syncDispatchSessionMessages({ hydrate: true });
    }
  };

  const handleDispatchResume = async () => {
    if (!dispatchCanResume.value) return;
    const sessionId = String(dispatchSessionId.value || '').trim();
    if (!sessionId) return;
    composerError.value = '';
    try {
      const finalPayload = await startDispatchStream('resume', sessionId, {
        afterEventId: dispatchLastEventId.value
      });
      const replyText = extractReplyText(finalPayload);
      const assistantMessage = buildVisibleChatMessage('assistant', replyText);
      if (assistantMessage) {
        appendManualChatMessage(assistantMessage);
      }
    } catch (error: any) {
      if (error?.name === 'AbortError' || dispatchStopRequested) {
        dispatchRuntimeStatus.value = 'stopped';
        return;
      }
      const message = String(error?.message || '').trim() || options.t('chat.error.resumeFailed');
      dispatchRuntimeStatus.value = 'failed';
      composerError.value = message;
      ElMessage.error(message);
    } finally {
      if (dispatchSessionId.value) {
        await syncDispatchSessionMessages({ hydrate: true });
      }
      dispatchStreamController = null;
      composerSending.value = false;
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

  const displayChatMessages = computed(() =>
    [...manualChatMessages.value]
      .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      )
  );

  watch(
    composerTargetOptions,
    (optionsList) => {
      if (!optionsList.length) {
        composerTargetAgentId.value = '';
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
    const runtimeScopeKey = groupId ? `runtime:${groupId}` : chatRuntimeScopeKey.value;
    chatAuthDenied = false;
    chatMessagesClearedAfter.value = Number(
      getBeeroomMissionCanvasState(groupId ? `chat:${groupId}` : chatClearScopeKey.value)?.chatClearedAfter || 0
    );
    chatRealtimeCursor.value = 0;
    resetDispatchRuntime({ persist: false });
    lastTeamRealtimeRefreshAt = 0;
    lastSyncRequiredHistoryReloadAt = 0;
    clearSyncRequiredHistoryReloadTimer();
    composerText.value = '';
    composerError.value = '';
    restoreCachedChatState(runtimeScopeKey);
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
    }
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

  function stopChatPolling() {
    chatRealtimeRuntime.stopHealthPolling();
  }

  function clearTeamRealtimeReconcileTimer() {
    if (teamRealtimeReconcileTimer !== null) {
      window.clearTimeout(teamRealtimeReconcileTimer);
      teamRealtimeReconcileTimer = null;
    }
  }

  function clearSyncRequiredHistoryReloadTimer() {
    if (syncRequiredHistoryReloadTimer !== null) {
      window.clearTimeout(syncRequiredHistoryReloadTimer);
      syncRequiredHistoryReloadTimer = null;
    }
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

  const chatRealtimeRuntime = createBeeroomChatRealtimeRuntime({
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
    clearSyncRequiredHistoryReloadTimer();
    chatRealtimeRuntime.stop();
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
      scheduleSyncRequiredHistoryReload();
      return;
    }
    if (normalizedType === 'chat_cleared') {
      chatMessagesClearedAfter.value = Math.max(chatMessagesClearedAfter.value, Date.now() / 1000);
      manualChatMessages.value = [];
      persistCachedChatState();
      mergeBeeroomMissionCanvasState(chatClearScopeKey.value, {
        chatClearedAfter: chatMessagesClearedAfter.value
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
      void nextTick(() => syncMissionWorkflowState(forceWorkflowRefresh));
      scheduleTeamRealtimeReconcile(forceImmediateReconcile);
    }
  }

  function startChatRealtimeWatch(groupId: string) {
    chatRealtimeRuntime.activateGroup(groupId, { immediatePoll: false });
  }

  function restartChatPolling() {
    chatRealtimeRuntime.triggerHealthPoll('restart');
  }

  onMounted(() => {
    handleActiveGroupChanged(activeGroupId.value);
  });

  onBeforeUnmount(() => {
    // Preserve the last dispatch session snapshot so returning to swarms can replay
    // the mother/worker/subagent canvas from cached session references.
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
    dispatchPreview,
    displayChatMessages,
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
    avatarLabel
  };
};
