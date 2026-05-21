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
  hasRunningAssistantMessage,
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
  selectSessionBusyReason
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

import { isGoalActiveForLock } from './chatPersist';
import { getHistoryState, getRuntime, getSessionMessages, hasRuntimeControllers, resolveSessionKey } from './chatRuntimeState';
import { PendingApproval, SessionGoal } from './chatTypes';
import { resolveLegacyMessageRuntimeStatusFromStore, resolveProjectedVisibleMessagesFromStore } from './chatWatcher';
import {
  resolveMergedSessionBusy,
  resolveMergedSessionRuntimeStatus
} from './chatBusyState';

import { chatApprovalActions } from './chatApprovalActions';
import { chatCacheActions } from './chatCacheActions';
import { chatSubagentFeedbackActions } from './chatSubagentFeedbackActions';
import { chatSessionOpenLoadActions } from './chatSessionOpenLoadActions';
import { chatSessionMutationActions } from './chatSessionMutationActions';
import { chatCompactionActions } from './chatCompactionActions';
import { chatSendActions } from './chatSendActions';
import { chatStopResumeActions } from './chatStopResumeActions';
import { chatRealtimeRecoveryActions } from './chatRealtimeRecoveryActions';

export const useChatStore = defineStore('chat', {
  state: () => ({
    sessions: [],
    activeSessionId: null,
    messages: [],
    messageMutationVersion: 0,
    runtimeProjectionVersion: 0,
    sessionsLoadedAt: 0,
    loadingBySession: {},
    sessionGoals: {} as Record<string, SessionGoal>,
    runtimeProjection: createChatRuntimeProjection() as ChatRuntimeProjection,
    greetingOverride: '',
    draftAgentId: '',
    draftToolOverrides: null,
    pendingApprovals: [] as PendingApproval[]
  }),
  getters: {
    isSessionLoading: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      return Boolean(state.loadingBySession[key]);
    },
    isSessionBusy: (state) => (sessionId) => {
      const _projectionVersion = state.runtimeProjectionVersion;
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      const activeKey = resolveSessionKey(state.activeSessionId);
      const messages = activeKey === key ? state.messages : getSessionMessages(key);
      const runtime = getRuntime(key);
      return resolveMergedSessionBusy({
        projection: state.runtimeProjection,
        sessionId: key,
        loading: state.loadingBySession[key],
        messages,
        runtimeStatus: runtime?.threadStatus,
        runtimeKnown: Boolean(runtime),
        runtimeHasControllers: hasRuntimeControllers(runtime)
      });
    },
    sessionRuntimeStatus: (state) => (sessionId) => {
      const _projectionVersion = state.runtimeProjectionVersion;
      const key = resolveSessionKey(sessionId);
      const activeKey = resolveSessionKey(state.activeSessionId);
      const messages = activeKey === key ? state.messages : getSessionMessages(key);
      const runtime = getRuntime(key);
      return resolveMergedSessionRuntimeStatus({
        projection: state.runtimeProjection,
        sessionId: key,
        loading: state.loadingBySession[key],
        messages,
        runtimeStatus: runtime?.threadStatus,
        runtimeKnown: Boolean(runtime),
        runtimeHasControllers: hasRuntimeControllers(runtime)
      });
    },
    sessionBusyReason: (state) => (sessionId) => {
      const _projectionVersion = state.runtimeProjectionVersion;
      const key = resolveSessionKey(sessionId);
      if (!key) return null;
      return selectSessionBusyReason(state.runtimeProjection, key);
    },
    sessionGoal: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return null;
      return state.sessionGoals[key] || null;
    },
    isSessionGoalLocked: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      return isGoalActiveForLock(state.sessionGoals[key]);
    },
    messageRuntimeStatus: (state) => (sessionId, message) => {
      const _projectionVersion = state.runtimeProjectionVersion;
      return resolveLegacyMessageRuntimeStatusFromStore(state, sessionId, message);
    },
    visibleMessages: (state) => (sessionId = null) => {
      const _projectionVersion = state.runtimeProjectionVersion;
      return resolveProjectedVisibleMessagesFromStore(state, sessionId || state.activeSessionId);
    },
    historyLoading: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return Boolean(state?.loading);
    },
    canLoadMoreHistory: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return Boolean(state?.hasMore) && !state?.loading;
    },
    historyBeforeId: () => (sessionId) => {
      const state = getHistoryState(sessionId);
      return state?.beforeId ?? null;
    },
    activeApproval: (state) => (Array.isArray(state.pendingApprovals) ? state.pendingApprovals[0] : null)
  },
  actions: {
    ...chatApprovalActions,
    ...chatCacheActions,
    ...chatSubagentFeedbackActions,
    ...chatSessionOpenLoadActions,
    ...chatSessionMutationActions,
    ...chatCompactionActions,
    ...chatSendActions,
    ...chatStopResumeActions,
    ...chatRealtimeRecoveryActions
  }
});
