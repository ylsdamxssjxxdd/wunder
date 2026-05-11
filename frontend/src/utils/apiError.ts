import { ElMessage } from 'element-plus';

import { t } from '@/i18n';

const HEADER_TRACE_ID = 'x-trace-id';
const TRACE_ID_RE = /\b(?:trace[_-]?id|err_[a-z0-9]+)\b[:=\s-]*[a-z0-9_-]*/gi;

type HeaderBag = Headers | Record<string, unknown> | undefined | null;
type UnknownRecord = Record<string, unknown>;

type ResolvedApiError = {
  message: string;
  code: string;
  traceId: string;
  status: number | null;
  hint: string;
};

type NotificationOptions = {
  duration?: number;
};

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

const normalizeErrorText = (value: string): string => {
  const text = String(value || '')
    .replace(TRACE_ID_RE, ' ')
    .replace(/\s+/g, ' ')
    .trim();
  if (!text) return '';
  const lowered = text.toLowerCase();
  if (lowered === '[object object]' || lowered === 'object object') {
    return '';
  }
  return text;
};

const readHeader = (headers: HeaderBag, key: string): string => {
  if (!headers) return '';
  if (typeof (headers as Headers).get === 'function') {
    return String((headers as Headers).get(key) || '').trim();
  }
  const lowered = key.toLowerCase();
  for (const [name, value] of Object.entries(headers)) {
    if (String(name).toLowerCase() === lowered) {
      return String(value || '').trim();
    }
  }
  return '';
};

const containsChinese = (value: string): boolean => /[\u4e00-\u9fff]/.test(value);

const includesAny = (value: string, patterns: string[]): boolean =>
  patterns.some((pattern) => value.includes(pattern));

export const localizeApiErrorText = (
  message: string,
  status: number | null,
  fallback: string
): string => {
  const normalizedMessage = normalizeErrorText(message);
  if (!normalizedMessage) {
    return normalizeErrorText(fallback) || t('common.requestFailed');
  }
  if (containsChinese(normalizedMessage)) {
    return normalizedMessage;
  }
  const lowered = normalizedMessage.toLowerCase();
  if (
    includesAny(lowered, [
      'error parsing multipart/form-data request',
      'invalid boundary',
      'multipart',
      'form-data'
    ])
  ) {
    return '上传请求格式错误，请刷新页面后重试。';
  }
  if (
    includesAny(lowered, [
      'payload too large',
      'request body too large',
      'body too large',
      'file too large',
      'entity too large',
      'content too large'
    ])
  ) {
    return '上传内容过大，请压缩后重试。';
  }
  if (
    includesAny(lowered, [
      'network error',
      'failed to fetch',
      'load failed',
      'network request failed',
      'econnrefused',
      'socket hang up',
      'network'
    ])
  ) {
    return '网络连接失败，请检查服务是否可用后重试。';
  }
  if (
    includesAny(lowered, ['timeout', 'timed out', 'econnaborted', 'deadline has elapsed'])
  ) {
    return '请求超时，请稍后重试。';
  }
  if (
    includesAny(lowered, [
      'unauthorized',
      'forbidden',
      'auth required',
      'authentication failed',
      'invalid credentials',
      'permission denied',
      'access denied'
    ])
  ) {
    return status === 401 ? '登录状态已失效，请重新登录。' : '没有权限执行此操作。';
  }
  if (
    includesAny(lowered, [
      'not found',
      'file not found',
      'skill not found',
      'resource not found',
      '404'
    ])
  ) {
    return '目标内容不存在或已被删除。';
  }
  if (
    includesAny(lowered, [
      'bad request',
      'invalid request',
      'invalid parameter',
      'invalid payload',
      'validation failed',
      'missing parameter',
      'required'
    ])
  ) {
    return normalizeErrorText(fallback) || '请求参数不正确，请检查后重试。';
  }
  if (status !== null && status >= 500) {
    return '服务暂时异常，请稍后重试。';
  }
  return normalizeErrorText(fallback) || '操作失败，请稍后重试。';
};

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string') {
      const normalized = normalizeErrorText(value);
      if (normalized) {
        return normalized;
      }
    }
  }
  return '';
};

const normalizeDetailMessage = (detail: unknown): string => {
  if (!detail) return '';
  if (typeof detail === 'string') return detail;
  const payload = asRecord(detail);
  if (typeof payload.message === 'string' && payload.message.trim()) return payload.message.trim();
  if (typeof payload.error === 'string' && payload.error.trim()) return payload.error.trim();
  const nestedDetail = payload.detail;
  if (typeof nestedDetail === 'string' && nestedDetail.trim()) return nestedDetail.trim();
  const nestedPayload = asRecord(nestedDetail);
  if (typeof nestedPayload.message === 'string' && nestedPayload.message.trim()) {
    return nestedPayload.message.trim();
  }
  return '';
};

const parseErrorPayload = (payload: unknown) => {
  const source = asRecord(payload);
  if (!Object.keys(source).length) {
    return {
      message: '',
      code: '',
      traceId: '',
      status: null,
      hint: ''
    };
  }
  const error = asRecord(source.error);
  const detail = source.detail;
  const detailPayload = asRecord(detail);
  const statusNumber = Number(error.status);
  return {
    message: pickString(
      error.message,
      normalizeDetailMessage(detail),
      source.message,
      source.error_message,
      source.error
    ),
    code: pickString(error.code, detailPayload.code, source.code),
    traceId: pickString(error.trace_id, detailPayload.trace_id, source.trace_id),
    status: Number.isFinite(statusNumber) ? statusNumber : null,
    hint: pickString(error.hint, detailPayload.hint, source.hint)
  };
};

export const resolveApiError = (source: unknown, fallback = ''): ResolvedApiError => {
  const sourceRecord = asRecord(source);
  const response = asRecord(sourceRecord.response);
  const payload = response.data;
  const parsed = parseErrorPayload(payload);
  const traceId = pickString(parsed.traceId, readHeader(response.headers as HeaderBag, HEADER_TRACE_ID));
  const rawStatus = Number(response.status);
  const status = Number.isFinite(rawStatus) ? rawStatus : null;
  const rawMessage = pickString(parsed.message, sourceRecord.message);
  const message = localizeApiErrorText(rawMessage, parsed.status || status, fallback);
  return {
    message,
    code: parsed.code,
    traceId,
    status: parsed.status || status,
    hint: parsed.hint
  };
};

export const showApiError = (
  source: unknown,
  fallback = '',
  options: NotificationOptions = {}
): void => {
  const resolved = resolveApiError(source, fallback);
  if (!resolved.message) {
    return;
  }
  ElMessage({
    type: 'error',
    duration: Number.isFinite(options.duration) ? options.duration : 3200,
    showClose: true,
    message: resolved.message
  });
};
