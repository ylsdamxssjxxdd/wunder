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

import { buildWorkflowItem, dismissStaleInquiryPanels } from './chatDemoPanels';
import { applyRuntimeDerivedStatus, buildRuntimeDebugSnapshot, ensureRuntime, getRuntime, getSessionMessages, notifySessionSnapshot, refreshRuntimeStreamLifecycle, resolveSessionKey, syncChatRuntimeProjectionFromLegacy, touchSessionUpdatedAt } from './chatRuntimeState';
import { chatWatcherSharedState } from './chatSharedState';
import { buildMessage, normalizeHiddenInternalMessage, parseErrorText, resolveTimestampMs } from './chatStats';
import { getRuntimeLastEventId, normalizeStreamEventId, normalizeStreamRound } from './chatStreamIds';

export const setSessionLoading = (store, sessionId, value) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const beforeLoading = Boolean(store.loadingBySession[key]);
  const runtime = ensureRuntime(key);
  const beforeRuntime = buildRuntimeDebugSnapshot(runtime);
  if (value) {
    store.loadingBySession[key] = true;
  } else if (store.loadingBySession[key]) {
    delete store.loadingBySession[key];
  }
  syncChatRuntimeProjectionFromLegacy(store, key, null, {
    loading: Boolean(value),
    running: Boolean(value)
  });
  if (!runtime) return;
  if (value) {
    runtime.loaded = true;
    if (!isThreadRuntimeWaiting(runtime.threadStatus)) {
      runtime.threadStatus = 'running';
    }
    const afterRuntime = buildRuntimeDebugSnapshot(runtime);
    if (
      beforeLoading !== Boolean(value) ||
      beforeRuntime.threadStatus !== afterRuntime.threadStatus
    ) {
      chatDebugLog('chat.store.loading', 'set-session-loading', {
        sessionId: key,
        nextLoading: true,
        beforeLoading,
        beforeRuntime,
        afterRuntime
      });
    }
    return;
  }
  applyRuntimeDerivedStatus(store, key, runtime);
  const afterRuntime = buildRuntimeDebugSnapshot(runtime);
  const hasResidualControllers = afterRuntime.hasSendController || afterRuntime.hasResumeController;
  if (
    beforeLoading !== Boolean(value) ||
    beforeRuntime.threadStatus !== afterRuntime.threadStatus ||
    hasResidualControllers
  ) {
    chatDebugLog('chat.store.loading', 'set-session-loading', {
      sessionId: key,
      nextLoading: false,
      beforeLoading,
      beforeRuntime,
      afterRuntime,
      hasResidualControllers
    });
  }
};

export const clearWatchdog = (runtime) => {
  if (!runtime) return;
  if (runtime.watchdogTimer) {
    clearTimeout(runtime.watchdogTimer);
    runtime.watchdogTimer = null;
  }
  if (runtime.watchReconcileTimer) {
    clearTimeout(runtime.watchReconcileTimer);
    runtime.watchReconcileTimer = null;
  }
  runtime.watchReconcileAt = 0;
  runtime.watchdogBusy = false;
  runtime.watchLastEventAt = 0;
};

export const clearSlowClientResume = (runtime) => {
  if (!runtime) return;
  if (runtime.slowClientResumeTimer) {
    clearTimeout(runtime.slowClientResumeTimer);
    runtime.slowClientResumeTimer = null;
  }
  runtime.slowClientResumeAfterEventId = 0;
};

type RuntimeStreamAbortReason = 'user_stop' | 'local_recovery' | 'teardown';

type ClearRuntimeStreamStateOptions = {
  abort?: boolean;
  abortReason?: RuntimeStreamAbortReason;
};

export function clearRuntimeSendStreamState(runtime, options: ClearRuntimeStreamStateOptions = {}) {
  if (!runtime) return false;
  const controller = runtime.sendController;
  if (!controller) return false;
  if (options.abort === true && controller?.signal?.aborted !== true) {
    runtime.sendAbortReason = runtime.sendAbortReason || options.abortReason || 'teardown';
    controller.abort();
  }
  runtime.sendController = null;
  runtime.sendRequestId = null;
  runtime.sendStartedAt = 0;
  runtime.sendLastEventAt = 0;
  return true;
}

export function clearRuntimeResumeStreamState(runtime, options: ClearRuntimeStreamStateOptions = {}) {
  if (!runtime) return false;
  const controller = runtime.resumeController;
  if (!controller) return false;
  if (options.abort === true && controller?.signal?.aborted !== true) {
    runtime.resumeAbortReason = runtime.resumeAbortReason || options.abortReason || 'teardown';
    controller.abort();
  }
  runtime.resumeController = null;
  runtime.resumeRequestId = null;
  runtime.resumeStartedAt = 0;
  runtime.resumeLastEventAt = 0;
  return true;
}

export function clearRuntimeInteractiveControllers(
  runtime,
  options: ClearRuntimeStreamStateOptions = {}
) {
  if (!runtime) return false;
  const clearedSend = clearRuntimeSendStreamState(runtime, options);
  const clearedResume = clearRuntimeResumeStreamState(runtime, options);
  if (clearedSend || clearedResume) {
    runtime.stopRequested = false;
    refreshRuntimeStreamLifecycle(runtime);
  }
  return clearedSend || clearedResume;
}

export function markRuntimeSendStreamStarted(runtime) {
  if (!runtime) return;
  const now = Date.now();
  runtime.sendAbortReason = '';
  runtime.sendStartedAt = now;
  runtime.sendLastEventAt = now;
}

export function markRuntimeResumeStreamStarted(runtime) {
  if (!runtime) return;
  const now = Date.now();
  runtime.resumeAbortReason = '';
  runtime.resumeStartedAt = now;
  runtime.resumeLastEventAt = now;
}

export function markRuntimeSendStreamActivity(runtime) {
  if (!runtime?.sendController) return;
  runtime.sendLastEventAt = Date.now();
}

export function markRuntimeResumeStreamActivity(runtime) {
  if (!runtime?.resumeController) return;
  runtime.resumeLastEventAt = Date.now();
}

export function recoverRuntimeInteractiveControllers(
  store,
  sessionId,
  runtime,
  options: {
    remoteRunning?: unknown;
    remoteLastEventId?: unknown;
    localLastEventId?: unknown;
  } = {}
) {
  if (!runtime) return false;
  const key = resolveSessionKey(sessionId);
  if (!key) return false;
  const localSessionMessages =
    getSessionMessages(key) ||
    (resolveSessionKey(store?.activeSessionId) === key && Array.isArray(store?.messages)
      ? store.messages
      : null);
  const localLastEventId = Math.max(
    normalizeStreamEventId(options.localLastEventId) || 0,
    resolveMaterializedMessageEventId(localSessionMessages),
    getRuntimeLastEventId(runtime)
  );
  const remoteLastEventId = normalizeStreamEventId(options.remoteLastEventId) || 0;
  const loading = Boolean(store?.loadingBySession?.[key]);
  const nowMs = Date.now();
  const sendReason = resolveInteractiveControllerRecoveryReason({
    hasController: Boolean(runtime.sendController),
    controllerAborted: runtime.sendController?.signal?.aborted === true,
    startedAt: runtime.sendStartedAt,
    lastEventAt: runtime.sendLastEventAt,
    loading,
    remoteRunning: options.remoteRunning,
    remoteLastEventId,
    localLastEventId,
    nowMs
  });
  const resumeReason = resolveInteractiveControllerRecoveryReason({
    hasController: Boolean(runtime.resumeController),
    controllerAborted: runtime.resumeController?.signal?.aborted === true,
    startedAt: runtime.resumeStartedAt,
    lastEventAt: runtime.resumeLastEventAt,
    loading,
    remoteRunning: options.remoteRunning,
    remoteLastEventId,
    localLastEventId,
    nowMs
  });
  let changed = false;
  if (sendReason) {
    chatDebugLog('chat.store.controller-recovery', 'clear-send-controller', {
      sessionId: key,
      reason: sendReason,
      remoteRunning: options.remoteRunning,
      localLastEventId,
      remoteLastEventId
    });
    changed = clearRuntimeSendStreamState(runtime, {
      abort: sendReason !== 'aborted' && sendReason !== 'remote_idle',
      abortReason: 'local_recovery'
    }) || changed;
  }
  if (resumeReason) {
    chatDebugLog('chat.store.controller-recovery', 'clear-resume-controller', {
      sessionId: key,
      reason: resumeReason,
      remoteRunning: options.remoteRunning,
      localLastEventId,
      remoteLastEventId
    });
    changed = clearRuntimeResumeStreamState(runtime, {
      abort: resumeReason !== 'aborted' && resumeReason !== 'remote_idle',
      abortReason: 'local_recovery'
    }) || changed;
  }
  if (changed) {
    refreshRuntimeStreamLifecycle(runtime);
  }
  return changed;
}

export const abortWatchStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.watchController) {
    runtime.watchController.abort();
    runtime.watchController = null;
  }
  runtime.watchActiveRoundCount = 0;
  runtime.watchRequestId = null;
  clearWatchdog(runtime);
  refreshRuntimeStreamLifecycle(runtime);
};

export const clearSessionWatcher = () => {
  if (chatWatcherSharedState.sessionWatchSessionId) {
    abortWatchStream(chatWatcherSharedState.sessionWatchSessionId);
  }
  chatWatcherSharedState.sessionWatchSessionId = '';
};

export const resolveMaxStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  let maxId = 0;
  messages.forEach((message) => {
    const eventId = normalizeStreamEventId(message.stream_event_id);
    if (eventId && eventId > maxId) {
      maxId = eventId;
    }
  });
  return maxId > 0 ? maxId : null;
};

export const resolveLastStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    const eventId = normalizeStreamEventId(message?.stream_event_id);
    if (eventId !== null) {
      return eventId;
    }
  }
  return null;
};

export const resolveLastAssistantStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    const eventId = normalizeStreamEventId(message.stream_event_id);
    if (eventId !== null) {
      return eventId;
    }
  }
  return null;
};

export const resolveMaterializedMessageEventId = (messages) =>
  Math.max(
    resolveLastStreamEventId(messages) || 0,
    resolveLastAssistantStreamEventId(messages) || 0,
    resolveMaxStreamEventId(messages) || 0
  );

export const resolveHiddenInternalUserEvent = (payload, data) =>
  Boolean(
    data?.hidden_internal_user ??
      data?.hiddenInternalUser ??
      payload?.hidden_internal_user ??
      payload?.hiddenInternalUser
  );

export const resolveKnownSessionEventFloor = (sessionId, messages = getSessionMessages(sessionId)) => {
  const runtime = getRuntime(sessionId);
  const messageMaxEventId = resolveMaterializedMessageEventId(messages);
  return Math.max(getRuntimeLastEventId(runtime), messageMaxEventId);
};

export const resolveMaxStreamRound = (messages) => {
  if (!Array.isArray(messages)) return null;
  let maxRound = 0;
  messages.forEach((message) => {
    if (message?.role !== 'assistant') return;
    const round = normalizeStreamRound(message.stream_round);
    if (round && round > maxRound) {
      maxRound = round;
    }
  });
  return maxRound > 0 ? maxRound : null;
};

export const ensurePendingAssistantMessage = (store, sessionId, messages, baseEventId) => {
  if (!Array.isArray(messages)) return null;
  const existing = findPendingAssistantMessage(messages);
  if (existing) return existing;
  // Stable id keeps the render key from degrading to an index; avoids remounts
  // when neighboring messages are inserted or prepended.
  const placeholderId = `local-assistant:pending:${sessionId}:${baseEventId || 0}`;
  const placeholder = {
    ...buildMessage('assistant', ''),
    message_id: placeholderId,
    client_message_id: placeholderId,
    workflowItems: [],
    workflowStreaming: false,
    stream_incomplete: true,
    stream_event_id: baseEventId || 0,
    stream_round: null
  };
  messages.push(placeholder);
  notifySessionSnapshot(store, sessionId, messages, true);
  return placeholder;
};

export const isDraftSessionBootstrapMessage = (message) =>
  Boolean(message && typeof message === 'object' && message.draft_session_bootstrap === true);

export const clearDraftSessionBootstrapMessages = (messages) => {
  if (!Array.isArray(messages)) return;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (!isDraftSessionBootstrapMessage(messages[index])) continue;
    messages.splice(index, 1);
  }
};

export const clearDraftSessionBootstrapMarkers = (messages) => {
  if (!Array.isArray(messages)) return;
  messages.forEach((message) => {
    if (!isDraftSessionBootstrapMessage(message)) return;
    delete message.draft_session_bootstrap;
  });
};

export const markAssistantMessageRequestFailed = (assistantMessage, detail) => {
  if (!assistantMessage || assistantMessage.role !== 'assistant') return;
  const normalizedDetail = parseErrorText(detail) || t('chat.workflow.requestFailedDetail');
  if (!Array.isArray(assistantMessage.workflowItems)) {
    assistantMessage.workflowItems = [];
  }
  assistantMessage.workflowItems.push(
    buildWorkflowItem(
      t('chat.workflow.requestFailed'),
      normalizedDetail,
      'failed',
      { eventType: 'request_failed' }
    )
  );
  assistantMessage.workflowStreaming = false;
  assistantMessage.reasoningStreaming = false;
  assistantMessage.stream_incomplete = false;
  if (!assistantMessage.content) {
    assistantMessage.content = normalizedDetail;
  }
};

export const resolveLastAssistantTimestampMs = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    const timestamp = resolveTimestampMs(message.created_at);
    return Number.isFinite(timestamp) ? timestamp : null;
  }
  return null;
};

export const WATCH_USER_MESSAGE_DEDUP_MS = 2000;
export const WATCHDOG_IDLE_MS_ACTIVE = 1500;
export const WATCHDOG_IDLE_MS_BACKGROUND = 14000;
export const WATCHDOG_IDLE_MS_HIDDEN = 26000;
export const WATCHDOG_INTERVAL_MS_ACTIVE = 500;
export const WATCHDOG_INTERVAL_MS_BACKGROUND = 3500;
export const WATCHDOG_INTERVAL_MS_HIDDEN = 7000;
export const WATCH_RECONCILE_DELAY_MS = 150;
export const WATCH_RECONCILE_COOLDOWN_MS = 1800;
export const SLOW_CLIENT_RESUME_DELAY_MS = 120;
export const STREAM_FLUSH_BASE_MS = 40;
export const STREAM_FLUSH_MAX_MS = 160;
export const HISTORY_PAGE_LIMIT = 80;
export const HISTORY_PAGE_MAX = 200;
export const MESSAGE_WINDOW_LIMIT = 400;
export const MESSAGE_WINDOW_THRESHOLD = 600;
export const MESSAGE_WINDOW_MAX = 2000;
export const DESKTOP_MESSAGE_WINDOW_LIMIT = 96;
export const DESKTOP_MESSAGE_WINDOW_THRESHOLD = 140;
export const DESKTOP_MESSAGE_WINDOW_MAX = 640;
export const SESSION_DETAIL_MESSAGE_LIMIT = 500;
export const DESKTOP_SESSION_DETAIL_MESSAGE_LIMIT = DESKTOP_MESSAGE_WINDOW_LIMIT;
export const WINDOWING_ENABLED_KEY = 'wunder_chat_windowing';

export const resolveStreamFlushMs = (messageCount, override) => {
  if (Number.isFinite(override)) {
    return Math.min(STREAM_FLUSH_MAX_MS, Math.max(0, Number(override)));
  }
  return STREAM_FLUSH_BASE_MS;
};

export const resolveStreamFlushMsForMessages = (messages) =>
  resolveStreamFlushMs(Array.isArray(messages) ? messages.length : 0, null);

export const normalizeHistoryPageLimit = (value) => {
  const parsed = Number.parseInt(String(value ?? HISTORY_PAGE_LIMIT), 10);
  if (!Number.isFinite(parsed) || parsed <= 0) return HISTORY_PAGE_LIMIT;
  return Math.min(parsed, HISTORY_PAGE_MAX);
};

export const resolveSessionDetailMessageLimit = (desktopMode = false) =>
  desktopMode ? DESKTOP_SESSION_DETAIL_MESSAGE_LIMIT : SESSION_DETAIL_MESSAGE_LIMIT;

export const resolveMessageWindowLimit = (desktopMode = false) =>
  desktopMode ? DESKTOP_MESSAGE_WINDOW_LIMIT : MESSAGE_WINDOW_LIMIT;

export const resolveMessageWindowThreshold = (desktopMode = false) =>
  desktopMode ? DESKTOP_MESSAGE_WINDOW_THRESHOLD : MESSAGE_WINDOW_THRESHOLD;

export const resolveMessageWindowMax = (desktopMode = false) =>
  desktopMode ? DESKTOP_MESSAGE_WINDOW_MAX : MESSAGE_WINDOW_MAX;

export const isWindowingEnabled = () => {
  try {
    const raw = localStorage.getItem(WINDOWING_ENABLED_KEY);
    if (!raw) return true;
    return raw !== '0' && raw.toLowerCase() !== 'false';
  } catch (error) {
    return true;
  }
};

export const shouldInsertWatchUserMessage = (messages, content, eventTimestampMs, dedupeKey) => {
  if (!Array.isArray(messages) || !content) return false;
  // Normalize whitespace so trailing/leading spaces or CRLF differences between
  // the local optimistic user message and the backend round_start payload do
  // not defeat deduplication (a common cause of duplicate user bubbles).
  const normalizeForCompare = (value) => String(value || '').replace(/\s+/g, ' ').trim();
  const normalizedContent = normalizeForCompare(content);
  // Primary dedupe: if a dedupeKey (client_message_id or event_id) is provided,
  // skip insertion when an existing user message already carries the same key.
  if (dedupeKey) {
    const key = String(dedupeKey);
    for (let i = messages.length - 1; i >= 0; i -= 1) {
      const message = messages[i];
      if (message?.role !== 'user') continue;
      const existingKey =
        message.client_message_id || message.clientMessageId || message.message_id || message.id;
      if (existingKey && String(existingKey) === key) {
        return false;
      }
    }
  }
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'user') continue;
    const lastContent = normalizeForCompare(message.content);
    if (lastContent !== normalizedContent) {
      return true;
    }
    const lastTimestamp = resolveTimestampMs(message.created_at);
    if (!Number.isFinite(eventTimestampMs) || !Number.isFinite(lastTimestamp)) {
      return false;
    }
    return Math.abs(eventTimestampMs - lastTimestamp) > WATCH_USER_MESSAGE_DEDUP_MS;
  }
  return true;
};

export const isDocumentHidden = () =>
  typeof document !== 'undefined' && document.visibilityState === 'hidden';

export const resolveWatchdogProfile = (store, sessionId) => {
  if (isDocumentHidden()) {
    return {
      idleMs: WATCHDOG_IDLE_MS_HIDDEN,
      intervalMs: WATCHDOG_INTERVAL_MS_HIDDEN
    };
  }
  const activeSessionId = resolveSessionKey(store?.activeSessionId);
  const isForeground = Boolean(activeSessionId && activeSessionId === sessionId);
  if (isForeground) {
    return {
      idleMs: WATCHDOG_IDLE_MS_ACTIVE,
      intervalMs: WATCHDOG_INTERVAL_MS_ACTIVE
    };
  }
  return {
    idleMs: WATCHDOG_IDLE_MS_BACKGROUND,
    intervalMs: WATCHDOG_INTERVAL_MS_BACKGROUND
  };
};

export const hasAnchoredWatchUserMessage = (messages, anchor, content) => {
  if (!Array.isArray(messages) || !anchor || !content) return false;
  const anchorIndex = messages.indexOf(anchor);
  if (anchorIndex <= 0) return false;
  const previous = messages[anchorIndex - 1];
  if (previous?.role !== 'user') return false;
  return String(previous.content || '') === content;
};

export const insertWatchUserMessage = (
  store,
  sessionId,
  messages,
  content,
  eventTimestampMs,
  anchor,
  options = {}
) => {
  const optionRecord: Record<string, unknown> =
    options && typeof options === 'object' ? (options as Record<string, unknown>) : {};
  const dedupeKey = optionRecord.dedupeKey;
  if (hasAnchoredWatchUserMessage(messages, anchor, content)) {
    return;
  }
  if (!shouldInsertWatchUserMessage(messages, content, eventTimestampMs, dedupeKey)) {
    return;
  }
  const createdAt = Number.isFinite(eventTimestampMs)
    ? new Date(eventTimestampMs).toISOString()
    : undefined;
  const userMessage = buildMessage('user', content, createdAt, {
    hiddenInternal: normalizeHiddenInternalMessage(optionRecord.hiddenInternal)
  });
  // Stamp the dedupe key onto the inserted message so subsequent replay of the
  // same round_start event (e.g. after a watch reconnect) is suppressed.
  if (dedupeKey) {
    (userMessage as Record<string, unknown>).client_message_id = String(dedupeKey);
  }
  const anchorIndex = anchor ? messages.indexOf(anchor) : -1;
  if (anchorIndex >= 0) {
    messages.splice(anchorIndex, 0, userMessage);
  } else {
    messages.push(userMessage);
  }
  clearSupersededPendingAssistantMessages(messages);
  dismissStaleInquiryPanels(messages);
  touchSessionUpdatedAt(store, sessionId, eventTimestampMs ?? Date.now());
  notifySessionSnapshot(store, sessionId, messages, true);
};
