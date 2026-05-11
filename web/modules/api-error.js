import { t } from "./i18n.js?v=20260215-01";
import { notify } from "./notify.js";

const TRACE_HEADER = "x-trace-id";
const TRACE_ID_RE = /\b(?:trace[_-]?id|err_[a-z0-9]+)\b[:=\s-]*[a-z0-9_-]*/gi;

const pickString = (...values) => {
  for (const value of values) {
    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }
  return "";
};

const readHeader = (headers, key) => {
  if (!headers) {
    return "";
  }
  if (typeof headers.get === "function") {
    return String(headers.get(key) || "").trim();
  }
  const lowered = key.toLowerCase();
  const entries = Object.entries(headers || {});
  for (const pair of entries) {
    const name = String(pair[0] || "").toLowerCase();
    if (name === lowered) {
      return String(pair[1] || "").trim();
    }
  }
  return "";
};

const normalizeErrorText = (value) => {
  const text = String(value || "")
    .replace(TRACE_ID_RE, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!text) {
    return "";
  }
  const lowered = text.toLowerCase();
  if (lowered === "[object object]" || lowered === "object object") {
    return "";
  }
  return text;
};

const containsChinese = (value) => /[\u4e00-\u9fff]/.test(value);

const includesAny = (value, patterns) => patterns.some((pattern) => value.includes(pattern));

const localizeErrorMessage = (message, status, fallbackMessage) => {
  const normalizedMessage = normalizeErrorText(message);
  if (!normalizedMessage) {
    return normalizeErrorText(fallbackMessage) || t("common.requestFailed", { status: status || "-" });
  }
  if (containsChinese(normalizedMessage)) {
    return normalizedMessage;
  }
  const lowered = normalizedMessage.toLowerCase();
  if (
    includesAny(lowered, [
      "error parsing multipart/form-data request",
      "invalid boundary",
      "multipart",
      "form-data",
    ])
  ) {
    return "上传请求格式错误，请刷新页面后重试。";
  }
  if (
    includesAny(lowered, [
      "payload too large",
      "request body too large",
      "body too large",
      "file too large",
      "entity too large",
      "content too large",
    ])
  ) {
    return "上传内容过大，请压缩后重试。";
  }
  if (
    includesAny(lowered, [
      "network error",
      "failed to fetch",
      "load failed",
      "network request failed",
      "econnrefused",
      "socket hang up",
      "network",
    ])
  ) {
    return "网络连接失败，请检查服务是否可用后重试。";
  }
  if (includesAny(lowered, ["timeout", "timed out", "econnaborted", "deadline has elapsed"])) {
    return "请求超时，请稍后重试。";
  }
  if (
    includesAny(lowered, [
      "unauthorized",
      "forbidden",
      "auth required",
      "authentication failed",
      "invalid credentials",
      "permission denied",
      "access denied",
    ])
  ) {
    return status === 401 ? "登录状态已失效，请重新登录。" : "没有权限执行此操作。";
  }
  if (
    includesAny(lowered, [
      "not found",
      "file not found",
      "skill not found",
      "resource not found",
      "404",
    ])
  ) {
    return "目标内容不存在或已被删除。";
  }
  if (
    includesAny(lowered, [
      "bad request",
      "invalid request",
      "invalid parameter",
      "invalid payload",
      "validation failed",
      "missing parameter",
      "required",
    ])
  ) {
    return normalizeErrorText(fallbackMessage) || "请求参数不正确，请检查后重试。";
  }
  if (status && status >= 500) {
    return "服务暂时异常，请稍后重试。";
  }
  return normalizeErrorText(fallbackMessage) || "操作失败，请稍后重试。";
};

const normalizeDetailMessage = (detail) => {
  if (!detail) return "";
  if (typeof detail === "string") return detail;
  if (typeof detail.message === "string" && detail.message.trim()) return detail.message.trim();
  if (typeof detail.error === "string" && detail.error.trim()) return detail.error.trim();
  if (detail.detail) {
    if (typeof detail.detail === "string" && detail.detail.trim()) {
      return detail.detail.trim();
    }
    if (typeof detail.detail.message === "string" && detail.detail.message.trim()) {
      return detail.detail.message.trim();
    }
  }
  return "";
};

const parseApiErrorPayload = (payload) => {
  if (!payload || typeof payload !== "object") {
    return {
      message: "",
      code: "",
      traceId: "",
      status: null,
      hint: "",
    };
  }
  const error = payload.error && typeof payload.error === "object" ? payload.error : {};
  const detail = payload.detail;
  return {
    message: pickString(
      error.message,
      normalizeDetailMessage(detail),
      payload.message,
      payload.error_message,
      payload.error
    ),
    code: pickString(error.code, detail && detail.code, payload.code),
    traceId: pickString(error.trace_id, detail && detail.trace_id, payload.trace_id),
    status: Number.isFinite(Number(error.status)) ? Number(error.status) : null,
    hint: pickString(error.hint, detail && detail.hint, payload.hint),
  };
};

const readJsonSafe = async (response) => {
  if (!response) {
    return null;
  }
  try {
    const target = typeof response.clone === "function" ? response.clone() : response;
    return await target.json();
  } catch (error) {
    return null;
  }
};

export const resolveApiError = async (response, fallbackMessage = "") => {
  const payload = await readJsonSafe(response);
  const parsed = parseApiErrorPayload(payload);
  const traceId = pickString(parsed.traceId, readHeader(response && response.headers, TRACE_HEADER));
  const status = parsed.status || (response && response.status ? Number(response.status) : null);
  const message = localizeErrorMessage(
    parsed.message,
    status,
    pickString(
      fallbackMessage,
      status ? t("common.requestFailed", { status }) : t("common.requestFailed", { status: "-" })
    )
  );
  return {
    message,
    code: parsed.code,
    traceId,
    status,
    hint: parsed.hint,
  };
};

export const formatApiErrorMessage = (resolved, fallbackMessage = "", options = {}) => {
  return pickString(resolved && resolved.message, fallbackMessage);
};

export const resolveApiErrorMessage = async (response, fallbackMessage = "", options = {}) => {
  const resolved = await resolveApiError(response, fallbackMessage);
  return formatApiErrorMessage(resolved, fallbackMessage, options);
};

export const notifyApiError = async (response, fallbackMessage = "", options = {}) => {
  const resolved = await resolveApiError(response, fallbackMessage);
  notify(
    {
      message: resolved.message,
      hint: resolved.hint,
    },
    "error",
    {
      duration: Number.isFinite(options.duration) ? options.duration : 5200,
    }
  );
  return resolved;
};
