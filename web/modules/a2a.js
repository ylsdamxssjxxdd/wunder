import { elements } from "./elements.js?v=20260104-03";
import { state } from "./state.js";
import { parseHeadersValue, normalizeApiBase } from "./utils.js?v=20251229-02";
import { t } from "./i18n.js?v=20260104-03";

const A2A_STATE_KEY = "wunder_a2a_state";
const MAX_LOG_ITEMS = 300;
const STREAM_METHODS = new Set(["SendStreamingMessage", "SubscribeToTask"]);
const a2aLogTimestamps = new WeakMap();

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
  messages: [],
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
const appendA2aRequestLog = (title, detail, options = {}) =>
  appendA2aLogItem(elements.a2aRequestLog, title, {
    detail,
    showDuration: false,
    ...options,
  });

// 重置日志时间差计数，避免新一轮耗时计算串联
const resetA2aLogTimers = () => {
  if (elements.a2aEventLog) {
    a2aLogTimestamps.delete(elements.a2aEventLog);
  }
  if (elements.a2aRequestLog) {
    a2aLogTimestamps.delete(elements.a2aRequestLog);
  }
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
const resetA2aOutput = () => {
  a2aOutputState.messages = [];
  a2aOutputState.rawEvents = [];
  updateModelOutputText("");
};

// 根据缓存内容渲染模型输出
const resolveModelOutputText = () => {
  if (a2aOutputState.messages.length) {
    return a2aOutputState.messages.join("\n");
  }
  if (a2aOutputState.rawEvents.length) {
    return stringifyPayload(a2aOutputState.rawEvents[a2aOutputState.rawEvents.length - 1]);
  }
  return "";
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
  const taskId = payload.task?.id || payload.statusUpdate?.taskId || payload.message?.taskId;
  if (taskId) {
    a2aStats.taskId = taskId;
  }
  const contextId =
    payload.task?.contextId || payload.statusUpdate?.contextId || payload.message?.contextId;
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
  resetA2aLogTimers();
  resetA2aOutput();
  resetA2aStats();
};

const setStreamingState = (active) => {
  state.runtime.a2aStreaming = active;
  if (elements.a2aEventCard) {
    elements.a2aEventCard.classList.toggle("is-waiting", active);
  }
  if (elements.a2aStatus) {
    elements.a2aStatus.textContent = active ? t("a2a.status.streaming") : t("a2a.status.idle");
  }
  if (elements.a2aSendBtn) {
    elements.a2aSendBtn.disabled = active;
  }
  if (elements.a2aStopBtn) {
    elements.a2aStopBtn.disabled = !active;
  }
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

// 提取消息内容文本，优先读取 parts/text
const extractMessageText = (message) => {
  if (!message || typeof message !== "object") {
    return "";
  }
  const role = typeof message.role === "string" && message.role ? `${message.role}: ` : "";
  if (Array.isArray(message.parts)) {
    const texts = message.parts
      .map((part) => (typeof part?.text === "string" ? part.text : ""))
      .filter((text) => text);
    if (texts.length) {
      return `${role}${texts.join("")}`.trim();
    }
  }
  if (typeof message.content === "string" && message.content) {
    return `${role}${message.content}`.trim();
  }
  if (typeof message.text === "string" && message.text) {
    return `${role}${message.text}`.trim();
  }
  return role.trim();
};

// 追加模型输出中的消息文本
const appendOutputMessage = (message) => {
  const text = extractMessageText(message);
  if (!text) {
    return;
  }
  a2aOutputState.messages.push(text);
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
  const { title, eventType, stage } = describeStreamEvent(payload);
  appendA2aEventLog(title, { eventType, stage, detail: payload, timestamp: Date.now() });
  if (payload?.message) {
    appendOutputMessage(payload.message);
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
  clearLogs();
  if (state.runtime.a2aStreaming) {
    return;
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
  const isStreaming = STREAM_METHODS.has(method);
  const headers = buildHeaders(isStreaming);
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
  });
  const controller = new AbortController();
  state.runtime.a2aController = controller;
  setStreamingState(isStreaming);

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
    a2aOutputState.rawEvents = [data];
    bumpA2aEventCount();
    applyA2aStatsFromPayload(data?.result || data);
    updateModelOutputText(stringifyPayload(data));
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
    finishA2aStats();
  }
};

const handleStop = () => {
  if (state.runtime.a2aController) {
    state.runtime.a2aController.abort();
  }
  setStreamingState(false);
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
    a2aOutputState.rawEvents = [data];
    bumpA2aEventCount();
    updateModelOutputText(stringifyPayload(data));
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
  if (elements.a2aIncludeArtifacts) {
    elements.a2aIncludeArtifacts.checked = Boolean(stored.includeArtifacts);
  }
  if (elements.a2aParamsJson) {
    elements.a2aParamsJson.value = stored.paramsJson || "";
  }
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

  [
    "a2aVersion",
    "a2aAuthType",
    "a2aApiKey",
    "a2aHeaders",
    "a2aMethod",
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
  if (elements.a2aSendBtn) {
    elements.a2aSendBtn.addEventListener("click", handleSend);
  }
  if (elements.a2aStopBtn) {
    elements.a2aStopBtn.addEventListener("click", handleStop);
  }
  if (elements.a2aClearBtn) {
    elements.a2aClearBtn.addEventListener("click", clearLogs);
  }
  if (elements.a2aAgentCardBtn) {
    elements.a2aAgentCardBtn.addEventListener("click", handleAgentCard);
  }
};
