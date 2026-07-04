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
  isSupersededRunningManualCompactionMarker,
  mergeCompactionMarkersIntoMessages
} from './chatCompactionMarker';
import { useCommandSessionStore } from './commandSessions';
import { hasRetainedMessageConversationContext as hasRetainedConversationContext } from '@/views/messenger/messageConversationRetention';

import { normalizeInquiryPanelState, normalizePlanPayload, shouldAutoShowPlan } from './chatDemoPanels';
import { resolveChatSnapshotStorageKeys } from './chatPersist';
import { getSessionMessages } from './chatRuntimeState';
import { normalizeHiddenInternalMessage, normalizeInteractionTimestamp, normalizeMessageStats, normalizeMessageSubagents, parseOptionalCount } from './chatStats';
import { normalizeFlag, normalizeStreamEventId, normalizeStreamRound } from './chatStreamIds';
import { SnapshotAssistantMessage } from './chatTypes';
import { normalizeAssistantOutput, resolveAssistantReasoning } from './chatWorkflowHydration';

export const SNAPSHOT_FLUSH_MS = 800;
export const SNAPSHOT_IDLE_TIMEOUT_MS = 2000;
export const SNAPSHOT_MESSAGE_LIMIT = 200;
export const MAX_SNAPSHOT_MESSAGES = 50;
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

// 演示模式聊天缓存结构（仅用于本地暂存）
