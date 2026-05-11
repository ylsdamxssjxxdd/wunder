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
import { buildLegacyMessagesReconciledEvent } from '@/realtime/chat/chatRuntimeReplay';
import type { ChatRuntimeProjection } from '@/realtime/chat/chatRuntimeTypes';
import { dedupeAssistantMessages, dedupeAssistantMessagesInPlace } from './chatMessageDedup';
import {
  assistantEntriesShareTurnAnchor,
  buildAssistantMatchEntries,
  buildAssistantMatchEntryMap,
  findAnchoredAssistantContentMatchIndex
} from './chatAssistantMatch';
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
import { consumeChatWatchChannelMessage } from './chatWatchChannelMessageRuntime';
import { shouldWatchdogReconcileDrift } from './chatWatchdogRecovery';
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
import {
  hasRuntimeControllers as hasRuntimeControllersBase,
  resolveRuntimeDerivedStatus,
  shouldPreserveWatchRunningStatus
} from './chatRuntimeDerivedStatus';
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
import {
  mergeProtectedRealtimeMessages,
  upsertProtectedRealtimeMessage
} from './chatRealtimeMessageProtection';
import { useCommandSessionStore } from './commandSessions';
import { hasRetainedMessageConversationContext as hasRetainedConversationContext } from '@/views/messenger/messageConversationRetention';

import { clearSessionCommandSessions, ensureGreetingMessage, removeDemoChatSession, sortSessionsByActivity, syncDemoChatCache } from './chatDemoPanels';
import { DEFAULT_AGENT_KEY, applyMainSession, normalizeAgentKey, patchSessionRuntimeFields, persistAgentSession, persistDraftSession, resolvePersistedSessionId } from './chatPersist';
import { MESSAGE_WINDOW_LIMIT, MESSAGE_WINDOW_THRESHOLD, abortWatchStream, clearRuntimeInteractiveControllers, clearSessionWatcher, isWindowingEnabled, setSessionLoading } from './chatRuntimeControls';
import { clearChatSnapshot, findLiveAssistantInsertionIndex, findSnapshotAssistantIndexExcluding, mergeSnapshotAssistant, scheduleChatSnapshot } from './chatSnapshot';
import { buildMessage, clearAssistantRetryState, normalizeContextTokens, normalizeContextTotalTokens, normalizeMessageSubagents, parseOptionalCount, resolveTimestampIso, resolveTimestampMs } from './chatStats';
import { assignStreamEventId, normalizeFlag, normalizeStreamEventId, normalizeStreamRound } from './chatStreamIds';
import { SessionDetailSnapshotCacheEntry, SessionEventsSnapshotCacheEntry, ThreadControlSession } from './chatTypes';
import { abortResumeStream, abortSendStream } from './chatWatcher';
import { isTerminalRuntimeStatus, normalizeAssistantContent, normalizeStreamEventType, sessionWorkflowState } from './chatWorkflowHydration';

export const sessionRuntime = new Map();
export const sessionMessages = new Map();
export const sessionProtectedRealtimeMessages = new Map();
export const sessionListCache = new Map();
export const sessionListCacheInFlight = new Map();
export const sessionEventsSnapshotCache = new Map<string, SessionEventsSnapshotCacheEntry>();
export const sessionEventsSnapshotInFlight = new Map<string, Promise<Record<string, unknown> | null>>();
export const sessionDetailSnapshotCache = new Map<string, SessionDetailSnapshotCacheEntry>();
export const sessionHydratedMessageVersion = new Map<string, string>();
export const sessionDetailPrefetchInFlight = new Map();
export const sessionSubagentsInFlight = new Map();
export const sessionSubagentsCache = new Map<string, { cachedAt: number; items: unknown[] }>();
export const sessionDetailWarmState = new Map();
export const sessionHistoryState = new Map();

export const SESSION_LIST_CACHE_TTL_MS = 15 * 1000;
export const SESSION_EVENTS_CACHE_TTL_MS = 2500;
export const SESSION_EVENTS_RUNNING_CACHE_TTL_MS = 600;
export const SESSION_DETAIL_SNAPSHOT_TTL_MS = 2500;
export const SESSION_DETAIL_WARM_TTL_MS = 20 * 1000;
export const SESSION_SUBAGENTS_CACHE_TTL_MS = 12 * 1000;

export const resolveSessionKey = (sessionId) => String(sessionId || '').trim();

export const buildHistoryState = () => ({
  beforeId: null,
  hasMore: true,
  loading: false,
  windowLimit: MESSAGE_WINDOW_LIMIT
});

export const getHistoryState = (sessionId, options: { reset?: boolean } = {}) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return buildHistoryState();
  const reset = options.reset === true;
  let state = sessionHistoryState.get(key);
  if (!state || reset) {
    state = buildHistoryState();
    sessionHistoryState.set(key, state);
  }
  return state;
};

export const updateHistoryState = (sessionId, patch) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  const state = getHistoryState(key);
  Object.assign(state, patch);
  return state;
};

export const findOldestHistoryId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];
    const id = Number.parseInt(String(message?.history_id ?? ''), 10);
    if (Number.isFinite(id) && id > 0) {
      return id;
    }
  }
  return null;
};

export const applyMessageFeedbackByHistoryId = (messages, historyId, feedback) => {
  if (!Array.isArray(messages)) return false;
  const normalizedHistoryId = Number.parseInt(String(historyId ?? ''), 10);
  if (!Number.isFinite(normalizedHistoryId) || normalizedHistoryId <= 0) {
    return false;
  }
  const normalizedFeedback = normalizeMessageFeedback(feedback);
  if (!normalizedFeedback) return false;
  let updated = false;
  for (let i = 0; i < messages.length; i += 1) {
    const message = messages[i];
    if (!message || message.role !== 'assistant') continue;
    if (resolveMessageHistoryId(message) !== normalizedHistoryId) continue;
    const current = normalizeMessageFeedback(message.feedback);
    const shouldUpdate =
      !current ||
      current.vote !== normalizedFeedback.vote ||
      current.locked !== true ||
      String(current.created_at || '') !== String(normalizedFeedback.created_at || '');
    message.feedback = {
      ...normalizedFeedback,
      locked: true
    };
    if (shouldUpdate) {
      updated = true;
    }
  }
  return updated;
};

export const normalizeFeedbackMatchText = (value) =>
  normalizeAssistantContent(String(value || ''))
    .replace(/\s+/g, ' ')
    .trim();

export const isAssistantFeedbackCandidate = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) return false;
  if (resolveMessageHistoryId(message) > 0) return false;
  const text = normalizeFeedbackMatchText(message.content);
  return Boolean(text || message.created_at);
};

export const scoreAssistantHistoryMatch = (localMessage, remoteMessage) => {
  const localText = normalizeFeedbackMatchText(localMessage?.content);
  const remoteText = normalizeFeedbackMatchText(remoteMessage?.content);
  const localTime = resolveTimestampMs(localMessage?.created_at);
  const remoteTime = resolveTimestampMs(remoteMessage?.created_at);
  const hasTime = Number.isFinite(localTime) && Number.isFinite(remoteTime);
  const timeDelta = hasTime ? Math.abs(localTime - remoteTime) : Number.POSITIVE_INFINITY;

  let textScore = 0;
  if (localText && remoteText) {
    if (localText === remoteText) {
      textScore = 100000;
    } else if (localText.includes(remoteText) || remoteText.includes(localText)) {
      textScore = 80000;
    } else {
      return 0;
    }
  } else if (!localText && !remoteText) {
    if (!hasTime || timeDelta > 5000) {
      return 0;
    }
    textScore = 1000;
  } else {
    return 0;
  }

  let timeScore = 0;
  if (hasTime) {
    if (timeDelta <= 1000) {
      timeScore = 5000;
    } else if (timeDelta <= 10000) {
      timeScore = 4000;
    } else if (timeDelta <= 60000) {
      timeScore = 3000;
    } else if (timeDelta <= 180000) {
      timeScore = 1000;
    } else if (textScore < 100000) {
      return 0;
    }
  }
  return textScore + timeScore;
};

export const applyAssistantHistoryIdBackfill = (messages, historyMessages) => {
  if (!Array.isArray(messages) || !Array.isArray(historyMessages)) {
    return 0;
  }
  const localCandidates = [];
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (!isAssistantFeedbackCandidate(message)) continue;
    localCandidates.push(message);
  }
  if (!localCandidates.length) {
    return 0;
  }

  const remoteCandidates = [];
  for (let index = historyMessages.length - 1; index >= 0; index -= 1) {
    const message = historyMessages[index];
    if (!message || message.role !== 'assistant' || message.isGreeting) continue;
    const historyId = resolveMessageHistoryId(message);
    if (historyId <= 0) continue;
    remoteCandidates.push(message);
  }
  if (!remoteCandidates.length) {
    return 0;
  }

  const usedHistoryIds = new Set();
  let updated = 0;
  for (const localMessage of localCandidates) {
    let bestMatch = null;
    let bestScore = 0;
    for (const remoteMessage of remoteCandidates) {
      const historyId = resolveMessageHistoryId(remoteMessage);
      if (historyId <= 0 || usedHistoryIds.has(historyId)) continue;
      const score = scoreAssistantHistoryMatch(localMessage, remoteMessage);
      if (score > bestScore) {
        bestScore = score;
        bestMatch = remoteMessage;
      }
    }
    if (!bestMatch || bestScore <= 0) continue;
    const historyId = resolveMessageHistoryId(bestMatch);
    if (historyId <= 0) continue;
    usedHistoryIds.add(historyId);
    localMessage.history_id = historyId;
    const feedback = normalizeMessageFeedback(bestMatch.feedback);
    if (feedback) {
      localMessage.feedback = {
        ...feedback,
        locked: true
      };
    }
    updated += 1;
  }
  return updated;
};

export const applyMessageWindow = (store, sessionId, messages, options: { force?: boolean } = {}) => {
  if (!store || !isWindowingEnabled()) return;
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  const state = getHistoryState(key);
  const limit = Number(state.windowLimit) || MESSAGE_WINDOW_LIMIT;
  const threshold = Math.max(MESSAGE_WINDOW_THRESHOLD, limit);
  if (!options.force && messages.length <= threshold) return;
  if (messages.length <= limit) return;
  const overflow = messages.length - limit;
  if (overflow <= 0) return;
  messages.splice(0, overflow);
};

export const applyHistoryMeta = (sessionId, detail, messages) => {
  const beforeId = Number.parseInt(
    String(
      detail?.history_before_id ??
        detail?.history_beforeId ??
        detail?.historyBeforeId ??
        ''
    ),
    10
  );
  const hasMore =
    detail?.history_has_more ??
    detail?.historyHasMore ??
    detail?.history_more ??
    detail?.historyMore ??
    null;
  const resolvedBeforeId =
    Number.isFinite(beforeId) && beforeId > 0 ? beforeId : findOldestHistoryId(messages);
  updateHistoryState(sessionId, {
    beforeId: resolvedBeforeId,
    hasMore: hasMore === null ? Boolean(resolvedBeforeId) : Boolean(hasMore)
  });
};

export const cloneSerializable = (value, fallback) => {
  if (typeof structuredClone === 'function') {
    try {
      return structuredClone(value);
    } catch (error) {
      // Fallback to JSON clone when structuredClone fails.
    }
  }
  try {
    return JSON.parse(JSON.stringify(value));
  } catch (error) {
    return fallback;
  }
};

export const cloneSessionList = (sessions) => {
  const cloned = cloneSerializable(Array.isArray(sessions) ? sessions : [], []);
  return Array.isArray(cloned) ? cloned : [];
};

export const cloneSessionEventsPayload = (payload) => {
  const cloned = cloneSerializable(payload, null);
  return cloned && typeof cloned === 'object' && !Array.isArray(cloned) ? cloned : null;
};

export const cloneSessionDetailPayload = (payload) => {
  const cloned = cloneSerializable(payload, null);
  return cloned && typeof cloned === 'object' && !Array.isArray(cloned) ? cloned : null;
};

export const appendFingerprintHash = (seed, value) => {
  let hash = seed >>> 0;
  const text = String(value ?? '');
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
};

export const buildSessionMessageFingerprint = (messages) => {
  if (!Array.isArray(messages) || messages.length === 0) {
    return '0';
  }
  let hash = 2166136261;
  messages.forEach((message, index) => {
    const record = message && typeof message === 'object' ? message : {};
    const attachments = Array.isArray(record.attachments) ? record.attachments.length : 0;
    const subagents = Array.isArray(record.subagents) ? record.subagents.length : 0;
    const planSteps = Array.isArray(record?.plan?.steps)
      ? record.plan.steps.length
      : Array.isArray(record?.plan)
        ? record.plan.length
        : 0;
    const questionPanelSelected = Array.isArray(record?.questionPanel?.selected)
      ? record.questionPanel.selected.length
      : 0;
    hash = appendFingerprintHash(
      hash,
      [
        index,
        String(record.role || ''),
        String(record.created_at || ''),
        String(record.history_id ?? ''),
        String(record.stream_event_id ?? ''),
        String(record.stream_round ?? ''),
        String(record.content || '').length,
        String(record.reasoning || '').length,
        attachments,
        subagents,
        String(record?.feedback?.vote ?? ''),
        record?.feedback?.locked === true ? 1 : 0,
        planSteps,
        String(record?.questionPanel?.status ?? ''),
        questionPanelSelected,
        record?.hiddenInternal === true ? 1 : 0
      ].join('|')
    );
  });
  return hash.toString(36);
};

export const buildWorkflowRoundsFingerprint = (rounds) => {
  if (!Array.isArray(rounds) || rounds.length === 0) {
    return '0';
  }
  let hash = 2166136261;
  rounds.forEach((round, index) => {
    const events = Array.isArray(round?.events) ? round.events : [];
    const lastEvent = events.length > 0 ? events[events.length - 1] : null;
    hash = appendFingerprintHash(
      hash,
      [
        index,
        String(round?.user_round ?? round?.round ?? ''),
        events.length,
        String(lastEvent?.event ?? lastEvent?.type ?? ''),
        String(lastEvent?.timestamp ?? '')
      ].join('|')
    );
  });
  return hash.toString(36);
};

export const buildSessionHydratedMessageVersion = (sessionDetail, eventsPayload) => {
  const messages = Array.isArray(sessionDetail?.messages) ? sessionDetail.messages : [];
  const rounds = Array.isArray(eventsPayload?.rounds) ? eventsPayload.rounds : [];
  const remoteLastEventId =
    normalizeStreamEventId(eventsPayload?.last_event_id ?? eventsPayload?.lastEventId) || 0;
  const running = eventsPayload?.running === true ? 1 : 0;
  return [
    buildSessionMessageFingerprint(messages),
    buildWorkflowRoundsFingerprint(rounds),
    remoteLastEventId,
    running
  ].join(':');
};

export const readSessionHydratedMessageVersion = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return '';
  return String(sessionHydratedMessageVersion.get(sessionKey) || '');
};

export const writeSessionHydratedMessageVersion = (sessionId, version) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return '';
  const nextVersion = String(version || '').trim();
  if (!nextVersion) {
    sessionHydratedMessageVersion.delete(sessionKey);
    return '';
  }
  sessionHydratedMessageVersion.set(sessionKey, nextVersion);
  return nextVersion;
};

export const clearSessionEventsSnapshot = (sessionId, options: { keepInFlight?: boolean } = {}) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return;
  sessionEventsSnapshotCache.delete(sessionKey);
  sessionDetailSnapshotCache.delete(sessionKey);
  sessionHydratedMessageVersion.delete(sessionKey);
  if (options.keepInFlight !== true) {
    sessionEventsSnapshotInFlight.delete(sessionKey);
  }
};

export const cacheSessionDetailSnapshot = (sessionId, payload) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return null;
  const clonedPayload = cloneSessionDetailPayload(payload);
  sessionDetailSnapshotCache.set(sessionKey, {
    cachedAt: Date.now(),
    payload: clonedPayload
  });
  return cloneSessionDetailPayload(clonedPayload);
};

export const readSessionDetailSnapshot = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return null;
  const entry = sessionDetailSnapshotCache.get(sessionKey);
  if (!entry) return null;
  if (!Number.isFinite(entry.cachedAt) || Date.now() - entry.cachedAt > SESSION_DETAIL_SNAPSHOT_TTL_MS) {
    sessionDetailSnapshotCache.delete(sessionKey);
    return null;
  }
  return cloneSessionDetailPayload(entry.payload);
};

export const cacheSessionEventsSnapshot = (sessionId, payload) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return null;
  const clonedPayload = cloneSessionEventsPayload(payload);
  sessionEventsSnapshotCache.set(sessionKey, {
    cachedAt: Date.now(),
    running: clonedPayload?.running === true,
    lastEventId: normalizeStreamEventId(
      clonedPayload?.last_event_id ?? clonedPayload?.lastEventId
    ),
    payload: clonedPayload
  });
  return cloneSessionEventsPayload(clonedPayload);
};

export const readSessionEventsSnapshot = (
  sessionId,
  options: { allowRunning?: boolean; minLastEventId?: unknown } = {}
) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return null;
  const entry = sessionEventsSnapshotCache.get(sessionKey);
  if (!entry) return null;
  const ttlMs = entry.running ? SESSION_EVENTS_RUNNING_CACHE_TTL_MS : SESSION_EVENTS_CACHE_TTL_MS;
  if (!Number.isFinite(entry.cachedAt) || Date.now() - entry.cachedAt > ttlMs) {
    sessionEventsSnapshotCache.delete(sessionKey);
    return null;
  }
  if (entry.running && options.allowRunning !== true) {
    return null;
  }
  const runtime = sessionRuntime.get(sessionKey) || null;
  if (runtime?.sendController || runtime?.resumeController) {
    return null;
  }
  const minLastEventId = normalizeStreamEventId(options.minLastEventId);
  if (minLastEventId !== null) {
    const cachedLastEventId = normalizeStreamEventId(entry.lastEventId);
    if (cachedLastEventId === null || cachedLastEventId < minLastEventId) {
      return null;
    }
  }
  return cloneSessionEventsPayload(entry.payload);
};

export const loadSessionEventsSnapshot = (
  sessionId,
  options: {
    allowCached?: boolean;
    allowRunningCache?: boolean;
    dedupeInFlight?: boolean;
    minLastEventId?: unknown;
  } = {}
) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) {
    return Promise.resolve(null);
  }
  if (options.allowCached !== false) {
    const cached = readSessionEventsSnapshot(sessionKey, {
      allowRunning: options.allowRunningCache === true,
      minLastEventId: options.minLastEventId
    });
    if (cached) {
      return Promise.resolve(cached);
    }
  }
  const inFlight = sessionEventsSnapshotInFlight.get(sessionKey);
  if (inFlight && options.dedupeInFlight !== false) {
    return inFlight;
  }
  const request = getSessionEvents(sessionKey).then((response) =>
    cacheSessionEventsSnapshot(sessionKey, response?.data?.data || null)
  );
  sessionEventsSnapshotInFlight.set(sessionKey, request);
  return request.finally(() => {
    if (sessionEventsSnapshotInFlight.get(sessionKey) === request) {
      sessionEventsSnapshotInFlight.delete(sessionKey);
    }
  });
};

export const resolveTerminableSubagentSessionIds = (items: unknown[]): string[] =>
  normalizeMessageSubagents(items)
    .filter((item) => item.canTerminate && !item.terminal)
    .map((item) => String(item.session_id || '').trim())
    .filter(Boolean);

export const readProtectedRealtimeMessages = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return [];
  const entries = sessionProtectedRealtimeMessages.get(sessionKey);
  return Array.isArray(entries) ? entries.slice() : [];
};

export const writeProtectedRealtimeMessages = (sessionId, entries) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return [];
  const nextEntries = Array.isArray(entries) ? entries.slice() : [];
  if (!nextEntries.length) {
    sessionProtectedRealtimeMessages.delete(sessionKey);
    return [];
  }
  sessionProtectedRealtimeMessages.set(sessionKey, nextEntries);
  return nextEntries;
};

export const trackSessionProtectedRealtimeMessage = (
  sessionId,
  entry: {
    eventId?: unknown;
    role?: unknown;
    content?: unknown;
    createdAt?: unknown;
    hiddenInternal?: unknown;
  }
) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return [];
  const nextEntries = upsertProtectedRealtimeMessage(
    readProtectedRealtimeMessages(sessionKey),
    entry,
    normalizeStreamEventId
  );
  return writeProtectedRealtimeMessages(sessionKey, nextEntries);
};

export const mergeSessionProtectedRealtimeMessages = (sessionId, messages) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey || !Array.isArray(messages)) {
    return messages;
  }
  const entries = readProtectedRealtimeMessages(sessionKey);
  if (!entries.length) {
    return messages;
  }
  const result = mergeProtectedRealtimeMessages({
    messages,
    entries,
    normalizeEventId: normalizeStreamEventId,
    buildMessage,
    assignStreamEventId
  });
  writeProtectedRealtimeMessages(sessionKey, result.retainedEntries);
  return messages;
};

export const protectRealtimeChannelMessage = (
  sessionId,
  messages,
  eventId,
  role,
  content,
  eventTimestampMs,
  hiddenInternal = false
) => {
  const sessionKey = resolveSessionKey(sessionId);
  const normalizedEventId = normalizeStreamEventId(eventId);
  const normalizedRole = String(role || '').trim().toLowerCase();
  const normalizedContent = String(content || '').trim();
  if (
    !sessionKey ||
    !Array.isArray(messages) ||
    normalizedEventId === null ||
    (normalizedRole !== 'user' && normalizedRole !== 'assistant') ||
    !normalizedContent
  ) {
    return;
  }
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message?.role !== normalizedRole) continue;
    if (normalizeStreamEventId(message?.stream_event_id) !== normalizedEventId) continue;
    message.realtime_protected = true;
    break;
  }
  trackSessionProtectedRealtimeMessage(sessionKey, {
    eventId: normalizedEventId,
    role: normalizedRole,
    content: normalizedContent,
    createdAt: Number.isFinite(eventTimestampMs) ? new Date(eventTimestampMs).toISOString() : undefined,
    hiddenInternal: normalizedRole === 'user' && hiddenInternal === true
  });
};

export const filterSessionsByAgent = (agentId, sourceSessions = []) => {
  const normalizedAgentIdRaw = String(agentId || '').trim();
  const normalizedAgentId =
    normalizedAgentIdRaw === DEFAULT_AGENT_KEY ? '' : normalizedAgentIdRaw;
  return (Array.isArray(sourceSessions) ? sourceSessions : []).filter((session) => {
    const sessionAgentId = String(session?.agent_id || '').trim();
    return normalizedAgentId ? sessionAgentId === normalizedAgentId : !sessionAgentId;
  });
};

export const resolveInitialSessionIdFromList = (agentId, sourceSessions = []) => {
  const sessions = filterSessionsByAgent(agentId, sourceSessions);
  if (!sessions.length) return '';
  const mainSession = sessions.find((session) => session.is_main);
  if (mainSession?.id) {
    return mainSession.id;
  }
  const persistedSessionId = resolvePersistedSessionId(agentId);
  if (persistedSessionId && sessions.some((session) => session.id === persistedSessionId)) {
    return persistedSessionId;
  }
  return sessions[0]?.id || '';
};

export const resolveSessionListCacheKey = (agentId) => normalizeAgentKey(agentId);

export const readSessionListCache = (agentId, options: { maxAgeMs?: number } = {}) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  const cached = sessionListCache.get(cacheKey);
  if (!cached) return null;
  const requestedMaxAgeMs = Number(options?.maxAgeMs);
  const maxAgeMs = Number.isFinite(requestedMaxAgeMs)
    ? Math.max(0, requestedMaxAgeMs)
    : SESSION_LIST_CACHE_TTL_MS;
  if (!Number.isFinite(cached.cachedAt) || Date.now() - cached.cachedAt > maxAgeMs) {
    sessionListCache.delete(cacheKey);
    return null;
  }
  return cloneSessionList(cached.sessions);
};

export const readSessionListCacheEntry = (agentId, options: { maxAgeMs?: number } = {}) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  const cached = sessionListCache.get(cacheKey);
  if (!cached) return null;
  const requestedMaxAgeMs = Number(options?.maxAgeMs);
  const maxAgeMs = Number.isFinite(requestedMaxAgeMs)
    ? Math.max(0, requestedMaxAgeMs)
    : SESSION_LIST_CACHE_TTL_MS;
  if (!Number.isFinite(cached.cachedAt) || Date.now() - cached.cachedAt > maxAgeMs) {
    sessionListCache.delete(cacheKey);
    return null;
  }
  return {
    cachedAt: cached.cachedAt,
    sessions: cloneSessionList(cached.sessions)
  };
};

export const writeSessionListCache = (agentId, sessions) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  sessionListCache.set(cacheKey, {
    cachedAt: Date.now(),
    sessions: cloneSessionList(sessions)
  });
};

export const normalizeThreadControlSession = (value) => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const session = value as Record<string, unknown>;
  const id = resolveSessionKey(session.id ?? session.session_id ?? session.sessionId);
  if (!id) return null;
  return {
    ...session,
    id
  } as ThreadControlSession;
};

export const applyThreadControlSessionPatch = (store, session, options: { allowArchived?: boolean } = {}) => {
  const normalized = normalizeThreadControlSession(session);
  if (!normalized) return null;
  const targetId = resolveSessionKey(normalized.id);
  if (!targetId) return null;
  const status = String(normalized.status || '').trim().toLowerCase();
  const allowArchived = options.allowArchived === true;
  const targetAgentId = String(normalized.agent_id || '').trim();
  if (status === 'archived' && !allowArchived) {
    store.sessions = (Array.isArray(store.sessions) ? store.sessions : []).filter(
      (item) => resolveSessionKey(item?.id) !== targetId
    );
    if (resolvePersistedSessionId(targetAgentId) === targetId) {
      persistAgentSession(targetAgentId, '');
    }
    return { ...normalized, id: targetId };
  }
  const index = (Array.isArray(store.sessions) ? store.sessions : []).findIndex(
    (item) => resolveSessionKey(item?.id) === targetId
  );
  if (index >= 0) {
    const current = store.sessions[index] || {};
    store.sessions[index] = patchSessionRuntimeFields({
      ...current,
      ...normalized,
      id: targetId
    });
    return store.sessions[index];
  }
  const merged = patchSessionRuntimeFields({ ...normalized, id: targetId });
  store.sessions.unshift(merged);
  return merged;
};

export const applyThreadControlCaches = (store, agentIds: Set<string>) => {
  store.sessions = sortSessionsByActivity(store.sessions);
  agentIds.forEach((agentId) => {
    writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
  });
  syncDemoChatCache({ sessions: store.sessions });
};

export const handleThreadControlWorkflowEvent = async (store, payloadRaw) => {
  const payload =
    payloadRaw && typeof payloadRaw === 'object' && !Array.isArray(payloadRaw)
      ? (payloadRaw as Record<string, unknown>)
      : {};
  const primarySession = normalizeThreadControlSession(payload.session);
  const mainSession = normalizeThreadControlSession(payload.main_session ?? payload.mainSession);
  const switchSession = normalizeThreadControlSession(
    payload.switch_session ?? payload.switchSession ?? payload.session
  );
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  const retainIds = new Set(
    [activeSessionId, resolveSessionKey(mainSession?.id), resolveSessionKey(switchSession?.id)].filter(Boolean)
  );
  const affectedAgentIds = new Set<string>();
  const applyPatch = (session, options: { allowArchived?: boolean } = {}) => {
    const patched = applyThreadControlSessionPatch(store, session, options);
    if (!patched) return null;
    affectedAgentIds.add(String(patched.agent_id || '').trim());
    return patched;
  };

  const patchedPrimary = applyPatch(primarySession, {
    allowArchived: retainIds.has(resolveSessionKey(primarySession?.id))
  });
  const patchedMain = applyPatch(mainSession, { allowArchived: true });
  const patchedSwitch = applyPatch(switchSession, { allowArchived: true });

  if (patchedMain?.id) {
    const mainAgentId = String(patchedMain.agent_id || '').trim();
    store.sessions = applyMainSession(store.sessions, mainAgentId, patchedMain.id);
    persistAgentSession(mainAgentId, patchedMain.id);
    affectedAgentIds.add(mainAgentId);
  }

  if (!patchedMain?.id && patchedPrimary?.status === 'archived') {
    const archivedAgentId = String(patchedPrimary.agent_id || '').trim();
    if (resolvePersistedSessionId(archivedAgentId) === patchedPrimary.id) {
      persistAgentSession(archivedAgentId, '');
    }
  }

  applyThreadControlCaches(store, affectedAgentIds);

  const shouldSwitch = payload.switch === true;
  const targetSwitchId = resolveSessionKey(
    patchedSwitch?.id ?? payload.switch_session_id ?? payload.switchSessionId ?? ''
  );
  if (shouldSwitch && targetSwitchId && targetSwitchId !== activeSessionId) {
    await store.loadSessionDetail(targetSwitchId);
  }
};

export const resolveChatHttpStatus = (error) => {
  const status = Number(error?.response?.status ?? error?.status ?? 0);
  return Number.isFinite(status) ? status : 0;
};

export const isSessionUnavailableStatus = (status) => [401, 403, 404].includes(Number(status || 0));

export const hasKnownSessionInStore = (store, sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return false;
  const sessions = Array.isArray(store?.sessions) ? store.sessions : [];
  if (!sessions.length) return true;
  return sessions.some((item) => resolveSessionKey(item?.id) === targetId);
};

export const purgeUnavailableSession = (store, sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return '';
  const sessions = Array.isArray(store?.sessions) ? store.sessions : [];
  const targetSession = sessions.find((item) => resolveSessionKey(item?.id) === targetId) || null;
  const targetAgentId = String(targetSession?.agent_id || '').trim();
  abortResumeStream(targetId);
  abortSendStream(targetId);
  abortWatchStream(targetId);
  if (resolveSessionKey(store?.activeSessionId) === targetId) {
    clearSessionWatcher();
  }
  setSessionLoading(store, targetId, false);
  if (typeof store?.clearPendingApprovals === 'function') {
    store.clearPendingApprovals({ sessionId: targetId });
  }
  sessionRuntime.delete(targetId);
  sessionMessages.delete(targetId);
  sessionProtectedRealtimeMessages.delete(targetId);
  clearSessionEventsSnapshot(targetId);
  sessionDetailWarmState.delete(targetId);
  sessionDetailPrefetchInFlight.delete(targetId);
  sessionSubagentsInFlight.delete(targetId);
  sessionSubagentsCache.delete(targetId);
  sessionHistoryState.delete(targetId);
  sessionWorkflowState.delete(targetId);
  clearSessionCommandSessions(targetId);
  removeDemoChatSession(targetId);
  clearChatSnapshot(targetId);

  const nextSessions = sessions.filter((item) => resolveSessionKey(item?.id) !== targetId);
  if (Array.isArray(store?.sessions)) {
    store.sessions = nextSessions;
  }
  if (resolvePersistedSessionId(targetAgentId) === targetId) {
    persistAgentSession(targetAgentId, '');
  }
  writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, nextSessions));

  if (resolveSessionKey(store?.activeSessionId) === targetId) {
    store.activeSessionId = null;
    store.draftAgentId = targetAgentId;
    store.draftToolOverrides = null;
    store.messages = ensureGreetingMessage([], {
      greeting: store?.greetingOverride
    });
    persistDraftSession();
  }
  syncDemoChatCache({
    sessions: Array.isArray(store?.sessions) ? store.sessions : nextSessions,
    sessionId: store?.activeSessionId || null,
    messages: Array.isArray(store?.messages) ? store.messages : []
  });
  return targetAgentId;
};

export const markSessionDetailWarm = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return;
  sessionDetailWarmState.set(sessionKey, Date.now() + SESSION_DETAIL_WARM_TTL_MS);
};

export const isSessionDetailWarm = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return false;
  const warmUntil = Number(sessionDetailWarmState.get(sessionKey));
  if (!Number.isFinite(warmUntil)) {
    sessionDetailWarmState.delete(sessionKey);
    return false;
  }
  if (warmUntil <= Date.now()) {
    sessionDetailWarmState.delete(sessionKey);
    return false;
  }
  return true;
};

export const ensureRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  if (!sessionRuntime.has(key)) {
    sessionRuntime.set(key, {
      sendController: null,
      compactController: null,
      resumeController: null,
      sendRequestId: null,
      resumeRequestId: null,
      sendStartedAt: 0,
      sendLastEventAt: 0,
      resumeStartedAt: 0,
      resumeLastEventAt: 0,
      watchController: null,
      watchActiveRoundCount: 0,
      watchRequestId: null,
      watchLastEventAt: 0,
      watchdogTimer: null,
      watchdogBusy: false,
      watchReconcileTimer: null,
      watchReconcileAt: 0,
      slowClientResumeTimer: null,
      slowClientResumeAfterEventId: 0,
      streamLifecycle: 'idle',
      stopRequested: false,
      pendingManualCompaction: null,
      lastEventId: 0,
      remoteLastEventId: 0,
      threadStatus: 'not_loaded',
      loaded: false,
      activeTurnId: '',
      pendingApprovalIds: [],
      pendingApprovalCount: 0,
      waitingForUserInput: false,
      lastThreadStatusAt: 0
    });
  }
  return sessionRuntime.get(key);
};

export const getRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionRuntime.get(key) || null;
};

export const refreshRuntimeStreamLifecycle = (runtime) => {
  if (!runtime) return 'idle';
  if (runtime.sendController) {
    runtime.streamLifecycle = 'sending';
    return runtime.streamLifecycle;
  }
  if (runtime.resumeController) {
    runtime.streamLifecycle = 'resuming';
    return runtime.streamLifecycle;
  }
  if (runtime.watchController) {
    runtime.streamLifecycle = 'watching';
    return runtime.streamLifecycle;
  }
  runtime.streamLifecycle = 'idle';
  return runtime.streamLifecycle;
};

export const getRuntimeStreamLifecycle = (runtime) =>
  normalizeStreamLifecyclePhase(runtime?.streamLifecycle);

export function resolveRuntimeSessionId(sessionId, payload) {
  const direct = resolveSessionKey(sessionId ?? payload?.session_id ?? payload?.sessionId);
  if (direct) return direct;
  const threadId = String(payload?.thread_id ?? payload?.threadId ?? '').trim();
  if (!threadId.startsWith('thread_')) return null;
  return resolveSessionKey(threadId.slice('thread_'.length));
}

export function normalizeRuntimeApprovalIds(value) {
  if (!Array.isArray(value)) return [];
  return Array.from(
    new Set(
      value
        .map((item) => String(item || '').trim())
        .filter(Boolean)
    )
  );
}

export function resolveRuntimeLoading(store, sessionId, runtime) {
  const key = resolveSessionKey(sessionId);
  if (!key) return false;
  if (Boolean(store?.loadingBySession?.[key])) {
    return true;
  }
  return hasRuntimeControllers(runtime);
}

export function hasRuntimeControllers(runtime) {
  return hasRuntimeControllersBase(runtime);
}

export function applyRuntimeDerivedStatus(store, sessionId, runtime) {
  if (!runtime) return 'not_loaded';
  const loading = resolveRuntimeLoading(store, sessionId, runtime);
  const nextStatus = resolveRuntimeDerivedStatus({ runtime, loading });
  if (nextStatus === 'waiting_user_input') {
    runtime.threadStatus = nextStatus;
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  if (nextStatus === 'waiting_approval') {
    runtime.threadStatus = nextStatus;
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  if (nextStatus === 'running') {
    if (shouldPreserveWatchRunningStatus(runtime, loading)) {
      runtime.loaded = true;
      chatDebugLog('chat.store.loading', 'preserve-watch-running', {
        sessionId: resolveSessionKey(sessionId),
        runtime: buildRuntimeDebugSnapshot(runtime)
      });
      return nextStatus;
    }
    runtime.threadStatus = nextStatus;
    runtime.loaded = true;
    return runtime.threadStatus;
  }
  runtime.threadStatus = nextStatus;
  return runtime.threadStatus;
}

export function applySessionRuntimeSnapshot(runtime, snapshot) {
  if (!runtime || !snapshot || typeof snapshot !== 'object' || Array.isArray(snapshot)) {
    return false;
  }
  const source = snapshot as Record<string, unknown>;
  const turn =
    source.turn && typeof source.turn === 'object' && !Array.isArray(source.turn)
      ? (source.turn as Record<string, unknown>)
      : {};
  const pendingApprovalIds = normalizeRuntimeApprovalIds(
    turn.pending_approval_ids ?? turn.pendingApprovalIds
  );
  const waitingForUserInput = normalizeFlag(
    turn.waiting_for_user_input ?? turn.waitingForUserInput
  );
  const explicitApprovalCount = Number.parseInt(
    String(turn.pending_approval_count ?? turn.pendingApprovalCount ?? ''),
    10
  );
  runtime.loaded =
    source.loaded === undefined
      ? runtime.loaded || normalizeThreadRuntimeStatus(source.thread_status ?? source.status) !== 'not_loaded'
      : normalizeFlag(source.loaded);
  runtime.activeTurnId = String(
    source.active_turn_id ?? source.activeTurnId ?? turn.turn_id ?? turn.turnId ?? ''
  ).trim();
  runtime.pendingApprovalIds = pendingApprovalIds;
  runtime.pendingApprovalCount =
    Number.isFinite(explicitApprovalCount) && explicitApprovalCount >= 0
      ? explicitApprovalCount
      : pendingApprovalIds.length;
  runtime.waitingForUserInput = waitingForUserInput;
  runtime.threadStatus = normalizeThreadRuntimeStatus(source.thread_status ?? source.status);
  runtime.lastThreadStatusAt = Date.now();
  if (runtime.waitingForUserInput) {
    runtime.threadStatus = 'waiting_user_input';
    runtime.loaded = true;
  } else if (runtime.pendingApprovalCount > 0) {
    runtime.threadStatus = 'waiting_approval';
    runtime.loaded = true;
  } else if (runtime.threadStatus === 'not_loaded') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
  }
  return true;
}

export function applySessionRuntimeEvent(store, sessionId, payload, eventType = 'thread_status') {
  const targetId = resolveRuntimeSessionId(sessionId, payload);
  if (!targetId) return null;
  const runtime = ensureRuntime(targetId);
  if (!runtime) return null;
  const applied = applySessionRuntimeSnapshot(runtime, payload);
  if (!applied && eventType === 'thread_closed') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
    runtime.pendingApprovalIds = [];
    runtime.pendingApprovalCount = 0;
    runtime.waitingForUserInput = false;
    runtime.threadStatus = 'not_loaded';
    runtime.lastThreadStatusAt = Date.now();
  } else if (applied && eventType === 'thread_closed') {
    runtime.loaded = false;
    runtime.activeTurnId = '';
    runtime.pendingApprovalIds = [];
    runtime.pendingApprovalCount = 0;
    runtime.waitingForUserInput = false;
    runtime.threadStatus = 'not_loaded';
  }
  syncChatRuntimeProjectionStatus(store, targetId, runtime.threadStatus, {
    eventType: eventType === 'thread_closed' ? 'session_idle' : 'session_runtime'
  });
  const terminalRuntimeStatus = isTerminalRuntimeStatus(runtime.threadStatus);
  if (terminalRuntimeStatus) {
    // Terminal runtime status is authoritative for this thread; clear stale interactive
    // controllers so they cannot keep the session in a phantom "running" state.
    clearRuntimeInteractiveControllers(runtime, { abort: false });
    chatDebugLog('chat.store.runtime', 'terminal-runtime-event', {
      sessionId: targetId,
      eventType,
      threadStatus: runtime.threadStatus,
      runtime: buildRuntimeDebugSnapshot(runtime)
    });
  }
  const shouldSettleTerminalState =
    !hasRuntimeControllers(runtime)
    && terminalRuntimeStatus;
  if (shouldSettleTerminalState) {
    const targetMessages = resolveSessionKey(store?.activeSessionId) === targetId
      ? store?.messages
      : getSessionMessages(targetId);
    const clearedSuperseded = clearSupersededPendingAssistantMessages(targetMessages);
    const clearedTrailing = clearTrailingPendingAssistantMessages(targetMessages) > 0;
    if (clearedSuperseded || clearedTrailing) {
      notifySessionSnapshot(store, targetId, targetMessages, true);
    }
    setSessionLoading(store, targetId, false);
    chatDebugLog('chat.store.runtime', 'settle-terminal-state', {
      sessionId: targetId,
      eventType,
      threadStatus: runtime.threadStatus,
      clearedSuperseded,
      clearedTrailing
    });
  }
  return runtime;
}

export function syncSessionPendingApprovalRuntime(store, sessionId) {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  const runtime = ensureRuntime(key);
  if (!runtime) return null;
  const approvals = Array.isArray(store?.pendingApprovals)
    ? store.pendingApprovals.filter((item) => resolveSessionKey(item?.session_id) === key)
    : [];
  runtime.pendingApprovalIds = approvals
    .map((item) => String(item?.approval_id || '').trim())
    .filter(Boolean);
  runtime.pendingApprovalCount = runtime.pendingApprovalIds.length;
  applyRuntimeDerivedStatus(store, key, runtime);
  return runtime;
}

export const getSessionMessages = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionMessages.get(key) || null;
};

export const resolveSessionMessageArray = (store, sessionId, fallbackMessages = null) => {
  const key = resolveSessionKey(sessionId);
  if (!key) {
    return Array.isArray(fallbackMessages) ? fallbackMessages : [];
  }
  return resolveRealtimeMessageArrayReference({
    sessionId: key,
    activeSessionId: resolveSessionKey(store?.activeSessionId),
    activeMessages: store?.messages,
    cachedMessages: getSessionMessages(key),
    fallbackMessages
  });
};

export const cacheSessionMessages = (sessionId, messages) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  dedupeAssistantMessagesInPlace(messages);
  sessionMessages.set(key, messages);
};

export const hasSubmittedUserMessage = (messages) =>
  (Array.isArray(messages) ? messages : []).some((message) => {
    if (!message || message.isGreeting || String(message.role || '').trim() !== 'user') {
      return false;
    }
    const hasText = Boolean(String(message.content || '').trim());
    const hasAttachments = Array.isArray(message.attachments) && message.attachments.length > 0;
    return hasText || hasAttachments;
  });

export const isSessionSpawnedFromAnotherThread = (session) => {
  if (!session || typeof session !== 'object') return false;
  const source = session as Record<string, unknown>;
  return Boolean(
    String(source.parent_session_id ?? '').trim()
    || String(source.parent_message_id ?? '').trim()
    || String(source.spawned_by ?? '').trim()
    || String(source.spawn_label ?? '').trim()
  );
};

export const isReusableFreshSession = (session, fallbackMessages = null) => {
  if (!session || typeof session !== 'object') return false;
  const sessionId = resolveSessionKey(session.id);
  if (!sessionId) return false;
  const status = String(session.status || '').trim().toLowerCase();
  if (status === 'archived') return false;
  // "New thread" can only reuse root sessions. Spawned/subagent threads must not be recycled.
  if (isSessionSpawnedFromAnotherThread(session)) return false;
  const cachedMessages = getSessionMessages(sessionId);
  const messages = Array.isArray(cachedMessages) && cachedMessages.length ? cachedMessages : fallbackMessages;
  if (hasSubmittedUserMessage(messages)) {
    return false;
  }
  const createdAt = resolveTimestampMs(session.created_at);
  const lastMessageAt = resolveTimestampMs(session.last_message_at);
  if (createdAt !== null && lastMessageAt !== null && lastMessageAt > createdAt + 1000) {
    return false;
  }
  return true;
};

export const touchSessionUpdatedAt = (store, sessionId, timestamp) => {
  if (!store || !Array.isArray(store.sessions)) return;
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const session = store.sessions.find((item) => String(item?.id || '').trim() === key);
  if (!session) return;
  const resolved = resolveTimestampIso(timestamp);
  session.updated_at = resolved || new Date().toISOString();
};

export const resolveSessionContextTokens = (store, sessionId) => {
  if (!store || !Array.isArray(store.sessions)) return null;
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  const session = store.sessions.find((item) => resolveSessionKey(item?.id) === key);
  if (!session || typeof session !== 'object') return null;
  return normalizeContextTokens(
    session.context_occupancy_tokens ??
      session.contextOccupancyTokens ??
      session.context_usage?.context_occupancy_tokens ??
      session.context_usage?.contextOccupancyTokens ??
      session.contextTokens ??
      session.context_tokens ??
      session.context_usage?.contextTokens ??
      session.context_usage?.context_tokens
  );
};

export const syncSessionContextTokens = (store, sessionId, contextTokens, contextTotalTokens = null) => {
  if (!store || !Array.isArray(store.sessions)) return;
  const key = resolveSessionKey(sessionId);
  const normalized = parseOptionalCount(contextTokens);
  const normalizedTotal = normalizeContextTotalTokens(contextTotalTokens);
  if (!key || normalized === null) return;
  const index = store.sessions.findIndex((item) => resolveSessionKey(item?.id) === key);
  if (index < 0) return;
  const current = store.sessions[index] || {};
  const next = {
    ...current,
    context_tokens: normalized,
    context_occupancy_tokens: normalized,
    contextTokens: normalized,
    contextOccupancyTokens: normalized,
    ...(normalizedTotal !== null
      ? {
          context_max_tokens: normalizedTotal,
          context_total_tokens: normalizedTotal,
          contextTotalTokens: normalizedTotal
        }
      : {})
  };
  store.sessions[index] = next;
  const agentId = String(next.agent_id || '').trim();
  writeSessionListCache(agentId, filterSessionsByAgent(agentId, store.sessions));
  syncDemoChatCache({ sessions: store.sessions });
};

export const notifySessionSnapshot = (store, sessionId, messages, immediate = false, options: { skipWindowing?: boolean } = {}) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  dedupeTerminalCompactionMarkersInPlace(messages);
  cacheSessionMessages(key, messages);
  syncChatRuntimeProjectionFromLegacy(store, key, messages);
  const activeKey = resolveSessionKey(store?.activeSessionId);
  if (activeKey && activeKey === key) {
    if (store && typeof store === 'object') {
      store.messageMutationVersion = Number(store.messageMutationVersion || 0) + 1;
    }
    if (options.skipWindowing !== true) {
      applyMessageWindow(store, key, messages);
    }
    scheduleChatSnapshot(store, immediate);
  }
};

export const ensureChatRuntimeProjectionForStore = (store): ChatRuntimeProjection | null => {
  if (!store || typeof store !== 'object') return null;
  if (!store.runtimeProjection) {
    store.runtimeProjection = createChatRuntimeProjection();
  }
  return store.runtimeProjection as ChatRuntimeProjection;
};

export const resolveProjectionAgentId = (store, sessionId): string => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(store?.sessions)) return '';
  const session = store.sessions.find((item) => resolveSessionKey(item?.id) === key);
  return String(session?.agent_id || '').trim();
};

export const syncChatRuntimeProjectionFromLegacy = (
  store,
  sessionId,
  messages = null,
  options: { loading?: boolean; running?: boolean } = {}
) => {
  const key = resolveSessionKey(sessionId);
  const projection = ensureChatRuntimeProjectionForStore(store);
  if (!key || !projection) return;
  projection.activeSessionId = resolveSessionKey(store?.activeSessionId) || null;
  const targetMessages = Array.isArray(messages)
    ? messages
    : getSessionMessages(key) || (resolveSessionKey(store?.activeSessionId) === key ? store?.messages : []);
  const runtime = getRuntime(key);
  const loading =
    options.loading === undefined
      ? Boolean(store?.loadingBySession?.[key])
      : Boolean(options.loading);
  const running =
    options.running === undefined
      ? loading || isThreadRuntimeBusy(runtime?.threadStatus)
      : Boolean(options.running);
  applyChatRuntimeEvent(
    projection,
    buildLegacyMessagesReconciledEvent({
      sessionId: key,
      agentId: resolveProjectionAgentId(store, key),
      messages: Array.isArray(targetMessages) ? targetMessages : [],
      loading,
      running
    })
  );
};

export const syncChatRuntimeProjectionStatus = (
  store,
  sessionId,
  status,
  options: { eventType?: string } = {}
) => {
  const key = resolveSessionKey(sessionId);
  const projection = ensureChatRuntimeProjectionForStore(store);
  if (!key || !projection) return;
  projection.activeSessionId = resolveSessionKey(store?.activeSessionId) || null;
  applyChatRuntimeEvent(projection, {
    event_type: options.eventType || 'session_runtime',
    source: 'legacy',
    strict: false,
    session_id: key,
    agent_id: resolveProjectionAgentId(store, key),
    runtime_status: status
  });
};

export const shouldPreferCachedMessages = (cached, server) => {
  if (!Array.isArray(cached) || cached.length === 0) return false;
  if (!Array.isArray(server) || server.length === 0) return true;
  if (cached.some((message) => isPendingAssistantMessage(message))) {
    return true;
  }
  const cachedLastAssistant = [...cached].reverse().find((message) => message?.role === 'assistant');
  const serverLastAssistant = [...server].reverse().find((message) => message?.role === 'assistant');
  if (cachedLastAssistant || serverLastAssistant) {
    const cachedEventId = normalizeStreamEventId(cachedLastAssistant?.stream_event_id);
    const serverEventId = normalizeStreamEventId(serverLastAssistant?.stream_event_id);
    if (cachedEventId !== null && (serverEventId === null || cachedEventId > serverEventId)) {
      return true;
    }
    const cachedContentLen = String(cachedLastAssistant?.content || '').length;
    const serverContentLen = String(serverLastAssistant?.content || '').length;
    if (cachedContentLen > serverContentLen) {
      return true;
    }
  }
  return cached.length > server.length;
};

export const MANUAL_COMPACTION_PENDING_MARKER_TTL_MS = 30_000;

export const isFreshPendingManualCompactionMarker = (message, now = Date.now()): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (!isCompactionMarkerAssistantMessage(message)) return false;
  if (!normalizeFlag(message.manual_compaction_marker ?? message.manualCompactionMarker)) {
    return false;
  }
  if (
    !normalizeFlag(message.workflowStreaming) &&
    !normalizeFlag(message.stream_incomplete) &&
    !normalizeFlag(message.reasoningStreaming)
  ) {
    return false;
  }
  const createdAtMs = resolveTimestampMs(message.created_at);
  if (createdAtMs === null) {
    return true;
  }
  return Math.max(0, now - createdAtMs) <= MANUAL_COMPACTION_PENDING_MARKER_TTL_MS;
};

export const clearCompletedAssistantStreamingState = (
  messages,
  options: { preservePendingManualCompaction?: boolean } = {}
) => {
  const preservePendingManualCompaction = options.preservePendingManualCompaction !== false;
  const now = Date.now();
  if (!Array.isArray(messages)) return;
  messages.forEach((message) => {
    if (!message || message.role !== 'assistant') return;
    if (
      preservePendingManualCompaction &&
      isFreshPendingManualCompactionMarker(message, now)
    ) {
      return;
    }
    if (!stopPendingAssistantMessage(message)) {
      message.workflowStreaming = false;
      message.stream_incomplete = false;
      message.reasoningStreaming = false;
    }
    clearAssistantRetryState(message);
  });
};

export const countAssistantStreamingMessages = (messages) => {
  if (!Array.isArray(messages)) return 0;
  return messages.reduce((count, message) => {
    if (!message || message.role !== 'assistant') {
      return count;
    }
    return count + (isPendingAssistantMessage(message) ? 1 : 0);
  }, 0);
};

export function buildRuntimeDebugSnapshot(runtime) {
  const pendingManualCompactionStartedAt = Number(
    runtime?.pendingManualCompaction?.startedAt ?? 0
  );
  return {
    threadStatus: normalizeThreadRuntimeStatus(runtime?.threadStatus),
    loaded: Boolean(runtime?.loaded),
    streamLifecycle: normalizeStreamLifecyclePhase(runtime?.streamLifecycle),
    hasWatchController: Boolean(runtime?.watchController),
    watchActiveRoundCount: Number(runtime?.watchActiveRoundCount) || 0,
    hasSendController: Boolean(runtime?.sendController),
    hasResumeController: Boolean(runtime?.resumeController),
    sendAborted: runtime?.sendController?.signal?.aborted === true,
    resumeAborted: runtime?.resumeController?.signal?.aborted === true,
    pendingManualCompaction: Boolean(runtime?.pendingManualCompaction),
    pendingManualCompactionAgeMs:
      Number.isFinite(pendingManualCompactionStartedAt) && pendingManualCompactionStartedAt > 0
        ? Math.max(0, Date.now() - pendingManualCompactionStartedAt)
        : null
  };
}

export const shouldRetainActiveSessionDuringListRefresh = (store, nextSessions) => {
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  if (!activeSessionId) {
    return false;
  }
  if ((Array.isArray(nextSessions) ? nextSessions : []).some((item) => resolveSessionKey(item?.id) === activeSessionId)) {
    return false;
  }
  const activeMessages = Array.isArray(store?.messages) ? store.messages : [];
  const runtime = getRuntime(activeSessionId);
  const runtimeStatus = normalizeThreadRuntimeStatus(runtime?.threadStatus);
  const isRuntimeHot =
    Boolean(store?.loadingBySession?.[activeSessionId]) ||
    isThreadRuntimeBusy(runtimeStatus) ||
    hasRunningAssistantMessage(activeMessages);
  const hasContext = hasRetainedConversationContext({
    activeSessionId,
    draftAgentId: store?.draftAgentId,
    messageCount: activeMessages.length
  });
  return isRuntimeHot || hasContext;
};

export const mergeRetainedActiveSessionIntoList = (store, nextSessions) => {
  const normalizedNextSessions = Array.isArray(nextSessions) ? nextSessions : [];
  if (!shouldRetainActiveSessionDuringListRefresh(store, normalizedNextSessions)) {
    return normalizedNextSessions;
  }
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  const existingActiveSession =
    (Array.isArray(store?.sessions)
      ? store.sessions.find((item) => resolveSessionKey(item?.id) === activeSessionId)
      : null) || null;
  if (!existingActiveSession) {
    return normalizedNextSessions;
  }
  chatDebugLog('messenger.conversation', 'retain-active-session-during-refresh', {
    activeSessionId,
    previousSessionCount: Array.isArray(store?.sessions) ? store.sessions.length : 0,
    nextSessionCount: normalizedNextSessions.length,
    messageCount: Array.isArray(store?.messages) ? store.messages.length : 0,
    runtime: buildRuntimeDebugSnapshot(getRuntime(activeSessionId))
  });
  return sortSessionsByActivity([existingActiveSession, ...normalizedNextSessions]);
};

export const readRuntimePendingManualCompaction = (runtime, sessionId = null) => {
  const pending = runtime?.pendingManualCompaction;
  if (!pending || typeof pending !== 'object') {
    return null;
  }
  const startedAt = Number((pending as Record<string, unknown>).startedAt ?? 0);
  if (
    Number.isFinite(startedAt) &&
    startedAt > 0 &&
    Date.now() - startedAt > MANUAL_COMPACTION_PENDING_MARKER_TTL_MS
  ) {
    runtime.pendingManualCompaction = null;
    chatDebugLog('chat.compaction.manual', 'pending-marker-cleared', {
      sessionId,
      reason: 'stale',
      pendingAgeMs: Math.max(0, Date.now() - startedAt)
    });
    return null;
  }
  return pending as Record<string, unknown>;
};

export const markRuntimePendingManualCompaction = (runtime, sessionId = null) => {
  if (!runtime) return;
  runtime.pendingManualCompaction = {
    startedAt: Date.now()
  };
  chatDebugLog('chat.compaction.manual', 'pending-marker-set', {
    sessionId,
    runtime: buildRuntimeDebugSnapshot(runtime)
  });
};

export const clearRuntimePendingManualCompaction = (runtime, sessionId = null, reason = 'clear') => {
  const pending = readRuntimePendingManualCompaction(runtime, sessionId);
  if (!pending) return false;
  const startedAt = Number(pending.startedAt ?? 0);
  runtime.pendingManualCompaction = null;
  chatDebugLog('chat.compaction.manual', 'pending-marker-cleared', {
    sessionId,
    reason,
    pendingAgeMs:
      Number.isFinite(startedAt) && startedAt > 0 ? Math.max(0, Date.now() - startedAt) : null
  });
  return true;
};

export const claimRuntimePendingManualCompaction = (runtime, sessionId = null, roundNumber = null) => {
  const pending = readRuntimePendingManualCompaction(runtime, sessionId);
  if (!pending) return false;
  const startedAt = Number(pending.startedAt ?? 0);
  runtime.pendingManualCompaction = null;
  chatDebugLog('chat.compaction.manual', 'pending-marker-claimed', {
    sessionId,
    round: normalizeStreamRound(roundNumber),
    pendingAgeMs:
      Number.isFinite(startedAt) && startedAt > 0 ? Math.max(0, Date.now() - startedAt) : null
  });
  return true;
};

export const summarizeWorkflowItemsForDebug = (items) => {
  if (!Array.isArray(items) || items.length === 0) {
    return [];
  }
  return items.slice(-3).map((item) => ({
    eventType: String(item?.eventType || item?.event || '').trim().toLowerCase() || null,
    status: String(item?.status || '').trim().toLowerCase() || null,
    toolName: String(item?.toolName || item?.tool || item?.name || '').trim() || null,
    toolCallId: String(item?.toolCallId || item?.tool_call_id || '').trim() || null
  }));
};

export const summarizeAssistantMessageForDebug = (message) => {
  if (!message || message.role !== 'assistant') {
    return null;
  }
  return {
    createdAt: String(message.created_at || '').trim() || null,
    streamEventId: normalizeStreamEventId(message.stream_event_id),
    streamRound: normalizeStreamRound(message.stream_round),
    streamIncomplete: normalizeFlag(message.stream_incomplete),
    workflowStreaming: normalizeFlag(message.workflowStreaming),
    reasoningStreaming: normalizeFlag(message.reasoningStreaming),
    contentLength: String(message.content || '').length,
    reasoningLength: String(message.reasoning || '').length,
    contextTokens: normalizeContextTokens(message.stats?.contextTokens),
    contextTotalTokens: normalizeContextTotalTokens(message.stats?.contextTotalTokens),
    workflowItemCount: Array.isArray(message.workflowItems) ? message.workflowItems.length : 0,
    workflowTail: summarizeWorkflowItemsForDebug(message.workflowItems),
    questionPanelStatus: String(message?.questionPanel?.status || '').trim() || null,
    manualCompactionMarker: normalizeFlag(
      message?.manual_compaction_marker ?? message?.manualCompactionMarker
    )
  };
};

export const summarizeMessagesForDebug = (messages) => {
  if (!Array.isArray(messages)) {
    return {
      messageCount: 0,
      assistantCount: 0,
      pendingAssistant: null,
      tailAssistant: null
    };
  }
  const assistants = messages.filter((message) => message?.role === 'assistant' && !message?.isGreeting);
  return {
    messageCount: messages.length,
    assistantCount: assistants.length,
    pendingAssistant: summarizeAssistantMessageForDebug(findPendingAssistantMessage(messages)),
    tailAssistant: summarizeAssistantMessageForDebug(assistants[assistants.length - 1] || null)
  };
};

export const isForegroundRealtimeAssistant = (message) =>
  Boolean(
    message &&
      message.role === 'assistant' &&
      !message.isGreeting &&
      !message.hiddenInternal &&
      isPendingAssistantMessage(message)
  );

export const shouldPreserveUnmatchedLiveAssistant = (message) => {
  if (!message || message.role !== 'assistant' || message.isGreeting) {
    return false;
  }
  if (message.realtime_protected === true) {
    return true;
  }
  if (isPendingAssistantMessage(message)) {
    return true;
  }
  if (
    isCompactionMarkerAssistantMessage(message) &&
    !normalizeFlag(message?.workflowStreaming) &&
    !normalizeFlag(message?.reasoningStreaming) &&
    !normalizeFlag(message?.stream_incomplete)
  ) {
    return true;
  }
  return false;
};

export const mergeForegroundHydratedMessagesWithLive = (liveMessages, hydratedMessages) => {
  if (!Array.isArray(hydratedMessages)) {
    return {
      messages: Array.isArray(liveMessages) ? liveMessages : [],
      debug: {
        matchedLiveAssistantCount: 0,
        appendedLivePending: false,
        pendingAssistantPreserved: false
      }
    };
  }
  if (!Array.isArray(liveMessages) || liveMessages.length === 0) {
    return {
      messages: hydratedMessages,
      debug: {
        matchedLiveAssistantCount: 0,
        appendedLivePending: false,
        pendingAssistantPreserved: false
      }
    };
  }
  const liveAssistants = buildAssistantMatchEntries(liveMessages);
  if (liveAssistants.length === 0) {
    return {
      messages: hydratedMessages,
      debug: {
        matchedLiveAssistantCount: 0,
        appendedLivePending: false,
        pendingAssistantPreserved: false
      }
    };
  }
  const matchedLiveAssistants = new Set();
  const hydratedAssistantEntryMap = buildAssistantMatchEntryMap(hydratedMessages);
  const mergedMessages = hydratedMessages.map((message) => {
    if (!message || message.role !== 'assistant' || message.isGreeting) {
      return message;
    }
    const matchIndex = findSnapshotAssistantIndexExcluding(
      message,
      hydratedAssistantEntryMap.get(message),
      liveAssistants,
      matchedLiveAssistants
    );
    if (matchIndex < 0) {
      return message;
    }
    const liveTarget = liveAssistants[matchIndex].message;
    matchedLiveAssistants.add(matchIndex);
    mergeSnapshotAssistant(liveTarget, message);
    return liveTarget;
  });
  // Preserve only live assistants that are still semantically authoritative:
  // pending live output, explicitly realtime-protected items, or terminal
  // compaction markers. Older completed assistants should defer to hydrated
  // history; otherwise stale image/text replies can be reinserted forever.
  for (let i = 0; i < liveAssistants.length; i += 1) {
    if (!matchedLiveAssistants.has(i)) {
      const liveTarget = liveAssistants[i].message;
      if (!shouldPreserveUnmatchedLiveAssistant(liveTarget)) {
        continue;
      }
      const insertAfterIndex = findLiveAssistantInsertionIndex(liveTarget, mergedMessages);
      if (insertAfterIndex >= 0) {
        mergedMessages.splice(insertAfterIndex + 1, 0, liveTarget);
      } else {
        mergedMessages.push(liveTarget);
      }
      matchedLiveAssistants.add(i);
    }
  }
  const livePendingAssistant = findPendingAssistantMessage(liveMessages);
  let appendedLivePending = false;
  const suppressedLivePendingCompaction =
    isCompactionMarkerAssistantMessage(livePendingAssistant) &&
    isSupersededRunningManualCompactionMarker(livePendingAssistant, mergedMessages);
  // Check if livePendingAssistant was already matched by checking its index in liveAssistants
  const livePendingAssistantIndex = liveAssistants.findIndex(
    (entry) => entry.message === livePendingAssistant
  );
  const livePendingAlreadyMatched = livePendingAssistantIndex >= 0 && matchedLiveAssistants.has(livePendingAssistantIndex);
  if (
    isForegroundRealtimeAssistant(livePendingAssistant) &&
    !livePendingAlreadyMatched &&
    !suppressedLivePendingCompaction
  ) {
    mergedMessages.push(livePendingAssistant);
    appendedLivePending = true;
  }
  return {
    messages: mergedMessages,
    debug: {
      matchedLiveAssistantCount: matchedLiveAssistants.size,
      liveAssistantCount: liveAssistants.length,
      appendedLivePending,
      suppressedLivePendingCompaction,
      pendingAssistantPreserved: Boolean(
        livePendingAssistant && mergedMessages.includes(livePendingAssistant)
      ),
      livePendingAssistant: summarizeAssistantMessageForDebug(livePendingAssistant),
      liveMessages: summarizeMessagesForDebug(liveMessages),
      hydratedMessages: summarizeMessagesForDebug(hydratedMessages),
      mergedMessages: summarizeMessagesForDebug(mergedMessages)
    }
  };
};

export const captureRealtimeWorkflowMutationBaseline = (message, messages) => ({
  messageIndex: Array.isArray(messages) ? messages.indexOf(message) : -1,
  workflowItemCount: Array.isArray(message?.workflowItems) ? message.workflowItems.length : 0,
  summary: summarizeAssistantMessageForDebug(message)
});

export const logRealtimeWorkflowMutation = ({
  phase,
  sessionId,
  eventType,
  eventId,
  roundNumber,
  userRoundNumber,
  message,
  messages,
  before
}) => {
  if (!isChatDebugEnabled()) {
    return;
  }
  const normalizedEventType = normalizeStreamEventType(eventType);
  const afterMessageIndex = Array.isArray(messages) ? messages.indexOf(message) : -1;
  const afterWorkflowItemCount = Array.isArray(message?.workflowItems) ? message.workflowItems.length : 0;
  const detached = before.messageIndex >= 0 && afterMessageIndex < 0;
  const workflowChanged = before.workflowItemCount !== afterWorkflowItemCount;
  const shouldLog =
    detached ||
    workflowChanged ||
    normalizedEventType === 'tool_call' ||
    normalizedEventType === 'tool_result' ||
    normalizedEventType === 'tool_output_delta' ||
    normalizedEventType === 'approval_request' ||
    normalizedEventType === 'llm_output';
  if (!shouldLog) {
    return;
  }
  chatDebugLog('chat.store.runtime', 'realtime-workflow-mutation', {
    phase,
    sessionId,
    eventType: normalizedEventType,
    eventId: normalizeStreamEventId(eventId),
    roundNumber: normalizeStreamRound(roundNumber),
    userRoundNumber: normalizeStreamRound(userRoundNumber),
    messageIndexBefore: before.messageIndex,
    messageIndexAfter: afterMessageIndex,
    messageDetached: detached,
    workflowItemCountBefore: before.workflowItemCount,
    workflowItemCountAfter: afterWorkflowItemCount,
    before: before.summary,
    after: summarizeAssistantMessageForDebug(message),
    pendingAssistant: summarizeAssistantMessageForDebug(findPendingAssistantMessage(messages))
  });
};
