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

import { hasPlanSteps, normalizeInquiryPanelState, normalizePlanPayload, shouldAutoShowPlan } from './chatDemoPanels';
import { resolveChatSnapshotStorageKeys } from './chatPersist';
import { getSessionMessages } from './chatRuntimeState';
import { clearAssistantRetryState, mergeMessageStats, normalizeHiddenInternalMessage, normalizeInteractionTimestamp, normalizeMessageStats, normalizeMessageSubagents, parseOptionalCount, resolveTimestampMs } from './chatStats';
import { normalizeFlag, normalizeStreamEventId, normalizeStreamRound } from './chatStreamIds';
import { SnapshotAssistantMessage } from './chatTypes';
import { normalizeAssistantContent, normalizeAssistantOutput, resolveAssistantReasoning } from './chatWorkflowHydration';

export const SNAPSHOT_FLUSH_MS = 800;
export const SNAPSHOT_IDLE_TIMEOUT_MS = 2000;
export const SNAPSHOT_MESSAGE_LIMIT = 200;
export const MAX_SNAPSHOT_MESSAGES = 50;
export const SNAPSHOT_MATCH_WINDOW_MS = 2000;
let snapshotTimer = null;
let pendingSnapshotContext = null;
export const normalizeSnapshotAttachments = (attachments) => {
  if (!Array.isArray(attachments)) return [];
  return attachments
    .map((item) => {
      if (!item || typeof item !== 'object') return null;
      const record = item;
      const name = String(record?.name || '').trim();
      const contentType = String(
        record?.content_type ?? record?.mime_type ?? record?.mimeType ?? ''
      )
        .trim()
        .toLowerCase();
      const publicPath = String(record?.public_path ?? record?.publicPath ?? '').trim();
      const rawContent = String(record?.content || '').trim();
      const content = !rawContent.startsWith('data:') ? rawContent : '';
      if (!name && !contentType && !publicPath && !content) return null;
      const normalized: Record<string, unknown> = {};
      if (name) normalized.name = name;
      if (contentType) normalized.content_type = contentType;
      if (publicPath) normalized.public_path = publicPath;
      if (content) normalized.content = content;
      return normalized;
    })
    .filter(Boolean);
};

export const normalizeSnapshotMessage = (message) => {
  if (!message || typeof message !== 'object') return null;
  const normalizedAssistant =
    message.role === 'assistant'
      ? normalizeAssistantOutput(message.content, resolveAssistantReasoning(message))
      : null;
  const base: SnapshotAssistantMessage = {
    role: message.role,
    content:
      message.role === 'assistant'
        ? normalizedAssistant?.content || ''
        : typeof message.content === 'string'
          ? message.content
          : String(message.content || ''),
    created_at: message.created_at || ''
  };
  if (normalizeHiddenInternalMessage(message.hiddenInternal)) {
    base.hiddenInternal = true;
  }
  if (normalizeFlag(message.manual_compaction_marker ?? message.manualCompactionMarker)) {
    base.manual_compaction_marker = true;
  }
  if (message.realtime_protected === true) {
    base.realtime_protected = true;
  }
  const streamEventId = normalizeStreamEventId(message.stream_event_id);
  if (streamEventId !== null) {
    base.stream_event_id = streamEventId;
  }
  if (message.role === 'assistant') {
    base.reasoning = normalizedAssistant?.reasoning || '';
    base.reasoningStreaming = normalizeFlag(message.reasoningStreaming);
    base.workflowStreaming = normalizeFlag(message.workflowStreaming);
    base.stream_incomplete = normalizeFlag(message.stream_incomplete);
    if (normalizeFlag(message.slow_client)) {
      base.slow_client = true;
    }
    if (normalizeFlag(message.resume_available)) {
      base.resume_available = true;
    }
    const streamRound = normalizeStreamRound(message.stream_round);
    if (streamRound !== null) {
      base.stream_round = streamRound;
    }
    const waitingUpdatedAtMs = normalizeInteractionTimestamp(
      message.waiting_updated_at_ms ?? message.waitingUpdatedAtMs
    );
    if (waitingUpdatedAtMs !== null) {
      base.waiting_updated_at_ms = waitingUpdatedAtMs;
    }
    const waitingFirstOutputAtMs = normalizeInteractionTimestamp(
      message.waiting_first_output_at_ms ?? message.waitingFirstOutputAtMs
    );
    if (waitingFirstOutputAtMs !== null) {
      base.waiting_first_output_at_ms = waitingFirstOutputAtMs;
    }
    const waitingPhaseFirstOutputAtMs = normalizeInteractionTimestamp(
      message.waiting_phase_first_output_at_ms ?? message.waitingPhaseFirstOutputAtMs
    );
    if (waitingPhaseFirstOutputAtMs !== null) {
      base.waiting_phase_first_output_at_ms = waitingPhaseFirstOutputAtMs;
    }
    const retryState = String(message.retry_state ?? message.retryState ?? '').trim().toLowerCase();
    if (retryState) {
      base.retry_state = retryState;
    }
    const retryAttempt = parseOptionalCount(message.retry_attempt ?? message.retryAttempt);
    if (retryAttempt !== null) {
      base.retry_attempt = retryAttempt;
    }
    const retryMaxAttempts = parseOptionalCount(
      message.retry_max_attempts ?? message.retryMaxAttempts
    );
    if (retryMaxAttempts !== null) {
      base.retry_max_attempts = retryMaxAttempts;
    }
    const retryDelaySeconds = Number(message.retry_delay_s ?? message.retryDelayS);
    if (Number.isFinite(retryDelaySeconds) && retryDelaySeconds > 0) {
      base.retry_delay_s = retryDelaySeconds;
    }
    const retryStartedAtMs = normalizeInteractionTimestamp(
      message.retry_started_at_ms ?? message.retryStartedAtMs
    );
    if (retryStartedAtMs !== null) {
      base.retry_started_at_ms = retryStartedAtMs;
    }
    const retryNextAttemptAtMs = normalizeInteractionTimestamp(
      message.retry_next_attempt_at_ms ?? message.retryNextAttemptAtMs
    );
    if (retryNextAttemptAtMs !== null) {
      base.retry_next_attempt_at_ms = retryNextAttemptAtMs;
    }
    const retryReason = String(message.retry_reason ?? message.retryReason ?? '').trim();
    if (retryReason) {
      base.retry_reason = retryReason;
    }
    const retryError = String(message.retry_error ?? message.retryError ?? '').trim();
    if (retryError) {
      base.retry_error = retryError;
    }
    if (Array.isArray(message.workflowItems) && message.workflowItems.length) {
      base.workflowItems = message.workflowItems;
    }
    const subagents = normalizeMessageSubagents(message.subagents);
    if (subagents.length > 0) {
      base.subagents = subagents;
    }
    const plan = normalizePlanPayload(message.plan);
    if (plan) {
      base.plan = plan;
    }
    const questionPanel = normalizeInquiryPanelState(message.questionPanel);
    if (questionPanel) {
      base.questionPanel = questionPanel;
    }
    const feedback = normalizeMessageFeedback(message.feedback);
    if (feedback) {
      base.feedback = feedback;
    }
    const stats = normalizeMessageStats(message.stats);
    if (stats) {
      base.stats = stats;
    }
    base.planVisible = shouldAutoShowPlan(plan, message);
  }
  if (message.isGreeting) {
    base.isGreeting = true;
  }
  const attachments = normalizeSnapshotAttachments(message.attachments);
  if (attachments.length) {
    base.attachments = attachments;
  }
  return base;
};

export const buildSnapshotMessages = (messages = []) => {
  const sliced = messages.slice(-MAX_SNAPSHOT_MESSAGES);
  return sliced
    .map((message) => {
      const normalized = normalizeSnapshotMessage(message);
      if (!normalized) return null;
      const hasWorkflowItems =
        Array.isArray(normalized.workflowItems) && normalized.workflowItems.length > 0;
      const shouldKeepWorkflow =
        hasWorkflowItems || normalized.stream_incomplete || normalized.workflowStreaming;
      if (!shouldKeepWorkflow) {
        delete normalized.workflowItems;
      }
      if (Array.isArray(normalized.subagents) && normalized.subagents.length === 0) {
        delete normalized.subagents;
      }
      return normalized;
    })
    .filter(Boolean);
};

export const buildChatSnapshot = (sessionId, sourceMessages = []) => {
  const normalizedSessionId = String(sessionId || '').trim();
  if (!normalizedSessionId) return null;
  const safeSourceMessages = Array.isArray(sourceMessages) ? sourceMessages : [];
  const trimmed =
    safeSourceMessages.length > SNAPSHOT_MESSAGE_LIMIT
      ? safeSourceMessages.slice(-SNAPSHOT_MESSAGE_LIMIT)
      : safeSourceMessages;
  const messages = buildSnapshotMessages(trimmed);
  if (!messages.length) return null;
  return {
    sessionId: normalizedSessionId,
    messages,
    updatedAt: Date.now()
  };
};

export const readChatSnapshot = () => {
  try {
    const keys = resolveChatSnapshotStorageKeys();
    const raw =
      localStorage.getItem(keys.primary) ??
      localStorage.getItem(keys.legacyScoped) ??
      localStorage.getItem(keys.globalPrimary) ??
      localStorage.getItem(keys.globalLegacy);
    if (!raw) return null;
    if (!localStorage.getItem(keys.primary)) {
      localStorage.setItem(keys.primary, raw);
    }
    if (!localStorage.getItem(keys.legacyScoped)) {
      localStorage.setItem(keys.legacyScoped, raw);
    }
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return null;
    const sessionId = String(parsed.sessionId || '');
    const messages = Array.isArray(parsed.messages) ? parsed.messages : [];
    if (!sessionId || !messages.length) return null;
    return {
      sessionId,
      messages
    };
  } catch (error) {
    return null;
  }
};

export const writeChatSnapshot = (payload) => {
  if (!payload) return;
  try {
    const serialized = JSON.stringify(payload);
    const keys = resolveChatSnapshotStorageKeys();
    localStorage.setItem(keys.primary, serialized);
    localStorage.setItem(keys.legacyScoped, serialized);
  } catch (error) {
    // ignore persistence errors
  }
};

export const clearChatSnapshot = (sessionId) => {
  try {
    const current = readChatSnapshot();
    if (!current || current.sessionId !== String(sessionId || '')) return;
    const keys = resolveChatSnapshotStorageKeys();
    localStorage.removeItem(keys.primary);
    localStorage.removeItem(keys.legacyScoped);
  } catch (error) {
    // ignore storage errors
  }
};

export const clearAllChatSnapshots = () => {
  try {
    const keys = resolveChatSnapshotStorageKeys();
    localStorage.removeItem(keys.primary);
    localStorage.removeItem(keys.legacyScoped);
  } catch (error) {
    // ignore storage errors
  }
};

export const clearScheduledChatSnapshot = () => {
  if (snapshotTimer !== null) {
    clearTimeout(snapshotTimer);
    snapshotTimer = null;
  }
  pendingSnapshotContext = null;
};

export const flushScheduledChatSnapshot = (storeState, context = null) => {
  const source = resolveChatSnapshotScheduleSource(storeState, context, (sessionId) => getSessionMessages(sessionId));
  if (!source) {
    return;
  }
  const snapshot = buildChatSnapshot(source.sessionId, source.messages);
  if (snapshot) {
    writeChatSnapshot(snapshot);
  }
};

export const scheduleChatSnapshot = (storeState, immediate = false) => {
  pendingSnapshotContext = captureChatSnapshotScheduleContext(storeState);
  const flush = () => {
    if (!chatPerf.enabled()) {
      flushScheduledChatSnapshot(storeState, pendingSnapshotContext);
      return;
    }
    const start = performance.now();
    flushScheduledChatSnapshot(storeState, pendingSnapshotContext);
    chatPerf.recordDuration('chat_snapshot_flush', performance.now() - start, {
      messageCount: Array.isArray(storeState?.messages) ? storeState.messages.length : 0
    });
  };
  if (immediate) {
    if (snapshotTimer !== null) {
      clearTimeout(snapshotTimer);
      snapshotTimer = null;
    }
    flush();
    pendingSnapshotContext = null;
    return;
  }
  if (snapshotTimer !== null) return;
  snapshotTimer = setTimeout(() => {
    const scheduledContext = pendingSnapshotContext;
    pendingSnapshotContext = null;
    snapshotTimer = null;
    if (typeof requestIdleCallback === 'function') {
      requestIdleCallback(
        () => {
          if (!chatPerf.enabled()) {
            flushScheduledChatSnapshot(storeState, scheduledContext);
            return;
          }
          const start = performance.now();
          flushScheduledChatSnapshot(storeState, scheduledContext);
          chatPerf.recordDuration('chat_snapshot_flush', performance.now() - start, {
            messageCount: Array.isArray(storeState?.messages) ? storeState.messages.length : 0
          });
        },
        { timeout: SNAPSHOT_IDLE_TIMEOUT_MS }
      );
      return;
    }
    if (!chatPerf.enabled()) {
      flushScheduledChatSnapshot(storeState, scheduledContext);
      return;
    }
    const start = performance.now();
    flushScheduledChatSnapshot(storeState, scheduledContext);
    chatPerf.recordDuration('chat_snapshot_flush', performance.now() - start, {
      messageCount: Array.isArray(storeState?.messages) ? storeState.messages.length : 0
    });
  }, SNAPSHOT_FLUSH_MS);
};

export const mergeSnapshotAssistant = (target, snapshot) => {
  if (!target || target.role !== 'assistant' || !snapshot) return false;
  const targetIsCompactionMarker = isCompactionMarkerAssistantMessage(target);
  const snapshotIsCompactionMarker = isCompactionMarkerAssistantMessage(snapshot);
  if (targetIsCompactionMarker !== snapshotIsCompactionMarker) {
    return false;
  }
  const snapshotContent = String(snapshot.content || '');
  const serverContent = String(target.content || '');
  const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const snapshotRound = normalizeStreamRound(snapshot.stream_round);
  const targetRound = normalizeStreamRound(target.stream_round);
  const snapshotPlan = normalizePlanPayload(snapshot.plan);
  const hasSnapshotPlan = hasPlanSteps(snapshotPlan);
  const snapshotFeedback = normalizeMessageFeedback(snapshot.feedback);
  const snapshotStats = normalizeMessageStats(snapshot.stats);
  const snapshotSubagents = normalizeMessageSubagents(snapshot.subagents);
  const snapshotWaitingUpdatedAtMs = normalizeInteractionTimestamp(snapshot.waiting_updated_at_ms);
  const snapshotWaitingFirstOutputAtMs = normalizeInteractionTimestamp(snapshot.waiting_first_output_at_ms);
  const snapshotWaitingPhaseFirstOutputAtMs = normalizeInteractionTimestamp(
    snapshot.waiting_phase_first_output_at_ms
  );
  const snapshotRetryState = String(snapshot.retry_state || '').trim().toLowerCase();
  const snapshotRetryAttempt = parseOptionalCount(snapshot.retry_attempt);
  const snapshotRetryMaxAttempts = parseOptionalCount(snapshot.retry_max_attempts);
  const snapshotRetryDelaySeconds = Number(snapshot.retry_delay_s);
  const snapshotRetryStartedAtMs = normalizeInteractionTimestamp(snapshot.retry_started_at_ms);
  const snapshotRetryNextAttemptAtMs = normalizeInteractionTimestamp(snapshot.retry_next_attempt_at_ms);
  const snapshotRetryReason = String(snapshot.retry_reason || '').trim();
  const snapshotRetryError = String(snapshot.retry_error || '').trim();
  const snapshotManualCompactionMarker = normalizeFlag(
    snapshot.manual_compaction_marker ?? snapshot.manualCompactionMarker
  );
  const snapshotPendingManualCompaction =
    snapshotManualCompactionMarker &&
    (
      normalizeFlag(snapshot.stream_incomplete) ||
      normalizeFlag(snapshot.workflowStreaming) ||
      normalizeFlag(snapshot.reasoningStreaming)
    );
  const preserveTerminalCompactionState =
    targetIsCompactionMarker &&
    snapshotIsCompactionMarker &&
    shouldPreserveTerminalCompactionMarkerState(target, snapshot);
  const hasWorkflowItems =
    Array.isArray(snapshot.workflowItems) && snapshot.workflowItems.length > 0;
  const shouldMergeContent =
    snapshotContent.length > serverContent.length ||
    (snapshot.stream_incomplete && serverContent.length === 0);
  const shouldMergeFlags = Boolean(
    snapshot.stream_incomplete ||
      snapshot.workflowStreaming ||
      snapshot.reasoningStreaming ||
      snapshot.resume_available ||
      snapshot.slow_client ||
      snapshotWaitingUpdatedAtMs !== null ||
      snapshotWaitingFirstOutputAtMs !== null ||
      snapshotWaitingPhaseFirstOutputAtMs !== null ||
      Boolean(snapshotRetryState) ||
      snapshotRetryAttempt !== null ||
      snapshotRetryMaxAttempts !== null ||
      snapshotRetryStartedAtMs !== null ||
      snapshotRetryNextAttemptAtMs !== null ||
      Boolean(snapshotRetryReason) ||
      Boolean(snapshotRetryError) ||
      hasWorkflowItems ||
      hasSnapshotPlan ||
      snapshotRound !== null ||
      snapshotEventId !== null ||
      snapshot.questionPanel ||
      snapshotFeedback ||
      snapshotSubagents.length > 0 ||
      snapshotManualCompactionMarker
  );
  if (!shouldMergeContent && !shouldMergeFlags && !snapshotStats) {
    return false;
  }
  if (shouldMergeContent && snapshotContent) {
    target.content = snapshotContent;
  }
  if (snapshot.reasoning) {
    target.reasoning = snapshot.reasoning;
  }
  if (shouldMergeFlags) {
    target.reasoningStreaming = preserveTerminalCompactionState
      ? normalizeFlag(target.reasoningStreaming)
      : normalizeFlag(snapshot.reasoningStreaming) || normalizeFlag(target.reasoningStreaming);
    if (hasWorkflowItems) {
      const targetHasItems =
        Array.isArray(target.workflowItems) && target.workflowItems.length > 0;
      const shouldPreferSnapshotWorkflowItems =
        !preserveTerminalCompactionState &&
        snapshotPendingManualCompaction && isCompactionMarkerAssistantMessage(target);
      if (
        shouldPreferSnapshotWorkflowItems ||
        !targetHasItems ||
        snapshot.workflowItems.length >= target.workflowItems.length
      ) {
        target.workflowItems = snapshot.workflowItems;
      }
    }
    target.workflowStreaming = preserveTerminalCompactionState
      ? normalizeFlag(target.workflowStreaming)
      : normalizeFlag(snapshot.workflowStreaming) || normalizeFlag(target.workflowStreaming);
    target.stream_incomplete = preserveTerminalCompactionState
      ? normalizeFlag(target.stream_incomplete)
      : normalizeFlag(snapshot.stream_incomplete) || normalizeFlag(target.stream_incomplete);
    target.resume_available =
      normalizeFlag(snapshot.resume_available) ||
      normalizeFlag(target.resume_available);
    target.slow_client =
      normalizeFlag(snapshot.slow_client) ||
      normalizeFlag(target.slow_client);
    if (snapshotRound !== null && (targetRound === null || snapshotRound > targetRound)) {
      target.stream_round = snapshotRound;
    }
    if (snapshotEventId !== null && (targetEventId === null || snapshotEventId > targetEventId)) {
      target.stream_event_id = snapshotEventId;
    }
    const panel = normalizeInquiryPanelState(snapshot.questionPanel);
    if (panel) {
      target.questionPanel = panel;
    }
    if (snapshotPlan) {
      target.plan = snapshotPlan;
      target.planVisible =
        Boolean(target.planVisible) || shouldAutoShowPlan(snapshotPlan, snapshot);
    }
    if (snapshotFeedback) {
      target.feedback = snapshotFeedback;
    }
    if (snapshotSubagents.length > 0) {
      target.subagents = snapshotSubagents;
    }
    if (snapshotManualCompactionMarker) {
      target.manual_compaction_marker = true;
    }
    if (
      snapshotWaitingUpdatedAtMs !== null &&
      snapshotWaitingUpdatedAtMs >= (normalizeInteractionTimestamp(target.waiting_updated_at_ms) ?? 0)
    ) {
      target.waiting_updated_at_ms = snapshotWaitingUpdatedAtMs;
    }
    if (
      snapshotWaitingFirstOutputAtMs !== null &&
      (
        normalizeInteractionTimestamp(target.waiting_first_output_at_ms) === null ||
        snapshotWaitingFirstOutputAtMs <=
          (
            normalizeInteractionTimestamp(target.waiting_first_output_at_ms)
            ?? snapshotWaitingFirstOutputAtMs
          )
      )
    ) {
      target.waiting_first_output_at_ms = snapshotWaitingFirstOutputAtMs;
    }
    if (
      snapshotWaitingPhaseFirstOutputAtMs !== null &&
      snapshotWaitingPhaseFirstOutputAtMs >=
        (normalizeInteractionTimestamp(target.waiting_phase_first_output_at_ms) ?? 0)
    ) {
      target.waiting_phase_first_output_at_ms = snapshotWaitingPhaseFirstOutputAtMs;
    }
    if (snapshotRetryState) {
      target.retry_state = snapshotRetryState;
    } else if (
      !normalizeFlag(snapshot.stream_incomplete) &&
      !normalizeFlag(snapshot.workflowStreaming) &&
      !normalizeFlag(snapshot.reasoningStreaming)
    ) {
      clearAssistantRetryState(target);
    }
    if (snapshotRetryAttempt !== null) {
      target.retry_attempt = snapshotRetryAttempt;
    }
    if (snapshotRetryMaxAttempts !== null) {
      target.retry_max_attempts = snapshotRetryMaxAttempts;
    }
    if (Number.isFinite(snapshotRetryDelaySeconds) && snapshotRetryDelaySeconds > 0) {
      target.retry_delay_s = snapshotRetryDelaySeconds;
    }
    if (
      snapshotRetryStartedAtMs !== null &&
      snapshotRetryStartedAtMs >= (normalizeInteractionTimestamp(target.retry_started_at_ms) ?? 0)
    ) {
      target.retry_started_at_ms = snapshotRetryStartedAtMs;
    }
    if (
      snapshotRetryNextAttemptAtMs !== null &&
      snapshotRetryNextAttemptAtMs >= (normalizeInteractionTimestamp(target.retry_next_attempt_at_ms) ?? 0)
    ) {
      target.retry_next_attempt_at_ms = snapshotRetryNextAttemptAtMs;
    }
    if (snapshotRetryReason) {
      target.retry_reason = snapshotRetryReason;
    }
    if (snapshotRetryError) {
      target.retry_error = snapshotRetryError;
    }
  }
  if (snapshotStats) {
    target.stats = mergeMessageStats(target.stats, snapshotStats);
  }
  return true;
};

export const findSnapshotAssistantIndex = (target, targetEntry, snapshotAssistants, cursor) => {
  if (!target || cursor < 0) return -1;
  const targetIsCompactionMarker = isCompactionMarkerAssistantMessage(target);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const targetRound = normalizeStreamRound(target.stream_round);
  const targetTime = resolveTimestampMs(target.created_at);
  const targetContent = normalizeAssistantContent(target.content || '');
  let bestIndex = -1;
  let bestDelta = Infinity;
  for (let i = cursor; i >= 0; i -= 1) {
    const snapshotEntry = snapshotAssistants[i];
    const snapshot = snapshotEntry?.message;
    if (!snapshot) continue;
    if (!assistantEntriesShareTurnAnchor(targetEntry, snapshotEntry)) {
      continue;
    }
    if (isCompactionMarkerAssistantMessage(snapshot) !== targetIsCompactionMarker) {
      continue;
    }
    const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
    if (targetEventId !== null && snapshotEventId !== null && snapshotEventId === targetEventId) {
      return i;
    }
    const snapshotRound = normalizeStreamRound(snapshot.stream_round);
    if (targetRound !== null && snapshotRound !== null && snapshotRound === targetRound) {
      return i;
    }
    const snapshotTime = resolveTimestampMs(snapshot.created_at);
    if (Number.isFinite(targetTime) && Number.isFinite(snapshotTime)) {
      const delta = Math.abs(targetTime - snapshotTime);
      if (delta <= SNAPSHOT_MATCH_WINDOW_MS && delta < bestDelta) {
        bestDelta = delta;
        bestIndex = i;
        if (delta === 0) break;
      }
      continue;
    }
  }
  if (bestIndex >= 0) return bestIndex;
  if (!targetIsCompactionMarker && targetContent) {
    const contentMatchIndex = findAnchoredAssistantContentMatchIndex(
      targetEntry,
      targetContent,
      snapshotAssistants,
      { maxIndex: cursor }
    );
    if (contentMatchIndex >= 0) {
      return contentMatchIndex;
    }
  }
  const isPendingTarget =
    isPendingAssistantMessage(target) || !normalizeAssistantContent(target.content || '');
  if (isPendingTarget || !Number.isFinite(targetTime)) {
    const fallbackEntry = snapshotAssistants[cursor];
    const fallback = fallbackEntry?.message;
    if (!fallback) return -1;
    if (!assistantEntriesShareTurnAnchor(targetEntry, fallbackEntry)) {
      return -1;
    }
    if (isCompactionMarkerAssistantMessage(fallback) !== targetIsCompactionMarker) {
      return -1;
    }
    const fallbackEventId = normalizeStreamEventId(fallback.stream_event_id);
    const fallbackRound = normalizeStreamRound(fallback.stream_round);
    const fallbackPending = isPendingAssistantMessage(fallback);
    if (targetEventId !== null && fallbackEventId !== null && targetEventId === fallbackEventId) {
      return cursor;
    }
    if (targetRound !== null && fallbackRound !== null && targetRound === fallbackRound) {
      return cursor;
    }
    if (
      fallbackPending &&
      (targetRound === null || fallbackRound === null || targetRound === fallbackRound)
    ) {
      return cursor;
    }
    return -1;
  }
  return -1;
};

// Like findSnapshotAssistantIndex but skips indices present in the excluded set,
// preventing the cursor-exhaustion bug where a single early match prevents all
// subsequent matches from succeeding.
export const findSnapshotAssistantIndexExcluding = (
  target,
  targetEntry,
  snapshotAssistants,
  excludedIndices
) => {
  if (!target || !Array.isArray(snapshotAssistants) || snapshotAssistants.length === 0) return -1;
  const targetIsCompactionMarker = isCompactionMarkerAssistantMessage(target);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const targetRound = normalizeStreamRound(target.stream_round);
  const targetTime = resolveTimestampMs(target.created_at);
  const targetContent = normalizeAssistantContent(target.content || '');
  let bestIndex = -1;
  let bestDelta = Infinity;
  // Search backward from the tail, same as findSnapshotAssistantIndex,
  // but skip indices already claimed by a prior match.
  for (let i = snapshotAssistants.length - 1; i >= 0; i -= 1) {
    if (excludedIndices.has(i)) continue;
    const snapshotEntry = snapshotAssistants[i];
    const snapshot = snapshotEntry?.message;
    if (!snapshot) continue;
    if (!assistantEntriesShareTurnAnchor(targetEntry, snapshotEntry)) {
      continue;
    }
    if (isCompactionMarkerAssistantMessage(snapshot) !== targetIsCompactionMarker) {
      continue;
    }
    const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
    if (targetEventId !== null && snapshotEventId !== null && snapshotEventId === targetEventId) {
      return i;
    }
    const snapshotRound = normalizeStreamRound(snapshot.stream_round);
    if (targetRound !== null && snapshotRound !== null && snapshotRound === targetRound) {
      return i;
    }
    const snapshotTime = resolveTimestampMs(snapshot.created_at);
    if (Number.isFinite(targetTime) && Number.isFinite(snapshotTime)) {
      const delta = Math.abs(targetTime - snapshotTime);
      if (delta <= SNAPSHOT_MATCH_WINDOW_MS && delta < bestDelta) {
        bestDelta = delta;
        bestIndex = i;
        if (delta === 0) break;
      }
      continue;
    }
  }
  if (bestIndex >= 0) return bestIndex;
  if (!targetIsCompactionMarker && targetContent) {
    const contentMatchIndex = findAnchoredAssistantContentMatchIndex(
      targetEntry,
      targetContent,
      snapshotAssistants,
      { excludedIndices }
    );
    if (contentMatchIndex >= 0) {
      return contentMatchIndex;
    }
  }
  // Fallback: for pending / incomplete targets, try the last unmatched position
  const isPendingTarget =
    isPendingAssistantMessage(target) || !normalizeAssistantContent(target.content || '');
  if (isPendingTarget || !Number.isFinite(targetTime)) {
    for (let i = snapshotAssistants.length - 1; i >= 0; i -= 1) {
      if (excludedIndices.has(i)) continue;
      const snapshotEntry = snapshotAssistants[i];
      const snapshot = snapshotEntry?.message;
      if (!snapshot) continue;
      if (!assistantEntriesShareTurnAnchor(targetEntry, snapshotEntry)) {
        continue;
      }
      if (isCompactionMarkerAssistantMessage(snapshot) !== targetIsCompactionMarker) continue;
      const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
      const snapshotRound = normalizeStreamRound(snapshot.stream_round);
      const snapshotPending = isPendingAssistantMessage(snapshot);
      if (targetEventId !== null && snapshotEventId !== null && targetEventId === snapshotEventId) {
        return i;
      }
      if (targetRound !== null && snapshotRound !== null && targetRound === snapshotRound) {
        return i;
      }
      if (
        snapshotPending &&
        (targetRound === null || snapshotRound === null || targetRound === snapshotRound)
      ) {
        return i;
      }
    }
    return -1;
  }
  return -1;
};

// Find the best insertion index for an unmatched live assistant message
// by looking at the position of the previous live assistant in the merged list.
export const findLiveAssistantInsertionIndex = (liveAssistant, mergedMessages) => {
  const liveCreatedTime = resolveTimestampMs(liveAssistant?.created_at);
  let bestIndex = -1;
  for (let i = mergedMessages.length - 1; i >= 0; i -= 1) {
    const msg = mergedMessages[i];
    if (!msg || msg.role !== 'assistant' || msg.isGreeting) continue;
    const msgTime = resolveTimestampMs(msg.created_at);
    if (Number.isFinite(liveCreatedTime) && Number.isFinite(msgTime) && msgTime <= liveCreatedTime) {
      bestIndex = i;
      break;
    }
  }
  return bestIndex;
};

export const mergeSnapshotIntoMessages = (messages, snapshot) => {
  if (!snapshot || !Array.isArray(snapshot.messages) || snapshot.messages.length === 0) {
    return messages;
  }
  if (!Array.isArray(messages) || messages.length === 0) {
    return snapshot.messages.map((item) => normalizeSnapshotMessage(item)).filter(Boolean);
  }
  const snapshotMessages = snapshot.messages
    .map((item) => normalizeSnapshotMessage(item))
    .filter(Boolean);
  if (!snapshotMessages.length) {
    return messages;
  }
  const snapshotAssistants = buildAssistantMatchEntries(snapshotMessages);
  if (!snapshotAssistants.length) return messages;
  const messageAssistantEntryMap = buildAssistantMatchEntryMap(messages);
  let cursor = snapshotAssistants.length - 1;
  for (let i = messages.length - 1; i >= 0 && cursor >= 0; i -= 1) {
    const target = messages[i];
    if (target?.role !== 'assistant') {
      continue;
    }
    const matchIndex = findSnapshotAssistantIndex(
      target,
      messageAssistantEntryMap.get(target),
      snapshotAssistants,
      cursor
    );
    if (matchIndex < 0) {
      continue;
    }
    mergeSnapshotAssistant(target, snapshotAssistants[matchIndex].message);
    cursor = matchIndex - 1;
  }
  return messages;
};

// 演示模式聊天缓存结构（仅用于本地暂存）
