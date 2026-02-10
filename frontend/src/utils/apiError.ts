import { h } from 'vue';
import { ElMessage, ElNotification } from 'element-plus';

import { t } from '@/i18n';

const HEADER_TRACE_ID = 'x-trace-id';

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
  title?: string;
  copyLabel?: string;
  duration?: number;
};

const asRecord = (value: unknown): UnknownRecord =>
  value && typeof value === 'object' ? (value as UnknownRecord) : {};

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

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
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

const copyWithExecCommand = (text: string): void => {
  const textarea = document.createElement('textarea');
  textarea.value = text;
  textarea.setAttribute('readonly', 'readonly');
  textarea.style.position = 'fixed';
  textarea.style.opacity = '0';
  textarea.style.left = '-9999px';
  document.body.appendChild(textarea);
  textarea.select();
  const ok = document.execCommand('copy');
  document.body.removeChild(textarea);
  if (!ok) {
    throw new Error('copy failed');
  }
};

const copyTraceId = async (traceId: string): Promise<void> => {
  if (!traceId) return;
  if (navigator?.clipboard?.writeText) {
    await navigator.clipboard.writeText(traceId);
    return;
  }
  copyWithExecCommand(traceId);
};

export const resolveApiError = (source: unknown, fallback = ''): ResolvedApiError => {
  const sourceRecord = asRecord(source);
  const response = asRecord(sourceRecord.response);
  const payload = response.data;
  const parsed = parseErrorPayload(payload);
  const traceId = pickString(parsed.traceId, readHeader(response.headers as HeaderBag, HEADER_TRACE_ID));
  const rawStatus = Number(response.status);
  const status = Number.isFinite(rawStatus) ? rawStatus : null;
  const message = pickString(parsed.message, sourceRecord.message, fallback, t('common.requestFailed'));
  return {
    message,
    code: parsed.code,
    traceId,
    status: parsed.status || status,
    hint: parsed.hint
  };
};

const buildNotificationMessage = (resolved: ResolvedApiError, options: NotificationOptions = {}) => {
  const copyLabel = options.copyLabel || t('common.copy');
  return h('div', { style: 'display:flex;flex-direction:column;gap:8px;line-height:1.5;' }, [
    h('div', resolved.message),
    h('div', { style: 'display:flex;align-items:center;gap:8px;flex-wrap:wrap;font-size:12px;opacity:.92;' }, [
      h('span', `${t('common.traceId')}: ${resolved.traceId}`),
      h(
        'button',
        {
          type: 'button',
          style:
            'border:1px solid var(--el-color-primary,#409eff);background:transparent;color:var(--el-color-primary,#409eff);border-radius:4px;padding:2px 8px;cursor:pointer;font-size:12px;',
          onClick: async () => {
            try {
              await copyTraceId(resolved.traceId);
              ElMessage.success(t('common.traceIdCopied'));
            } catch {
              ElMessage.error(t('common.traceIdCopyFailed'));
            }
          }
        },
        copyLabel
      )
    ]),
    resolved.hint ? h('div', { style: 'font-size:12px;opacity:.9;' }, resolved.hint) : null
  ]);
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
  if (!resolved.traceId) {
    ElMessage.error(resolved.message);
    return;
  }
  ElNotification({
    title: options.title || t('common.requestFailed'),
    type: 'error',
    duration: Number.isFinite(options.duration) ? options.duration : 8000,
    message: buildNotificationMessage(resolved, options)
  });
};
