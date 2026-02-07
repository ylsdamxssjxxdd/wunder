import { h } from 'vue';
import { ElMessage, ElNotification } from 'element-plus';

import { t } from '@/i18n';

const HEADER_TRACE_ID = 'x-trace-id';

const readHeader = (headers, key) => {
  if (!headers) return '';
  if (typeof headers.get === 'function') {
    return String(headers.get(key) || '').trim();
  }
  const lowered = key.toLowerCase();
  for (const [name, value] of Object.entries(headers)) {
    if (String(name).toLowerCase() === lowered) {
      return String(value || '').trim();
    }
  }
  return '';
};

const pickString = (...values) => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
};

const normalizeDetailMessage = (detail) => {
  if (!detail) return '';
  if (typeof detail === 'string') return detail;
  if (typeof detail.message === 'string' && detail.message.trim()) return detail.message.trim();
  if (typeof detail.error === 'string' && detail.error.trim()) return detail.error.trim();
  if (detail.detail) {
    if (typeof detail.detail === 'string' && detail.detail.trim()) return detail.detail.trim();
    if (typeof detail.detail.message === 'string' && detail.detail.message.trim()) {
      return detail.detail.message.trim();
    }
  }
  return '';
};

const parseErrorPayload = (payload) => {
  if (!payload || typeof payload !== 'object') {
    return {
      message: '',
      code: '',
      traceId: '',
      status: null,
      hint: ''
    };
  }
  const error = payload.error && typeof payload.error === 'object' ? payload.error : {};
  const detail = payload.detail;
  return {
    message: pickString(
      error.message,
      normalizeDetailMessage(detail),
      payload.message,
      payload.error_message,
      payload.error
    ),
    code: pickString(error.code, detail?.code, payload.code),
    traceId: pickString(error.trace_id, detail?.trace_id, payload.trace_id),
    status: Number.isFinite(Number(error.status)) ? Number(error.status) : null,
    hint: pickString(error.hint, detail?.hint, payload.hint)
  };
};

const copyWithExecCommand = (text) => {
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

const copyTraceId = async (traceId) => {
  if (!traceId) return;
  if (navigator?.clipboard?.writeText) {
    await navigator.clipboard.writeText(traceId);
    return;
  }
  copyWithExecCommand(traceId);
};

export const resolveApiError = (source, fallback = '') => {
  const response = source?.response;
  const payload = response?.data;
  const parsed = parseErrorPayload(payload);
  const traceId = pickString(parsed.traceId, readHeader(response?.headers, HEADER_TRACE_ID));
  const message = pickString(parsed.message, source?.message, fallback, t('common.requestFailed'));
  return {
    message,
    code: parsed.code,
    traceId,
    status: parsed.status || (response?.status ? Number(response.status) : null),
    hint: parsed.hint
  };
};

const buildNotificationMessage = (resolved, options = {}) => {
  const copyLabel = options.copyLabel || t('common.copy');
  return h('div', { style: 'display:flex;flex-direction:column;gap:8px;line-height:1.5;' }, [
    h('div', resolved.message),
    h('div', { style: 'display:flex;align-items:center;gap:8px;flex-wrap:wrap;font-size:12px;opacity:.92;' }, [
      h('span', t('common.traceId') + ': ' + resolved.traceId),
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
            } catch (error) {
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

export const showApiError = (source, fallback = '', options = {}) => {
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
