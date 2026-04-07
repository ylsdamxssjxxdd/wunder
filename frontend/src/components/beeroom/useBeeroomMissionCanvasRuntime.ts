import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, type Ref, watch } from 'vue';

import {
  appendBeeroomChatMessage,
  clearBeeroomChatMessages,
  listBeeroomChatMessages
} from '@/api/beeroom';
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
import {
  getBeeroomMissionCanvasState,
  mergeBeeroomMissionCanvasState
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import {
  resolveBeeroomMotherAgentId,
  resolveBeeroomSwarmScopeKey
} from '@/components/beeroom/canvas/swarmCanvasModel';
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
  type BeeroomMissionTask,
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

  const shortTaskId = (value: unknown) => {
    const text = String(value || '').trim();
    if (!text) return '-';
    if (text.length <= 12) return text;
    return `${text.slice(0, 6)}...${text.slice(-4)}`;
  };

  const resolveStatusLabel = (value: unknown) => {
    const normalized = String(value || '').trim().toLowerCase();
    const keyMap: Record<string, string> = {
      queued: 'beeroom.status.queued',
      running: 'beeroom.status.running',
      awaiting_idle: 'beeroom.status.awaitingIdle',
      completed: 'beeroom.status.completed',
      success: 'beeroom.status.completed',
      failed: 'beeroom.status.failed',
      error: 'beeroom.status.failed',
      timeout: 'beeroom.status.timeout',
      cancelled: 'beeroom.status.cancelled',
      idle: 'beeroom.members.idle'
    };
    return options.t(keyMap[normalized] || 'beeroom.status.unknown');
  };

  const resolveTaskTimelineMoment = (task: BeeroomMissionTask | null | undefined) =>
    Number(task?.updated_time || task?.finished_time || task?.started_time || 0);

  const resolveCollaborationMeta = (task: BeeroomMissionTask | null | undefined) => {
    if (!task) return '';
    if (String(task.session_run_id || '').trim()) {
      return `${options.t('beeroom.task.runId')} ${shortTaskId(task.session_run_id)}`;
    }
    const sessionId = task.spawned_session_id || task.target_session_id || '';
    if (String(sessionId || '').trim()) {
      return `${options.t('beeroom.task.sessionId')} ${shortTaskId(sessionId)}`;
    }
    return formatDateTime(task.updated_time || task.finished_time || task.started_time || 0);
  };

  const normalizeManualChatTone = (value: unknown): MissionChatMessage['tone'] => {
    const tone = String(value || '').trim().toLowerCase();
    if (tone === 'mother' || tone === 'worker' || tone === 'system' || tone === 'user') {
      return tone;
    }
    return 'system';
  };

  const mapApiChatMessage = (value: unknown): MissionChatMessage | null => {
    if (!value || typeof value !== 'object') return null;
    const payload = value as Record<string, unknown>;
    const messageId = Number(payload.message_id || 0);
    const time = Number(payload.created_at || payload.time || 0);
    if (!Number.isFinite(time) || time <= 0) return null;
    const body = String(payload.body || '').trim();
    if (!body) return null;
    const key = String(payload.key || '').trim() || `history:${messageId || time}`;
    return {
      key,
      senderName:
        String(payload.sender_name || payload.senderName || '').trim() || options.t('messenger.section.swarms'),
      senderAgentId: String(payload.sender_agent_id || payload.senderAgentId || '').trim(),
      mention: String(payload.mention_name || payload.mention || payload.mentionName || '').trim(),
      body,
      meta: String(payload.meta || '').trim(),
      time,
      timeLabel: formatDateTime(time),
      tone: normalizeManualChatTone(payload.tone)
    };
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
  };

  const replaceManualChatMessages = (messages: MissionChatMessage[]) => {
    manualChatMessages.value = [...messages]
      .filter(
        (message) =>
          !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
      )
      .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
      .slice(-MANUAL_CHAT_HISTORY_LIMIT);
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

  const resolveHttpStatus = (error: unknown): number => {
    const status = Number((error as { response?: { status?: unknown } })?.response?.status ?? 0);
    return Number.isFinite(status) ? status : 0;
  };

  const isAuthDeniedStatus = (status: number): boolean => status === 401 || status === 403;

  const loadManualChatHistory = async () => {
    if (chatAuthDenied) return;
    const groupId = activeGroupId.value;
    if (!groupId) {
      manualChatMessages.value = [];
      return;
    }
    try {
      const response = await listBeeroomChatMessages(groupId, { limit: MANUAL_CHAT_HISTORY_LIMIT });
      chatAuthDenied = false;
      const items = Array.isArray(response?.data?.data?.items)
        ? response.data.data.items
            .map((item: unknown) => mapApiChatMessage(item))
            .filter((item: MissionChatMessage | null): item is MissionChatMessage => !!item)
        : [];
      const next = [...items]
        .filter(
          (message) =>
            !chatMessagesClearedAfter.value || Number(message.time || 0) > chatMessagesClearedAfter.value
        )
        .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
        .slice(-MANUAL_CHAT_HISTORY_LIMIT);
      if (!sameManualChatMessages(manualChatMessages.value, next)) {
        replaceManualChatMessages(next);
      }
    } catch (error) {
      if (isAuthDeniedStatus(resolveHttpStatus(error))) {
        chatAuthDenied = true;
        stopChatRealtimeWatch();
        stopChatPolling();
      }
    }
  };

  const persistManualChatMessage = async (payload: {
    senderKind: string;
    senderName: string;
    senderAgentId?: string;
    mention?: string;
    mentionAgentId?: string;
    body: string;
    meta?: string;
    tone: MissionChatMessage['tone'];
    createdAt?: number;
    clientMsgId?: string;
  }) => {
    const groupId = activeGroupId.value;
    if (!groupId) {
      throw new Error(options.t('common.requestFailed'));
    }
    const response = await appendBeeroomChatMessage(groupId, {
      senderKind: payload.senderKind,
      senderName: payload.senderName,
      senderAgentId: payload.senderAgentId,
      mentionName: payload.mention,
      mentionAgentId: payload.mentionAgentId,
      body: payload.body,
      meta: payload.meta,
      tone: payload.tone,
      createdAt: payload.createdAt,
      clientMsgId: payload.clientMsgId
    });
    const message = mapApiChatMessage(response?.data?.data);
    if (!message) {
      throw new Error(options.t('common.requestFailed'));
    }
    appendManualChatMessage(message);
    return message;
  };

  const clearManualChatHistory = async () => {
    composerError.value = '';
    const clearedAfter = Date.now() / 1000;
    chatMessagesClearedAfter.value = Math.max(chatMessagesClearedAfter.value, clearedAfter);
    manualChatMessages.value = [];
    mergeBeeroomMissionCanvasState(chatClearScopeKey.value, {
      chatClearedAfter: chatMessagesClearedAfter.value
    });
    try {
      const groupId = activeGroupId.value;
      if (groupId) {
        await clearBeeroomChatMessages(groupId);
      }
    } catch {
      // Keep the panel cleared locally even if the server-side cleanup fails.
    }
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
    return clipMessageBody(candidates.find((item) => String(item || '').trim()) || '');
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

  const sortChatSessionsByActivity = (sessions: Record<string, unknown>[]): Record<string, unknown>[] =>
    [...sessions]
      .map((session, index) => ({ session, index }))
      .sort((left, right) => {
        const leftAt = toSessionTimestampMs(
          left.session.updated_at ?? left.session.last_message_at ?? left.session.created_at
        );
        const rightAt = toSessionTimestampMs(
          right.session.updated_at ?? right.session.last_message_at ?? right.session.created_at
        );
        if (leftAt !== rightAt) {
          return rightAt - leftAt;
        }
        return left.index - right.index;
      })
      .map((item) => item.session);

  const syncDispatchSessionToChatStore = (payload: {
    sessionId: string;
    agentId: string;
    sessionSummary: Record<string, unknown> | null;
    userPreview: string;
  }) => {
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
    const currentSessions = Array.isArray(chatStore.sessions)
      ? (chatStore.sessions as Record<string, unknown>[])
      : [];
    const targetIndex = currentSessions.findIndex((item) => String(item?.id || '').trim() === targetSessionId);
    const mergedSessions = [...currentSessions];
    if (targetIndex >= 0) {
      mergedSessions[targetIndex] = {
        ...mergedSessions[targetIndex],
        ...nextSession
      };
    } else {
      mergedSessions.unshift(nextSession);
    }
    chatStore.sessions = sortChatSessionsByActivity(mergedSessions);
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

  const resetDispatchRuntime = (options: { keepSession?: boolean } = {}) => {
    if (dispatchStreamController) {
      dispatchStreamController.abort();
      dispatchStreamController = null;
    }
    composerSending.value = false;
    dispatchStopRequested = false;
    dispatchRespondingApprovalId.value = '';
    dispatchRequestId.value = '';
    dispatchRuntimeStatus.value = 'idle';
    if (!options.keepSession) {
      dispatchSessionId.value = '';
      dispatchLastEventId.value = 0;
      dispatchTargetAgentId.value = '';
      dispatchTargetName.value = '';
      dispatchTargetTone.value = 'worker';
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
    payload: { content?: string; afterEventId?: number } = {}
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

    return consumeDispatchStream(response);
  };

  const persistDispatchReply = async (finalPayload: Record<string, any> | null) => {
    await persistManualChatMessage({
      senderKind: 'agent',
      senderName: dispatchTargetName.value || options.t('messenger.section.swarms'),
      senderAgentId: dispatchTargetAgentId.value,
      body: extractReplyText(finalPayload) || options.t('beeroom.canvas.chatDispatchAccepted'),
      meta: options.t('beeroom.canvas.chatResultMeta'),
      tone: dispatchTargetTone.value,
      createdAt: Math.floor(Date.now() / 1000),
      clientMsgId: nextManualMessageKey('reply')
    });
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
    try {
      await persistManualChatMessage({
        senderKind: 'user',
        senderName: options.t('chat.message.user'),
        mention: targetName,
        mentionAgentId: target.agentId,
        body: visibleBody,
        meta: options.group.value?.name || options.group.value?.group_id || '',
        tone: 'user',
        createdAt: now,
        clientMsgId: nextManualMessageKey('user')
      });
      await persistManualChatMessage({
        senderKind: 'system',
        senderName: options.t('messenger.section.swarms'),
        mention: targetName,
        mentionAgentId: target.agentId,
        body: options.t('beeroom.canvas.chatDispatchPending'),
        meta: options.group.value?.group_id || '',
        tone: 'system',
        createdAt: now,
        clientMsgId: nextManualMessageKey('dispatch')
      });

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
      });
      const finalPayload = await startDispatchStream('send', sessionId, { content: visibleBody });
      await persistDispatchReply(finalPayload);
    } catch (error: any) {
      if (error?.name === 'AbortError' || dispatchStopRequested) {
        dispatchRuntimeStatus.value = 'stopped';
        return;
      }
      const message = String(error?.message || '').trim() || options.t('common.requestFailed');
      dispatchRuntimeStatus.value = 'failed';
      composerError.value = message;
      try {
        await persistManualChatMessage({
          senderKind: 'system',
          senderName: options.t('messenger.section.swarms'),
          mention: targetName,
          mentionAgentId: target.agentId,
          body: options.t('beeroom.canvas.chatDispatchFailed'),
          meta: message,
          tone: 'system',
          createdAt: Math.floor(Date.now() / 1000),
          clientMsgId: nextManualMessageKey('error')
        });
      } catch {
        // Keep the original dispatch error visible even if chat persistence fails.
      }
      ElMessage.error(message);
    } finally {
      if (dispatchSessionId.value) {
        void chatStore.preloadSessionDetail(dispatchSessionId.value).catch(() => undefined);
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
    }
    try {
      await persistManualChatMessage({
        senderKind: 'system',
        senderName: options.t('messenger.section.swarms'),
        mention: dispatchTargetName.value,
        mentionAgentId: dispatchTargetAgentId.value,
        body: options.t('chat.workflow.aborted'),
        meta: options.t('chat.workflow.abortedDetail'),
        tone: 'system',
        createdAt: Math.floor(Date.now() / 1000),
        clientMsgId: nextManualMessageKey('abort')
      });
    } catch {
      // Ignore local command-log persistence failures.
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
      await persistDispatchReply(finalPayload);
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
        void chatStore.preloadSessionDetail(dispatchSessionId.value).catch(() => undefined);
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

  const missionChatMessages = computed(() => {
    const mission = options.mission.value;
    const messages: MissionChatMessage[] = [];

    if (!mission) {
      messages.push({
        key: 'standby',
        senderName: options.t('messenger.section.swarms'),
        senderAgentId: '',
        mention: '',
        body: options.t('beeroom.canvas.chatStandbyBody', { count: options.agents.value.length || 0 }),
        meta:
          options.group.value?.mother_agent_name || options.group.value?.mother_agent_id
            ? `${options.t('beeroom.summary.motherAgent')}: ${options.group.value?.mother_agent_name || options.group.value?.mother_agent_id}`
            : '',
        time: Number(options.group.value?.updated_time || options.group.value?.created_time || 0),
        timeLabel: formatDateTime(options.group.value?.updated_time || options.group.value?.created_time || 0),
        tone: 'system'
      });
      return messages;
    }

    const missionMotherAgentId = String(
      mission.mother_agent_id || options.group.value?.mother_agent_id || mission.entry_agent_id || ''
    ).trim();
    const motherName = resolveAgentNameById(missionMotherAgentId || options.group.value?.mother_agent_name || '');
    const entryName = resolveAgentNameById(mission.entry_agent_id || missionMotherAgentId || '');
    const kickoffBody = String(mission.summary || mission.strategy || options.t('beeroom.missions.noSummary')).trim();

    messages.push({
      key: `kickoff:${mission.mission_id || mission.team_run_id}`,
      senderName: entryName,
      senderAgentId: String(mission.entry_agent_id || missionMotherAgentId || '').trim(),
      mention: missionMotherAgentId && String(mission.entry_agent_id || '').trim() !== missionMotherAgentId ? motherName : '',
      body: kickoffBody,
      meta: `${options.t('beeroom.canvas.currentStatus')}: ${resolveStatusLabel(mission.completion_status || mission.status)}`,
      time: Number(mission.started_time || mission.updated_time || 0),
      timeLabel: formatDateTime(mission.started_time || mission.updated_time || 0),
      tone: 'mother'
    });

    (Array.isArray(mission.tasks) ? mission.tasks : []).forEach((task) => {
      const workerName = resolveAgentNameById(task.agent_id);
      const startedTime = Number(task.started_time || task.updated_time || 0);
      const finishedTime = Number(task.finished_time || task.updated_time || task.started_time || 0);
      const priority = Number(task.priority || 0);
      const taskId = shortTaskId(task.task_id);
      const runMeta = resolveCollaborationMeta(task);

      messages.push({
        key: `dispatch:${task.task_id}`,
        senderName: motherName,
        senderAgentId: missionMotherAgentId,
        mention: workerName,
        body: options.t('beeroom.canvas.chatDispatchBody', { taskId, priority }),
        meta: runMeta,
        time: startedTime,
        timeLabel: formatDateTime(startedTime),
        tone: 'mother'
      });

      const normalizedStatus = String(task.status || '').trim().toLowerCase();
      if (['queued', 'running', 'awaiting_idle'].includes(normalizedStatus)) {
        messages.push({
          key: `accept:${task.task_id}`,
          senderName: workerName,
          senderAgentId: String(task.agent_id || '').trim(),
          mention: motherName,
          body: options.t('beeroom.canvas.chatAcceptBody', { taskId }),
          meta: resolveStatusLabel(task.status),
          time: Number(task.updated_time || task.started_time || 0),
          timeLabel: formatDateTime(task.updated_time || task.started_time || 0),
          tone: 'worker'
        });
      }

      if (
        task.result_summary ||
        task.error ||
        ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(normalizedStatus)
      ) {
        messages.push({
          key: `result:${task.task_id}`,
          senderName: workerName,
          senderAgentId: String(task.agent_id || '').trim(),
          mention: motherName,
          body: String(task.result_summary || task.error || resolveStatusLabel(task.status)).trim(),
          meta: runMeta,
          time: finishedTime,
          timeLabel: formatDateTime(finishedTime),
          tone: 'worker'
        });
      }
    });

    return messages.sort((left, right) => left.time - right.time).slice(-24);
  });

  const displayChatMessages = computed(() =>
    [...missionChatMessages.value, ...manualChatMessages.value]
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
    chatAuthDenied = false;
    chatMessagesClearedAfter.value = Number(
      getBeeroomMissionCanvasState(groupId ? `chat:${groupId}` : chatClearScopeKey.value)?.chatClearedAfter || 0
    );
    chatRealtimeCursor.value = 0;
    resetDispatchRuntime();
    lastTeamRealtimeRefreshAt = 0;
    lastSyncRequiredHistoryReloadAt = 0;
    clearSyncRequiredHistoryReloadTimer();
    composerText.value = '';
    composerError.value = '';
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
      mergeBeeroomMissionCanvasState(chatClearScopeKey.value, {
        chatClearedAfter: chatMessagesClearedAfter.value
      });
      return;
    }
    if (normalizedType === 'chat_message') {
      const message = mapApiChatMessage(payload);
      if (message) {
        appendManualChatMessage(message);
      }
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
    resetDispatchRuntime();
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
