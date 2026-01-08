import { elements } from "./elements.js?v=20260105-02";
import { getCurrentLanguage, t } from "./i18n.js?v=20260105-01";

// 控制日志数量上限，避免详情节点堆积导致页面卡顿
const MAX_LOG_ITEMS = 300;
const lastLogTimestamps = new WeakMap();

const trimLogItems = (container) => {
  if (!container) {
    return;
  }
  while (container.children.length > MAX_LOG_ITEMS) {
    container.removeChild(container.firstChild);
  }
};

const buildDetailText = (detail, fallback) => {
  if (detail === undefined || detail === null) {
    return fallback;
  }
  if (typeof detail === "string") {
    return detail;
  }
  try {
    return JSON.stringify(detail, null, 2);
  } catch (error) {
    return String(detail);
  }
};

const normalizeLogTimestampText = (value) => {
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

// 统一格式化日志时间，兼容 ISO 字符串/时间戳/Date
const resolveLogTimestamp = (value, fallbackMs) => {
  const fallbackTime = Number.isFinite(fallbackMs) ? new Date(fallbackMs) : new Date();
  if (!value) {
    return fallbackTime.toLocaleTimeString(getCurrentLanguage());
  }
  if (value instanceof Date) {
    return value.toLocaleTimeString(getCurrentLanguage());
  }
  if (typeof value === "number") {
    const parsed = new Date(value);
    return Number.isNaN(parsed.getTime()) ? fallbackTime.toLocaleTimeString(getCurrentLanguage()) : parsed.toLocaleTimeString(getCurrentLanguage());
  }
  if (typeof value === "string") {
    const parsed = new Date(normalizeLogTimestampText(value));
    if (!Number.isNaN(parsed.getTime())) {
      return parsed.toLocaleTimeString(getCurrentLanguage());
    }
    return value;
  }
  return fallbackTime.toLocaleTimeString(getCurrentLanguage());
};

// 解析日志时间为毫秒时间戳，便于统计相邻事件间隔
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
    const parsed = new Date(normalizeLogTimestampText(value));
    return Number.isNaN(parsed.getTime()) ? null : parsed.getTime();
  }
  return null;
};

// 统一格式化耗时文本，统一使用秒单位展示
const formatDuration = (durationMs) => {
  if (!Number.isFinite(durationMs)) {
    return "";
  }
  const seconds = Math.max(0, durationMs) / 1000;
  return `${seconds.toFixed(2)}s`;
};

// 计算与上一条日志的耗时差，允许外部覆盖展示
const resolveDurationMs = (container, timestampMs, overrideMs) => {
  if (Number.isFinite(overrideMs)) {
    if (Number.isFinite(timestampMs)) {
      lastLogTimestamps.set(container, timestampMs);
    }
    return overrideMs;
  }
  if (!Number.isFinite(timestampMs)) {
    return null;
  }
  const lastMs = lastLogTimestamps.get(container);
  lastLogTimestamps.set(container, timestampMs);
  if (!Number.isFinite(lastMs)) {
    // 首条日志显示 0ms，避免用户误以为耗时标签未渲染
    return 0;
  }
  const diff = timestampMs - lastMs;
  return diff >= 0 ? diff : 0;
};

const appendLogItem = (container, title, options = {}) => {
  const timestampMs = resolveLogTimestampMs(options.timestamp);
  const fallbackMs = Number.isFinite(timestampMs) ? timestampMs : Date.now();
  const timestamp = resolveLogTimestamp(options.timestamp, fallbackMs);
  // 请求日志不展示耗时，事件日志默认展示，允许外部显式关闭
  const showDuration = options.showDuration !== false && container !== elements.requestLog;
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
  const highlightEvent = options.highlight === true;
  const highlightClass =
    typeof options.highlightClass === "string" ? options.highlightClass.trim() : "";
  let showEventBadge = options.showEventBadge !== false;
  if (eventText && eventText.toLowerCase().startsWith("compaction")) {
    showEventBadge = false;
  }

  const item = document.createElement("details");
  item.className = "log-item";
  if (highlightEvent) {
    item.classList.add("log-item--tool");
    if (highlightClass) {
      item.classList.add(`log-item--tool-${highlightClass}`);
    }
  }

  const summary = document.createElement("summary");
  summary.className = "log-summary";

  const timeNode = document.createElement("span");
  timeNode.className = "log-time";
  timeNode.textContent = `[${timestamp}]`;
  summary.appendChild(timeNode);

  if (eventText && showEventBadge) {
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

  // 将耗时与右侧标签统一放在右侧容器，确保对齐
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

// 将事件日志追加到页面中，默认折叠显示
export const appendLog = (text, options = {}) => {
  return appendLogItem(elements.eventLog, text, options);
};

// 将请求日志追加到页面中，默认折叠显示
export const appendRequestLog = (title, detail, options = {}) => {
  return appendLogItem(elements.requestLog, title, { detail, ...options });
};

// 清空日志与回复区域，便于开始新一轮调试
export const clearOutput = () => {
  elements.eventLog.innerHTML = "";
  elements.requestLog.innerHTML = "";
  if (elements.modelOutputText) {
    elements.modelOutputText.textContent = "";
  } else if (elements.modelOutput) {
    elements.modelOutput.textContent = "";
  }
  if (elements.modelOutputA2ui) {
    elements.modelOutputA2ui.innerHTML = "";
  }
  elements.finalAnswer.textContent = "";
  if (elements.eventLog) {
    lastLogTimestamps.delete(elements.eventLog);
  }
  if (elements.requestLog) {
    lastLogTimestamps.delete(elements.requestLog);
  }
};



