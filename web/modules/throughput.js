import { APP_CONFIG } from "../app.config.js?v=20260110-04";
import { elements } from "./elements.js?v=20260118-04";
import { state } from "./state.js";
import { getWunderBase } from "./api.js";
import { notify } from "./notify.js";
import { escapeHtml, formatDuration, formatTimestamp, formatTokenCount } from "./utils.js?v=20251229-02";
import { openMonitorDetail } from "./monitor.js?v=20260113-01";
import { ensureLlmConfigLoaded } from "./llm.js";
import { t } from "./i18n.js?v=20260118-06";

const THROUGHPUT_STATE_KEY = "wunder_throughput_state";
const MAX_CONCURRENCY = 500;
const DEFAULT_CONFIG = {
  concurrency_list: "1,2,4,8,16,30",
  user_id_prefix: "throughput_user",
  request_timeout_s: 0,
  max_tokens: 256,
  model_name: "",
};
const ACTIVE_RUN_STATUSES = new Set(["running", "stopping"]);
const ACTIVE_SESSION_STATUSES = new Set(["running", "cancelling"]);
const FAILED_SESSION_STATUSES = new Set(["error", "cancelled"]);
const THREAD_FILTERS = ["all", "active", "finished", "failed"];
const MAX_SAMPLES = 500;
const CURVE_METRICS = [
  {
    key: "total_prefill_speed_tps",
    labelKey: "throughput.metric.totalPrefillSpeed",
    color: "#2563eb",
  },
  {
    key: "single_prefill_speed_tps",
    labelKey: "throughput.metric.singlePrefillSpeed",
    color: "#8b5cf6",
  },
  {
    key: "total_decode_speed_tps",
    labelKey: "throughput.metric.totalDecodeSpeed",
    color: "#16a34a",
  },
  {
    key: "single_decode_speed_tps",
    labelKey: "throughput.metric.singleDecodeSpeed",
    color: "#f59e0b",
  },
];

let initialized = false;
let chartCurve = null;
let currentRunId = "";
let currentStatus = "";
let samples = [];
let reportSampleByConcurrency = new Map();
let lastReportFetchAt = 0;
let throughputSessions = [];
let throughputSessionMap = new Map();
let throughputSessionRunId = "";
let throughputSessionStartMs = null;
let throughputSessionPrefix = "";
let throughputThreadFilter = "all";
let currentTotalPrefillSpeed = null;
let currentTotalDecodeSpeed = null;
let currentSinglePrefillSpeed = null;
let currentSingleDecodeSpeed = null;
let pendingModelName = "";
let currentRunSnapshot = null;

const readStoredConfig = () => {
  try {
    const raw = localStorage.getItem(THROUGHPUT_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch (error) {
    return {};
  }
};

const writeStoredConfig = (patch) => {
  const next = { ...readStoredConfig(), ...(patch || {}) };
  try {
    localStorage.setItem(THROUGHPUT_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // ignore storage failure
  }
  return next;
};

const applyStoredConfig = () => {
  const stored = { ...DEFAULT_CONFIG, ...readStoredConfig() };
  const storedConcurrencyList = String(
    stored.concurrency_list ?? stored.concurrencyList ?? DEFAULT_CONFIG.concurrency_list
  ).trim();
  if (elements.throughputConcurrencyList && !elements.throughputConcurrencyList.value) {
    elements.throughputConcurrencyList.value = storedConcurrencyList;
  }
  if (elements.throughputUserPrefix && !elements.throughputUserPrefix.value) {
    elements.throughputUserPrefix.value = String(
      stored.user_id_prefix ?? DEFAULT_CONFIG.user_id_prefix
    );
  }
  if (elements.throughputMaxTokens && !elements.throughputMaxTokens.value) {
    elements.throughputMaxTokens.value = String(
      stored.max_tokens ?? stored.maxTokens ?? DEFAULT_CONFIG.max_tokens
    );
  }
  if (elements.throughputTimeout && !elements.throughputTimeout.value) {
    elements.throughputTimeout.value = String(
      stored.request_timeout_s ?? DEFAULT_CONFIG.request_timeout_s
    );
  }
  pendingModelName = String(stored.model_name ?? stored.modelName ?? "").trim();
};

const getStoredModelName = () => {
  const stored = readStoredConfig();
  return String(stored.model_name ?? stored.modelName ?? "").trim();
};

const getSelectedModelName = () =>
  String(elements.throughputModelSelect?.value || "").trim();

const setModelSelectValue = (value) => {
  if (!elements.throughputModelSelect) {
    return;
  }
  const select = elements.throughputModelSelect;
  const desired = String(value || "").trim();
  if (!desired) {
    select.value = "";
    return;
  }
  const options = Array.from(select.options);
  const exists = options.some((option) => option.value === desired);
  select.value = exists ? desired : "";
};

const getDefaultModelLabel = () => {
  const defaultName = String(state.llm.defaultName || "").trim();
  if (defaultName) {
    return t("llm.defaultWithName", { name: defaultName });
  }
  return t("llm.default");
};

const resolveModelName = (run, fallback) => {
  const raw = run?.model_name ?? run?.modelName ?? "";
  const text = String(raw || "").trim();
  if (text) {
    return text;
  }
  const fallbackText = String(fallback || "").trim();
  if (fallbackText) {
    return fallbackText;
  }
  return getDefaultModelLabel();
};

const updateModelValue = (run) => {
  if (!elements.throughputModelValue) {
    return;
  }
  const effectiveRun = run ?? currentRunSnapshot;
  elements.throughputModelValue.textContent = resolveModelName(
    effectiveRun,
    getSelectedModelName()
  );
};

const renderThroughputModelOptions = (options = {}) => {
  if (!elements.throughputModelSelect) {
    return;
  }
  const select = elements.throughputModelSelect;
  const preferred =
    String(options.value ?? pendingModelName ?? select.value ?? "").trim();
  select.textContent = "";
  const defaultOption = document.createElement("option");
  defaultOption.value = "";
  defaultOption.textContent = getDefaultModelLabel();
  select.appendChild(defaultOption);
  state.llm.order.forEach((name) => {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    select.appendChild(option);
  });
  setModelSelectValue(preferred);
  pendingModelName = "";
};

const syncModelSelectState = (run, options = {}) => {
  if (!elements.throughputModelSelect) {
    return;
  }
  const status = String(run?.status || "");
  const historyView = options.historyView === true || state.runtime.throughputHistoryMode;
  const isActive = ACTIVE_RUN_STATUSES.has(status);
  const shouldLock = historyView || isActive;
  elements.throughputModelSelect.disabled = shouldLock;
  if (shouldLock) {
    setModelSelectValue(run?.model_name ?? run?.modelName);
    return;
  }
  if (!getSelectedModelName()) {
    const stored = getStoredModelName();
    setModelSelectValue(stored || run?.model_name || run?.modelName);
  }
};

const scheduleConfigSave = () => {
  if (!elements.throughputFormStatus) {
    return;
  }
  if (elements.throughputFormStatus.dataset.syncing === "true") {
    return;
  }
  elements.throughputFormStatus.dataset.syncing = "true";
  setTimeout(() => {
    elements.throughputFormStatus.dataset.syncing = "false";
    persistConfig();
    if (!state.runtime.throughputHistoryMode && !ACTIVE_RUN_STATUSES.has(currentStatus)) {
      updateModelValue(null);
    }
  }, 200);
};

const persistConfig = () => {
  const modelName = getSelectedModelName();
  const concurrencyListText = String(elements.throughputConcurrencyList?.value || "").trim();
  writeStoredConfig({
    concurrency_list: concurrencyListText || DEFAULT_CONFIG.concurrency_list,
    user_id_prefix: String(elements.throughputUserPrefix?.value || "").trim(),
    max_tokens: readPositiveInt(elements.throughputMaxTokens, DEFAULT_CONFIG.max_tokens),
    request_timeout_s: readNumber(elements.throughputTimeout, DEFAULT_CONFIG.request_timeout_s),
    model_name: modelName || undefined,
  });
};

const readNumber = (element, fallback) => {
  if (!element) {
    return fallback;
  }
  const parsed = Number(element.value);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const readPositiveInt = (element, fallback) => {
  if (!element) {
    return fallback;
  }
  const parsed = Number(element.value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  const value = Math.floor(parsed);
  return value > 0 ? value : fallback;
};

const parseNonNegativeInt = (element) => {
  if (!element) {
    return null;
  }
  const raw = String(element.value ?? "");
  if (!raw.trim()) {
    return null;
  }
  const parsed = Number(raw);
  if (!Number.isFinite(parsed)) {
    return null;
  }
  const value = Math.floor(parsed);
  return value >= 0 ? value : null;
};

const parseConcurrencyList = (raw) => {
  const text = String(raw ?? "").trim();
  if (!text) {
    return null;
  }
  const parts = text.split(/[,，\s]+/).map((item) => item.trim()).filter(Boolean);
  if (!parts.length) {
    return null;
  }
  const list = [];
  const seen = new Set();
  for (const part of parts) {
    const value = Number(part);
    if (!Number.isFinite(value)) {
      return null;
    }
    const intValue = Math.floor(value);
    if (intValue <= 0 || intValue > MAX_CONCURRENCY) {
      return null;
    }
    if (!seen.has(intValue)) {
      seen.add(intValue);
      list.push(intValue);
    }
  }
  return list.length ? list : null;
};

const deriveConcurrencyList = (maxConcurrency, step) => {
  if (!Number.isFinite(maxConcurrency) || maxConcurrency <= 0) {
    return [];
  }
  const normalizedStep = Number.isFinite(step) && step > 0 ? Math.floor(step) : 0;
  if (!normalizedStep) {
    return [Math.floor(maxConcurrency)];
  }
  const list = [];
  let current = 1;
  while (current < maxConcurrency) {
    list.push(current);
    current += normalizedStep;
  }
  if (list[list.length - 1] !== maxConcurrency) {
    list.push(Math.floor(maxConcurrency));
  }
  return list;
};

const resolveRunConcurrencyList = (run) => {
  const list = Array.isArray(run?.concurrency_list)
    ? run.concurrency_list
    : Array.isArray(run?.concurrencyList)
    ? run.concurrencyList
    : [];
  const normalized = list
    .map((value) => Number(value))
    .filter((value) => Number.isFinite(value) && value > 0);
  if (normalized.length) {
    return normalized;
  }
  const maxConcurrency = Number(run?.max_concurrency ?? run?.maxConcurrency ?? run?.users);
  const step = Number(run?.step);
  return deriveConcurrencyList(maxConcurrency, step);
};

const resolveRunMaxConcurrency = (run, list) => {
  const value = Number(run?.max_concurrency ?? run?.maxConcurrency ?? run?.users);
  if (Number.isFinite(value) && value > 0) {
    return value;
  }
  if (Array.isArray(list) && list.length) {
    return Math.max(...list);
  }
  return null;
};

const formatConcurrencyList = (list, options = {}) => {
  const values = Array.isArray(list)
    ? list.filter((value) => Number.isFinite(value) && value > 0)
    : [];
  if (!values.length) {
    return { text: "-", full: "" };
  }
  const full = values.join(", ");
  const maxItems = options.maxItems ?? 6;
  if (values.length <= maxItems) {
    return { text: full, full };
  }
  const text = `${values.slice(0, maxItems).join(", ")} ... (+${values.length - maxItems})`;
  return { text, full };
};

const formatCount = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return Math.max(0, value).toLocaleString();
};

const formatRate = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return value.toFixed(2);
};

const formatPercent = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return `${value.toFixed(1)}%`;
};

const formatLatency = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return `${Math.max(0, Math.round(value))} ms`;
};

const formatElapsed = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  return formatDuration(value);
};

const formatTokenRate = (value, options = {}) => {
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
  const prefix = options.lowerBound ? ">=" : "";
  return `${prefix}${scaled.toFixed(decimals)}${unit} ${t("monitor.detail.tokenRate.unit")}`;
};

const parseTimestampMs = (value) => {
  if (!value) {
    return null;
  }
  const raw = String(value);
  let parsed = Date.parse(raw);
  if (!Number.isFinite(parsed)) {
    const trimmed = raw.replace(/(\.\d{3})\d+(Z|[+-]\d{2}:\d{2})$/, "$1$2");
    parsed = Date.parse(trimmed);
  }
  return Number.isFinite(parsed) ? parsed : null;
};

const resolveStatusLabel = (status) => {
  if (!status) {
    return t("throughput.status.idle");
  }
  const key = `throughput.status.${status}`;
  const text = t(key);
  return text || status;
};

const getSessionStatusLabel = (status) => {
  const normalized = String(status || "");
  const mapping = {
    running: t("monitor.sessionStatus.running"),
    cancelling: t("monitor.sessionStatus.cancelling"),
    finished: t("monitor.sessionStatus.finished"),
    error: t("monitor.sessionStatus.error"),
    cancelled: t("monitor.sessionStatus.cancelled"),
  };
  return mapping[normalized] || normalized || "-";
};

const buildStatusBadge = (status) => {
  const span = document.createElement("span");
  span.className = `monitor-status ${status}`;
  span.textContent = getSessionStatusLabel(status);
  return span;
};

const sortSessionsByUpdate = (sessions) =>
  [...sessions].sort((a, b) => {
    const left = new Date(b.updated_time || b.start_time).getTime();
    const right = new Date(a.updated_time || a.start_time).getTime();
    return left - right;
  });

const applyStatusBadge = (status) => {
  if (!elements.throughputStatusBadge) {
    return;
  }
  const badge = elements.throughputStatusBadge;
  badge.textContent = resolveStatusLabel(status);
  badge.classList.remove("running", "stopping", "finished", "stopped");
  if (status) {
    badge.classList.add(status);
  }
};

const updateStatusIndicator = (status) => {
  if (!elements.throughputStatusIndicator) {
    return;
  }
  const normalized = status || "idle";
  const label = resolveStatusLabel(normalized);
  const indicator = elements.throughputStatusIndicator;
  const text = indicator.querySelector(".status-text");
  indicator.dataset.status = normalized;
  indicator.classList.toggle("is-active", ["running", "stopping"].includes(normalized));
  if (text) {
    text.textContent = label;
    text.setAttribute("data-i18n", `throughput.status.${normalized}`);
  }
};

const setFormStatus = (text) => {
  if (!elements.throughputFormStatus) {
    return;
  }
  elements.throughputFormStatus.textContent = text || "";
};

const resolvePrimarySnapshot = (payload) => {
  if (!payload || typeof payload !== "object") {
    return { snapshot: null, fromHistory: false };
  }
  if (payload.active) {
    return { snapshot: payload.active, fromHistory: false };
  }
  const history = Array.isArray(payload.history) ? payload.history : [];
  if (!history.length) {
    return { snapshot: null, fromHistory: false };
  }
  return { snapshot: history[history.length - 1], fromHistory: true };
};

const updateToggleButton = (status) => {
  if (!elements.throughputToggleBtn) {
    return;
  }
  const running = ACTIVE_RUN_STATUSES.has(status);
  const stopping = status === "stopping";
  const button = elements.throughputToggleBtn;
  const label = button.querySelector("[data-role='label']");
  const icon = button.querySelector("[data-role='icon']");
  button.dataset.state = running ? "stop" : "start";
  button.classList.toggle("danger", running);
  button.disabled = stopping;
  if (label) {
    const key = running ? "throughput.action.stop" : "throughput.action.start";
    label.setAttribute("data-i18n", key);
    label.textContent = t(key);
  }
  if (icon) {
    icon.className = `fa-solid ${running ? "fa-stop" : "fa-play"}`;
  }
};

const renderSnapshot = (snapshot, fromHistory, options = {}) => {
  if (!snapshot) {
    currentRunSnapshot = null;
    applyStatusBadge("");
    currentStatus = "";
    updateToggleButton("");
    setStatusHint(t("throughput.status.emptyHint"));
    updateStatusIndicator("idle");
    setTotalSpeedMetrics(null, null);
    setSingleSpeedMetrics(null, null);
    fillMetric();
    resetThroughputSessions({ resetContext: true });
    resetCharts();
    return;
  }
  const run = snapshot.run || {};
  currentRunSnapshot = run;
  syncThroughputSessionContext(run);
  syncCurveRun(run.id);
  const metrics = snapshot.metrics || {};
  const status = run.status || "";
  currentStatus = status;
  applyStatusBadge(status);
  updateStatusIndicator(status || "idle");
  updateToggleButton(status);
  if (options.historyView) {
    setStatusHint(t("throughput.status.historyViewHint"));
    setSingleSpeedMetrics(null, null);
    setTotalSpeedMetrics(null, null);
    resetThroughputSessions();
  } else {
    setStatusHint(fromHistory ? t("throughput.status.historyHint") : "");
  }
  setText(elements.throughputStatusText, resolveStatusLabel(status));
  setText(elements.throughputStartedAt, formatTimestamp(run.started_at));
  setText(elements.throughputElapsed, formatElapsed(run.elapsed_s));
  const concurrencyList = resolveRunConcurrencyList(run);
  const maxConcurrency = resolveRunMaxConcurrency(run, concurrencyList);
  setText(
    elements.throughputMaxConcurrencyValue,
    formatCount(Number.isFinite(maxConcurrency) && maxConcurrency > 0 ? maxConcurrency : null)
  );
  const concurrencySummary = formatConcurrencyList(concurrencyList);
  setText(elements.throughputConcurrencyListValue, concurrencySummary.text);
  if (elements.throughputConcurrencyListValue) {
    elements.throughputConcurrencyListValue.title = concurrencySummary.full || "";
  }
  syncModelSelectState(run, { historyView: options.historyView });
  updateModelValue(run);
  setText(elements.throughputTotal, formatCount(metrics.total_requests));
  setText(elements.throughputSuccess, formatCount(metrics.success_requests));
  setText(elements.throughputError, formatCount(metrics.error_requests));
  setText(elements.throughputRps, formatRate(metrics.rps));
  setText(elements.throughputAvgLatency, formatLatency(metrics.avg_latency_ms));
  setText(
    elements.throughputFirstTokenLatency,
    formatLatency(metrics.first_token_latency_ms)
  );
  setText(elements.throughputTotalTokens, formatTokenCount(metrics.total_tokens));
  setText(elements.throughputAvgTokens, formatTokenCount(metrics.avg_total_tokens));
  applySpeedMetrics();
};

const fillMetric = (...values) => {
  const filled = new Array(17).fill("-");
  values.forEach((value, index) => {
    if (index < filled.length) {
      filled[index] = value;
    }
  });
  const [
    status,
    started,
    elapsed,
    maxConcurrency,
    concurrencyList,
    total,
    success,
    error,
    rps,
    avgLatency,
    firstTokenLatency,
    totalPrefillSpeed,
    totalDecodeSpeed,
    singlePrefillSpeed,
    singleDecodeSpeed,
    totalTokens,
    avgTokens,
  ] = filled;
  setText(elements.throughputStatusText, status);
  setText(elements.throughputStartedAt, started);
  setText(elements.throughputElapsed, elapsed);
  setText(elements.throughputMaxConcurrencyValue, maxConcurrency);
  setText(elements.throughputConcurrencyListValue, concurrencyList);
  if (elements.throughputConcurrencyListValue) {
    elements.throughputConcurrencyListValue.title = "";
  }
  syncModelSelectState(null);
  updateModelValue(null);
  setText(elements.throughputTotal, total);
  setText(elements.throughputSuccess, success);
  setText(elements.throughputError, error);
  setText(elements.throughputRps, rps);
  setText(elements.throughputAvgLatency, avgLatency);
  setText(elements.throughputFirstTokenLatency, firstTokenLatency);
  setText(elements.throughputTotalPrefillSpeed, totalPrefillSpeed);
  setText(elements.throughputTotalDecodeSpeed, totalDecodeSpeed);
  setText(elements.throughputSinglePrefillSpeed, singlePrefillSpeed);
  setText(elements.throughputSingleDecodeSpeed, singleDecodeSpeed);
  setText(elements.throughputTotalTokens, totalTokens);
  setText(elements.throughputAvgTokens, avgTokens);
};

const setText = (element, value) => {
  if (!element) {
    return;
  }
  element.textContent = value || "-";
};

const setStatusHint = (text) => {
  if (!elements.throughputStatusHint) {
    return;
  }
  elements.throughputStatusHint.textContent = text || "";
};

const applySpeedMetrics = () => {
  if (elements.throughputTotalPrefillSpeed) {
    elements.throughputTotalPrefillSpeed.textContent = formatTokenRate(currentTotalPrefillSpeed);
  }
  if (elements.throughputTotalDecodeSpeed) {
    elements.throughputTotalDecodeSpeed.textContent = formatTokenRate(currentTotalDecodeSpeed);
  }
  if (elements.throughputSinglePrefillSpeed) {
    elements.throughputSinglePrefillSpeed.textContent = formatTokenRate(currentSinglePrefillSpeed);
  }
  if (elements.throughputSingleDecodeSpeed) {
    elements.throughputSingleDecodeSpeed.textContent = formatTokenRate(currentSingleDecodeSpeed);
  }
};

const setTotalSpeedMetrics = (prefill, decode) => {
  currentTotalPrefillSpeed = Number.isFinite(prefill) ? prefill : null;
  currentTotalDecodeSpeed = Number.isFinite(decode) ? decode : null;
  applySpeedMetrics();
};

const setSingleSpeedMetrics = (prefill, decode) => {
  currentSinglePrefillSpeed = Number.isFinite(prefill) ? prefill : null;
  currentSingleDecodeSpeed = Number.isFinite(decode) ? decode : null;
  applySpeedMetrics();
};

const enterHistoryMode = (runId) => {
  state.runtime.throughputHistoryMode = true;
  state.runtime.throughputHistoryRunId = runId || "";
  setTotalSpeedMetrics(null, null);
  setSingleSpeedMetrics(null, null);
  resetThroughputSessions({ resetContext: true });
  stopPolling();
};

const exitHistoryMode = () => {
  state.runtime.throughputHistoryMode = false;
  state.runtime.throughputHistoryRunId = "";
};

const resolveThreadPrefix = (run) => {
  const prefix = String(run?.user_id_prefix || "").trim();
  if (prefix) {
    return prefix;
  }
  const fallback = String(elements.throughputUserPrefix?.value || "").trim();
  return fallback || DEFAULT_CONFIG.user_id_prefix;
};

const resolveSessionConcurrency = (session) => {
  const runId = throughputSessionRunId;
  const sessionId = String(session?.session_id || "");
  if (runId && sessionId) {
    const prefix = `throughput_${runId}_`;
    if (!sessionId.startsWith(prefix)) {
      return null;
    }
    const remainder = sessionId.slice(prefix.length);
    const value = Number(remainder.split("_")[0]);
    if (Number.isFinite(value) && value > 0) {
      return value;
    }
    return null;
  }
  const userPrefix = throughputSessionPrefix;
  const userId = String(session?.user_id || "");
  if (userPrefix && userId.startsWith(`${userPrefix}-`)) {
    const remainder = userId.slice(userPrefix.length + 1);
    const value = Number(remainder.split("-")[0]);
    if (Number.isFinite(value) && value > 0) {
      return value;
    }
  }
  return null;
};

const resolveMaxConcurrency = (sessions) => {
  let max = 0;
  sessions.forEach((session) => {
    const value = resolveSessionConcurrency(session);
    if (Number.isFinite(value) && value > max) {
      max = value;
    }
  });
  return max > 0 ? max : null;
};

const resolveTargetConcurrency = (sessions) => {
  const active = sessions.filter((session) => ACTIVE_SESSION_STATUSES.has(session?.status));
  return resolveMaxConcurrency(active) ?? resolveMaxConcurrency(sessions);
};

const filterSessionsByConcurrency = (sessions, concurrency) => {
  if (!Number.isFinite(concurrency) || concurrency <= 0) {
    return sessions;
  }
  return sessions.filter((session) => resolveSessionConcurrency(session) === concurrency);
};

const syncThreadFilterButtons = () => {
  const mapping = {
    all: elements.throughputThreadFilterAll,
    active: elements.throughputThreadFilterActive,
    finished: elements.throughputThreadFilterFinished,
    failed: elements.throughputThreadFilterFailed,
  };
  Object.entries(mapping).forEach(([key, button]) => {
    if (!button) {
      return;
    }
    button.classList.toggle("active", throughputThreadFilter === key);
  });
};

const setThreadFilter = (filter) => {
  const next = THREAD_FILTERS.includes(filter) ? filter : "active";
  if (throughputThreadFilter === next) {
    return;
  }
  throughputThreadFilter = next;
  syncThreadFilterButtons();
  renderThroughputSessions();
};

const resetThroughputSessions = (options = {}) => {
  throughputSessionMap = new Map();
  throughputSessions = [];
  if (options.resetContext) {
    throughputSessionRunId = "";
    throughputSessionStartMs = null;
    throughputSessionPrefix = "";
  }
  renderThroughputSessions();
};

const syncThroughputSessionContext = (run) => {
  const runId = run?.id || "";
  const startedAtMs = parseTimestampMs(run?.started_at);
  const prefix = resolveThreadPrefix(run);
  const runChanged = runId !== throughputSessionRunId;
  if (runChanged) {
    throughputSessionMap = new Map();
    throughputSessions = [];
    setSingleSpeedMetrics(null, null);
    setTotalSpeedMetrics(null, null);
  }
  throughputSessionRunId = runId;
  throughputSessionStartMs = startedAtMs;
  throughputSessionPrefix = prefix;
  if (runChanged) {
    renderThroughputSessions();
  }
};

const syncCurveRun = (runId) => {
  const nextId = runId || "";
  if (!nextId || nextId === currentRunId) {
    return;
  }
  currentRunId = nextId;
  samples = [];
  reportSampleByConcurrency = new Map();
  lastReportFetchAt = 0;
  renderCurveChart();
};

const resolveThreadEmptyText = () => {
  const key = state.runtime.throughputHistoryMode
    ? "throughput.threads.emptyHistory"
    : "throughput.threads.empty";
  return { key, text: t(key) };
};

const matchThroughputSession = (session) => {
  const runId = throughputSessionRunId;
  const sessionId = String(session?.session_id || "");
  if (runId) {
    const prefix = `throughput_${runId}_`;
    if (!sessionId.startsWith(prefix)) {
      return false;
    }
  }
  const prefix = throughputSessionPrefix;
  if (prefix) {
    const userId = String(session?.user_id || "");
    if (!userId.startsWith(prefix)) {
      return false;
    }
  }
  if (throughputSessionStartMs) {
    const startedAtMs = parseTimestampMs(session?.start_time);
    if (Number.isFinite(startedAtMs) && startedAtMs + 1000 < throughputSessionStartMs) {
      return false;
    }
  }
  return true;
};

const updateThroughputSessions = (sessions) => {
  sessions.forEach((session) => {
    const sessionId = session?.session_id;
    if (!sessionId) {
      return;
    }
    const existing = throughputSessionMap.get(sessionId);
    throughputSessionMap.set(sessionId, existing ? { ...existing, ...session } : session);
  });
  throughputSessions = sortSessionsByUpdate(Array.from(throughputSessionMap.values()));
};

const resolvePositiveNumber = (value) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const collectSessionSpeedStats = (sessions, tokenKey, durationKey, speedKey) => {
  let tokensTotal = 0;
  let durationTotal = 0;
  let speedSum = 0;
  let speedCount = 0;
  sessions.forEach((session) => {
    const tokens = resolvePositiveNumber(session?.[tokenKey]);
    const duration = resolvePositiveNumber(session?.[durationKey]);
    const speed = resolvePositiveNumber(session?.[speedKey]);
    let resolvedSpeed = null;
    if (tokens && duration) {
      tokensTotal += tokens;
      durationTotal += duration;
      resolvedSpeed = tokens / duration;
    } else if (tokens && speed) {
      const derivedDuration = tokens / speed;
      if (Number.isFinite(derivedDuration) && derivedDuration > 0) {
        tokensTotal += tokens;
        durationTotal += derivedDuration;
        resolvedSpeed = speed;
      }
    } else if (speed) {
      resolvedSpeed = speed;
    }
    if (resolvedSpeed) {
      speedSum += resolvedSpeed;
      speedCount += 1;
    }
  });
  return { tokensTotal, durationTotal, speedSum, speedCount };
};

const computeAverageSpeed = (sessions, tokenKey, durationKey, speedKey) => {
  const stats = collectSessionSpeedStats(sessions, tokenKey, durationKey, speedKey);
  if (stats.tokensTotal > 0 && stats.durationTotal > 0) {
    return stats.tokensTotal / stats.durationTotal;
  }
  if (stats.speedCount > 0) {
    return stats.speedSum / stats.speedCount;
  }
  return null;
};

const computeSumSpeed = (sessions, tokenKey, durationKey, speedKey) => {
  const stats = collectSessionSpeedStats(sessions, tokenKey, durationKey, speedKey);
  if (stats.speedCount > 0) {
    return stats.speedSum;
  }
  if (stats.tokensTotal > 0 && stats.durationTotal > 0) {
    return stats.tokensTotal / stats.durationTotal;
  }
  return null;
};

const renderThroughputSessions = () => {
  if (!elements.throughputThreadBody || !elements.throughputThreadEmpty) {
    return;
  }
  const body = elements.throughputThreadBody;
  body.textContent = "";
  const list = Array.isArray(throughputSessions) ? throughputSessions : [];
  const filtered = list.filter((session) => {
    const status = session?.status;
    if (throughputThreadFilter === "all") {
      return true;
    }
    if (throughputThreadFilter === "active") {
      return ACTIVE_SESSION_STATUSES.has(status);
    }
    if (throughputThreadFilter === "finished") {
      return status === "finished";
    }
    if (throughputThreadFilter === "failed") {
      return FAILED_SESSION_STATUSES.has(status);
    }
    return true;
  });
  if (!filtered.length) {
    const empty = resolveThreadEmptyText();
    elements.throughputThreadEmpty.setAttribute("data-i18n", empty.key);
    elements.throughputThreadEmpty.textContent = empty.text;
    elements.throughputThreadEmpty.style.display = "block";
    return;
  }
  elements.throughputThreadEmpty.style.display = "none";
  filtered.forEach((session) => {
    const row = document.createElement("tr");
    const startCell = document.createElement("td");
    startCell.textContent = formatTimestamp(session.start_time);
    const sessionCell = document.createElement("td");
    const rawSessionId = session.session_id || "";
    sessionCell.textContent = rawSessionId ? rawSessionId.slice(0, 4) : "-";
    if (rawSessionId) {
      sessionCell.title = rawSessionId;
    }
    const userCell = document.createElement("td");
    userCell.textContent = session.user_id || "-";
    const questionCell = document.createElement("td");
    questionCell.textContent = session.question || "-";
    if (session.question) {
      questionCell.title = session.question;
    }
    const statusCell = document.createElement("td");
    statusCell.appendChild(buildStatusBadge(session.status || ""));
    const tokenCell = document.createElement("td");
    tokenCell.textContent = formatTokenCount(session.token_usage);
    const elapsedCell = document.createElement("td");
    elapsedCell.textContent = formatDuration(session.elapsed_s);
    const stageCell = document.createElement("td");
    stageCell.textContent = session.stage || "-";
    row.appendChild(startCell);
    row.appendChild(sessionCell);
    row.appendChild(userCell);
    row.appendChild(questionCell);
    row.appendChild(statusCell);
    row.appendChild(tokenCell);
    row.appendChild(elapsedCell);
    row.appendChild(stageCell);
    row.addEventListener("click", () => {
      if (session.session_id) {
        openMonitorDetail(session.session_id);
      }
    });
    body.appendChild(row);
  });
};

const fetchMonitorSessions = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("throughput.error.apiBase"));
  }
  const url = new URL(`${wunderBase}/admin/monitor`);
  url.searchParams.set("active_only", "false");
  const response = await fetch(url.toString());
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const payload = await response.json();
  return {
    sessions: Array.isArray(payload.sessions) ? payload.sessions : [],
    service: payload.service || null,
  };
};

const loadThroughputSessions = async (options = {}) => {
  if (!elements.throughputThreadBody || !elements.throughputThreadEmpty) {
    return;
  }
  if (state.runtime.throughputHistoryMode) {
    resetThroughputSessions();
    return;
  }
  const silent = options.silent === true;
  if (!silent) {
    elements.throughputThreadEmpty.textContent = t("common.loading");
    elements.throughputThreadEmpty.style.display = "block";
  }
  try {
    const payload = await fetchMonitorSessions();
    const sessions = payload.sessions || [];
    const scoped = sessions.filter((session) => matchThroughputSession(session));
    updateThroughputSessions(scoped);
    renderThroughputSessions();
    const targetConcurrency = resolveTargetConcurrency(scoped);
    if (!applyReportSampleSpeedMetrics(targetConcurrency)) {
      const concurrencySessions = filterSessionsByConcurrency(scoped, targetConcurrency);
      const prefillSpeed = computeAverageSpeed(
        concurrencySessions,
        "prefill_tokens",
        "prefill_duration_s",
        "prefill_speed_tps"
      );
      const decodeSpeed = computeAverageSpeed(
        concurrencySessions,
        "decode_tokens",
        "decode_duration_s",
        "decode_speed_tps"
      );
      if (Number.isFinite(prefillSpeed) || Number.isFinite(decodeSpeed)) {
        setSingleSpeedMetrics(prefillSpeed, decodeSpeed);
      }
      const totalPrefillSpeed = computeSumSpeed(
        concurrencySessions,
        "prefill_tokens",
        "prefill_duration_s",
        "prefill_speed_tps"
      );
      const totalDecodeSpeed = computeSumSpeed(
        concurrencySessions,
        "decode_tokens",
        "decode_duration_s",
        "decode_speed_tps"
      );
      if (Number.isFinite(totalPrefillSpeed) || Number.isFinite(totalDecodeSpeed)) {
        setTotalSpeedMetrics(totalPrefillSpeed, totalDecodeSpeed);
      }
    }
  } catch (error) {
    if (!silent) {
      elements.throughputThreadEmpty.textContent = t("common.loadFailedWithMessage", {
        message: error.message,
      });
      elements.throughputThreadEmpty.style.display = "block";
    }
  }
};

const buildPayload = () => {
  const concurrencyList = parseConcurrencyList(elements.throughputConcurrencyList?.value);
  if (!concurrencyList) {
    throw new Error(t("throughput.error.concurrencyList"));
  }
  const user_id_prefix = String(elements.throughputUserPrefix?.value || "").trim() || undefined;
  const rawModelName = getSelectedModelName();
  const defaultModelName = String(state.llm.defaultName || "").trim();
  const model_name = rawModelName || defaultModelName || undefined;
  const max_tokens = parseNonNegativeInt(elements.throughputMaxTokens);
  if (!Number.isFinite(max_tokens) || max_tokens <= 0) {
    throw new Error(t("throughput.error.maxTokens"));
  }
  const request_timeout_s = Number(elements.throughputTimeout?.value);
  if (Number.isFinite(request_timeout_s) && request_timeout_s < 0) {
    throw new Error(t("throughput.error.timeout"));
  }
  return {
    concurrency_list: concurrencyList,
    user_id_prefix,
    model_name,
    max_tokens,
    request_timeout_s: Number.isFinite(request_timeout_s) ? request_timeout_s : undefined,
  };
};

const resetCharts = () => {
  samples = [];
  reportSampleByConcurrency = new Map();
  currentRunId = "";
  lastReportFetchAt = 0;
  renderCurveChart();
};

const ensureCurveChart = () => {
  if (!window.echarts || !elements.throughputCurveChart) {
    return null;
  }
  if (!chartCurve) {
    chartCurve = window.echarts.init(elements.throughputCurveChart);
  }
  return chartCurve;
};

const toMetricValue = (value) => {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizeCurveSample = (sample) => {
  if (!sample || typeof sample !== "object") {
    return null;
  }
  const concurrency = Number(sample.concurrency) || 0;
  if (concurrency <= 0) {
    return null;
  }
  return {
    concurrency,
    metrics: {
      total_prefill_speed_tps: toMetricValue(sample.total_prefill_speed_tps),
      single_prefill_speed_tps: toMetricValue(sample.single_prefill_speed_tps),
      total_decode_speed_tps: toMetricValue(sample.total_decode_speed_tps),
      single_decode_speed_tps: toMetricValue(sample.single_decode_speed_tps),
    },
  };
};

const normalizeCurveSamples = (reportSamples) =>
  reportSamples
    .map(normalizeCurveSample)
    .filter(Boolean)
    .sort((left, right) => left.concurrency - right.concurrency);

const buildReportSampleMap = (reportSamples) => {
  const map = new Map();
  reportSamples.forEach((sample) => {
    const concurrency = Number(sample?.concurrency);
    if (Number.isFinite(concurrency) && concurrency > 0) {
      map.set(concurrency, sample);
    }
  });
  return map;
};

const getCurveBaseline = (curveSamples) => {
  const baseline = {};
  const first = curveSamples.find((sample) => sample.concurrency === 1) || curveSamples[0];
  if (!first) {
    return baseline;
  }
  CURVE_METRICS.forEach((metric) => {
    const value = Number(first.metrics?.[metric.key]);
    baseline[metric.key] = Number.isFinite(value) && value > 0 ? value : null;
  });
  return baseline;
};

const renderCurveChart = () => {
  const chart = ensureCurveChart();
  if (!chart) {
    return;
  }
  const curveSamples = Array.isArray(samples) ? samples : [];
  const baseline = getCurveBaseline(curveSamples);
  const xValues = curveSamples.map((sample) => sample.concurrency);
  const series = CURVE_METRICS.map((metric) => {
    const values = curveSamples.map((sample) => {
      const value = Number(sample.metrics?.[metric.key]);
      const base = baseline[metric.key];
      if (!Number.isFinite(value) || !Number.isFinite(base) || base <= 0) {
        return null;
      }
      const delta = ((value - base) / base) * 100;
      return Number.isFinite(delta) ? Number(delta.toFixed(2)) : null;
    });
    return {
      name: t(metric.labelKey),
      type: "line",
      smooth: true,
      showSymbol: false,
      data: values,
      lineStyle: { color: metric.color, width: 2 },
      itemStyle: { color: metric.color },
    };
  });
  const option = {
    tooltip: {
      trigger: "axis",
      formatter: (params) => {
        if (!Array.isArray(params) || !params.length) {
          return "";
        }
        const dataIndex = params[0]?.dataIndex ?? -1;
        const sample = curveSamples[dataIndex];
        const concurrencyLabel = t("throughput.chart.axis.concurrency");
        const header = `${concurrencyLabel}: ${sample?.concurrency ?? "-"}`;
        const lines = params.map((item) => {
          const metric = CURVE_METRICS[item.seriesIndex];
          const actualValue = sample?.metrics?.[metric.key];
          const actualText = Number.isFinite(actualValue) ? formatTokenRate(actualValue) : "-";
          const deltaText = Number.isFinite(item.value)
            ? `${Number(item.value).toFixed(2)}%`
            : "-";
          const suffix = actualText !== "-" ? ` (${actualText})` : "";
          return `${item.marker}${t(metric.labelKey)}: ${deltaText}${suffix}`;
        });
        return [header, ...lines].join("<br/>");
      },
    },
    legend: {
      data: CURVE_METRICS.map((metric) => t(metric.labelKey)),
      textStyle: { color: "#64748b" },
    },
    grid: { left: 50, right: 24, top: 30, bottom: 30 },
    xAxis: {
      type: "category",
      name: t("throughput.chart.axis.concurrency"),
      data: xValues,
      axisLabel: { color: "#94a3b8" },
      axisLine: { lineStyle: { color: "#e2e8f0" } },
    },
    yAxis: {
      type: "value",
      name: t("throughput.chart.axis.delta"),
      axisLabel: {
        color: "#94a3b8",
        formatter: (value) => `${value}%`,
      },
      splitLine: { lineStyle: { color: "#e2e8f0" } },
    },
    series,
  };
  chart.setOption(option, false);
  chart.resize();
};

const applyCurveReport = (report) => {
  if (!report) {
    return;
  }
  const runId = report?.summary?.run?.id || "";
  if (runId && runId !== currentRunId) {
    currentRunId = runId;
  }
  const reportSamples = Array.isArray(report.samples) ? report.samples : [];
  reportSampleByConcurrency = buildReportSampleMap(reportSamples);
  const normalized = normalizeCurveSamples(reportSamples);
  samples = normalized.slice(-MAX_SAMPLES);
  renderCurveChart();
  applyLiveSpeedMetrics(report);
};

const fetchThroughputStatus = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("throughput.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/throughput/status`);
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const fetchThroughputReport = async (runId) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("throughput.error.apiBase"));
  }
  const url = new URL(`${wunderBase}/admin/throughput/report`);
  if (runId) {
    url.searchParams.set("run_id", runId);
  }
  const response = await fetch(url.toString());
  if (!response.ok) {
    const message = await parseErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const loadCurveReport = async (runId, options = {}) => {
  const silent = options.silent === true;
  const now = Date.now();
  if (!options.force && now - lastReportFetchAt < 800) {
    return;
  }
  lastReportFetchAt = now;
  try {
    const report = await fetchThroughputReport(runId);
    applyCurveReport(report);
  } catch (error) {
    if (!silent) {
      throw error;
    }
  }
};

const parseErrorMessage = async (response) => {
  try {
    const payload = await response.json();
    return payload?.detail?.message || payload?.message || "";
  } catch (error) {
    try {
      return await response.text();
    } catch (innerError) {
      return "";
    }
  }
};

const openHistoryModal = async () => {
  if (!elements.throughputHistoryModal) {
    return;
  }
  elements.throughputHistoryModal.classList.add("active");
  await loadHistoryList();
};

const closeHistoryModal = () => {
  elements.throughputHistoryModal?.classList.remove("active");
};

const renderHistoryMeta = (count) => {
  if (!elements.throughputHistoryMeta) {
    return;
  }
  elements.throughputHistoryMeta.textContent = t("throughput.history.meta", { count });
};

const formatQuestionSetSummary = (run) => {
  const questionSet = String(run?.question_set ?? run?.questionSet ?? "").trim();
  const questionCount = Number(run?.question_count ?? run?.questionCount);
  if (questionSet) {
    if (questionSet === "builtin") {
      const count = Number.isFinite(questionCount) && questionCount > 0 ? questionCount : 50;
      return t("throughput.dataset.builtin", { count });
    }
    if (Number.isFinite(questionCount) && questionCount > 0) {
      return `${questionSet} (${questionCount})`;
    }
    return questionSet;
  }
  const questions = Array.isArray(run?.questions) ? [...run.questions] : [];
  if (!questions.length && run?.question) {
    questions.push(run.question);
  }
  if (!questions.length) {
    return "-";
  }
  if (questions.length === 1) {
    return questions[0];
  }
  return `${questions[0]} (+${questions.length - 1})`;
};

const renderHistoryList = (history) => {
  if (!elements.throughputHistoryList || !elements.throughputHistoryEmpty) {
    return;
  }
  const list = Array.isArray(history) ? history.slice().reverse() : [];
  elements.throughputHistoryList.textContent = "";
  if (!list.length) {
    elements.throughputHistoryEmpty.style.display = "block";
    renderHistoryMeta(0);
    return;
  }
  elements.throughputHistoryEmpty.style.display = "none";
  renderHistoryMeta(list.length);
  list.forEach((snapshot) => {
    const run = snapshot.run || {};
    const metrics = snapshot.metrics || {};
    const row = document.createElement("tr");
    const startedCell = document.createElement("td");
    startedCell.textContent = formatTimestamp(run.started_at);
    const statusCell = document.createElement("td");
    statusCell.textContent = resolveStatusLabel(run.status || "");
    const concurrencyList = resolveRunConcurrencyList(run);
    const maxConcurrencyValue = resolveRunMaxConcurrency(run, concurrencyList);
    const maxCell = document.createElement("td");
    maxCell.textContent = formatCount(
      Number.isFinite(maxConcurrencyValue) && maxConcurrencyValue > 0 ? maxConcurrencyValue : null
    );
    const concurrencyCell = document.createElement("td");
    const concurrencySummary = formatConcurrencyList(concurrencyList, { maxItems: 4 });
    concurrencyCell.textContent = concurrencySummary.text;
    if (concurrencySummary.full) {
      concurrencyCell.title = concurrencySummary.full;
    }
    const totalCell = document.createElement("td");
    totalCell.textContent = formatCount(metrics.total_requests);
    const successRateCell = document.createElement("td");
    const totalRequests = Number(metrics.total_requests);
    const successRequests = Number(metrics.success_requests);
    const successRate =
      totalRequests > 0 ? formatPercent((successRequests / totalRequests) * 100) : "-";
    successRateCell.textContent = successRate;
    const avgLatencyCell = document.createElement("td");
    avgLatencyCell.textContent = formatLatency(metrics.avg_latency_ms);
    const rpsCell = document.createElement("td");
    rpsCell.textContent = formatRate(metrics.rps);
    const datasetCell = document.createElement("td");
    datasetCell.textContent = formatQuestionSetSummary(run);
    const actionCell = document.createElement("td");
    const restoreBtn = document.createElement("button");
    restoreBtn.className = "secondary btn-with-icon";
    restoreBtn.type = "button";
    restoreBtn.textContent = t("throughput.history.restore");
    restoreBtn.addEventListener("click", async () => {
      try {
        await restoreHistoryReport(run.id);
        closeHistoryModal();
      } catch (error) {
        const message = error?.message || String(error);
        notify(t("throughput.history.restoreFailed", { message }), "error");
      }
    });
    actionCell.appendChild(restoreBtn);
    row.appendChild(startedCell);
    row.appendChild(statusCell);
    row.appendChild(maxCell);
    row.appendChild(concurrencyCell);
    row.appendChild(totalCell);
    row.appendChild(successRateCell);
    row.appendChild(avgLatencyCell);
    row.appendChild(rpsCell);
    row.appendChild(datasetCell);
    row.appendChild(actionCell);
    elements.throughputHistoryList.appendChild(row);
  });
};

const loadHistoryList = async () => {
  if (!elements.throughputHistoryList || !elements.throughputHistoryEmpty) {
    return;
  }
  elements.throughputHistoryList.textContent = t("common.loading");
  elements.throughputHistoryEmpty.style.display = "none";
  try {
    const payload = await fetchThroughputStatus();
    renderHistoryList(payload?.history || []);
  } catch (error) {
    elements.throughputHistoryList.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const applyReport = (report) => {
  if (!report || !report.summary) {
    return;
  }
  const summary = report.summary;
  renderSnapshot(summary, true, { historyView: true });
  applyCurveReport(report);
  applyHistorySpeedMetrics(report);
};

const resolveHistorySample = (report) => {
  const reportSamples = Array.isArray(report?.samples) ? report.samples : [];
  if (!reportSamples.length) {
    return null;
  }
  return reportSamples[reportSamples.length - 1];
};

const getReportSampleByConcurrency = (concurrency) => {
  if (!Number.isFinite(concurrency) || concurrency <= 0) {
    return null;
  }
  return reportSampleByConcurrency.get(concurrency) || null;
};

const resolveSpeedMetrics = (sample) => {
  if (!sample) {
    return null;
  }
  const concurrency = Number(sample.concurrency);
  const legacyPrefill = Number(sample.prefill_speed_tps);
  const legacyDecode = Number(sample.decode_speed_tps);
  let singlePrefill = Number(sample.single_prefill_speed_tps);
  if (!Number.isFinite(singlePrefill) || singlePrefill <= 0) {
    singlePrefill = Number.isFinite(legacyPrefill) && legacyPrefill > 0 ? legacyPrefill : NaN;
  }
  let singleDecode = Number(sample.single_decode_speed_tps);
  if (!Number.isFinite(singleDecode) || singleDecode <= 0) {
    singleDecode = Number.isFinite(legacyDecode) && legacyDecode > 0 ? legacyDecode : NaN;
  }
  const totalPrefillRaw = Number(sample.total_prefill_speed_tps);
  const totalDecodeRaw = Number(sample.total_decode_speed_tps);
  let totalPrefill = Number.isFinite(totalPrefillRaw) && totalPrefillRaw > 0 ? totalPrefillRaw : NaN;
  let totalDecode = Number.isFinite(totalDecodeRaw) && totalDecodeRaw > 0 ? totalDecodeRaw : NaN;
  if (
    (!Number.isFinite(totalPrefill) || totalPrefill <= 0) &&
    Number.isFinite(singlePrefill) &&
    Number.isFinite(concurrency) &&
    concurrency > 0
  ) {
    totalPrefill = singlePrefill * concurrency;
  }
  if (
    (!Number.isFinite(totalDecode) || totalDecode <= 0) &&
    Number.isFinite(singleDecode) &&
    Number.isFinite(concurrency) &&
    concurrency > 0
  ) {
    totalDecode = singleDecode * concurrency;
  }
  if ((!Number.isFinite(singlePrefill) || singlePrefill <= 0) && Number.isFinite(totalPrefill)) {
    if (Number.isFinite(concurrency) && concurrency > 0) {
      singlePrefill = totalPrefill / concurrency;
    }
  }
  if ((!Number.isFinite(singleDecode) || singleDecode <= 0) && Number.isFinite(totalDecode)) {
    if (Number.isFinite(concurrency) && concurrency > 0) {
      singleDecode = totalDecode / concurrency;
    }
  }
  return {
    singlePrefill,
    singleDecode,
    totalPrefill,
    totalDecode,
  };
};

const applyReportSampleSpeedMetrics = (concurrency) => {
  const sample = getReportSampleByConcurrency(concurrency);
  if (!sample) {
    return false;
  }
  const metrics = resolveSpeedMetrics(sample);
  if (!metrics) {
    return false;
  }
  const hasSingle =
    Number.isFinite(metrics.singlePrefill) || Number.isFinite(metrics.singleDecode);
  const hasTotal =
    Number.isFinite(metrics.totalPrefill) || Number.isFinite(metrics.totalDecode);
  if (hasSingle) {
    setSingleSpeedMetrics(metrics.singlePrefill, metrics.singleDecode);
  }
  if (hasTotal) {
    setTotalSpeedMetrics(metrics.totalPrefill, metrics.totalDecode);
  }
  return hasSingle || hasTotal;
};

const applyHistorySpeedMetrics = (report) => {
  const sample = resolveHistorySample(report);
  if (!sample) {
    setTotalSpeedMetrics(null, null);
    setSingleSpeedMetrics(null, null);
    return;
  }
  const metrics = resolveSpeedMetrics(sample);
  if (!metrics) {
    return;
  }
  setSingleSpeedMetrics(metrics.singlePrefill, metrics.singleDecode);
  setTotalSpeedMetrics(metrics.totalPrefill, metrics.totalDecode);
};

const applyLiveSpeedMetrics = (report) => {
  if (state.runtime.throughputHistoryMode) {
    return;
  }
  const sample = resolveHistorySample(report);
  if (!sample) {
    return;
  }
  const metrics = resolveSpeedMetrics(sample);
  if (!metrics) {
    return;
  }
  if (Number.isFinite(metrics.singlePrefill) || Number.isFinite(metrics.singleDecode)) {
    setSingleSpeedMetrics(metrics.singlePrefill, metrics.singleDecode);
  }
  if (Number.isFinite(metrics.totalPrefill) || Number.isFinite(metrics.totalDecode)) {
    setTotalSpeedMetrics(metrics.totalPrefill, metrics.totalDecode);
  }
};

const restoreHistoryReport = async (runId) => {
  if (!runId) {
    throw new Error(t("throughput.history.restoreMissing"));
  }
  setFormStatus(t("throughput.history.restoring"));
  const report = await fetchThroughputReport(runId);
  enterHistoryMode(runId);
  applyReport(report);
  setFormStatus(t("throughput.history.restored"));
  notify(t("throughput.history.restored"), "success");
};

const startThroughput = async (payload) => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("throughput.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/throughput/start`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const message = await parseErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const stopThroughput = async () => {
  const wunderBase = getWunderBase();
  if (!wunderBase) {
    throw new Error(t("throughput.error.apiBase"));
  }
  const response = await fetch(`${wunderBase}/admin/throughput/stop`, {
    method: "POST",
  });
  if (!response.ok) {
    const message = await parseErrorMessage(response);
    throw new Error(message || t("common.requestFailed", { status: response.status }));
  }
  return response.json();
};

const buildReportFilename = (report, format) => {
  const runId = report?.summary?.run?.id || "throughput";
  const startedAt = report?.summary?.run?.started_at;
  const timestamp = startedAt
    ? startedAt.replace(/[:T]/g, "-").split(".")[0]
    : new Date().toISOString().replace(/[:T]/g, "-").split(".")[0];
  return `throughput_${timestamp}_${runId}.${format}`;
};

const toCsvValue = (value) => {
  if (value === null || value === undefined) {
    return "";
  }
  const text = String(value);
  if (/[",\n]/.test(text)) {
    return `"${text.replace(/"/g, "\"\"")}"`;
  }
  return text;
};

const buildCsv = (report) => {
  const summary = report?.summary || {};
  const run = summary.run || {};
  const metrics = summary.metrics || {};
  const concurrencyList = resolveRunConcurrencyList(run);
  const maxConcurrency = resolveRunMaxConcurrency(run, concurrencyList);
  const concurrencyText = concurrencyList.join(",");
  const concurrencyCount = concurrencyList.length;
  const questionSet = formatQuestionSetSummary(run);
  const questionCount = Number(run.question_count ?? run.questionCount);
  const legacyQuestions = Array.isArray(run.questions)
    ? run.questions
    : run.question
    ? [run.question]
    : [];
  const questionCountValue =
    Number.isFinite(questionCount) && questionCount > 0 ? questionCount : legacyQuestions.length;
  const columns = [
    "section",
    "run_id",
    "status",
    "started_at",
    "finished_at",
    "max_concurrency",
    "concurrency_list",
    "concurrency_count",
    "user_id_prefix",
    "question_set",
    "question_count",
    "concurrency",
    "elapsed_s",
    "timestamp",
    "total_requests",
    "success_requests",
    "error_requests",
    "rps",
    "avg_latency_ms",
    "p50_latency_ms",
    "p90_latency_ms",
    "p99_latency_ms",
    "total_prefill_speed_tps",
    "single_prefill_speed_tps",
    "total_decode_speed_tps",
    "single_decode_speed_tps",
    "input_tokens",
    "output_tokens",
    "total_tokens",
    "avg_total_tokens",
  ];
  const buildRow = (section, row, elapsed, timestamp) =>
    [
      section,
      run.id || "",
      run.status || "",
      run.started_at || "",
      run.finished_at || "",
      maxConcurrency ?? "",
      concurrencyText,
      concurrencyCount,
      run.user_id_prefix || "",
      questionSet,
      questionCountValue,
      row.concurrency ?? "",
      elapsed ?? "",
      timestamp || "",
      row.total_requests ?? "",
      row.success_requests ?? "",
      row.error_requests ?? "",
      row.rps ?? "",
      row.avg_latency_ms ?? "",
      row.p50_latency_ms ?? "",
      row.p90_latency_ms ?? "",
      row.p99_latency_ms ?? "",
      row.total_prefill_speed_tps ?? "",
      row.single_prefill_speed_tps ?? "",
      row.total_decode_speed_tps ?? "",
      row.single_decode_speed_tps ?? "",
      row.input_tokens ?? "",
      row.output_tokens ?? "",
      row.total_tokens ?? "",
      row.avg_total_tokens ?? "",
    ]
      .map(toCsvValue)
      .join(",");
  const rows = [columns.join(",")];
  rows.push(buildRow("summary", metrics, run.elapsed_s, ""));
  (report?.samples || []).forEach((sample) => {
    rows.push(buildRow("sample", sample, sample.elapsed_s, sample.timestamp));
  });
  return rows.join("\n");
};

const formatSuccessRate = (total, success) => {
  const totalValue = Number(total);
  const successValue = Number(success);
  if (!Number.isFinite(totalValue) || totalValue <= 0 || !Number.isFinite(successValue)) {
    return "-";
  }
  return formatPercent((successValue / totalValue) * 100);
};

const formatTimeoutValue = (value) => {
  const numeric = Number(value);
  if (!Number.isFinite(numeric)) {
    return "-";
  }
  return String(numeric);
};

const pickSpeedSample = (reportSamples, maxConcurrency) => {
  const list = Array.isArray(reportSamples) ? reportSamples : [];
  if (!list.length) {
    return null;
  }
  if (Number.isFinite(maxConcurrency) && maxConcurrency > 0) {
    const match = list.find(
      (sample) => Number(sample.concurrency) === Number(maxConcurrency)
    );
    if (match) {
      return match;
    }
  }
  return list[list.length - 1];
};

const buildLatencyRows = (buckets, totalRequests) => {
  const list = Array.isArray(buckets) ? buckets : [];
  if (!list.length) {
    return { rows: [], total: 0, lastBound: 0 };
  }
  const lastBound = list.reduce((max, bucket) => {
    const bound = Number(bucket?.le_ms);
    return Number.isFinite(bound) ? Math.max(max, bound) : max;
  }, 0);
  const total =
    Number.isFinite(totalRequests) && totalRequests > 0
      ? totalRequests
      : list.reduce((sum, bucket) => sum + Number(bucket?.count || 0), 0);
  const rows = list.map((bucket) => {
    const bound = Number(bucket?.le_ms);
    const label = Number.isFinite(bound)
      ? `<= ${bound} ms`
      : lastBound > 0
      ? `> ${lastBound} ms`
      : "> 0 ms";
    const count = Number(bucket?.count || 0);
    const ratio = total > 0 ? formatPercent((count / total) * 100) : "-";
    return {
      label,
      count: formatCount(count),
      ratio,
    };
  });
  return { rows, total, lastBound };
};

const buildHtmlReport = (report, options = {}) => {
  const summary = report?.summary || {};
  const run = summary.run || {};
  const metrics = summary.metrics || {};
  const errors = Array.isArray(summary.errors) ? summary.errors : [];
  const reportSamples = Array.isArray(report?.samples) ? report.samples : [];
  const concurrencyList = resolveRunConcurrencyList(run);
  const maxConcurrency = resolveRunMaxConcurrency(run, concurrencyList);
  const speedSample = pickSpeedSample(reportSamples, maxConcurrency);
  const speedMetrics = resolveSpeedMetrics(speedSample);
  const latencyBuckets = Array.isArray(metrics.latency_buckets)
    ? metrics.latency_buckets
    : [];
  const totalRequests = Number(metrics.total_requests);
  const { rows: latencyRows } = buildLatencyRows(latencyBuckets, totalRequests);
  const chartImage = String(options.chartImage || "");
  const language = document?.documentElement?.lang || "zh-CN";
  const safe = (value) => escapeHtml(value ?? "-");
  const statusValue = run.status || "";
  const metaRows = [
    { label: t("throughput.report.meta.generatedAt"), value: formatTimestamp(new Date().toISOString()) },
    { label: t("throughput.report.meta.runId"), value: run.id || "-" },
    { label: t("throughput.report.meta.status"), value: resolveStatusLabel(statusValue) },
    { label: t("throughput.report.meta.model"), value: resolveModelName(run, getSelectedModelName()) },
    { label: t("throughput.report.meta.startedAt"), value: formatTimestamp(run.started_at) },
    { label: t("throughput.report.meta.finishedAt"), value: formatTimestamp(run.finished_at) },
    { label: t("throughput.report.meta.elapsed"), value: formatElapsed(run.elapsed_s) },
    { label: t("throughput.report.meta.userPrefix"), value: run.user_id_prefix || "-" },
    {
      label: t("throughput.report.meta.concurrencyList"),
      value: concurrencyList.length ? concurrencyList.join(", ") : "-",
    },
    {
      label: t("throughput.report.meta.maxConcurrency"),
      value: formatCount(
        Number.isFinite(maxConcurrency) && maxConcurrency > 0 ? maxConcurrency : null
      ),
    },
    { label: t("throughput.report.meta.dataset"), value: formatQuestionSetSummary(run) },
    {
      label: t("throughput.report.meta.requestTimeout"),
      value: formatTimeoutValue(run.request_timeout_s),
    },
    {
      label: t("throughput.report.meta.maxTokens"),
      value: formatCount(run.max_tokens),
    },
    {
      label: t("throughput.report.meta.stream"),
      value: run.stream ? t("common.yes") : t("common.no"),
    },
  ];
  const summaryRows = [
    { label: t("throughput.metric.total"), value: formatCount(metrics.total_requests) },
    { label: t("throughput.metric.success"), value: formatCount(metrics.success_requests) },
    { label: t("throughput.metric.error"), value: formatCount(metrics.error_requests) },
    {
      label: t("throughput.history.successRate"),
      value: formatSuccessRate(metrics.total_requests, metrics.success_requests),
    },
    { label: t("throughput.metric.rps"), value: formatRate(metrics.rps) },
    { label: t("throughput.metric.avgLatency"), value: formatLatency(metrics.avg_latency_ms) },
    {
      label: t("throughput.metric.firstTokenLatency"),
      value: formatLatency(metrics.first_token_latency_ms),
    },
    { label: t("throughput.metric.p50"), value: formatLatency(metrics.p50_latency_ms) },
    { label: t("throughput.metric.p90"), value: formatLatency(metrics.p90_latency_ms) },
    { label: t("throughput.metric.p99"), value: formatLatency(metrics.p99_latency_ms) },
    { label: t("throughput.metric.inputTokens"), value: formatTokenCount(metrics.input_tokens) },
    {
      label: t("throughput.metric.outputTokens"),
      value: formatTokenCount(metrics.output_tokens),
    },
    { label: t("throughput.metric.totalTokens"), value: formatTokenCount(metrics.total_tokens) },
    { label: t("throughput.metric.avgTokens"), value: formatTokenCount(metrics.avg_total_tokens) },
    {
      label: t("throughput.metric.totalPrefillSpeed"),
      value: formatTokenRate(speedMetrics?.totalPrefill),
    },
    {
      label: t("throughput.metric.singlePrefillSpeed"),
      value: formatTokenRate(speedMetrics?.singlePrefill),
    },
    {
      label: t("throughput.metric.totalDecodeSpeed"),
      value: formatTokenRate(speedMetrics?.totalDecode),
    },
    {
      label: t("throughput.metric.singleDecodeSpeed"),
      value: formatTokenRate(speedMetrics?.singleDecode),
    },
  ];
  const summaryRowsHtml = summaryRows
    .map((row) => `<tr><td>${safe(row.label)}</td><td>${safe(row.value)}</td></tr>`)
    .join("");
  const metaRowsHtml = metaRows
    .map((row) => `<tr><td>${safe(row.label)}</td><td>${safe(row.value)}</td></tr>`)
    .join("");
  const sampleColumns = [
    { label: t("throughput.chart.axis.concurrency"), value: (sample) => formatCount(sample.concurrency) },
    { label: t("throughput.field.elapsed"), value: (sample) => formatElapsed(sample.elapsed_s) },
    { label: t("throughput.metric.total"), value: (sample) => formatCount(sample.total_requests) },
    {
      label: t("throughput.history.successRate"),
      value: (sample) =>
        formatSuccessRate(sample.total_requests, sample.success_requests),
    },
    { label: t("throughput.metric.rps"), value: (sample) => formatRate(sample.rps) },
    {
      label: t("throughput.metric.avgLatency"),
      value: (sample) => formatLatency(sample.avg_latency_ms),
    },
    { label: t("throughput.metric.p50"), value: (sample) => formatLatency(sample.p50_latency_ms) },
    { label: t("throughput.metric.p90"), value: (sample) => formatLatency(sample.p90_latency_ms) },
    { label: t("throughput.metric.p99"), value: (sample) => formatLatency(sample.p99_latency_ms) },
    {
      label: t("throughput.metric.totalPrefillSpeed"),
      value: (sample) => formatTokenRate(sample.total_prefill_speed_tps),
    },
    {
      label: t("throughput.metric.singlePrefillSpeed"),
      value: (sample) => formatTokenRate(sample.single_prefill_speed_tps),
    },
    {
      label: t("throughput.metric.totalDecodeSpeed"),
      value: (sample) => formatTokenRate(sample.total_decode_speed_tps),
    },
    {
      label: t("throughput.metric.singleDecodeSpeed"),
      value: (sample) => formatTokenRate(sample.single_decode_speed_tps),
    },
    {
      label: t("throughput.metric.inputTokens"),
      value: (sample) => formatTokenCount(sample.input_tokens),
    },
    {
      label: t("throughput.metric.outputTokens"),
      value: (sample) => formatTokenCount(sample.output_tokens),
    },
    { label: t("throughput.metric.totalTokens"), value: (sample) => formatTokenCount(sample.total_tokens) },
    {
      label: t("throughput.metric.avgTokens"),
      value: (sample) => formatTokenCount(sample.avg_total_tokens),
    },
  ];
  const sampleRowsHtml = reportSamples.length
    ? reportSamples
        .map(
          (sample) =>
            `<tr>${sampleColumns
              .map((column) => `<td>${safe(column.value(sample))}</td>`)
              .join("")}</tr>`
        )
        .join("")
    : `<tr><td colspan="${sampleColumns.length}" class="muted">${safe(
        t("common.noData")
      )}</td></tr>`;
  const errorRowsHtml = errors.length
    ? errors
        .map(
          (error) =>
            `<tr><td>${safe(formatTimestamp(error.timestamp))}</td><td>${safe(
              error.message
            )}</td></tr>`
        )
        .join("")
    : `<tr><td colspan="2" class="muted">${safe(t("throughput.errors.empty"))}</td></tr>`;
  const latencyRowsHtml = latencyRows.length
    ? latencyRows
        .map(
          (row) =>
            `<tr><td>${safe(row.label)}</td><td>${safe(row.count)}</td><td>${safe(
              row.ratio
            )}</td></tr>`
        )
        .join("")
    : `<tr><td colspan="3" class="muted">${safe(t("common.noData"))}</td></tr>`;
  const chartHtml = chartImage
    ? `<img class="chart-image" src="${safe(chartImage)}" alt="${safe(
        t("throughput.report.section.curve")
      )}" />`
    : `<div class="muted">${safe(t("common.noData"))}</div>`;
  return `<!DOCTYPE html>
<html lang="${safe(language)}">
  <head>
    <meta charset="utf-8" />
    <title>${safe(t("throughput.report.title"))}</title>
    <style>
      :root { color-scheme: light; }
      body {
        margin: 32px;
        font-family: "Segoe UI", "Noto Sans", "PingFang SC", "Microsoft Yahei", Arial, sans-serif;
        color: #0f172a;
        background: #ffffff;
      }
      h1 { font-size: 24px; margin: 0 0 4px; }
      h2 { font-size: 18px; margin: 24px 0 12px; }
      .subtitle { font-size: 13px; color: #64748b; }
      .report-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 16px; }
      .status-badge {
        display: inline-flex;
        align-items: center;
        padding: 4px 10px;
        border-radius: 999px;
        font-size: 12px;
        font-weight: 600;
        background: #e2e8f0;
        color: #475569;
      }
      .status-badge.running { background: #dbeafe; color: #1d4ed8; }
      .status-badge.stopping { background: #fef9c3; color: #a16207; }
      .status-badge.finished { background: #dcfce7; color: #15803d; }
      .status-badge.stopped { background: #fee2e2; color: #b91c1c; }
      .muted { color: #94a3b8; font-size: 12px; }
      table { width: 100%; border-collapse: collapse; }
      th, td { border: 1px solid #e2e8f0; padding: 8px 10px; font-size: 12px; }
      th { background: #f8fafc; text-align: left; }
      .table-scroll { overflow-x: auto; }
      .section-block { margin-bottom: 20px; }
      .note-list { margin: 0; padding-left: 18px; font-size: 12px; color: #475569; }
      .chart-wrap { border: 1px solid #e2e8f0; border-radius: 8px; padding: 12px; }
      .chart-image { max-width: 100%; height: auto; display: block; }
    </style>
  </head>
  <body>
    <header class="report-header">
      <div>
        <h1>${safe(t("throughput.report.title"))}</h1>
        <div class="subtitle">${safe(t("throughput.report.subtitle"))}</div>
      </div>
      <div class="status-badge ${safe(statusValue)}">${safe(
        resolveStatusLabel(statusValue)
      )}</div>
    </header>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.overview"))}</h2>
      <table>
        <tbody>
          ${metaRowsHtml}
        </tbody>
      </table>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.summary"))}</h2>
      <table>
        <thead>
          <tr>
            <th>${safe(t("throughput.report.table.metric"))}</th>
            <th>${safe(t("throughput.report.table.value"))}</th>
          </tr>
        </thead>
        <tbody>
          ${summaryRowsHtml}
        </tbody>
      </table>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.curve"))}</h2>
      <div class="chart-wrap">
        ${chartHtml}
      </div>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.samples"))}</h2>
      <div class="table-scroll">
        <table>
          <thead>
            <tr>
              ${sampleColumns.map((column) => `<th>${safe(column.label)}</th>`).join("")}
            </tr>
          </thead>
          <tbody>
            ${sampleRowsHtml}
          </tbody>
        </table>
      </div>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.errors"))}</h2>
      <table>
        <thead>
          <tr>
            <th>${safe(t("throughput.errors.time"))}</th>
            <th>${safe(t("throughput.errors.message"))}</th>
          </tr>
        </thead>
        <tbody>
          ${errorRowsHtml}
        </tbody>
      </table>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.latency"))}</h2>
      <table>
        <thead>
          <tr>
            <th>${safe(t("throughput.report.latency.bucket"))}</th>
            <th>${safe(t("throughput.report.latency.count"))}</th>
            <th>${safe(t("throughput.report.latency.ratio"))}</th>
          </tr>
        </thead>
        <tbody>
          ${latencyRowsHtml}
        </tbody>
      </table>
    </section>
    <section class="section-block">
      <h2>${safe(t("throughput.report.section.notes"))}</h2>
      <ul class="note-list">
        <li>${safe(t("throughput.report.note.tokens"))}</li>
      </ul>
    </section>
  </body>
</html>`;
};

const getCurveChartDataUrl = () => {
  const chart = ensureCurveChart();
  if (!chart || typeof chart.getDataURL !== "function") {
    return "";
  }
  try {
    return chart.getDataURL({
      type: "png",
      pixelRatio: 2,
      backgroundColor: "#ffffff",
    });
  } catch (error) {
    return "";
  }
};

const downloadBlob = (blob, filename) => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
};

const handleExport = async () => {
  try {
    setFormStatus(t("throughput.message.exporting"));
    const format = String(elements.throughputExportFormat?.value || "json").toLowerCase();
    const runId = state.runtime.throughputHistoryMode
      ? state.runtime.throughputHistoryRunId
      : "";
    const report = await fetchThroughputReport(runId || undefined);
    let blob;
    let extension = "json";
    if (format === "csv") {
      const csv = buildCsv(report);
      blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
      extension = "csv";
    } else if (format === "html") {
      const chartImage = getCurveChartDataUrl();
      const html = buildHtmlReport(report, { chartImage });
      blob = new Blob([html], { type: "text/html;charset=utf-8" });
      extension = "html";
    } else {
      const json = JSON.stringify(report, null, 2);
      blob = new Blob([json], { type: "application/json;charset=utf-8" });
      extension = "json";
    }
    const filename = buildReportFilename(report, extension);
    downloadBlob(blob, filename);
    setFormStatus(t("throughput.message.exported"));
    notify(t("throughput.message.exported"), "success");
  } catch (error) {
    const message = error?.message || String(error);
    setFormStatus(t("throughput.message.exportFailed", { message }));
    notify(t("throughput.message.exportFailed", { message }), "error");
  }
};

const handleToggle = async () => {
  if (ACTIVE_RUN_STATUSES.has(currentStatus)) {
    await handleStop();
  } else {
    await handleStart();
  }
};

const handleStart = async () => {
  try {
    exitHistoryMode();
    const payload = buildPayload();
    persistConfig();
    setFormStatus(t("throughput.message.starting"));
    const snapshot = await startThroughput(payload);
    renderSnapshot(snapshot, false);
    setFormStatus(t("throughput.message.started"));
    notify(t("throughput.message.started"), "success");
    if (state.runtime.activePanel === "throughput") {
      loadThroughputSessions({ silent: true }).catch(() => {});
    }
    if (state.runtime.activePanel === "throughput") {
      startPolling();
    }
  } catch (error) {
    const message = error?.message || String(error);
    setFormStatus(t("throughput.message.startFailed", { message }));
    notify(t("throughput.message.startFailed", { message }), "error");
  }
};

const handleStop = async () => {
  try {
    exitHistoryMode();
    setFormStatus(t("throughput.message.stopping"));
    const snapshot = await stopThroughput();
    renderSnapshot(snapshot, false);
    setFormStatus(t("throughput.message.stopped"));
    notify(t("throughput.message.stopped"), "info");
    if (state.runtime.activePanel === "throughput") {
      loadThroughputSessions({ silent: true }).catch(() => {});
    }
    if (!ACTIVE_RUN_STATUSES.has(snapshot?.run?.status)) {
      stopPolling();
    }
  } catch (error) {
    const message = error?.message || String(error);
    setFormStatus(t("throughput.message.stopFailed", { message }));
    notify(t("throughput.message.stopFailed", { message }), "error");
  }
};

const handleRefresh = async () => {
  try {
    exitHistoryMode();
    await loadThroughputStatus();
  } catch (error) {
    const message = error?.message || String(error);
    setFormStatus(t("throughput.message.refreshFailed", { message }));
    notify(t("throughput.message.refreshFailed", { message }), "error");
  }
};

const stopPolling = () => {
  if (state.runtime.throughputPollTimer) {
    clearInterval(state.runtime.throughputPollTimer);
    state.runtime.throughputPollTimer = null;
  }
};

const startPolling = () => {
  if (state.runtime.throughputPollTimer) {
    return;
  }
  state.runtime.throughputPollTimer = setInterval(async () => {
    try {
      await loadThroughputStatus({ silent: true });
    } catch (error) {
      // ignore polling errors
    }
  }, APP_CONFIG.monitorPollIntervalMs);
};

const loadThroughputStatus = async (options = {}) => {
  const silent = options.silent === true;
  try {
    const payload = await fetchThroughputStatus();
    const { snapshot, fromHistory } = resolvePrimarySnapshot(payload);
    renderSnapshot(snapshot, fromHistory);
    if (!state.runtime.throughputHistoryMode && snapshot?.run?.id) {
      await loadCurveReport(snapshot.run.id, { silent: true });
    }
    if (snapshot && state.runtime.activePanel === "throughput") {
      loadThroughputSessions({ silent: true }).catch(() => {});
    }
    if (snapshot && ACTIVE_RUN_STATUSES.has(snapshot.run?.status)) {
      if (state.runtime.activePanel === "throughput") {
        startPolling();
      }
    } else {
      stopPolling();
    }
    if (!silent) {
      setFormStatus(t("throughput.message.synced"));
    }
    return snapshot;
  } catch (error) {
    if (!silent) {
      throw error;
    }
    return null;
  }
};

export const toggleThroughputPolling = (enabled) => {
  if (!initialized) {
    return;
  }
  if (state.runtime.throughputHistoryMode) {
    return;
  }
  if (!enabled) {
    stopPolling();
    return;
  }
  loadThroughputStatus({ silent: true }).catch(() => {});
};

export const initThroughputPanel = async () => {
  if (initialized) {
    return;
  }
  initialized = true;
  applyStoredConfig();
  ensureLlmConfigLoaded()
    .then(() => {
      renderThroughputModelOptions();
      syncModelSelectState(null);
    })
    .catch(() => {
      renderThroughputModelOptions();
      syncModelSelectState(null);
    });
  if (elements.throughputToggleBtn) {
    elements.throughputToggleBtn.addEventListener("click", handleToggle);
  }
  if (elements.throughputRefreshBtn) {
    elements.throughputRefreshBtn.addEventListener("click", handleRefresh);
  }
  if (elements.throughputHistoryBtn) {
    elements.throughputHistoryBtn.addEventListener("click", openHistoryModal);
  }
  if (elements.throughputExportBtn) {
    elements.throughputExportBtn.addEventListener("click", handleExport);
  }
  if (elements.throughputHistoryClose) {
    elements.throughputHistoryClose.addEventListener("click", closeHistoryModal);
  }
  if (elements.throughputHistoryCloseBtn) {
    elements.throughputHistoryCloseBtn.addEventListener("click", closeHistoryModal);
  }
  if (elements.throughputHistoryModal) {
    elements.throughputHistoryModal.addEventListener("click", (event) => {
      if (event.target === elements.throughputHistoryModal) {
        closeHistoryModal();
      }
    });
  }
  [
    { button: elements.throughputThreadFilterAll, filter: "all" },
    { button: elements.throughputThreadFilterActive, filter: "active" },
    { button: elements.throughputThreadFilterFinished, filter: "finished" },
    { button: elements.throughputThreadFilterFailed, filter: "failed" },
  ].forEach(({ button, filter }) => {
    if (!button) {
      return;
    }
    button.addEventListener("click", () => setThreadFilter(filter));
  });
  [
    elements.throughputConcurrencyList,
    elements.throughputUserPrefix,
    elements.throughputMaxTokens,
    elements.throughputTimeout,
    elements.throughputModelSelect,
  ].forEach((input) => {
    if (!input) {
      return;
    }
    input.addEventListener("input", scheduleConfigSave);
    input.addEventListener("change", scheduleConfigSave);
  });
  window.addEventListener("wunder:llm-updated", () => {
    renderThroughputModelOptions({ value: getSelectedModelName() });
    updateModelValue(null);
  });
  window.addEventListener("wunder:language-changed", () => {
    renderThroughputModelOptions({ value: getSelectedModelName() });
    updateModelValue(null);
  });
  syncThreadFilterButtons();
  renderThroughputSessions();
  resetCharts();
  await loadThroughputStatus({ silent: true });
};


