import { t } from "./i18n.js?v=20260124-01";
import { notify } from "./notify.js";

const TRACE_HEADER = "x-trace-id";

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
  const message = pickString(
    parsed.message,
    fallbackMessage,
    status ? t("common.requestFailed", { status }) : t("common.requestFailed", { status: "-" })
  );
  return {
    message,
    code: parsed.code,
    traceId,
    status,
    hint: parsed.hint,
  };
};

const TRACE_SUFFIX_LABEL = "trace_id";

const normalizeTraceLabel = (label) => {
  if (typeof label === "string" && label.trim()) {
    return label.trim();
  }
  return TRACE_SUFFIX_LABEL;
};

export const formatApiErrorMessage = (resolved, fallbackMessage = "", options = {}) => {
  const message = pickString(resolved && resolved.message, fallbackMessage);
  if (!message) {
    return "";
  }
  const traceId = pickString(resolved && resolved.traceId);
  if (!traceId) {
    return message;
  }
  const traceLabel = normalizeTraceLabel(options.traceLabel);
  return `${message} (${traceLabel}: ${traceId})`;
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
      traceId: resolved.traceId,
      hint: resolved.hint,
    },
    "error",
    {
      duration: Number.isFinite(options.duration) ? options.duration : 5200,
      actionLabel: options.actionLabel || t("common.copy"),
      actionSuccess: options.actionSuccess || t("common.traceIdCopied"),
      actionFailed: options.actionFailed || t("common.traceIdCopyFailed"),
      traceLabel: options.traceLabel || t("common.traceId"),
    }
  );
  return resolved;
};
