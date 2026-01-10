import { getCurrentLanguage, t } from "./i18n.js?v=20260110-04";

// 工具函数：纯逻辑处理，便于多模块复用

// 判断是否为普通对象，避免数组或空值影响解析
export const isPlainObject = (value) => Boolean(value && typeof value === "object" && !Array.isArray(value));

// 解析请求头JSON，便于在输入错误时给出提示
export const parseHeadersValue = (raw) => {
  if (!raw || !raw.trim()) {
    return { headers: {}, error: "" };
  }
  try {
    const parsed = JSON.parse(raw);
    if (!isPlainObject(parsed)) {
      return { headers: null, error: t("utils.headers.notObject") };
    }
    return { headers: parsed, error: "" };
  } catch (error) {
    return { headers: null, error: t("utils.headers.parseFailed") };
  }
};

// 转义 HTML，避免提示词内容被浏览器解析
export const escapeHtml = (text) =>
  String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");

// 将Markdown 文本转换为标题高亮的 HTML，用于编辑区背景标记一级标题
export const buildHeadingHighlightHtml = (text) => {
  const raw = String(text ?? "");
  const lines = raw.replace(/\r/g, "").split("\n");
  return lines
    .map((line) => {
      const escaped = escapeHtml(line) || "&nbsp;";
      const isHeading = /^\s*#\s+\S/.test(line);
      const classes = isHeading
        ? "knowledge-editor-line knowledge-heading-line"
        : "knowledge-editor-line";
      return `<span class="${classes}">${escaped}</span>`;
    })
    .join("");
};

// 统一格式化工具输入结构，避免空值或异常结构导致展示混乱
export const formatToolSchema = (schema) => {
  if (schema === null || schema === undefined) {
    return t("utils.toolSchema.empty");
  }
  if (typeof schema === "string") {
    const trimmed = schema.trim();
    return trimmed ? trimmed : t("utils.toolSchema.empty");
  }
  if (Array.isArray(schema) && schema.length === 0) {
    return t("utils.toolSchema.empty");
  }
  if (isPlainObject(schema) && Object.keys(schema).length === 0) {
    return t("utils.toolSchema.empty");
  }
  try {
    return JSON.stringify(schema, null, 2);
  } catch (error) {
    return String(schema);
  }
};

// 读取工具输入结构字段，兼容input_schema/inputSchema/args_schema 等命名
export const getToolInputSchema = (tool) =>
  tool?.input_schema ?? tool?.inputSchema ?? tool?.args_schema ?? tool?.argsSchema ?? null;

// 格式化字节为可读大小
export const formatBytes = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unitIndex]}`;
};

// 格式化耗时
export const formatDuration = (seconds) => {
  if (!Number.isFinite(seconds)) {
    return "-";
  }
  const total = Math.max(0, Math.round(seconds));
  const mins = Math.floor(total / 60);
  const secs = total % 60;
  if (mins > 0) {
    return `${mins}m ${secs}s`;
  }
  return `${secs}s`;
};

// 格式化长耗时，适用于系统运行时长与平均耗时展示
export const formatDurationLong = (seconds) => {
  if (!Number.isFinite(seconds)) {
    return "-";
  }
  const total = Math.max(0, Math.floor(seconds));
  const days = Math.floor(total / 86400);
  const hours = Math.floor((total % 86400) / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  if (days > 0) {
    return t("time.format.daysHours", { days, hours });
  }
  if (hours > 0) {
    return t("time.format.hoursMinutes", { hours, minutes: mins });
  }
  if (mins > 0) {
    return t("time.format.minutesSeconds", { minutes: mins, seconds: secs });
  }
  return t("time.format.seconds", { seconds: secs });
};

// 格式化token 数量：小于100 万用 k，达到100 万及以上用m
export const formatTokenCount = (value) => {
  if (!Number.isFinite(value)) {
    return "-";
  }
  const tokens = Math.max(0, Number(value));
  const useMillion = tokens >= 1_000_000;
  const base = useMillion ? 1_000_000 : 1_000;
  const unit = useMillion ? "m" : "k";
  const scaled = tokens / base;
  let decimals = 1;
  if (scaled >= 100) {
    decimals = 0;
  } else if (scaled < 1) {
    decimals = 2;
  }
  return `${scaled.toFixed(decimals)}${unit}`;
};

// 格式化开始时间，避免无效时间导致显示异常
export const formatTimestamp = (value) => {
  if (!value) {
    return "-";
  }
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return "-";
  }
  return parsed.toLocaleString(getCurrentLanguage());
};

// 规范化API 地址，确保以 /wunder 结尾并清理重复路径
export const normalizeApiBase = (raw) => {
  const trimmed = String(raw || "").trim();
  if (!trimmed) {
    return "";
  }
  try {
    const url = new URL(trimmed);
    url.search = "";
    url.hash = "";
    const basePath = url.pathname.replace(/\/+$/, "");
    const collapsed = basePath.replace(/(\/wunder)+$/, "/wunder");
    url.pathname = collapsed.endsWith("/wunder") ? collapsed : `${collapsed}/wunder`;
    return url.toString().replace(/\/$/, "");
  } catch (error) {
    const basePath = trimmed.replace(/\/+$/, "");
    const collapsed = basePath.replace(/(\/wunder)+$/, "/wunder");
    return collapsed.endsWith("/wunder") ? collapsed : `${collapsed}/wunder`;
  }
};
