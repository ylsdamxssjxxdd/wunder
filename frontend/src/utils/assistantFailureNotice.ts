import { formatStructuredErrorText } from '@/utils/streamError';

type Translator = (key: string, named?: Record<string, unknown>) => string;
type UnknownRecord = Record<string, unknown>;

type WorkflowTerminalItem = {
  item: UnknownRecord;
  status: string;
};

export type AssistantFailureNotice = {
  detail: string;
};

const FAILURE_STATUSES = new Set([
  'failed',
  'error',
  'timeout',
  'aborted',
  'terminated',
  'cancelled',
  'canceled'
]);

const SUCCESS_STATUSES = new Set(['completed', 'complete', 'done', 'finished', 'success', 'succeeded']);
const PENDING_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);

const normalizeText = (value: unknown): string =>
  String(value ?? '')
    .replace(/\r?\n+/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();

const truncateText = (value: string, max = 220): string => {
  if (value.length <= max) return value;
  return `${value.slice(0, Math.max(0, max - 1)).trimEnd()}…`;
};

const normalizeStatus = (value: unknown): string => String(value ?? '').trim().toLowerCase();

const isAssistantStreaming = (message: UnknownRecord): boolean =>
  Boolean(message?.stream_incomplete) || Boolean(message?.workflowStreaming) || Boolean(message?.reasoningStreaming);

const resolveLatestTerminalWorkflowItem = (message: UnknownRecord): WorkflowTerminalItem | null => {
  const items = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (!item || typeof item !== 'object') continue;
    const status = normalizeStatus((item as UnknownRecord).status);
    // Walk backwards and stop at the last non-streaming workflow state.
    if (!status || PENDING_STATUSES.has(status)) continue;
    return {
      item: item as UnknownRecord,
      status
    };
  }
  return null;
};

const isAbortLikeFailure = (item: UnknownRecord, t: Translator): boolean => {
  const rawDetail = normalizeText(item.detail ?? item.error ?? item.message);
  const detail = normalizeText(formatStructuredErrorText(rawDetail, rawDetail));
  const title = normalizeText(item.title);
  const combined = `${title} ${detail}`.toLowerCase();
  const localizedTokens = [t('chat.workflow.aborted'), t('chat.workflow.abortedDetail')]
    .map((value) => normalizeText(value).toLowerCase())
    .filter(Boolean);
  if (localizedTokens.some((token) => combined.includes(token))) {
    return true;
  }
  return ['aborted', 'cancelled', 'canceled', 'stopped by user'].some((token) => combined.includes(token));
};

const resolveFailureDetail = (item: UnknownRecord, t: Translator): string => {
  const rawDetail = normalizeText(item.detail ?? item.error ?? item.message);
  const parsedDetail = normalizeText(formatStructuredErrorText(rawDetail, rawDetail));
  const fallbackTitle = normalizeText(item.title);
  const genericTitles = new Set([
    normalizeText(t('chat.workflow.requestFailed')),
    normalizeText(t('chat.workflow.error'))
  ]);
  const detail = parsedDetail || rawDetail || fallbackTitle;
  if (!detail || genericTitles.has(detail)) {
    return t('chat.workflow.requestFailedDetail');
  }
  return truncateText(detail);
};

export const resolveAssistantFailureNotice = (
  message: Record<string, unknown>,
  t: Translator
): AssistantFailureNotice | null => {
  if (String(message?.role || '') !== 'assistant') return null;
  if (isAssistantStreaming(message)) return null;
  const terminal = resolveLatestTerminalWorkflowItem(message);
  if (!terminal) return null;
  if (SUCCESS_STATUSES.has(terminal.status)) return null;
  if (!FAILURE_STATUSES.has(terminal.status)) return null;
  // User-triggered aborts already have explicit interaction feedback, so keep the bubble clean.
  if (isAbortLikeFailure(terminal.item, t)) return null;
  return {
    detail: resolveFailureDetail(terminal.item, t)
  };
};

export const buildAssistantDisplayContent = (
  message: Record<string, unknown>,
  t: Translator
): string => {
  const baseContent = typeof message?.content === 'string' ? message.content : String(message?.content ?? '');
  const notice = resolveAssistantFailureNotice(message, t);
  if (!notice) {
    return baseContent;
  }
  const prefix = [
    `**⚠️ ${t('chat.message.failedInlineTitle')}**`,
    '',
    t('chat.message.failedInlineReason', { detail: notice.detail })
  ];
  if (normalizeText(baseContent)) {
    prefix.push('', t('chat.message.failedInlinePartial'), '', '---', '', baseContent);
  }
  return prefix.join('\n');
};
