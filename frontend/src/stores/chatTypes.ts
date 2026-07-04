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

export type SnapshotAssistantMessage = {
  role: string;
  content: string;
  created_at: string;
  reasoning?: string;
  reasoningStreaming?: boolean;
  workflowStreaming?: boolean;
  stream_incomplete?: boolean;
  slow_client?: boolean;
  resume_available?: boolean;
  stream_event_id?: number;
  stream_round?: number;
  waiting_updated_at_ms?: number | null;
  waiting_first_output_at_ms?: number | null;
  waiting_phase_first_output_at_ms?: number | null;
  retry_state?: string;
  retry_attempt?: number | null;
  retry_max_attempts?: number | null;
  retry_delay_s?: number | null;
  retry_started_at_ms?: number | null;
  retry_next_attempt_at_ms?: number | null;
  retry_reason?: string;
  retry_error?: string;
  workflowItems?: unknown[];
  plan?: unknown;
  questionPanel?: unknown;
  feedback?: unknown;
  stats?: unknown;
  planVisible?: boolean;
  isGreeting?: boolean;
  attachments?: unknown[];
  subagents?: unknown[];
  hiddenInternal?: boolean;
  manual_compaction_marker?: boolean;
  manual_goal_marker?: boolean;
  realtime_protected?: boolean;
};

export type DemoChatCachePatch = {
  sessions?: unknown[];
  sessionId?: string | number | null;
  messages?: unknown[];
};

export type GreetingMessageOptions = {
  greeting?: unknown;
  createdAt?: unknown;
  sessionCreatedAt?: unknown;
};

export type CreateSessionOptions = {
  preserveCurrentMessages?: boolean;
};

export type SessionEventsSnapshotCacheEntry = {
  cachedAt: number;
  limit: number | null;
  running: boolean;
  lastEventId: number | null;
  payload: Record<string, unknown> | null;
};

export type SessionDetailSnapshotCacheEntry = {
  cachedAt: number;
  payload: Record<string, unknown> | null;
};

export type SessionWorkflowStateOptions = {
  reset?: boolean;
};

export type ThreadControlSession = Record<string, unknown> & {
  id: string;
  status?: unknown;
  agent_id?: unknown;
};

export type NormalizedUsagePayload = {
  input: number;
  output: number;
  total: number;
};

export type InquiryPanelPatch = {
  status?: unknown;
  selected?: unknown[];
};

export type MessageSubagentItem = {
  key: string;
  session_id: string;
  run_id: string;
  dispatch_id: string;
  title: string;
  label: string;
  status: string;
  summary: string;
  terminal: boolean;
  failed: boolean;
  canTerminate: boolean;
  updated_at: string;
  updated_at_ms: number | null;
  parent_user_round: number | null;
  parent_model_round: number | null;
  detail: Record<string, unknown>;
  agent_state: {
    status: string;
    message: string;
  };
};

export type DesktopOverlayBridge = {
  showControllerHint?: (payload: {
    x: number;
    y: number;
    description?: string;
    durationMs?: number;
  }) => Promise<boolean> | boolean;
  showControllerDone?: (payload: {
    x: number;
    y: number;
    description?: string;
    durationMs?: number;
  }) => Promise<boolean> | boolean;
  showMonitorCountdown?: (payload: { waitMs: number }) => Promise<boolean> | boolean;
  hideOverlay?: () => Promise<boolean> | boolean;
};

export type LoadSessionsOptions = {
  agent_id?: string | number | boolean | null | undefined;
  traceId?: string;
  traceSource?: string;
  force?: boolean;
  preferCache?: boolean;
  backgroundRefresh?: boolean;
  maxCacheAgeMs?: number;
};

export type ListSessionsByStatusOptions = {
  agent_id?: string | number | boolean | null | undefined;
  status?: 'active' | 'archived' | 'all' | string;
};

export type OpenDraftSessionOptions = {
  agent_id?: string | number | boolean | null | undefined;
};

export type LoadSessionDetailOptions = {
  preserveWatcher?: boolean;
  forceHydrateForeground?: boolean;
  startWatcherAfterHydration?: boolean;
};

export type SendMessageOptions = {
  attachments?: unknown[];
  suppressQueuedNotice?: boolean;
  approvalMode?: string;
  approval_mode?: string;
};

export type AppendLocalMessageOptions = {
  createdAt?: unknown;
  sessionId?: unknown;
  immediate?: boolean;
  manualGoalMarker?: boolean;
  localTurnId?: string;
  localModelTurnId?: string;
};

export type ResumeStreamOptions = {
  force?: boolean;
  afterEventId?: number | string;
};

export type ApprovalDecision = 'approve_once' | 'approve_session' | 'deny';

export type PendingApproval = {
  approval_id: string;
  request_id: string;
  session_id: string;
  tool: string;
  kind: string;
  summary: string;
  detail: unknown;
  args: unknown;
  created_at: string;
};

export type SessionOrchestrationLock = {
  active: boolean;
  group_id: string;
  orchestration_id: string;
  run_id: string;
  mother_agent_id: string;
  role: string;
};

export type SessionGoal = {
  goal_id: string;
  session_id: string;
  user_id: string;
  objective: string;
  status: string;
  token_budget: number | null;
  tokens_used: number;
  time_used_seconds: number;
  created_at: number | null;
  updated_at: number | null;
  completed_at: number | null;
  last_continued_at: number | null;
  source: string;
};
