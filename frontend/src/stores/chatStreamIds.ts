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

export const normalizeStreamEventId = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const getRuntimeLastEventId = (runtime) => {
  const normalized = normalizeStreamEventId(runtime?.lastEventId);
  return normalized === null ? 0 : normalized;
};

export const updateRuntimeLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(runtime.lastEventId);
  if (current === null || normalized > current) {
    runtime.lastEventId = normalized;
  }
};

export const setRuntimeLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  runtime.lastEventId = normalized;
};

export const updateRuntimeRemoteLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(runtime.remoteLastEventId);
  if (current === null || normalized > current) {
    runtime.remoteLastEventId = normalized;
  }
};

export const normalizeStreamRound = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

export const readDeltaSegments = (value) => {
  if (!value || typeof value !== 'object') return [];
  if (Array.isArray(value.segments)) {
    return value.segments;
  }
  if (value.data && typeof value.data === 'object' && Array.isArray(value.data.segments)) {
    return value.data.segments;
  }
  if (
    value.data &&
    typeof value.data === 'object' &&
    value.data.data &&
    typeof value.data.data === 'object' &&
    Array.isArray(value.data.data.segments)
  ) {
    return value.data.data.segments;
  }
  return [];
};

const asObjectRecord = (value) =>
  value && typeof value === 'object' && !Array.isArray(value) ? value : null;

const collectEventPayloadSources = (payload, data) => {
  const sources = [];
  const push = (value) => {
    const record = asObjectRecord(value);
    if (!record) return;
    const inner = asObjectRecord(record.data);
    if (inner && inner !== record) {
      push(inner);
    }
    if (!sources.includes(record)) {
      sources.push(record);
    }
  };
  push(data);
  push(payload);
  return sources;
};

const resolveFirstPositiveRound = (source, keys) => {
  const record = asObjectRecord(source);
  if (!record) return null;
  for (const key of keys) {
    const round = normalizeStreamRound(record[key]);
    if (round !== null) return round;
  }
  return null;
};

const hasExplicitUserRound = (source) =>
  resolveFirstPositiveRound(source, ['user_round', 'userRound', 'user_turn_index', 'userTurnIndex']) !== null;

const hasExplicitModelRound = (source) =>
  resolveFirstPositiveRound(source, ['model_round', 'modelRound', 'model_turn_index', 'modelTurnIndex']) !== null;

export const parseSegmentedDelta = (payload, data) => {
  const candidates = [data, payload];
  for (const source of candidates) {
    const segments = readDeltaSegments(source);
    if (!segments.length) continue;
    let delta = '';
    let reasoningDelta = '';
    let userRound = null;
    let modelRound = null;
    let genericRound = null;
    segments.forEach((segment) => {
      if (!segment || typeof segment !== 'object') return;
      if (typeof segment.delta === 'string' && segment.delta) {
        delta += segment.delta;
      }
      if (typeof segment.reasoning_delta === 'string' && segment.reasoning_delta) {
        reasoningDelta += segment.reasoning_delta;
      }
      if (typeof segment.think_delta === 'string' && segment.think_delta) {
        reasoningDelta += segment.think_delta;
      }
      const segmentUserRound = resolveFirstPositiveRound(segment, ['user_round', 'userRound']);
      if (segmentUserRound !== null) {
        userRound = segmentUserRound;
      }
      const segmentModelRound = resolveFirstPositiveRound(segment, ['model_round', 'modelRound']);
      if (segmentModelRound !== null) {
        modelRound = segmentModelRound;
      }
      const segmentGenericRound = normalizeStreamRound(segment.round);
      if (segmentGenericRound !== null) {
        genericRound = segmentGenericRound;
      }
    });
    return {
      delta,
      reasoningDelta,
      userRound,
      modelRound,
      round: userRound ?? genericRound ?? modelRound
    };
  }
  return null;
};

export const resolveEventUserRoundNumber = (payload, data) => {
  const sources = collectEventPayloadSources(payload, data);
  for (const source of sources) {
    const directRound = resolveFirstPositiveRound(source, [
      'user_round',
      'userRound',
      'user_turn_index',
      'userTurnIndex'
    ]);
    if (directRound !== null) {
      return directRound;
    }
  }
  for (const source of sources) {
    const segments = readDeltaSegments(source);
    for (const segment of segments) {
      const segmentRound = resolveFirstPositiveRound(segment, ['user_round', 'userRound']);
      if (segmentRound !== null) {
        return segmentRound;
      }
    }
  }
  for (const source of sources) {
    if (hasExplicitModelRound(source)) continue;
    const fallbackRound = normalizeStreamRound(source.round);
    if (fallbackRound !== null) {
      return fallbackRound;
    }
  }
  return null;
};

export const resolveEventModelRoundNumber = (payload, data) => {
  const sources = collectEventPayloadSources(payload, data);
  for (const source of sources) {
    const directRound = resolveFirstPositiveRound(source, [
      'model_round',
      'modelRound',
      'model_turn_index',
      'modelTurnIndex'
    ]);
    if (directRound !== null) {
      return directRound;
    }
  }
  for (const source of sources) {
    const segments = readDeltaSegments(source);
    for (const segment of segments) {
      const segmentRound = resolveFirstPositiveRound(segment, ['model_round', 'modelRound']);
      if (segmentRound !== null) {
        return segmentRound;
      }
    }
  }
  for (const source of sources) {
    if (hasExplicitUserRound(source)) continue;
    const fallbackRound = normalizeStreamRound(source.round);
    if (fallbackRound !== null) {
      return fallbackRound;
    }
  }
  return null;
};

export const resolveEventRoundNumber = resolveEventUserRoundNumber;

export const assignStreamEventId = (message, eventId) => {
  if (!message || typeof message !== 'object') return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(message.stream_event_id);
  if (current === null || normalized > current) {
    message.stream_event_id = normalized;
  }
};

export const normalizeFlag = (value) => value === true || value === 'true';
export const normalizeApprovalMode = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return '';
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return '';
};
