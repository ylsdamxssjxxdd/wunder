import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260118-07";
import { state } from "./state.js";
import { appendLog, appendRequestLog, clearOutput } from "./log.js?v=20260108-02";
import { applyA2uiMessages, resetA2uiState } from "./a2ui.js";
import { getWunderBase } from "./api.js";
import { applyPromptToolError, ensureToolSelectionLoaded, getSelectedToolNames } from "./tools.js?v=20251227-13";
import { loadWorkspace } from "./workspace.js?v=20260118-07";
import { notify } from "./notify.js";
import { formatTimestamp } from "./utils.js?v=20251229-02";
import { ensureLlmConfigLoaded } from "./llm.js";
import { getCurrentLanguage, t } from "./i18n.js?v=20260118-07";

const DEBUG_STATE_KEY = "wunder_debug_state";
const DEBUG_ACTIVE_STATUSES = new Set(["running", "cancelling"]);
const MIN_PREFILL_DURATION_S = 0.05;
const STOP_REASON_LABELS = {
  "zh-CN": {
    model_response: "正常结束",
    final_tool: "最终回复工具",
    a2ui: "A2UI 工具",
    max_rounds: "达到最大轮次",
    unknown: "未知",
  },
  "en-US": {
    model_response: "Normal completion",
    final_tool: "Final reply tool",
    a2ui: "A2UI tool",
    max_rounds: "Max rounds reached",
    unknown: "Unknown",
  },
};
// 调试面板附件支持：图片走多模态，文件走 doc2md 解析
const DEBUG_IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg"]);
const DEBUG_DOC_EXTENSIONS = [
  ".txt",
  ".md",
  ".markdown",
  ".html",
  ".htm",
  ".py",
  ".c",
  ".cpp",
  ".cc",
  ".h",
  ".hpp",
  ".json",
  ".js",
  ".ts",
  ".css",
  ".ini",
  ".cfg",
  ".log",
  ".doc",
  ".docx",
  ".odt",
  ".pptx",
  ".odp",
  ".xlsx",
  ".ods",
  ".wps",
  ".et",
  ".dps",
];
const DEBUG_UPLOAD_ACCEPT = ["image/*", ...DEBUG_DOC_EXTENSIONS].join(",");
const resolveQuestionPresets = () => {
  const raw = APP_CONFIG.debugQuestionPresets;
  if (Array.isArray(raw)) {
    return raw;
  }
  if (raw && typeof raw === "object") {
    const language = getCurrentLanguage();
    if (Array.isArray(raw[language])) {
      return raw[language];
    }
    if (Array.isArray(raw["zh-CN"])) {
      return raw["zh-CN"];
    }
    if (Array.isArray(raw["en-US"])) {
      return raw["en-US"];
    }
  }
  return [];
};
const DEBUG_RESTORE_EVENT_TYPES = new Set([
  "progress",
  "compaction",
  "tool_call",
  "tool_result",
  "plan_update",
  "question_panel",
  "llm_request",
  "llm_response",
  "knowledge_request",
  "llm_output_delta",
  "llm_output",
  "llm_stream_retry",
  // Token 用量事件在刷新后也需要保留，避免调试日志丢失
  "token_usage",
  "a2ui",
  "final",
  "error",
]);

// 模型输出文本区可能拆成独立容器，优先使用专用节点。
const resolveModelOutputText = () => elements.modelOutputText || elements.modelOutput;
// 滚动应作用于外层容器，避免 <pre> 本身不滚动。
const resolveModelOutputScrollContainer = () => elements.modelOutput || resolveModelOutputText();
// 缓冲模型输出，降低频繁 DOM 拼接导致的卡顿
const modelOutputBuffer = {
  chunks: [],
  scheduled: false,
  pendingScroll: false,
  rafId: 0,
};
// 预览弹窗状态：记录 markdown 渲染器初始化状态
const outputPreviewState = {
  markedReady: false,
};
const debugAttachments = [];
let debugAttachmentBusy = 0;
let debugStats = null;
const pendingRequestLogs = [];
let pendingRequestSeq = 0;
let stopReasonHint = "";

const resetStopReasonHint = () => {
  stopReasonHint = "";
};

const resolveStopReasonLabel = (reason) => {
  const normalized = String(reason || "").trim();
  if (!normalized) {
    return "";
  }
  const language = getCurrentLanguage();
  const labels = STOP_REASON_LABELS[language] || STOP_REASON_LABELS["zh-CN"];
  return labels?.[normalized] || normalized;
};

// 重置请求-回复关联状态，避免日志错位
const resetPendingRequestLogs = () => {
  pendingRequestLogs.length = 0;
  pendingRequestSeq = 0;
};

const buildResponseText = (data) => {
  if (!data || typeof data !== "object") {
    return t("debug.response.empty");
  }
  const content = data.content ? String(data.content) : "";
  const reasoning = data.reasoning ? String(data.reasoning) : data.reasoning_content ? String(data.reasoning_content) : "";
  const toolCallsText = !content ? formatToolCalls(data.tool_calls) : "";
  const sections = [];
  if (reasoning) {
    sections.push(`${t("debug.response.thought")}\n${reasoning}`);
  }
  if (content) {
    sections.push(content);
  }
  if (!content && toolCallsText) {
    sections.push(`${t("debug.response.toolCalls")}\n${toolCallsText}`);
  }
  if (!sections.length) {
    return t("debug.response.empty");
  }
  return sections.join("\n\n");
};

const parseJsonIfPossible = (value) => {
  if (typeof value !== "string") {
    return value;
  }
  const text = value.trim();
  if (!text) {
    return value;
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    return value;
  }
};

const normalizeToolCallEntry = (entry) => {
  if (!entry || typeof entry !== "object") {
    return entry;
  }
  const functionValue = entry.function && typeof entry.function === "object" ? entry.function : null;
  const name =
    functionValue?.name ||
    entry.name ||
    entry.tool ||
    entry.tool_name ||
    entry.toolName ||
    entry.function_name ||
    entry.functionName ||
    "";
  const rawArgs =
    functionValue?.arguments ??
    entry.arguments ??
    entry.args ??
    entry.parameters ??
    entry.params ??
    entry.input ??
    entry.payload;
  const normalizedArgs = parseJsonIfPossible(rawArgs ?? {});
  const id = entry.id || entry.tool_call_id || entry.toolCallId || entry.call_id || entry.callId || "";
  const output = {};
  if (id) {
    output.id = id;
  }
  if (name) {
    output.name = name;
  }
  output.arguments = normalizedArgs;
  return output;
};

const normalizeToolCallsPayload = (value) => {
  if (value === null || value === undefined) {
    return null;
  }
  const parsed = parseJsonIfPossible(value);
  if (Array.isArray(parsed)) {
    return parsed.map(normalizeToolCallEntry);
  }
  if (parsed && typeof parsed === "object") {
    if (Array.isArray(parsed.tool_calls)) {
      return parsed.tool_calls.map(normalizeToolCallEntry);
    }
    if (parsed.tool_calls) {
      return normalizeToolCallEntry(parsed.tool_calls);
    }
    if (parsed.tool_call) {
      return normalizeToolCallEntry(parsed.tool_call);
    }
    if (parsed.function_call) {
      return normalizeToolCallEntry(parsed.function_call);
    }
  }
  return normalizeToolCallEntry(parsed);
};

const formatToolCalls = (value) => {
  const normalized = normalizeToolCallsPayload(value);
  if (normalized === null || normalized === undefined || normalized === "") {
    return "";
  }
  if (typeof normalized === "string") {
    return normalized;
  }
  try {
    return JSON.stringify(normalized, null, 2);
  } catch (error) {
    return String(normalized);
  }
};

// 在请求日志条目上补充耗时标签，保持与事件日志展示一致
const appendRequestDurationBadge = (item, durationText) => {
  if (!item || !durationText || durationText === "-") {
    return;
  }
  const summary = item.querySelector("summary");
  if (!summary) {
    return;
  }
  let rightWrap = summary.querySelector(".log-right");
  if (!rightWrap) {
    rightWrap = document.createElement("span");
    rightWrap.className = "log-right";
    summary.appendChild(rightWrap);
  }
  let durationNode = rightWrap.querySelector(".log-duration");
  if (!durationNode) {
    durationNode = document.createElement("span");
    durationNode.className = "log-duration";
    const durationLabel = document.createElement("span");
    durationLabel.className = "log-duration-label";
    durationLabel.textContent = t("log.duration");
    const durationValue = document.createElement("span");
    durationValue.className = "log-duration-value";
    durationValue.textContent = durationText;
    durationNode.appendChild(durationLabel);
    durationNode.appendChild(durationValue);
    rightWrap.appendChild(durationNode);
  } else {
    const durationValue = durationNode.querySelector(".log-duration-value");
    if (durationValue) {
      durationValue.textContent = durationText;
    }
  }
};

const attachResponseToRequest = (response, options = {}) => {
  if (!pendingRequestLogs.length) {
    return;
  }
  const entry = pendingRequestLogs.shift();
  if (!entry || !entry.item) {
    return;
  }
  if (entry.responseAttached) {
    return;
  }
  entry.responseAttached = true;
  if (Number.isFinite(entry.requestTimestampMs)) {
    const responseTimestampMs = resolveTimestampMs(options.timestamp);
    const endTimestampMs = Number.isFinite(responseTimestampMs) ? responseTimestampMs : Date.now();
    const durationText = formatDurationSeconds(entry.requestTimestampMs, endTimestampMs);
    appendRequestDurationBadge(entry.item, durationText);
  }
  const responseText = buildResponseText(response);
  const detailNode = entry.item.querySelector(".log-detail");
  if (!detailNode) {
    return;
  }
  const responseNode = document.createElement("div");
  responseNode.className = "log-response";
  responseNode.textContent = `${t("debug.response.title")}\n${responseText}`;
  detailNode.appendChild(responseNode);
};

const finalizePendingRequestDurations = (timestamp) => {
  if (!pendingRequestLogs.length) {
    return;
  }
  const endTimestampMs = resolveTimestampMs(timestamp);
  const endMs = Number.isFinite(endTimestampMs) ? endTimestampMs : Date.now();
  pendingRequestLogs.forEach((entry) => {
    if (!entry || !entry.item || entry.responseAttached) {
      return;
    }
    entry.responseAttached = true;
    if (Number.isFinite(entry.requestTimestampMs)) {
      const durationText = formatDurationSeconds(entry.requestTimestampMs, endMs);
      appendRequestDurationBadge(entry.item, durationText);
    }
  });
  pendingRequestLogs.length = 0;
};

const flushPendingRequests = (message, options = {}) => {
  if (!pendingRequestLogs.length) {
    return;
  }
  const content = message
    ? t("debug.request.error", { message })
    : t("debug.request.errorNoResponse");
  while (pendingRequestLogs.length) {
    attachResponseToRequest({ content, reasoning: "" }, options);
  }
};

// 控制调试日志等待态，便于管理员判断对话是否仍在进行
const setDebugLogWaiting = (waiting) => {
  [elements.eventLog, elements.requestLog].forEach((target) => {
    if (!target) {
      return;
    }
    const card = target.closest(".log-card");
    if (card) {
      card.classList.toggle("is-waiting", waiting);
    }
    target.setAttribute("aria-busy", waiting ? "true" : "false");
  });
};

const setSendToggleState = (active) => {
  if (!elements.sendBtn) {
    return;
  }
  const isStop = Boolean(active);
  const icon = elements.sendBtn.querySelector("i");
  if (icon) {
    icon.className = isStop ? "fa-solid fa-stop" : "fa-solid fa-paper-plane";
  }
  elements.sendBtn.classList.toggle("danger", isStop);
  const label = isStop ? t("debug.send.stop") : t("debug.send.send");
  elements.sendBtn.setAttribute("aria-label", label);
  elements.sendBtn.title = label;
};

const updateDebugLogWaiting = (force) => {
  if (typeof force === "boolean") {
    setDebugLogWaiting(force);
    setSendToggleState(force);
    return;
  }
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldWait = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  setDebugLogWaiting(shouldWait);
  setSendToggleState(shouldWait);
};

// 初始化统计信息结构，便于调试面板复用
const createDebugStats = () => ({
  tokenInput: 0,
  tokenOutput: 0,
  tokenTotal: 0,
  prefillTokens: 0,
  prefillDuration: 0,
  decodeTokens: 0,
  decodeDuration: 0,
  llmRounds: {},
  firstRound: null,
  latestRound: null,
  lastRoundSeen: null,
  implicitRound: 0,
  toolCalls: 0,
  toolOk: 0,
  toolFailed: 0,
  sandboxCalls: 0,
  llmRequests: 0,
  knowledgeRequests: 0,
  errorCount: 0,
  eventCount: 0,
  hasTokenUsage: false,
  timeRangeStartMs: null,
  timeRangeEndMs: null,
  requestStartMs: null,
  requestEndMs: null,
});

const createRoundMetrics = () => ({
  startMs: null,
  firstOutputMs: null,
  lastOutputMs: null,
  inputTokens: null,
  outputTokens: null,
  prefillDuration: null,
  decodeDuration: null,
});

const resetDebugStats = () => {
  debugStats = createDebugStats();
  resetStopReasonHint();
  renderDebugStats();
};

const resetLlmRoundMetrics = () => {
  if (!debugStats) {
    return;
  }
  debugStats.prefillTokens = 0;
  debugStats.prefillDuration = 0;
  debugStats.decodeTokens = 0;
  debugStats.decodeDuration = 0;
  debugStats.llmRounds = {};
  debugStats.firstRound = null;
  debugStats.latestRound = null;
  debugStats.lastRoundSeen = null;
  debugStats.implicitRound = 0;
  debugStats.requestStartMs = null;
  debugStats.requestEndMs = null;
};

const isRequestBoundaryEvent = (eventType, payload) => {
  if (eventType === "round_start" || eventType === "received") {
    return true;
  }
  if (eventType !== "progress") {
    return false;
  }
  const data = payload?.data || payload;
  return String(data?.stage || "").trim() === "start";
};

const parseOptionalNumber = (value) => {
  if (value === null || value === undefined) {
    return null;
  }
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const resolveRoundNumber = (value) => parseOptionalNumber(value);

const parseUsageTokens = (usage) => {
  if (!usage || typeof usage !== "object") {
    return { inputTokens: null, outputTokens: null };
  }
  return {
    inputTokens: parseOptionalNumber(usage.input_tokens ?? usage.input),
    outputTokens: parseOptionalNumber(usage.output_tokens ?? usage.output),
  };
};

const ensureRoundMetrics = (round) => {
  if (!debugStats) {
    return null;
  }
  const key = String(round);
  if (!debugStats.llmRounds[key]) {
    debugStats.llmRounds[key] = createRoundMetrics();
  }
  return debugStats.llmRounds[key];
};

const resolveRoundDuration = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return null;
  }
  return (endMs - startMs) / 1000;
};

const recomputeSpeedSummary = () => {
  if (!debugStats) {
    return;
  }
  const roundIds = Object.keys(debugStats.llmRounds || {})
    .map((key) => Number(key))
    .filter((value) => Number.isFinite(value));
  if (roundIds.length === 0) {
    debugStats.prefillTokens = 0;
    debugStats.prefillDuration = 0;
    debugStats.decodeTokens = 0;
    debugStats.decodeDuration = 0;
    return;
  }
  let earliestStartMs = null;
  let earliestOutputMs = null;
  let latestOutputMs = null;
  let earliestOutputRound = null;
  let outputTokensTotal = 0;
  roundIds.forEach((round) => {
    const metrics = debugStats.llmRounds[String(round)];
    if (!metrics) {
      return;
    }
    if (Number.isFinite(metrics.startMs)) {
      if (earliestStartMs === null || metrics.startMs < earliestStartMs) {
        earliestStartMs = metrics.startMs;
      }
    }
    if (Number.isFinite(metrics.firstOutputMs)) {
      if (earliestOutputMs === null || metrics.firstOutputMs < earliestOutputMs) {
        earliestOutputMs = metrics.firstOutputMs;
        earliestOutputRound = round;
      }
    }
    if (Number.isFinite(metrics.lastOutputMs)) {
      if (latestOutputMs === null || metrics.lastOutputMs > latestOutputMs) {
        latestOutputMs = metrics.lastOutputMs;
      }
    }
    if (Number.isFinite(metrics.outputTokens) && metrics.outputTokens > 0) {
      outputTokensTotal += metrics.outputTokens;
    }
  });
  const firstRound = Number.isFinite(earliestOutputRound)
    ? earliestOutputRound
    : Number.isFinite(debugStats.firstRound)
      ? debugStats.firstRound
      : Math.min(...roundIds);
  const latestRound = Number.isFinite(debugStats.latestRound)
    ? debugStats.latestRound
    : Number.isFinite(debugStats.lastRoundSeen)
      ? debugStats.lastRoundSeen
      : Math.max(...roundIds);
  const prefillMetrics = debugStats.llmRounds[String(firstRound)];
  const decodeMetrics = debugStats.llmRounds[String(latestRound)] || prefillMetrics;
  const prefillTokens = parseOptionalNumber(prefillMetrics?.inputTokens);
  let prefillDuration = parseOptionalNumber(prefillMetrics?.prefillDuration);
  let prefillStartMs = null;
  if (Number.isFinite(prefillMetrics?.startMs)) {
    prefillStartMs = prefillMetrics.startMs;
  }
  if (Number.isFinite(debugStats.requestStartMs)) {
    prefillStartMs =
      prefillStartMs === null
        ? debugStats.requestStartMs
        : Math.min(prefillStartMs, debugStats.requestStartMs);
  }
  if (Number.isFinite(earliestStartMs)) {
    prefillStartMs =
      prefillStartMs === null ? earliestStartMs : Math.min(prefillStartMs, earliestStartMs);
  }
  const prefillFirstOutputMs = Number.isFinite(prefillMetrics?.firstOutputMs)
    ? prefillMetrics?.firstOutputMs
    : earliestOutputMs;
  const observedPrefill = resolveRoundDuration(prefillStartMs, prefillFirstOutputMs);
  if (
    observedPrefill !== null &&
    (prefillDuration === null || observedPrefill > prefillDuration)
  ) {
    prefillDuration = observedPrefill;
  }
  if (prefillDuration !== null && prefillDuration < MIN_PREFILL_DURATION_S) {
    prefillDuration = MIN_PREFILL_DURATION_S;
  }
  const decodeTokens =
    outputTokensTotal > 0 ? outputTokensTotal : parseOptionalNumber(decodeMetrics?.outputTokens);
  let decodeDuration = resolveRoundDuration(earliestOutputMs, latestOutputMs);
  if (decodeDuration !== null && decodeDuration <= 0) {
    decodeDuration = null;
  }
  if (decodeDuration === null) {
    decodeDuration = parseOptionalNumber(decodeMetrics?.decodeDuration);
  }
  if (decodeDuration === null) {
    decodeDuration = resolveRoundDuration(
      decodeMetrics?.firstOutputMs,
      decodeMetrics?.lastOutputMs
    );
  }
  debugStats.prefillTokens = Number.isFinite(prefillTokens) ? prefillTokens : 0;
  debugStats.prefillDuration = Number.isFinite(prefillDuration) ? prefillDuration : 0;
  debugStats.decodeTokens = Number.isFinite(decodeTokens) ? decodeTokens : 0;
  debugStats.decodeDuration = Number.isFinite(decodeDuration) ? decodeDuration : 0;
};

const updateLlmRoundMetrics = (eventType, payload, timestamp) => {
  if (!debugStats) {
    return;
  }
  if (isRequestBoundaryEvent(eventType, payload)) {
    resetLlmRoundMetrics();
    const boundaryMs = resolveTimestampMs(timestamp);
    if (Number.isFinite(boundaryMs)) {
      debugStats.requestStartMs = boundaryMs;
    }
    return;
  }
  if (!["llm_request", "llm_output_delta", "llm_output", "token_usage"].includes(eventType)) {
    return;
  }
  const data = payload?.data || payload;
  let round = resolveRoundNumber(data?.round);
  if (
    Number.isFinite(round) &&
    Number.isFinite(debugStats.lastRoundSeen) &&
    round < debugStats.lastRoundSeen
  ) {
    resetLlmRoundMetrics();
    const boundaryMs = resolveTimestampMs(timestamp);
    if (Number.isFinite(boundaryMs)) {
      debugStats.requestStartMs = boundaryMs;
    }
  }
  if (eventType === "llm_request" && round === null) {
    debugStats.implicitRound += 1;
    round = debugStats.implicitRound;
  }
  if (round === null) {
    round = debugStats.lastRoundSeen;
  }
  if (round === null) {
    return;
  }
  debugStats.lastRoundSeen = round;
  if (!Number.isFinite(debugStats.firstRound)) {
    debugStats.firstRound = round;
  }
  const metrics = ensureRoundMetrics(round);
  if (!metrics) {
    return;
  }
  const tsMs = resolveTimestampMs(timestamp) ?? Date.now();
  if (eventType === "llm_request") {
    if (!Number.isFinite(metrics.startMs)) {
      metrics.startMs = tsMs;
    }
  } else if (eventType === "llm_output_delta" || eventType === "llm_output") {
    if (!Number.isFinite(metrics.firstOutputMs)) {
      metrics.firstOutputMs = tsMs;
    }
    metrics.lastOutputMs = tsMs;
    if (eventType === "llm_output") {
      const { inputTokens, outputTokens } = parseUsageTokens(data?.usage);
      if (metrics.inputTokens === null && inputTokens !== null) {
        metrics.inputTokens = inputTokens;
      }
      if (metrics.outputTokens === null && outputTokens !== null) {
        metrics.outputTokens = outputTokens;
      }
      const prefillDuration = parseOptionalNumber(data?.prefill_duration_s);
      if (metrics.prefillDuration === null && prefillDuration !== null) {
        metrics.prefillDuration = prefillDuration;
      }
      const decodeDuration = parseOptionalNumber(data?.decode_duration_s);
      if (metrics.decodeDuration === null && decodeDuration !== null) {
        metrics.decodeDuration = decodeDuration;
      }
    }
  } else if (eventType === "token_usage") {
    const inputTokens = parseOptionalNumber(data?.input_tokens);
    const outputTokens = parseOptionalNumber(data?.output_tokens);
    if (metrics.inputTokens === null && inputTokens !== null) {
      metrics.inputTokens = inputTokens;
    }
    if (metrics.outputTokens === null && outputTokens !== null) {
      metrics.outputTokens = outputTokens;
    }
    const prefillDuration = parseOptionalNumber(data?.prefill_duration_s);
    if (metrics.prefillDuration === null && prefillDuration !== null) {
      metrics.prefillDuration = prefillDuration;
    }
    const decodeDuration = parseOptionalNumber(data?.decode_duration_s);
    if (metrics.decodeDuration === null && decodeDuration !== null) {
      metrics.decodeDuration = decodeDuration;
    }
  }
  if (metrics.outputTokens !== null && metrics.outputTokens > 0) {
    debugStats.latestRound = round;
  }
  recomputeSpeedSummary();
};

const formatStatNumber = (value, fallback = "-") => {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return parsed.toLocaleString();
};

const formatTokenRate = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const tokens = Math.max(0, Number(value));
  const useMillion = tokens >= 1_000_000;
  const useThousand = tokens >= 1_000 && tokens < 1_000_000;
  const base = useMillion ? 1_000_000 : useThousand ? 1_000 : 1;
  const unit = useMillion ? "m" : useThousand ? "k" : "";
  const scaled = tokens / base;
  let decimals = 2;
  if (scaled >= 100) {
    decimals = 0;
  } else if (scaled >= 10) {
    decimals = 1;
  }
  return `${scaled.toFixed(decimals)}${unit} ${t("monitor.detail.tokenRate.unit")}`;
};

const normalizeTimestampText = (value) => {
  if (!value) {
    return "";
  }
  const text = String(value).trim();
  if (!text) {
    return "";
  }
  const match = text.match(
    /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$/
  );
  if (!match) {
    return text;
  }
  const base = match[1];
  const fraction = match[2];
  const zone = match[3] || "";
  let normalized = base;
  if (fraction) {
    const digits = fraction.slice(1, 4);
    if (digits) {
      normalized += `.${digits}`;
    }
  }
  normalized += zone;
  return normalized;
};

// 解析事件时间为毫秒，用于统计会话耗时
const resolveTimestampMs = (value) => {
  if (!value) {
    return null;
  }
  if (value instanceof Date) {
    return value.getTime();
  }
  if (typeof value === "number") {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  if (typeof value === "string") {
    const parsed = new Date(normalizeTimestampText(value));
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  return null;
};

// 记录事件时间范围，计算整体耗时
const applyEventTimestamp = (timestamp) => {
  const ts = resolveTimestampMs(timestamp);
  if (!Number.isFinite(ts)) {
    return;
  }
  if (!Number.isFinite(debugStats.timeRangeStartMs) || ts < debugStats.timeRangeStartMs) {
    debugStats.timeRangeStartMs = ts;
  }
  if (!Number.isFinite(debugStats.timeRangeEndMs) || ts > debugStats.timeRangeEndMs) {
    debugStats.timeRangeEndMs = ts;
  }
};

// 请求开始/结束时补齐时间范围，避免无事件时耗时为空
const markRequestStart = () => {
  const now = Date.now();
  debugStats.requestStartMs = now;
  if (!Number.isFinite(debugStats.timeRangeStartMs)) {
    debugStats.timeRangeStartMs = now;
  }
};

const markRequestEnd = () => {
  const now = Date.now();
  debugStats.requestEndMs = now;
  if (!Number.isFinite(debugStats.timeRangeEndMs) || now > debugStats.timeRangeEndMs) {
    debugStats.timeRangeEndMs = now;
  }
};

const formatDurationSeconds = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return "-";
  }
  return `${((endMs - startMs) / 1000).toFixed(2)}s`;
};

const renderDebugStats = () => {
  if (!elements.finalAnswer) {
    return;
  }
  if (!debugStats) {
    elements.finalAnswer.textContent = t("debug.stats.empty");
    return;
  }
  const sessionId = String(state.runtime.debugSessionId || "").trim();
  const tokenText = debugStats.hasTokenUsage
    ? t("debug.stats.tokenUsage", {
        total: formatStatNumber(debugStats.tokenTotal, "0"),
        input: formatStatNumber(debugStats.tokenInput, "0"),
        output: formatStatNumber(debugStats.tokenOutput, "0"),
      })
    : "-";
  const prefillTokens = Number.isFinite(debugStats.prefillTokens) ? debugStats.prefillTokens : 0;
  const prefillDuration = Number.isFinite(debugStats.prefillDuration)
    ? debugStats.prefillDuration
    : 0;
  const decodeTokens = Number.isFinite(debugStats.decodeTokens) ? debugStats.decodeTokens : 0;
  const decodeDuration = Number.isFinite(debugStats.decodeDuration)
    ? debugStats.decodeDuration
    : 0;
  const prefillSpeed =
    prefillTokens > 0 && prefillDuration > 0 ? prefillTokens / prefillDuration : null;
  const decodeSpeed =
    decodeTokens > 0 && decodeDuration > 0 ? decodeTokens / decodeDuration : null;
  const prefillSpeedText = formatTokenRate(prefillSpeed);
  const decodeSpeedText = formatTokenRate(decodeSpeed);
  const toolText = t("debug.stats.toolCalls", {
    total: formatStatNumber(debugStats.toolCalls, "0"),
    ok: formatStatNumber(debugStats.toolOk, "0"),
    failed: formatStatNumber(debugStats.toolFailed, "0"),
  });
  const startMs = Number.isFinite(debugStats.timeRangeStartMs)
    ? debugStats.timeRangeStartMs
    : debugStats.requestStartMs;
  const endMs = state.runtime.debugStreaming && Number.isFinite(startMs)
    ? Date.now()
    : Number.isFinite(debugStats.timeRangeEndMs)
      ? debugStats.timeRangeEndMs
      : debugStats.requestEndMs;
  const durationText = formatDurationSeconds(startMs, endMs);

  // 使用表格呈现统计信息，提升可读性与对齐效果
  const rows = [
    { label: t("debug.stats.sessionId"), value: sessionId || "-" },
    { label: t("debug.stats.duration"), value: durationText },
    { label: t("debug.stats.tokenUsageLabel"), value: tokenText },
    { label: t("debug.stats.prefillSpeed"), value: prefillSpeedText },
    { label: t("debug.stats.decodeSpeed"), value: decodeSpeedText },
    { label: t("debug.stats.llmRequests"), value: formatStatNumber(debugStats.llmRequests, "0") },
    { label: t("debug.stats.knowledgeRequests"), value: formatStatNumber(debugStats.knowledgeRequests, "0") },
    { label: t("debug.stats.toolCallsLabel"), value: toolText },
    { label: t("debug.stats.sandboxCalls"), value: formatStatNumber(debugStats.sandboxCalls, "0") },
    { label: t("debug.stats.errorCount"), value: formatStatNumber(debugStats.errorCount, "0") },
  ];

  const table = document.createElement("table");
  table.className = "stats-table";
  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  const headLabel = document.createElement("th");
  headLabel.textContent = t("debug.stats.header.metric");
  const headValue = document.createElement("th");
  headValue.textContent = t("debug.stats.header.value");
  headRow.appendChild(headLabel);
  headRow.appendChild(headValue);
  thead.appendChild(headRow);
  table.appendChild(thead);

  const tbody = document.createElement("tbody");
  rows.forEach((row) => {
    const tr = document.createElement("tr");
    const labelCell = document.createElement("td");
    labelCell.className = "stats-label";
    labelCell.textContent = row.label;
    const valueCell = document.createElement("td");
    valueCell.className = "stats-value";
    valueCell.textContent = row.value;
    tr.appendChild(labelCell);
    tr.appendChild(valueCell);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);

  elements.finalAnswer.textContent = "";
  elements.finalAnswer.appendChild(table);
};

const applyTokenUsage = (usage) => {
  if (!usage || typeof usage !== "object") {
    return;
  }
  const inputTokens = Number(usage.input_tokens ?? 0);
  const outputTokens = Number(usage.output_tokens ?? 0);
  const totalTokens = Number(usage.total_tokens ?? 0);
  if (Number.isFinite(inputTokens)) {
    debugStats.tokenInput += inputTokens;
  }
  if (Number.isFinite(outputTokens)) {
    debugStats.tokenOutput += outputTokens;
  }
  if (Number.isFinite(totalTokens)) {
    debugStats.tokenTotal += totalTokens;
  }
  debugStats.hasTokenUsage = true;
};

const applyTokenUsageSnapshot = (usage, options = {}) => {
  if (!usage || typeof usage !== "object") {
    return;
  }
  const inputTokens = Number(usage.input_tokens ?? 0);
  const outputTokens = Number(usage.output_tokens ?? 0);
  const totalTokens = Number(usage.total_tokens ?? 0);
  const hasMeaningful =
    (Number.isFinite(totalTokens) && totalTokens > 0) ||
    (Number.isFinite(inputTokens) && inputTokens > 0) ||
    (Number.isFinite(outputTokens) && outputTokens > 0);
  if (!hasMeaningful) {
    return;
  }
  if (options.override === true || !debugStats.hasTokenUsage) {
    debugStats.tokenInput = Number.isFinite(inputTokens) ? inputTokens : 0;
    debugStats.tokenOutput = Number.isFinite(outputTokens) ? outputTokens : 0;
    debugStats.tokenTotal = Number.isFinite(totalTokens) ? totalTokens : 0;
    debugStats.hasTokenUsage = true;
    return;
  }
  if (Number.isFinite(inputTokens)) {
    debugStats.tokenInput = Math.max(debugStats.tokenInput, inputTokens);
  }
  if (Number.isFinite(outputTokens)) {
    debugStats.tokenOutput = Math.max(debugStats.tokenOutput, outputTokens);
  }
  if (Number.isFinite(totalTokens)) {
    debugStats.tokenTotal = Math.max(debugStats.tokenTotal, totalTokens);
  }
};

// 生成附件唯一标识，便于删除操作定位
const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

// 更新附件提示信息，避免用户忘记当前绑定的文件/图片
const updateAttachmentMeta = () => {
  if (!elements.debugAttachmentMeta) {
    return;
  }
  if (debugAttachmentBusy > 0) {
    elements.debugAttachmentMeta.textContent = t("debug.attachments.processing", {
      count: debugAttachmentBusy,
    });
    return;
  }
  const total = debugAttachments.length;
  elements.debugAttachmentMeta.textContent = total
    ? t("debug.attachments.added", { count: total })
    : t("debug.attachments.none");
};

// 渲染附件列表，提供删除入口与状态提示
const renderAttachmentList = () => {
  if (!elements.debugAttachmentList) {
    return;
  }
  elements.debugAttachmentList.textContent = "";
  if (!debugAttachments.length) {
    elements.debugAttachmentList.textContent = t("debug.attachments.empty");
    updateAttachmentMeta();
    return;
  }
  debugAttachments.forEach((attachment) => {
    const item = document.createElement("div");
    item.className = "debug-attachment-item";

    const icon = document.createElement("i");
    icon.className = `debug-attachment-icon fa-solid ${
      attachment.type === "image" ? "fa-image" : "fa-file-lines"
    }`;

    const info = document.createElement("div");
    info.className = "debug-attachment-info";

    const name = document.createElement("div");
    name.className = "debug-attachment-name";
    name.textContent = attachment.name || t("debug.attachments.unnamed");

    const meta = document.createElement("div");
    meta.className = "debug-attachment-meta";
    if (attachment.type === "image") {
      meta.textContent = t("debug.attachments.type.image");
    } else if (attachment.converter) {
      meta.textContent = t("debug.attachments.type.fileWithConverter", {
        converter: attachment.converter,
      });
    } else {
      meta.textContent = t("debug.attachments.type.file");
    }

    info.appendChild(name);
    info.appendChild(meta);

    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.className = "danger btn-with-icon btn-compact debug-attachment-remove";
    removeBtn.innerHTML = `<i class="fa-solid fa-trash"></i>${t("common.delete")}`;
    removeBtn.addEventListener("click", () => {
      removeDebugAttachment(attachment.id);
    });

    item.appendChild(icon);
    item.appendChild(info);
    item.appendChild(removeBtn);
    elements.debugAttachmentList.appendChild(item);
  });
  updateAttachmentMeta();
};

// 删除指定附件，避免无效内容随请求发送
const removeDebugAttachment = (id) => {
  const index = debugAttachments.findIndex((item) => item.id === id);
  if (index < 0) {
    return;
  }
  debugAttachments.splice(index, 1);
  renderAttachmentList();
};

// 归一化预设问题列表，避免空值与无效内容
const normalizeQuestionPresets = (presets) =>
  (Array.isArray(presets) ? presets : [])
    .map((item) => String(item || "").trim())
    .filter(Boolean);

// 渲染右键预设问题菜单，支持动态配置与空态提示
const renderQuestionPresetMenu = () => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  const menu = elements.debugQuestionMenu;
  const presets = normalizeQuestionPresets(resolveQuestionPresets());
  menu.textContent = "";
  if (!presets.length) {
    const empty = document.createElement("button");
    empty.type = "button";
    empty.disabled = true;
    empty.textContent = t("debug.question.presets.empty");
    menu.appendChild(empty);
    return;
  }
  presets.forEach((preset) => {
    const item = document.createElement("button");
    item.type = "button";
    item.textContent = preset;
    item.addEventListener("click", () => {
      applyQuestionPreset(preset);
    });
    menu.appendChild(item);
  });
};

// 应用预设问题并触发输入同步
const applyQuestionPreset = (preset) => {
  if (!elements.question) {
    return;
  }
  elements.question.value = preset;
  elements.question.dispatchEvent(new Event("input", { bubbles: true }));
  elements.question.focus();
  closeQuestionPresetMenu();
};

// 打开右键菜单，确保不会超出视口
const openQuestionPresetMenu = (event) => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  renderQuestionPresetMenu();
  const menu = elements.debugQuestionMenu;
  menu.style.display = "flex";
  const menuRect = menu.getBoundingClientRect();
  const maxLeft = window.innerWidth - menuRect.width - 8;
  const maxTop = window.innerHeight - menuRect.height - 8;
  const left = Math.min(event.clientX, maxLeft);
  const top = Math.min(event.clientY, maxTop);
  menu.style.left = `${Math.max(8, left)}px`;
  menu.style.top = `${Math.max(8, top)}px`;
};

// 关闭右键菜单
const closeQuestionPresetMenu = () => {
  if (!elements.debugQuestionMenu) {
    return;
  }
  elements.debugQuestionMenu.style.display = "none";
};

// 提取文件扩展名，统一用于图片与文档判断
const resolveFileExtension = (filename) => {
  const parts = String(filename || "").trim().split(".");
  if (parts.length < 2) {
    return "";
  }
  return parts.pop().toLowerCase();
};

// 判断是否为图片文件，优先使用 MIME 类型兜底扩展名
const isImageFile = (file) => {
  if (file?.type && file.type.startsWith("image/")) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? DEBUG_IMAGE_EXTENSIONS.has(ext) : false;
};

// 读取图片为 data URL，便于按多模态格式发送
const readFileAsDataUrl = (file) =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(new Error(t("debug.attachment.imageReadFailed")));
    reader.readAsDataURL(file);
  });

// 构建附件载荷，发送时只透出必要字段
const buildAttachmentPayload = () => {
  return debugAttachments
    .filter((item) => String(item?.content || "").trim())
    .map((item) => {
      const payload = {
        type: item.type,
        name: String(item.name || ""),
        content: item.content,
      };
      if (item.mimeType) {
        payload.mime_type = item.mimeType;
      }
      return payload;
    });
};

// 调用后端转换附件为 Markdown，确保走 doc2md 解析链路
const convertAttachmentFile = async (file) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("debug.apiBaseEmpty"));
  }
  const endpoint = `${wunderBase}/attachments/convert`;
  const formData = new FormData();
  formData.append("file", file, file.name || "upload");
  const response = await fetch(endpoint, {
    method: "POST",
    body: formData,
  });
  if (!response.ok) {
    let detail = "";
    try {
      const payload = await response.json();
      detail = payload?.message || payload?.detail?.message || payload?.detail || "";
    } catch (error) {
      detail = "";
    }
    throw new Error(detail || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

// 处理用户选择的附件，区分图片与文件解析
const handleAttachmentSelection = async (file) => {
  if (!file) {
    return;
  }
  const filename = file.name || "upload";
  debugAttachmentBusy += 1;
  updateAttachmentMeta();
  try {
    if (isImageFile(file)) {
      const dataUrl = await readFileAsDataUrl(file);
      if (!dataUrl) {
        throw new Error(t("debug.attachment.imageEmpty"));
      }
      debugAttachments.push({
        id: buildAttachmentId(),
        type: "image",
        name: filename,
        content: dataUrl,
        mimeType: file.type || "",
      });
      renderAttachmentList();
      notify(t("debug.attachment.imageAdded", { name: filename }), "success");
      return;
    }
    const extension = resolveFileExtension(filename);
    if (!extension || !DEBUG_DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(
        t("debug.attachment.unsupportedType", {
          ext: extension || t("debug.attachment.unknownExt"),
        })
      );
    }
    const result = await convertAttachmentFile(file);
    const content = typeof result?.content === "string" ? result.content : "";
    if (!content.trim()) {
      throw new Error(t("debug.attachment.emptyResult"));
    }
    debugAttachments.push({
      id: buildAttachmentId(),
      type: "file",
      name: result?.name || filename,
      content,
      mimeType: file.type || "",
      converter: result?.converter || "",
    });
    renderAttachmentList();
    const warnings = Array.isArray(result?.warnings) ? result.warnings : [];
    if (warnings.length) {
      notify(t("debug.attachment.convertWarning", { message: warnings[0] }), "warn");
    } else {
      notify(t("debug.attachment.fileParsed", { name: result?.name || filename }), "success");
    }
  } finally {
    debugAttachmentBusy = Math.max(0, debugAttachmentBusy - 1);
    updateAttachmentMeta();
  }
};

// 组装请求体，统一处理输入字段与可选参数
const buildPayload = () => {
  const payload = {
    user_id: elements.userId.value.trim(),
    question: elements.question.value.trim(),
    session_id: elements.sessionId.value.trim() || null,
    stream: true,
    debug_payload: true,
  };
  const modelName = String(elements.debugModelName?.value || "").trim();
  if (modelName) {
    payload.model_name = modelName;
  }
  const toolNames = getSelectedToolNames();
  if (toolNames.length) {
    payload.tool_names = toolNames;
  }
  const attachments = buildAttachmentPayload();
  if (attachments.length) {
    payload.attachments = attachments;
  }
  return payload;
};

// 将 SSE 块解析为事件类型与数据内容
const parseSseBlock = (block) => {
  const lines = block.split(/\r?\n/);
  let eventType = "message";
  const dataLines = [];
  lines.forEach((line) => {
    if (line.startsWith("event:")) {
      eventType = line.slice(6).trim();
    } else if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  });
  return {
    eventType,
    dataText: dataLines.join("\n"),
  };
};

// 读取本地持久化的调试面板状态
const readDebugState = () => {
  try {
    const raw = localStorage.getItem(DEBUG_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") {
      return {};
    }
    return parsed;
  } catch (error) {
    return {};
  }
};

// 写入本地调试面板状态，避免刷新后丢失输入
const writeDebugState = (patch) => {
  const next = { ...readDebugState(), ...patch };
  try {
    localStorage.setItem(DEBUG_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // 忽略浏览器存储异常，避免打断交互
  }
  return next;
};

// 将当前输入同步到本地存储
const syncDebugInputs = () => {
  writeDebugState({
    apiKey: elements.apiKey?.value || "",
    userId: elements.userId?.value || "",
    sessionId: elements.sessionId?.value || "",
    question: elements.question?.value || "",
    modelName: elements.debugModelName?.value || "",
  });
};

// 格式化事件时间，兼容 ISO 字符串/时间戳
const formatEventTime = (value) => {
  if (!value) {
    return new Date().toLocaleTimeString();
  }
  const parsed = new Date(normalizeTimestampText(value));
  if (Number.isNaN(parsed.getTime())) {
    return new Date().toLocaleTimeString();
  }
  return parsed.toLocaleTimeString();
};

// 更新会话 ID 并同步存储，确保刷新后能恢复
const updateSessionId = (sessionId) => {
  const trimmed = String(sessionId || "").trim();
  if (!trimmed) {
    return;
  }
  if (elements.sessionId && elements.sessionId.value !== trimmed) {
    elements.sessionId.value = trimmed;
  }
  if (state.runtime.debugSessionId !== trimmed) {
    state.runtime.debugSessionId = trimmed;
    state.runtime.debugEventCursor = 0;
    state.runtime.debugRestored = false;
  }
  writeDebugState({ sessionId: trimmed });
};

// 重置模型输出的流式状态，避免新旧请求串联
const resetModelOutputState = (options = {}) => {
  const resetRound = options.resetRound !== false;
  const resetContent = options.resetContent === true;
  if (!state.runtime.llmOutput) {
    state.runtime.llmOutput = {
      globalRound: 0,
      currentRound: null,
      rounds: [],
      selectedRound: null,
      userSelectedRound: false,
    };
  }
  const outputState = state.runtime.llmOutput;
  if (resetRound) {
    outputState.globalRound = 0;
    outputState.currentRound = null;
  }
  if (resetContent) {
    outputState.rounds = [];
    outputState.selectedRound = null;
    outputState.userSelectedRound = false;
    resetModelOutputBuffer();
    const outputText = resolveModelOutputText();
    if (outputText) {
      outputText.textContent = "";
    }
    // 清空 A2UI 渲染状态，避免旧 UI 残留。
    resetA2uiState(elements.modelOutputA2ui);
    renderRoundSelectOptions(outputState);
    updateModelOutputPreviewButton(outputState);
    resetPlanBoardState();
  }
};

// 重置指定轮次的输出内容，避免流式重连时重复拼接
const resetRoundOutput = (roundId) => {
  const outputState = getModelOutputState();
  const targetRound = Number.isFinite(roundId) ? roundId : outputState.currentRound;
  if (!Number.isFinite(targetRound)) {
    return;
  }
  const entry = findRoundEntry(outputState, targetRound);
  if (!entry) {
    return;
  }
  entry.chunks = [];
  entry.totalChars = 0;
  entry.contentChars = 0;
  entry.tail = "";
  entry.lastChar = "";
  entry.section = null;
  entry.headerWritten = false;
  entry.streaming = false;
  entry.reasoningStreaming = false;
  entry.a2uiMessages = null;
  entry.a2uiUid = "";
  entry.a2uiContent = "";
  entry.contentChunks = [];
  if (outputState.selectedRound === entry.id) {
    resetModelOutputBuffer();
    const outputText = resolveModelOutputText();
    if (outputText) {
      outputText.textContent = "";
    }
  }
  renderRoundSelectOptions(outputState);
  updateModelOutputPreviewButton(outputState);
  refreshModelOutputPreview();
};

const getModelOutputState = () => {
  if (!state.runtime.llmOutput) {
    state.runtime.llmOutput = {
      globalRound: 0,
      currentRound: null,
      rounds: [],
      selectedRound: null,
      userSelectedRound: false,
    };
  }
  if (!Number.isFinite(state.runtime.llmOutput.globalRound)) {
    state.runtime.llmOutput.globalRound = 0;
  }
  if (!Array.isArray(state.runtime.llmOutput.rounds)) {
    state.runtime.llmOutput.rounds = [];
  }
  if (!Number.isFinite(state.runtime.llmOutput.currentRound)) {
    state.runtime.llmOutput.currentRound = null;
  }
  if (!Number.isFinite(state.runtime.llmOutput.selectedRound)) {
    state.runtime.llmOutput.selectedRound = null;
  }
  if (typeof state.runtime.llmOutput.userSelectedRound !== "boolean") {
    state.runtime.llmOutput.userSelectedRound = false;
  }
  return state.runtime.llmOutput;
};

// 统一推进模型轮次，并准备对应的轮次容器
const advanceModelRound = (timestamp, options = {}) => {
  const autoSelect = options.autoSelect !== false;
  const outputState = getModelOutputState();
  outputState.globalRound = (Number.isFinite(outputState.globalRound) ? outputState.globalRound : 0) + 1;
  outputState.currentRound = outputState.globalRound;
  ensureRoundEntry(outputState, outputState.currentRound, timestamp, { autoSelect });
  return outputState.currentRound;
};

// 重置模型输出缓冲，避免清空后仍写入旧数据
const resetModelOutputBuffer = () => {
  if (modelOutputBuffer.rafId) {
    cancelAnimationFrame(modelOutputBuffer.rafId);
  }
  modelOutputBuffer.rafId = 0;
  modelOutputBuffer.chunks = [];
  modelOutputBuffer.scheduled = false;
  modelOutputBuffer.pendingScroll = false;
};

// 合并缓冲并刷新到 DOM，集中处理滚动
const flushModelOutput = () => {
  const outputText = resolveModelOutputText();
  if (!outputText) {
    return;
  }
  if (modelOutputBuffer.chunks.length) {
    const text = modelOutputBuffer.chunks.join("");
    modelOutputBuffer.chunks = [];
    const lastNode = outputText.lastChild;
    if (lastNode && lastNode.nodeType === Node.TEXT_NODE) {
      lastNode.appendData(text);
    } else {
      outputText.appendChild(document.createTextNode(text));
    }
  }
  if (modelOutputBuffer.pendingScroll) {
    const scrollContainer = resolveModelOutputScrollContainer();
    if (scrollContainer) {
      scrollContainer.scrollTop = scrollContainer.scrollHeight;
    }
    modelOutputBuffer.pendingScroll = false;
  }
};

// 计划在下一帧刷新输出，避免每个 token 都触发 DOM 更新
const scheduleModelOutputFlush = () => {
  if (modelOutputBuffer.scheduled) {
    return;
  }
  modelOutputBuffer.scheduled = true;
  modelOutputBuffer.rafId = requestAnimationFrame(() => {
    modelOutputBuffer.scheduled = false;
    modelOutputBuffer.rafId = 0;
    flushModelOutput();
  });
};

// 仅触发滚动到底部，不追加新内容
const scheduleModelOutputScroll = () => {
  modelOutputBuffer.pendingScroll = true;
  scheduleModelOutputFlush();
};

// 将输出追加到当前可见的模型输出区
const appendModelOutputChunk = (text, options = {}) => {
  if (!text) {
    return;
  }
  const textValue = String(text);
  modelOutputBuffer.chunks.push(textValue);
  if (options.scroll !== false) {
    modelOutputBuffer.pendingScroll = true;
  }
  scheduleModelOutputFlush();
};

// 记录轮次输出尾部字符，避免频繁读取完整字符串
const updateRoundTail = (entry, text) => {
  if (!text) {
    return;
  }
  const textValue = String(text);
  if (textValue.length >= 2) {
    entry.tail = textValue.slice(-2);
    entry.lastChar = textValue.slice(-1);
    return;
  }
  const tailSource = `${entry.tail || ""}${textValue}`;
  entry.tail = tailSource.slice(-2);
  entry.lastChar = tailSource.slice(-1);
};

// 组装下拉框展示文案
const buildRoundLabel = (entry) => {
  if (!entry) {
    return "";
  }
  return entry.timeText
    ? t("debug.round.labelWithTime", { id: entry.id, time: entry.timeText })
    : t("debug.round.label", { id: entry.id });
};

// 获取轮次输出文本，切换轮次时用于重建可视区域
const buildRoundText = (entry) => {
  if (!entry || !Array.isArray(entry.chunks)) {
    return "";
  }
  return entry.chunks.join("");
};

// 同步轮次下拉框选项，保持 UI 与运行时一致
const renderRoundSelectOptions = (outputState) => {
  if (!elements.modelOutputRoundSelect) {
    return;
  }
  const select = elements.modelOutputRoundSelect;
  const rounds = Array.isArray(outputState.rounds) ? outputState.rounds : [];
  select.textContent = "";
  if (!rounds.length) {
    const emptyOption = document.createElement("option");
    emptyOption.value = "";
    emptyOption.textContent = t("debug.round.empty");
    emptyOption.disabled = true;
    emptyOption.selected = true;
    select.appendChild(emptyOption);
    select.disabled = true;
    return;
  }
  select.disabled = false;
  const hasSelected = rounds.some((entry) => entry.id === outputState.selectedRound);
  if (!hasSelected) {
    outputState.selectedRound = rounds[rounds.length - 1].id;
    outputState.userSelectedRound = false;
  }
  rounds.forEach((entry) => {
    const option = document.createElement("option");
    option.value = String(entry.id);
    option.textContent = buildRoundLabel(entry);
    if (entry.id === outputState.selectedRound) {
      option.selected = true;
    }
    select.appendChild(option);
  });
};

// 查找已有轮次记录
const findRoundEntry = (outputState, roundId) => {
  if (!Number.isFinite(roundId)) {
    return null;
  }
  const rounds = Array.isArray(outputState.rounds) ? outputState.rounds : [];
  return rounds.find((entry) => entry.id === roundId) || null;
};

// 创建新的轮次输出容器
const buildRoundEntry = (roundId, timestamp) => ({
  id: roundId,
  timeText: timestamp ? formatEventTime(timestamp) : "",
  chunks: [],
  contentChunks: [],
  totalChars: 0,
  contentChars: 0,
  section: null,
  streaming: false,
  reasoningStreaming: false,
  tail: "",
  lastChar: "",
  headerWritten: false,
  a2uiUid: "",
  a2uiMessages: null,
  a2uiContent: "",
});

// 归一化 A2UI 消息，保证渲染时能直接回放
const normalizeA2uiMessages = (payload) => {
  if (!payload) {
    return [];
  }
  if (Array.isArray(payload.messages)) {
    return payload.messages;
  }
  if (Array.isArray(payload.a2ui)) {
    return payload.a2ui;
  }
  if (Array.isArray(payload)) {
    return payload;
  }
  if (typeof payload === "string") {
    try {
      const parsed = JSON.parse(payload);
      if (Array.isArray(parsed)) {
        return parsed;
      }
      if (parsed && typeof parsed === "object") {
        return [parsed];
      }
    } catch (error) {
      return [];
    }
  }
  if (typeof payload === "object") {
    return [payload];
  }
  return [];
};

// 获取预览用文本，优先展示纯输出内容，避免混入调试标记
const resolvePreviewEntryText = (entry) => {
  if (!entry) {
    return "";
  }
  if (typeof entry.a2uiContent === "string" && entry.a2uiContent.trim()) {
    return entry.a2uiContent;
  }
  if (Array.isArray(entry.contentChunks) && entry.contentChunks.length) {
    return entry.contentChunks.join("");
  }
  return buildRoundText(entry);
};

const hasPreviewText = (entry) => Boolean(resolvePreviewEntryText(entry).trim());

const hasPreviewA2ui = (entry) =>
  Array.isArray(entry?.a2uiMessages) && entry.a2uiMessages.length > 0;

// 同步预览按钮可用状态：只要文本或 A2UI 存在即可预览
const updateModelOutputPreviewButton = (outputState) => {
  if (!elements.modelOutputPreviewBtn) {
    return;
  }
  const entry = findRoundEntry(outputState, outputState.selectedRound);
  const enabled = hasPreviewText(entry) || hasPreviewA2ui(entry);
  elements.modelOutputPreviewBtn.disabled = !enabled;
  elements.modelOutputPreviewBtn.setAttribute("aria-label", t("debug.output.preview"));
  elements.modelOutputPreviewBtn.setAttribute("title", t("debug.output.preview"));
  const icon = elements.modelOutputPreviewBtn.querySelector("i");
  if (icon) {
    icon.className = "fa-solid fa-eye";
  }
};

const isModelOutputPreviewOpen = () =>
  Boolean(elements.modelOutputPreviewModal?.classList.contains("active"));

// 初始化 markdown 渲染器，确保预览支持换行
const ensureMarkedReady = () => {
  if (outputPreviewState.markedReady) {
    return;
  }
  const renderer = globalThis.marked;
  if (renderer && typeof renderer.setOptions === "function") {
    renderer.setOptions({ breaks: true, gfm: true });
  }
  outputPreviewState.markedReady = true;
};

// 渲染文本预览，默认按 Markdown 处理
const renderPreviewText = (entry) => {
  if (!elements.modelOutputPreviewText) {
    return;
  }
  const text = resolvePreviewEntryText(entry);
  const trimmed = text.trim();
  const container = elements.modelOutputPreviewText;
  container.classList.toggle("is-empty", !trimmed);
  if (!trimmed) {
    container.textContent = t("debug.output.previewEmpty");
    return;
  }
  const renderer = globalThis.marked;
  if (renderer && typeof renderer.parse === "function") {
    ensureMarkedReady();
    try {
      container.innerHTML = renderer.parse(text);
    } catch (error) {
      container.textContent = text;
    }
  } else {
    container.textContent = text;
  }
};

// 渲染 A2UI 预览
const renderPreviewA2ui = (entry) => {
  if (!elements.modelOutputPreviewA2ui) {
    return;
  }
  resetA2uiState(elements.modelOutputPreviewA2ui);
  const messages = Array.isArray(entry?.a2uiMessages) ? entry.a2uiMessages : [];
  if (!messages.length) {
    const empty = document.createElement("div");
    empty.className = "a2ui-empty";
    empty.textContent = t("debug.a2ui.empty");
    elements.modelOutputPreviewA2ui.appendChild(empty);
    return;
  }
  applyA2uiMessages(elements.modelOutputPreviewA2ui, {
    uid: entry?.a2uiUid || "",
    messages,
  });
};

// 自动选择预览模式：优先展示 A2UI，其次为文本渲染
const resolvePreviewMode = (entry) => (hasPreviewA2ui(entry) ? "a2ui" : "text");

// 切换预览模式，仅展示对应的渲染结果
const applyModelOutputPreviewMode = (mode) => {
  const showText = mode !== "a2ui";
  elements.modelOutputPreviewText?.classList.toggle("active", showText);
  elements.modelOutputPreviewA2ui?.classList.toggle("active", !showText);
};

// 刷新预览内容，确保切换轮次后同步更新
const refreshModelOutputPreview = () => {
  if (!isModelOutputPreviewOpen()) {
    return;
  }
  const outputState = getModelOutputState();
  const entry = findRoundEntry(outputState, outputState.selectedRound);
  const mode = resolvePreviewMode(entry);
  if (mode === "a2ui") {
    renderPreviewA2ui(entry);
  } else {
    renderPreviewText(entry);
  }
  applyModelOutputPreviewMode(mode);
};

// 打开模型输出预览弹窗
const openModelOutputPreview = () => {
  if (!elements.modelOutputPreviewModal) {
    return;
  }
  const outputState = getModelOutputState();
  const entry = findRoundEntry(outputState, outputState.selectedRound);
  const mode = resolvePreviewMode(entry);
  if (mode === "a2ui") {
    renderPreviewA2ui(entry);
  } else {
    renderPreviewText(entry);
  }
  applyModelOutputPreviewMode(mode);
  elements.modelOutputPreviewModal.classList.add("active");
  elements.modelOutputPreviewBtn?.classList.add("is-active");
};

// 关闭模型输出预览弹窗
const closeModelOutputPreview = () => {
  elements.modelOutputPreviewModal?.classList.remove("active");
  elements.modelOutputPreviewBtn?.classList.remove("is-active");
};

const normalizePlanStatus = (value) => {
  const raw = String(value || "").trim().toLowerCase();
  if (!raw) {
    return "pending";
  }
  const normalized = raw.replace(/[-\s]+/g, "_");
  if (normalized === "pending") {
    return "pending";
  }
  if (normalized === "in_progress" || normalized === "inprogress") {
    return "in_progress";
  }
  if (normalized === "completed" || normalized === "complete" || normalized === "done") {
    return "completed";
  }
  return "pending";
};

const normalizePlanPayload = (payload) => {
  if (!payload) {
    return null;
  }
  const rawPlan = Array.isArray(payload?.plan)
    ? payload.plan
    : Array.isArray(payload?.steps)
      ? payload.steps
      : Array.isArray(payload)
        ? payload
        : [];
  if (!rawPlan.length) {
    return null;
  }
  const explanation = typeof payload?.explanation === "string" ? payload.explanation.trim() : "";
  const steps = [];
  let hasInProgress = false;
  rawPlan.forEach((item) => {
    if (!item) {
      return;
    }
    const step = String(item?.step ?? item?.title ?? item).trim();
    if (!step) {
      return;
    }
    let status = normalizePlanStatus(item?.status);
    if (status === "in_progress") {
      if (hasInProgress) {
        status = "pending";
      } else {
        hasInProgress = true;
      }
    }
    steps.push({ step, status });
  });
  if (!steps.length) {
    return null;
  }
  return { explanation, steps };
};

const resolvePlanStatusLabel = (status) => {
  if (status === "in_progress") {
    return t("debug.plan.status.in_progress");
  }
  if (status === "completed") {
    return t("debug.plan.status.completed");
  }
  return t("debug.plan.status.pending");
};

const getPlanBoardState = () => {
  if (!state.runtime.planBoard) {
    state.runtime.planBoard = {
      explanation: "",
      steps: [],
      updatedAt: null,
    };
  }
  if (!Array.isArray(state.runtime.planBoard.steps)) {
    state.runtime.planBoard.steps = [];
  }
  if (typeof state.runtime.planBoard.explanation !== "string") {
    state.runtime.planBoard.explanation = "";
  }
  return state.runtime.planBoard;
};

const hasPlanBoardSteps = (planState) =>
  Array.isArray(planState?.steps) && planState.steps.length > 0;

const updatePlanBoardButton = () => {
  if (!elements.modelOutputPlanBtn) {
    return;
  }
  elements.modelOutputPlanBtn.disabled = false;
  elements.modelOutputPlanBtn.setAttribute("aria-label", t("debug.output.plan"));
  elements.modelOutputPlanBtn.setAttribute("title", t("debug.output.plan"));
  const icon = elements.modelOutputPlanBtn.querySelector("i");
  if (icon) {
    icon.className = "fa-solid fa-table";
  }
};

const renderPlanBoard = () => {
  const planState = getPlanBoardState();
  const explanation = String(planState.explanation || "").trim();
  if (elements.planBoardExplanation) {
    elements.planBoardExplanation.textContent = explanation;
    elements.planBoardExplanation.style.display = explanation ? "" : "none";
  }
  if (elements.planBoardList) {
    elements.planBoardList.textContent = "";
    planState.steps.forEach((item, index) => {
      const row = document.createElement("div");
      row.className = `plan-board-item plan-board-item--${item.status}`;
      const indexNode = document.createElement("span");
      indexNode.className = "plan-board-index";
      indexNode.textContent = String(index + 1);
      const textNode = document.createElement("div");
      textNode.className = "plan-board-text";
      textNode.textContent = item.step;
      const statusNode = document.createElement("span");
      statusNode.className = "plan-board-status";
      statusNode.textContent = resolvePlanStatusLabel(item.status);
      row.appendChild(indexNode);
      row.appendChild(textNode);
      row.appendChild(statusNode);
      elements.planBoardList.appendChild(row);
    });
  }
  if (elements.planBoardEmpty) {
    elements.planBoardEmpty.style.display = hasPlanBoardSteps(planState) ? "none" : "block";
  }
};

const isPlanBoardOpen = () =>
  Boolean(elements.planBoardModal?.classList.contains("active"));

const openPlanBoard = () => {
  if (!elements.planBoardModal) {
    return;
  }
  renderPlanBoard();
  elements.planBoardModal.classList.add("active");
  elements.modelOutputPlanBtn?.classList.add("is-active");
};

const closePlanBoard = () => {
  elements.planBoardModal?.classList.remove("active");
  elements.modelOutputPlanBtn?.classList.remove("is-active");
};

const resetPlanBoardState = () => {
  const planState = getPlanBoardState();
  planState.explanation = "";
  planState.steps = [];
  planState.updatedAt = null;
  renderPlanBoard();
  updatePlanBoardButton();
  if (isPlanBoardOpen()) {
    closePlanBoard();
  }
};

const applyPlanUpdate = (payload) => {
  const normalized = normalizePlanPayload(payload);
  if (!normalized) {
    return null;
  }
  const planState = getPlanBoardState();
  planState.explanation = normalized.explanation;
  planState.steps = normalized.steps;
  planState.updatedAt = Date.now();
  renderPlanBoard();
  updatePlanBoardButton();
  openPlanBoard();
  return normalized;
};

const recordA2uiMessages = (payload, timestamp) => {
  const outputState = getModelOutputState();
  const messages = normalizeA2uiMessages(payload);
  const content = typeof payload?.content === "string" ? payload.content : "";
  if (!messages.length && !content) {
    updateModelOutputPreviewButton(outputState);
    return null;
  }
  let roundId = resolveOutputRoundId(outputState, payload?.round);
  let entry = null;
  if (!Number.isFinite(roundId)) {
    roundId = advanceModelRound(timestamp);
    entry = findRoundEntry(outputState, roundId);
  } else {
    entry = ensureRoundEntry(outputState, roundId, timestamp, { autoSelect: false });
  }
  if (!entry) {
    return null;
  }
  if (!Array.isArray(entry.a2uiMessages)) {
    entry.a2uiMessages = [];
  }
  entry.a2uiMessages.push(...messages);
  const uid = typeof payload?.uid === "string" ? payload.uid : "";
  if (uid) {
    entry.a2uiUid = uid;
  }
  if (content) {
    entry.a2uiContent = content;
  }
  updateModelOutputPreviewButton(outputState);
  refreshModelOutputPreview();
  return entry;
};

// 判断是否自动切换到新轮次
const shouldAutoSelectRound = (outputState, roundId) => {
  if (!outputState.userSelectedRound) {
    return true;
  }
  return outputState.selectedRound === roundId;
};

// 渲染指定轮次的输出内容
const renderSelectedRound = (outputState, entry, options = {}) => {
  const outputText = resolveModelOutputText();
  if (!outputText) {
    return;
  }
  resetModelOutputBuffer();
  outputText.textContent = entry ? buildRoundText(entry) : "";
  const scrollContainer = resolveModelOutputScrollContainer();
  const scrollTo = options.scrollTo || (entry && entry.id === outputState.currentRound ? "bottom" : "top");
  if (scrollTo === "bottom") {
    scheduleModelOutputScroll();
  } else if (scrollContainer) {
    scrollContainer.scrollTop = 0;
  }
};

// 切换当前选中的轮次，更新下拉框与输出区域
const selectRound = (outputState, roundId, options = {}) => {
  const entry = findRoundEntry(outputState, roundId);
  outputState.selectedRound = entry ? entry.id : null;
  if (options.manual) {
    outputState.userSelectedRound = outputState.selectedRound !== outputState.currentRound;
  } else if (options.auto) {
    outputState.userSelectedRound = false;
  }
  renderRoundSelectOptions(outputState);
  const finalEntry = findRoundEntry(outputState, outputState.selectedRound);
  renderSelectedRound(outputState, finalEntry, { scrollTo: options.scrollTo });
  updateModelOutputPreviewButton(outputState);
  refreshModelOutputPreview();
};

// 确保轮次存在，并在需要时自动切换
const ensureRoundEntry = (outputState, roundId, timestamp, options = {}) => {
  if (!Number.isFinite(roundId)) {
    return null;
  }
  let entry = findRoundEntry(outputState, roundId);
  let needsRender = false;
  if (!entry) {
    entry = buildRoundEntry(roundId, timestamp);
    outputState.rounds.push(entry);
    needsRender = true;
  }
  if (timestamp && !entry.timeText) {
    entry.timeText = formatEventTime(timestamp);
    needsRender = true;
  }
  if (!Number.isFinite(entry.contentChars)) {
    entry.contentChars = 0;
  }
  if (!Array.isArray(entry.contentChunks)) {
    entry.contentChunks = [];
  }
  if (typeof entry.a2uiContent !== "string") {
    entry.a2uiContent = "";
  }
  if (needsRender) {
    renderRoundSelectOptions(outputState);
  }
  if (options.autoSelect && shouldAutoSelectRound(outputState, roundId)) {
    selectRound(outputState, roundId, { auto: true });
  }
  return entry;
};

// 将轮次抬头补齐到输出中，保证每轮有独立起始标记
const ensureRoundHeader = (outputState, entry, timestamp) => {
  if (!entry || entry.headerWritten) {
    if (entry && timestamp && !entry.timeText) {
      entry.timeText = formatEventTime(timestamp);
      renderRoundSelectOptions(outputState);
    }
    return;
  }
  if (timestamp && !entry.timeText) {
    entry.timeText = formatEventTime(timestamp);
    renderRoundSelectOptions(outputState);
  }
  const timeText = entry.timeText ? `[${entry.timeText}] ` : "";
  appendRoundText(
    outputState,
    entry,
    t("debug.round.title", { time: timeText, id: entry.id }) + "\n"
  );
  entry.headerWritten = true;
  entry.section = null;
};

// 确保思考/输出分区标题存在，避免混杂显示
const ensureRoundSection = (outputState, entry, label) => {
  if (!entry || entry.section === label) {
    return;
  }
  if (entry.totalChars > 0 && entry.lastChar !== "\n") {
    appendRoundText(outputState, entry, "\n");
  }
  appendRoundText(outputState, entry, `[${label}]\n`);
  entry.section = label;
};

// 追加轮次输出，同时在当前选中轮次时刷新 DOM
const appendRoundText = (outputState, entry, text, options = {}) => {
  if (!entry || !text) {
    return;
  }
  const textValue = String(text);
  entry.chunks.push(textValue);
  entry.totalChars += textValue.length;
  if (options.countContent) {
    if (!Number.isFinite(entry.contentChars)) {
      entry.contentChars = 0;
    }
    entry.contentChars += textValue.length;
    if (!Array.isArray(entry.contentChunks)) {
      entry.contentChunks = [];
    }
    entry.contentChunks.push(textValue);
  }
  updateRoundTail(entry, textValue);
  if (entry.id === outputState.selectedRound) {
    appendModelOutputChunk(textValue, { scroll: options.scroll !== false });
    updateModelOutputPreviewButton(outputState);
  }
};

// 解析事件携带的轮次编号，确保能与当前轮次保持同步
const resolveOutputRoundId = (outputState, dataRound) => {
  const fallbackRound = Number.isFinite(dataRound) ? dataRound : null;
  if (Number.isFinite(outputState.currentRound)) {
    return outputState.currentRound;
  }
  if (Number.isFinite(fallbackRound)) {
    outputState.currentRound = fallbackRound;
    if (!Number.isFinite(outputState.globalRound) || outputState.globalRound < fallbackRound) {
      outputState.globalRound = fallbackRound;
    }
    return fallbackRound;
  }
  return null;
};

// 下拉框切换轮次时只展示选中内容
const handleModelOutputRoundChange = () => {
  if (!elements.modelOutputRoundSelect) {
    return;
  }
  const outputState = getModelOutputState();
  const value = String(elements.modelOutputRoundSelect.value || "").trim();
  const roundId = value ? Number(value) : Number.NaN;
  if (!Number.isFinite(roundId)) {
    selectRound(outputState, null, { manual: true, scrollTo: "top" });
    return;
  }
  selectRound(outputState, roundId, { manual: true, scrollTo: "top" });
};

// 将模型增量输出追加到调试面板，保持流式阅读体验
const appendModelOutputDelta = (data, timestamp) => {
  const delta = typeof data?.delta === "string" ? data.delta : "";
  const reasoningDelta = typeof data?.reasoning_delta === "string" ? data.reasoning_delta : "";
  if (!delta && !reasoningDelta) {
    return;
  }
  const outputState = getModelOutputState();
  const displayRound = resolveOutputRoundId(outputState, data?.round);
  if (!Number.isFinite(displayRound)) {
    return;
  }
  const entry = ensureRoundEntry(outputState, displayRound, timestamp, { autoSelect: true });
  ensureRoundHeader(outputState, entry, timestamp);
  if (reasoningDelta) {
    ensureRoundSection(outputState, entry, t("debug.output.thoughtSection"));
    appendRoundText(outputState, entry, reasoningDelta);
    entry.reasoningStreaming = true;
  }
  if (delta) {
    ensureRoundSection(outputState, entry, t("debug.output.answerSection"));
    appendRoundText(outputState, entry, delta, { countContent: true });
    entry.streaming = true;
  }
};

// 根据工具名称判断所属类别，便于与系统提示词高亮颜色保持一致
const resolveToolCategory = (toolName) => {
  const name = String(toolName || "").trim();
  if (!name) {
    return "default";
  }
  if (state.toolSelection?.builtin?.some((item) => item.name === name)) {
    return "builtin";
  }
  if (state.toolSelection?.knowledge?.some((item) => item.name === name)) {
    return "knowledge";
  }
  if (state.toolSelection?.userTools?.some((item) => item.name === name)) {
    return "user";
  }
  if (state.toolSelection?.sharedTools?.some((item) => item.name === name)) {
    return "shared";
  }
  if (state.toolSelection?.skills?.some((item) => item.name === name)) {
    return "skill";
  }
  if (state.toolSelection?.mcp?.some((item) => item.name === name)) {
    return "mcp";
  }
  if (name.includes("@")) {
    return "mcp";
  }
  return "default";
};

// 统一处理 SSE 事件，按类型更新界面
const handleEvent = (eventType, dataText, options = {}) => {
  if (!dataText) {
    return;
  }
  if (!debugStats) {
    resetDebugStats();
  }
  let payload = null;
  try {
    payload = JSON.parse(dataText);
  } catch (error) {
    appendLog(t("debug.event.parseFailed", { message: dataText }));
    return;
  }
  const eventTimestamp = options.timestamp || payload.timestamp;
  const sessionId = typeof payload?.session_id === "string" ? payload.session_id : "";
  if (sessionId) {
    updateSessionId(sessionId);
  }
  debugStats.eventCount += 1;
  applyEventTimestamp(eventTimestamp || Date.now());
  updateLlmRoundMetrics(eventType, payload, eventTimestamp || Date.now());

  if (eventType === "final") {
    state.runtime.debugSawFinal = true;
    const usage = payload.data?.usage;
    const rawStopReason = payload.data?.stop_reason;
    const stopReason =
      String(rawStopReason || "").trim() || stopReasonHint || "model_response";
    const stopReasonLabel = resolveStopReasonLabel(stopReason);
    // 最终事件里包含的 usage 也要写入事件日志，避免漏看整体用量
    applyTokenUsageSnapshot(usage, { override: true });
    renderDebugStats();
    const summary = t("debug.event.final");
    const detailPayload = {
      stop_reason: stopReason,
      stop_reason_label: stopReasonLabel,
    };
    if (usage && typeof usage === "object") {
      detailPayload.usage = usage;
    }
    const detail = JSON.stringify(detailPayload, null, 2);
    appendLog(summary, { detail, timestamp: eventTimestamp });
    finalizePendingRequestDurations(eventTimestamp);
    resetPendingRequestLogs();
    loadWorkspace({ refreshTree: true });
    resetStopReasonHint();
    return;
  }

  if (eventType === "a2ui") {
    const data = payload.data || payload;
    const messages = normalizeA2uiMessages(data);
    recordA2uiMessages(data, eventTimestamp);
    const messageCount = messages.length;
    stopReasonHint = "a2ui";
    const detail = JSON.stringify(
      {
        uid: data?.uid || "",
        message_count: messageCount,
      },
      null,
      2
    );
    appendLog(t("debug.event.a2ui"), { detail, timestamp: eventTimestamp });
    return;
  }

  if (eventType === "error") {
    state.runtime.debugSawFinal = true;
    debugStats.errorCount += 1;
    renderDebugStats();
    const errorMessage =
      payload?.data?.message || payload?.message || payload?.data?.detail?.error || "";
    flushPendingRequests(errorMessage, { timestamp: eventTimestamp });
    appendLog(t("debug.event.error"), {
      detail: JSON.stringify(payload.data || payload, null, 2),
      timestamp: eventTimestamp,
    });
    loadWorkspace({ refreshTree: true });
    return;
  }

  if (eventType === "progress") {
    const data = payload.data || payload;
    let detailData = data;
    const stage = typeof data?.stage === "string" ? data.stage : "";
    let summary = typeof data?.summary === "string" ? data.summary : "";
    if (stage === "received") {
      summary = t("debug.sse.connected");
    }
    const showStageBadge = stage && !["received", "llm_call", "compacting"].includes(stage);
    if (stage === "llm_call") {
      const roundNumber = advanceModelRound(eventTimestamp, { autoSelect: false });
      summary = t("debug.event.llmCall", { round: roundNumber });
      if (Number.isFinite(data?.round)) {
        if (data.round !== roundNumber) {
          detailData = { ...data, request_round: data.round, round: roundNumber };
        } else {
          detailData = { ...data };
        }
      } else {
        detailData = { ...data, round: roundNumber };
      }
      renderDebugStats();
    }
    appendLog(summary || t("debug.event.progress"), {
      stage: showStageBadge ? stage : "",
      detail: JSON.stringify(detailData, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "compaction") {
    const data = payload.data || payload;
    const reason =
      data?.reason === "history"
        ? t("debug.compaction.reason.history")
        : t("debug.compaction.reason.context");
    const status = typeof data?.status === "string" ? data.status : "";
    const title = status
      ? t("debug.compaction.titleWithStatus", { reason, status })
      : t("debug.compaction.title", { reason });
    const detail = JSON.stringify(data, null, 2);
    appendLog(title, {
      detail,
      timestamp: eventTimestamp,
      showEventBadge: false,
    });
    return;
  }

  if (eventType === "tool_call") {
    const data = payload.data || payload;
    const toolName = typeof data?.tool === "string" ? data.tool : "";
    const title = toolName ? `tool_call - ${toolName}` : "tool_call";
    const category = resolveToolCategory(toolName);
    if (toolName === "最终回复" || toolName === "final_response") {
      stopReasonHint = "final_tool";
    }
    debugStats.toolCalls += 1;
    renderDebugStats();
    appendLog(title, {
      eventType: "tool_call",
      highlight: true,
      highlightClass: category,
      showEventBadge: false,
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "tool_result") {
    const data = payload.data || payload;
    const toolName = typeof data?.tool === "string" ? data.tool : "";
    const sandboxed = data?.sandbox === true;
    const title = toolName ? `tool_result - ${toolName}` : "tool_result";
    if (data?.ok === true) {
      debugStats.toolOk += 1;
    } else if (data?.ok === false) {
      debugStats.toolFailed += 1;
    }
    if (sandboxed) {
      debugStats.sandboxCalls += 1;
    }
    renderDebugStats();
    appendLog(title, {
      eventType: "tool_result",
      showEventBadge: false,
      rightTag: sandboxed ? "sandbox" : "",
      rightTagClass: sandboxed ? "log-tag--sandbox" : "",
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "plan_update") {
    const data = payload.data || payload;
    const normalized = applyPlanUpdate(data);
    if (normalized) {
      appendLog(t("debug.event.planUpdate"), {
        detail: JSON.stringify(data, null, 2),
        timestamp: eventTimestamp,
      });
    }
    return;
  }

  if (eventType === "question_panel") {
    const data = payload.data || payload;
    appendLog(t("debug.event.questionPanel"), {
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "llm_request") {
    const data = payload.data || payload;
    const hasPayload = data && typeof data === "object" && "payload" in data;
    const hasSummary = data && typeof data === "object" && "payload_summary" in data;
    const purpose = typeof data?.purpose === "string" ? data.purpose : "";
    let title = hasSummary && !hasPayload
      ? t("debug.llm.requestSummary")
      : t("debug.llm.requestPayload");
    if (purpose === "compaction_summary") {
      title = t("debug.llm.compactionPayload");
    }
    const detail = JSON.stringify(data, null, 2);
    debugStats.llmRequests += 1;
    renderDebugStats();
    const item = appendRequestLog(title, detail, { eventType: "llm_request", timestamp: eventTimestamp });
    if (item) {
      const requestTimestampMs = resolveTimestampMs(eventTimestamp);
      pendingRequestLogs.push({
        id: ++pendingRequestSeq,
        item,
        purpose,
        responseAttached: false,
        requestTimestampMs: Number.isFinite(requestTimestampMs) ? requestTimestampMs : Date.now(),
      });
    }
    return;
  }

  if (eventType === "llm_response") {
    const data = payload.data || payload;
    attachResponseToRequest(data, { timestamp: eventTimestamp });
    return;
  }

  if (eventType === "knowledge_request") {
    const data = payload.data || payload;
    const detail = JSON.stringify(data, null, 2);
    const title = data?.knowledge_base
      ? t("debug.knowledge.requestWithBase", { base: data.knowledge_base })
      : t("debug.knowledge.request");
    debugStats.knowledgeRequests += 1;
    renderDebugStats();
    appendRequestLog(title, detail, { eventType: "knowledge_request", timestamp: eventTimestamp });
    return;
  }

  if (eventType === "llm_output_delta") {
    const data = payload.data || payload;
    renderDebugStats();
    appendModelOutputDelta(data, eventTimestamp);
    return;
  }

  if (eventType === "llm_stream_retry") {
    const data = payload.data || payload;
    const attempt = Number.isFinite(data?.attempt) ? data.attempt : 0;
    const maxAttempts = Number.isFinite(data?.max_attempts) ? data.max_attempts : 0;
    const delayValue = Number.isFinite(data?.delay_s) ? `${data.delay_s}s` : "";
    const delayNote = delayValue ? t("debug.streamRetry.delay", { delay: delayValue }) : "";
    const willRetry = data?.will_retry !== false;
    let summary = t("debug.streamRetry.pending");
    if (maxAttempts) {
      summary = willRetry
        ? t("debug.streamRetry.retrying", {
            attempt,
            max: maxAttempts,
            delay: delayNote,
          })
        : t("debug.streamRetry.failed", { attempt, max: maxAttempts });
    } else if (!willRetry) {
      summary = t("debug.streamRetry.failedSimple");
    }
    if (data?.reset_output === true) {
      resetRoundOutput(data?.round);
    }
    appendLog(summary, {
      detail: JSON.stringify(data, null, 2),
      timestamp: eventTimestamp,
    });
    return;
  }

  if (eventType === "llm_output") {
    const data = payload.data || payload;
    renderDebugStats();
    attachResponseToRequest(data, { timestamp: eventTimestamp });
    const outputState = getModelOutputState();
    const displayRound = resolveOutputRoundId(outputState, data?.round);
    if (!Number.isFinite(displayRound)) {
      return;
    }
    const entry = ensureRoundEntry(outputState, displayRound, eventTimestamp, { autoSelect: true });
    ensureRoundHeader(outputState, entry, eventTimestamp);
    const content = data?.content ? String(data.content) : "";
    const reasoning = data?.reasoning ? String(data.reasoning) : "";
    const toolCallsText = formatToolCalls(data?.tool_calls);
    const hasContent = Boolean(content);
    const hasReasoning = Boolean(reasoning);
    const hasToolCalls = Boolean(toolCallsText);
    const isContentStreaming = entry.streaming;
    const isReasoningStreaming = entry.reasoningStreaming;

    if (isContentStreaming && (!hasReasoning || isReasoningStreaming) && !hasContent) {
      if (hasToolCalls) {
        ensureRoundSection(outputState, entry, t("debug.output.toolCallSection"));
        appendRoundText(outputState, entry, toolCallsText, { countContent: true });
      }
      // 已通过增量输出渲染过内容时，仅补齐换行并结束该轮流式状态
      if (entry.totalChars > 0 && entry.tail !== "\n\n") {
        appendRoundText(outputState, entry, "\n\n");
      }
      entry.streaming = false;
      entry.reasoningStreaming = false;
      entry.section = null;
      if (entry.id === outputState.selectedRound) {
        scheduleModelOutputScroll();
      }
      return;
    }

    if (hasReasoning && !isReasoningStreaming) {
      ensureRoundSection(outputState, entry, t("debug.output.thoughtSection"));
      appendRoundText(outputState, entry, reasoning);
    }
    if (hasContent && !isContentStreaming) {
      ensureRoundSection(outputState, entry, t("debug.output.answerSection"));
      appendRoundText(outputState, entry, content, { countContent: true });
    }
    if (hasToolCalls && !hasContent) {
      ensureRoundSection(outputState, entry, t("debug.output.toolCallSection"));
      appendRoundText(outputState, entry, toolCallsText, { countContent: true });
    }

    if (entry.totalChars > 0 && entry.tail !== "\n\n") {
      appendRoundText(outputState, entry, "\n\n");
    }
    entry.streaming = false;
    entry.reasoningStreaming = false;
    entry.section = null;
    if (entry.id === outputState.selectedRound) {
      scheduleModelOutputScroll();
    }
    refreshModelOutputPreview();
    return;
  }

  if (eventType === "token_usage") {
    const data = payload.data || payload;
    // 流式 token_usage 仅记录日志，统计信息等待 final usage 再对齐
    if (!state.runtime.debugStreaming) {
      applyTokenUsage(data);
    }
    renderDebugStats();
    const summary = data?.total_tokens ? `token_usage: ${data.total_tokens}` : "token_usage";
    appendLog(summary, { detail: JSON.stringify(data, null, 2), timestamp: eventTimestamp });
    return;
  }

  const data = payload.data || payload;
  const summary = data?.name ? `${eventType}: ${data.name}` : eventType;
  appendLog(summary, { detail: JSON.stringify(data, null, 2), timestamp: eventTimestamp });
};

// 发送流式请求并解析 SSE

// 还原本地保存的调试输入，便于刷新后继续查看
const applyStoredDebugInputs = () => {
  const stored = readDebugState();
  if (stored.apiKey && elements.apiKey) {
    elements.apiKey.value = stored.apiKey;
  }
  if (stored.userId && elements.userId) {
    elements.userId.value = stored.userId;
  }
  if (stored.sessionId && elements.sessionId) {
    elements.sessionId.value = stored.sessionId;
  }
  if (stored.question && elements.question) {
    elements.question.value = stored.question;
  }
  if (stored.modelName && elements.debugModelName) {
    elements.debugModelName.value = stored.modelName;
  }
  if (stored.sessionId) {
    updateSessionId(stored.sessionId);
  }
  return stored;
};

// 获取历史会话使用的 user_id，空值表示不限定用户
const getHistoryUserId = () => String(elements.userId?.value || "").trim();

const resolveHistoryTime = (session) => session?.updated_time || session?.start_time || "";

// 按更新时间倒序排列，便于快速定位最新会话
const sortSessionsByUpdate = (sessions) =>
  [...sessions].sort(
    (a, b) => new Date(resolveHistoryTime(b)).getTime() - new Date(resolveHistoryTime(a)).getTime()
  );

// 拉取调试历史会话列表，支持按用户过滤
const fetchDebugSessions = async () => {
  const wunderBase = getWunderBase();
  const userId = getHistoryUserId();
  const endpoint = userId
    ? `${wunderBase}/admin/users/${encodeURIComponent(userId)}/sessions?active_only=false`
    : `${wunderBase}/admin/monitor?active_only=false`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const result = await response.json();
  return {
    userId,
    sessions: Array.isArray(result.sessions) ? result.sessions : [],
  };
};

// 渲染历史会话列表，支持点击恢复
const renderDebugHistoryList = (sessions, options = {}) => {
  if (!elements.debugHistoryList) {
    return;
  }
  elements.debugHistoryList.textContent = "";
  if (!Array.isArray(sessions) || sessions.length === 0) {
    elements.debugHistoryList.textContent = t("debug.history.empty");
    return;
  }
  const userId = options.userId || "";
  sortSessionsByUpdate(sessions).forEach((session) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    const sessionId = String(session?.session_id || "").trim();
    if (sessionId && sessionId === state.runtime.debugSessionId) {
      item.classList.add("active");
    }

    const title = document.createElement("div");
    title.textContent = session?.question || t("debug.question.noQuestion");

    const metaParts = [];
    metaParts.push(sessionId || "-");
    metaParts.push(session?.user_id || userId || "-");
    metaParts.push(session?.status || "-");
    const timeText = formatTimestamp(resolveHistoryTime(session));
    if (timeText && timeText !== "-") {
      metaParts.push(timeText);
    }
    const meta = document.createElement("small");
    meta.textContent = metaParts.join(" · ");

    item.appendChild(title);
    item.appendChild(meta);
    item.addEventListener("click", async () => {
      if (!sessionId) {
        notify(t("debug.history.missingSessionId"), "warn");
        return;
      }
      if (state.runtime.debugStreaming) {
        notify(t("debug.history.restoreBusy"), "warn");
        return;
      }
      if (elements.userId) {
        elements.userId.value = session?.user_id || elements.userId.value || "";
      }
      if (elements.sessionId) {
        elements.sessionId.value = sessionId;
      }
      if (elements.question) {
        elements.question.value = session?.question || "";
      }
      updateSessionId(sessionId);
      state.runtime.debugEventCursor = 0;
      state.runtime.debugRestored = false;
      syncDebugInputs();
      closeDebugHistoryModal();
      const status = await restoreDebugPanel({ refresh: true, syncInputs: false });
      if (!status) {
        notify(t("debug.history.restoreFailed"), "error");
        return;
      }
      notify(t("debug.history.restoreSuccess"), "success");
    });
    elements.debugHistoryList.appendChild(item);
  });
};

const updateDebugHistoryMeta = (sessions, userId) => {
  if (!elements.debugHistoryMeta) {
    return;
  }
  const count = Array.isArray(sessions) ? sessions.length : 0;
  elements.debugHistoryMeta.textContent = userId
    ? t("debug.history.metaWithUser", { userId, count })
    : t("debug.history.metaAll", { count });
};

const loadDebugHistory = async () => {
  if (!elements.debugHistoryList) {
    return;
  }
  elements.debugHistoryList.textContent = t("common.loading");
  try {
    const { sessions, userId } = await fetchDebugSessions();
    updateDebugHistoryMeta(sessions, userId);
    renderDebugHistoryList(sessions, { userId });
  } catch (error) {
    elements.debugHistoryList.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const openDebugHistoryModal = async () => {
  if (!elements.debugHistoryModal) {
    return;
  }
  elements.debugHistoryModal.classList.add("active");
  await loadDebugHistory();
};

const closeDebugHistoryModal = () => {
  elements.debugHistoryModal?.classList.remove("active");
};

// 读取监控详情并返回事件列表
const fetchMonitorDetail = async (sessionId) => {
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}`;
  const response = await fetch(endpoint);
  if (!response.ok) {
    const error = new Error(t("common.requestFailed", { status: response.status }));
    error.status = response.status;
    throw error;
  }
  return response.json();
};

const syncDebugEventCursor = async (sessionId) => {
  const cleaned = String(sessionId || "").trim();
  if (!cleaned) {
    return;
  }
  try {
    const detail = await fetchMonitorDetail(cleaned);
    const session = detail?.session || {};
    const events = Array.isArray(detail?.events) ? detail.events : [];
    if (session.status) {
      state.runtime.debugSessionStatus = session.status;
    }
    state.runtime.debugEventCursor = events.length;
    state.runtime.debugRestored = true;
  } catch (error) {
    // 静默失败，避免影响当前会话输出
  }
};

const unwrapMonitorEventData = (payload) => {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return payload;
  }
  const hasSessionId = typeof payload.session_id === "string" && payload.session_id.trim();
  const hasTimestamp = typeof payload.timestamp === "string" && payload.timestamp.trim();
  const inner = payload.data;
  if (hasSessionId && hasTimestamp && inner && typeof inner === "object") {
    return inner;
  }
  return payload;
};

// 使用监控事件恢复调试面板日志
const applyMonitorDetail = (detail, options = {}) => {
  const session = detail?.session || {};
  const events = Array.isArray(detail?.events) ? detail.events : [];
  const sessionId = session.session_id || state.runtime.debugSessionId || "";
  if (sessionId) {
    updateSessionId(sessionId);
  }
  if (session.user_id && elements.userId && !elements.userId.value.trim()) {
    elements.userId.value = session.user_id;
  }
  if (session.question && elements.question && !elements.question.value.trim()) {
    elements.question.value = session.question;
  }
  state.runtime.debugSessionStatus = session.status || "";
  updateDebugLogWaiting();
  let appendOnly = options.appendOnly === true;
  if (!Number.isFinite(state.runtime.debugEventCursor) || state.runtime.debugEventCursor <= 0) {
    appendOnly = false;
  }
  if (appendOnly && state.runtime.debugEventCursor > events.length) {
    appendOnly = false;
  }
  if (!appendOnly) {
    clearOutput();
    resetPendingRequestLogs();
    resetModelOutputState({ resetContent: true });
    resetDebugStats();
  }
  const startIndex = appendOnly ? state.runtime.debugEventCursor : 0;
  events.slice(startIndex).forEach((item) => {
    if (!DEBUG_RESTORE_EVENT_TYPES.has(item.type)) {
      return;
    }
    const dataText = JSON.stringify({
      data: unwrapMonitorEventData(item.data),
      session_id: sessionId,
    });
    handleEvent(item.type, dataText, { timestamp: item.timestamp });
  });
  state.runtime.debugEventCursor = events.length;
  state.runtime.debugRestored = true;
  syncDebugInputs();
};

// 刷新调试面板并恢复历史事件
export const restoreDebugPanel = async (options = {}) => {
  const refresh = options.refresh === true;
  const syncInputs = options.syncInputs !== false;
  const stored = syncInputs ? applyStoredDebugInputs() : readDebugState();
  const sessionId = state.runtime.debugSessionId || stored.sessionId || "";
  if (!sessionId || state.runtime.debugStreaming) {
    return null;
  }
  try {
    const detail = await fetchMonitorDetail(sessionId);
    applyMonitorDetail(detail, { appendOnly: refresh && state.runtime.debugRestored });
    return state.runtime.debugSessionStatus;
  } catch (error) {
    if (error?.status == 404) {
      writeDebugState({ sessionId: "" });
      state.runtime.debugSessionId = "";
    }
    appendLog(t("debug.tools.loadFailed", { message: error.message }));
    return null;
  }
};

const stopDebugPolling = () => {
  if (state.runtime.debugPollTimer) {
    clearInterval(state.runtime.debugPollTimer);
    state.runtime.debugPollTimer = null;
  }
};

const startDebugPolling = () => {
  if (state.runtime.debugPollTimer) {
    return;
  }
  state.runtime.debugPollTimer = setInterval(async () => {
    if (state.runtime.debugStreaming) {
      return;
    }
    const status = await restoreDebugPanel({ refresh: true, syncInputs: false });
    if (status && !DEBUG_ACTIVE_STATUSES.has(status)) {
      stopDebugPolling();
    }
  }, APP_CONFIG.monitorPollIntervalMs);
};

// 控制调试面板自动刷新
export const toggleDebugPolling = (enabled) => {
  if (!enabled || state.runtime.debugStreaming) {
    stopDebugPolling();
    return;
  }
  restoreDebugPanel({ refresh: true }).then((status) => {
    if (status && DEBUG_ACTIVE_STATUSES.has(status)) {
      startDebugPolling();
    } else {
      stopDebugPolling();
    }
  });
};

const extractErrorMessage = (payload) => {
  if (!payload) {
    return "";
  }
  const detail = payload.detail;
  if (detail) {
    if (typeof detail === "string") {
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
  return payload.message || payload.error || "";
};

const readErrorMessage = async (response) => {
  if (!response) {
    return "";
  }
  try {
    const text = await response.text();
    if (!text) {
      return "";
    }
    try {
      const payload = JSON.parse(text);
      return extractErrorMessage(payload) || text;
    } catch (error) {
      return text;
    }
  } catch (error) {
    return "";
  }
};

const sendStreamRequest = async (endpoint, payload) => {
  stopDebugPolling();
  state.runtime.debugStreaming = true;
  state.runtime.debugSawFinal = false;
  updateDebugLogWaiting();
  state.runtime.activeController = new AbortController();
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
      signal: state.runtime.activeController.signal,
    });

    if (!response.ok || !response.body) {
      const message = await readErrorMessage(response);
      throw new Error(message || t("common.requestFailed", { status: response.status }));
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder("utf-8");
    let buffer = "";

    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split("\n\n");
      buffer = parts.pop() || "";
      parts.forEach((part) => {
        if (!part.trim()) {
          return;
        }
        const { eventType, dataText } = parseSseBlock(part);
        handleEvent(eventType, dataText);
      });
    }

    appendLog(t("debug.sse.closed"));
    if (!state.runtime.debugSawFinal) {
      finalizePendingRequestDurations(Date.now());
    }
  } finally {
    state.runtime.debugStreaming = false;
    updateDebugLogWaiting();
    state.runtime.activeController = null;
    if (!debugStats) {
      resetDebugStats();
    }
    markRequestEnd();
    renderDebugStats();
    if (state.runtime.debugSyncAfterStream) {
      state.runtime.debugSyncAfterStream = false;
      if (!state.runtime.debugSawFinal) {
        state.runtime.debugEventCursor = 0;
        state.runtime.debugRestored = false;
        await restoreDebugPanel({ refresh: true, syncInputs: false });
      } else {
        await syncDebugEventCursor(state.runtime.debugSessionId);
      }
    }
  }
};

// 发送非流式请求，直接解析 JSON 响应
const sendNonStreamRequest = async (endpoint, payload) => {
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    const message = await readErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }

  const result = await response.json();
  if (result?.session_id) {
    updateSessionId(result.session_id);
  }
  if (!debugStats) {
    resetDebugStats();
  }
  markRequestEnd();
  applyTokenUsageSnapshot(result?.usage, { override: true });
  renderDebugStats();
  if (Array.isArray(result?.a2ui)) {
    recordA2uiMessages(
      {
        uid: result?.uid || "",
        messages: result.a2ui,
      },
      Date.now()
    );
  }
  appendLog(t("debug.nonStream.response", { payload: JSON.stringify(result) }));
};

// 统一入口：根据是否开启 SSE 选择请求方式
const handleSend = async () => {
  if (!elements.question.value.trim()) {
    appendLog(t("debug.question.empty"));
    return;
  }
  if (debugAttachmentBusy > 0) {
    appendLog(t("debug.attachments.busy"));
    notify(t("debug.attachments.busy"), "warn");
    return;
  }

  let payload = null;
  try {
    try {
      await ensureToolSelectionLoaded();
    } catch (error) {
      appendLog(t("debug.tools.loadFailed", { message: error.message }));
      applyPromptToolError(error.message);
    }
    payload = buildPayload();
  } catch (error) {
    appendLog(error.message);
    return;
  }

  const requestedSessionId = String(payload.session_id || "").trim();
  const previousSessionId = String(state.runtime.debugSessionId || "").trim();
  const hasSessionId = Boolean(requestedSessionId);
  if (!hasSessionId) {
    clearOutput();
    resetPendingRequestLogs();
    resetModelOutputState({ resetContent: true });
    resetDebugStats();
    state.runtime.debugEventCursor = 0;
    state.runtime.debugRestored = false;
    state.runtime.debugSessionStatus = "";
    state.runtime.debugSessionId = "";
    writeDebugState({ sessionId: "" });
  } else {
    const sessionChanged = requestedSessionId !== previousSessionId;
    updateSessionId(requestedSessionId);
    if (sessionChanged || !state.runtime.debugRestored) {
      await restoreDebugPanel({ refresh: true, syncInputs: false });
    }
    if (!state.runtime.debugRestored) {
      clearOutput();
      resetPendingRequestLogs();
      resetModelOutputState({ resetContent: true });
      resetDebugStats();
      state.runtime.debugEventCursor = 0;
      state.runtime.debugSessionStatus = "";
    } else {
      resetModelOutputState({ resetRound: false });
    }
  }
  syncDebugInputs();
  if (!debugStats) {
    resetDebugStats();
  }
  state.runtime.debugSawFinal = false;
  markRequestStart();
  renderDebugStats();
  updateDebugLogWaiting(true);

  const wunderBase = getWunderBase();
  const endpoint = wunderBase;

  try {
    if (payload.stream) {
      state.runtime.debugSyncAfterStream = hasSessionId && state.runtime.debugRestored === true;
      await sendStreamRequest(endpoint, payload);
    } else {
      await sendNonStreamRequest(endpoint, payload);
    }
  } catch (error) {
    appendLog(t("debug.request.error", { message: error.message }));
  } finally {
    updateDebugLogWaiting();
  }
};

// 请求后端终止指定会话，确保真正停止智能体线程
const requestCancelSession = async (sessionId) => {
  if (!sessionId) {
    return;
  }
  const wunderBase = getWunderBase();
  const endpoint = `${wunderBase}/admin/monitor/${encodeURIComponent(sessionId)}/cancel`;
  const response = await fetch(endpoint, { method: "POST" });
  if (!response.ok) {
    throw new Error(t("debug.stopFailed", { status: response.status }));
  }
  const result = await response.json();
  appendLog(result.message || t("debug.stopRequested"));
};

// 停止流式请求：前端中断连接并通知后端取消执行
const handleStop = async () => {
  if (state.runtime.activeController) {
    state.runtime.activeController.abort();
    appendLog(t("debug.sse.stopRequested"));
  }
  const sessionId = String(state.runtime.debugSessionId || elements.sessionId?.value || "").trim();
  if (!sessionId) {
    appendLog(t("debug.stopMissingSession"));
    return;
  }
  try {
    await requestCancelSession(sessionId);
  } catch (error) {
    appendLog(t("debug.stopFailedWithMessage", { message: error.message }));
    notify(t("debug.stopFailedWithMessage", { message: error.message }), "error");
  }
};

// 等待流式状态完全结束，避免清空后又被流式回写日志
const waitForStreamStop = async (timeoutMs = 4000) => {
  const start = Date.now();
  while (state.runtime.debugStreaming) {
    if (Date.now() - start >= timeoutMs) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 120));
  }
};

// 重置会话标识与本地缓存，确保下一次请求是全新会话
const resetDebugSessionState = () => {
  if (elements.sessionId) {
    elements.sessionId.value = "";
  }
  state.runtime.debugSessionId = "";
  state.runtime.debugSessionStatus = "";
  state.runtime.debugEventCursor = 0;
  state.runtime.debugRestored = false;
  state.runtime.debugSyncAfterStream = false;
  writeDebugState({ sessionId: "" });
};

// 新会话：清空日志与统计，并清除会话 ID，避免旧上下文残留
const handleNewSession = async () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldStop = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  state.runtime.debugSyncAfterStream = false;
  state.runtime.debugSawFinal = false;
  if (shouldStop) {
    await handleStop();
    await waitForStreamStop();
  }
  stopDebugPolling();
  clearOutput();
  resetPendingRequestLogs();
  resetModelOutputState({ resetContent: true });
  resetDebugStats();
  resetDebugSessionState();
  syncDebugInputs();
  updateDebugLogWaiting(false);
};

const handleSendToggle = async () => {
  const status = String(state.runtime.debugSessionStatus || "").trim();
  const shouldStop = Boolean(state.runtime.debugStreaming) || DEBUG_ACTIVE_STATUSES.has(status);
  if (shouldStop) {
    await handleStop();
    return;
  }
  await handleSend();
};

// 清空输出时同步重置流式渲染状态
const handleClear = () => {
  clearOutput();
  resetPendingRequestLogs();
  resetModelOutputState({ resetContent: true });
  resetDebugStats();
  closeModelOutputPreview();
};

// 初始化调试面板交互
export const initDebugPanel = () => {
  resetDebugStats();
  applyStoredDebugInputs();
  ensureLlmConfigLoaded().catch((error) => {
    appendLog(t("debug.llm.loadFailed", { message: error.message }));
  });

  let syncTimer = null;
  const scheduleSync = () => {
    if (syncTimer) {
      clearTimeout(syncTimer);
    }
    syncTimer = setTimeout(() => {
      syncTimer = null;
      syncDebugInputs();
    }, 200);
  };

  if (elements.apiKey) {
    elements.apiKey.addEventListener("change", syncDebugInputs);
  }
  if (elements.userId) {
    elements.userId.addEventListener("input", scheduleSync);
  }
  if (elements.sessionId) {
    elements.sessionId.addEventListener("input", scheduleSync);
  }
  if (elements.debugModelName) {
    elements.debugModelName.addEventListener("change", syncDebugInputs);
  }
  if (elements.question) {
    elements.question.addEventListener("input", scheduleSync);
  }
  if (elements.question && elements.debugQuestionMenu) {
    elements.question.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      event.stopPropagation();
      openQuestionPresetMenu(event);
    });
  }
  if (elements.debugUploadInput) {
    elements.debugUploadInput.accept = DEBUG_UPLOAD_ACCEPT;
  }
  if (elements.debugUploadBtn && elements.debugUploadInput) {
    elements.debugUploadBtn.addEventListener("click", () => {
      // 重置 input 值，确保重复选择同一文件也能触发 change
      elements.debugUploadInput.value = "";
      elements.debugUploadInput.click();
    });
  }
  if (elements.debugUploadInput) {
    elements.debugUploadInput.addEventListener("change", async () => {
      const files = Array.from(elements.debugUploadInput.files || []);
      if (!files.length) {
        return;
      }
      for (const file of files) {
        try {
          await handleAttachmentSelection(file);
        } catch (error) {
          notify(t("debug.attachments.failed", { message: error.message }), "error");
        }
      }
    });
  }

  if (elements.debugNewSessionBtn) {
    elements.debugNewSessionBtn.addEventListener("click", handleNewSession);
  }
  if (elements.sendBtn) {
    elements.sendBtn.addEventListener("click", handleSendToggle);
  }
  if (elements.clearBtn) {
    elements.clearBtn.addEventListener("click", handleClear);
  }
  if (elements.modelOutputRoundSelect) {
    elements.modelOutputRoundSelect.addEventListener("change", handleModelOutputRoundChange);
  }
  if (elements.modelOutputPreviewBtn) {
    elements.modelOutputPreviewBtn.addEventListener("click", openModelOutputPreview);
  }
  if (elements.modelOutputPlanBtn) {
    elements.modelOutputPlanBtn.addEventListener("click", openPlanBoard);
  }
  if (elements.modelOutputPreviewClose) {
    elements.modelOutputPreviewClose.addEventListener("click", closeModelOutputPreview);
  }
  if (elements.planBoardClose) {
    elements.planBoardClose.addEventListener("click", closePlanBoard);
  }
  if (elements.modelOutputPreviewModal) {
    elements.modelOutputPreviewModal.addEventListener("click", (event) => {
      if (event.target === elements.modelOutputPreviewModal) {
        closeModelOutputPreview();
      }
    });
  }
  if (elements.planBoardModal) {
    elements.planBoardModal.addEventListener("click", (event) => {
      if (event.target === elements.planBoardModal) {
        closePlanBoard();
      }
    });
  }

  if (elements.debugHistoryBtn) {
    elements.debugHistoryBtn.addEventListener("click", openDebugHistoryModal);
  }
  if (elements.debugHistoryClose) {
    elements.debugHistoryClose.addEventListener("click", closeDebugHistoryModal);
  }
  if (elements.debugHistoryCloseBtn) {
    elements.debugHistoryCloseBtn.addEventListener("click", closeDebugHistoryModal);
  }
  if (elements.debugHistoryModal) {
    elements.debugHistoryModal.addEventListener("click", (event) => {
      if (event.target === elements.debugHistoryModal) {
        closeDebugHistoryModal();
      }
    });
  }
  if (elements.debugQuestionMenu) {
    document.addEventListener("click", (event) => {
      if (elements.debugQuestionMenu.contains(event.target)) {
        return;
      }
      closeQuestionPresetMenu();
    });
    document.addEventListener("scroll", closeQuestionPresetMenu, true);
    window.addEventListener("resize", closeQuestionPresetMenu);
    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        closeQuestionPresetMenu();
      }
    });
  }
  renderAttachmentList();
  const outputState = getModelOutputState();
  renderRoundSelectOptions(outputState);
  updateModelOutputPreviewButton(outputState);
  updatePlanBoardButton();
};


