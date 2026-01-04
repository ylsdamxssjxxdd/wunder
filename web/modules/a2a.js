import { elements } from "./elements.js?v=20260104-11";
import { state } from "./state.js";
import { parseHeadersValue, normalizeApiBase, formatTimestamp } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-11";
import { notify } from "./notify.js";

const A2A_STATE_KEY = "wunder_a2a_state";
const MAX_LOG_ITEMS = 300;
const STREAM_METHODS = new Set(["SendStreamingMessage", "SubscribeToTask"]);
const a2aLogTimestamps = new WeakMap();
const pendingA2aRequests = [];

const buildEmptyStats = () => ({
  requestId: "",
  method: "",
  endpoint: "",
  stream: false,
  httpStatus: "",
  taskId: "",
  contextId: "",
  status: "",
  eventCount: 0,
  errorCount: 0,
  requestStartMs: null,
  requestEndMs: null,
});

let a2aStats = buildEmptyStats();
const a2aOutputState = {
  rounds: [],
  currentRound: null,
  selectedRound: null,
  userSelectedRound: false,
  rawEvents: [],
};

// 读取本地存储状态，避免刷新丢失输入
const readA2aState = () => {
  try {
    const raw = localStorage.getItem(A2A_STATE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch (error) {
    return {};
  }
};

// 写入本地存储状态
const writeA2aState = (patch) => {
  const next = { ...readA2aState(), ...(patch || {}) };
  try {
    localStorage.setItem(A2A_STATE_KEY, JSON.stringify(next));
  } catch (error) {
    // 忽略浏览器存储异常
  }
  return next;
};

// 根据 /wunder API 基础路径推导 A2A Endpoint
const buildDefaultEndpoint = () => {
  const base = normalizeApiBase(elements.apiBase?.value || "");
  if (!base) {
    return "";
  }
  const stripped = base.replace(/\/wunder\/?$/i, "");
  return `${stripped}/a2a`;
};

const resolveEndpoint = () => {
  const value = String(elements.a2aEndpoint?.value || "").trim();
  if (value) {
    return value;
  }
  return buildDefaultEndpoint();
};

const resolveAgentCardUrl = () => {
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    return "";
  }
  const base = endpoint.replace(/\/a2a\/?$/i, "");
  return `${base}/.well-known/agent-card.json`;
};

// 生成请求 ID，避免重复冲突
const generateRequestId = () => `a2a-${Date.now()}-${Math.random().toString(16).slice(2, 10)}`;

// 格式化 JSON 输出，避免异常导致页面报错
const stringifyPayload = (payload) => {
  if (payload === undefined) {
    return "";
  }
  if (typeof payload === "string") {
    return payload;
  }
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
};

// 控制日志条目数量，避免日志过多导致页面卡顿
const trimLogItems = (container) => {
  if (!container) {
    return;
  }
  while (container.children.length > MAX_LOG_ITEMS) {
    container.removeChild(container.firstChild);
  }
};

// 构建日志详情文本，统一转为可读字符串
const buildDetailText = (detail, fallback) => {
  if (detail === undefined || detail === null) {
    return fallback;
  }
  if (typeof detail === "string") {
    return detail;
  }
  return stringifyPayload(detail);
};

// 解析日志时间戳为显示文本
const resolveLogTimestamp = (value, fallbackMs) => {
  const fallbackTime = Number.isFinite(fallbackMs) ? new Date(fallbackMs) : new Date();
  if (!value) {
    return fallbackTime.toLocaleTimeString();
  }
  if (value instanceof Date) {
    return value.toLocaleTimeString();
  }
  if (typeof value === "number") {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? fallbackTime.toLocaleTimeString() : parsed.toLocaleTimeString();
  }
  if (typeof value === "string") {
    const parsed = new Date(value);
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toLocaleTimeString();
    }
    return value;
  }
  return fallbackTime.toLocaleTimeString();
};

// 解析日志时间戳为毫秒，便于计算耗时
const resolveLogTimestampMs = (value) => {
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
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  return null;
};

// 将耗时格式化为秒级展示
const formatDuration = (durationMs) => {
  if (!Number.isFinite(durationMs)) {
    return "";
  }
  const seconds = Math.max(0, durationMs) / 1000;
  return `${seconds.toFixed(2)}s`;
};

// 在请求日志条目右侧补充耗时标签，保持与事件日志对齐
const appendA2aRequestDurationBadge = (item, durationText) => {
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

// 解析 A2A 事件中的时间戳，提升日志耗时准确度
const resolveA2aEventTimestamp = (payload) => {
  if (!payload || typeof payload !== "object") {
    return null;
  }
  const candidates = [
    payload.timestamp,
    payload.statusUpdate?.status?.timestamp,
    payload.task?.status?.timestamp,
    payload.message?.timestamp,
    payload.artifactUpdate?.metadata?.timestamp,
    payload.artifactUpdate?.artifact?.metadata?.timestamp,
    payload.artifactUpdate?.artifact?.timestamp,
    payload.task?.metadata?.timestamp,
  ];
  for (const value of candidates) {
    if (Number.isFinite(resolveLogTimestampMs(value))) {
      return value;
    }
  }
  return null;
};

// 计算耗时差，首条日志默认显示 0s
const resolveDurationMs = (container, timestampMs, overrideMs) => {
  if (Number.isFinite(overrideMs)) {
    if (Number.isFinite(timestampMs)) {
      a2aLogTimestamps.set(container, timestampMs);
    }
    return overrideMs;
  }
  if (!Number.isFinite(timestampMs)) {
    return null;
  }
  const lastMs = a2aLogTimestamps.get(container);
  a2aLogTimestamps.set(container, timestampMs);
  if (!Number.isFinite(lastMs)) {
    return 0;
  }
  const diff = timestampMs - lastMs;
  return diff >= 0 ? diff : 0;
};

// 追加结构化日志条目，支持点击查看详情
const appendA2aLogItem = (container, title, options = {}) => {
  if (!container) {
    return null;
  }
  const timestampMs = resolveLogTimestampMs(options.timestamp);
  const fallbackMs = Number.isFinite(timestampMs) ? timestampMs : Date.now();
  const timestamp = resolveLogTimestamp(options.timestamp, fallbackMs);
  const showDuration = options.showDuration !== false;
  const durationMs = showDuration
    ? resolveDurationMs(
        container,
        Number.isFinite(timestampMs) ? timestampMs : fallbackMs,
        options.durationMs
      )
    : null;
  const durationText = showDuration ? formatDuration(durationMs) : "";
  const detailText = buildDetailText(options.detail, title);
  const stageText = typeof options.stage === "string" ? options.stage.trim() : "";
  const eventText = typeof options.eventType === "string" ? options.eventType.trim() : "";
  const rightTagText = typeof options.rightTag === "string" ? options.rightTag.trim() : "";
  const rightTagClass =
    typeof options.rightTagClass === "string" ? options.rightTagClass.trim() : "";

  const item = document.createElement("details");
  item.className = "log-item";

  const summary = document.createElement("summary");
  summary.className = "log-summary";

  const timeNode = document.createElement("span");
  timeNode.className = "log-time";
  timeNode.textContent = `[${timestamp}]`;
  summary.appendChild(timeNode);

  if (eventText) {
    const eventNode = document.createElement("span");
    eventNode.className = "log-event";
    eventNode.textContent = eventText;
    summary.appendChild(eventNode);
  }

  if (stageText) {
    const stageNode = document.createElement("span");
    stageNode.className = "log-stage";
    stageNode.textContent = stageText;
    summary.appendChild(stageNode);
  }

  const titleNode = document.createElement("span");
  titleNode.className = "log-title";
  titleNode.textContent = title;
  summary.appendChild(titleNode);

  const rightWrap = document.createElement("span");
  rightWrap.className = "log-right";
  let hasRightItems = false;

  if (rightTagText) {
    const tagNode = document.createElement("span");
    tagNode.className = "log-tag";
    if (rightTagClass) {
      tagNode.classList.add(rightTagClass);
    }
    tagNode.textContent = rightTagText;
    rightWrap.appendChild(tagNode);
    hasRightItems = true;
  }

  if (durationText) {
    const durationNode = document.createElement("span");
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
    hasRightItems = true;
  }

  if (hasRightItems) {
    summary.appendChild(rightWrap);
  }

  const detailNode = document.createElement("div");
  detailNode.className = "log-detail";
  detailNode.textContent = detailText;

  item.appendChild(summary);
  item.appendChild(detailNode);
  container.appendChild(item);
  trimLogItems(container);
  container.scrollTop = container.scrollHeight;
  return item;
};

// 追加 A2A 事件日志条目
const appendA2aEventLog = (title, options = {}) =>
  appendA2aLogItem(elements.a2aEventLog, title, options);

// 追加 A2A 请求日志条目
const appendA2aRequestLog = (title, detail, options = {}) => {
  const timestampValue = options.timestamp;
  const timestampMs = resolveLogTimestampMs(timestampValue);
  const fallbackMs = Number.isFinite(timestampMs) ? timestampMs : Date.now();
  const item = appendA2aLogItem(elements.a2aRequestLog, title, {
    detail,
    showDuration: false,
    ...options,
    timestamp: Number.isFinite(timestampMs) ? timestampValue : fallbackMs,
  });
  if (item) {
    pendingA2aRequests.push({
      item,
      requestTimestampMs: Number.isFinite(timestampMs) ? timestampMs : fallbackMs,
    });
  }
  return item;
};

// 重置日志时间差计数，避免新一轮耗时计算串联
const resetA2aLogTimers = () => {
  if (elements.a2aEventLog) {
    a2aLogTimestamps.delete(elements.a2aEventLog);
  }
  if (elements.a2aRequestLog) {
    a2aLogTimestamps.delete(elements.a2aRequestLog);
  }
};

// 清空正在等待的请求日志状态
const resetPendingA2aRequests = () => {
  pendingA2aRequests.length = 0;
};

// 计算并补充请求耗时展示
const finishA2aRequestLog = (options = {}) => {
  if (!pendingA2aRequests.length) {
    return;
  }
  const entry = pendingA2aRequests.shift();
  if (!entry || !entry.item) {
    return;
  }
  const responseTimestampMs = resolveLogTimestampMs(options.timestamp);
  const endTimestampMs = Number.isFinite(responseTimestampMs) ? responseTimestampMs : Date.now();
  const durationText = formatDurationSeconds(entry.requestTimestampMs, endTimestampMs);
  appendA2aRequestDurationBadge(entry.item, durationText);
};

// 统一更新模型输出区域文本
const updateModelOutputText = (text) => {
  if (elements.a2aModelOutputText) {
    elements.a2aModelOutputText.textContent = text || "";
    return;
  }
  if (elements.a2aModelOutput) {
    elements.a2aModelOutput.textContent = text || "";
  }
};

// 重置模型输出缓存与展示
const resetA2aOutput = (options = {}) => {
  const resetRounds = options.resetRounds !== false;
  if (resetRounds) {
    a2aOutputState.rounds = [];
    a2aOutputState.currentRound = null;
    a2aOutputState.selectedRound = null;
    a2aOutputState.userSelectedRound = false;
    renderA2aRoundSelectOptions();
  }
  a2aOutputState.rawEvents = [];
  updateModelOutputText("");
};

const normalizeSessionIdValue = (value) => {
  const trimmed = String(value || "").trim();
  if (!trimmed) {
    return "";
  }
  return trimmed.startsWith("tasks/") ? trimmed.slice("tasks/".length) : trimmed;
};

const resolveSessionIdPair = (taskId, contextId) => {
  const normalizedTaskId = normalizeSessionIdValue(taskId);
  const normalizedContextId = normalizeSessionIdValue(contextId);
  if (!normalizedTaskId && !normalizedContextId) {
    return { taskId: "", contextId: "" };
  }
  return {
    taskId: normalizedTaskId || normalizedContextId,
    contextId: normalizedContextId || normalizedTaskId,
  };
};

const readSessionInputs = () => ({
  taskId: normalizeSessionIdValue(elements.a2aTaskId?.value || ""),
  contextId: normalizeSessionIdValue(elements.a2aContextId?.value || ""),
});

const clearSessionState = () => {
  state.runtime.a2aTaskId = "";
  state.runtime.a2aContextId = "";
  if (elements.a2aTaskId) {
    elements.a2aTaskId.value = "";
  }
  if (elements.a2aContextId) {
    elements.a2aContextId.value = "";
  }
  writeA2aState({ taskId: "", contextId: "" });
};

const updateSessionState = (taskId, contextId) => {
  const next = resolveSessionIdPair(taskId, contextId);
  if (!next.taskId && !next.contextId) {
    return false;
  }
  let changed = false;
  if (state.runtime.a2aTaskId !== next.taskId) {
    state.runtime.a2aTaskId = next.taskId;
    changed = true;
  }
  if (state.runtime.a2aContextId !== next.contextId) {
    state.runtime.a2aContextId = next.contextId;
    changed = true;
  }
  if (elements.a2aTaskId && elements.a2aTaskId.value !== next.taskId) {
    elements.a2aTaskId.value = next.taskId;
    changed = true;
  }
  if (elements.a2aContextId && elements.a2aContextId.value !== next.contextId) {
    elements.a2aContextId.value = next.contextId;
    changed = true;
  }
  if (changed) {
    writeA2aState({ taskId: next.taskId, contextId: next.contextId });
  }
  return changed;
};

const extractSessionIds = (payload) => {
  if (!payload || typeof payload !== "object") {
    return {};
  }
  if (Array.isArray(payload.tasks)) {
    return {};
  }
  let taskId =
    payload.task?.id ||
    payload.statusUpdate?.taskId ||
    payload.message?.taskId ||
    payload.taskId ||
    "";
  let contextId =
    payload.task?.contextId ||
    payload.statusUpdate?.contextId ||
    payload.message?.contextId ||
    payload.contextId ||
    "";
  if (!taskId && typeof payload.id === "string" && payload.contextId) {
    taskId = payload.id;
  }
  return { taskId, contextId };
};

const applySessionFromPayload = (payload) => {
  const { taskId, contextId } = extractSessionIds(payload);
  updateSessionState(taskId, contextId);
};

// 构造轮次条目
const buildA2aRoundEntry = (roundId, timestamp) => ({
  id: roundId,
  timestamp,
  timeText: new Date(timestamp).toLocaleTimeString(),
  messages: [],
});

// 获取下一轮编号
const getNextA2aRoundId = () => {
  if (!a2aOutputState.rounds.length) {
    return 1;
  }
  const lastId = a2aOutputState.rounds[a2aOutputState.rounds.length - 1].id;
  return Number.isFinite(lastId) ? lastId + 1 : a2aOutputState.rounds.length + 1;
};

// 查找指定轮次条目
const findA2aRoundEntry = (roundId) => {
  if (!Number.isFinite(roundId)) {
    return null;
  }
  return a2aOutputState.rounds.find((entry) => entry.id === roundId) || null;
};

// 渲染轮次选择下拉
const renderA2aRoundSelectOptions = () => {
  if (!elements.a2aOutputRoundSelect) {
    return;
  }
  const select = elements.a2aOutputRoundSelect;
  select.textContent = "";
  if (!a2aOutputState.rounds.length) {
    const emptyOption = document.createElement("option");
    emptyOption.value = "";
    emptyOption.textContent = t("debug.round.empty");
    select.appendChild(emptyOption);
    select.value = "";
    return;
  }
  if (
    !Number.isFinite(a2aOutputState.selectedRound) ||
    !findA2aRoundEntry(a2aOutputState.selectedRound)
  ) {
    a2aOutputState.selectedRound = a2aOutputState.rounds[a2aOutputState.rounds.length - 1].id;
  }
  a2aOutputState.rounds.forEach((entry) => {
    const option = document.createElement("option");
    option.value = String(entry.id);
    option.textContent = entry.timeText
      ? t("debug.round.labelWithTime", { id: entry.id, time: entry.timeText })
      : t("debug.round.label", { id: entry.id });
    select.appendChild(option);
  });
  select.value = String(a2aOutputState.selectedRound || "");
};

// 选中指定轮次
const selectA2aRound = (roundId, options = {}) => {
  const entry = findA2aRoundEntry(roundId);
  if (!entry) {
    return;
  }
  a2aOutputState.selectedRound = entry.id;
  if (options.manual) {
    a2aOutputState.userSelectedRound = true;
  }
  if (options.auto) {
    a2aOutputState.userSelectedRound = false;
  }
  renderA2aRoundSelectOptions();
  renderA2aModelOutput();
};

// 确保存在当前轮次条目
const ensureA2aRoundEntry = (options = {}) => {
  const entry =
    findA2aRoundEntry(a2aOutputState.currentRound) ||
    (() => {
      const roundId = getNextA2aRoundId();
      const created = buildA2aRoundEntry(roundId, Date.now());
      a2aOutputState.rounds.push(created);
      a2aOutputState.currentRound = roundId;
      return created;
    })();
  if (options.resetUserSelection) {
    a2aOutputState.userSelectedRound = false;
  }
  if (options.forceSelect || !a2aOutputState.userSelectedRound) {
    selectA2aRound(entry.id, { auto: true });
  } else {
    renderA2aRoundSelectOptions();
  }
  return entry;
};

// 创建新轮次并默认切换
const startA2aRound = () => {
  const roundId = getNextA2aRoundId();
  const entry = buildA2aRoundEntry(roundId, Date.now());
  a2aOutputState.rounds.push(entry);
  a2aOutputState.currentRound = roundId;
  a2aOutputState.selectedRound = roundId;
  a2aOutputState.userSelectedRound = false;
  renderA2aRoundSelectOptions();
  renderA2aModelOutput();
  return entry;
};

// 根据缓存内容渲染模型输出
const resolveModelOutputText = () => {
  const entry =
    findA2aRoundEntry(a2aOutputState.selectedRound) ||
    findA2aRoundEntry(a2aOutputState.currentRound);
  if (!entry) {
    return "";
  }
  return entry.messages.join("\n");
};

const renderA2aModelOutput = () => {
  updateModelOutputText(resolveModelOutputText());
};

// 统计耗时展示
const formatDurationSeconds = (startMs, endMs) => {
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs < startMs) {
    return "-";
  }
  return `${((endMs - startMs) / 1000).toFixed(2)}s`;
};

// 渲染 A2A 统计信息
const renderA2aStats = () => {
  if (!elements.a2aStats) {
    return;
  }
  const hasStats =
    a2aStats.requestId ||
    a2aStats.method ||
    a2aStats.endpoint ||
    Number.isFinite(a2aStats.requestStartMs);
  if (!hasStats) {
    elements.a2aStats.textContent = t("a2a.stats.empty");
    return;
  }
  const endMs = state.runtime.a2aStreaming ? Date.now() : a2aStats.requestEndMs;
  const durationText = formatDurationSeconds(a2aStats.requestStartMs, endMs);
  const rows = [
    { label: t("a2a.stats.requestId"), value: a2aStats.requestId || "-" },
    { label: t("a2a.stats.method"), value: a2aStats.method || "-" },
    { label: t("a2a.stats.endpoint"), value: a2aStats.endpoint || "-" },
    { label: t("a2a.stats.stream"), value: a2aStats.stream ? t("common.yes") : t("common.no") },
    { label: t("a2a.stats.httpStatus"), value: a2aStats.httpStatus || "-" },
    { label: t("a2a.stats.taskId"), value: a2aStats.taskId || "-" },
    { label: t("a2a.stats.contextId"), value: a2aStats.contextId || "-" },
    { label: t("a2a.stats.status"), value: a2aStats.status || "-" },
    { label: t("a2a.stats.events"), value: String(a2aStats.eventCount || 0) },
    { label: t("a2a.stats.errorCount"), value: String(a2aStats.errorCount || 0) },
    { label: t("a2a.stats.duration"), value: durationText },
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

  elements.a2aStats.textContent = "";
  elements.a2aStats.appendChild(table);
};

// 初始化统计信息
const resetA2aStats = () => {
  a2aStats = buildEmptyStats();
  renderA2aStats();
};

// 启动新请求统计
const startA2aStats = ({ requestId, method, endpoint, stream } = {}) => {
  a2aStats = {
    ...buildEmptyStats(),
    requestId: requestId || "",
    method: method || "",
    endpoint: endpoint || "",
    stream: Boolean(stream),
    requestStartMs: Date.now(),
  };
  renderA2aStats();
};

// 更新统计结束时间
const finishA2aStats = () => {
  if (!Number.isFinite(a2aStats.requestStartMs)) {
    return;
  }
  a2aStats.requestEndMs = Date.now();
  renderA2aStats();
};

// 同步统计中的状态字段
const applyA2aStatsFromPayload = (payload) => {
  if (!payload || typeof payload !== "object") {
    return;
  }
  const { taskId, contextId } = extractSessionIds(payload);
  if (taskId) {
    a2aStats.taskId = taskId;
  }
  if (contextId) {
    a2aStats.contextId = contextId;
  }
  const statusValue =
    payload.task?.status?.state ||
    payload.statusUpdate?.status?.state ||
    payload.task?.status ||
    payload.statusUpdate?.status;
  if (statusValue) {
    if (typeof statusValue === "string" || typeof statusValue === "number") {
      a2aStats.status = String(statusValue);
    } else if (typeof statusValue === "object") {
      a2aStats.status = stringifyPayload(statusValue);
    }
  }
  renderA2aStats();
};

// 累计事件数量并刷新统计
const bumpA2aEventCount = () => {
  a2aStats.eventCount += 1;
  renderA2aStats();
};

// 累计错误数量并刷新统计
const bumpA2aErrorCount = () => {
  a2aStats.errorCount += 1;
  renderA2aStats();
};

const clearLogs = () => {
  if (elements.a2aEventLog) {
    elements.a2aEventLog.innerHTML = "";
  }
  if (elements.a2aRequestLog) {
    elements.a2aRequestLog.innerHTML = "";
  }
  resetPendingA2aRequests();
  resetA2aLogTimers();
  resetA2aOutput();
  resetA2aStats();
};

// 统一更新 A2A 日志等待状态
const setA2aLogWaiting = (waiting) => {
  [
    { card: elements.a2aEventCard, log: elements.a2aEventLog },
    { card: elements.a2aRequestCard, log: elements.a2aRequestLog },
  ].forEach(({ card, log }) => {
    const target = card || log?.closest?.(".log-card");
    if (target) {
      target.classList.toggle("is-waiting", waiting);
    }
    if (log) {
      log.setAttribute("aria-busy", waiting ? "true" : "false");
    }
  });
};

// 控制发送/停止按钮状态
const setA2aSendToggleState = (active) => {
  if (!elements.a2aSendBtn) {
    return;
  }
  const isStop = Boolean(active);
  const icon = elements.a2aSendBtn.querySelector("i");
  if (icon) {
    icon.className = isStop ? "fa-solid fa-stop" : "fa-solid fa-paper-plane";
  }
  elements.a2aSendBtn.classList.toggle("danger", isStop);
  const label = isStop ? t("a2a.action.stop") : t("a2a.action.send");
  elements.a2aSendBtn.setAttribute("aria-label", label);
  elements.a2aSendBtn.title = label;
};

const setStreamingState = (active) => {
  state.runtime.a2aStreaming = active;
  setA2aLogWaiting(active);
  setA2aSendToggleState(active);
};

// 打开连接设置弹窗，确保默认 Endpoint 已同步
const openA2aConnectionModal = () => {
  if (!elements.a2aConnectionModal) {
    return;
  }
  syncEndpointDefault();
  elements.a2aConnectionModal.classList.add("active");
  elements.a2aEndpoint?.focus();
};

// 关闭连接设置弹窗
const closeA2aConnectionModal = () => {
  elements.a2aConnectionModal?.classList.remove("active");
};

// 打开高级设置弹窗
const openA2aAdvancedModal = () => {
  if (!elements.a2aAdvancedModal) {
    return;
  }
  elements.a2aAdvancedModal.classList.add("active");
};

// 关闭高级设置弹窗
const closeA2aAdvancedModal = () => {
  elements.a2aAdvancedModal?.classList.remove("active");
};

// 读取 AgentCard 字段，兼容不同命名风格
const readAgentCardValue = (card, keys) => {
  for (const key of keys) {
    if (card && Object.prototype.hasOwnProperty.call(card, key)) {
      return card[key];
    }
  }
  return undefined;
};

// 格式化 AgentCard 字段输出
const formatAgentCardValue = (value) => {
  if (value === undefined || value === null) {
    return "-";
  }
  if (typeof value === "boolean") {
    return value ? t("common.yes") : t("common.no");
  }
  if (Array.isArray(value)) {
    return value.filter((item) => item !== undefined && item !== null).join(", ") || "-";
  }
  if (typeof value === "object") {
    return stringifyPayload(value);
  }
  const text = String(value).trim();
  return text ? text : "-";
};

// 渲染 AgentCard 表格
const renderAgentCardTable = (container, rows) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!rows.length) {
    const empty = document.createElement("div");
    empty.className = "agentcard-empty";
    empty.textContent = t("common.noData");
    container.appendChild(empty);
    return;
  }
  const table = document.createElement("table");
  table.className = "agentcard-kv-table";
  const tbody = document.createElement("tbody");
  rows.forEach((row) => {
    const tr = document.createElement("tr");
    const th = document.createElement("th");
    th.textContent = row.label;
    const td = document.createElement("td");
    td.textContent = formatAgentCardValue(row.value);
    tr.appendChild(th);
    tr.appendChild(td);
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  container.appendChild(table);
};

// 渲染列表空态
const renderAgentCardEmpty = (container) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  const empty = document.createElement("div");
  empty.className = "agentcard-empty";
  empty.textContent = t("common.noData");
  container.appendChild(empty);
};

// 渲染接口列表
const renderAgentCardInterfaces = (container, interfaces) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!Array.isArray(interfaces) || !interfaces.length) {
    renderAgentCardEmpty(container);
    return;
  }
  interfaces.forEach((item) => {
    const card = document.createElement("div");
    card.className = "agentcard-item";
    const summary = document.createElement("div");
    summary.className = "agentcard-item-title";
    const protocol = String(item?.protocolBinding || item?.protocol_binding || "").trim();
    const url = String(item?.url || "").trim();
    summary.textContent = protocol ? `${protocol} · ${url}` : url || "-";
    card.appendChild(summary);
    container.appendChild(card);
  });
};

// 渲染技能列表
const renderAgentCardSkills = (container, skills) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!Array.isArray(skills) || !skills.length) {
    renderAgentCardEmpty(container);
    return;
  }
  skills.forEach((skill) => {
    const item = document.createElement("details");
    item.className = "agentcard-item";
    const summary = document.createElement("summary");
    summary.textContent = String(skill?.name || skill?.id || "-");
    item.appendChild(summary);
    const desc = document.createElement("div");
    desc.className = "agentcard-item-desc";
    desc.textContent = String(skill?.description || "");
    item.appendChild(desc);
    const metaParts = [];
    if (Array.isArray(skill?.tags) && skill.tags.length) {
      metaParts.push(`${t("a2a.agentCard.skill.tags")}: ${skill.tags.join(", ")}`);
    }
    if (Array.isArray(skill?.examples) && skill.examples.length) {
      metaParts.push(`${t("a2a.agentCard.skill.examples")}: ${skill.examples.join(" | ")}`);
    }
    if (Array.isArray(skill?.inputModes) && skill.inputModes.length) {
      metaParts.push(`${t("a2a.agentCard.skill.inputModes")}: ${skill.inputModes.join(", ")}`);
    }
    if (Array.isArray(skill?.outputModes) && skill.outputModes.length) {
      metaParts.push(`${t("a2a.agentCard.skill.outputModes")}: ${skill.outputModes.join(", ")}`);
    }
    if (metaParts.length) {
      const meta = document.createElement("div");
      meta.className = "agentcard-item-meta";
      meta.textContent = metaParts.join(" · ");
      item.appendChild(meta);
    }
    container.appendChild(item);
  });
};

// 渲染工具分组
const renderAgentCardTools = (container, tooling) => {
  if (!container) {
    return;
  }
  container.textContent = "";
  if (!tooling || typeof tooling !== "object") {
    renderAgentCardEmpty(container);
    return;
  }
  const groups = [
    { key: "builtin", label: t("a2a.agentCard.group.builtin") },
    { key: "mcp", label: t("a2a.agentCard.group.mcp") },
    { key: "a2a", label: t("a2a.agentCard.group.a2a") },
    { key: "knowledge", label: t("a2a.agentCard.group.knowledge") },
  ];
  let hasAny = false;
  groups.forEach((group) => {
    const items = Array.isArray(tooling[group.key]) ? tooling[group.key] : [];
    if (!items.length) {
      return;
    }
    hasAny = true;
    const groupWrap = document.createElement("div");
    const title = document.createElement("div");
    title.className = "agentcard-tool-group-title";
    title.textContent = `${group.label} (${items.length})`;
    groupWrap.appendChild(title);
    const list = document.createElement("div");
    list.className = "agentcard-list";
    items.forEach((tool) => {
      const item = document.createElement("details");
      item.className = "agentcard-item";
      const summary = document.createElement("summary");
      const name = String(tool?.tool || tool?.name || "-");
      const server = String(tool?.server || "").trim();
      summary.textContent = server ? `${server}@${name}` : name;
      item.appendChild(summary);
      const desc = document.createElement("div");
      desc.className = "agentcard-item-desc";
      desc.textContent = String(tool?.description || "");
      item.appendChild(desc);
      if (tool?.argsSchema) {
        const schema = document.createElement("pre");
        schema.className = "agentcard-item-schema";
        schema.textContent = stringifyPayload(tool.argsSchema);
        item.appendChild(schema);
      }
      list.appendChild(item);
    });
    groupWrap.appendChild(list);
    container.appendChild(groupWrap);
  });
  if (!hasAny) {
    renderAgentCardEmpty(container);
  }
};

// 渲染 AgentCard 弹窗内容
const renderAgentCardModal = (card) => {
  if (!card || typeof card !== "object") {
    return;
  }
  const protocolVersion = readAgentCardValue(card, ["protocolVersion", "protocol_version"]);
  const version = readAgentCardValue(card, ["version"]);
  const name = readAgentCardValue(card, ["name"]);
  const description = readAgentCardValue(card, ["description"]);
  const provider = readAgentCardValue(card, ["provider"]);
  const documentationUrl = readAgentCardValue(card, ["documentationUrl", "documentation_url"]);
  const inputModes = readAgentCardValue(card, ["defaultInputModes", "default_input_modes"]);
  const outputModes = readAgentCardValue(card, ["defaultOutputModes", "default_output_modes"]);
  const supportedInterfaces = readAgentCardValue(card, [
    "supportedInterfaces",
    "supported_interfaces",
  ]);
  const capabilities = readAgentCardValue(card, ["capabilities"]) || {};
  const supportsExtended = readAgentCardValue(card, ["supportsExtendedAgentCard"]);
  const skills = readAgentCardValue(card, ["skills"]);
  const tooling =
    readAgentCardValue(card, ["tooling"]) ||
    readAgentCardValue(card, ["tools"]) ||
    capabilities?.extensions?.find?.((item) => item?.uri === "wunder.tools")?.params ||
    null;

  if (elements.a2aAgentCardTitle) {
    elements.a2aAgentCardTitle.textContent = t("a2a.agentCard.title");
  }
  if (elements.a2aAgentCardName) {
    elements.a2aAgentCardName.textContent = name ? String(name) : "-";
  }
  if (elements.a2aAgentCardDescription) {
    elements.a2aAgentCardDescription.textContent = description ? String(description) : "-";
  }

  const providerText =
    provider && typeof provider === "object"
      ? `${provider.organization || ""} ${provider.url || ""}`.trim()
      : formatAgentCardValue(provider);

  renderAgentCardTable(elements.a2aAgentCardBasic, [
    { label: t("a2a.agentCard.field.protocolVersion"), value: protocolVersion },
    { label: t("a2a.agentCard.field.version"), value: version },
    { label: t("a2a.agentCard.field.provider"), value: providerText },
    { label: t("a2a.agentCard.field.documentation"), value: documentationUrl },
    { label: t("a2a.agentCard.field.inputModes"), value: inputModes },
    { label: t("a2a.agentCard.field.outputModes"), value: outputModes },
  ]);

  renderAgentCardTable(elements.a2aAgentCardCapabilities, [
    {
      label: t("a2a.agentCard.field.streaming"),
      value: readAgentCardValue(capabilities, ["streaming"]),
    },
    {
      label: t("a2a.agentCard.field.pushNotifications"),
      value: readAgentCardValue(capabilities, ["pushNotifications", "push_notifications"]),
    },
    {
      label: t("a2a.agentCard.field.stateTransitionHistory"),
      value: readAgentCardValue(capabilities, ["stateTransitionHistory", "state_transition_history"]),
    },
    {
      label: t("a2a.agentCard.field.supportsExtended"),
      value: supportsExtended,
    },
  ]);

  renderAgentCardInterfaces(elements.a2aAgentCardInterfaces, supportedInterfaces);
  renderAgentCardSkills(elements.a2aAgentCardSkills, skills);
  renderAgentCardTools(elements.a2aAgentCardTools, tooling);
};

// 打开 AgentCard 弹窗
const openAgentCardModal = (card) => {
  if (!elements.a2aAgentCardModal) {
    return;
  }
  renderAgentCardModal(card);
  elements.a2aAgentCardModal.classList.add("active");
};

// 关闭 AgentCard 弹窗
const closeAgentCardModal = () => {
  elements.a2aAgentCardModal?.classList.remove("active");
};

const syncEndpointDefault = () => {
  if (!elements.a2aEndpoint) {
    return;
  }
  const isManual = elements.a2aEndpoint.dataset.manual === "true";
  if (isManual) {
    return;
  }
  const next = buildDefaultEndpoint();
  if (next && elements.a2aEndpoint.value !== next) {
    elements.a2aEndpoint.value = next;
  }
};

const updateHeaderError = (message) => {
  if (elements.a2aHeadersError) {
    elements.a2aHeadersError.textContent = message || "";
  }
};

const resolveAuthKey = () => {
  const useGlobal = Boolean(elements.a2aUseGlobalKey?.checked);
  if (useGlobal) {
    return String(elements.apiKey?.value || "").trim();
  }
  return String(elements.a2aApiKey?.value || "").trim();
};

// 根据输入构造请求头
const buildHeaders = (streaming) => {
  const headers = new Headers();
  headers.set("Content-Type", "application/json");
  if (streaming) {
    headers.set("Accept", "text/event-stream");
  }
  const version = String(elements.a2aVersion?.value || "").trim();
  if (version) {
    headers.set("A2A-Version", version);
  }
  const { headers: extraHeaders, error } = parseHeadersValue(elements.a2aHeaders?.value || "");
  updateHeaderError(error);
  if (extraHeaders) {
    Object.entries(extraHeaders).forEach(([key, value]) => {
      headers.set(key, String(value));
    });
  }

  const authType = String(elements.a2aAuthType?.value || "apiKey");
  const useGlobal = Boolean(elements.a2aUseGlobalKey?.checked);
  const apiKey = resolveAuthKey();
  if (authType === "none") {
    headers.set("X-API-Key", "");
    return headers;
  }
  if (authType === "bearer" && apiKey) {
    headers.set("Authorization", `Bearer ${apiKey}`);
  } else if (authType === "apiKey" && apiKey) {
    headers.set("X-API-Key", apiKey);
  } else if (!useGlobal) {
    // 禁用全局 API Key 注入，避免误带鉴权头
    headers.set("X-API-Key", "");
  }
  return headers;
};

// 将 Headers 转为对象，便于日志展示
const headersToObject = (headers) => {
  const output = {};
  if (!headers) {
    return output;
  }
  headers.forEach((value, key) => {
    output[key] = value;
  });
  return output;
};

const normalizeTaskName = (taskId) => {
  const trimmed = String(taskId || "").trim();
  if (!trimmed) {
    return "";
  }
  if (trimmed.startsWith("tasks/")) {
    return trimmed;
  }
  return `tasks/${trimmed}`;
};

const splitToolNames = (raw) => {
  const items = String(raw || "")
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item);
  return items.length ? items : null;
};

// 从 Part 列表中提取文本内容
const extractTextParts = (parts) => {
  if (!Array.isArray(parts)) {
    return "";
  }
  const texts = parts
    .map((part) => (typeof part?.text === "string" ? part.text : ""))
    .filter((text) => text);
  return texts.join("");
};

// 提取消息内容文本，忽略 user 消息避免重复回显
const extractMessageText = (message) => {
  if (!message || typeof message !== "object") {
    return "";
  }
  const role = String(message.role || "").trim().toLowerCase();
  if (role === "user") {
    return "";
  }
  const text = extractTextParts(message.parts || []);
  if (text) {
    return text.trim();
  }
  if (typeof message.content === "string" && message.content) {
    return String(message.content).trim();
  }
  if (typeof message.text === "string" && message.text) {
    return String(message.text).trim();
  }
  return "";
};

// 提取 Artifact 中的文本内容
const extractArtifactText = (artifact) => {
  if (!artifact || typeof artifact !== "object") {
    return "";
  }
  const text = extractTextParts(artifact.parts || []);
  return text.trim();
};

// 追加模型输出文本并刷新显示
const appendOutputText = (text) => {
  const value = String(text || "").trim();
  if (!value) {
    return;
  }
  const entry = ensureA2aRoundEntry();
  entry.messages.push(value);
  renderA2aModelOutput();
};

// 覆盖模型输出文本
const setOutputText = (text) => {
  const value = String(text || "").trim();
  const entry = ensureA2aRoundEntry();
  entry.messages = value ? [value] : [];
  renderA2aModelOutput();
};

// 追加模型输出中的消息文本
const appendOutputMessage = (message) => {
  const text = extractMessageText(message);
  if (!text) {
    return;
  }
  appendOutputText(text);
};

// 追加模型输出中的 Artifact 文本
const appendOutputArtifact = (artifact) => {
  const text = extractArtifactText(artifact);
  if (!text) {
    return;
  }
  appendOutputText(text);
};

const resolveUserId = () => {
  const userId = String(elements.a2aUserId?.value || "").trim();
  if (userId) {
    return userId;
  }
  return String(elements.userId?.value || "").trim() || "a2a";
};

// 根据表单生成 params，允许被 Params JSON 覆盖
const buildParamsFromForm = (method) => {
  if (method === "SendMessage" || method === "SendStreamingMessage") {
    const messageText = String(elements.a2aMessage?.value || "").trim();
    if (!messageText) {
      throw new Error(t("a2a.error.messageRequired"));
    }
    const message = {
      role: "user",
      parts: [{ text: messageText }],
    };
    const taskId = String(elements.a2aTaskId?.value || "").trim();
    const contextId = String(elements.a2aContextId?.value || "").trim();
    if (taskId) {
      message.taskId = taskId.startsWith("tasks/") ? taskId.replace("tasks/", "") : taskId;
    }
    if (contextId) {
      message.contextId = contextId;
    }
    const params = {
      userId: resolveUserId(),
      message,
    };
    const configuration = {};
    const historyLength = String(elements.a2aHistoryLength?.value || "").trim();
    if (historyLength) {
      configuration.historyLength = Number(historyLength);
    }
    if (elements.a2aBlocking?.checked) {
      configuration.blocking = true;
    }
    if (Object.keys(configuration).length) {
      params.configuration = configuration;
    }
    const toolNames = splitToolNames(elements.a2aToolNames?.value || "");
    if (toolNames) {
      params.toolNames = toolNames;
    }
    const modelName = String(elements.a2aModelName?.value || "").trim();
    if (modelName) {
      params.modelName = modelName;
    }
    return params;
  }

  if (method === "GetTask") {
    return {
      name: normalizeTaskName(elements.a2aTaskId?.value || ""),
      historyLength: Number(elements.a2aHistoryLength?.value || 0) || undefined,
    };
  }

  if (method === "ListTasks") {
    const params = {};
    const contextId = String(elements.a2aContextId?.value || "").trim();
    if (contextId) {
      params.contextId = contextId;
    }
    const status = String(elements.a2aStatusFilter?.value || "").trim();
    if (status) {
      params.status = status;
    }
    const pageSize = Number(elements.a2aPageSize?.value || 0);
    if (Number.isFinite(pageSize) && pageSize > 0) {
      params.pageSize = pageSize;
    }
    const pageToken = String(elements.a2aPageToken?.value || "").trim();
    if (pageToken) {
      params.pageToken = pageToken;
    }
    const historyLength = Number(elements.a2aHistoryLength?.value || 0);
    if (Number.isFinite(historyLength) && historyLength > 0) {
      params.historyLength = historyLength;
    }
    if (elements.a2aIncludeArtifacts?.checked) {
      params.includeArtifacts = true;
    }
    return params;
  }

  if (method === "CancelTask" || method === "SubscribeToTask") {
    return {
      name: normalizeTaskName(elements.a2aTaskId?.value || ""),
    };
  }

  return {};
};

const buildJsonRpcPayload = () => {
  const method = String(elements.a2aMethod?.value || "").trim() || "SendMessage";
  const paramsJson = String(elements.a2aParamsJson?.value || "").trim();
  let params = null;
  if (paramsJson) {
    try {
      params = JSON.parse(paramsJson);
    } catch (error) {
      throw new Error(t("a2a.error.paramsJson"));
    }
  }
  if (!params) {
    params = buildParamsFromForm(method);
  }
  const requestId = String(elements.a2aRequestId?.value || "").trim() || generateRequestId();
  return {
    jsonrpc: "2.0",
    id: requestId,
    method,
    params,
  };
};

// 从 Task 或结果中提取模型输出文本
const collectOutputTexts = (result) => {
  if (!result || typeof result !== "object") {
    return [];
  }
  const outputs = [];
  if (result.message) {
    const text = extractMessageText(result.message);
    if (text) {
      outputs.push(text);
    }
  }
  if (Array.isArray(result.messages)) {
    result.messages.forEach((item) => {
      const text = extractMessageText(item);
      if (text) {
        outputs.push(text);
      }
    });
  }
  const task = result.task;
  if (task && typeof task === "object") {
    if (Array.isArray(task.artifacts)) {
      task.artifacts.forEach((artifact) => {
        const text = extractArtifactText(artifact);
        if (text) {
          outputs.push(text);
        }
      });
    }
    if (!outputs.length && Array.isArray(task.history)) {
      const history = [...task.history].reverse();
      for (const entry of history) {
        const text = extractMessageText(entry);
        if (text) {
          outputs.push(text);
          break;
        }
      }
    }
  }
  return outputs;
};

// 应用模型输出文本，返回是否成功写入
const applyOutputFromResult = (result) => {
  const outputs = collectOutputTexts(result);
  if (!outputs.length) {
    return false;
  }
  setOutputText(outputs.join("\n"));
  return true;
};

const resolveHistoryTaskId = (task) => {
  const value = task?.id || task?.taskId || task?.name || "";
  return String(value || "").trim();
};

const resolveHistoryTaskStatus = (task) => {
  const value = task?.status?.state || task?.status?.status || task?.status || "";
  return String(value || "").trim();
};

const resolveHistoryTaskTimestamp = (task) =>
  task?.status?.timestamp || task?.status?.message?.timestamp || task?.metadata?.timestamp || "";

const resolveHistoryTaskTitle = (task) => {
  const summary = extractMessageText(task?.status?.message);
  if (summary) {
    return summary;
  }
  const taskId = resolveHistoryTaskId(task);
  return taskId ? `task ${taskId}` : "-";
};

const sortHistoryTasksByTime = (tasks) =>
  [...tasks].sort((a, b) => {
    const aTime = resolveLogTimestampMs(resolveHistoryTaskTimestamp(a)) || 0;
    const bTime = resolveLogTimestampMs(resolveHistoryTaskTimestamp(b)) || 0;
    return bTime - aTime;
  });

const updateA2aHistoryMeta = (tasks) => {
  if (!elements.a2aHistoryMeta) {
    return;
  }
  const count = Array.isArray(tasks) ? tasks.length : 0;
  elements.a2aHistoryMeta.textContent = t("a2a.history.meta", { count });
};

const renderA2aHistoryList = (tasks) => {
  if (!elements.a2aHistoryList) {
    return;
  }
  elements.a2aHistoryList.textContent = "";
  if (!Array.isArray(tasks) || tasks.length === 0) {
    elements.a2aHistoryList.textContent = t("a2a.history.empty");
    return;
  }
  sortHistoryTasksByTime(tasks).forEach((task) => {
    const taskId = resolveHistoryTaskId(task);
    const contextId = String(task?.contextId || "").trim();
    const status = resolveHistoryTaskStatus(task);
    const item = document.createElement("button");
    item.type = "button";
    item.className = "list-item";
    if (taskId && taskId === state.runtime.a2aTaskId) {
      item.classList.add("active");
    }

    const title = document.createElement("div");
    title.textContent = resolveHistoryTaskTitle(task);

    const metaParts = [];
    metaParts.push(taskId || "-");
    if (contextId && contextId !== taskId) {
      metaParts.push(contextId);
    }
    metaParts.push(status || "-");
    const timeText = formatTimestamp(resolveHistoryTaskTimestamp(task));
    if (timeText && timeText !== "-") {
      metaParts.push(timeText);
    }
    const meta = document.createElement("small");
    meta.textContent = metaParts.join(" · ");

    item.appendChild(title);
    item.appendChild(meta);
    item.addEventListener("click", () => {
      restoreA2aHistoryTask(task);
    });
    elements.a2aHistoryList.appendChild(item);
  });
};

const fetchA2aHistoryTasks = async () => {
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    throw new Error(t("a2a.error.endpointRequired"));
  }
  const headers = buildHeaders(false);
  const params = buildParamsFromForm("ListTasks");
  if (!Object.prototype.hasOwnProperty.call(params, "pageSize")) {
    params.pageSize = 50;
  }
  const payload = {
    jsonrpc: "2.0",
    id: generateRequestId(),
    method: "ListTasks",
    params,
  };
  const response = await fetch(endpoint, {
    method: "POST",
    headers,
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(t("common.requestFailed", { status: response.status }));
  }
  const data = await response.json();
  const result = data?.result || data || {};
  return Array.isArray(result.tasks) ? result.tasks : [];
};

const loadA2aHistory = async () => {
  if (!elements.a2aHistoryList) {
    return;
  }
  elements.a2aHistoryList.textContent = t("common.loading");
  try {
    const tasks = await fetchA2aHistoryTasks();
    updateA2aHistoryMeta(tasks);
    renderA2aHistoryList(tasks);
  } catch (error) {
    elements.a2aHistoryList.textContent = t("common.loadFailedWithMessage", {
      message: error.message,
    });
  }
};

const openA2aHistoryModal = async () => {
  if (!elements.a2aHistoryModal) {
    return;
  }
  elements.a2aHistoryModal.classList.add("active");
  await loadA2aHistory();
};

const closeA2aHistoryModal = () => {
  elements.a2aHistoryModal?.classList.remove("active");
};

const requestA2aTaskDetail = async (taskId) => {
  updateHeaderError("");
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    appendA2aEventLog(t("a2a.error.endpointRequired"), {
      eventType: t("a2a.event.error"),
      detail: {},
    });
    bumpA2aErrorCount();
    return false;
  }
  const params = buildParamsFromForm("GetTask");
  if (!params.name) {
    params.name = normalizeTaskName(taskId);
  }
  const payload = {
    jsonrpc: "2.0",
    id: generateRequestId(),
    method: "GetTask",
    params,
  };
  const headers = buildHeaders(false);
  const requestTimestamp = Date.now();
  startA2aStats({
    requestId: payload.id,
    method: payload.method,
    endpoint,
    stream: false,
  });
  appendA2aRequestLog(payload.method, {
    endpoint,
    method: "POST",
    headers: headersToObject(headers),
    payload,
  }, {
    eventType: t("a2a.event.request"),
    timestamp: requestTimestamp,
  });
  const controller = new AbortController();
  state.runtime.a2aController = controller;
  setStreamingState(true);

  let success = false;
  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    a2aStats.httpStatus = String(response.status || "");
    renderA2aStats();
    if (!response.ok) {
      const text = await response.text();
      appendA2aEventLog(t("a2a.event.error"), {
        eventType: t("a2a.event.error"),
        detail: text || response.status,
        rightTag: String(response.status),
      });
      bumpA2aErrorCount();
      updateModelOutputText(text || String(response.status));
      return false;
    }
    const data = await response.json();
    const result = data?.result || data;
    const normalizedResult = result?.task ? result : { task: result };
    a2aOutputState.rawEvents = [normalizedResult];
    bumpA2aEventCount();
    applyA2aStatsFromPayload(normalizedResult);
    applySessionFromPayload(normalizedResult);
    const appliedOutput = applyOutputFromResult(normalizedResult);
    if (!appliedOutput) {
      updateModelOutputText("");
    }
    appendA2aEventLog(t("a2a.event.response"), {
      eventType: t("a2a.event.response"),
      detail: data,
      timestamp: Date.now(),
    });
    success = true;
  } catch (error) {
    if (error.name === "AbortError") {
      appendA2aEventLog(t("a2a.event.aborted"), {
        eventType: t("a2a.event.aborted"),
        detail: {},
      });
    } else {
      appendA2aEventLog(t("a2a.event.error"), {
        eventType: t("a2a.event.error"),
        detail: error.message || String(error),
      });
      bumpA2aErrorCount();
    }
  } finally {
    setStreamingState(false);
    state.runtime.a2aController = null;
    finishA2aRequestLog({ timestamp: Date.now() });
    finishA2aStats();
  }
  return success;
};

const restoreA2aHistoryTask = async (task) => {
  const taskId = resolveHistoryTaskId(task);
  if (!taskId) {
    notify(t("a2a.history.missingTaskId"), "warn");
    return;
  }
  if (state.runtime.a2aStreaming) {
    notify(t("a2a.history.restoreBusy"), "warn");
    return;
  }
  const contextId = String(task?.contextId || "").trim() || taskId;
  if (elements.a2aTaskId) {
    elements.a2aTaskId.value = normalizeSessionIdValue(taskId);
  }
  if (elements.a2aContextId) {
    elements.a2aContextId.value = normalizeSessionIdValue(contextId);
  }
  updateSessionState(taskId, contextId);
  syncInputs();
  closeA2aHistoryModal();
  clearLogs();
  const restored = await requestA2aTaskDetail(taskId);
  if (!restored) {
    notify(t("a2a.history.restoreFailed"), "error");
    return;
  }
  notify(t("a2a.history.restoreSuccess"), "success");
};

// 解析 SSE 块，提取 data 行
const parseSseBlock = (block) => {
  const lines = block.split(/\r?\n/);
  const dataLines = [];
  lines.forEach((line) => {
    if (line.startsWith("data:")) {
      dataLines.push(line.slice(5).trim());
    }
  });
  if (!dataLines.length) {
    return null;
  }
  const text = dataLines.join("\n");
  try {
    return JSON.parse(text);
  } catch (error) {
    return text;
  }
};

const describeStreamEvent = (payload) => {
  if (!payload || typeof payload !== "object") {
    return { title: t("a2a.event.unknown"), eventType: t("a2a.event.unknown"), stage: "" };
  }
  if (payload.task) {
    const taskId = payload.task.id || "";
    const state = payload.task.status?.state || "";
    return {
      title: taskId ? `task ${taskId}` : t("a2a.event.task"),
      eventType: t("a2a.event.task"),
      stage: state,
    };
  }
  if (payload.statusUpdate) {
    const state = payload.statusUpdate.status?.state || "";
    const final = payload.statusUpdate.final ? t("a2a.event.final") : "";
    return {
      title: state || t("a2a.event.status"),
      eventType: t("a2a.event.status"),
      stage: final,
    };
  }
  if (payload.artifactUpdate) {
    const name =
      payload.artifactUpdate.artifact?.name || payload.artifactUpdate.artifact?.artifactId || "";
    return {
      title: name || t("a2a.event.artifact"),
      eventType: t("a2a.event.artifact"),
      stage: "",
    };
  }
  if (payload.message) {
    const role = payload.message.role || "";
    return {
      title: role ? `${role}` : t("a2a.event.message"),
      eventType: t("a2a.event.message"),
      stage: "",
    };
  }
  return { title: t("a2a.event.unknown"), eventType: t("a2a.event.unknown"), stage: "" };
};

// 处理单条流式事件并刷新输出
const handleStreamEvent = (payload) => {
  a2aOutputState.rawEvents.push(payload);
  bumpA2aEventCount();
  applyA2aStatsFromPayload(payload);
  applySessionFromPayload(payload);
  const { title, eventType, stage } = describeStreamEvent(payload);
  const eventTimestamp = resolveA2aEventTimestamp(payload) || Date.now();
  appendA2aEventLog(title, { eventType, stage, detail: payload, timestamp: eventTimestamp });
  if (payload?.message) {
    appendOutputMessage(payload.message);
  }
  if (payload?.artifactUpdate?.artifact) {
    appendOutputArtifact(payload.artifactUpdate.artifact);
  }
  renderA2aModelOutput();
};

const streamResponse = async (response) => {
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error(t("a2a.error.streamNotSupported"));
  }
  const decoder = new TextDecoder();
  let buffer = "";
  while (true) {
    const { value, done } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    const parts = buffer.split(/\n\n/);
    buffer = parts.pop() || "";
    parts.forEach((part) => {
      const parsed = parseSseBlock(part);
      if (parsed === null) {
        return;
      }
      handleStreamEvent(parsed);
    });
  }
  if (buffer.trim()) {
    const parsed = parseSseBlock(buffer);
    if (parsed !== null) {
      handleStreamEvent(parsed);
    }
  }
};

const handleSend = async () => {
  updateHeaderError("");
  if (state.runtime.a2aStreaming) {
    return;
  }
  const { taskId, contextId } = readSessionInputs();
  const nextSession = resolveSessionIdPair(taskId, contextId);
  const hasSession = Boolean(nextSession.taskId || nextSession.contextId);
  const sessionChanged =
    hasSession &&
    (nextSession.taskId !== (state.runtime.a2aTaskId || "") ||
      nextSession.contextId !== (state.runtime.a2aContextId || ""));
  if (!hasSession) {
    clearLogs();
    clearSessionState();
  } else if (sessionChanged) {
    clearLogs();
    updateSessionState(nextSession.taskId, nextSession.contextId);
  } else {
    resetA2aLogTimers();
  }
  const endpoint = resolveEndpoint();
  if (!endpoint) {
    appendA2aEventLog(t("a2a.error.endpointRequired"), {
      eventType: t("a2a.event.error"),
      detail: {},
    });
    bumpA2aErrorCount();
    return;
  }
  let payload;
  try {
    payload = buildJsonRpcPayload();
  } catch (error) {
    appendA2aEventLog(t("a2a.event.error"), {
      eventType: t("a2a.event.error"),
      detail: error.message || String(error),
    });
    bumpA2aErrorCount();
    return;
  }

  const method = payload.method;
  if (method === "SendMessage" || method === "SendStreamingMessage") {
    startA2aRound();
  }
  const isStreaming = STREAM_METHODS.has(method);
  const headers = buildHeaders(isStreaming);
  const requestTimestamp = Date.now();
  startA2aStats({
    requestId: payload.id,
    method,
    endpoint,
    stream: isStreaming,
  });
  appendA2aRequestLog(method, {
    endpoint,
    method: "POST",
    headers: headersToObject(headers),
    payload,
  }, {
    eventType: t("a2a.event.request"),
    timestamp: requestTimestamp,
  });
  const controller = new AbortController();
  state.runtime.a2aController = controller;
  setStreamingState(true);

  try {
    const response = await fetch(endpoint, {
      method: "POST",
      headers,
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    a2aStats.httpStatus = String(response.status || "");
    renderA2aStats();
    if (!response.ok) {
      const text = await response.text();
      appendA2aEventLog(t("a2a.event.error"), {
        eventType: t("a2a.event.error"),
        detail: text || response.status,
        rightTag: String(response.status),
      });
      bumpA2aErrorCount();
      updateModelOutputText(text || String(response.status));
      return;
    }
    if (isStreaming) {
      await streamResponse(response);
      appendA2aEventLog(t("a2a.event.streamFinished"), {
        eventType: t("a2a.event.streamFinished"),
        detail: {},
        timestamp: Date.now(),
      });
      return;
    }
    const data = await response.json();
    const result = data?.result || data;
    a2aOutputState.rawEvents = [result];
    bumpA2aEventCount();
    applyA2aStatsFromPayload(result);
    applySessionFromPayload(result);
    const appliedOutput = applyOutputFromResult(result);
    if (!appliedOutput) {
      updateModelOutputText("");
    }
    appendA2aEventLog(t("a2a.event.response"), {
      eventType: t("a2a.event.response"),
      detail: data,
      timestamp: Date.now(),
    });
  } catch (error) {
    if (error.name === "AbortError") {
      appendA2aEventLog(t("a2a.event.aborted"), {
        eventType: t("a2a.event.aborted"),
        detail: {},
      });
    } else {
      appendA2aEventLog(t("a2a.event.error"), {
        eventType: t("a2a.event.error"),
        detail: error.message || String(error),
      });
      bumpA2aErrorCount();
    }
  } finally {
    setStreamingState(false);
    state.runtime.a2aController = null;
    finishA2aRequestLog({ timestamp: Date.now() });
    finishA2aStats();
  }
};

const handleStop = () => {
  if (state.runtime.a2aController) {
    state.runtime.a2aController.abort();
  }
  setStreamingState(false);
};

// 新会话：清空日志并重置会话标识
const handleNewSession = () => {
  if (state.runtime.a2aStreaming) {
    handleStop();
  }
  clearLogs();
  clearSessionState();
};

// 选择指定轮次输出
const handleOutputRoundSelect = () => {
  if (!elements.a2aOutputRoundSelect) {
    return;
  }
  const value = Number(elements.a2aOutputRoundSelect.value);
  if (!Number.isFinite(value)) {
    return;
  }
  selectA2aRound(value, { manual: true });
};

const handleAgentCard = async () => {
  updateHeaderError("");
  const url = resolveAgentCardUrl();
  if (!url) {
    appendA2aEventLog(t("a2a.error.endpointRequired"), {
      eventType: t("a2a.event.error"),
      detail: {},
    });
    bumpA2aErrorCount();
    return;
  }
  const headers = buildHeaders(false);
  const requestTimestamp = Date.now();
  startA2aStats({
    requestId: "",
    method: "AgentCard",
    endpoint: url,
    stream: false,
  });
  appendA2aRequestLog("AgentCard", {
    endpoint: url,
    method: "GET",
    headers: headersToObject(headers),
  }, {
    eventType: t("a2a.event.request"),
    timestamp: requestTimestamp,
  });
  try {
    const response = await fetch(url, { headers });
    a2aStats.httpStatus = String(response.status || "");
    renderA2aStats();
    if (!response.ok) {
      const text = await response.text();
      appendA2aEventLog(t("a2a.event.error"), {
        eventType: t("a2a.event.error"),
        detail: text || response.status,
        rightTag: String(response.status),
      });
      bumpA2aErrorCount();
      updateModelOutputText(text || String(response.status));
      return;
    }
    const data = await response.json();
    bumpA2aEventCount();
    openAgentCardModal(data);
    appendA2aEventLog(t("a2a.event.agentCard"), {
      eventType: t("a2a.event.agentCard"),
      detail: data,
      timestamp: Date.now(),
    });
  } catch (error) {
    appendA2aEventLog(t("a2a.event.error"), {
      eventType: t("a2a.event.error"),
      detail: error.message || String(error),
    });
    bumpA2aErrorCount();
  } finally {
    finishA2aRequestLog({ timestamp: Date.now() });
    finishA2aStats();
  }
};

const syncInputs = () => {
  writeA2aState({
    endpoint: elements.a2aEndpoint?.value || "",
    version: elements.a2aVersion?.value || "",
    authType: elements.a2aAuthType?.value || "apiKey",
    useGlobalKey: Boolean(elements.a2aUseGlobalKey?.checked),
    apiKey: elements.a2aApiKey?.value || "",
    headers: elements.a2aHeaders?.value || "",
    method: elements.a2aMethod?.value || "",
    requestId: elements.a2aRequestId?.value || "",
    userId: elements.a2aUserId?.value || "",
    taskId: elements.a2aTaskId?.value || "",
    contextId: elements.a2aContextId?.value || "",
    message: elements.a2aMessage?.value || "",
    toolNames: elements.a2aToolNames?.value || "",
    modelName: elements.a2aModelName?.value || "",
    historyLength: elements.a2aHistoryLength?.value || "",
    pageSize: elements.a2aPageSize?.value || "",
    pageToken: elements.a2aPageToken?.value || "",
    statusFilter: elements.a2aStatusFilter?.value || "",
    blocking: Boolean(elements.a2aBlocking?.checked),
    includeArtifacts: Boolean(elements.a2aIncludeArtifacts?.checked),
    paramsJson: elements.a2aParamsJson?.value || "",
  });
};

const applyStoredState = () => {
  const stored = readA2aState();
  if (elements.a2aEndpoint) {
    if (stored.endpoint) {
      elements.a2aEndpoint.value = stored.endpoint;
      elements.a2aEndpoint.dataset.manual = "true";
    } else {
      syncEndpointDefault();
    }
  }
  if (elements.a2aVersion) {
    elements.a2aVersion.value = stored.version || "1.0";
  }
  if (elements.a2aAuthType) {
    elements.a2aAuthType.value = stored.authType || "apiKey";
  }
  if (elements.a2aUseGlobalKey) {
    elements.a2aUseGlobalKey.checked = stored.useGlobalKey !== false;
  }
  if (elements.a2aApiKey) {
    elements.a2aApiKey.value = stored.apiKey || "";
  }
  if (elements.a2aHeaders) {
    elements.a2aHeaders.value = stored.headers || "";
  }
  if (elements.a2aMethod) {
    elements.a2aMethod.value = stored.method || "SendMessage";
  }
  if (elements.a2aRequestId) {
    elements.a2aRequestId.value = stored.requestId || "";
  }
  if (elements.a2aUserId) {
    elements.a2aUserId.value = stored.userId || elements.userId?.value || "";
  }
  if (elements.a2aTaskId) {
    elements.a2aTaskId.value = stored.taskId || "";
  }
  if (elements.a2aContextId) {
    elements.a2aContextId.value = stored.contextId || "";
  }
  if (elements.a2aMessage) {
    elements.a2aMessage.value = stored.message || "";
  }
  if (elements.a2aToolNames) {
    elements.a2aToolNames.value = stored.toolNames || "";
  }
  if (elements.a2aModelName) {
    elements.a2aModelName.value = stored.modelName || "";
  }
  if (elements.a2aHistoryLength) {
    elements.a2aHistoryLength.value = stored.historyLength || "";
  }
  if (elements.a2aPageSize) {
    elements.a2aPageSize.value = stored.pageSize || "";
  }
  if (elements.a2aPageToken) {
    elements.a2aPageToken.value = stored.pageToken || "";
  }
  if (elements.a2aStatusFilter) {
    elements.a2aStatusFilter.value = stored.statusFilter || "";
  }
  if (elements.a2aBlocking) {
    elements.a2aBlocking.checked = Boolean(stored.blocking);
  }
  if (elements.a2aMethod && elements.a2aBlocking) {
    const hasBlocking = Object.prototype.hasOwnProperty.call(stored, "blocking");
    if (!hasBlocking && elements.a2aMethod.value === "SendMessage") {
      elements.a2aBlocking.checked = true;
    }
  }
  if (elements.a2aIncludeArtifacts) {
    elements.a2aIncludeArtifacts.checked = Boolean(stored.includeArtifacts);
  }
  if (elements.a2aParamsJson) {
    elements.a2aParamsJson.value = stored.paramsJson || "";
  }
  state.runtime.a2aTaskId = normalizeSessionIdValue(elements.a2aTaskId?.value || "");
  state.runtime.a2aContextId = normalizeSessionIdValue(elements.a2aContextId?.value || "");
};

const bindInputs = () => {
  if (elements.a2aEndpoint) {
    elements.a2aEndpoint.addEventListener("input", () => {
      elements.a2aEndpoint.dataset.manual = elements.a2aEndpoint.value.trim() ? "true" : "false";
      syncInputs();
    });
  }
  if (elements.a2aUseGlobalKey && elements.a2aApiKey) {
    const toggleApiKey = () => {
      const useGlobal = elements.a2aUseGlobalKey.checked;
      elements.a2aApiKey.disabled = useGlobal;
    };
    toggleApiKey();
    elements.a2aUseGlobalKey.addEventListener("change", () => {
      toggleApiKey();
      syncInputs();
    });
  }
  if (elements.a2aMethod && elements.a2aBlocking) {
    elements.a2aMethod.addEventListener("change", () => {
      if (elements.a2aMethod.value === "SendMessage") {
        elements.a2aBlocking.checked = true;
      }
      syncInputs();
    });
  }

  [
    "a2aVersion",
    "a2aAuthType",
    "a2aApiKey",
    "a2aHeaders",
    "a2aRequestId",
    "a2aUserId",
    "a2aTaskId",
    "a2aContextId",
    "a2aMessage",
    "a2aToolNames",
    "a2aModelName",
    "a2aHistoryLength",
    "a2aPageSize",
    "a2aPageToken",
    "a2aStatusFilter",
    "a2aBlocking",
    "a2aIncludeArtifacts",
    "a2aParamsJson",
  ].forEach((key) => {
    const el = elements[key];
    if (!el) {
      return;
    }
    el.addEventListener("change", syncInputs);
  });

  if (elements.apiBase) {
    elements.apiBase.addEventListener("change", syncEndpointDefault);
  }
};

export const initA2aPanel = () => {
  applyStoredState();
  syncEndpointDefault();
  bindInputs();
  setStreamingState(false);
  resetA2aStats();
  resetA2aOutput();
  state.panelLoaded.a2a = true;
  if (elements.a2aConnectBtn) {
    elements.a2aConnectBtn.addEventListener("click", openA2aConnectionModal);
  }
  if (elements.a2aAdvancedBtn) {
    elements.a2aAdvancedBtn.addEventListener("click", openA2aAdvancedModal);
  }
  if (elements.a2aConnectionClose) {
    elements.a2aConnectionClose.addEventListener("click", closeA2aConnectionModal);
  }
  if (elements.a2aConnectionCloseBtn) {
    elements.a2aConnectionCloseBtn.addEventListener("click", closeA2aConnectionModal);
  }
  if (elements.a2aConnectionModal) {
    elements.a2aConnectionModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aConnectionModal) {
        closeA2aConnectionModal();
      }
    });
  }
  if (elements.a2aAdvancedClose) {
    elements.a2aAdvancedClose.addEventListener("click", closeA2aAdvancedModal);
  }
  if (elements.a2aAdvancedCloseBtn) {
    elements.a2aAdvancedCloseBtn.addEventListener("click", closeA2aAdvancedModal);
  }
  if (elements.a2aAdvancedModal) {
    elements.a2aAdvancedModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aAdvancedModal) {
        closeA2aAdvancedModal();
      }
    });
  }
  if (elements.a2aAgentCardClose) {
    elements.a2aAgentCardClose.addEventListener("click", closeAgentCardModal);
  }
  if (elements.a2aAgentCardCloseBtn) {
    elements.a2aAgentCardCloseBtn.addEventListener("click", closeAgentCardModal);
  }
  if (elements.a2aAgentCardModal) {
    elements.a2aAgentCardModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aAgentCardModal) {
        closeAgentCardModal();
      }
    });
  }
  if (elements.a2aSendBtn) {
    elements.a2aSendBtn.addEventListener("click", () => {
      if (state.runtime.a2aStreaming) {
        handleStop();
        return;
      }
      handleSend();
    });
  }
  if (elements.a2aNewSessionBtn) {
    elements.a2aNewSessionBtn.addEventListener("click", handleNewSession);
  }
  if (elements.a2aAgentCardBtn) {
    elements.a2aAgentCardBtn.addEventListener("click", handleAgentCard);
  }
  if (elements.a2aHistoryBtn) {
    elements.a2aHistoryBtn.addEventListener("click", openA2aHistoryModal);
  }
  if (elements.a2aHistoryClose) {
    elements.a2aHistoryClose.addEventListener("click", closeA2aHistoryModal);
  }
  if (elements.a2aHistoryCloseBtn) {
    elements.a2aHistoryCloseBtn.addEventListener("click", closeA2aHistoryModal);
  }
  if (elements.a2aHistoryModal) {
    elements.a2aHistoryModal.addEventListener("click", (event) => {
      if (event.target === elements.a2aHistoryModal) {
        closeA2aHistoryModal();
      }
    });
  }
  if (elements.a2aOutputRoundSelect) {
    elements.a2aOutputRoundSelect.addEventListener("change", handleOutputRoundSelect);
  }
};
