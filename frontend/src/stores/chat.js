import { defineStore } from 'pinia';

import {
  cancelMessageStream,
  createSession,
  deleteSession as deleteSessionApi,
  getSession,
  getSessionEvents,
  listSessions,
  resumeMessageStream,
  sendMessageStream
} from '@/api/chat';
import { useAuthStore } from '@/stores/auth';
import { consumeSseStream } from '@/utils/sse';
import { loadSharedToolSelection } from '@/utils/toolSelection';
import { isDemoMode, loadDemoChatState, saveDemoChatState } from '@/utils/demo';

const buildMessageStats = () => ({
  toolCalls: 0,
  fileChanges: 0,
  usage: null,
  prefill_duration_s: null,
  decode_duration_s: null
});

const normalizeStatsCount = (value) => {
  if (value === null || value === undefined) return 0;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
};

const normalizeDurationValue = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : null;
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
  return {
    toolCalls: normalizeStatsCount(stats.toolCalls),
    fileChanges: normalizeStatsCount(stats.fileChanges),
    usage: normalizeUsagePayload(stats.usage ?? stats.tokenUsage ?? stats.token_usage),
    prefill_duration_s: normalizeDurationValue(
      stats.prefill_duration_s ?? stats.prefillDurationS ?? stats.prefillDuration
    ),
    decode_duration_s: normalizeDurationValue(
      stats.decode_duration_s ?? stats.decodeDurationS ?? stats.decodeDuration
    )
  };
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
  return {
    toolCalls: Math.max(left.toolCalls, right.toolCalls),
    fileChanges: Math.max(left.fileChanges, right.fileChanges),
    usage: right.usage || left.usage,
    prefill_duration_s:
      right.prefill_duration_s === null || right.prefill_duration_s === undefined
        ? left.prefill_duration_s
        : right.prefill_duration_s,
    decode_duration_s:
      right.decode_duration_s === null || right.decode_duration_s === undefined
        ? left.decode_duration_s
        : right.decode_duration_s
  };
};

const FILE_CHANGE_TOOL_NAMES = new Set(['写入文件', '替换文本', '编辑文件', '写文件', 'write_file', 'replace_text', 'edit_file']);
const FILE_CHANGE_TOOL_KEYS = new Set(['writefile', 'replacetext', 'editfile']);

const isFileChangeTool = (toolName) => {
  const raw = String(toolName || '').trim();
  if (!raw) return false;
  if (FILE_CHANGE_TOOL_NAMES.has(raw)) return true;
  const normalized = raw.toLowerCase().replace(/[\s_-]/g, '');
  return FILE_CHANGE_TOOL_KEYS.has(normalized);
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

const buildMessage = (role, content, createdAt) => ({
  role,
  content,
  created_at: resolveTimestampIso(createdAt) || new Date().toISOString(),
  reasoning: '',
  reasoningStreaming: false,
  plan: null,
  planVisible: false,
  stats: role === 'assistant' ? buildMessageStats() : null
});

const DEFAULT_GREETING = '你好！我是智能体助手，有什么可以帮你的吗？';
const CHAT_STATE_KEY = 'wille-chat-state';

const buildChatPersistState = () => ({
  activeSessionId: '',
  draft: false
});

const normalizeChatPersistState = (value) => {
  if (!value || typeof value !== 'object') {
    return buildChatPersistState();
  }
  return {
    activeSessionId: typeof value.activeSessionId === 'string' ? value.activeSessionId : '',
    draft: value.draft === true
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

const writeChatPersistState = (patch) => {
  try {
    const current = readChatPersistState();
    const next = normalizeChatPersistState({ ...current, ...patch });
    localStorage.setItem(CHAT_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // ignore persistence errors
  }
};

const persistActiveSession = (sessionId) => {
  writeChatPersistState({ activeSessionId: String(sessionId || ''), draft: false });
};

const persistDraftSession = () => {
  writeChatPersistState({ activeSessionId: '', draft: true });
};

const CHAT_SNAPSHOT_KEY = 'wille-chat-snapshot';
const SNAPSHOT_FLUSH_MS = 400;
const MAX_SNAPSHOT_MESSAGES = 50;
let snapshotTimer = null;
let pageUnloading = false;

const normalizeStreamEventId = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizeStreamRound = (value) => {
  if (value === null || value === undefined) return null;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
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
  const base = {
    role: message.role,
    content: typeof message.content === 'string' ? message.content : String(message.content || ''),
    created_at: message.created_at || ''
  };
  if (message.role === 'assistant') {
    base.reasoning = message.reasoning || '';
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
  let lastAssistantIndex = -1;
  for (let i = sliced.length - 1; i >= 0; i -= 1) {
    if (sliced[i]?.role === 'assistant') {
      lastAssistantIndex = i;
      break;
    }
  }
  return sliced
    .map((message, index) => {
      const normalized = normalizeSnapshotMessage(message);
      if (!normalized) return null;
      const shouldKeepWorkflow =
        index === lastAssistantIndex || normalized.stream_incomplete || normalized.workflowStreaming;
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
  const snapshotLastAssistant = [...snapshotMessages]
    .reverse()
    .find((message) => message.role === 'assistant');
  const snapshotLastMessage = snapshotMessages[snapshotMessages.length - 1];
  const serverLastMessage = messages[messages.length - 1];
  if (
    serverLastMessage?.role === 'user' &&
    snapshotLastMessage?.role === 'assistant' &&
    snapshotLastMessage.stream_incomplete
  ) {
    return [...messages, { ...snapshotLastMessage }];
  }
  if (!snapshotLastAssistant) return messages;
  let lastAssistantIndex = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]?.role === 'assistant') {
      lastAssistantIndex = i;
      break;
    }
  }
  if (lastAssistantIndex < 0) return messages;
  const target = messages[lastAssistantIndex];
  const snapshotContent = String(snapshotLastAssistant.content || '');
  const serverContent = String(target.content || '');
  const snapshotEventId = normalizeStreamEventId(snapshotLastAssistant.stream_event_id);
  const targetEventId = normalizeStreamEventId(target.stream_event_id);
  const snapshotRound = normalizeStreamRound(snapshotLastAssistant.stream_round);
  const targetRound = normalizeStreamRound(target.stream_round);
  const snapshotPlan = normalizePlanPayload(snapshotLastAssistant.plan);
  const hasSnapshotPlan = hasPlanSteps(snapshotPlan);
  const snapshotStats = normalizeMessageStats(snapshotLastAssistant.stats);
  const shouldMergeContent =
    snapshotContent.length > serverContent.length ||
    (snapshotLastAssistant.stream_incomplete && serverContent.length === 0);
  const shouldMergeFlags = Boolean(
    snapshotLastAssistant.stream_incomplete ||
      snapshotLastAssistant.workflowStreaming ||
      snapshotLastAssistant.reasoningStreaming ||
      (Array.isArray(snapshotLastAssistant.workflowItems) && snapshotLastAssistant.workflowItems.length > 0) ||
      hasSnapshotPlan ||
      snapshotRound !== null ||
      snapshotEventId !== null
  );
  if (!shouldMergeContent && !shouldMergeFlags) {
    return messages;
  }
  if (shouldMergeContent && snapshotContent) {
    target.content = snapshotContent;
  }
  if (snapshotLastAssistant.reasoning) {
    target.reasoning = snapshotLastAssistant.reasoning;
  }
  if (shouldMergeFlags) {
    target.reasoningStreaming =
      normalizeFlag(snapshotLastAssistant.reasoningStreaming) ||
      normalizeFlag(target.reasoningStreaming);
    if (Array.isArray(snapshotLastAssistant.workflowItems) && snapshotLastAssistant.workflowItems.length) {
      target.workflowItems = snapshotLastAssistant.workflowItems;
    }
    target.workflowStreaming =
      normalizeFlag(snapshotLastAssistant.workflowStreaming) ||
      normalizeFlag(target.workflowStreaming);
    target.stream_incomplete =
      normalizeFlag(snapshotLastAssistant.stream_incomplete) ||
      normalizeFlag(target.stream_incomplete);
    if (snapshotRound !== null && targetRound === null) {
      target.stream_round = snapshotRound;
    }
    if (snapshotRound !== null && targetRound !== null && snapshotRound > targetRound) {
      target.stream_round = snapshotRound;
    }
    if (snapshotEventId !== null && (targetEventId === null || snapshotEventId > targetEventId)) {
      target.stream_event_id = snapshotEventId;
    }
    if (snapshotPlan) {
      target.plan = snapshotPlan;
      target.planVisible =
        Boolean(target.planVisible) || shouldAutoShowPlan(snapshotPlan, snapshotLastAssistant);
    }
  }
  if (snapshotStats) {
    target.stats = mergeMessageStats(target.stats, snapshotStats);
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

const syncDemoChatCache = ({ sessions, sessionId, messages }) => {
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

const sortSessionsByCreatedAt = (sessions = []) =>
  (Array.isArray(sessions) ? sessions.slice() : [])
    .map((session, index) => ({ session, index }))
    .sort((a, b) => {
      const aTime = resolveTimestampMs(a.session?.created_at);
      const bTime = resolveTimestampMs(b.session?.created_at);
      if (aTime !== null && bTime !== null && aTime !== bTime) {
        return bTime - aTime;
      }
      if (aTime !== null && bTime === null) return -1;
      if (aTime === null && bTime !== null) return 1;
      return a.index - b.index;
    })
    .map((item) => item.session);

const buildGreetingMessage = (createdAt) => ({
  ...buildMessage('assistant', DEFAULT_GREETING, createdAt),
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

const ensureGreetingMessage = (messages, options = {}) => {
  const safeMessages = Array.isArray(messages) ? messages : [];
  // 无论历史会话与否，都补一条问候语，保证提示词预览入口稳定可见
  const greetingIndex = safeMessages.findIndex((message) => message?.isGreeting);
  if (greetingIndex >= 0) {
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
  return [buildGreetingMessage(greetingAt), ...safeMessages];
};

const safeJsonParse = (raw) => {
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch (error) {
    return null;
  }
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

const getSessionWorkflowState = (sessionId, options = {}) => {
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

const normalizeAssistantContent = (content) => {
  if (!content) return content;
  const payload = safeJsonParse(content);
  if (!payload) return content;
  const answer = extractAnswerFromPayload(payload);
  return answer || content;
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

const buildWorkflowEventRaw = (data, timestamp) => {
  const payload = { data: data ?? null };
  if (timestamp) {
    payload.timestamp = timestamp;
  }
  return JSON.stringify(payload);
};

const normalizeWorkflowEvents = (events, message) => {
  if (!Array.isArray(events) || events.length === 0) {
    return [];
  }
  const content = normalizeAssistantContent(message?.content || '');
  const reasoning = message?.reasoning || '';
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
    const roundIndex = Number(round?.round);
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

let resumeController = null;
let sendController = null;
let stopRequested = false;

const abortResumeStream = () => {
  if (resumeController) {
    resumeController.abort();
    resumeController = null;
  }
};

const abortSendStream = () => {
  if (sendController) {
    sendController.abort();
    sendController = null;
  }
};

const createWorkflowProcessor = (assistantMessage, workflowState, onSnapshot) => {
  const roundState = normalizeSessionWorkflowState(workflowState);
  const toolItemMap = new Map();
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
  // 思考内容需要同步到消息头部展示
  assistantMessage.reasoning = assistantMessage.reasoning || '';
  assistantMessage.reasoningStreaming = normalizeFlag(assistantMessage.reasoningStreaming);
  const normalizedPlan = normalizePlanPayload(assistantMessage.plan);
  assistantMessage.plan = normalizedPlan;
  assistantMessage.planVisible =
    Boolean(assistantMessage.planVisible) || shouldAutoShowPlan(normalizedPlan, assistantMessage);
  const stats = ensureMessageStats(assistantMessage);
  let outputContent = assistantMessage.content || '';
  let outputReasoning = assistantMessage.reasoning || '';
  const existingOutput = assistantMessage.workflowItems?.find((item) => item.title === '模型输出');
  if (existingOutput) {
    outputItemId = existingOutput.id;
  }

  const syncReasoningToMessage = () => {
    assistantMessage.reasoning = outputReasoning;
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
    if (isFileChangeTool(toolName)) {
      stats.fileChanges = normalizeStatsCount(stats.fileChanges) + 1;
    }
  };

  const updateUsageStats = (usagePayload, prefillDuration, decodeDuration) => {
    if (!stats) return;
    const normalizedUsage = normalizeUsagePayload(usagePayload);
    if (normalizedUsage) {
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
    const roundValue = data?.round ?? payload?.round;
    const roundNumber = Number(roundValue);
    if (Number.isFinite(roundNumber)) {
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
    if (outputReasoning) {
      parts.push(`[思考过程]\n${tailText(outputReasoning)}`);
    }
    if (outputContent) {
      parts.push(`[模型输出]\n${tailText(outputContent)}`);
    }
    if (!parts.length) {
      return tailText(assistantMessage.content || '');
    }
    return parts.join('\n\n');
  };

  let pendingContent = '';
  let pendingReasoning = '';
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
    const hasContentDelta = Boolean(pendingContent);
    const hasReasoningDelta = Boolean(pendingReasoning);
    if (!hasContentDelta && !hasReasoningDelta && !force) {
      return;
    }
    if (hasReasoningDelta) {
      outputReasoning += pendingReasoning;
      pendingReasoning = '';
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
    pendingReasoning = '';
  };

  const notifySnapshot = () => {
    if (typeof onSnapshot === 'function') {
      onSnapshot();
    }
  };

  const clearVisibleOutput = () => {
    resetStreamPending();
    assistantMessage.content = '';
    outputContent = '';
    outputReasoning = '';
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

  const handleEvent = (eventName, raw) => {
    const payload = safeJsonParse(raw);
    const data = payload?.data ?? payload;
    const eventType = resolveEventType(eventName, payload);

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
        const delta = data?.delta ?? payload?.delta ?? data?.content ?? payload?.content ?? '';
        const reasoningDelta = data?.reasoning_delta ?? payload?.reasoning_delta ?? '';
        const reasoningDeltaText =
          typeof reasoningDelta === 'string' ? reasoningDelta : reasoningDelta ? String(reasoningDelta) : '';
        if (reasoningDeltaText) {
          pendingReasoning += reasoningDeltaText;
          outputState.reasoningStreaming = true;
        }
        if (typeof delta === 'string' && delta) {
          pendingContent += delta;
          outputState.streaming = true;
        }
        if (pendingContent || pendingReasoning) {
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
          '';
        const reasoningText =
          typeof reasoningRaw === 'string' ? reasoningRaw : reasoningRaw ? String(reasoningRaw) : '';
        const hasContent = typeof content === 'string' && content !== '';
        const toolCallsPayload =
          data?.tool_calls ??
          payload?.tool_calls ??
          data?.tool_call ??
          payload?.tool_call ??
          data?.function_call ??
          payload?.function_call;
        const toolCallAnswer = !hasContent ? extractFinalAnswerFromToolCalls(toolCallsPayload) : '';
        const resolvedContent = hasContent ? content : toolCallAnswer;
        const resolvedHasContent =
          typeof resolvedContent === 'string' && resolvedContent !== '';
        const hasReasoning = reasoningText !== '';
        if (
          !resolvedHasContent &&
          !hasReasoning &&
          (outputState.streaming || outputState.reasoningStreaming)
        ) {
          outputState.streaming = false;
          outputState.reasoningStreaming = false;
        } else {
          if (hasReasoning) {
            outputReasoning = reasoningText;
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
          assistantMessage.content = answerText;
          outputContent = answerText;
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
        const detail = data?.message ?? payload?.message ?? raw ?? '发生错误';
        assistantMessage.workflowItems.push(
          buildWorkflowItem('错误', pickText(detail), 'failed')
        );
        if (!assistantMessage.content) {
          assistantMessage.content = '发生错误，请稍后再试。';
        }
        break;
      }
      default: {
        const fallbackName = data?.name ?? payload?.name;
        const summary = fallbackName ? `${eventType}: ${fallbackName}` : `事件：${eventType}`;
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
    notifySnapshot();
  };

  return { handleEvent, finalize };
};

const hydrateMessage = (message, workflowState) => {
  if (!message || message.role !== 'assistant') {
    return message;
  }
  const hydrated = {
    ...message,
    content: normalizeAssistantContent(message.content),
    workflowItems: [],
    workflowStreaming: normalizeFlag(message?.workflowStreaming),
    stream_incomplete: normalizeFlag(message?.stream_incomplete),
    reasoning: message?.reasoning || '',
    reasoningStreaming: normalizeFlag(message?.reasoningStreaming),
    stats: normalizeMessageStats(message.stats) || buildMessageStats()
  };
  const plan = normalizePlanPayload(message.plan);
  hydrated.plan = plan;
  hydrated.planVisible = shouldAutoShowPlan(plan, message);
  if (Array.isArray(message.workflow_events) && message.workflow_events.length > 0) {
    const processor = createWorkflowProcessor(hydrated, workflowState, null);
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
    loading: false
  }),
  actions: {
    markPageUnloading() {
      pageUnloading = true;
    },
    getPersistedState() {
      return readChatPersistState();
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
    async loadSessions() {
      const { data } = await listSessions();
      this.sessions = sortSessionsByCreatedAt(data.data.items || []);
      syncDemoChatCache({ sessions: this.sessions });
      return this.sessions;
    },
    openDraftSession() {
      abortResumeStream();
      abortSendStream();
      stopRequested = false;
      this.loading = false;
      this.activeSessionId = null;
      this.messages = ensureGreetingMessage([]);
      persistDraftSession();
    },
    async createSession(payload = {}) {
      abortResumeStream();
      const { data } = await createSession(payload);
      const session = data.data;
      this.sessions.unshift(session);
      this.activeSessionId = session.id;
      this.messages = ensureGreetingMessage([], { createdAt: session.created_at });
      getSessionWorkflowState(session.id, { reset: true });
      persistActiveSession(session.id);
      syncDemoChatCache({
        sessions: this.sessions,
        sessionId: this.activeSessionId,
        messages: this.messages
      });
      return session;
    },
    async loadSessionDetail(sessionId) {
      abortResumeStream();
      this.activeSessionId = sessionId;
      persistActiveSession(sessionId);
      const snapshot = this.getSnapshotForSession(sessionId);
      if (snapshot?.messages?.length) {
        const cachedMessages = snapshot.messages
          .map((item) => normalizeSnapshotMessage(item))
          .filter(Boolean);
        this.messages = ensureGreetingMessage(cachedMessages);
      }
      const [sessionRes, eventsRes] = await Promise.all([
        getSession(sessionId),
        getSessionEvents(sessionId).catch(() => null)
      ]);
      const data = sessionRes?.data;
      const sessionCreatedAt = data?.data?.created_at;
      const rounds = eventsRes?.data?.data?.rounds || [];
      const workflowState = getSessionWorkflowState(sessionId, { reset: true });
      const rawMessages = attachWorkflowEvents(data?.data?.messages || [], rounds);
      let messages = rawMessages.map((message) =>
        hydrateMessage(message, workflowState)
      );
      messages = mergeSnapshotIntoMessages(messages, snapshot);
      this.messages = ensureGreetingMessage(messages, { createdAt: sessionCreatedAt });
      syncDemoChatCache({ sessionId: sessionId, messages: this.messages });
      const pendingMessage = [...this.messages]
        .reverse()
        .find((message) => message.role === 'assistant' && message.stream_incomplete);
      if (pendingMessage) {
        this.resumeStream(sessionId, pendingMessage);
      }
      this.scheduleSnapshot(true);
      return data.data;
    },
    async deleteSession(sessionId) {
      const targetId = sessionId || this.activeSessionId;
      if (!targetId) return;
      abortResumeStream();
      abortSendStream();
      await deleteSessionApi(targetId);
      this.sessions = this.sessions.filter((item) => item.id !== targetId);
      sessionWorkflowState.delete(String(targetId));
      removeDemoChatSession(targetId);
      clearChatSnapshot(targetId);
      if (this.activeSessionId === targetId) {
        if (this.sessions.length > 0) {
          await this.loadSessionDetail(this.sessions[0].id);
        } else {
          this.openDraftSession();
        }
      }
    },
    async sendMessage(content, options = {}) {
      abortResumeStream();
      abortSendStream();
      stopRequested = false;
      if (!this.activeSessionId) {
        await this.createSession();
      }
      const userMessage = buildMessage('user', content);
      this.messages.push(userMessage);
      const authStore = useAuthStore();
      // 共享工具需要用户勾选后才允许注入，默认不启用
      const sharedSelection = Array.from(
        loadSharedToolSelection(authStore.user?.id).values()
      );
      const attachments = Array.isArray(options.attachments) ? options.attachments : [];

      const activeSession = this.sessions.find((item) => item.id === this.activeSessionId);
      if (activeSession && shouldAutoTitle(activeSession.title)) {
        const autoTitle = buildSessionTitle(content);
        if (autoTitle) {
          activeSession.title = autoTitle;
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
      this.messages.push(assistantMessageRaw);
      const assistantMessage = this.messages[this.messages.length - 1];
      this.scheduleSnapshot(true);

      this.loading = true;

      const workflowState = getSessionWorkflowState(this.activeSessionId);
      const processor = createWorkflowProcessor(
        assistantMessage,
        workflowState,
        () => this.scheduleSnapshot()
      );

      try {
        sendController = new AbortController();
        const response = await sendMessageStream(
          this.activeSessionId,
          {
            content,
            stream: true,
            selected_shared_tools: sharedSelection,
            ...(attachments.length > 0 ? { attachments } : {})
          },
          {
            signal: sendController.signal
          }
        );
        if (!response.ok) {
          const errorText = await response.text();
          throw new Error(errorText || `请求失败 (${response.status})`);
        }
        await consumeSseStream(response, (eventType, dataText, eventId) => {
          assignStreamEventId(assistantMessage, eventId);
          processor.handleEvent(eventType, dataText);
        });
      } catch (error) {
        if (error?.name === 'AbortError' || stopRequested || pageUnloading) {
          if (!pageUnloading) {
            assistantMessage.workflowItems.push(
              buildWorkflowItem('已终止', '用户终止了当前请求', 'failed')
            );
          }
        } else {
          assistantMessage.workflowItems.push(
            buildWorkflowItem('请求失败', error?.message || '无法获取回复', 'failed')
          );
          if (!assistantMessage.content) {
            assistantMessage.content = '请求失败，请稍后再试。';
          }
        }
      } finally {
        assistantMessage.workflowStreaming = false;
        assistantMessage.stream_incomplete = false;
        this.loading = false;
        processor.finalize();
        sendController = null;
        stopRequested = false;
        syncDemoChatCache({
          sessions: this.sessions,
          sessionId: this.activeSessionId,
          messages: this.messages
        });
        this.scheduleSnapshot(true);
      }
    },
    async stopStream() {
      if (!this.activeSessionId) {
        return;
      }
      stopRequested = true;
      abortSendStream();
      try {
        await cancelMessageStream(this.activeSessionId);
      } catch (error) {
        // 终止失败时仅记录状态，不影响前端中止动作
      }
      this.loading = false;
    },
    async resumeStream(sessionId, message) {
      if (!message || !message.stream_incomplete) return;
      this.loading = true;
      message.workflowStreaming = true;
      message.stream_incomplete = true;
      this.scheduleSnapshot();
      const workflowState = getSessionWorkflowState(sessionId);
      const processor = createWorkflowProcessor(message, workflowState, () => this.scheduleSnapshot());
      abortResumeStream();
      resumeController = new AbortController();
      let aborted = false;
      const afterEventId = normalizeStreamEventId(message.stream_event_id);
      try {
        const response = await resumeMessageStream(sessionId, {
          signal: resumeController.signal,
          afterEventId
        });
        if (!response.ok) {
          const errorText = await response.text();
          throw new Error(errorText || `恢复失败 (${response.status})`);
        }
        await consumeSseStream(response, (eventType, dataText, eventId) => {
          assignStreamEventId(message, eventId);
          processor.handleEvent(eventType, dataText);
        });
      } catch (error) {
        if (error?.name === 'AbortError') {
          aborted = true;
        } else {
          message.workflowItems.push(
            buildWorkflowItem('恢复失败', error?.message || '无法续传会话', 'failed')
          );
          if (!message.content) {
            message.content = '会话续传失败，请稍后再试。';
          }
        }
      } finally {
        message.workflowStreaming = false;
        if (!aborted) {
          message.stream_incomplete = false;
        }
        this.loading = false;
        processor.finalize();
        if (!aborted) {
          resumeController = null;
        }
        this.scheduleSnapshot(true);
      }
    }
  }
});
