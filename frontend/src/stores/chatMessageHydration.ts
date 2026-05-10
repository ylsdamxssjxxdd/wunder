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

import { normalizeInquiryPanelState, normalizePlanPayload, shouldAutoShowPlan } from './chatDemoPanels';
import { STREAM_FLUSH_BASE_MS } from './chatRuntimeControls';
import { buildMessageStats, normalizeHiddenInternalMessage, normalizeInteractionTimestamp, normalizeMessageStats, normalizeMessageSubagents, parseOptionalCount } from './chatStats';
import { normalizeFlag } from './chatStreamIds';
import { normalizeAssistantOutput, resolveAssistantReasoning } from './chatWorkflowHydration';
import { createWorkflowProcessor } from './chatWorkflowProcessor';

export const hydrateMessage = (message, workflowState) => {
  if (!message || message.role !== 'assistant') {
    return message;
  }
  const normalizedOutput = normalizeAssistantOutput(
    message.content,
    resolveAssistantReasoning(message)
  );
  const hydrated = {
    ...message,
    content: normalizedOutput.content,
    workflowItems: [],
    workflowStreaming: normalizeFlag(message?.workflowStreaming),
    stream_incomplete: normalizeFlag(message?.stream_incomplete),
    resume_available: normalizeFlag(message?.resume_available),
    slow_client: normalizeFlag(message?.slow_client),
    reasoning: normalizedOutput.reasoning,
    reasoningStreaming: normalizeFlag(message?.reasoningStreaming),
    waiting_updated_at_ms: normalizeInteractionTimestamp(
      message?.waiting_updated_at_ms ?? message?.waitingUpdatedAtMs
    ),
    waiting_first_output_at_ms: normalizeInteractionTimestamp(
      message?.waiting_first_output_at_ms ?? message?.waitingFirstOutputAtMs
    ),
    waiting_phase_first_output_at_ms: normalizeInteractionTimestamp(
      message?.waiting_phase_first_output_at_ms ?? message?.waitingPhaseFirstOutputAtMs
    ),
    retry_state: String(message?.retry_state ?? message?.retryState ?? '').trim().toLowerCase(),
    retry_attempt: parseOptionalCount(message?.retry_attempt ?? message?.retryAttempt),
    retry_max_attempts: parseOptionalCount(
      message?.retry_max_attempts ?? message?.retryMaxAttempts
    ),
    retry_delay_s: (() => {
      const retryDelaySeconds = Number(message?.retry_delay_s ?? message?.retryDelayS);
      return Number.isFinite(retryDelaySeconds) && retryDelaySeconds > 0 ? retryDelaySeconds : null;
    })(),
    retry_started_at_ms: normalizeInteractionTimestamp(
      message?.retry_started_at_ms ?? message?.retryStartedAtMs
    ),
    retry_next_attempt_at_ms: normalizeInteractionTimestamp(
      message?.retry_next_attempt_at_ms ?? message?.retryNextAttemptAtMs
    ),
    retry_reason: String(message?.retry_reason ?? message?.retryReason ?? '').trim(),
    retry_error: String(message?.retry_error ?? message?.retryError ?? '').trim(),
    hiddenInternal: normalizeHiddenInternalMessage(message?.hiddenInternal),
    manual_compaction_marker: normalizeFlag(
      message?.manual_compaction_marker ?? message?.manualCompactionMarker
    ),
    subagents: normalizeMessageSubagents(message?.subagents),
    feedback: normalizeMessageFeedback(message?.feedback),
    stats: normalizeMessageStats(message.stats) || buildMessageStats()
  };
  const plan = normalizePlanPayload(message.plan);
  hydrated.plan = plan;
  hydrated.planVisible = shouldAutoShowPlan(plan, message);
  hydrated.questionPanel = normalizeInquiryPanelState(message.questionPanel);
  if (Array.isArray(message.workflow_events) && message.workflow_events.length > 0) {
    const processor = createWorkflowProcessor(
      hydrated,
      workflowState,
      null,
      {
        finalizeWithNow: false,
        streamFlushMs: STREAM_FLUSH_BASE_MS,
        sessionId: message?.session_id ?? message?.sessionId ?? null
      }
    );
    message.workflow_events.forEach((event) => {
      processor.handleEvent(event?.event || '', event?.raw || '');
    });
    processor.finalize();
  }
  return hydrated;
};
