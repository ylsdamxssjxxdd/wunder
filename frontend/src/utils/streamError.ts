const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value.trim()) {
      return value.trim();
    }
  }
  return '';
};

const parseStatus = (value: unknown): number | null => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
};

const normalizeDetailMessage = (detail: unknown): string => {
  if (!detail) return '';
  if (typeof detail === 'string' && detail.trim()) {
    return detail.trim();
  }
  const record = asRecord(detail);
  const nested = asRecord(record.detail);
  return pickString(record.message, record.error, nested.message, nested.error);
};

const formatInputTextDetail = (detail: unknown): string => {
  const record = asRecord(detail);
  const field = String(record.field || '').trim();
  if (field !== 'input_text') {
    return '';
  }
  const actualChars = parseStatus(record.actual_chars);
  const maxChars = parseStatus(record.max_chars);
  if (actualChars !== null && maxChars !== null) {
    return `text input ${actualChars}/${maxChars} chars`;
  }
  if (actualChars !== null) {
    return `text input ${actualChars} chars`;
  }
  if (maxChars !== null) {
    return `text input limit ${maxChars} chars`;
  }
  return '';
};

const formatDetailSummary = (detail: unknown): string =>
  pickString(formatInputTextDetail(detail), normalizeDetailMessage(detail));

export type StructuredErrorMeta = {
  message: string;
  code: string;
  status: number | null;
  hint: string;
  traceId: string;
  detail: unknown;
};

export const parseStructuredErrorPayload = (payload: unknown): StructuredErrorMeta => {
  const root = asRecord(payload);
  const error = asRecord(root.error);
  const detail = root.detail ?? error.detail ?? null;
  const detailSummary = formatDetailSummary(detail);
  const message = pickString(
    error.message,
    root.message,
    normalizeDetailMessage(detail),
    root.error_message,
    typeof root.error === 'string' ? root.error : ''
  );
  const combinedMessage =
    detailSummary && message && detailSummary !== message && !message.includes(detailSummary)
      ? `${message} (${detailSummary})`
      : message || detailSummary;
  return {
    message: combinedMessage,
    code: pickString(error.code, root.code, asRecord(detail).code),
    status: parseStatus(error.status ?? root.status),
    hint: pickString(error.hint, root.hint, asRecord(detail).hint),
    traceId: pickString(error.trace_id, root.trace_id, asRecord(detail).trace_id),
    detail
  };
};

export const formatStructuredErrorMessage = (payload: unknown, fallback = ''): string =>
  pickString(parseStructuredErrorPayload(payload).message, fallback);

export const formatStructuredErrorText = (text: string, fallback = ''): string => {
  if (!text) {
    return fallback;
  }
  try {
    const payload = JSON.parse(text);
    return formatStructuredErrorMessage(payload, fallback || text);
  } catch {
    return text || fallback;
  }
};
