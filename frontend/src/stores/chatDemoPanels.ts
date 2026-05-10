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

import { patchSessionRuntimeFields } from './chatPersist';
import { resolveSessionKey } from './chatRuntimeState';
import { buildMessage, resolveGreetingContent, resolveTimestampIso, resolveTimestampMs } from './chatStats';
import { normalizeFlag } from './chatStreamIds';
import { DemoChatCachePatch, GreetingMessageOptions, PendingApproval } from './chatTypes';
import { pickString } from './chatWorkflowHydration';

export const buildDemoChatState = () => ({
  sessions: [],
  messages: {}
});

export const normalizeDemoChatState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildDemoChatState();
  }
  return {
    sessions: Array.isArray(value.sessions) ? value.sessions : [],
    messages: value.messages && typeof value.messages === 'object' ? value.messages : {}
  };
};

export const getDemoChatState = () => normalizeDemoChatState(loadDemoChatState());

export const persistDemoChatState = (state) => saveDemoChatState(state);

export const syncDemoChatCache = ({ sessions, sessionId, messages }: DemoChatCachePatch = {}) => {
  if (!isDemoMode()) return;
  const state = getDemoChatState();
  if (Array.isArray(sessions)) {
    state.sessions = sessions;
  }
  if (sessionId) {
    state.messages = state.messages || {};
    state.messages[sessionId] = Array.isArray(messages) ? messages : [];
  }
  persistDemoChatState(state);
};

export const removeDemoChatSession = (sessionId) => {
  if (!isDemoMode() || !sessionId) return;
  const state = getDemoChatState();
  state.sessions = (state.sessions || []).filter((item) => item.id !== sessionId);
  if (state.messages?.[sessionId]) {
    delete state.messages[sessionId];
  }
  persistDemoChatState(state);
};

export const resolveSessionActivityTime = (session) =>
  resolveTimestampMs(
    session?.updated_at ?? session?.last_message_at ?? session?.created_at
  );

export const sortSessionsByActivity = (sessions = []) =>
  (Array.isArray(sessions) ? sessions.slice() : [])
    .map((session, index) => ({ session: patchSessionRuntimeFields(session), index }))
    .sort((a, b) => {
      const aTime = resolveSessionActivityTime(a.session);
      const bTime = resolveSessionActivityTime(b.session);
      if (aTime !== null && bTime !== null && aTime !== bTime) {
        return bTime - aTime;
      }
      if (aTime !== null && bTime === null) return -1;
      if (aTime === null && bTime !== null) return 1;
      return a.index - b.index;
    })
    .map((item) => item.session);

export const buildGreetingMessage = (createdAt, greeting) => ({
  ...buildMessage('assistant', resolveGreetingContent(greeting), createdAt),
  workflowItems: [],
  workflowStreaming: false,
  isGreeting: true
});

export const resolveGreetingTimestamp = (messages, createdAt) => {
  const direct = resolveTimestampIso(createdAt);
  if (direct) return direct;
  const safeMessages = Array.isArray(messages) ? messages : [];
  const candidate = safeMessages.find((message) => message?.created_at)?.created_at;
  return resolveTimestampIso(candidate);
};

export const ensureGreetingMessage = (messages, options: GreetingMessageOptions = {}) => {
  const safeMessages = Array.isArray(messages) ? messages : [];
  const greetingText = resolveGreetingContent(options?.greeting);
  // 无论历史会话与否，都补一条问候语，保证提示词预览入口稳定可见
  const greetingIndex = safeMessages.findIndex((message) => message?.isGreeting);
  if (greetingIndex >= 0) {
    if (safeMessages[greetingIndex]?.content !== greetingText) {
      safeMessages[greetingIndex].content = greetingText;
    }
    const createdAt = options?.createdAt ?? options?.sessionCreatedAt;
    if (createdAt) {
      const greetingAt = resolveGreetingTimestamp(safeMessages, createdAt);
      if (greetingAt) {
        const currentAt = resolveTimestampIso(safeMessages[greetingIndex]?.created_at);
        if (currentAt !== greetingAt) {
          safeMessages[greetingIndex].created_at = greetingAt;
        }
      }
    }
    return safeMessages;
  }
  const greetingAt = resolveGreetingTimestamp(safeMessages, options?.createdAt ?? options?.sessionCreatedAt);
  return [buildGreetingMessage(greetingAt, greetingText), ...safeMessages];
};

export const safeJsonParse = (raw) => {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (error) {
    return null;
  }
};

export const normalizeApprovalKind = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'exec' || raw === 'patch') {
    return raw;
  }
  return '';
};

export const normalizePendingApproval = (payload, requestId, sessionId): PendingApproval | null => {
  if (!payload || typeof payload !== 'object') {
    return null;
  }
  const approvalId = String(payload.approval_id ?? payload.id ?? '').trim();
  if (!approvalId) {
    return null;
  }
  const summary = String(payload.summary || '').trim();
  const tool = String(payload.tool || payload.name || '').trim();
  const kind = normalizeApprovalKind(payload.kind);
  const normalizedSessionId = String(
    payload.session_id ?? payload.sessionId ?? sessionId ?? ''
  ).trim();
  if (!normalizedSessionId) {
    return null;
  }
  return {
    approval_id: approvalId,
    request_id: String(requestId || '').trim(),
    session_id: normalizedSessionId,
    tool,
    kind,
    summary: summary || tool || approvalId,
    detail: payload.detail ?? null,
    args: payload.args ?? null,
    created_at: new Date().toISOString()
  };
};

export const normalizeApprovalResultId = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  return String(payload.approval_id ?? payload.id ?? '').trim();
};

export const stringifyPayload = (payload) => {
  if (payload === null || payload === undefined) return '';
  if (typeof payload === 'string') return payload;
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
};

export const tailText = (text, maxLength = 240) => {
  if (!text) return '';
  return text.length > maxLength ? `...${text.slice(-maxLength)}` : text;
};

export const normalizePlanStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return 'pending';
  const normalized = raw.replace(/[-\s]+/g, '_');
  if (normalized === 'pending') return 'pending';
  if (normalized === 'in_progress' || normalized === 'inprogress') return 'in_progress';
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') return 'completed';
  return 'pending';
};

export const normalizePlanPayload = (payload) => {
  if (!payload) return null;
  const rawPlan = Array.isArray(payload?.plan)
    ? payload.plan
    : Array.isArray(payload?.steps)
      ? payload.steps
      : Array.isArray(payload)
        ? payload
        : [];
  if (!rawPlan.length) return null;
  const explanation = typeof payload?.explanation === 'string' ? payload.explanation.trim() : '';
  const steps = [];
  let hasInProgress = false;
  rawPlan.forEach((item) => {
    if (!item) return;
    const step = String(item?.step ?? item?.title ?? item).trim();
    if (!step) return;
    let status = normalizePlanStatus(item?.status);
    if (status === 'in_progress') {
      if (hasInProgress) {
        status = 'pending';
      } else {
        hasInProgress = true;
      }
    }
    steps.push({ step, status });
  });
  if (!steps.length) return null;
  return {
    explanation,
    steps
  };
};

export const isRecommendedLabel = (label) => {
  const normalized = String(label || '').trim();
  if (!normalized) return false;
  const lowered = normalized.toLowerCase();
  const keywords = new Set(['推荐', 'recommended', t('chat.inquiry.recommended')]);
  for (const keyword of keywords) {
    if (!keyword) continue;
    if (lowered.includes(String(keyword).toLowerCase())) {
      return true;
    }
  }
  return false;
};

export const normalizeInquiryRoutes = (routes) =>
  (Array.isArray(routes) ? routes : [])
    .map((item) => {
      if (!item) return null;
      if (typeof item === 'string') {
        const label = item.trim();
        if (!label) return null;
        return { label, description: '', recommended: isRecommendedLabel(label) };
      }
      if (typeof item !== 'object') return null;
      const label = String(item.label ?? item.title ?? item.name ?? '').trim();
      if (!label) return null;
      const description = String(item.description ?? item.detail ?? item.desc ?? item.summary ?? '').trim();
      return {
        label,
        description,
        recommended: Boolean(item.recommended ?? item.preferred) || isRecommendedLabel(label)
      };
    })
    .filter(Boolean);

export const normalizeInquiryPanelPayload = (payload) => {
  if (!payload || typeof payload !== 'object') return null;
  const question = String(
    payload.question ?? payload.prompt ?? payload.title ?? payload.header ?? ''
  ).trim();
  let normalizedRoutes = normalizeInquiryRoutes(payload.routes);
  if (!normalizedRoutes.length) {
    normalizedRoutes = normalizeInquiryRoutes(payload.options);
  }
  if (!normalizedRoutes.length) {
    normalizedRoutes = normalizeInquiryRoutes(payload.choices);
  }
  normalizedRoutes = Array.isArray(normalizedRoutes) ? normalizedRoutes.filter(Boolean) : [];
  if (normalizedRoutes.length === 0) {
    return null;
  }
  const keepOpenRaw = payload.keep_open ?? payload.keepOpen ?? payload.awaiting;
  const keepOpen = keepOpenRaw === undefined ? true : keepOpenRaw === true;
  return {
    question: question || t('chat.inquiry.defaultQuestion'),
    routes: normalizedRoutes,
    multiple: payload.multiple === true || payload.allow_multiple === true || payload.multi === true,
    keepOpen
  };
};

export const normalizeInquiryPanelStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'answered') return 'answered';
  if (raw === 'dismissed') return 'dismissed';
  return 'pending';
};

export const normalizeInquiryPanelState = (panel) => {
  const normalized = normalizeInquiryPanelPayload(panel);
  if (!normalized) return null;
  const status = normalizeInquiryPanelStatus(panel?.status);
  const selected = Array.isArray(panel?.selected)
    ? panel.selected.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  return { ...normalized, status, selected };
};

export const dismissStaleInquiryPanels = (messages = []) => {
  if (!Array.isArray(messages)) return false;
  let hasUserAfter = false;
  let updated = false;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (!message) continue;
    if (message.role === 'user') {
      hasUserAfter = true;
      continue;
    }
    if (message.role !== 'assistant') {
      continue;
    }
    const panel = normalizeInquiryPanelState(message.questionPanel);
    if (!panel) continue;
    if (
      panel.status === 'pending' &&
      (hasUserAfter || (!panel.keepOpen && !isMessageRunning(message)))
    ) {
      message.questionPanel = { ...panel, status: 'dismissed' };
      updated = true;
      continue;
    }
    message.questionPanel = panel;
  }
  return updated;
};

export const isQuestionPanelToolName = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return false;
  if (raw === '问询面板') return true;
  const lower = raw.toLowerCase();
  return lower === 'question_panel' || lower === 'ask_panel';
};

export const hasPlanSteps = (plan) => Array.isArray(plan?.steps) && plan.steps.length > 0;
export const isMessageRunning = (message) =>
  normalizeFlag(message?.stream_incomplete) || normalizeFlag(message?.workflowStreaming);
export const shouldAutoShowPlan = (plan, message) => hasPlanSteps(plan) && isMessageRunning(message);

export const applyPlanUpdate = (assistantMessage, payload) => {
  if (!assistantMessage || assistantMessage.role !== 'assistant') return null;
  const normalized = normalizePlanPayload(payload);
  if (!normalized) return null;
  assistantMessage.plan = normalized;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalized, assistantMessage);
  return normalized;
};

export const buildWorkflowItem = (title, detail, status = 'completed', meta = {}) => ({
  id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
  title,
  detail,
  status,
  ...meta
});

export const buildToolIdentityMeta = (...sources) => {
  for (const source of sources) {
    if (!source || typeof source !== 'object') continue;
    const toolDisplayName = pickString(
      source.tool_display_name,
      source.toolDisplayName,
      source.display_name,
      source.displayName
    );
    const toolRuntimeName = pickString(
      source.tool_runtime_name,
      source.toolRuntimeName,
      source.runtime_name,
      source.runtimeName
    );
    const toolFunctionName = pickString(
      source.tool_function_name,
      source.toolFunctionName,
      source.function_name,
      source.functionName
    );
    if (toolDisplayName || toolRuntimeName || toolFunctionName) {
      return {
        ...(toolDisplayName ? { toolDisplayName, tool_display_name: toolDisplayName } : {}),
        ...(toolRuntimeName ? { toolRuntimeName, tool_runtime_name: toolRuntimeName } : {}),
        ...(toolFunctionName ? { toolFunctionName, tool_function_name: toolFunctionName } : {})
      };
    }
  }
  return {};
};

export const hydrateSessionCommandSessions = (sessionId, snapshots) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !Array.isArray(snapshots)) return;
  useCommandSessionStore().hydrateSession(targetId, snapshots);
};

export const upsertCommandSessionRuntime = (sessionId, payload) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !payload) return null;
  return useCommandSessionStore().upsertSnapshot(targetId, payload);
};

export const appendCommandSessionRuntimeDelta = (
  sessionId,
  commandSessionId,
  stream,
  delta,
  meta = {}
) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId || !commandSessionId || !delta) return null;
  return useCommandSessionStore().appendDelta(targetId, commandSessionId, stream, delta, meta);
};

export const clearSessionCommandSessions = (sessionId) => {
  const targetId = resolveSessionKey(sessionId);
  if (!targetId) return;
  useCommandSessionStore().clearSession(targetId);
};

// 会话级模型轮次状态，保证同一会话的轮次连续递增
