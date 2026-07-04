import { defineStore } from 'pinia';

import {
  archiveSession as archiveSessionApi,
  cancelMessageStream,
  compactSession as compactSessionApi,
  controlSessionSubagents as controlSessionSubagentsApi,
  createSession,
  deleteSession as deleteSessionApi,
  getSession,
  getSessionGoal,
  getSessionEvents,
  getSessionHistoryPage,
  getSessionSubagents,
  listSessions,
  openChatSocket,
  renameSession as renameSessionApi,
  restoreSession as restoreSessionApi,
  setSessionGoal as setSessionGoalApi,
  submitMessageFeedback as submitMessageFeedbackApi,
  updateSessionTools as updateSessionToolsApi
} from '@/api/chat';
import { t } from '@/i18n';
import { setDefaultSession } from '@/api/agents';
import { formatStructuredErrorText } from '@/utils/streamError';
import { resolveCompactionProgressTitle } from '@/utils/chatCompactionUi';
import {
  buildChatRequestTextInputOverflowError,
  resolveChatRequestTextInputOverflow
} from '@/utils/chatRequestInputLimit';
import {
  hasActiveSubagentsAfterLatestUser,
  hasRunningAssistantMessage,
  hasStreamingAssistantMessage,
  isSessionBusyFromSignals,
  isThreadRuntimeBusy,
  isThreadRuntimeWaiting,
  normalizeThreadRuntimeStatus
} from '@/utils/chatSessionRuntime';
import {
  isSubagentItemActive,
  normalizeSubagentRuntimeFlag,
  isSubagentStatusFailed,
  isSubagentStatusSuccessful,
  normalizeSubagentRuntimeStatus
} from '@/utils/subagentRuntime';
import { normalizeChatDurationSeconds, normalizeChatTimestampMs } from '@/utils/chatTiming';
import {
  mergeSessionsByIdPreservingRuntimeFields
} from '@/stores/chatSessionMerge';
import {
  estimateChatTextTokens,
  estimateRequestContextTokens,
  resolveRequestContextPreviewTokens
} from '@/utils/chatContextEstimate';
import { resolveWorkflowDurationMs } from '@/utils/toolWorkflowTiming';
import { summarizeTurnDecodeSpeed } from '@/utils/turnDecodeSpeed';
import {
  normalizeMessageFeedback,
  normalizeMessageFeedbackVote,
  resolveMessageHistoryId
} from '@/utils/messageFeedback';
import { createWsMultiplexer } from '@/utils/ws';
import { isDemoMode, loadDemoChatState, saveDemoChatState } from '@/utils/demo';
import { emitAgentRuntimeRefresh, emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { chatPerf } from '@/utils/chatPerf';
import { chatDebugLog, isChatDebugEnabled } from '@/utils/chatDebug';
import { getDesktopToolCallModeForRequest, isDesktopModeEnabled } from '@/config/desktop';
import { resolveAccessToken } from '@/api/requestAuth';
import {
  createChatRuntimeProjection,
  applyChatRuntimeEvent
} from '@/realtime/chat/chatRuntimeReducer';
import {
  selectLegacyMessageStatus,
  selectVisibleMessageProjections,
  selectSessionBusy,
  selectSessionBusyReason,
  selectSessionRuntimeStatus
} from '@/realtime/chat/chatRuntimeSelectors';
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
import {
  clearTrailingPendingAssistantMessages,
  clearSupersededPendingAssistantMessages,
  findPendingAssistantMessage,
  isPendingAssistantMessage,
  stopPendingAssistantMessage
} from './chatPendingMessage';
import {
  captureChatSnapshotScheduleContext,
  resolveChatSnapshotScheduleSource
} from './chatSnapshotScheduler';
import { resolveInteractiveControllerRecoveryReason } from './chatInteractiveRuntimeRecovery';
import {
  normalizeStreamLifecyclePhase,
  shouldForcePreserveWatcherForActiveSession,
  shouldApplyForegroundDetailHydration,
  shouldKeepForegroundInteractiveRuntime,
  shouldKeepForegroundLiveMessagesDuringRunningGap,
  shouldKeepForegroundLiveMessages,
  shouldRestartWatchAfterInteractiveStream
} from './chatWatchLifecycle';
import { isCompactionSummaryEvent } from '@/utils/chatCompactionWorkflow';
import {
  dedupeTerminalCompactionMarkersInPlace,
  isCompactionMarkerAssistantMessage,
  isSupersededRunningManualCompactionMarker,
  mergeCompactionMarkersIntoMessages,
  shouldPreserveTerminalCompactionMarkerState
} from './chatCompactionMarker';
import {
  replaceMessageArrayKeepingReference,
  resolveRealtimeMessageArrayReference
} from './chatMessageArraySync';
import { useCommandSessionStore } from './commandSessions';
import { hasRetainedMessageConversationContext as hasRetainedConversationContext } from '@/views/messenger/messageConversationRetention';

import { syncDemoChatCache } from './chatDemoPanels';
import { filterSessionsByAgent, resolveSessionKey, writeSessionListCache } from './chatRuntimeState';
import { DesktopOverlayBridge, SessionGoal, SessionOrchestrationLock } from './chatTypes';

export const CHAT_STATE_KEY = 'beeroom-chat-state';
export const LEGACY_CHAT_STATE_KEY = 'wille-chat-state';
export const CHAT_STATE_KEY_PREFIX = 'beeroom-chat-state:';
export const LEGACY_CHAT_STATE_KEY_PREFIX = 'wille-chat-state:';
export const DEFAULT_AGENT_KEY = '__default__';
export const CHAT_SNAPSHOT_KEY = 'beeroom-chat-snapshot';
export const LEGACY_CHAT_SNAPSHOT_KEY = 'wille-chat-snapshot';
export const CHAT_SNAPSHOT_KEY_PREFIX = 'beeroom-chat-snapshot:';
export const LEGACY_CHAT_SNAPSHOT_KEY_PREFIX = 'wille-chat-snapshot:';
export const CHAT_STORAGE_ANONYMOUS_SCOPE = 'anonymous';

export const normalizeAgentKey = (agentId) => {
  const cleaned = String(agentId || '').trim();
  return cleaned || DEFAULT_AGENT_KEY;
};

export const normalizeSessionMap = (value) => {
  if (!value || typeof value !== 'object') {
    return {};
  }
  const output = {};
  Object.entries(value).forEach(([key, sessionId]) => {
    const cleanedKey = String(key || '').trim();
    const cleanedSessionId = String(sessionId || '').trim();
    if (cleanedKey && cleanedSessionId) {
      output[cleanedKey] = cleanedSessionId;
    }
  });
  return output;
};

export const buildChatPersistState = () => ({
  activeSessionId: '',
  draft: false,
  lastSessionByAgent: {}
});

export const resolveChatStorageScope = (): string => {
  const token = String(resolveAccessToken() || '').trim();
  if (token) {
    return token;
  }
  return CHAT_STORAGE_ANONYMOUS_SCOPE;
};

export const buildScopedStorageKey = (prefix: string, scope: string): string => `${prefix}${scope}`;

export const resolveChatStateStorageKeys = () => {
  const scope = resolveChatStorageScope();
  return {
    primary: buildScopedStorageKey(CHAT_STATE_KEY_PREFIX, scope),
    legacyScoped: buildScopedStorageKey(LEGACY_CHAT_STATE_KEY_PREFIX, scope),
    globalPrimary: CHAT_STATE_KEY,
    globalLegacy: LEGACY_CHAT_STATE_KEY
  };
};

export const resolveChatSnapshotStorageKeys = () => {
  const scope = resolveChatStorageScope();
  return {
    primary: buildScopedStorageKey(CHAT_SNAPSHOT_KEY_PREFIX, scope),
    legacyScoped: buildScopedStorageKey(LEGACY_CHAT_SNAPSHOT_KEY_PREFIX, scope),
    globalPrimary: CHAT_SNAPSHOT_KEY,
    globalLegacy: LEGACY_CHAT_SNAPSHOT_KEY
  };
};

export const normalizeChatPersistState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildChatPersistState();
  }
  return {
    activeSessionId: typeof value.activeSessionId === 'string' ? value.activeSessionId : '',
    draft: value.draft === true,
    lastSessionByAgent: normalizeSessionMap(value.lastSessionByAgent)
  };
};

export const readChatPersistState = () => {
  try {
    const keys = resolveChatStateStorageKeys();
    const raw =
      localStorage.getItem(keys.primary) ??
      localStorage.getItem(keys.legacyScoped) ??
      localStorage.getItem(keys.globalPrimary) ??
      localStorage.getItem(keys.globalLegacy);
    if (!raw) return buildChatPersistState();
    if (!localStorage.getItem(keys.primary)) {
      localStorage.setItem(keys.primary, raw);
    }
    if (!localStorage.getItem(keys.legacyScoped)) {
      localStorage.setItem(keys.legacyScoped, raw);
    }
    return normalizeChatPersistState(JSON.parse(raw));
  } catch (error) {
    return buildChatPersistState();
  }
};

export const updateChatPersistState = (updater) => {
  try {
    const current = readChatPersistState();
    const next = normalizeChatPersistState(updater(current));
    const serialized = JSON.stringify(next);
    const keys = resolveChatStateStorageKeys();
    localStorage.setItem(keys.primary, serialized);
    localStorage.setItem(keys.legacyScoped, serialized);
  } catch (error) {
    // ignore persistence errors
  }
};

export const updateAgentSessionMap = (map, agentId, sessionId) => {
  const key = normalizeAgentKey(agentId);
  const cleanedSessionId = String(sessionId || '').trim();
  const nextMap = { ...map };
  if (cleanedSessionId) {
    nextMap[key] = cleanedSessionId;
  } else {
    delete nextMap[key];
  }
  return nextMap;
};

export const normalizeSessionOrchestrationLock = (value): SessionOrchestrationLock | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const source = value as Record<string, unknown>;
  const orchestrationId = String(source.orchestration_id || '').trim();
  const runId = String(source.run_id || '').trim();
  if (!orchestrationId || !runId) {
    return null;
  }
  return {
    active: source.active === true,
    group_id: String(source.group_id || '').trim(),
    orchestration_id: orchestrationId,
    run_id: runId,
    mother_agent_id: String(source.mother_agent_id || '').trim(),
    role: String(source.role || '').trim()
  };
};

export const normalizeGoalNumber = (value): number | null => {
  if (value === null || value === undefined || value === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

export const normalizeSessionGoal = (value): SessionGoal | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const source = value as Record<string, unknown>;
  const goalId = String(source.goal_id ?? source.goalId ?? '').trim();
  const objective = String(source.objective ?? '').trim();
  const status = String(source.status ?? '').trim().toLowerCase();
  if (!goalId || !objective || !status) {
    return null;
  }
  return {
    goal_id: goalId,
    session_id: String(source.session_id ?? source.sessionId ?? '').trim(),
    user_id: String(source.user_id ?? source.userId ?? '').trim(),
    objective,
    status,
    token_budget: normalizeGoalNumber(source.token_budget ?? source.tokenBudget),
    tokens_used: Math.max(0, Math.trunc(normalizeGoalNumber(source.tokens_used ?? source.tokensUsed) ?? 0)),
    time_used_seconds: Math.max(
      0,
      Math.trunc(normalizeGoalNumber(source.time_used_seconds ?? source.timeUsedSeconds) ?? 0)
    ),
    created_at: normalizeGoalNumber(source.created_at ?? source.createdAt),
    updated_at: normalizeGoalNumber(source.updated_at ?? source.updatedAt),
    completed_at: normalizeGoalNumber(source.completed_at ?? source.completedAt),
    last_continued_at: normalizeGoalNumber(source.last_continued_at ?? source.lastContinuedAt),
    source: String(source.source ?? '').trim()
  };
};

export const isGoalActiveForLock = (goal): boolean => {
  const status = String(goal?.status || '').trim().toLowerCase();
  return status === 'active' || status === 'paused';
};

export const isGoalCompletedForNotice = (goal): boolean => {
  const status = String(goal?.status || '').trim().toLowerCase();
  return status === 'complete';
};

export const isGoalMarkerMessage = (message): boolean =>
  Boolean(
    message &&
      message.role === 'assistant' &&
      String(message.content || '').trim() &&
      (message.manual_goal_marker === true || message.manualGoalMarker === true)
  );

export const goalSessionIdFromPayload = (value, fallback = ''): string => {
  const source =
    value && typeof value === 'object' && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  return resolveSessionKey(
    source.session_id ?? source.sessionId ?? source.id ?? fallback
  );
};

export const writeSessionGoalState = (
  store,
  sessionId,
  goal,
  options: { clear?: boolean } = {}
): SessionGoal | null => {
  if (!store || typeof store !== 'object') {
    return null;
  }
  const key = resolveSessionKey(sessionId) || goalSessionIdFromPayload(goal);
  if (!key) {
    return null;
  }
  const normalizedGoal = normalizeSessionGoal(goal);
  if (!store.sessionGoals || typeof store.sessionGoals !== 'object') {
    store.sessionGoals = {};
  }
  if (!normalizedGoal || options.clear === true) {
    delete store.sessionGoals[key];
  } else {
    store.sessionGoals[key] = {
      ...normalizedGoal,
      session_id: normalizedGoal.session_id || key
    };
  }
  if (Array.isArray(store.sessions)) {
    const index = store.sessions.findIndex((item) => resolveSessionKey(item?.id) === key);
    if (index >= 0) {
      const current = store.sessions[index] || {};
      const next =
        normalizedGoal && options.clear !== true
          ? { ...current, goal: store.sessionGoals[key] }
          : { ...current };
      if (!normalizedGoal || options.clear === true) {
        delete next.goal;
      }
      store.sessions[index] = patchSessionRuntimeFields(next);
      const agentId = String(store.sessions[index]?.agent_id || '').trim();
      writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
    }
  }
  syncDemoChatCache({ sessions: Array.isArray(store.sessions) ? store.sessions : [] });
  return normalizedGoal && options.clear !== true ? store.sessionGoals[key] : null;
};

export const syncGoalFromSessionRecord = (store, session): SessionGoal | null => {
  const key = resolveSessionKey(session?.id ?? session?.session_id ?? session?.sessionId);
  if (!key) {
    return null;
  }
  if (Object.prototype.hasOwnProperty.call(session || {}, 'goal')) {
    return writeSessionGoalState(store, key, session?.goal, { clear: !session?.goal });
  }
  return null;
};

export const hasManualGoalMarkerMessage = (messages: unknown[]): boolean =>
  Array.isArray(messages) &&
  messages.some((message) => {
    if (!message || typeof message !== 'object' || Array.isArray(message)) return false;
    const record = message as Record<string, unknown>;
    return record.manual_goal_marker === true || record.manualGoalMarker === true;
  });

export const syncGoalsFromSessionList = (store, sessions) => {
  (Array.isArray(sessions) ? sessions : []).forEach((session) => {
    syncGoalFromSessionRecord(store, session);
  });
};

export const applyGoalStreamEvent = (store, sessionId, eventType, payload): boolean => {
  const normalizedEventType = String(eventType || '').trim();
  if (
    normalizedEventType !== 'goal_updated' &&
    normalizedEventType !== 'goal_continuation_started' &&
    normalizedEventType !== 'goal_budget_limited' &&
    normalizedEventType !== 'goal_cleared'
  ) {
    return false;
  }
  const data =
    payload && typeof payload === 'object' && !Array.isArray(payload)
      ? (payload as Record<string, unknown>)
      : {};
  const goal = data.goal ?? (data.data as Record<string, unknown> | undefined)?.goal;
  const key = resolveSessionKey(sessionId) || goalSessionIdFromPayload(goal);
  writeSessionGoalState(store, key, goal, {
    clear: normalizedEventType === 'goal_cleared'
  });
  return true;
};

export const patchSessionRuntimeFields = (session) => {
  if (!session || typeof session !== 'object') {
    return session;
  }
  let next: Record<string, unknown> | null = null;
  const normalizedLock = normalizeSessionOrchestrationLock(
    (session as Record<string, unknown>).orchestration_lock
  );
  if (!normalizedLock) {
    if (Object.prototype.hasOwnProperty.call(session, 'orchestration_lock')) {
      next = { ...(session as Record<string, unknown>) };
      delete next.orchestration_lock;
    }
  } else {
    next = { ...(session as Record<string, unknown>), orchestration_lock: normalizedLock };
  }
  const source = (next || session) as Record<string, unknown>;
  const normalizedGoal = normalizeSessionGoal(source.goal);
  if (!normalizedGoal) {
    if (Object.prototype.hasOwnProperty.call(source, 'goal')) {
      next = { ...source };
      delete next.goal;
    }
  } else {
    next = { ...source, goal: normalizedGoal };
  }
  return next || session;
};

export const resolveErrorCode = (error) =>
  String(
    error?.response?.data?.error?.code ||
      error?.response?.data?.detail?.code ||
      error?.response?.data?.code ||
      ''
  )
    .trim()
    .toUpperCase();

export const persistAgentSession = (agentId, sessionId) => {
  updateChatPersistState((current) => ({
    ...current,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, sessionId)
  }));
};

export const persistActiveSession = (sessionId, agentId) => {
  const cleanedSessionId = String(sessionId || '').trim();
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: cleanedSessionId,
    draft: false,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, cleanedSessionId)
  }));
};

export const applyMainSession = (sessions, agentId, sessionId) => {
  const normalizedAgent = String(agentId || '').trim();
  const normalizedSessionId = String(sessionId || '').trim();
  return sessions.map((session) => {
    const sessionAgentId = String(session.agent_id || '').trim();
    const isMatch = normalizedAgent ? sessionAgentId === normalizedAgent : !sessionAgentId;
    if (!isMatch) return session;
    const isMain = Boolean(normalizedSessionId && session.id === normalizedSessionId);
    if (session.is_main === isMain) return session;
    return { ...session, is_main: isMain };
  });
};

export const persistDraftSession = () => {
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: '',
    draft: true
  }));
};

export const resolvePersistedSessionId = (agentId) => {
  const key = normalizeAgentKey(agentId);
  const state = readChatPersistState();
  return state.lastSessionByAgent?.[key] || '';
};

export const getDesktopOverlayBridge = (): DesktopOverlayBridge | null => {
  if (typeof window === 'undefined') {
    return null;
  }
  const candidate = (window as Window & { wunderDesktop?: DesktopOverlayBridge }).wunderDesktop;
  return candidate || null;
};

export const applyDesktopOverlayEvent = (eventType: string, data: unknown): boolean => {
  if (!isDesktopModeEnabled()) {
    return false;
  }
  const bridge = getDesktopOverlayBridge();
  if (!bridge) {
    return false;
  }
  const payload = (data && typeof data === 'object' ? data : {}) as Record<string, unknown>;
  const toNumber = (value: unknown): number | null => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
  };
  if (eventType === 'desktop_controller_hint' && typeof bridge.showControllerHint === 'function') {
    const x = toNumber(payload.x);
    const y = toNumber(payload.y);
    if (x === null || y === null) return true;
    const durationMs = toNumber(payload.duration_ms ?? payload.durationMs) ?? undefined;
    bridge.showControllerHint({
      x,
      y,
      description: typeof payload.description === 'string' ? payload.description : undefined,
      durationMs
    });
    return true;
  }
  if (eventType === 'desktop_controller_hint_done' && typeof bridge.showControllerDone === 'function') {
    const x = toNumber(payload.x);
    const y = toNumber(payload.y);
    if (x === null || y === null) return true;
    const durationMs = toNumber(payload.duration_ms ?? payload.durationMs) ?? undefined;
    bridge.showControllerDone({
      x,
      y,
      description: typeof payload.description === 'string' ? payload.description : undefined,
      durationMs
    });
    return true;
  }
  if (eventType === 'desktop_monitor_countdown' && typeof bridge.showMonitorCountdown === 'function') {
    const waitMs = toNumber(payload.wait_ms ?? payload.waitMs ?? 0) ?? 0;
    bridge.showMonitorCountdown({ waitMs });
    return true;
  }
  if (
    eventType === 'desktop_monitor_countdown_done' &&
    typeof bridge.hideOverlay === 'function'
  ) {
    bridge.hideOverlay();
    return true;
  }
  return false;
};
