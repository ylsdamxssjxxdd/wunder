import { defineStore } from 'pinia';

import {
  cancelMessageStream,
  compactSession as compactSessionApi,
  createSession,
  deleteSession as deleteSessionApi,
  fetchChatTransportProfile,
  getSession,
  getSessionEvents,
  listSessions,
  openChatSocket,
  resumeMessageStream,
  sendMessageStream,
  updateSessionTools as updateSessionToolsApi
} from '@/api/chat';
import { t } from '@/i18n';
import { setDefaultSession } from '@/api/agents';
import { consumeSseStream } from '@/utils/sse';
import { createWsMultiplexer } from '@/utils/ws';
import { isDemoMode, loadDemoChatState, saveDemoChatState } from '@/utils/demo';
import { emitWorkspaceRefresh } from '@/utils/workspaceEvents';
import { getDesktopToolCallModeForRequest } from '@/config/desktop';

type SnapshotAssistantMessage = {
  role: string;
  content: string;
  created_at: string;
  reasoning?: string;
  reasoningStreaming?: boolean;
  workflowStreaming?: boolean;
  stream_incomplete?: boolean;
  stream_event_id?: number;
  stream_round?: number;
  workflowItems?: unknown[];
  plan?: unknown;
  questionPanel?: unknown;
  stats?: unknown;
  planVisible?: boolean;
  isGreeting?: boolean;
};

type DemoChatCachePatch = {
  sessions?: unknown[];
  sessionId?: string | number | null;
  messages?: unknown[];
};

type GreetingMessageOptions = {
  greeting?: unknown;
  createdAt?: unknown;
  sessionCreatedAt?: unknown;
};

type SessionWorkflowStateOptions = {
  reset?: boolean;
};

type WorkflowEventRawPayload = {
  data: unknown;
  timestamp?: unknown;
};

type WorkflowProcessorOptions = {
  finalizeWithNow?: boolean;
};

type UsageStatsOptions = {
  updateUsage?: boolean;
};

type QuestionPanelApplyOptions = {
  appendWorkflow?: boolean;
};

type InquiryPanelPatch = {
  status?: unknown;
  selected?: unknown[];
};

type LoadSessionsOptions = {
  skipTransportRefresh?: boolean;
  refresh_transport?: boolean;
  agent_id?: string | number | boolean | null | undefined;
};

type OpenDraftSessionOptions = {
  agent_id?: string | number | boolean | null | undefined;
};

type SendMessageOptions = {
  attachments?: unknown[];
};

type AppendLocalMessageOptions = {
  createdAt?: unknown;
  sessionId?: unknown;
  immediate?: boolean;
};

type ResumeStreamOptions = {
  force?: boolean;
  afterEventId?: number | string;
};

type ApprovalDecision = 'approve_once' | 'approve_session' | 'deny';

type PendingApproval = {
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

const MESSENGER_AGENT_APPROVAL_MODE_STORAGE_KEY = 'messenger_agent_approval_mode';

const normalizeApprovalModeForRequest = (value: unknown): string | null => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'suggest') return 'suggest';
  if (raw === 'auto_edit' || raw === 'auto-edit') return 'auto_edit';
  if (raw === 'full_auto' || raw === 'full-auto') return 'full_auto';
  return null;
};

const getMessengerApprovalModeForRequest = (): string | null => {
  if (typeof window === 'undefined') return null;
  try {
    return normalizeApprovalModeForRequest(
      window.localStorage.getItem(MESSENGER_AGENT_APPROVAL_MODE_STORAGE_KEY)
    );
  } catch {
    return null;
  }
};

const buildMessageStats = () => ({
  toolCalls: 0,
  usage: null,
  prefill_duration_s: null,
  decode_duration_s: null,
  quotaConsumed: 0,
  quotaSnapshot: null,
  contextTokens: null,
  interaction_start_ms: null,
  interaction_end_ms: null,
  interaction_duration_s: null
});

const normalizeStatsCount = (value) => {
  if (value === null || value === undefined) return 0;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

const parseOptionalCount = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const normalizeQuotaConsumed = (value) => {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return normalizeStatsCount(value.consumed ?? value.used ?? value.count ?? 0);
  }
  return normalizeStatsCount(value);
};

const normalizeQuotaSnapshot = (value) => {
  if (!value || typeof value !== 'object') return null;
  const daily = parseOptionalCount(
    value.daily_quota ?? value.dailyQuota ?? value.daily ?? value.quota ?? value.total
  );
  const used = parseOptionalCount(
    value.used ?? value.consumed ?? value.count ?? value.usage
  );
  const remaining = parseOptionalCount(
    value.remaining ?? value.left ?? value.quota_remaining ?? value.remain
  );
  const date = value.date ?? value.quota_date ?? value.quotaDate ?? '';
  if (daily === null && used === null && remaining === null && !date) return null;
  return {
    daily,
    used,
    remaining,
    date: date ? String(date) : ''
  };
};

const normalizeContextTokens = (value) => parseOptionalCount(value);

const normalizeDurationValue = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
};

const resolveInteractionDuration = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return null;
  }
  return (endMs - startMs) / 1000;
};

const normalizeInteractionTimestamp = (value) => {
  const millis = resolveTimestampMs(value);
  return Number.isFinite(millis) ? millis : null;
};

const normalizeUsagePayload = (payload) => {
  if (!payload || typeof payload !== 'object') return null;
  const source = payload;
  const input = Number.parseInt(
    source.input_tokens ?? source.prompt_tokens ?? source.input ?? source.prompt ?? 0,
    10
  );
  const output = Number.parseInt(
    source.output_tokens ?? source.completion_tokens ?? source.output ?? source.completion ?? 0,
    10
  );
  const totalRaw = source.total_tokens ?? source.total ?? null;
  const totalParsed = totalRaw === null || totalRaw === undefined ? null : Number.parseInt(totalRaw, 10);
  const hasInput = Number.isFinite(input) && input > 0;
  const hasOutput = Number.isFinite(output) && output > 0;
  const total =
    Number.isFinite(totalParsed) && totalParsed >= 0 ? totalParsed : (hasInput || hasOutput ? input + output : null);
  if (!hasInput && !hasOutput && total === null) {
    return null;
  }
  return {
    input: hasInput ? input : 0,
    output: hasOutput ? output : 0,
    total: total ?? 0
  };
};

const normalizeMessageStats = (stats) => {
  if (!stats || typeof stats !== 'object') {
    return null;
  }
  const quotaSnapshot = normalizeQuotaSnapshot(
    stats.quotaSnapshot ?? stats.quota ?? stats.quota_usage ?? stats.quotaUsage
  );
  const contextTokens = normalizeContextTokens(
    stats.contextTokens ??
      stats.context_tokens ??
      stats.context_tokens_total ??
      stats.contextUsage ??
      stats.context_usage?.context_tokens ??
      stats.context_usage?.contextTokens
  );
  const interactionStartMs = normalizeInteractionTimestamp(
    stats.interaction_start_ms ?? stats.interactionStartMs ?? stats.interaction_start ?? stats.started_at
  );
  const interactionEndMs = normalizeInteractionTimestamp(
    stats.interaction_end_ms ?? stats.interactionEndMs ?? stats.interaction_end ?? stats.ended_at
  );
  const interactionDuration = normalizeDurationValue(
    stats.interaction_duration_s ??
      stats.interactionDurationS ??
      stats.interactionDuration ??
      stats.duration_s ??
      stats.elapsed_s
  );
  const rangedDuration = resolveInteractionDuration(interactionStartMs, interactionEndMs);
  return {
    toolCalls: normalizeStatsCount(stats.toolCalls),
    usage: normalizeUsagePayload(stats.usage ?? stats.tokenUsage ?? stats.token_usage),
    prefill_duration_s: normalizeDurationValue(
      stats.prefill_duration_s ?? stats.prefillDurationS ?? stats.prefillDuration
    ),
    decode_duration_s: normalizeDurationValue(
      stats.decode_duration_s ?? stats.decodeDurationS ?? stats.decodeDuration
    ),
    quotaConsumed: normalizeQuotaConsumed(
      stats.quotaConsumed ?? stats.quota_consumed ?? stats.quota
    ),
    quotaSnapshot,
    contextTokens,
    interaction_start_ms: interactionStartMs,
    interaction_end_ms: interactionEndMs,
    interaction_duration_s: rangedDuration ?? interactionDuration
  };
};

const extractErrorMessage = (payload) => {
  if (!payload || typeof payload !== 'object') {
    return '';
  }
  const detail = payload.detail;
  if (detail) {
    if (typeof detail === 'string') {
      return detail;
    }
    if (detail.message) {
      return detail.message;
    }
    if (detail.error) {
      return detail.error;
    }
    if (detail.detail?.message) {
      return detail.detail.message;
    }
  }
  return payload.message || payload.error || '';
};

const parseErrorText = (text) => {
  if (!text) {
    return '';
  }
  try {
    const payload = JSON.parse(text);
    return extractErrorMessage(payload) || text;
  } catch (error) {
    return text;
  }
};

const readResponseError = async (response) => {
  if (!response) {
    return '';
  }
  try {
    const text = await response.text();
    return parseErrorText(text);
  } catch (error) {
    return '';
  }
};

const ensureMessageStats = (message) => {
  if (!message || message.role !== 'assistant') return null;
  const normalized = normalizeMessageStats(message.stats);
  if (normalized) {
    message.stats = normalized;
    return normalized;
  }
  const fresh = buildMessageStats();
  message.stats = fresh;
  return fresh;
};

const mergeMessageStats = (base, incoming) => {
  const left = normalizeMessageStats(base);
  const right = normalizeMessageStats(incoming);
  if (!left && !right) return null;
  if (!left) return right;
  if (!right) return left;
  const startMs =
    left.interaction_start_ms === null || left.interaction_start_ms === undefined
      ? right.interaction_start_ms
      : right.interaction_start_ms === null || right.interaction_start_ms === undefined
        ? left.interaction_start_ms
        : Math.min(left.interaction_start_ms, right.interaction_start_ms);
  const endMs =
    left.interaction_end_ms === null || left.interaction_end_ms === undefined
      ? right.interaction_end_ms
      : right.interaction_end_ms === null || right.interaction_end_ms === undefined
        ? left.interaction_end_ms
        : Math.max(left.interaction_end_ms, right.interaction_end_ms);
  const rangedDuration = resolveInteractionDuration(startMs, endMs);
  const duration =
    rangedDuration ??
    normalizeDurationValue(
      right.interaction_duration_s ?? left.interaction_duration_s
    );
  const quotaSnapshot = right.quotaSnapshot || left.quotaSnapshot;
  const contextTokens =
    right.contextTokens === null || right.contextTokens === undefined
      ? left.contextTokens
      : right.contextTokens;
  return {
    toolCalls: Math.max(left.toolCalls, right.toolCalls),
    usage: right.usage || left.usage,
    prefill_duration_s:
      right.prefill_duration_s === null || right.prefill_duration_s === undefined
        ? left.prefill_duration_s
        : right.prefill_duration_s,
    decode_duration_s:
      right.decode_duration_s === null || right.decode_duration_s === undefined
        ? left.decode_duration_s
        : right.decode_duration_s,
    quotaConsumed: Math.max(left.quotaConsumed, right.quotaConsumed),
    quotaSnapshot,
    contextTokens,
    interaction_start_ms: startMs,
    interaction_end_ms: endMs,
    interaction_duration_s: duration
  };
};

const resolveTimestampMs = (value) => {
  if (value === null || value === undefined) return null;
  if (value instanceof Date) {
    const time = value.getTime();
    return Number.isNaN(time) ? null : time;
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) return null;
    const millis = value < 1e12 ? value * 1000 : value;
    return Number.isFinite(millis) ? millis : null;
  }
  const text = String(value).trim();
  if (!text) return null;
  if (/^\d+$/.test(text)) {
    const numeric = Number.parseInt(text, 10);
    if (!Number.isFinite(numeric)) return null;
    const millis = numeric < 1e12 ? numeric * 1000 : numeric;
    return Number.isFinite(millis) ? millis : null;
  }
  const parsed = new Date(text);
  const time = parsed.getTime();
  return Number.isNaN(time) ? null : time;
};

const resolveTimestampIso = (value) => {
  const millis = resolveTimestampMs(value);
  return millis === null ? '' : new Date(millis).toISOString();
};

const buildMessage = (role, content, createdAt = undefined) => ({
  role,
  content,
  created_at: resolveTimestampIso(createdAt) || new Date().toISOString(),
  reasoning: '',
  reasoningStreaming: false,
  plan: null,
  planVisible: false,
  questionPanel: null,
  stats: role === 'assistant' ? buildMessageStats() : null
});

const resolveGreetingContent = (override) => {
  const trimmed = String(override || '').trim();
  return trimmed ? trimmed : t('chat.greeting');
};
const CHAT_STATE_KEY = 'wille-chat-state';
const DEFAULT_AGENT_KEY = '__default__';

const normalizeAgentKey = (agentId) => {
  const cleaned = String(agentId || '').trim();
  return cleaned || DEFAULT_AGENT_KEY;
};

const normalizeSessionMap = (value) => {
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

const buildChatPersistState = () => ({
  activeSessionId: '',
  draft: false,
  lastSessionByAgent: {}
});

const normalizeChatPersistState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildChatPersistState();
  }
  return {
    activeSessionId: typeof value.activeSessionId === 'string' ? value.activeSessionId : '',
    draft: value.draft === true,
    lastSessionByAgent: normalizeSessionMap(value.lastSessionByAgent)
  };
};

const readChatPersistState = () => {
  try {
    const raw = localStorage.getItem(CHAT_STATE_KEY);
    if (!raw) return buildChatPersistState();
    return normalizeChatPersistState(JSON.parse(raw));
  } catch (error) {
    return buildChatPersistState();
  }
};

const updateChatPersistState = (updater) => {
  try {
    const current = readChatPersistState();
    const next = normalizeChatPersistState(updater(current));
    localStorage.setItem(CHAT_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // ignore persistence errors
  }
};

const updateAgentSessionMap = (map, agentId, sessionId) => {
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

const persistAgentSession = (agentId, sessionId) => {
  updateChatPersistState((current) => ({
    ...current,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, sessionId)
  }));
};

const persistActiveSession = (sessionId, agentId) => {
  const cleanedSessionId = String(sessionId || '').trim();
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: cleanedSessionId,
    draft: false,
    lastSessionByAgent: updateAgentSessionMap(current.lastSessionByAgent, agentId, cleanedSessionId)
  }));
};

const applyMainSession = (sessions, agentId, sessionId) => {
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

const persistDraftSession = () => {
  updateChatPersistState((current) => ({
    ...current,
    activeSessionId: '',
    draft: true
  }));
};

const resolvePersistedSessionId = (agentId) => {
  const key = normalizeAgentKey(agentId);
  const state = readChatPersistState();
  return state.lastSessionByAgent?.[key] || '';
};

const CHAT_SNAPSHOT_KEY = 'wille-chat-snapshot';
const SNAPSHOT_FLUSH_MS = 400;
const MAX_SNAPSHOT_MESSAGES = 50;
const SNAPSHOT_MATCH_WINDOW_MS = 2000;
let snapshotTimer = null;
let pageUnloading = false;

const normalizeStreamEventId = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const getRuntimeLastEventId = (runtime) => {
  const normalized = normalizeStreamEventId(runtime?.lastEventId);
  return normalized === null ? 0 : normalized;
};

const updateRuntimeLastEventId = (runtime, eventId) => {
  if (!runtime) return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(runtime.lastEventId);
  if (current === null || normalized > current) {
    runtime.lastEventId = normalized;
  }
};

const normalizeStreamRound = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const readDeltaSegments = (value) => {
  if (!value || typeof value !== 'object') return [];
  if (Array.isArray(value.segments)) {
    return value.segments;
  }
  if (value.data && typeof value.data === 'object' && Array.isArray(value.data.segments)) {
    return value.data.segments;
  }
  return [];
};

const parseSegmentedDelta = (payload, data) => {
  const candidates = [data, payload];
  for (const source of candidates) {
    const segments = readDeltaSegments(source);
    if (!segments.length) continue;
    let delta = '';
    let reasoningDelta = '';
    let round = null;
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
      const segmentRound = normalizeStreamRound(
        segment.user_round ?? segment.model_round ?? segment.round
      );
      if (segmentRound !== null) {
        round = segmentRound;
      }
    });
    return { delta, reasoningDelta, round };
  }
  return null;
};

const resolveEventRoundNumber = (payload, data) => {
  const directRound = normalizeStreamRound(
    data?.user_round ??
      payload?.user_round ??
      data?.model_round ??
      payload?.model_round ??
      data?.round ??
      payload?.round
  );
  if (directRound !== null) {
    return directRound;
  }
  return parseSegmentedDelta(payload, data)?.round ?? null;
};

const assignStreamEventId = (message, eventId) => {
  if (!message || message.role !== 'assistant') return;
  const normalized = normalizeStreamEventId(eventId);
  if (normalized === null) return;
  const current = normalizeStreamEventId(message.stream_event_id);
  if (current === null || normalized > current) {
    message.stream_event_id = normalized;
  }
};

const normalizeFlag = (value) => value === true || value === 'true';

const normalizeSnapshotMessage = (message) => {
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
  if (message.role === 'assistant') {
    base.reasoning = normalizedAssistant?.reasoning || '';
    base.reasoningStreaming = normalizeFlag(message.reasoningStreaming);
    base.workflowStreaming = normalizeFlag(message.workflowStreaming);
    base.stream_incomplete = normalizeFlag(message.stream_incomplete);
    const streamEventId = normalizeStreamEventId(message.stream_event_id);
    if (streamEventId !== null) {
      base.stream_event_id = streamEventId;
    }
    const streamRound = normalizeStreamRound(message.stream_round);
    if (streamRound !== null) {
      base.stream_round = streamRound;
    }
    if (Array.isArray(message.workflowItems) && message.workflowItems.length) {
      base.workflowItems = message.workflowItems;
    }
    const plan = normalizePlanPayload(message.plan);
    if (plan) {
      base.plan = plan;
    }
    const questionPanel = normalizeInquiryPanelState(message.questionPanel);
    if (questionPanel) {
      base.questionPanel = questionPanel;
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
  return base;
};

const buildSnapshotMessages = (messages = []) => {
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
      return normalized;
    })
    .filter(Boolean);
};

const buildChatSnapshot = (storeState) => {
  const sessionId = String(storeState.activeSessionId || '');
  if (!sessionId) return null;
  const messages = buildSnapshotMessages(storeState.messages || []);
  if (!messages.length) return null;
  return {
    sessionId,
    messages,
    updatedAt: Date.now()
  };
};

const readChatSnapshot = () => {
  try {
    const raw = localStorage.getItem(CHAT_SNAPSHOT_KEY);
    if (!raw) return null;
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

const writeChatSnapshot = (payload) => {
  if (!payload) return;
  try {
    localStorage.setItem(CHAT_SNAPSHOT_KEY, JSON.stringify(payload));
  } catch (error) {
    // ignore persistence errors
  }
};

const clearChatSnapshot = (sessionId) => {
  try {
    const current = readChatSnapshot();
    if (!current || current.sessionId !== String(sessionId || '')) return;
    localStorage.removeItem(CHAT_SNAPSHOT_KEY);
  } catch (error) {
    // ignore storage errors
  }
};

const scheduleChatSnapshot = (storeState, immediate = false) => {
  const flush = () => {
    const snapshot = buildChatSnapshot(storeState);
    if (snapshot) {
      writeChatSnapshot(snapshot);
    }
  };
  if (immediate) {
    flush();
    return;
  }
  if (snapshotTimer !== null) return;
  snapshotTimer = setTimeout(() => {
    snapshotTimer = null;
    flush();
  }, SNAPSHOT_FLUSH_MS);
};

const mergeSnapshotAssistant = (target, snapshot) => {
  if (!target || target.role !== 'assistant' || !snapshot) return false;
  const snapshotContent = String(snapshot.content || '');
  const serverContent = String(target.content || '');
  const snapshotEventId = normalizeStreamEventId(snapshot.stream_event_id);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const snapshotRound = normalizeStreamRound(snapshot.stream_round);
  const targetRound = normalizeStreamRound(target.stream_round);
  const snapshotPlan = normalizePlanPayload(snapshot.plan);
  const hasSnapshotPlan = hasPlanSteps(snapshotPlan);
  const snapshotStats = normalizeMessageStats(snapshot.stats);
  const hasWorkflowItems =
    Array.isArray(snapshot.workflowItems) && snapshot.workflowItems.length > 0;
  const shouldMergeContent =
    snapshotContent.length > serverContent.length ||
    (snapshot.stream_incomplete && serverContent.length === 0);
  const shouldMergeFlags = Boolean(
    snapshot.stream_incomplete ||
      snapshot.workflowStreaming ||
      snapshot.reasoningStreaming ||
      hasWorkflowItems ||
      hasSnapshotPlan ||
      snapshotRound !== null ||
      snapshotEventId !== null ||
      snapshot.questionPanel
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
    target.reasoningStreaming =
      normalizeFlag(snapshot.reasoningStreaming) ||
      normalizeFlag(target.reasoningStreaming);
    if (hasWorkflowItems) {
      const targetHasItems =
        Array.isArray(target.workflowItems) && target.workflowItems.length > 0;
      if (!targetHasItems || snapshot.workflowItems.length >= target.workflowItems.length) {
        target.workflowItems = snapshot.workflowItems;
      }
    }
    target.workflowStreaming =
      normalizeFlag(snapshot.workflowStreaming) ||
      normalizeFlag(target.workflowStreaming);
    target.stream_incomplete =
      normalizeFlag(snapshot.stream_incomplete) ||
      normalizeFlag(target.stream_incomplete);
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
  }
  if (snapshotStats) {
    target.stats = mergeMessageStats(target.stats, snapshotStats);
  }
  return true;
};

const findSnapshotAssistantIndex = (target, snapshotAssistants, cursor) => {
  if (!target || cursor < 0) return -1;
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const targetRound = normalizeStreamRound(target.stream_round);
  const targetTime = resolveTimestampMs(target.created_at);
  const targetContent = normalizeAssistantContent(target.content || '');
  let bestIndex = -1;
  let bestDelta = Infinity;
  for (let i = cursor; i >= 0; i -= 1) {
    const snapshot = snapshotAssistants[i];
    if (!snapshot) continue;
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
    if (targetContent) {
      const snapshotContent = normalizeAssistantContent(snapshot.content || '');
      if (
        snapshotContent &&
        (targetContent.includes(snapshotContent) || snapshotContent.includes(targetContent))
      ) {
        return i;
      }
    }
  }
  if (bestIndex >= 0) return bestIndex;
  const isPendingTarget =
    normalizeFlag(target.stream_incomplete) || !normalizeAssistantContent(target.content || '');
  if (isPendingTarget || !Number.isFinite(targetTime)) {
    return cursor;
  }
  return -1;
};

const hasMatchingSnapshotAssistant = (messages, snapshotAssistant) => {
  if (!snapshotAssistant || !Array.isArray(messages)) return false;
  const snapshotEventId = normalizeStreamEventId(snapshotAssistant.stream_event_id);
  const snapshotRound = normalizeStreamRound(snapshotAssistant.stream_round);
  const snapshotTime = resolveTimestampMs(snapshotAssistant.created_at);
  const snapshotContent = normalizeAssistantContent(snapshotAssistant.content || '');
  return messages.some((message) => {
    if (!message || message.role !== 'assistant') return false;
    const messageEventId = normalizeStreamEventId(message.stream_event_id);
    if (snapshotEventId !== null && messageEventId !== null && snapshotEventId === messageEventId) {
      return true;
    }
    const messageRound = normalizeStreamRound(message.stream_round);
    if (snapshotRound !== null && messageRound !== null && snapshotRound === messageRound) {
      return true;
    }
    const messageTime = resolveTimestampMs(message.created_at);
    if (
      Number.isFinite(snapshotTime) &&
      Number.isFinite(messageTime) &&
      Math.abs(snapshotTime - messageTime) <= SNAPSHOT_MATCH_WINDOW_MS
    ) {
      return true;
    }
    if (snapshotContent) {
      const messageContent = normalizeAssistantContent(message.content || '');
      if (
        messageContent &&
        (messageContent.includes(snapshotContent) || snapshotContent.includes(messageContent))
      ) {
        return true;
      }
    }
    return false;
  });
};

const mergeSnapshotIntoMessages = (messages, snapshot) => {
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
  const snapshotAssistants = snapshotMessages.filter(
    (message) => message.role === 'assistant' && !message.isGreeting
  );
  if (!snapshotAssistants.length) return messages;
  let cursor = snapshotAssistants.length - 1;
  for (let i = messages.length - 1; i >= 0 && cursor >= 0; i -= 1) {
    const target = messages[i];
    if (target?.role !== 'assistant') {
      continue;
    }
    const matchIndex = findSnapshotAssistantIndex(target, snapshotAssistants, cursor);
    if (matchIndex < 0) {
      continue;
    }
    mergeSnapshotAssistant(target, snapshotAssistants[matchIndex]);
    cursor = matchIndex - 1;
  }
  const snapshotLastMessage = snapshotMessages[snapshotMessages.length - 1];
  const serverLastMessage = messages[messages.length - 1];
  if (
    serverLastMessage?.role === 'user' &&
    snapshotLastMessage?.role === 'assistant' &&
    normalizeFlag(snapshotLastMessage.stream_incomplete) &&
    !hasMatchingSnapshotAssistant(messages, snapshotLastMessage)
  ) {
    return [...messages, { ...snapshotLastMessage }];
  }
  return messages;
};

// 演示模式聊天缓存结构（仅用于本地暂存）
const buildDemoChatState = () => ({
  sessions: [],
  messages: {}
});

const normalizeDemoChatState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildDemoChatState();
  }
  return {
    sessions: Array.isArray(value.sessions) ? value.sessions : [],
    messages: value.messages && typeof value.messages === 'object' ? value.messages : {}
  };
};

const getDemoChatState = () => normalizeDemoChatState(loadDemoChatState());

const persistDemoChatState = (state) => saveDemoChatState(state);

const syncDemoChatCache = ({ sessions, sessionId, messages }: DemoChatCachePatch = {}) => {
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

const removeDemoChatSession = (sessionId) => {
  if (!isDemoMode() || !sessionId) return;
  const state = getDemoChatState();
  state.sessions = (state.sessions || []).filter((item) => item.id !== sessionId);
  if (state.messages?.[sessionId]) {
    delete state.messages[sessionId];
  }
  persistDemoChatState(state);
};

const resolveSessionActivityTime = (session) =>
  resolveTimestampMs(
    session?.updated_at ?? session?.last_message_at ?? session?.created_at
  );

const sortSessionsByActivity = (sessions = []) =>
  (Array.isArray(sessions) ? sessions.slice() : [])
    .map((session, index) => ({ session, index }))
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

const buildGreetingMessage = (createdAt, greeting) => ({
  ...buildMessage('assistant', resolveGreetingContent(greeting), createdAt),
  workflowItems: [],
  workflowStreaming: false,
  isGreeting: true
});

const resolveGreetingTimestamp = (messages, createdAt) => {
  const direct = resolveTimestampIso(createdAt);
  if (direct) return direct;
  const safeMessages = Array.isArray(messages) ? messages : [];
  const candidate = safeMessages.find((message) => message?.created_at)?.created_at;
  return resolveTimestampIso(candidate);
};

const ensureGreetingMessage = (messages, options: GreetingMessageOptions = {}) => {
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

const safeJsonParse = (raw) => {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (error) {
    return null;
  }
};

const normalizeApprovalKind = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'exec' || raw === 'patch') {
    return raw;
  }
  return '';
};

const normalizePendingApproval = (payload, requestId, sessionId): PendingApproval | null => {
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

const normalizeApprovalResultId = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  return String(payload.approval_id ?? payload.id ?? '').trim();
};

const stringifyPayload = (payload) => {
  if (payload === null || payload === undefined) return '';
  if (typeof payload === 'string') return payload;
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
};

const tailText = (text, maxLength = 240) => {
  if (!text) return '';
  return text.length > maxLength ? `...${text.slice(-maxLength)}` : text;
};

const normalizePlanStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (!raw) return 'pending';
  const normalized = raw.replace(/[-\s]+/g, '_');
  if (normalized === 'pending') return 'pending';
  if (normalized === 'in_progress' || normalized === 'inprogress') return 'in_progress';
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') return 'completed';
  return 'pending';
};

const normalizePlanPayload = (payload) => {
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

const isRecommendedLabel = (label) => {
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

const normalizeInquiryRoutes = (routes) =>
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

const normalizeInquiryPanelPayload = (payload) => {
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

const normalizeInquiryPanelStatus = (value) => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'answered') return 'answered';
  if (raw === 'dismissed') return 'dismissed';
  return 'pending';
};

const normalizeInquiryPanelState = (panel) => {
  const normalized = normalizeInquiryPanelPayload(panel);
  if (!normalized) return null;
  const status = normalizeInquiryPanelStatus(panel?.status);
  const selected = Array.isArray(panel?.selected)
    ? panel.selected.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  return { ...normalized, status, selected };
};

const dismissStaleInquiryPanels = (messages = []) => {
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

const isQuestionPanelToolName = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return false;
  if (raw === '问询面板') return true;
  const lower = raw.toLowerCase();
  return lower === 'question_panel' || lower === 'ask_panel';
};

const hasPlanSteps = (plan) => Array.isArray(plan?.steps) && plan.steps.length > 0;
const isMessageRunning = (message) =>
  normalizeFlag(message?.stream_incomplete) || normalizeFlag(message?.workflowStreaming);
const shouldAutoShowPlan = (plan, message) => hasPlanSteps(plan) && isMessageRunning(message);

const applyPlanUpdate = (assistantMessage, payload) => {
  if (!assistantMessage || assistantMessage.role !== 'assistant') return null;
  const normalized = normalizePlanPayload(payload);
  if (!normalized) return null;
  assistantMessage.plan = normalized;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalized, assistantMessage);
  return normalized;
};

const buildWorkflowItem = (title, detail, status = 'completed', meta = {}) => ({
  id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
  title,
  detail,
  status,
  ...meta
});

// 会话级模型轮次状态，保证同一会话的轮次连续递增
const sessionWorkflowState = new Map();

const buildSessionWorkflowState = () => ({
  globalRound: 0,
  currentRound: null
});

const normalizeSessionWorkflowState = (state) => {
  if (!state || typeof state !== 'object') {
    return buildSessionWorkflowState();
  }
  if (!Number.isFinite(state.globalRound)) {
    state.globalRound = 0;
  }
  if (!Number.isFinite(state.currentRound)) {
    state.currentRound = null;
  }
  return state;
};

const getSessionWorkflowState = (sessionId, options: SessionWorkflowStateOptions = {}) => {
  const sessionKey = sessionId ? String(sessionId) : '';
  if (!sessionKey) {
    return buildSessionWorkflowState();
  }
  const reset = options.reset === true;
  let state = sessionWorkflowState.get(sessionKey);
  if (!state || reset) {
    state = buildSessionWorkflowState();
    sessionWorkflowState.set(sessionKey, state);
  }
  return normalizeSessionWorkflowState(state);
};

const updateWorkflowItem = (items, id, patch) => {
  const target = items.find((item) => item.id === id);
  if (target) {
    Object.assign(target, patch);
  }
};

const resolveEventType = (eventName, payload) => {
  // SSE 事件名优先，但遇到默认的 message 时允许使用 payload 内部字段
  const normalized = (eventName || '').trim();
  if (normalized && normalized !== 'message') return normalized;
  if (payload?.event) return payload.event;
  if (payload?.type) return payload.type;
  return normalized || 'message';
};

const handleApprovalEvent = (store, eventType, payload, requestId, sessionId) => {
  if (!store) return;
  if (eventType === 'approval_request') {
    store.enqueueApprovalRequest(requestId, sessionId, payload);
    return;
  }
  if (eventType === 'approval_result') {
    store.resolveApprovalResult(payload);
  }
};

const pickText = (value, fallback = '') => {
  if (value === null || value === undefined) return fallback;
  if (typeof value === 'string') return value;
  return stringifyPayload(value);
};

// 保留完整详情，供弹窗查看完整内容
const buildDetail = (payload) => stringifyPayload(payload);

const defaultSessionTitles = new Set(['新会话', '未命名会话']);

const buildSessionTitle = (content, maxLength = 20) => {
  const cleaned = String(content || '').trim().replace(/\s+/g, ' ');
  if (!cleaned) return '';
  if (cleaned.length <= maxLength) return cleaned;
  return `${cleaned.slice(0, maxLength)}...`;
};

const shouldAutoTitle = (title) => {
  if (!title) return true;
  return defaultSessionTitles.has(String(title).trim());
};

const extractAnswerFromPayload = (payload) => {
  if (!payload || typeof payload !== 'object') return '';
  const data = payload.data;
  if (data && typeof data === 'object') {
    const answer = data.answer || data.content || data.message;
    if (answer) return String(answer);
  }
  const answer = payload.answer || payload.content || payload.message;
  return answer ? String(answer) : '';
};

const THINK_OPEN_TAG = '<think>';
const THINK_CLOSE_TAG = '</think>';

const normalizeReasoningText = (value) =>
  typeof value === 'string' ? value : value ? String(value) : '';

const resolveAssistantReasoning = (payload) =>
  normalizeReasoningText(
    payload?.reasoning ??
      payload?.reasoning_content ??
      payload?.think_content
  );

const extractAssistantPayloadText = (content) => {
  if (!content) return '';
  const rawText = typeof content === 'string' ? content : String(content);
  const payload = safeJsonParse(rawText);
  if (!payload) return rawText;
  const answer = extractAnswerFromPayload(payload);
  return answer || rawText;
};

const splitThinkTaggedContent = (content) => {
  const source = extractAssistantPayloadText(content);
  if (!source) {
    return { content: '', reasoning: '' };
  }
  let cursor = 0;
  let visibleContent = '';
  let reasoningContent = '';
  let firstThinkSeen = false;
  let thinkStartedAtHead = false;
  while (cursor < source.length) {
    const thinkStart = source.indexOf(THINK_OPEN_TAG, cursor);
    if (thinkStart < 0) {
      visibleContent += source.slice(cursor);
      break;
    }
    if (!firstThinkSeen) {
      firstThinkSeen = true;
      thinkStartedAtHead = thinkStart === 0;
    }
    visibleContent += source.slice(cursor, thinkStart);
    const thinkEnd = source.indexOf(THINK_CLOSE_TAG, thinkStart + THINK_OPEN_TAG.length);
    const thinkPayloadStart = thinkStart + THINK_OPEN_TAG.length;
    if (thinkEnd < 0) {
      reasoningContent += source.slice(thinkPayloadStart);
      break;
    }
    const thinkCloseEnd = thinkEnd + THINK_CLOSE_TAG.length;
    reasoningContent += source.slice(thinkPayloadStart, thinkEnd);
    cursor = thinkCloseEnd;
  }
  if (reasoningContent && thinkStartedAtHead) {
    visibleContent = visibleContent.replace(/^\s+/, '');
  }
  return {
    content: visibleContent,
    reasoning: reasoningContent
  };
};

const normalizeAssistantOutput = (content, reasoning = '') => {
  const normalizedReasoningRaw = normalizeReasoningText(reasoning);
  const normalizedReasoning = normalizedReasoningRaw
    ? splitThinkTaggedContent(normalizedReasoningRaw).reasoning || normalizedReasoningRaw
    : '';
  const parsed = splitThinkTaggedContent(content);
  return {
    content: parsed.content,
    inlineReasoning: parsed.reasoning,
    reasoning: normalizedReasoning || parsed.reasoning
  };
};

const normalizeAssistantContent = (content) => normalizeAssistantOutput(content).content;

const createThinkTagStreamParser = () => {
  let inThinkTag = false;
  let bufferedTail = '';
  const resolveSuffixLength = (text, marker) => {
    const maxLength = Math.min(Math.max(marker.length - 1, 0), text.length);
    for (let length = maxLength; length > 0; length -= 1) {
      if (marker.startsWith(text.slice(-length))) {
        return length;
      }
    }
    return 0;
  };
  const takeStableText = (source, cursor, marker) => {
    const remaining = source.slice(cursor);
    const suffixLength = resolveSuffixLength(remaining, marker);
    const safeEnd = source.length - suffixLength;
    return {
      stable: source.slice(cursor, safeEnd),
      nextCursor: source.length,
      tail: source.slice(safeEnd)
    };
  };
  const push = (chunk, flush = false) => {
    const source = `${bufferedTail}${chunk || ''}`;
    bufferedTail = '';
    if (!source) {
      return { content: '', reasoning: '' };
    }
    let contentDelta = '';
    let reasoningDelta = '';
    let cursor = 0;
    while (cursor < source.length) {
      const marker = inThinkTag ? THINK_CLOSE_TAG : THINK_OPEN_TAG;
      const markerIndex = source.indexOf(marker, cursor);
      if (markerIndex < 0) {
        if (flush) {
          const tail = source.slice(cursor);
          if (tail) {
            if (inThinkTag) {
              reasoningDelta += tail;
            } else {
              contentDelta += tail;
            }
          }
          cursor = source.length;
        } else {
          const { stable, nextCursor, tail } = takeStableText(source, cursor, marker);
          if (stable) {
            if (inThinkTag) {
              reasoningDelta += stable;
            } else {
              contentDelta += stable;
            }
          }
          bufferedTail = tail;
          cursor = nextCursor;
        }
        continue;
      }
      const stable = source.slice(cursor, markerIndex);
      if (stable) {
        if (inThinkTag) {
          reasoningDelta += stable;
        } else {
          contentDelta += stable;
        }
      }
      const markerEnd = markerIndex + marker.length;
      cursor = markerEnd;
      inThinkTag = !inThinkTag;
    }
    return {
      content: contentDelta,
      reasoning: reasoningDelta
    };
  };
  const reset = () => {
    inThinkTag = false;
    bufferedTail = '';
  };
  return { push, reset };
};

const normalizeToolNameForFinal = (name) => {
  const raw = String(name || '').trim();
  if (!raw) return '';
  if (raw === '最终回复') return raw;
  return raw.toLowerCase().replace(/[\s-]+/g, '_');
};

const isFinalToolName = (name) => {
  const normalized = normalizeToolNameForFinal(name);
  return (
    normalized === '最终回复' ||
    normalized === 'final_response' ||
    normalized === 'final' ||
    normalized === 'final_answer'
  );
};

const normalizeToolCallsPayload = (toolCalls) => {
  if (!toolCalls) return [];
  let payload = toolCalls;
  if (typeof payload === 'string') {
    const parsed = safeJsonParse(payload);
    if (parsed !== null) {
      payload = parsed;
    }
  }
  if (Array.isArray(payload)) return payload;
  if (payload && typeof payload === 'object') {
    if (Array.isArray(payload.tool_calls)) return payload.tool_calls;
    if (payload.tool_calls) return [payload.tool_calls];
    if (payload.tool_call) return [payload.tool_call];
    if (payload.function_call) return [payload.function_call];
    return [payload];
  }
  return [];
};

const parseToolCallArgs = (value) => {
  if (value === null || value === undefined) return null;
  if (typeof value === 'string') {
    const parsed = safeJsonParse(value);
    return parsed !== null ? parsed : value;
  }
  if (typeof value === 'object') return value;
  return String(value);
};

const extractFinalAnswerFromToolCalls = (toolCalls) => {
  const calls = normalizeToolCallsPayload(toolCalls);
  for (const call of calls) {
    if (!call || typeof call !== 'object') continue;
    const functionPayload = call.function || call;
    const name = functionPayload.name || call.name || call.tool;
    if (!isFinalToolName(name)) continue;
    const argsRaw =
      functionPayload.arguments ??
      call.arguments ??
      call.args ??
      functionPayload.args ??
      functionPayload.parameters ??
      call.parameters;
    const args = parseToolCallArgs(argsRaw);
    if (typeof args === 'string') {
      const text = args.trim();
      if (text) return text;
      continue;
    }
    if (args && typeof args === 'object') {
      const answer = args.content ?? args.answer ?? args.message;
      if (answer !== undefined && answer !== null) {
        const text = String(answer).trim();
        if (text) return text;
      }
    }
  }
  return '';
};

const buildWorkflowEventRaw = (data, timestamp = undefined) => {
  const payload: WorkflowEventRawPayload = { data: data ?? null };
  if (timestamp) {
    payload.timestamp = timestamp;
  }
  return JSON.stringify(payload);
};

const normalizeWorkflowEvents = (events, message) => {
  if (!Array.isArray(events) || events.length === 0) {
    return [];
  }
  const normalizedOutput = normalizeAssistantOutput(
    message?.content || '',
    resolveAssistantReasoning(message)
  );
  const content = normalizedOutput.content;
  const reasoning = normalizedOutput.reasoning;
  const normalized = [];
  events.forEach((event) => {
    const eventName = String(event?.event || '').trim();
    if (!eventName || eventName === 'final') {
      return;
    }
    let data = event?.data ?? null;
    if (eventName === 'llm_output' && (content || reasoning)) {
      if (data && typeof data === 'object' && !Array.isArray(data)) {
        data = { ...data, content, reasoning };
      } else {
        data = { content, reasoning };
      }
    }
    normalized.push({
      event: eventName,
      raw: buildWorkflowEventRaw(data, event?.timestamp)
    });
  });
  if (content || reasoning) {
    normalized.push({
      event: 'final',
      raw: buildWorkflowEventRaw({ answer: content, content, reasoning })
    });
  }
  return normalized;
};

const attachWorkflowEvents = (messages, rounds) => {
  if (!Array.isArray(messages) || !Array.isArray(rounds) || rounds.length === 0) {
    return messages;
  }
  const roundMap = new Map();
  rounds.forEach((round) => {
    const roundIndex = Number(round?.user_round ?? round?.round);
    if (!Number.isFinite(roundIndex)) return;
    const events = Array.isArray(round?.events) ? round.events : [];
    if (events.length) {
      roundMap.set(roundIndex, events);
    }
  });
  if (!roundMap.size) {
    return messages;
  }
  let currentRound = 0;
  let lastAssistantIndex = null;
  const assignRound = () => {
    if (!Number.isFinite(currentRound) || currentRound <= 0 || lastAssistantIndex === null) {
      return;
    }
    const events = roundMap.get(currentRound);
    if (!events || events.length === 0) {
      return;
    }
    const target = messages[lastAssistantIndex];
    target.workflow_events = normalizeWorkflowEvents(events, target);
  };
  messages.forEach((message, index) => {
    if (message?.role === 'user') {
      assignRound();
      currentRound += 1;
      lastAssistantIndex = null;
      return;
    }
    if (message?.role === 'assistant') {
      lastAssistantIndex = index;
    }
  });
  assignRound();
  return messages;
};

const isFailedResult = (payload) => {
  const status = payload?.data?.status ?? payload?.status;
  if (status && String(status).toLowerCase() === 'failed') {
    return true;
  }
  return Boolean(payload?.data?.error || payload?.error);
};

const normalizeToolCategory = (value) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) return '';
  if (normalized.includes('builtin') || normalized.includes('built-in') || normalized.includes('built_in')) {
    return 'builtin';
  }
  if (normalized.includes('user')) {
    return 'user';
  }
  if (normalized.includes('shared')) {
    return 'shared';
  }
  if (normalized.includes('knowledge') || normalized.includes('knowledge_base') || normalized.includes('knowledgebase')) {
    return 'knowledge';
  }
  if (normalized.includes('skill')) {
    return 'skill';
  }
  if (normalized.includes('mcp')) {
    return 'mcp';
  }
  if (normalized.includes('default') || normalized === 'tool') {
    return 'default';
  }
  return '';
};

// 根据工具名称与事件字段推断分类，用于工作流高亮
const resolveToolCategory = (toolName, payload) => {
  const explicit = normalizeToolCategory(
    payload?.category ??
      payload?.tool_category ??
      payload?.toolCategory ??
      payload?.tool_type ??
      payload?.toolType
  );
  if (explicit) return explicit;
  const name = String(toolName || '').trim();
  if (!name) return 'default';
  if (name.includes('@')) return 'mcp';
  const lowerName = name.toLowerCase();
  if (lowerName.includes('mcp')) return 'mcp';
  if (lowerName.includes('knowledge') || lowerName.startsWith('kb_') || name.includes('知识')) {
    return 'knowledge';
  }
  if (lowerName.includes('skill') || name.includes('技能')) return 'skill';
  if (lowerName.includes('builtin') || lowerName.includes('built-in') || lowerName.includes('system')) {
    return 'builtin';
  }
  return 'default';
};

const sessionRuntime = new Map();
const sessionMessages = new Map();
const sessionListCache = new Map();
const sessionListCacheInFlight = new Map();
const sessionDetailPrefetchInFlight = new Map();
const sessionDetailWarmState = new Map();

const SESSION_LIST_CACHE_TTL_MS = 15 * 1000;
const SESSION_DETAIL_WARM_TTL_MS = 20 * 1000;

const resolveSessionKey = (sessionId) => String(sessionId || '').trim();

const cloneSerializable = (value, fallback) => {
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

const cloneSessionList = (sessions) => {
  const cloned = cloneSerializable(Array.isArray(sessions) ? sessions : [], []);
  return Array.isArray(cloned) ? cloned : [];
};

const filterSessionsByAgent = (agentId, sourceSessions = []) => {
  const normalizedAgentId = String(agentId || '').trim();
  return (Array.isArray(sourceSessions) ? sourceSessions : []).filter((session) => {
    const sessionAgentId = String(session?.agent_id || '').trim();
    return normalizedAgentId ? sessionAgentId === normalizedAgentId : !sessionAgentId;
  });
};

const resolveInitialSessionIdFromList = (agentId, sourceSessions = []) => {
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

const resolveSessionListCacheKey = (agentId) => normalizeAgentKey(agentId);

const readSessionListCache = (agentId) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  const cached = sessionListCache.get(cacheKey);
  if (!cached) return null;
  if (!Number.isFinite(cached.cachedAt) || Date.now() - cached.cachedAt > SESSION_LIST_CACHE_TTL_MS) {
    sessionListCache.delete(cacheKey);
    return null;
  }
  return cloneSessionList(cached.sessions);
};

const writeSessionListCache = (agentId, sessions) => {
  const cacheKey = resolveSessionListCacheKey(agentId);
  sessionListCache.set(cacheKey, {
    cachedAt: Date.now(),
    sessions: cloneSessionList(sessions)
  });
};

const markSessionDetailWarm = (sessionId) => {
  const sessionKey = resolveSessionKey(sessionId);
  if (!sessionKey) return;
  sessionDetailWarmState.set(sessionKey, Date.now() + SESSION_DETAIL_WARM_TTL_MS);
};

const isSessionDetailWarm = (sessionId) => {
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

const ensureRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  if (!sessionRuntime.has(key)) {
    sessionRuntime.set(key, {
      sendController: null,
      resumeController: null,
      sendRequestId: null,
      resumeRequestId: null,
      watchController: null,
      watchRequestId: null,
      stopRequested: false,
      lastEventId: 0
    });
  }
  return sessionRuntime.get(key);
};

const getRuntime = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionRuntime.get(key) || null;
};

const getSessionMessages = (sessionId) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return null;
  return sessionMessages.get(key) || null;
};

const cacheSessionMessages = (sessionId, messages) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  sessionMessages.set(key, messages);
};

const touchSessionUpdatedAt = (store, sessionId, timestamp) => {
  if (!store || !Array.isArray(store.sessions)) return;
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  const session = store.sessions.find((item) => String(item?.id || '').trim() === key);
  if (!session) return;
  const resolved = resolveTimestampIso(timestamp);
  session.updated_at = resolved || new Date().toISOString();
};

const notifySessionSnapshot = (store, sessionId, messages, immediate = false) => {
  const key = resolveSessionKey(sessionId);
  if (!key || !Array.isArray(messages)) return;
  cacheSessionMessages(key, messages);
  const activeKey = resolveSessionKey(store?.activeSessionId);
  if (activeKey && activeKey === key) {
    scheduleChatSnapshot(store, immediate);
  }
};

const shouldPreferCachedMessages = (cached, server) => {
  if (!Array.isArray(cached) || cached.length === 0) return false;
  if (!Array.isArray(server) || server.length === 0) return true;
  if (cached.some((message) => message?.stream_incomplete || message?.workflowStreaming)) {
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

const setSessionLoading = (store, sessionId, value) => {
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  if (value) {
    store.loadingBySession[key] = true;
  } else if (store.loadingBySession[key]) {
    delete store.loadingBySession[key];
  }
};

let sessionWatchSessionId = '';

const abortWatchStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.watchController) {
    runtime.watchController.abort();
    runtime.watchController = null;
  }
  runtime.watchRequestId = null;
};

const clearSessionWatcher = () => {
  if (sessionWatchSessionId) {
    abortWatchStream(sessionWatchSessionId);
  }
  sessionWatchSessionId = '';
};

const resolveMaxStreamEventId = (messages) => {
  if (!Array.isArray(messages)) return null;
  let maxId = 0;
  messages.forEach((message) => {
    if (message?.role !== 'assistant') return;
    const eventId = normalizeStreamEventId(message.stream_event_id);
    if (eventId && eventId > maxId) {
      maxId = eventId;
    }
  });
  return maxId > 0 ? maxId : null;
};

const findPendingAssistantMessage = (messages) =>
  Array.isArray(messages)
    ? [...messages]
        .reverse()
        .find((message) => message?.role === 'assistant' && normalizeFlag(message.stream_incomplete))
    : null;

const findAssistantMessageByRound = (messages, roundNumber) => {
  if (!Array.isArray(messages) || !Number.isFinite(roundNumber) || roundNumber <= 0) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    if (normalizeStreamRound(message.stream_round) === roundNumber) {
      return message;
    }
  }
  return null;
};

const ensurePendingAssistantMessage = (store, sessionId, messages, baseEventId) => {
  if (!Array.isArray(messages)) return null;
  const existing = findPendingAssistantMessage(messages);
  if (existing) return existing;
  const placeholder = {
    ...buildMessage('assistant', ''),
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

const resolveLastAssistantTimestampMs = (messages) => {
  if (!Array.isArray(messages)) return null;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'assistant') continue;
    const timestamp = resolveTimestampMs(message.created_at);
    return Number.isFinite(timestamp) ? timestamp : null;
  }
  return null;
};

const WATCH_USER_MESSAGE_DEDUP_MS = 2000;

const shouldInsertWatchUserMessage = (messages, content, eventTimestampMs) => {
  if (!Array.isArray(messages) || !content) return false;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message?.role !== 'user') continue;
    const lastContent = String(message.content || '');
    if (lastContent !== content) {
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

const hasAnchoredWatchUserMessage = (messages, anchor, content) => {
  if (!Array.isArray(messages) || !anchor || !content) return false;
  const anchorIndex = messages.indexOf(anchor);
  if (anchorIndex <= 0) return false;
  const previous = messages[anchorIndex - 1];
  if (previous?.role !== 'user') return false;
  return String(previous.content || '') === content;
};

const insertWatchUserMessage = (store, sessionId, messages, content, eventTimestampMs, anchor) => {
  if (hasAnchoredWatchUserMessage(messages, anchor, content)) {
    return;
  }
  if (!shouldInsertWatchUserMessage(messages, content, eventTimestampMs)) {
    return;
  }
  const createdAt = Number.isFinite(eventTimestampMs)
    ? new Date(eventTimestampMs).toISOString()
    : undefined;
  const userMessage = buildMessage('user', content, createdAt);
  const anchorIndex = anchor ? messages.indexOf(anchor) : -1;
  if (anchorIndex >= 0) {
    messages.splice(anchorIndex, 0, userMessage);
  } else {
    messages.push(userMessage);
  }
  touchSessionUpdatedAt(store, sessionId, eventTimestampMs ?? Date.now());
  notifySessionSnapshot(store, sessionId, messages, true);
};

const startSessionWatcher = (store, sessionId) => {
  clearSessionWatcher();
  const key = resolveSessionKey(sessionId);
  if (!key) return;
  sessionWatchSessionId = key;
  const runtime = ensureRuntime(key);
  if (!runtime || runtime.sendController || runtime.resumeController) return;
  runtime.watchController = new AbortController();
  const controller = runtime.watchController;
  const requestId = buildWsRequestId();
  runtime.watchRequestId = requestId;
  const sessionMessagesRef = getSessionMessages(key) || store.messages;
  cacheSessionMessages(key, sessionMessagesRef);
  const workflowState = getSessionWorkflowState(key);
  const roundStates = new Map();
  const completedRounds = new Set();
  let lastEventId = Math.max(
    resolveMaxStreamEventId(sessionMessagesRef) || 0,
    getRuntimeLastEventId(runtime)
  );
  const minEventTimestampMs =
    lastEventId > 0 ? null : resolveLastAssistantTimestampMs(sessionMessagesRef);

  const ensureRoundState = (roundNumber, eventTimestampMs) => {
    if (!Number.isFinite(roundNumber) || roundNumber <= 0) return null;
    if (completedRounds.has(roundNumber)) return null;
    const existing = roundStates.get(roundNumber);
    if (existing) return existing;
    const candidate =
      findAssistantMessageByRound(sessionMessagesRef, roundNumber) ||
      findPendingAssistantMessage(sessionMessagesRef);
    if (candidate) {
      const assignedRound = normalizeStreamRound(candidate.stream_round);
      const alreadyTracked = Array.from(roundStates.values()).some((entry) => entry.message === candidate);
      if (!alreadyTracked && (assignedRound === null || assignedRound === roundNumber)) {
        if (assignedRound === null) {
          candidate.stream_round = roundNumber;
        }
        if (!candidate.created_at && Number.isFinite(eventTimestampMs)) {
          candidate.created_at = new Date(eventTimestampMs).toISOString();
        }
        candidate.workflowStreaming = true;
        candidate.stream_incomplete = true;
        const processor = createWorkflowProcessor(
          candidate,
          workflowState,
          () => notifySessionSnapshot(store, key, sessionMessagesRef)
        );
        const state = { message: candidate, processor, userInserted: false };
        roundStates.set(roundNumber, state);
        return state;
      }
    }
    const createdAt = Number.isFinite(eventTimestampMs)
      ? new Date(eventTimestampMs).toISOString()
      : undefined;
    const assistantMessage = {
      ...buildMessage('assistant', '', createdAt),
      workflowItems: [],
      workflowStreaming: true,
      stream_incomplete: true,
      stream_event_id: lastEventId || 0,
      stream_round: roundNumber
    };
    sessionMessagesRef.push(assistantMessage);
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    const processor = createWorkflowProcessor(
      assistantMessage,
      workflowState,
      () => notifySessionSnapshot(store, key, sessionMessagesRef)
    );
    const state = { message: assistantMessage, processor, userInserted: false };
    roundStates.set(roundNumber, state);
    return state;
  };

  const finalizeRound = (roundNumber, aborted) => {
    const state = roundStates.get(roundNumber);
    if (!state) return;
    state.processor.finalize();
    if (!aborted) {
      state.message.stream_incomplete = false;
      state.message.workflowStreaming = false;
    }
    notifySessionSnapshot(store, key, sessionMessagesRef, true);
    roundStates.delete(roundNumber);
    completedRounds.add(roundNumber);
  };

  const finalizeAll = (aborted) => {
    Array.from(roundStates.keys()).forEach((round) => finalizeRound(round, aborted));
  };

  const onEvent = (eventType, dataText, eventId) => {
    if (runtime.sendController || runtime.resumeController) {
      return;
    }
    const payload = safeJsonParse(dataText);
    const data = payload?.data ?? payload;
    handleApprovalEvent(store, eventType, data, requestId, key);
    if (eventType === 'slow_client' && !data) {
      return;
    }
    const normalizedEventId = normalizeStreamEventId(eventId);
    if (normalizedEventId !== null) {
      if (normalizedEventId <= lastEventId) {
        return;
      }
      lastEventId = normalizedEventId;
      updateRuntimeLastEventId(runtime, normalizedEventId);
    }
    const eventTimestampMs = resolveTimestampMs(payload?.timestamp ?? data?.timestamp);
    if (
      Number.isFinite(minEventTimestampMs) &&
      Number.isFinite(eventTimestampMs) &&
      eventTimestampMs <= minEventTimestampMs
    ) {
      return;
    }
    const roundNumber = resolveEventRoundNumber(payload, data);
    const stage = data?.stage ?? payload?.stage;
    const isRoundStart = eventType === 'round_start' || (eventType === 'progress' && stage === 'start');
    const state = ensureRoundState(roundNumber, eventTimestampMs);
    if (isRoundStart && state) {
      const question =
        data?.question ??
        payload?.question ??
        data?.message ??
        payload?.message ??
        '';
      if (!state.userInserted && typeof question === 'string' && question.trim()) {
        insertWatchUserMessage(
          store,
          key,
          sessionMessagesRef,
          question.trim(),
          eventTimestampMs,
          state.message
        );
        state.userInserted = true;
      }
    }
    if (!state) return;
    state.message.workflowStreaming = true;
    state.message.stream_incomplete = true;
    assignStreamEventId(state.message, eventId);
    state.processor.handleEvent(eventType, dataText);
    if (eventType === 'final' || eventType === 'error') {
      finalizeRound(roundNumber, false);
    }
  };

  const baseEventId = lastEventId || 0;
  const watchWithSse = async () => {
    // SSE watch may complete on idle; reconnect to keep behavior close to WS watch mode.
    while (!controller.signal.aborted) {
      const response = await resumeMessageStream(key, {
        signal: controller.signal,
        afterEventId: lastEventId || baseEventId
      });
      if (!response.ok) {
        const errorText = await readResponseError(response);
        throw new Error(errorText || t('chat.error.resumeFailedWithStatus', { status: response.status }));
      }
      await consumeSseStream(response, onEvent);
      if (controller.signal.aborted) {
        break;
      }
      await new Promise((resolve) => setTimeout(resolve, 120));
    }
  };
  const watchWithWs = () =>
    chatWsClient.request({
      requestId,
      sessionId: key,
      message: {
        type: 'watch',
        request_id: requestId,
        session_id: key,
        payload: { after_event_id: baseEventId }
      },
      onEvent,
      signal: controller.signal,
      closeOnFinal: false
    });
  const preferredTransport = resolveStreamTransport();
  store.streamTransport = preferredTransport;
  const watchPromise =
    preferredTransport === 'ws'
      ? watchWithWs().catch((error) => {
          if (error?.phase === 'connect') {
            markWsUnavailable();
            store.streamTransport = 'sse';
            return watchWithSse();
          }
          throw error;
        })
      : watchWithSse();
  watchPromise.catch((error) => {
    store.clearPendingApprovals({ requestId, sessionId: key });
    if (error?.name === 'AbortError' || error?.phase === 'aborted') {
      finalizeAll(true);
      return;
    }
    finalizeAll(false);
  });
};

const DEFAULT_STREAM_TRANSPORT = 'ws';
const chatWsClient = createWsMultiplexer(() => openChatSocket(), {
  idleTimeoutMs: 30000,
  connectTimeoutMs: 10000
});
let wsUnavailableUntil = 0;
let wsRequestSeq = 0;

const buildWsRequestId = () => {
  wsRequestSeq = (wsRequestSeq + 1) % 1000000;
  return `req_${Date.now().toString(36)}_${wsRequestSeq}`;
};

const markWsUnavailable = (ttlMs = 60000) => {
  const now = Date.now();
  wsUnavailableUntil = now + Math.max(ttlMs, 5000);
};

const resolveStreamTransport = () => {
  if (typeof WebSocket === 'undefined') {
    return 'sse';
  }
  if (wsUnavailableUntil && Date.now() < wsUnavailableUntil) {
    return 'sse';
  }
  const stored = localStorage.getItem('chat_stream_transport');
  if (stored === 'sse' || stored === 'ws') {
    return stored;
  }
  return DEFAULT_STREAM_TRANSPORT;
};

const abortResumeStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.resumeController) {
    runtime.resumeController.abort();
    runtime.resumeController = null;
  }
  runtime.resumeRequestId = null;
};

const abortSendStream = (sessionId) => {
  const runtime = getRuntime(sessionId);
  if (!runtime) return;
  if (runtime.sendController) {
    runtime.sendController.abort();
    runtime.sendController = null;
  }
  runtime.sendRequestId = null;
};

const createWorkflowProcessor = (assistantMessage, workflowState, onSnapshot, options: WorkflowProcessorOptions = {}) => {
  const roundState = normalizeSessionWorkflowState(workflowState);
  const toolItemMap = new Map();
  const approvalItemMap = new Map();
  const toolOutputItemMap = new Map();
  const toolOutputBufferMap = new Map();
  let outputItemId = null;
  const blockedRounds = new Set();
  let lastRound = null;
  const initialRound = normalizeStreamRound(assistantMessage.stream_round);
  let visibleRound = initialRound;
  // 参照调试面板：记录模型输出轮次与内容，方便还原事件日志
  const outputState = {
    streaming: false,
    reasoningStreaming: false
  };
  const finalizeWithNow = options.finalizeWithNow !== false;
  const initialReasoning = resolveAssistantReasoning(assistantMessage);
  const normalizedOutput = normalizeAssistantOutput(
    assistantMessage.content,
    initialReasoning
  );
  const hasExplicitInitialReasoning = initialReasoning !== '';
  assistantMessage.content = normalizedOutput.content;
  // 思考内容需要同步到消息头部展示
  assistantMessage.reasoning = normalizedOutput.reasoning;
  assistantMessage.reasoningStreaming = normalizeFlag(assistantMessage.reasoningStreaming);
  const normalizedPlan = normalizePlanPayload(assistantMessage.plan);
  assistantMessage.plan = normalizedPlan;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalizedPlan, assistantMessage);
  assistantMessage.questionPanel = normalizeInquiryPanelState(assistantMessage.questionPanel);
  const stats = ensureMessageStats(assistantMessage);
  const refreshInteractionDuration = () => {
    if (!stats) return;
    const duration = resolveInteractionDuration(
      stats.interaction_start_ms,
      stats.interaction_end_ms
    );
    if (duration !== null) {
      stats.interaction_duration_s = duration;
    }
  };
  const applyInteractionTimestamp = (timestamp) => {
    if (!stats) return;
    const millis = resolveTimestampMs(timestamp);
    if (!Number.isFinite(millis)) {
      return;
    }
    if (!Number.isFinite(stats.interaction_start_ms)) {
      stats.interaction_start_ms = millis;
    } else {
      stats.interaction_start_ms = Math.min(stats.interaction_start_ms, millis);
    }
    if (!Number.isFinite(stats.interaction_end_ms)) {
      stats.interaction_end_ms = millis;
    } else {
      stats.interaction_end_ms = Math.max(stats.interaction_end_ms, millis);
    }
    refreshInteractionDuration();
  };
  const ensureInteractionStart = () => {
    if (!stats) return;
    if (!Number.isFinite(stats.interaction_start_ms)) {
      stats.interaction_start_ms = Date.now();
    }
  };
  const finalizeInteractionDuration = () => {
    if (!stats) return;
    if (!finalizeWithNow) {
      refreshInteractionDuration();
      return;
    }
    ensureInteractionStart();
    const now = Date.now();
    if (!Number.isFinite(stats.interaction_end_ms)) {
      stats.interaction_end_ms = now;
    } else {
      stats.interaction_end_ms = Math.max(stats.interaction_end_ms, now);
    }
    refreshInteractionDuration();
  };
  let outputContent = assistantMessage.content || '';
  let outputReasoningExplicit = hasExplicitInitialReasoning ? initialReasoning : '';
  let outputReasoningFallback = hasExplicitInitialReasoning
    ? normalizeReasoningText(normalizedOutput.inlineReasoning)
    : normalizeReasoningText(normalizedOutput.reasoning);
  const existingOutput = assistantMessage.workflowItems?.find((item) => item.title === '模型输出');
  if (existingOutput) {
    outputItemId = existingOutput.id;
  }

  const resolveReasoningOutput = () => outputReasoningExplicit || outputReasoningFallback;

  const syncReasoningToMessage = () => {
    assistantMessage.reasoning = resolveReasoningOutput();
    assistantMessage.reasoningStreaming = outputState.reasoningStreaming;
  };

  const normalizeToolName = (title) => {
    if (!title) return '';
    if (title.startsWith('调用工具：')) {
      return title.replace('调用工具：', '').trim();
    }
    return '';
  };

  const registerToolStats = (toolName) => {
    if (!stats) return;
    stats.toolCalls = normalizeStatsCount(stats.toolCalls) + 1;
  };

  const updateUsageStats = (usagePayload, prefillDuration, decodeDuration, options: UsageStatsOptions = {}) => {
    if (!stats) return;
    const normalizedUsage = normalizeUsagePayload(usagePayload);
    if (normalizedUsage && options.updateUsage !== false) {
      stats.usage = normalizedUsage;
    }
    const prefill = normalizeDurationValue(prefillDuration);
    if (prefill !== null) {
      stats.prefill_duration_s = prefill;
    }
    const decode = normalizeDurationValue(decodeDuration);
    if (decode !== null) {
      stats.decode_duration_s = decode;
    }
  };

  const updateQuotaUsage = (payload) => {
    if (!stats) return;
    const rawIncrement =
      payload && typeof payload === 'object' ? payload.consumed ?? payload.count ?? payload.used : null;
    const increment = normalizeStatsCount(rawIncrement);
    const delta = increment > 0 ? increment : 1;
    stats.quotaConsumed = normalizeStatsCount(stats.quotaConsumed) + delta;
    const snapshot = normalizeQuotaSnapshot(payload);
    if (snapshot) {
      stats.quotaSnapshot = snapshot;
    }
  };

  const updateContextUsage = (payload) => {
    if (!stats) return;
    const contextTokens = normalizeContextTokens(
      payload?.context_tokens ??
        payload?.contextTokens ??
        payload?.context ??
        payload?.contextUsage ??
        payload?.context_usage?.context_tokens ??
        payload?.context_usage?.contextTokens
    );
    if (contextTokens !== null) {
      stats.contextTokens = contextTokens;
    }
  };

  const registerToolItem = (toolName, itemId) => {
    if (!toolName || !itemId) return;
    if (!toolItemMap.has(toolName)) {
      toolItemMap.set(toolName, []);
    }
    toolItemMap.get(toolName).push(itemId);
  };

  const resolveToolItemId = (toolName) => {
    if (!toolName) return null;
    const queue = toolItemMap.get(toolName);
    if (!queue || queue.length === 0) return null;
    return queue.shift() || null;
  };

  const peekToolItemId = (toolName) => {
    if (!toolName) return null;
    const queue = toolItemMap.get(toolName);
    if (!queue || queue.length === 0) return null;
    return queue[0] || null;
  };

  const resolveToolOutputKey = (toolName, callId) => {
    if (callId) return `call:${callId}`;
    if (toolName) return `tool:${toolName}`;
    return 'tool:unknown';
  };

  const getToolOutputBuffer = (key) => {
    let buffer = toolOutputBufferMap.get(key);
    if (!buffer) {
      buffer = { stdout: '', stderr: '', command: '' };
      toolOutputBufferMap.set(key, buffer);
    }
    return buffer;
  };

  const buildToolOutputDetail = (buffer) => {
    if (!buffer) return '';
    const parts = [];
    if (buffer.command) {
      parts.push(`[command]\n${buffer.command}`);
    }
    if (buffer.stdout) {
      parts.push(`[stdout]\n${buffer.stdout}`);
    }
    if (buffer.stderr) {
      parts.push(`[stderr]\n${buffer.stderr}`);
    }
    return parts.join('\n\n');
  };

  const ensureToolOutputItem = (toolName, key, toolCategory) => {
    if (!key) return null;
    const existing = toolOutputItemMap.get(key);
    if (existing) return existing;
    const title = toolName ? `工具输出：${toolName}` : '工具输出';
    const item = buildWorkflowItem(title, '', 'loading', {
      isTool: true,
      toolCategory
    });
    assistantMessage.workflowItems.push(item);
    toolOutputItemMap.set(key, item.id);
    return item.id;
  };

  const finalizeToolOutputItem = (key, failed) => {
    if (!key) return;
    const itemId = toolOutputItemMap.get(key);
    if (!itemId) return;
    const buffer = toolOutputBufferMap.get(key);
    updateWorkflowItem(assistantMessage.workflowItems, itemId, {
      status: failed ? 'failed' : 'completed',
      detail: buffer ? buildToolOutputDetail(buffer) : ''
    });
    toolOutputItemMap.delete(key);
    toolOutputBufferMap.delete(key);
  };

  const updateRoundState = (roundNumber) => {
    if (!Number.isFinite(roundNumber)) {
      return;
    }
    if (!Number.isFinite(roundState.globalRound) || roundState.globalRound < roundNumber) {
      roundState.globalRound = roundNumber;
    }
    roundState.currentRound = roundNumber;
  };

  const resolveRound = (payload, data) => {
    const roundNumber = resolveEventRoundNumber(payload, data);
    if (roundNumber !== null) {
      updateRoundState(roundNumber);
      return roundNumber;
    }
    return Number.isFinite(roundState.currentRound) ? roundState.currentRound : null;
  };

  const advanceModelRound = () => {
    const nextRound = (Number.isFinite(roundState.globalRound) ? roundState.globalRound : 0) + 1;
    updateRoundState(nextRound);
    return nextRound;
  };

  const buildOutputDetail = () => {
    const parts = [];
    const reasoningText = resolveReasoningOutput();
    if (reasoningText) {
      parts.push(`[${t('chat.workflow.detail.reasoning')}]\n${tailText(reasoningText)}`);
    }
    if (outputContent) {
      parts.push(`[${t('chat.workflow.detail.output')}]\n${tailText(outputContent)}`);
    }
    if (!parts.length) {
      return tailText(assistantMessage.content || '');
    }
    return parts.join('\n\n');
  };

  let pendingContent = '';
  let pendingReasoningExplicit = '';
  let pendingReasoningFallback = '';
  const thinkStreamParser = createThinkTagStreamParser();
  let streamTimer = null;
  const scheduleFrame =
    typeof requestAnimationFrame === 'function'
      ? requestAnimationFrame
      : (callback) => setTimeout(callback, 16);
  const cancelFrame = typeof cancelAnimationFrame === 'function' ? cancelAnimationFrame : clearTimeout;

  const flushStream = (force = false) => {
    if (streamTimer !== null) {
      cancelFrame(streamTimer);
      streamTimer = null;
    }
    if (force) {
      const trailingDelta = thinkStreamParser.push('', true);
      if (trailingDelta.content) {
        pendingContent += trailingDelta.content;
      }
      if (trailingDelta.reasoning) {
        pendingReasoningFallback += trailingDelta.reasoning;
      }
    }
    const hasContentDelta = Boolean(pendingContent);
    const hasReasoningDelta = Boolean(
      pendingReasoningExplicit || pendingReasoningFallback
    );
    if (!hasContentDelta && !hasReasoningDelta && !force) {
      return;
    }
    if (pendingReasoningExplicit) {
      outputReasoningExplicit += pendingReasoningExplicit;
      pendingReasoningExplicit = '';
      outputState.reasoningStreaming = true;
    }
    if (pendingReasoningFallback) {
      outputReasoningFallback += pendingReasoningFallback;
      pendingReasoningFallback = '';
      outputState.reasoningStreaming = true;
    }
    if (hasContentDelta) {
      outputContent += pendingContent;
      pendingContent = '';
      assistantMessage.content = outputContent;
      outputState.streaming = true;
    }
    syncReasoningToMessage();
    if (hasContentDelta || hasReasoningDelta) {
      const outputId = ensureOutputItem();
      updateWorkflowItem(assistantMessage.workflowItems, outputId, {
        detail: buildOutputDetail()
      });
    }
    if (hasContentDelta || hasReasoningDelta) {
      notifySnapshot();
    }
  };

  const scheduleStreamFlush = () => {
    if (streamTimer !== null) return;
    streamTimer = scheduleFrame(() => {
      streamTimer = null;
      flushStream();
    });
  };

  const resetStreamPending = () => {
    if (streamTimer !== null) {
      cancelFrame(streamTimer);
      streamTimer = null;
    }
    pendingContent = '';
    pendingReasoningExplicit = '';
    pendingReasoningFallback = '';
    thinkStreamParser.reset();
  };

  const notifySnapshot = () => {
    if (typeof onSnapshot === 'function') {
      onSnapshot();
    }
  };

  ensureInteractionStart();

  const clearVisibleOutput = () => {
    resetStreamPending();
    assistantMessage.content = '';
    outputContent = '';
    outputReasoningExplicit = '';
    outputReasoningFallback = '';
    outputState.streaming = false;
    outputState.reasoningStreaming = false;
    assistantMessage.stream_round = null;
    syncReasoningToMessage();
    if (outputItemId) {
      updateWorkflowItem(assistantMessage.workflowItems, outputItemId, {
        detail: '',
        status: 'loading'
      });
    }
    visibleRound = null;
  };

  // 续传时需要把已有的工具调用记录挂载到映射，避免结果无法回填
  if (Array.isArray(assistantMessage.workflowItems)) {
    assistantMessage.workflowItems.forEach((item) => {
      const toolName = normalizeToolName(item?.title);
      if (toolName && item?.status === 'loading') {
        registerToolItem(toolName, item.id);
      }
    });
  }
  if (initialRound !== null) {
    updateRoundState(initialRound);
  }

  const ensureOutputItem = () => {
    if (!outputItemId) {
      const item = buildWorkflowItem('模型输出', '', 'loading');
      outputItemId = item.id;
      assistantMessage.workflowItems.push(item);
    }
    return outputItemId;
  };

  const applyQuestionPanelPayload = (payload, options: QuestionPanelApplyOptions = {}) => {
    const normalized = normalizeInquiryPanelPayload(payload);
    if (!normalized) return false;
    assistantMessage.questionPanel = {
      ...normalized,
      status: 'pending',
      selected: []
    };
    if (options.appendWorkflow !== false) {
      assistantMessage.workflowItems.push(
        buildWorkflowItem('问询面板', buildDetail(normalized))
      );
    }
    return true;
  };

  const handleEvent = (eventName, raw) => {
    const payload = safeJsonParse(raw);
    const data = payload?.data ?? payload;
    const eventType = resolveEventType(eventName, payload);
    applyInteractionTimestamp(payload?.timestamp ?? data?.timestamp);

    // 基于事件类型生成工作流条目并更新回复内容
    switch (eventType) {
      case 'progress': {
        const stage = data?.stage ?? payload?.stage;
        let summary = data?.summary ?? payload?.summary;
        let detailSource = data;
        if (stage === 'llm_call') {
          const roundNumber = advanceModelRound();
          summary = `调用模型（第 ${roundNumber} 轮）`;
          lastRound = roundNumber;
          // 保持详情中的轮次与会话累计轮次一致
          if (data && typeof data === 'object') {
            detailSource = { ...data, round: roundNumber };
          } else {
            detailSource = { stage, summary: data?.summary ?? payload?.summary ?? '调用模型', round: roundNumber };
          }
        }
        const showStage = stage && !['received', 'llm_call'].includes(stage);
        const title = summary ? pickText(summary) : showStage ? `阶段：${stage}` : '进度更新';
        assistantMessage.workflowItems.push(
          buildWorkflowItem(title, buildDetail(detailSource))
        );
        break;
      }
      case 'llm_request': {
        const hasPayload = data && typeof data === 'object' && 'payload' in data;
        const hasSummary = data && typeof data === 'object' && 'payload_summary' in data;
        const title = hasSummary && !hasPayload ? '模型请求摘要' : '模型请求体';
        assistantMessage.workflowItems.push(buildWorkflowItem(title, buildDetail(data)));
        break;
      }
      case 'knowledge_request': {
        const base = data?.knowledge_base ?? data?.knowledgeBase ?? '';
        const title = base ? `知识库请求体（${base}）` : '知识库请求体';
        assistantMessage.workflowItems.push(buildWorkflowItem(title, buildDetail(data)));
        break;
      }
      case 'tool_call': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '未知工具';
        const detailSource = data && typeof data === 'object' ? data : payload ?? data;
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const item = buildWorkflowItem(`调用工具：${toolName}`, buildDetail(detailSource), 'loading', {
          isTool: true,
          toolCategory
        });
        assistantMessage.workflowItems.push(item);
        registerToolItem(toolName, item.id);
        registerToolStats(toolName);
        if (lastRound !== null) {
          // 工具调用轮次的模型输出不展示在正式回答区
          blockedRounds.add(lastRound);
          if (visibleRound === lastRound) {
            clearVisibleOutput();
          }
        }
        break;
      }
      case 'tool_output_delta': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name ?? '';
        const delta = data?.delta ?? payload?.delta ?? '';
        if (!delta) {
          break;
        }
        const streamName = String(data?.stream ?? payload?.stream ?? 'stdout').toLowerCase();
        const command = typeof data?.command === 'string' ? data.command : payload?.command;
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const callId = toolName ? peekToolItemId(toolName) : null;
        const outputKey = resolveToolOutputKey(toolName, callId);
        const buffer = getToolOutputBuffer(outputKey);
        if (command && !buffer.command) {
          buffer.command = String(command);
        }
        if (streamName.includes('err')) {
          buffer.stderr += delta;
        } else {
          buffer.stdout += delta;
        }
        const itemId = ensureToolOutputItem(toolName, outputKey, toolCategory);
        if (itemId) {
          updateWorkflowItem(assistantMessage.workflowItems, itemId, {
            detail: buildToolOutputDetail(buffer),
            status: 'loading'
          });
        }
        break;
      }
      case 'tool_result': {
        const toolName = data?.tool ?? payload?.tool ?? data?.name ?? payload?.name;
        const result = data?.result ?? payload?.result ?? data?.output ?? payload?.output ?? data ?? payload;
        const failed = isFailedResult(payload);
        const targetId = toolName ? resolveToolItemId(toolName) : null;
        const toolCategory = resolveToolCategory(toolName, data ?? payload);
        const sandboxed = data?.sandbox === true;
        const outputKey = resolveToolOutputKey(toolName, targetId);
        const detailSource =
          data && typeof data === 'object'
            ? data
            : payload && typeof payload === 'object'
              ? payload
              : result;
        const detailPayload =
          sandboxed && detailSource && typeof detailSource === 'object' && !('sandbox' in detailSource)
            ? { ...detailSource, sandbox: true }
            : detailSource;
        const detail = buildDetail(detailPayload ?? result);
        if (targetId) {
          updateWorkflowItem(assistantMessage.workflowItems, targetId, {
            status: failed ? 'failed' : 'completed'
          });
        }
        finalizeToolOutputItem(outputKey, failed);
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            `工具结果：${toolName || '未知工具'}`,
            detail,
            failed ? 'failed' : 'completed',
            {
              isTool: true,
              toolCategory
            }
          )
        );
        if (!assistantMessage.questionPanel && isQuestionPanelToolName(toolName)) {
          const panelPayload = data?.data ?? data?.result ?? data?.output ?? null;
          applyQuestionPanelPayload(panelPayload);
        }
        break;
      }
      case 'approval_request': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const toolName = String(data?.tool ?? payload?.tool ?? '').trim();
        const title = toolName ? `等待审批：${toolName}` : '等待审批';
        const item = buildWorkflowItem(title, buildDetail(data ?? payload), 'loading');
        assistantMessage.workflowItems.push(item);
        if (approvalId) {
          approvalItemMap.set(approvalId, item.id);
        }
        break;
      }
      case 'approval_result': {
        const approvalId = String(data?.approval_id ?? payload?.approval_id ?? '').trim();
        const statusRaw = String(data?.status ?? payload?.status ?? '').trim().toLowerCase();
        const itemStatus = statusRaw === 'approved' ? 'completed' : 'failed';
        const targetId = approvalId ? approvalItemMap.get(approvalId) : null;
        if (targetId) {
          updateWorkflowItem(assistantMessage.workflowItems, targetId, {
            status: itemStatus,
            detail: buildDetail(data ?? payload)
          });
          approvalItemMap.delete(approvalId);
        } else {
          assistantMessage.workflowItems.push(
            buildWorkflowItem('审批结果', buildDetail(data ?? payload), itemStatus)
          );
        }
        break;
      }
      case 'workspace_update': {
        const sessionId = payload?.session_id ?? payload?.sessionId ?? null;
        const agentId = data?.agent_id ?? data?.agentId ?? '';
        const treeVersion = data?.tree_version ?? data?.treeVersion ?? null;
        emitWorkspaceRefresh({
          sessionId,
          agentId,
          treeVersion,
          reason: data?.reason || 'workspace_update'
        });
        break;
      }
      case 'plan_update': {
        const normalized = applyPlanUpdate(assistantMessage, data);
        if (normalized) {
          assistantMessage.workflowItems.push(
            buildWorkflowItem(
              '计划更新',
              buildDetail({ explanation: normalized.explanation, plan: normalized.steps })
            )
          );
        }
        break;
      }
      case 'question_panel': {
        const appendWorkflow = !assistantMessage.questionPanel;
        applyQuestionPanelPayload(data, { appendWorkflow });
        break;
      }
      case 'slow_client': {
        const capacity = data?.queue_capacity ?? payload?.queue_capacity ?? '-';
        assistantMessage.slow_client = true;
        assistantMessage.workflowItems.push(
          buildWorkflowItem(
            t('chat.workflow.slowClient'),
            t('chat.workflow.slowClientDetail', { capacity }),
            'failed'
          )
        );
        break;
      }
      case 'llm_output_delta': {
        const round = resolveRound(payload, data);
        if (round !== null) {
          lastRound = round;
          assistantMessage.stream_round = round;
        }
        if (round !== null && blockedRounds.has(round)) {
          break;
        }
        if (round !== null && visibleRound !== round) {
          if (visibleRound === null && outputContent) {
            visibleRound = round;
          } else {
            clearVisibleOutput();
            visibleRound = round;
          }
        }
        const segmentedDelta = parseSegmentedDelta(payload, data);
        const delta =
          data?.delta ??
          payload?.delta ??
          data?.content ??
          payload?.content ??
          segmentedDelta?.delta ??
          '';
        const reasoningDelta =
          data?.reasoning_delta ??
          payload?.reasoning_delta ??
          data?.think_delta ??
          payload?.think_delta ??
          segmentedDelta?.reasoningDelta ??
          '';
        const reasoningDeltaText =
          typeof reasoningDelta === 'string' ? reasoningDelta : reasoningDelta ? String(reasoningDelta) : '';
        if (reasoningDeltaText) {
          pendingReasoningExplicit += reasoningDeltaText;
          outputState.reasoningStreaming = true;
        }
        if (typeof delta === 'string' && delta) {
          const parsedDelta = thinkStreamParser.push(delta);
          if (parsedDelta.reasoning) {
            pendingReasoningFallback += parsedDelta.reasoning;
            outputState.reasoningStreaming = true;
          }
          if (parsedDelta.content) {
            pendingContent += parsedDelta.content;
            outputState.streaming = true;
          }
        }
        if (pendingContent || pendingReasoningExplicit || pendingReasoningFallback) {
          ensureOutputItem();
          scheduleStreamFlush();
        }
        break;
      }
      case 'llm_output': {
        const round = resolveRound(payload, data);
        updateUsageStats(
          data?.usage ?? payload?.usage ?? data,
          data?.prefill_duration_s ?? payload?.prefill_duration_s,
          data?.decode_duration_s ?? payload?.decode_duration_s
        );
        if (round !== null) {
          lastRound = round;
          assistantMessage.stream_round = round;
        }
        if (round !== null && blockedRounds.has(round)) {
          break;
        }
        if (round !== null && visibleRound !== round) {
          if (visibleRound === null && outputContent) {
            visibleRound = round;
          } else {
            clearVisibleOutput();
            visibleRound = round;
          }
        }
        flushStream(true);
        const content = data?.content ?? payload?.content ?? data?.output ?? payload?.output ?? '';
        const reasoningRaw =
          data?.reasoning ??
          payload?.reasoning ??
          data?.reasoning_content ??
          payload?.reasoning_content ??
          data?.think_content ??
          payload?.think_content ??
          '';
        const reasoningText = normalizeReasoningText(reasoningRaw);
        const parsedContent = normalizeAssistantOutput(content, reasoningText);
        const hasContent = typeof content === 'string' && content !== '';
        const toolCallsPayload =
          data?.tool_calls ??
          payload?.tool_calls ??
          data?.tool_call ??
          payload?.tool_call ??
          data?.function_call ??
          payload?.function_call;
        const toolCallAnswer = !hasContent ? extractFinalAnswerFromToolCalls(toolCallsPayload) : '';
        const parsedToolCallAnswer = normalizeAssistantOutput(toolCallAnswer, '');
        const resolvedContent = hasContent ? parsedContent.content : parsedToolCallAnswer.content;
        const inlineReasoning = hasContent
          ? parsedContent.inlineReasoning
          : parsedToolCallAnswer.inlineReasoning;
        const resolvedHasContent =
          typeof resolvedContent === 'string' && resolvedContent !== '';
        const hasReasoning = reasoningText !== '' || inlineReasoning !== '';
        if (
          !resolvedHasContent &&
          !hasReasoning &&
          (outputState.streaming || outputState.reasoningStreaming)
        ) {
          outputState.streaming = false;
          outputState.reasoningStreaming = false;
        } else {
          if (reasoningText) {
            outputReasoningExplicit = reasoningText;
          } else if (inlineReasoning) {
            outputReasoningFallback = inlineReasoning;
          }
          if (resolvedHasContent) {
            outputContent = resolvedContent;
            assistantMessage.content = resolvedContent;
          }
          outputState.streaming = false;
          outputState.reasoningStreaming = false;
        }
        syncReasoningToMessage();
        const outputId = ensureOutputItem();
        updateWorkflowItem(assistantMessage.workflowItems, outputId, {
          status: 'completed',
          detail: buildOutputDetail()
        });
        break;
      }
      case 'token_usage': {
        updateUsageStats(
          data?.usage ?? payload?.usage ?? data,
          data?.prefill_duration_s ?? payload?.prefill_duration_s,
          data?.decode_duration_s ?? payload?.decode_duration_s
        );
        break;
      }
      case 'context_usage': {
        updateContextUsage(data ?? payload ?? {});
        break;
      }
      case 'quota_usage': {
        updateQuotaUsage(data ?? payload ?? {});
        break;
      }
      case 'final': {
        flushStream(true);
        const answer =
          data?.answer ??
          payload?.answer ??
          data?.content ??
          payload?.content ??
          data?.message ??
          payload?.message ??
          raw;
        if (answer) {
          const answerText = pickText(answer, assistantMessage.content);
          const normalizedAnswer = normalizeAssistantOutput(answerText, '');
          assistantMessage.content = normalizedAnswer.content;
          outputContent = normalizedAnswer.content;
          if (normalizedAnswer.inlineReasoning) {
            outputReasoningFallback = normalizedAnswer.inlineReasoning;
          }
          visibleRound = lastRound ?? visibleRound;
        }
        if (lastRound !== null) {
          assistantMessage.stream_round = lastRound;
        }
        outputState.streaming = false;
        outputState.reasoningStreaming = false;
        syncReasoningToMessage();
        const outputId = ensureOutputItem();
        updateWorkflowItem(assistantMessage.workflowItems, outputId, {
          status: 'completed',
          detail: buildOutputDetail()
        });
        assistantMessage.workflowItems.push(
          buildWorkflowItem('最终回复', buildDetail(data || answer))
        );
        break;
      }
      case 'error': {
        const detail = data?.message ?? payload?.message ?? raw ?? t('chat.error.generic');
        assistantMessage.workflowItems.push(
          buildWorkflowItem(t('chat.workflow.error'), pickText(detail), 'failed')
        );
        if (!assistantMessage.content) {
          assistantMessage.content = t('chat.error.retry');
        }
        break;
      }
      case 'team_start':
      case 'team_task_dispatch':
      case 'team_task_update':
      case 'team_task_result':
      case 'team_merge':
      case 'team_finish':
      case 'team_error': {
        const status = eventType === 'team_error' ? 'failed' : eventType === 'team_finish' ? 'completed' : 'loading';
        assistantMessage.workflowItems.push(
          buildWorkflowItem(t('chat.workflow.event', { event: eventType }), buildDetail(data || raw), status)
        );
        break;
      }
      default: {
        const fallbackName = data?.name ?? payload?.name;
        const summary = fallbackName
          ? t('chat.workflow.eventWithName', { event: eventType, name: fallbackName })
          : t('chat.workflow.event', { event: eventType });
        assistantMessage.workflowItems.push(buildWorkflowItem(summary, buildDetail(data || raw)));
        break;
      }
    }
    notifySnapshot();
  };

  const finalize = () => {
    flushStream(true);
    outputState.streaming = false;
    outputState.reasoningStreaming = false;
    syncReasoningToMessage();
    if (outputItemId) {
      updateWorkflowItem(assistantMessage.workflowItems, outputItemId, {
        status: 'completed'
      });
    }
    finalizeInteractionDuration();
    notifySnapshot();
  };

  return { handleEvent, finalize };
};

const hydrateMessage = (message, workflowState) => {
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
    reasoning: normalizedOutput.reasoning,
    reasoningStreaming: normalizeFlag(message?.reasoningStreaming),
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
      { finalizeWithNow: false }
    );
    message.workflow_events.forEach((event) => {
      processor.handleEvent(event?.event || '', event?.raw || '');
    });
    processor.finalize();
  }
  return hydrated;
};

export const useChatStore = defineStore('chat', {
  state: () => ({
    sessions: [],
    activeSessionId: null,
    messages: [],
    loadingBySession: {},
    greetingOverride: '',
    draftAgentId: '',
    draftToolOverrides: null,
    pendingApprovals: [] as PendingApproval[],
    streamTransport: resolveStreamTransport()
  }),
  getters: {
    isSessionLoading: (state) => (sessionId) => {
      const key = resolveSessionKey(sessionId);
      if (!key) return false;
      return Boolean(state.loadingBySession[key]);
    },
    activeApproval: (state) => (Array.isArray(state.pendingApprovals) ? state.pendingApprovals[0] : null)
  },
  actions: {
    markPageUnloading() {
      pageUnloading = true;
      clearSessionWatcher();
    },
    setStreamTransport(transport) {
      const next = transport === 'sse' ? 'sse' : 'ws';
      localStorage.setItem('chat_stream_transport', next);
      if (next === 'ws') {
        wsUnavailableUntil = 0;
      }
      this.streamTransport = next;
    },
    async refreshStreamTransportPolicy() {
      try {
        const { data } = await fetchChatTransportProfile();
        const remote = String(data?.data?.chat_stream_channel || '').trim().toLowerCase();
        const next = remote === 'sse' ? 'sse' : 'ws';
        localStorage.setItem('chat_stream_transport', next);
        if (next === 'ws') {
          wsUnavailableUntil = 0;
        }
        this.streamTransport = resolveStreamTransport();
        return next;
      } catch (error) {
        this.streamTransport = resolveStreamTransport();
        return this.streamTransport;
      }
    },
    toggleStreamTransport() {
      const next = this.streamTransport === 'sse' ? 'ws' : 'sse';
      this.setStreamTransport(next);
    },
    enqueueApprovalRequest(requestId, sessionId, payload) {
      const approval = normalizePendingApproval(payload, requestId, sessionId);
      if (!approval) return null;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const filtered = current.filter((item) => item?.approval_id !== approval.approval_id);
      this.pendingApprovals = [...filtered, approval];
      return approval;
    },
    resolveApprovalResult(payload) {
      const approvalId = normalizeApprovalResultId(payload);
      if (!approvalId) return false;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const next = current.filter((item) => item?.approval_id !== approvalId);
      const changed = next.length !== current.length;
      if (changed) {
        this.pendingApprovals = next;
      }
      return changed;
    },
    clearPendingApprovals(options: { sessionId?: string; requestId?: string } = {}) {
      const targetSessionId = String(options.sessionId || '').trim();
      const targetRequestId = String(options.requestId || '').trim();
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      if (!targetSessionId && !targetRequestId) {
        this.pendingApprovals = [];
        return;
      }
      this.pendingApprovals = current.filter((item) => {
        if (!item) return false;
        if (targetSessionId && String(item.session_id || '').trim() !== targetSessionId) {
          return true;
        }
        if (targetRequestId && String(item.request_id || '').trim() !== targetRequestId) {
          return true;
        }
        return false;
      });
    },
    async respondApproval(decision: ApprovalDecision, approvalId = '') {
      const normalizedDecision = String(decision || '').trim().toLowerCase();
      const resolvedDecision: ApprovalDecision =
        normalizedDecision === 'approve_session'
          ? 'approve_session'
          : normalizedDecision === 'approve_once'
            ? 'approve_once'
            : 'deny';
      const targetApprovalId = String(
        approvalId || this.activeApproval?.approval_id || ''
      ).trim();
      if (!targetApprovalId) return false;
      const current = Array.isArray(this.pendingApprovals) ? this.pendingApprovals : [];
      const target = current.find((item) => item?.approval_id === targetApprovalId);
      if (!target) return false;
      try {
        await chatWsClient.notify({
          type: 'approval',
          session_id: target.session_id,
          payload: {
            approval_id: targetApprovalId,
            decision: resolvedDecision
          }
        });
      } catch (error) {
        if (resolvedDecision !== 'deny') {
          throw error;
        }
      }
      this.pendingApprovals = current.filter((item) => item?.approval_id !== targetApprovalId);
      return true;
    },
    getPersistedState() {
      return readChatPersistState();
    },
    getLastSessionId(agentId) {
      return resolvePersistedSessionId(agentId);
    },
    setLastSessionId(agentId, sessionId) {
      persistAgentSession(agentId, sessionId);
    },
    hasSessionMessages(sessionId) {
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) && cached.length > 0;
    },
    getCachedSessionMessages(sessionId) {
      const cached = getSessionMessages(sessionId);
      return Array.isArray(cached) ? cached : [];
    },
    getCachedSessions(agentId) {
      const cached = readSessionListCache(agentId);
      if (cached) return cached;
      return filterSessionsByAgent(agentId, this.sessions);
    },
    resolveInitialSessionId(agentId, sourceSessions = null) {
      const targetSessions = Array.isArray(sourceSessions) ? sourceSessions : this.getCachedSessions(agentId);
      return resolveInitialSessionIdFromList(agentId, targetSessions);
    },
    async prefetchAgentSessions(agentId) {
      const normalizedAgentId = String(agentId ?? '').trim();
      const cached = readSessionListCache(normalizedAgentId);
      if (cached) {
        return cached;
      }
      const cacheKey = resolveSessionListCacheKey(normalizedAgentId);
      const inFlight = sessionListCacheInFlight.get(cacheKey);
      if (inFlight) {
        return inFlight;
      }
      const request = (async () => {
        const params = { agent_id: normalizedAgentId };
        const { data } = await listSessions(params);
        const sessions = sortSessionsByActivity(data?.data?.items || []);
        writeSessionListCache(normalizedAgentId, sessions);
        return cloneSessionList(sessions);
      })().finally(() => {
        sessionListCacheInFlight.delete(cacheKey);
      });
      sessionListCacheInFlight.set(cacheKey, request);
      return request;
    },
    async preloadSessionDetail(sessionId) {
      const targetId = resolveSessionKey(sessionId);
      if (!targetId) return null;
      if (isSessionDetailWarm(targetId) && getSessionMessages(targetId)?.length) {
        return this.sessions.find((session) => session.id === targetId) || null;
      }
      const inFlight = sessionDetailPrefetchInFlight.get(targetId);
      if (inFlight) {
        return inFlight;
      }
      const request = (async () => {
        const [sessionRes, eventsRes] = await Promise.all([
          getSession(targetId),
          getSessionEvents(targetId).catch(() => null)
        ]);
        const payload = sessionRes?.data;
        const sessionDetail = payload?.data || null;
        const rounds = eventsRes?.data?.data?.rounds || [];
        const workflowState = buildSessionWorkflowState();
        const rawMessages = attachWorkflowEvents(sessionDetail?.messages || [], rounds);
        let messages = rawMessages.map((message) => hydrateMessage(message, workflowState));
        dismissStaleInquiryPanels(messages);
        const greetingMessages = ensureGreetingMessage(messages, {
          createdAt: sessionDetail?.created_at,
          greeting: this.greetingOverride
        });
        cacheSessionMessages(targetId, greetingMessages);
        markSessionDetailWarm(targetId);
        return sessionDetail;
      })().finally(() => {
        sessionDetailPrefetchInFlight.delete(targetId);
      });
      sessionDetailPrefetchInFlight.set(targetId, request);
      return request;
    },
    getSnapshotForSession(sessionId) {
      const snapshot = readChatSnapshot();
      if (!snapshot || snapshot.sessionId !== String(sessionId || '')) {
        return null;
      }
      return snapshot;
    },
    scheduleSnapshot(immediate = false) {
      scheduleChatSnapshot(this, immediate);
    },
    setGreetingOverride(content) {
      const next = String(content || '').trim();
      this.greetingOverride = next;
      const greetingIndex = this.messages.findIndex((message) => message?.isGreeting);
      if (greetingIndex < 0) return;
      const greetingText = resolveGreetingContent(next);
      if (this.messages[greetingIndex].content !== greetingText) {
        this.messages[greetingIndex].content = greetingText;
        this.scheduleSnapshot(true);
      }
    },
    resolveInquiryPanel(message, patch: InquiryPanelPatch = {}) {
      if (!message || message.role !== 'assistant') return;
      const panel = normalizeInquiryPanelState(message.questionPanel);
      if (!panel) return;
      message.questionPanel = {
        ...panel,
        status: normalizeInquiryPanelStatus(patch.status ?? panel.status),
        selected: Array.isArray(patch.selected) ? patch.selected : panel.selected
      };
      this.scheduleSnapshot(true);
    },
    dismissPendingInquiryPanel() {
      for (let i = this.messages.length - 1; i >= 0; i -= 1) {
        const message = this.messages[i];
        if (message?.role !== 'assistant') continue;
        if (message?.questionPanel?.status !== 'pending') continue;
        this.resolveInquiryPanel(message, { status: 'dismissed' });
        return;
      }
    },
    appendLocalMessage(role: string, content: string, options: AppendLocalMessageOptions = {}) {
      const normalizedRole = role === 'assistant' ? 'assistant' : 'user';
      const text = String(content || '').trim();
      if (!text) return null;
      const message = buildMessage(normalizedRole, text, options.createdAt);
      this.messages.push(message);
      const targetSessionId = String(options.sessionId ?? this.activeSessionId ?? '').trim();
      if (targetSessionId) {
        cacheSessionMessages(targetSessionId, this.messages);
        touchSessionUpdatedAt(this, targetSessionId, message.created_at || Date.now());
        notifySessionSnapshot(this, targetSessionId, this.messages, options.immediate !== false);
      } else {
        this.scheduleSnapshot(options.immediate !== false);
      }
      return message;
    },
    async loadSessions(options: LoadSessionsOptions = {}) {
      const shouldRefreshTransport =
        options.skipTransportRefresh !== true && options.refresh_transport !== false;
      if (shouldRefreshTransport) {
        await this.refreshStreamTransportPolicy();
      }
      const params: { agent_id?: string } = {};
      let requestedAgentId: string | null = null;

      if (Object.prototype.hasOwnProperty.call(options, 'agent_id')) {
        requestedAgentId = String(options.agent_id ?? '');
        params.agent_id = requestedAgentId;
      }
      const { data } = await listSessions(Object.keys(params).length ? params : undefined);
      this.sessions = sortSessionsByActivity(data.data.items || []);
      if (requestedAgentId !== null) {
        writeSessionListCache(requestedAgentId, this.sessions);
      }
      syncDemoChatCache({ sessions: this.sessions });
      return this.sessions;
    },
    openDraftSession(options: OpenDraftSessionOptions = {}) {
      const currentSessionId = this.activeSessionId;
      cacheSessionMessages(currentSessionId, this.messages);
      abortResumeStream(currentSessionId);
      clearSessionWatcher();
      const runtime = ensureRuntime(currentSessionId);
      if (runtime) {
        runtime.stopRequested = false;
      }
      // Keep in-flight send stream alive so switching agent/thread won't cancel background runs.
      if (!runtime?.sendController) {
        setSessionLoading(this, currentSessionId, false);
      }
      this.activeSessionId = null;
      this.draftAgentId = String(options.agent_id || '').trim();
      this.draftToolOverrides = null;
      this.messages = ensureGreetingMessage([], { greeting: this.greetingOverride });
      persistDraftSession();
    },
    setDraftToolOverrides(overrides) {
      if (!Array.isArray(overrides) || overrides.length === 0) {
        this.draftToolOverrides = null;
        return;
      }
      this.draftToolOverrides = [...overrides];
    },
    async createSession(payload: Record<string, unknown> = {}) {
      abortResumeStream(this.activeSessionId);
      clearSessionWatcher();
      const { data } = await createSession(payload);
      const session = data.data;
      this.sessions.unshift(session);
      this.sessions = applyMainSession(this.sessions, session.agent_id, session.id);
      writeSessionListCache(session.agent_id, filterSessionsByAgent(session.agent_id, this.sessions));
      this.activeSessionId = session.id;
      this.draftAgentId = String(session.agent_id || '').trim();
      this.messages = ensureGreetingMessage([], {
        createdAt: session.created_at,
        greeting: this.greetingOverride
      });
      cacheSessionMessages(session.id, this.messages);
      touchSessionUpdatedAt(this, session.id, session.updated_at || session.created_at);
      getSessionWorkflowState(session.id, { reset: true });
      persistActiveSession(session.id, session.agent_id);
      syncDemoChatCache({
        sessions: this.sessions,
        sessionId: this.activeSessionId,
        messages: this.messages
      });
      startSessionWatcher(this, session.id);
      return session;
    },
    async setMainSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const agentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      const apiAgentId = agentId || DEFAULT_AGENT_KEY;
      await setDefaultSession(apiAgentId, { session_id: targetId });
      this.sessions = applyMainSession(this.sessions, agentId, targetId);
      writeSessionListCache(agentId, filterSessionsByAgent(agentId, this.sessions));
      persistAgentSession(agentId, targetId);
      return targetId;
    },

    async loadSessionDetail(sessionId) {
      const previousSessionId = this.activeSessionId;
      if (previousSessionId && previousSessionId !== sessionId) {
        cacheSessionMessages(previousSessionId, this.messages);
      }
      abortResumeStream(previousSessionId);
      clearSessionWatcher();
      this.activeSessionId = sessionId;
      const cachedSessionMessages = getSessionMessages(sessionId);
      const snapshot = this.getSnapshotForSession(sessionId);
      if (cachedSessionMessages?.length) {
        this.messages = ensureGreetingMessage(cachedSessionMessages, {
          greeting: this.greetingOverride
        });
      } else if (snapshot?.messages?.length) {
        const cachedMessages = snapshot.messages
          .map((item) => normalizeSnapshotMessage(item))
          .filter(Boolean);
        this.messages = ensureGreetingMessage(cachedMessages, {
          greeting: this.greetingOverride
        });
      }
      if (cachedSessionMessages?.length || snapshot?.messages?.length) {
        cacheSessionMessages(sessionId, this.messages);
      }
      const [sessionRes, eventsRes] = await Promise.all([
        getSession(sessionId),
        getSessionEvents(sessionId).catch(() => null)
      ]);
      const data = sessionRes?.data;
      const sessionDetail = data?.data || null;
      const sessionCreatedAt = sessionDetail?.created_at;
      if (sessionDetail?.id) {
        const index = this.sessions.findIndex((item) => item.id === sessionDetail.id);
        if (index >= 0) {
          this.sessions[index] = { ...this.sessions[index], ...sessionDetail };
        } else {
          this.sessions.unshift(sessionDetail);
        }
      }
      this.draftAgentId = String(sessionDetail?.agent_id || '').trim();
      const resolvedAgentId =
        sessionDetail?.agent_id ??
        this.sessions.find((item) => item.id === sessionId)?.agent_id ??
        this.draftAgentId;
      writeSessionListCache(resolvedAgentId, filterSessionsByAgent(resolvedAgentId, this.sessions));
      persistActiveSession(sessionId, resolvedAgentId);
      this.draftToolOverrides = null;
      const rounds = eventsRes?.data?.data?.rounds || [];
      const workflowState = getSessionWorkflowState(sessionId, { reset: true });
      const rawMessages = attachWorkflowEvents(sessionDetail?.messages || [], rounds);
      let messages = rawMessages.map((message) =>
        hydrateMessage(message, workflowState)
      );
      messages = mergeSnapshotIntoMessages(messages, snapshot);
      const finalCachedMessages = getSessionMessages(sessionId);
      if (shouldPreferCachedMessages(finalCachedMessages, messages)) {
        messages = finalCachedMessages;
      }
      dismissStaleInquiryPanels(messages);
      this.messages = ensureGreetingMessage(messages, {
        createdAt: sessionCreatedAt,
        greeting: this.greetingOverride
      });
      cacheSessionMessages(sessionId, this.messages);
      markSessionDetailWarm(sessionId);
      syncDemoChatCache({ sessionId: sessionId, messages: this.messages });
      const pendingMessage = [...this.messages]
        .reverse()
        .find((message) => message.role === 'assistant' && message.stream_incomplete);
      if (pendingMessage) {
        this.resumeStream(sessionId, pendingMessage);
      }
      this.scheduleSnapshot(true);
      startSessionWatcher(this, sessionId);
      return data.data;
    },
    async deleteSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return;
      if (resolveSessionKey(targetId) === resolveSessionKey(this.activeSessionId)) {
        clearSessionWatcher();
      }
      const targetSession = this.sessions.find((item) => item.id === targetId) || null;
      const targetAgentId = String(targetSession?.agent_id || this.draftAgentId || '').trim();
      abortResumeStream(targetId);
      abortSendStream(targetId);
      setSessionLoading(this, targetId, false);
      this.clearPendingApprovals({ sessionId: targetId });
      sessionRuntime.delete(resolveSessionKey(targetId));
      sessionMessages.delete(resolveSessionKey(targetId));
      sessionDetailWarmState.delete(resolveSessionKey(targetId));
      sessionDetailPrefetchInFlight.delete(resolveSessionKey(targetId));
      await deleteSessionApi(targetId);
      this.sessions = this.sessions.filter((item) => item.id !== targetId);
      if (targetSession?.is_main) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        const apiAgentId = targetAgentId || DEFAULT_AGENT_KEY;
        if (fallback) {
          await setDefaultSession(apiAgentId, { session_id: fallback.id });
          this.sessions = applyMainSession(this.sessions, targetAgentId, fallback.id);
          persistAgentSession(targetAgentId, fallback.id);
        } else {
          this.sessions = applyMainSession(this.sessions, targetAgentId, '');
          persistAgentSession(targetAgentId, '');
        }
      }
      sessionWorkflowState.delete(String(targetId));
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (resolvePersistedSessionId(targetAgentId) === targetId) {
        const fallback = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        persistAgentSession(targetAgentId, fallback?.id || '');
      }
      writeSessionListCache(targetAgentId, filterSessionsByAgent(targetAgentId, this.sessions));
      if (this.activeSessionId === targetId) {
        const nextSession = this.sessions.find((item) => {
          const agentId = String(item.agent_id || '').trim();
          return targetAgentId ? agentId === targetAgentId : !agentId;
        });
        if (nextSession) {
          await this.loadSessionDetail(nextSession.id);
        } else {
          this.openDraftSession({ agent_id: targetAgentId });
        }
      }
    },
    async updateSessionTools(sessionId, toolOverrides = []) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return null;
      const payload = {
        tool_overrides: Array.isArray(toolOverrides) ? toolOverrides : []
      };
      const { data } = await updateSessionToolsApi(targetId, payload);
      const overrides = data?.data?.tool_overrides || [];
      const index = this.sessions.findIndex((item) => item.id === targetId);
      if (index >= 0) {
        this.sessions[index] = {
          ...this.sessions[index],
          tool_overrides: overrides
        };
      }
      return overrides;
    },
    async compactSession(sessionId, payload: Record<string, unknown> = {}) {
      const targetId = String(sessionId || this.activeSessionId || '').trim();
      if (!targetId) {
        throw new Error(t('chat.command.compactMissingSession'));
      }
      const { data } = await compactSessionApi(targetId, payload);
      return data?.data?.message || data?.message || '';
    },

    async sendMessage(content: string, options: SendMessageOptions = {}) {
      const initialSessionId = this.activeSessionId;
      abortResumeStream(initialSessionId);
      abortSendStream(initialSessionId);
      abortWatchStream(initialSessionId);
      const initialRuntime = ensureRuntime(initialSessionId);
      if (initialRuntime) {
        initialRuntime.stopRequested = false;
      }
      if (!this.activeSessionId) {
        const payload = this.draftAgentId ? { agent_id: this.draftAgentId } : {};
        const session = await this.createSession(payload);
        if (Array.isArray(this.draftToolOverrides)) {
          try {
            await this.updateSessionTools(session.id, this.draftToolOverrides);
          } catch (error) {
            // ignore draft tool override failures
          }
        }
        this.draftToolOverrides = null;
      }
      const userMessage = buildMessage('user', content);
      this.messages.push(userMessage);
      const requestStartMs = resolveTimestampMs(userMessage.created_at) ?? Date.now();
      const attachments = Array.isArray(options.attachments) ? options.attachments : [];
      const sessionId = this.activeSessionId;
      const runtime = ensureRuntime(sessionId);
      cacheSessionMessages(sessionId, this.messages);
      touchSessionUpdatedAt(this, sessionId, userMessage.created_at);

      const activeSession = this.sessions.find((item) => item.id === sessionId);
      if (activeSession) {
        this.sessions = applyMainSession(this.sessions, activeSession.agent_id, sessionId);
        persistAgentSession(activeSession.agent_id, sessionId);
        if (shouldAutoTitle(activeSession.title)) {
          const autoTitle = buildSessionTitle(content);
          if (autoTitle) {
            activeSession.title = autoTitle;
          }
        }
      }

      const assistantMessageRaw = {
        ...buildMessage('assistant', ''),
        workflowItems: [],
        workflowStreaming: true,
        stream_incomplete: true,
        stream_event_id: 0,
        stream_round: null
      };
      if (assistantMessageRaw.stats) {
        assistantMessageRaw.stats.interaction_start_ms = requestStartMs;
      }
      this.messages.push(assistantMessageRaw);
      const assistantMessage = this.messages[this.messages.length - 1];
      const sessionMessagesRef = this.messages;
      notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);

      setSessionLoading(this, sessionId, true);

      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        assistantMessage,
        workflowState,
        () => notifySessionSnapshot(this, sessionId, sessionMessagesRef)
      );
      let queued = false;

      try {
        if (runtime) {
          runtime.sendController = new AbortController();
        }
        const desktopToolCallMode = getDesktopToolCallModeForRequest();
        const messengerApprovalMode = getMessengerApprovalModeForRequest();
        const payload = {
          content,
          stream: true,
          ...(attachments.length > 0 ? { attachments } : {}),
          ...(desktopToolCallMode ? { tool_call_mode: desktopToolCallMode } : {}),
          ...(messengerApprovalMode ? { approval_mode: messengerApprovalMode } : {})
        };
        const onEvent = (eventType, dataText, eventId) => {
          const payload = safeJsonParse(dataText);
          const approvalPayload = payload?.data ?? payload;
          handleApprovalEvent(
            this,
            eventType,
            approvalPayload,
            runtime?.sendRequestId || '',
            sessionId
          );
          const queuedFlag =
            eventType === 'queued' || payload?.queued === true || payload?.data?.queued === true;
          if (queuedFlag) {
            if (!queued) {
              queued = true;
              assistantMessage.workflowItems.push(
                buildWorkflowItem(t('chat.workflow.queued'), t('chat.workflow.queuedDetail'), 'pending')
              );
              notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
            }
            assistantMessage.stream_incomplete = true;
            assistantMessage.workflowStreaming = true;
            return;
          }
          assignStreamEventId(assistantMessage, eventId);
          updateRuntimeLastEventId(runtime, eventId);
          processor.handleEvent(eventType, dataText);
        };
        const streamWithSse = async () => {
          const response = await sendMessageStream(sessionId, payload, {
            signal: runtime?.sendController?.signal
          });
          if (!response.ok) {
            const errorText = await readResponseError(response);
            throw new Error(
              errorText || t('chat.error.requestFailedWithStatus', { status: response.status })
            );
          }
          await consumeSseStream(response, onEvent);
        };
        const streamWithWs = async () => {
          const requestId = buildWsRequestId();
          if (runtime) {
            runtime.sendRequestId = requestId;
          }
          await chatWsClient.request({
            requestId,
            sessionId,
            message: {
              type: 'start',
              request_id: requestId,
              session_id: sessionId,
              payload
            },
            onEvent,
            signal: runtime?.sendController?.signal,
            closeOnFinal: true
          });
        };
        await this.refreshStreamTransportPolicy();
        const transport = resolveStreamTransport();
        this.streamTransport = transport;
        if (transport === 'ws') {
          try {
            await streamWithWs();
          } catch (error) {
            if (error?.phase === 'connect') {
              markWsUnavailable();
              this.streamTransport = 'sse';
              await streamWithSse();
            } else {
              throw error;
            }
          }
        } else {
          await streamWithSse();
        }
      } catch (error) {
        if (error?.name === 'AbortError' || runtime?.stopRequested || pageUnloading) {
          if (!pageUnloading) {
            assistantMessage.workflowItems.push(
              buildWorkflowItem(
                t('chat.workflow.aborted'),
                t('chat.workflow.abortedDetail'),
                'failed'
              )
            );
          }
        } else {
          assistantMessage.workflowItems.push(
            buildWorkflowItem(
              t('chat.workflow.requestFailed'),
              error?.message || t('chat.workflow.requestFailedDetail'),
              'failed'
            )
          );
          if (!assistantMessage.content) {
            assistantMessage.content = t('chat.error.requestFailed');
          }
        }
        this.dismissPendingInquiryPanel();
      } finally {
        const finishedRequestId = runtime?.sendRequestId || '';
        assistantMessage.workflowStreaming = false;
        assistantMessage.stream_incomplete = queued ? true : false;
        setSessionLoading(this, sessionId, false);
        processor.finalize();
        touchSessionUpdatedAt(this, sessionId, Date.now());
        this.clearPendingApprovals({ requestId: finishedRequestId, sessionId });
        if (runtime) {
          runtime.sendController = null;
          runtime.stopRequested = false;
          runtime.sendRequestId = null;
        }
        syncDemoChatCache({
          sessions: this.sessions,
          sessionId,
          messages: sessionMessagesRef
        });
        notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
        if (this.activeSessionId === sessionId && !pageUnloading) {
          startSessionWatcher(this, sessionId);
        }
      }
    },
    async stopStream() {
      if (!this.activeSessionId) {
        return false;
      }
      const sessionId = this.activeSessionId;
      const runtime = ensureRuntime(sessionId);
      if (runtime) {
        runtime.stopRequested = true;
      }
      abortSendStream(sessionId);
      let cancelled = false;
      try {
        const { data } = await cancelMessageStream(sessionId);
        cancelled = Boolean(data?.data?.cancelled);
      } catch (error) {
        // Ignore cancel API failures; local stop behavior still applies.
      }
      setSessionLoading(this, sessionId, false);
      if (!pageUnloading) {
        startSessionWatcher(this, sessionId);
      }
      return cancelled;
    },
    async resumeStream(sessionId, message, options: ResumeStreamOptions = {}) {
      const force = options.force === true;
      if (!message || (!message.stream_incomplete && !force)) return;
      abortWatchStream(sessionId);
      setSessionLoading(this, sessionId, true);
      message.workflowStreaming = true;
      message.stream_incomplete = true;
      const sessionMessagesRef = getSessionMessages(sessionId) || this.messages;
      cacheSessionMessages(sessionId, sessionMessagesRef);
      notifySessionSnapshot(this, sessionId, sessionMessagesRef);
      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(
        message,
        workflowState,
        () => notifySessionSnapshot(this, sessionId, sessionMessagesRef)
      );
      abortResumeStream(sessionId);
      const runtime = ensureRuntime(sessionId);
      if (runtime) {
        runtime.resumeController = new AbortController();
      }
      let aborted = false;
      const forcedEventId = options.afterEventId;
      const normalizedMessageEventId = normalizeStreamEventId(message.stream_event_id);
      const afterEventId = Number.isFinite(Number(forcedEventId))
        ? Number.parseInt(String(forcedEventId), 10)
        : normalizedMessageEventId;
      const resumeAfterEventId = Number.isFinite(afterEventId) ? Math.max(afterEventId, 0) : 0;
      try {
        const onEvent = (eventType, dataText, eventId) => {
          const payload = safeJsonParse(dataText);
          handleApprovalEvent(
            this,
            eventType,
            payload?.data ?? payload,
            runtime?.resumeRequestId || '',
            sessionId
          );
          assignStreamEventId(message, eventId);
          updateRuntimeLastEventId(runtime, eventId);
          processor.handleEvent(eventType, dataText);
        };
        const streamWithSse = async () => {
          const response = await resumeMessageStream(sessionId, {
            signal: runtime?.resumeController?.signal,
            afterEventId: resumeAfterEventId
          });
          if (!response.ok) {
            const errorText = await readResponseError(response);
            throw new Error(
              errorText || t('chat.error.resumeFailedWithStatus', { status: response.status })
            );
          }
          await consumeSseStream(response, onEvent);
        };
        const streamWithWs = async () => {
          const requestId = buildWsRequestId();
          if (runtime) {
            runtime.resumeRequestId = requestId;
          }
          await chatWsClient.request({
            requestId,
            sessionId,
            message: {
              type: 'resume',
              request_id: requestId,
              session_id: sessionId,
              payload: {
                after_event_id: Math.max(afterEventId, 1)
              }
            },
            onEvent,
            signal: runtime?.resumeController?.signal,
            closeOnFinal: true
          });
        };
        const hasAfterEventId = Number.isFinite(afterEventId) && afterEventId > 0;
        if (hasAfterEventId) {
          await this.refreshStreamTransportPolicy();
        }
        const transport = hasAfterEventId ? resolveStreamTransport() : 'sse';
        this.streamTransport = transport;
        if (transport === 'ws') {
          try {
            await streamWithWs();
          } catch (error) {
            if (error?.phase === 'connect') {
              markWsUnavailable();
              this.streamTransport = 'sse';
              await streamWithSse();
            } else {
              throw error;
            }
          }
        } else {
          await streamWithSse();
        }
      } catch (error) {
        if (error?.name === 'AbortError') {
          aborted = true;
        } else {
          message.workflowItems.push(
            buildWorkflowItem(
              t('chat.workflow.resumeFailed'),
              error?.message || t('chat.workflow.resumeFailedDetail'),
              'failed'
            )
          );
          if (!message.content) {
            message.content = t('chat.error.resumeFailed');
          }
        }
      } finally {
        const finishedRequestId = runtime?.resumeRequestId || '';
        message.workflowStreaming = false;
        if (!aborted) {
          message.stream_incomplete = false;
        }
        setSessionLoading(this, sessionId, false);
        processor.finalize();
        touchSessionUpdatedAt(this, sessionId, Date.now());
        this.clearPendingApprovals({ requestId: finishedRequestId, sessionId });
        if (runtime && !aborted) {
          runtime.resumeController = null;
        }
        if (runtime) {
          runtime.resumeRequestId = null;
        }
        notifySessionSnapshot(this, sessionId, sessionMessagesRef, true);
        if (!aborted && this.activeSessionId === sessionId && !pageUnloading) {
          startSessionWatcher(this, sessionId);
        }
      }
    }
  }
});
